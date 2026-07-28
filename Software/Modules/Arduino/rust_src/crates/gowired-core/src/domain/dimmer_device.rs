//! Covers `DIMMER`, `RGB` and `RGBW`.
//!
//! The three used to be three classes (`ActiveDimmer` / `RgbDimmer` /
//! `RgbwDimmer`) that differed only in channel count, the `S_*` class they
//! present as, and whether they advertise `V_RGB` or `V_RGBW`. That is data, not
//! behaviour, so they are one type parameterised by [`ColorModel`].

use crate::domain::config::{channel_count, ids, ButtonTiming, ColorModel, DimmerTuning};
use crate::domain::device::{Device, SafetyState};
use crate::domain::dimmer::{Dimmer, MAX_CHANNELS};
use crate::domain::input::{Button, ButtonEvent};
use crate::gw_text;
use crate::hal::{
    Bus, InboundMessage, Milliamps, Pin, Platform, SensorClass, SensorId, ValueType, NO_PIN,
};
use crate::text::Text;

const DIMMER_ID: SensorId = 0;

const fn sensor_class_for(model: ColorModel) -> SensorClass {
    match model {
        ColorModel::Rgb => SensorClass::RgbLight,
        ColorModel::Rgbw => SensorClass::RgbwLight,
        ColorModel::White => SensorClass::Dimmer,
    }
}

fn name_for(model: ColorModel) -> Text {
    match model {
        ColorModel::Rgb => gw_text!("RGB"),
        ColorModel::Rgbw => gw_text!("RGBW"),
        ColorModel::White => gw_text!("Dimmer"),
    }
}

/// How a dimmer board is wired.
#[derive(Copy, Clone, Debug)]
pub struct DimmerSpec {
    /// Colour model, which fixes the channel count.
    pub model: ColorModel,
    /// PWM pins, in R, G, B, W order for the colour models.
    pub led_pins: [Pin; MAX_CHANNELS],
    /// Wall-switch pins: 0 switches, 1 steps brightness.
    pub button_pins: [Pin; 2],
}

impl Default for DimmerSpec {
    fn default() -> Self {
        Self {
            model: ColorModel::White,
            led_pins: [NO_PIN; MAX_CHANNELS],
            button_pins: [NO_PIN; 2],
        }
    }
}

/// One dimmable strip, white or colour.
pub struct DimmerDevice<'a, P: Platform> {
    dimmer: Dimmer<'a, P>,
    buttons: [Button<'a, P>; 2],
    spec: DimmerSpec,
    tuning: DimmerTuning,
    special_button_enabled: bool,
}

impl<'a, P: Platform> DimmerDevice<'a, P> {
    /// Wires a strip to its PWM pins and buttons.
    pub fn new(
        pwm: &'a P::Pwm,
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        spec: DimmerSpec,
        tuning: DimmerTuning,
        button_timing: ButtonTiming,
        special_button_enabled: bool,
    ) -> Self {
        Self {
            dimmer: Dimmer::new(
                pwm,
                clock,
                spec.led_pins,
                channel_count(spec.model),
                tuning,
            ),
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
            spec,
            tuning,
            special_button_enabled,
        }
    }

    /// The brightness model. Test and reporting accessor.
    pub const fn dimmer(&self) -> &Dimmer<'a, P> {
        &self.dimmer
    }

    /// `V_RGB` for a 3-channel strip, `V_RGBW` for 4, nothing for plain white.
    const fn color_value_type(&self) -> Option<ValueType> {
        match self.spec.model {
            ColorModel::Rgb => Some(ValueType::Rgb),
            ColorModel::Rgbw => Some(ValueType::Rgbw),
            ColorModel::White => None,
        }
    }
}

