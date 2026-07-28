//! PWM dimmer with white / RGB / RGBW colour models.
//!
//! A port of GoWired-lib's `Dimmer` class. Two defects in the original are fixed
//! here and covered by tests:
//!
//! - Hex colour parsing only accepted upper-case digits. `_StringHexToByte()`
//!   did `c -= 7` for anything above `'9'`, which is right for `'A'..'F'` and off
//!   by 32 for `'a'..'f'`, so `"ff0000"` decoded to garbage. Controllers
//!   generally send lower case -- and the original's own init confirmation
//!   advertised `"ffffff"`. Parsing is case-insensitive here and accepts an
//!   optional `'#'`.
//!
//! - The ramp loops compared `current != target` while stepping by
//!   `DimmerTuning::step`, so any step size that did not exactly divide the
//!   distance overshot and spun forever until the watchdog fired. The final step
//!   is clamped to the remaining distance.

use crate::domain::config::DimmerTuning;
use crate::hal::{Clock, Pin, Platform, Pwm, NO_PIN};

/// Maximum channels a dimmer can drive (R, G, B, W).
pub const MAX_CHANNELS: usize = 4;

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn hex_byte(s: &[u8]) -> Option<u8> {
    let hi = hex_nibble(s[0])?;
    let lo = hex_nibble(s[1])?;
    Some((hi << 4) | lo)
}

/// Brightness and colour ramp over up to four PWM channels.
pub struct Dimmer<'a, P: Platform> {
    pwm: &'a P::Pwm,
    clock: &'a P::Clock,
    pins: [Pin; MAX_CHANNELS],
    channels: usize,
    tuning: DimmerTuning,

    on: bool,
    level: u8,
    target_level: u8,
    values: [u8; MAX_CHANNELS],
    target_values: [u8; MAX_CHANNELS],
}

impl<'a, P: Platform> Dimmer<'a, P> {
    /// Wires a dimmer to `channels` of the given pins.
    pub const fn new(
        pwm: &'a P::Pwm,
        clock: &'a P::Clock,
        pins: [Pin; MAX_CHANNELS],
        channels: u8,
        tuning: DimmerTuning,
    ) -> Self {
        let channels = if channels as usize > MAX_CHANNELS {
            MAX_CHANNELS
        } else {
            channels as usize
        };
        Self {
            pwm,
            clock,
            pins,
            channels,
            tuning,
            on: false,
            level: 20,
            target_level: 20,
            values: [0; MAX_CHANNELS],
            target_values: [255; MAX_CHANNELS],
        }
    }

    /// An unwired dimmer, for a device that has none.
    pub const fn disabled(pwm: &'a P::Pwm, clock: &'a P::Clock, tuning: DimmerTuning) -> Self {
        Self::new(pwm, clock, [NO_PIN; MAX_CHANNELS], 0, tuning)
    }

    /// Drives every channel to zero.
    pub fn begin(&mut self) {
        for i in 0..self.channels {
            self.pwm.write_duty(self.pins[i], 0);
        }
        self.on = false;
    }

    /// Whether the strip is switched on.
    pub const fn is_on(&self) -> bool {
        self.on
    }

    /// Brightness the dimmer is ramping towards, 0..100.
    pub const fn target_level(&self) -> u8 {
        self.target_level
    }

    /// Brightness currently applied to the outputs, 0..100.
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// Current value of one colour channel, 0..255.
    pub fn channel_value(&self, channel: usize) -> u8 {
        if channel < MAX_CHANNELS {
            self.values[channel]
        } else {
            0
        }
    }

    fn write_outputs(&self) {
        for i in 0..self.channels {
            let scaled = u32::from(self.level) * u32::from(self.values[i]) / 100;
            self.pwm
                .write_duty(self.pins[i], if scaled > 255 { 255 } else { scaled as u8 });
        }
    }

    /// Steps `current` towards `target` by at most `step`; true if it moved.
    fn step_towards(current: &mut u8, target: u8, step: u8) -> bool {
        if *current == target {
            return false;
        }
        let effective = if step == 0 { 1 } else { step };
        if *current < target {
            let room = target - *current;
            *current += effective.min(room);
        } else {
            let room = *current - target;
            *current -= effective.min(room);
        }
        true
    }

    /// `V_PERCENTAGE` from the controller. Values above 100 are clamped.
    pub fn set_target_level(&mut self, percent: u8) {
        self.target_level = percent.min(100);
    }

    /// Wall-switch brightness step. Wraps back to `step` once past 100.
    pub fn bump_level(&mut self, step: u8) {
        let next = u16::from(self.target_level) + u16::from(step);
        self.target_level = if next > 100 { step } else { next as u8 };
    }

    /// Turns the dimmer on or off, ramping the brightness.
    pub fn set_on(&mut self, on: bool) {
        if on == self.on {
            return;
        }
        self.on = on;

        // The requested brightness must survive an off/on cycle, so it is
        // restored after the ramp rather than used as the ramp target directly.
        let requested = self.target_level;

        if on {
            self.level = 0;
            self.target_level = requested;
            // Colours are unknown at switch-on; adopt the targets so the ramp
            // has something to scale.
            for i in 0..self.channels {
                if self.values[i] == 0 {
                    self.values[i] = self.target_values[i];
                }
            }
            self.update();
        } else {
            self.target_level = 0;
            self.update();
            self.level = requested; // remember brightness for the next switch-on
        }

        self.target_level = requested;
    }

    /// Parses `"RRGGBB"` or `"RRGGBBWW"`, optionally `'#'`-prefixed,
    /// case-insensitive.
    ///
    /// Returns `false` if the payload was not valid hex, leaving the colours
    /// untouched.
    pub fn set_colors_from_hex(&mut self, hex: &str) -> bool {
        let mut bytes = hex.as_bytes();
        if bytes.first() == Some(&b'#') {
            bytes = &bytes[1..];
        }
        if bytes.len() != 6 && bytes.len() != 8 {
            return false;
        }

        let available = bytes.len() / 2;
        let mut parsed = [0u8; MAX_CHANNELS];
        for i in 0..available.min(MAX_CHANNELS) {
            match hex_byte(&bytes[2 * i..]) {
                Some(b) => parsed[i] = b,
                // Reject atomically; do not half-apply a bad payload.
                None => return false,
            }
        }

        let n = self.channels.min(available);
        self.target_values[..n].copy_from_slice(&parsed[..n]);
        true
    }

    /// Advances the brightness/colour ramp to the target.
    ///
    /// Blocking, as in the original: returns once the outputs have reached the
    /// requested values.
    pub fn update(&mut self) {
        if !self.on {
            return;
        }

        let mut next_step_at = self.clock.now_ms();

        loop {
            let now = self.clock.now_ms();
            if now < next_step_at {
                // millis() rollover: resynchronise rather than stalling for 49
                // days.
                next_step_at = now;
                continue;
            }
            if now - next_step_at < u32::from(self.tuning.interval_ms) {
                continue;
            }

            let mut moved = Self::step_towards(&mut self.level, self.target_level, self.tuning.step);
            for i in 0..self.channels {
                let target = self.target_values[i];
                moved = Self::step_towards(&mut self.values[i], target, self.tuning.step) || moved;
            }

            self.write_outputs();
            next_step_at = self.clock.now_ms();

            if !moved {
                return;
            }
        }
    }
}
