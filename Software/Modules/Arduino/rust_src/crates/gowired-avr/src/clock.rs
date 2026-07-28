//! Millisecond time base and blocking delays.
//!
//! Timer 0 does double duty, exactly as the Arduino core has it: it generates
//! PWM on pins 5 and 6 *and* its overflow drives the millisecond counter. That
//! is not a coincidence to be tidied up -- fast-PWM mode does not change the
//! overflow rate, so one timer genuinely can do both, and Timer 2 is left free.
//!
//! # The fractional counter
//!
//! At 8 MHz with a /64 prescaler an overflow happens every
//! `64 * 256 / 8 = 2048` microseconds. That is 2 milliseconds plus 48
//! microseconds, so a counter that simply added 2 would lose 48 us per overflow
//! -- about 2.3%, or 35 minutes a day. The remainder is accumulated in eighths
//! of a millisecond, which is the same arithmetic `wiring.c` uses and the reason
//! [`FRACT_INC`] is 6 rather than 48.

use core::cell::UnsafeCell;

use gowired_core::hal;

use crate::regs::{rd, set_bits, wr, TCCR0A, TCCR0B, TIMSK0};
use crate::{without_interrupts, F_CPU};

/// Microseconds between Timer 0 overflows: prescaler 64, 256 counts.
pub const MICROS_PER_OVERFLOW: u32 = 64 * 256 / (F_CPU / 1_000_000);

/// Whole milliseconds each overflow contributes.
pub const MILLIS_INC: u8 = (MICROS_PER_OVERFLOW / 1000) as u8;

/// Leftover, in eighths of a millisecond.
pub const FRACT_INC: u8 = ((MICROS_PER_OVERFLOW % 1000) >> 3) as u8;

/// One millisecond, in eighths.
pub const FRACT_MAX: u8 = (1000 >> 3) as u8;

/// A `static mut` in a wrapper, because that is what a single-core MCU with one
/// interrupt handler actually needs.
///
/// Access from the main loop goes through [`without_interrupts`]; access from the
/// handler is already atomic, since interrupts are off inside it.
struct Shared<T>(UnsafeCell<T>);

// SAFETY: single core, and every multi-byte access is inside a critical section.
unsafe impl<T> Sync for Shared<T> {}

static MILLIS: Shared<u32> = Shared(UnsafeCell::new(0));
static FRACT: Shared<u8> = Shared(UnsafeCell::new(0));

/// Timer 0 overflow: the millisecond tick.
///
/// # Safety
///
/// Called by the hardware only. Named for the ATmega328P/PB vector table, where
/// `TIMER0_OVF` is vector 16 on both parts -- checked against `iom328p.h` and
/// `iom328pb.h`.
#[no_mangle]
pub unsafe extern "avr-interrupt" fn __vector_16() {
    let millis = MILLIS.0.get();
    let fract = FRACT.0.get();

    let mut m = millis.read_volatile();
    let mut f = fract.read_volatile() + FRACT_INC;

    m += u32::from(MILLIS_INC);
    if f >= FRACT_MAX {
        f -= FRACT_MAX;
        m += 1;
    }

    fract.write_volatile(f);
    millis.write_volatile(m);
}

/// Milliseconds since [`init`].
///
/// The read is inside a critical section because the counter is four bytes wide
/// and the handler can land between any two of them.
pub fn millis() -> u32 {
    without_interrupts(|| {
        // SAFETY: interrupts are off, so the handler cannot be mid-update.
        unsafe { MILLIS.0.get().read_volatile() }
    })
}

/// Starts Timer 0 in fast-PWM mode with the overflow interrupt enabled.
///
/// # Safety
///
/// Call once, before interrupts are enabled.
pub unsafe fn init() {
    // WGM01 | WGM00: fast PWM, 8-bit. The compare outputs stay disconnected
    // until `pwm::write_duty` needs them.
    wr(TCCR0A, (1 << 1) | (1 << 0));
    // CS01 | CS00: prescaler 64.
    wr(TCCR0B, (1 << 1) | (1 << 0));
    // TOIE0: overflow interrupt.
    set_bits(TIMSK0, 1 << 0);
}

/// Blocks for `ms` milliseconds.
///
/// Does not service the transport; see [`gowired_core::hal::Clock::delay_ms`].
pub fn delay_ms(ms: u32) {
    let start = millis();
    while millis().wrapping_sub(start) < ms {
        // The watchdog is deliberately *not* petted here: a delay long enough to
        // trip it is either the `cmd3` self-test or a genuine hang, and both
        // should end in a reset.
        core::hint::spin_loop();
    }
}

/// Blocks for roughly `us` microseconds.
///
/// Only accurate enough for the RS485 driver turnaround, which is its only
/// caller: five microseconds between asserting DE and the first bit. The loop
/// body is four cycles on AVR (`sbiw` plus `brne`), so at 8 MHz two iterations
/// are a microsecond -- close enough for a delay whose purpose is "not zero".
#[inline(never)]
pub fn delay_us(us: u16) {
    let mut iterations = us.saturating_mul(2);
    while iterations > 0 {
        // SAFETY: a `nop` has no preconditions. It is here to stop LLVM
        // recognising the loop as empty and deleting it.
        unsafe {
            core::arch::asm!("nop", options(nostack, preserves_flags));
        }
        iterations -= 1;
    }
}

/// The time base, behind [`hal::Clock`].
pub struct Clock;

impl hal::Clock for Clock {
    fn now_ms(&self) -> u32 {
        millis()
    }

    fn delay_ms(&self, ms: u32) {
        delay_ms(ms);
    }

    fn delay_us(&self, us: u16) {
        delay_us(us);
    }
}

/// Whether Timer 0 is running, for the firmware's startup self-check.
pub fn timer_running() -> bool {
    // SAFETY: reading TCCR0B has no side effects.
    unsafe { rd(TCCR0B) & 0b111 != 0 }
}