impl<P: Platform> Device<P> for DimmerDevice<'_, P> {
    fn begin(&mut self) {
        self.dimmer.begin();
        self.buttons[0].begin();
        self.buttons[1].begin();
    }

    fn present<B: Bus>(&mut self, bus: &B, presentation_delay_ms: u16) {
        bus.present(
            DIMMER_ID,
            sensor_class_for(self.spec.model),
            name_for(self.spec.model),
        );
        bus.wait(u32::from(presentation_delay_ms));
    }

    fn send_initial_state<B: Bus>(&mut self, bus: &B, echo_timeout_ms: u16) {
        let timeout = u32::from(echo_timeout_ms);

        bus.send_bool(DIMMER_ID, ValueType::Status, false);
        bus.request(DIMMER_ID, ValueType::Status);
        bus.wait_for_set(timeout, ValueType::Status);

        bus.send_uint(
            DIMMER_ID,
            ValueType::Percentage,
            u32::from(self.dimmer.target_level()),
        );
        bus.request(DIMMER_ID, ValueType::Percentage);
        bus.wait_for_set(timeout, ValueType::Percentage);

        if let Some(color_type) = self.color_value_type() {
            bus.send_literal(
                DIMMER_ID,
                color_type,
                if color_type == ValueType::Rgb {
                    gw_text!("ffffff")
                } else {
                    gw_text!("ffffffff")
                },
            );
            bus.request(DIMMER_ID, color_type);
            bus.wait_for_set(timeout, color_type);
        }
    }

    fn handle<B: Bus>(&mut self, msg: &InboundMessage<'_>, _bus: &B, _safety: &SafetyState) -> bool {
        if msg.sensor != DIMMER_ID {
            return false;
        }

        match msg.ty {
            ValueType::Status => {
                self.dimmer.set_on(msg.boolean);
                true
            }
            ValueType::Percentage => {
                self.dimmer
                    .set_target_level(msg.numeric.clamp(0, 100).unsigned_abs() as u8);
                true
            }
            ValueType::Rgb | ValueType::Rgbw => {
                self.dimmer.set_colors_from_hex(msg.text);
                true
            }
            _ => false,
        }
    }

    fn poll_buttons<B: Bus>(&mut self, bus: &B, safety: &SafetyState) {
        // Button 0 switches the strip; button 1 steps the brightness.
        match self.buttons[0].poll() {
            ButtonEvent::Toggle => {
                if !safety.blocks(0) {
                    let next = !self.dimmer.is_on();
                    self.dimmer.set_on(next);
                    bus.send_bool(DIMMER_ID, ValueType::Status, self.dimmer.is_on());
                }
            }
            ButtonEvent::LongPress => {
                if self.special_button_enabled {
                    bus.send_bool(ids::SPECIAL_BUTTON_1, ValueType::Status, true);
                }
            }
            ButtonEvent::None => {}
        }

        match self.buttons[1].poll() {
            ButtonEvent::Toggle => {
                // Stepping the brightness of a strip that is off would be
                // invisible.
                if self.dimmer.is_on() && !safety.blocks(0) {
                    self.dimmer.bump_level(self.tuning.toggle_step);
                    bus.send_uint(
                        DIMMER_ID,
                        ValueType::Percentage,
                        u32::from(self.dimmer.target_level()),
                    );
                }
            }
            ButtonEvent::LongPress => {
                if self.special_button_enabled {
                    bus.send_bool(ids::SPECIAL_BUTTON_2, ValueType::Status, true);
                }
            }
            ButtonEvent::None => {}
        }
    }

    fn tick<B: Bus>(&mut self, _bus: &B, _current: Milliamps) {
        self.dimmer.update();
    }

    fn shed_load<B: Bus>(&mut self, bus: &B, _safety: &SafetyState) {
        // A single LED channel: any fault sheds it.
        if !self.dimmer.is_on() {
            return;
        }
        self.dimmer.set_on(false);
        bus.send_bool(DIMMER_ID, ValueType::Status, self.dimmer.is_on());
    }

    fn power_channel_count(&self) -> u8 {
        1
    }

    fn draws_current(&self, _channel: u8) -> bool {
        self.dimmer.is_on()
    }

    fn uses_dc_measurement(&self) -> bool {
        true
    }
}
