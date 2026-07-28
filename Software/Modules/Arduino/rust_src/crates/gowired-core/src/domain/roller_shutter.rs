//! `ROLLER_SHUTTER` variant: one cover child driven by two relays.

use crate::domain::config::{ids, ButtonTiming, StoreLayout};
use crate::domain::device::{Device, SafetyState};
use crate::domain::input::{Button, ButtonEvent};
use crate::domain::shutter::{Motion, Shutter, ShutterPins};
use crate::gw_text;
use crate::hal::{
    Bus, Clock, CurrentSensor, InboundMessage, Milliamps, Millivolts, Pin, Platform, SensorClass,
    SensorId, ValueType, Watchdog, NO_PIN,
};

/// Settling time after a direction change, and after telling the controller a
/// movement started, before the next command is acted on.
const MOVEMENT_SETTLE_MS: u32 = 500;

const SHUTTER_ID: SensorId = 0;

/// How a cover board is wired and how far it travels.
#[derive(Copy, Clone, Debug)]
pub struct RollerShutterSpec {
    /// The two relay pins.
    pub pins: ShutterPins,
    /// Wall-switch pins: 0 opens, 1 closes.
    pub button_pins: [Pin; 2],
    /// Current below which the motor is considered to have reached an end stop.
    pub current_floor_ma: Milliamps,
    /// How many travel measurements a calibration run averages.
    pub calibration_samples: u8,
    /// Fallback full-open time when the EEPROM holds no measurement.
    pub default_up_time_s: u8,
    /// Fallback full-close time.
    pub default_down_time_s: u8,
    /// End-stop detection needs a current sensor. Without one the shutter runs
    /// purely on the configured travel times.
    pub current_sensing: bool,
}

impl Default for RollerShutterSpec {
    fn default() -> Self {
        Self {
            pins: ShutterPins::default(),
            button_pins: [NO_PIN; 2],
            current_floor_ma: 200,
            calibration_samples: 1,
            default_up_time_s: 21,
            default_down_time_s: 20,
            current_sensing: false,
        }
    }
}

/// One cover, two relays, two buttons.
pub struct RollerShutterDevice<'a, P: Platform> {
    shutter: Shutter<'a, P>,
    buttons: [Button<'a, P>; 2],
    clock: &'a P::Clock,
    spec: RollerShutterSpec,
    special_button_enabled: bool,

    movement_time_ms: u32,
    started_at_ms: u32,
    /// Direction to resume in after a reversal has braked to a stop.
    resume: Motion,
}

impl<'a, P: Platform> RollerShutterDevice<'a, P> {
    /// Wires a cover to its relays, buttons and EEPROM.
    pub fn new(
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        store: &'a P::Store,
        spec: RollerShutterSpec,
        layout: StoreLayout,
        button_timing: ButtonTiming,
        special_button_enabled: bool,
    ) -> Self {
        Self {
            shutter: Shutter::new(gpio, clock, store, spec.pins, layout),
            buttons: core::array::from_fn(|i| {
                Button::new(
                    gpio,
                    clock,
                    spec.button_pins[i],
                    false,
                    button_timing.longpress_ms,
                    button_timing.debounce_ms,
                )
            }),
            clock,
            spec,
            special_button_enabled,
            movement_time_ms: 0,
            started_at_ms: 0,
            resume: Motion::Stopped,
        }
    }

    /// The position model. Test and reporting accessor.
    pub const fn shutter(&self) -> &Shutter<'a, P> {
        &self.shutter
    }

    fn start_movement<B: Bus>(&mut self, bus: &B) {
        self.shutter.apply();
        self.started_at_ms = self.clock.now_ms();
        bus.send_bool(
            SHUTTER_ID,
            if matches!(self.shutter.motion(), Motion::Up) {
                ValueType::Up
            } else {
                ValueType::Down
            },
            true,
        );
        bus.wait(MOVEMENT_SETTLE_MS);
    }

    fn finish_movement<B: Bus>(&mut self, bus: &B, stopped_at_ms: u32) {
        let direction = self.shutter.motion();

        self.shutter.set_pending(Motion::Stopped);
        self.shutter.apply();
        bus.send_bool(SHUTTER_ID, ValueType::Stop, true);

        // Wrapping subtraction is rollover-correct, which is why the original's
        // explicit 0xFFFFFFFF fix-up branch is gone.
        self.shutter
            .advance(direction, stopped_at_ms.wrapping_sub(self.started_at_ms));
        self.shutter.persist_position();
        bus.send_uint(
            SHUTTER_ID,
            ValueType::Percentage,
            u32::from(self.shutter.position()),
        );

        if !matches!(self.resume, Motion::Stopped) {
            bus.wait(MOVEMENT_SETTLE_MS);
            self.shutter.set_pending(self.resume);
            self.resume = Motion::Stopped;
            self.start_movement(bus);
        }
    }
}

