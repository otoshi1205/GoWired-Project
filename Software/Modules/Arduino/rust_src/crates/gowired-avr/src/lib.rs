//! ATmega328P / ATmega328PB register layer for the GoWired module.
//!
//! Everything in [`gowired_core::hal`], implemented against the hardware. This
//! is the one crate that cannot be unit tested, so it is kept as thin as
//! possible: if a function here grows a conditional, it probably belongs in the
//! domain layer.
//!
//! # What it replaces
//!
//! The C++ build reached the hardware through the Arduino core: `pinMode`,
//! `digitalWrite`, `analogRead`, `analogWrite`, `millis`, `EEPROM`, `wdt_*`,
//! `HardwareSerial`, plus GoWired-lib for the ADC sampling loops and three
//! third-party libraries that were compiled in and never called. All of that is
//! about 600 lines here, with no dependencies.
//!
//! It also means the Arduino pin *numbering* is now a table in [`pins`] rather
//! than a variant file, so nothing outside that table cares whether the part is
//! a 328P or a 328PB -- the two differ only in the `-C target-cpu` given to the
//! compiler. The `variant=modelP` / `variant=modelPB` choice the C++ build had
//! to get right is gone.
//!
//! # Startup
//!
//! [`init`] must run before anything else: it configures the millisecond time
//! base, the PWM timers, the ADC prescaler and the USART, then enables
//! interrupts. Nothing else in this crate works until it has.
//!
//! # Honesty about verification
//!
//! None of this has run on a chip, and none of it can be unit tested: it needs
//! the registers it exists to touch. What *is* checked:
//!
//! - Register addresses and interrupt vector numbers, against the toolchain's own
//!   `iom328p.h` and `iom328pb.h` -- see [`regs`].
//! - That the crate compiles and links for both parts.
//! - The `lpm` flash-string path, by disassembly: no presentation string appears
//!   in `.data`.
//!
//! Everything that is arithmetic rather than register poking was deliberately
//! moved out, to [`gowired_core::domain::sensing`], where it is tested. What
//! remains here is sampling loops and register writes -- so a bug here is a wrong
//! address or a wrong bit, not a wrong calculation.
//!
//! None of that is the same as working. [`clock::MILLIS_INC`] and the USART
//! divisor in [`serial`] are derived from [`F_CPU`]; if the fitted crystal is not
//! 8 MHz, both are wrong and nothing else will make sense.

#![no_std]
// Inline asm for tier-3 architectures, and the interrupt-handler calling
// convention. Both are why this crate needs nightly; `avr-none` needs it anyway,
// because a tier-3 target ships no prebuilt `core`.
#![feature(asm_experimental_arch)]
#![feature(abi_avr_interrupt)]
#![allow(clippy::inline_always)]

pub mod adc;
pub mod clock;
pub mod gpio;
pub mod pins;
pub mod probe;
pub mod pwm;
pub mod regs;
pub mod sensors;
pub mod serial;
pub mod store;
pub mod watchdog;

pub use adc::VoltageReference;
pub use clock::{delay_ms, delay_us, millis, Clock};
pub use gpio::Gpio;
pub use pwm::Pwm;
pub use sensors::{CurrentSensor, TemperatureSensor};
pub use serial::Serial;
pub use store::Store;
pub use watchdog::Watchdog;

/// Clock frequency the module's crystal runs at.
///
/// 8 MHz external, per the board documentation and the MiniCore fuse settings
/// the C++ build used. Every delay and baud-rate calculation here depends on it.
pub const F_CPU: u32 = 8_000_000;

/// Runs with interrupts disabled, restoring the previous state afterwards.
///
/// Reading the 32-bit millisecond counter and the EEPROM write sequence both
/// need this. `SREG` bit 7 is the global interrupt enable; saving and restoring
/// the whole register rather than blindly calling `sei` afterwards is what makes
/// this safe to nest and safe to call from an interrupt handler.
#[inline(always)]
pub fn without_interrupts<T>(f: impl FnOnce() -> T) -> T {
    let sreg: u8;
    // SAFETY: reads SREG and clears the interrupt flag. `cli` has no
    // preconditions.
    unsafe {
        core::arch::asm!("in {0}, 0x3F", "cli", out(reg) sreg, options(nostack, preserves_flags));
    }
    let result = f();
    // SAFETY: restores exactly the SREG that was read above.
    unsafe {
        core::arch::asm!("out 0x3F, {0}", in(reg) sreg, options(nostack));
    }
    result
}

/// Enables interrupts.
///
/// # Safety
///
/// Callers must be ready for the timer and USART handlers to run, which means
/// [`clock::init`] and [`serial::init`] must already have happened.
#[inline(always)]
pub unsafe fn enable_interrupts() {
    core::arch::asm!("sei", options(nostack, preserves_flags));
}

/// Brings up the time base, the PWM timers, the ADC and the USART.
///
/// Call once, first. `baud` is the RS485 line rate.
///
/// # Safety
///
/// Must be called exactly once, before any other function in this crate, and
/// before interrupts are enabled.
pub unsafe fn init(baud: u32) {
    clock::init();
    pwm::init();
    adc::init();
    serial::init(baud);
    enable_interrupts();
}

/// Clears a watchdog reset left over from the previous boot.
///
/// The bootloader may leave `WDRF` set with a short timeout still armed, which
/// turns one watchdog reset into an endless series of them. This is the
/// equivalent of the C++ sketch's `before()`.
///
/// # Safety
///
/// Call once, at the very start of `main`, before [`init`].
pub unsafe fn clear_watchdog_reset() {
    watchdog::reset();
    regs::wr(regs::MCUSR, 0);
    watchdog::disable();
}
