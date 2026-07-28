//! Covers both `DOUBLE_RELAY` and `FOUR_RELAY`.
//!
//! The two used to be separate `#ifdef` blocks repeated across `setup()`,
//! `presentation()`, `InitConfirmation()`, `receive()`, `UpdateIO()` and
//! `loop()`. They differ only in relay count, whether wall switches are wired,
//! and whether there is one shared current sensor or one per relay -- so they
//! are one type with three parameters.

use crate::domain::config::{ids, ButtonTiming};
use crate::domain::device::{Device, SafetyState};
use crate::domain::input::{Button, ButtonEvent};
use crate::domain::relay::Relay;
use crate::gw_text;
use crate::hal::{Bus, InboundMessage, Milliamps, Pin, Platform, SensorClass, ValueType, NO_PIN};
use crate::text::Text;

/// Maximum relays one board carries.
pub const MAX_RELAYS: usize = 4;

fn relay_name(index: usize) -> Text {
    match index {
        0 => gw_text!("Relay 1"),
        1 => gw_text!("Relay 2"),
        2 => gw_text!("Relay 3"),
        _ => gw_text!("Relay 4"),
    }
}

/// How a relay board is wired.
#[derive(Copy, Clone, Debug)]
pub struct RelayBankSpec {
    /// How many relays are fitted.
    pub relay_count: u8,
    /// Their pins, in child-id order.
    pub relay_pins: [Pin; MAX_RELAYS],

    /// Wall switches, paired one-to-one with the first `button_count` relays.
    ///
    /// `FourRelay` has none, which is also why its relays must not be polled as
    /// inputs -- the old `UpdateIO()` ran `CheckInput()` over them and debounced
    /// an uninitialised pin.
    pub button_count: u8,
    /// Their pins.
    pub button_pins: [Pin; 2],

    /// Level that de-energises a relay.
    pub off_level: bool,
    /// `FourRelay` has an ACS712 per output; the others share one sensor.
    pub per_relay_power: bool,
}

impl Default for RelayBankSpec {
    fn default() -> Self {
        Self {
            relay_count: 0,
            relay_pins: [NO_PIN; MAX_RELAYS],
            button_count: 0,
            button_pins: [NO_PIN; 2],
            off_level: false,
            per_relay_power: false,
        }
    }
}

/// Two or four independently switched relays.
pub struct RelayBankDevice<'a, P: Platform> {
    spec: RelayBankSpec,
    special_button_enabled: bool,

    // Fixed-capacity storage: no allocator on this part, and only the first
    // `spec.relay_count` entries are live.
    relays: [Relay<'a, P>; MAX_RELAYS],
    buttons: [Button<'a, P>; 2],
    button_count: usize,
}

impl<'a, P: Platform> RelayBankDevice<'a, P> {
    /// Wires a relay bank to its pins.
    pub fn new(
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        spec: RelayBankSpec,
        button_timing: ButtonTiming,
        special_button_enabled: bool,
    ) -> Self {
        Self {
            spec,
            special_button_enabled,
            relays: core::array::from_fn(|i| Relay::new(gpio, spec.relay_pins[i], spec.off_level)),
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
            button_count: spec.button_count.min(2) as usize,
        }
    }

    fn relay_count(&self) -> usize {
        (self.spec.relay_count as usize).min(MAX_RELAYS)
    }

    /// Which safety channel governs relay `index`.
    ///
    /// With one shared sensor every relay is governed by channel 0, because that
    /// is the only channel a fault can ever latch on. With per-relay sensing each
    /// relay has its own.
    ///
    /// Getting this wrong is not academic: the C++ version used the raw relay
    /// index here while `shed_load` used this mapping, so on a 2SSR board an
    /// overcurrent switched relay 2 off and then let the controller switch it
    /// straight back on into the overload -- flapping once per loop pass for as
    /// long as the controller kept asking.
    fn fault_channel(&self, index: usize) -> u8 {
        if self.spec.per_relay_power {
            index as u8
        } else {
            0
        }
    }

    /// Whether relay `index` is energised. Test and reporting accessor.
    pub fn relay_on(&self, index: usize) -> bool {
        index < self.relay_count() && self.relays[index].is_on()
    }
}

impl<P: Platform> Device<P> for RelayBankDevice<'_, P> {
    fn begin(&mut self) {
        for i in 0..self.relay_count() {
            self.relays[i].begin();
        }
        for i in 0..self.button_count {
            self.buttons[i].begin();
        }
    }

    fn present<B: Bus>(&mut self, bus: &B, presentation_delay_ms: u16) {
        for i in 0..self.relay_count() {
            bus.present(i as u8, SensorClass::Binary, relay_name(i));
            bus.wait(u32::from(presentation_delay_ms));
        }
    }

    fn send_initial_state<B: Bus>(&mut self, bus: &B, echo_timeout_ms: u16) {
        for i in 0..self.relay_count() {
            bus.send_bool(i as u8, ValueType::Status, self.relays[i].is_on());
            bus.request(i as u8, ValueType::Status);
            bus.wait_for_set(u32::from(echo_timeout_ms), ValueType::Status);
        }
    }

    fn handle<B: Bus>(&mut self, msg: &InboundMessage<'_>, _bus: &B, safety: &SafetyState) -> bool {
        if msg.ty != ValueType::Status || msg.sensor as usize >= self.relay_count() {
            return false;
        }

        // Addressed to us either way, so the message is consumed even when a
        // fault means we refuse to act on it.
        if !safety.blocks(self.fault_channel(msg.sensor as usize)) {
            self.relays[msg.sensor as usize].set(msg.boolean);
        }
        true
    }

    fn poll_buttons<B: Bus>(&mut self, bus: &B, safety: &SafetyState) {
        for i in 0..self.button_count {
            match self.buttons[i].poll() {
                ButtonEvent::Toggle => {
                    if safety.blocks(self.fault_channel(i)) {
                        continue;
                    }
                    self.relays[i].toggle();
                    bus.send_bool(i as u8, ValueType::Status, self.relays[i].is_on());
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

    fn tick<B: Bus>(&mut self, _bus: &B, _current: Milliamps) {}

    fn shed_load<B: Bus>(&mut self, bus: &B, safety: &SafetyState) {
        for i in 0..self.relay_count() {
            // With one shared sensor every channel is implicated; with per-relay
            // sensing only the channel that actually tripped is.
            if !safety.blocks(self.fault_channel(i)) || !self.relays[i].is_on() {
                continue;
            }
            self.relays[i].set(false);
            bus.send_bool(i as u8, ValueType::Status, false);
        }
    }

    fn power_channel_count(&self) -> u8 {
        if self.spec.per_relay_power {
            4
        } else {
            1
        }
    }

    fn draws_current(&self, channel: u8) -> bool {
        if self.spec.per_relay_power {
            return (channel as usize) < self.relay_count() && self.relays[channel as usize].is_on();
        }
        // One shared sensor: sample whenever anything is energised.
        (0..self.relay_count()).any(|i| self.relays[i].is_on())
    }
}
