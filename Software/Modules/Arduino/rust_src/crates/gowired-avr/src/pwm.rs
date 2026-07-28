//! PWM on the four output pins.
//!
//! The GoWired shields put their outputs on Arduino pins 5, 6, 9 and 10, which is
//! two channels of Timer 0 and two of Timer 1:
//!
//! ```text
//! pin 5   OC0B   Timer 0   (also the millisecond time base)
//! pin 6   OC0A   Timer 0
//! pin 9   OC1A   Timer 1
//! pin 10  OC1B   Timer 1
//! ```
//!
//! Both timers run 8-bit fast PWM with a /64 prescaler, giving 8000000/64/256 =
//! 488 Hz. Fast enough that an LED strip does not flicker, slow enough that the
//! solid-state relays on the 2SSR shield are switching well inside their rating.
//!
//! Duty 0 and duty 255 disconnect the compare output and drive the pin directly,
//! which is what the Arduino core does and is not cosmetic: in fast-PWM mode a
//! compare value of 0 still produces a one-tick pulse every cycle, so a "off" LED
//! strip would glow and an "off" relay would buzz.

use gowired_core::hal::{self, Pin};

use crate::pins::port_of;
use crate::regs::{clear_bits, set_bits, wr, OCR0A, OCR0B, OCR1AH, OCR1AL, OCR1BH, OCR1BL, TCCR0A, TCCR1A, TCCR1B};

/// Starts Timer 1. Timer 0 is started by [`crate::clock::init`], which needs it
/// for the time base.
///
/// # Safety
///
/// Call once, at startup.
pub unsafe fn init() {
    // WGM10 with WGM12: 8-bit fast PWM, so OCR1AH/OCR1BH stay zero and only the
    // low bytes matter.
    wr(TCCR1A, 1 << 0);
    // WGM12 | CS11 | CS10: prescaler 64, matching Timer 0.
    wr(TCCR1B, (1 << 3) | (1 << 1) | (1 << 0));
    wr(OCR1AH, 0);
    wr(OCR1BH, 0);
}

/// Compare-output enable bit and compare register for a pin, if it has one.
const fn channel_of(pin: Pin) -> Option<(u16, u8, u16)> {
    match pin {
        // (control register, COMxx1 bit, compare register)
        5 => Some((TCCR0A, 1 << 5, OCR0B)),  // COM0B1
        6 => Some((TCCR0A, 1 << 7, OCR0A)),  // COM0A1
        9 => Some((TCCR1A, 1 << 7, OCR1AL)), // COM1A1
        10 => Some((TCCR1A, 1 << 5, OCR1BL)), // COM1B1
        _ => None,
    }
}

/// Drives a pin high or low, for the 0 and 255 cases and for pins with no timer.
fn write_level(pin: Pin, high: bool) {
    let Some(port) = port_of(pin) else {
        return;
    };
    // SAFETY: `port` came from the pin table.
    unsafe {
        if high {
            set_bits(port.out, port.mask());
        } else {
            clear_bits(port.out, port.mask());
        }
    }
}

/// The two PWM timers, behind [`hal::Pwm`].
pub struct Pwm;

impl hal::Pwm for Pwm {
    fn write_duty(&self, pin: Pin, duty: u8) {
        let Some(port) = port_of(pin) else {
            return;
        };
        // A PWM pin has to be an output; the dimmer never calls `configure`.
        // SAFETY: `port` came from the pin table.
        unsafe {
            set_bits(port.ddr, port.mask());
        }

        let Some((control, com_bit, compare)) = channel_of(pin) else {
            // No timer channel on this pin: fall back to on/off at the midpoint,
            // so a misconfigured pin map is visibly wrong rather than silently
            // dark.
            write_level(pin, duty >= 128);
            return;
        };

        // SAFETY: `control` and `compare` are timer registers from the table
        // above, and `com_bit` is the matching enable bit.
        unsafe {
            match duty {
                0 => {
                    clear_bits(control, com_bit);
                    write_level(pin, false);
                }
                255 => {
                    clear_bits(control, com_bit);
                    write_level(pin, true);
                }
                _ => {
                    wr(compare, duty);
                    set_bits(control, com_bit);
                }
            }
        }
    }
}
