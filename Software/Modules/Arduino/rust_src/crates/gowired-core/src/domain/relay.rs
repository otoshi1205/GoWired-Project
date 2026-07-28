//! A single on/off output.

use crate::hal::{Gpio, Pin, PinMode, Platform};

/// One relay, remembering its own state so the bank can report it.
pub struct Relay<'a, P: Platform> {
    gpio: &'a P::Gpio,
    pin: Pin,
    off_level: bool,
    on: bool,
}

impl<'a, P: Platform> Relay<'a, P> {
    /// `off_level` is the pin level that de-energises the relay.
    pub const fn new(gpio: &'a P::Gpio, pin: Pin, off_level: bool) -> Self {
        Self {
            gpio,
            pin,
            off_level,
            on: false,
        }
    }

    /// Configures the pin and de-energises the relay.
    pub fn begin(&mut self) {
        self.gpio.configure(self.pin, PinMode::Output);
        self.gpio.write(self.pin, self.off_level);
        self.on = false;
    }

    /// Energises or de-energises the relay.
    pub fn set(&mut self, on: bool) {
        self.gpio.write(self.pin, if on { !self.off_level } else { self.off_level });
        self.on = on;
    }

    /// Flips the relay.
    pub fn toggle(&mut self) {
        self.set(!self.on);
    }

    /// Whether the relay is energised.
    pub const fn is_on(&self) -> bool {
        self.on
    }

    /// The pin it drives.
    pub const fn pin(&self) -> Pin {
        self.pin
    }
}
