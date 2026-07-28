//! Debounced digital inputs: wall-switch buttons and generic sensors.
//!
//! A port of `CommonIO::CheckInput()` / `CommonIO::_ReadDigital()` from
//! GoWired-lib, with the pin access and the clock behind traits so the state
//! machines can be tested. The timing behaviour -- including the fact that
//! polling a button blocks for as long as the button is held -- is preserved
//! deliberately; see [`Button::poll`].

use crate::hal::{Clock, Gpio, Pin, PinMode, Platform};

/// A single debounced level read. Blocks up to 255 ms while the input settles.
pub struct DebouncedInput<'a, P: Platform> {
    gpio: &'a P::Gpio,
    clock: &'a P::Clock,
    pin: Pin,
    invert: bool,
    debounce_ms: u8,
}

/// Hardcoded overall timeout, carried over from `CommonIO::_ReadDigital`.
const SETTLE_TIMEOUT_MS: u32 = 255;

impl<'a, P: Platform> DebouncedInput<'a, P> {
    /// Wires a debounced reader to one pin.
    pub const fn new(
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        pin: Pin,
        invert: bool,
        debounce_ms: u8,
    ) -> Self {
        Self {
            gpio,
            clock,
            pin,
            invert,
            debounce_ms,
        }
    }

    /// The pin this reader is bound to.
    pub const fn pin(&self) -> Pin {
        self.pin
    }

    /// Reads the input.
    ///
    /// Returns `true` once the input has read active continuously for the
    /// debounce period, `false` if it settled inactive or timed out.
    pub fn read(&self) -> bool {
        let mut active;
        let mut previous = false;
        let mut result = false;
        let mut start = self.clock.now_ms();

        loop {
            // An unmodified input idles high (pulled up), so the raw read is
            // inverted unless the caller asked for the opposite polarity.
            active = if self.invert {
                self.gpio.read(self.pin)
            } else {
                !self.gpio.read(self.pin)
            };

            if active && !previous {
                start = self.clock.now_ms();
            }

            if self.clock.now_ms().wrapping_sub(start) > u32::from(self.debounce_ms) && active {
                result = true;
                break;
            }

            // Second test catches a millis() rollover mid-read.
            let now = self.clock.now_ms();
            if now.wrapping_sub(start) > SETTLE_TIMEOUT_MS || now < start {
                break;
            }

            previous = active;
            if !active {
                break;
            }
        }

        result
    }
}

/// What a wall-switch press meant.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ButtonEvent {
    /// Nothing happened.
    None,
    /// Short press: flip whatever this button controls.
    Toggle,
    /// Held past the long-press threshold.
    LongPress,
}

/// A momentary wall switch.
pub struct Button<'a, P: Platform> {
    input: DebouncedInput<'a, P>,
    gpio: &'a P::Gpio,
    clock: &'a P::Clock,
    longpress_ms: u16,
    /// Latches on release. Starts `true` so the first press after boot is
    /// ignored unless the button is already released -- matching `CommonIO`,
    /// which initialised `_HighStateDetected` to true.
    release_seen: bool,
}

impl<'a, P: Platform> Button<'a, P> {
    /// Wires a button to one pin.
    pub const fn new(
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        pin: Pin,
        invert: bool,
        longpress_ms: u16,
        debounce_ms: u8,
    ) -> Self {
        Self {
            input: DebouncedInput::new(gpio, clock, pin, invert, debounce_ms),
            gpio,
            clock,
            longpress_ms,
            release_seen: true,
        }
    }

    /// Configures the pin.
    pub fn begin(&self) {
        self.gpio.configure(self.input.pin(), PinMode::InputPullup);
    }

    /// Samples the button and classifies the press.
    ///
    /// Blocking, exactly as the original: while the button is held this spins
    /// until either the long-press threshold elapses or the button is released.
    /// A press is not accepted until a release has been observed, so holding the
    /// button yields one `LongPress` rather than a stream of `Toggle`s.
    pub fn poll(&mut self) -> ButtonEvent {
        let mut toggled = false;
        let mut first_pass = true;
        let mut event = ButtonEvent::None;
        let mut start = self.clock.now_ms();

        loop {
            let active = self.input.read();

            if first_pass {
                // Require a release before accepting another press. Without
                // this a held button would re-trigger on every main-loop pass.
                if !self.release_seen {
                    self.release_seen = !active;
                    break;
                }
                first_pass = false;
            }

            if !toggled && active {
                event = ButtonEvent::Toggle;
                toggled = true;
                self.release_seen = false;
            }

            if self.clock.now_ms().wrapping_sub(start) > u32::from(self.longpress_ms) {
                event = ButtonEvent::LongPress;
                break;
            }

            if self.clock.now_ms() < start {
                start = self.clock.now_ms(); // millis() rollover
            }

            if !active {
                break;
            }
        }

        event
    }
}

/// A generic on/off input reported to the controller as a binary child:
/// door/window contacts (pulled up) and motion sensors (floating).
pub struct DigitalSensor<'a, P: Platform> {
    input: DebouncedInput<'a, P>,
    gpio: &'a P::Gpio,
    pullup: bool,
    level: bool,
}

impl<'a, P: Platform> DigitalSensor<'a, P> {
    /// Wires a sensor to one pin.
    pub const fn new(
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        pin: Pin,
        invert: bool,
        pullup: bool,
        debounce_ms: u8,
    ) -> Self {
        Self {
            input: DebouncedInput::new(gpio, clock, pin, invert, debounce_ms),
            gpio,
            pullup,
            level: false,
        }
    }

    /// Configures the pin.
    pub fn begin(&self) {
        self.gpio.configure(
            self.input.pin(),
            if self.pullup {
                PinMode::InputPullup
            } else {
                PinMode::Input
            },
        );
    }

    /// Samples the input; returns the new level if it changed since last poll.
    pub fn poll(&mut self) -> Option<bool> {
        let reading = self.input.read();
        if reading == self.level {
            return None;
        }
        self.level = reading;
        Some(reading)
    }

    /// The level as of the last poll.
    pub const fn level(&self) -> bool {
        self.level
    }
}
