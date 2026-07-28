//! Roller-shutter position model and relay driving.
//!
//! A port of GoWired-lib's `Shutters` class. The position arithmetic and the
//! travel-time bookkeeping are pure and tested; the two relays and the EEPROM
//! are reached through HAL traits.
//!
//! Every method that returns a duration returns **milliseconds**. The original
//! returned seconds and left each caller to multiply by 1000 (or by 10, for the
//! percentage case), which is where the magic numbers in the old
//! `roller_shutter.cpp` came from.

use crate::domain::config::StoreLayout;
use crate::hal::{Clock, Gpio, Pin, PinMode, Platform, Store, NO_PIN};

/// Which way the shutter is travelling.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Motion {
    /// Opening.
    Up = 0,
    /// Closing.
    Down = 1,
    /// Not moving.
    Stopped = 2,
}

/// The two relay pins and their idle level.
#[derive(Copy, Clone, Debug)]
pub struct ShutterPins {
    /// Pin that drives the motor open.
    pub up: Pin,
    /// Pin that drives the motor closed.
    pub down: Pin,
    /// Level that de-energises either relay.
    pub off_level: bool,
}

impl Default for ShutterPins {
    fn default() -> Self {
        Self {
            up: NO_PIN,
            down: NO_PIN,
            off_level: false,
        }
    }
}

/// Time the opposite relay needs to drop out before the other is energised.
const DIRECTION_INTERLOCK_MS: u32 = 50;

const EEPROM_BLANK: u8 = 0xFF;

/// Clamps to 0..=100 and narrows in one step.
///
/// Returning `u8` rather than `i32` is what keeps the four call sites free of
/// casts that a reader would otherwise have to prove non-negative.
const fn clamp_percent(value: i32) -> u8 {
    if value > 100 {
        100
    } else if value < 0 {
        0
    } else {
        // Both out-of-range branches are already taken, so 0 <= value <= 100.
        value as u8
    }
}

/// Position model plus the two relays that move it.
pub struct Shutter<'a, P: Platform> {
    gpio: &'a P::Gpio,
    clock: &'a P::Clock,
    store: &'a P::Store,
    pins: ShutterPins,
    layout: StoreLayout,

    up_time_s: u8,
    down_time_s: u8,
    position: u8,
    calibrated: bool,
    motion: Motion,
    pending: Motion,
}

impl<'a, P: Platform> Shutter<'a, P> {
    /// Wires the model to two relay pins and an EEPROM.
    pub const fn new(
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        store: &'a P::Store,
        pins: ShutterPins,
        layout: StoreLayout,
    ) -> Self {
        Self {
            gpio,
            clock,
            store,
            pins,
            layout,
            up_time_s: 0,
            down_time_s: 0,
            position: 0,
            calibrated: false,
            motion: Motion::Stopped,
            pending: Motion::Stopped,
        }
    }

    /// Configures the pins, then reads persisted travel times and position.
    ///
    /// Falls back to the configured defaults when the EEPROM is blank.
    pub fn begin(&mut self, default_up_s: u8, default_down_s: u8) {
        self.gpio.configure(self.pins.up, PinMode::Output);
        self.gpio.write(self.pins.up, self.pins.off_level);
        self.gpio.configure(self.pins.down, PinMode::Output);
        self.gpio.write(self.pins.down, self.pins.off_level);

        let stored_down = self.store.read(self.layout.shutter_down_time);
        let stored_up = self.store.read(self.layout.shutter_up_time);

        if stored_up != EEPROM_BLANK && stored_down != EEPROM_BLANK {
            self.calibrated = true;
            self.up_time_s = stored_up;
            self.down_time_s = stored_down;
            self.position =
                clamp_percent(i32::from(self.store.read(self.layout.shutter_position)));
        } else {
            // Uncalibrated: adopt the manually configured times so the shutter
            // is usable, but stay flagged so a calibration run can still be
            // asked for.
            self.calibrated = false;
            self.up_time_s = default_up_s;
            self.down_time_s = default_down_s;
            self.position = 0;
        }

        self.motion = Motion::Stopped;
        self.pending = Motion::Stopped;
    }

    /// Whether travel times were measured rather than guessed.
    pub const fn calibrated(&self) -> bool {
        self.calibrated
    }

    /// Position in percent closed: 0 fully open, 100 fully closed.
    pub const fn position(&self) -> u8 {
        self.position
    }

    /// Which way the relays are currently driving.
    pub const fn motion(&self) -> Motion {
        self.motion
    }

    /// Seconds for a full open.
    pub const fn up_time_s(&self) -> u8 {
        self.up_time_s
    }

    /// Seconds for a full close.
    pub const fn down_time_s(&self) -> u8 {
        self.down_time_s
    }

