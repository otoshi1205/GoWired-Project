//! Digital I/O.

use gowired_core::hal::{self, Pin, PinMode};

use crate::pins::port_of;
use crate::regs::{clear_bits, rd, set_bits};

/// The ATmega's three ports, behind [`hal::Gpio`].
pub struct Gpio;

impl hal::Gpio for Gpio {
    fn configure(&self, pin: Pin, mode: PinMode) {
        let Some(port) = port_of(pin) else {
            return; // A6/A7 have no digital hardware
        };
        let mask = port.mask();

        // SAFETY: `port` came from the pin table, so the addresses are real
        // registers and `mask` is a single bit within them.
        unsafe {
            match mode {
                PinMode::Input => {
                    clear_bits(port.ddr, mask);
                    clear_bits(port.out, mask); // pull-up off
                }
                PinMode::InputPullup => {
                    clear_bits(port.ddr, mask);
                    // Direction before pull-up: setting PORT while the pin is
                    // still an output would drive the line for a few cycles.
                    set_bits(port.out, mask);
                }
                PinMode::Output => set_bits(port.ddr, mask),
            }
        }
    }

    fn read(&self, pin: Pin) -> bool {
        let Some(port) = port_of(pin) else {
            return false;
        };
        // SAFETY: reading PINx has no side effects.
        unsafe { rd(port.input) & port.mask() != 0 }
    }

    fn write(&self, pin: Pin, high: bool) {
        let Some(port) = port_of(pin) else {
            return;
        };
        let mask = port.mask();
        // SAFETY: as `configure`.
        //
        // Read-modify-write rather than the single-cycle `sbi`/`cbi`: an
        // interrupt handler that touched the same *port* would lose its write.
        // Nothing in this firmware writes a pin from an interrupt -- the only
        // handlers are the timer tick and USART receive -- so plain bit
        // operations are enough, and they are what the Arduino core did too.
        unsafe {
            if high {
                set_bits(port.out, mask);
            } else {
                clear_bits(port.out, mask);
            }
        }
    }
}
