//! The generic digital inputs (`INPUT_1..4`), independent of device kind.
//!
//! Replaces four copy-pasted `#ifdef` blocks in each of `setup()`,
//! `presentation()`, `InitConfirmation()` and `UpdateIO()`. It also removes the
//! constraint that made disabling `INPUT_3` or `INPUT_4` a compile error:
//! `NUMBER_OF_INPUTS` was a sum of possibly-undefined `PULLUP_n` macros used as
//! a C++ array bound, so an undefined macro leaked through as an identifier.

use crate::domain::config::{InputPin, MAX_INPUTS};
use crate::domain::input::DigitalSensor;
use crate::gw_text;
use crate::hal::{Bus, Platform, SensorClass, ValueType};
use crate::text::Text;

fn input_name(index: usize) -> Text {
    match index {
        0 => gw_text!("Input 1"),
        1 => gw_text!("Input 2"),
        2 => gw_text!("Input 3"),
        _ => gw_text!("Input 4"),
    }
}

/// All four input slots, enabled or not.
pub struct InputBank<'a, P: Platform> {
    pins: [InputPin; MAX_INPUTS],
    sensors: [DigitalSensor<'a, P>; MAX_INPUTS],
}

impl<'a, P: Platform> InputBank<'a, P> {
    /// Wires the bank to the configured pins.
    pub fn new(
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        pins: [InputPin; MAX_INPUTS],
        debounce_ms: u8,
    ) -> Self {
        Self {
            pins,
            sensors: core::array::from_fn(|i| {
                DigitalSensor::new(
                    gpio,
                    clock,
                    pins[i].pin,
                    pins[i].invert,
                    pins[i].pullup,
                    debounce_ms,
                )
            }),
        }
    }

    /// How many slots are enabled. Enabled slots need not be contiguous.
    pub fn enabled_count(&self) -> u8 {
        self.pins.iter().filter(|p| p.enabled).count() as u8
    }

    /// Configures the pins of the enabled slots.
    pub fn begin(&self) {
        for (pin, sensor) in self.pins.iter().zip(&self.sensors) {
            if pin.enabled {
                sensor.begin();
            }
        }
    }

    /// Announces the enabled slots.
    pub fn present<B: Bus>(&self, bus: &B, presentation_delay_ms: u16) {
        for (i, pin) in self.pins.iter().enumerate() {
            if !pin.enabled {
                continue;
            }
            bus.present(pin.id, SensorClass::Binary, input_name(i));
            bus.wait(u32::from(presentation_delay_ms));
        }
    }

    /// Publishes the level each enabled slot booted with.
    pub fn send_initial_state<B: Bus>(&self, bus: &B) {
        for (pin, sensor) in self.pins.iter().zip(&self.sensors) {
            if pin.enabled {
                bus.send_bool(pin.id, ValueType::Status, sensor.level());
            }
        }
    }

    /// Reports any input whose level changed.
    pub fn poll<B: Bus>(&mut self, bus: &B) {
        for (pin, sensor) in self.pins.iter().zip(&mut self.sensors) {
            if !pin.enabled {
                continue;
            }
            if let Some(level) = sensor.poll() {
                bus.send_bool(pin.id, ValueType::Status, level);
            }
        }
    }
}
