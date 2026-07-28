//! Watchdog timer, armed for 8 seconds.
//!
//! Every change to `WDTCSR` has to happen within four cycles of setting `WDCE`,
//! and `WDCE` can only be set with `WDE` already on. Interrupts are off for the
//! whole sequence: a timer tick landing between the two writes makes the change
//! silently not happen, which would leave the watchdog either disarmed or armed
//! with the wrong timeout.

use gowired_core::hal;

use crate::regs::{rd, wr, WDTCSR};
use crate::without_interrupts;

/// `WDCE`: change enable.
const WDCE: u8 = 1 << 4;
/// `WDE`: watchdog system reset enable.
const WDE: u8 = 1 << 3;
/// `WDP3`: with WDP0, selects the 8 s timeout.
const WDP3: u8 = 1 << 5;
/// `WDP0`.
const WDP0: u8 = 1 << 0;

/// Resets the countdown.
#[inline(always)]
pub fn reset() {
    // SAFETY: `wdr` has no preconditions.
    unsafe {
        core::arch::asm!("wdr", options(nostack, preserves_flags));
    }
}

/// Arms the watchdog with an 8 second timeout.
pub fn enable() {
    reset();
    without_interrupts(|| {
        // SAFETY: the documented unlock sequence. The second write must follow
        // the first within four cycles, hence the critical section.
        unsafe {
            wr(WDTCSR, WDCE | WDE);
            wr(WDTCSR, WDE | WDP3 | WDP0);
        }
    });
}

/// Disarms the watchdog.
pub fn disable() {
    reset();
    without_interrupts(|| {
        // SAFETY: as `enable`. `WDRF` in MCUSR has to be clear first or the
        // hardware refuses to clear `WDE` -- see `clear_watchdog_reset`.
        unsafe {
            wr(WDTCSR, WDCE | WDE);
            wr(WDTCSR, 0);
        }
    });
}

/// Whether the watchdog is armed, for the firmware's startup self-check.
pub fn armed() -> bool {
    // SAFETY: reading WDTCSR has no side effects.
    unsafe { rd(WDTCSR) & WDE != 0 }
}

/// The watchdog, behind [`hal::Watchdog`].
pub struct Watchdog;

impl hal::Watchdog for Watchdog {
    fn enable(&self) {
        enable();
    }

    fn disable(&self) {
        disable();
    }

    fn pet(&self) {
        reset();
    }
}