    /// Stores new travel times and persists them.
    pub fn set_travel_times(&mut self, up_s: u8, down_s: u8) {
        self.up_time_s = up_s;
        self.down_time_s = down_s;
        self.calibrated = true;
        self.store.write(self.layout.shutter_up_time, up_s);
        self.store.write(self.layout.shutter_down_time, down_s);
    }

    const fn travel_time_s(&self, motion: Motion) -> u8 {
        match motion {
            Motion::Up => self.up_time_s,
            Motion::Down => self.down_time_s,
            Motion::Stopped => 0,
        }
    }

    /// Direct `V_UP` / `V_DOWN` / `V_STOP` command.
    ///
    /// Returns how long the shutter should travel for, in milliseconds.
    pub fn request(&mut self, motion: Motion) -> u32 {
        self.pending = motion;
        if matches!(motion, Motion::Stopped) {
            return 0;
        }
        u32::from(self.travel_time_s(motion)) * 1000
    }

    /// `V_PERCENTAGE` command. 0 is fully open, 100 fully closed.
    ///
    /// Returns how long the shutter should travel for, in milliseconds.
    pub fn request_position(&mut self, percent: i32) -> u32 {
        let target = clamp_percent(percent);
        let range = i32::from(target) - i32::from(self.position);
        if range == 0 {
            self.pending = Motion::Stopped;
            return 0;
        }

        // Positive range means further closed, i.e. downward.
        let direction = if range > 0 { Motion::Down } else { Motion::Up };
        self.pending = direction;

        let magnitude = range.unsigned_abs();
        // travel_time_s is a full 0..100 traverse, so scale by percent/100. The
        // *10 (rather than *1000/100) is the original arithmetic, kept verbatim.
        u32::from(self.travel_time_s(direction)) * magnitude * 10
    }

    /// Wall-switch press. `button` 0 is up, 1 is down.
    ///
    /// Pressing the button for the direction already in progress stops the
    /// shutter. Returns the travel time in milliseconds.
    pub fn request_button(&mut self, button: u8) -> u32 {
        let requested = if button == 0 { Motion::Up } else { Motion::Down };

        // Pressing the direction that is already running means "stop".
        if !matches!(self.motion, Motion::Stopped) && self.motion == requested {
            self.pending = Motion::Stopped;
            return 0;
        }

        self.pending = requested;
        u32::from(self.travel_time_s(requested)) * 1000
    }

    /// The motion the model wants but has not applied to the relays yet.
    pub const fn pending(&self) -> Motion {
        self.pending
    }

    /// Sets the wanted motion without touching the relays.
    pub fn set_pending(&mut self, motion: Motion) {
        self.pending = motion;
    }

    /// Drives the relays to match [`Shutter::pending`], breaking before making.
    pub fn apply(&mut self) {
        match self.pending {
            Motion::Stopped => {
                self.gpio.write(self.pins.up, self.pins.off_level);
                self.gpio.write(self.pins.down, self.pins.off_level);
            }
            Motion::Up => {
                // Break before make: never energise both directions at once.
                if self.gpio.read(self.pins.down) != self.pins.off_level {
                    self.gpio.write(self.pins.down, self.pins.off_level);
                    self.clock.delay_ms(DIRECTION_INTERLOCK_MS);
                }
                self.gpio.write(self.pins.up, !self.pins.off_level);
            }
            Motion::Down => {
                if self.gpio.read(self.pins.up) != self.pins.off_level {
                    self.gpio.write(self.pins.up, self.pins.off_level);
                    self.clock.delay_ms(DIRECTION_INTERLOCK_MS);
                }
                self.gpio.write(self.pins.down, !self.pins.off_level);
            }
        }

        self.motion = self.pending;
    }

    /// Integrates a completed movement into the stored position.
    pub fn advance(&mut self, direction: Motion, elapsed_ms: u32) {
        let full_travel_s = self.travel_time_s(direction);
        if full_travel_s == 0 {
            return; // not calibrated; nothing sensible to integrate
        }

        // elapsed_ms / (full_travel_s * 1000) * 100 == elapsed_ms / (full_travel_s * 10)
        //
        // Integer division truncates towards zero, which is exactly what the
        // original's `(int)` cast of the float result did.
        let change = (elapsed_ms / (u32::from(full_travel_s) * 10)).min(i32::MAX as u32) as i32;

        let mut next = i32::from(self.position);
        next += if matches!(direction, Motion::Down) {
            change
        } else {
            -change
        };
        self.position = clamp_percent(next);
    }

    /// Overrides the position without moving.
    pub fn set_position(&mut self, percent: u8) {
        self.position = clamp_percent(i32::from(percent));
    }

    /// Writes the position to EEPROM.
    pub fn persist_position(&self) {
        self.store.write(self.layout.shutter_position, self.position);
    }
}
