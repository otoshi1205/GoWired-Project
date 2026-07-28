//! EEPROM.
//!
//! 1 KB on both parts. The MySensors stack owns 0..511 -- including the assigned
//! node id at address 0 -- and this firmware uses 512 upwards for the shutter's
//! travel times and position.

use gowired_core::hal;

use crate::regs::{rd, wr, EEARH, EEARL, EECR, EEDR};
use crate::without_interrupts;

/// `EERE`: read enable.
const EERE: u8 = 1 << 0;
/// `EEPE`: write enable.
const EEPE: u8 = 1 << 1;
/// `EEMPE`: master write enable.
const EEMPE: u8 = 1 << 2;

/// EEPROM size on the 328P and 328PB.
pub const SIZE: u16 = 1024;

fn wait_ready() {
    // SAFETY: reading EECR has no side effects.
    while unsafe { rd(EECR) } & EEPE != 0 {
        core::hint::spin_loop();
    }
}

/// Reads one byte.
pub fn read(address: u16) -> u8 {
    wait_ready();
    // SAFETY: the address register is 10 bits on this part, and `address` is
    // masked to that. Setting EERE latches the data on the next cycle.
    unsafe {
        wr(EEARH, ((address >> 8) & 0x03) as u8);
        wr(EEARL, (address & 0xFF) as u8);
        wr(EECR, EERE);
        rd(EEDR)
    }
}

/// Writes one byte, skipping the write if the value is already there.
///
/// The skip is not an optimisation. A cell is rated for about 100000 erase
/// cycles, the shutter's position is persisted at the end of every movement, and
/// an unnecessary erase also stalls the CPU for 3.4 ms.
pub fn write(address: u16, value: u8) {
    if read(address) == value {
        return;
    }

    wait_ready();
    without_interrupts(|| {
        // SAFETY: the EEPE bit must be set within four cycles of EEMPE, which is
        // why interrupts are off and why these two writes are adjacent. Anything
        // between them -- including a timer tick -- aborts the write silently.
        unsafe {
            wr(EEARH, ((address >> 8) & 0x03) as u8);
            wr(EEARL, (address & 0xFF) as u8);
            wr(EEDR, value);
            wr(EECR, EEMPE);
            wr(EECR, EEPE);
        }
    });
}

/// The EEPROM, behind [`hal::Store`].
pub struct Store;

impl hal::Store for Store {
    fn read(&self, address: u16) -> u8 {
        read(address)
    }

    fn write(&self, address: u16, value: u8) {
        write(address, value);
    }
}