impl<P: Platform> Device<P> for RollerShutterDevice<'_, P> {
    fn begin(&mut self) {
        self.shutter
            .begin(self.spec.default_up_time_s, self.spec.default_down_time_s);
        self.buttons[0].begin();
        self.buttons[1].begin();
    }

    fn present<B: Bus>(&mut self, bus: &B, presentation_delay_ms: u16) {
        bus.present(SHUTTER_ID, SensorClass::Cover, gw_text!("Roller Shutter"));
        bus.wait(u32::from(presentation_delay_ms));
    }

    fn send_initial_state<B: Bus>(&mut self, bus: &B, echo_timeout_ms: u16) {
        let timeout = u32::from(echo_timeout_ms);

        for ty in [ValueType::Up, ValueType::Down, ValueType::Stop] {
            bus.send_bool(SHUTTER_ID, ty, false);
            bus.request(SHUTTER_ID, ty);
            bus.wait_for_set(timeout, ty);
        }

        bus.send_uint(
            SHUTTER_ID,
            ValueType::Percentage,
            u32::from(self.shutter.position()),
        );
        bus.request(SHUTTER_ID, ValueType::Percentage);
        bus.wait_for_set(timeout, ValueType::Percentage);
    }

    fn handle<B: Bus>(&mut self, msg: &InboundMessage<'_>, _bus: &B, _safety: &SafetyState) -> bool {
        if msg.sensor != SHUTTER_ID {
            return false;
        }

        self.movement_time_ms = match msg.ty {
            ValueType::Percentage => self.shutter.request_position(msg.numeric),
            ValueType::Up => self.shutter.request(Motion::Up),
            ValueType::Down => self.shutter.request(Motion::Down),
            ValueType::Stop => self.shutter.request(Motion::Stopped),
            _ => return false,
        };
        true
    }

    fn poll_buttons<B: Bus>(&mut self, bus: &B, safety: &SafetyState) {
        for i in 0..2usize {
            match self.buttons[i].poll() {
                ButtonEvent::Toggle => {
                    if safety.blocks(0) {
                        continue;
                    }
                    self.movement_time_ms = self.shutter.request_button(i as u8);
                }
                ButtonEvent::LongPress => {
                    if self.special_button_enabled {
                        bus.send_bool(ids::SPECIAL_BUTTON_1 + i as u8, ValueType::Status, true);
                    }
                }
                ButtonEvent::None => {}
            }
        }
    }

    fn tick<B: Bus>(&mut self, bus: &B, current: Milliamps) {
        let now = self.clock.now_ms();
        let mut stop_now = false;
        let stopped_at = now;

        if !matches!(self.shutter.motion(), Motion::Stopped) {
            if now.wrapping_sub(self.started_at_ms) >= self.movement_time_ms {
                stop_now = true;
            } else if self.spec.current_sensing && current < self.spec.current_floor_ma {
                // Motor stopped drawing: the shutter reached an end stop early.
                stop_now = true;
            }
        }

        if self.shutter.motion() != self.shutter.pending() {
            if matches!(self.shutter.pending(), Motion::Stopped) {
                stop_now = true;
            } else if matches!(self.shutter.motion(), Motion::Stopped) {
                self.start_movement(bus);
                return;
            } else {
                // Reversal: brake first, then resume the other way.
                self.resume = self.shutter.pending();
                stop_now = true;
            }
        }

        if stop_now {
            self.finish_movement(bus, stopped_at);
        }
    }

    fn shed_load<B: Bus>(&mut self, bus: &B, _safety: &SafetyState) {
        // One motor: any fault stops it.
        if matches!(self.shutter.motion(), Motion::Stopped)
            && matches!(self.shutter.pending(), Motion::Stopped)
        {
            return;
        }
        self.resume = Motion::Stopped;
        self.shutter.set_pending(Motion::Stopped);
        let now = self.clock.now_ms();
        self.finish_movement(bus, now);
    }

    fn power_channel_count(&self) -> u8 {
        1
    }

    fn draws_current(&self, _channel: u8) -> bool {
        !matches!(self.shutter.motion(), Motion::Stopped)
    }

    fn prepare_for_maintenance(&mut self) {
        self.resume = Motion::Stopped;
        self.shutter.set_pending(Motion::Stopped);
        self.shutter.apply();
    }

    fn calibrate<B: Bus>(
        &mut self,
        bus: &B,
        sensor: &P::CurrentSensor,
        watchdog: &P::Watchdog,
        vcc_mv: Millivolts,
    ) -> bool {
        if !self.spec.current_sensing {
            return false; // no way to detect the end stops
        }

        // Drive fully open first, so both directions are measured from a known
        // end.
        self.shutter.set_pending(Motion::Up);
        self.shutter.apply();
        loop {
            self.clock.delay_ms(500);
            watchdog.pet();
            if sensor.measure_ac(vcc_mv) <= self.spec.current_floor_ma {
                break;
            }
        }

        self.shutter.set_pending(Motion::Stopped);
        self.shutter.apply();
        self.clock.delay_ms(1000);

        let mut down_total_s = 0u32;
        let mut up_total_s = 0u32;
        let samples = if self.spec.calibration_samples == 0 {
            1
        } else {
            self.spec.calibration_samples
        };

        for _ in 0..samples {
            // Down first, then up, so each pass ends back at fully open.
            for direction in [Motion::Down, Motion::Up] {
                self.shutter.set_pending(direction);
                self.shutter.apply();
                let start = self.clock.now_ms();
                let mut stop;

                loop {
                    self.clock.delay_ms(250);
                    stop = self.clock.now_ms();
                    watchdog.pet();
                    if sensor.measure_ac(vcc_mv) <= self.spec.current_floor_ma {
                        break;
                    }
                }

                self.shutter.set_pending(Motion::Stopped);
                self.shutter.apply();

                let measured_s = stop.wrapping_sub(start) / 1000;
                if matches!(direction, Motion::Down) {
                    down_total_s += measured_s;
                } else {
                    up_total_s += measured_s;
                }

                self.clock.delay_ms(1000);
            }
        }

        // +1 second of margin, as in the original, so a commanded full traverse
        // definitely reaches the end stop.
        let up_s = (up_total_s / u32::from(samples) + 1) as u8;
        let down_s = (down_total_s / u32::from(samples) + 1) as u8;

        self.shutter.set_travel_times(up_s, down_s);
        self.shutter.set_position(0);
        self.shutter.persist_position();

        bus.send_bool(SHUTTER_ID, ValueType::Stop, true);
        bus.send_uint(
            SHUTTER_ID,
            ValueType::Percentage,
            u32::from(self.shutter.position()),
        );
        true
    }
}
