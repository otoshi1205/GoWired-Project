//! String literals that live in flash instead of SRAM.
//!
//! # Why this exists
//!
//! On AVR there is no way to dereference a flash address with an ordinary
//! pointer: `lpm` is a different instruction to `ld`. The toolchain therefore
//! links `.rodata` *into SRAM* and copies it out of flash during startup, which
//! means a plain `&'static str` costs its full length in RAM on a part that has
//! 2 KB of it. The presentation strings alone ("Roller Shutter", "OVERCURRENT
//! ERROR", ...) are around 380 bytes.
//!
//! [`crate::gw_text!`] puts the bytes in `.progmem.data` -- a section avr-libc's
//! linker script places inside `.text` -- and hands back a [`Text`], which
//! reads them back through `lpm`. It is the direct equivalent of the C++
//! version's `GW_TEXT()` / `__FlashStringHelper` shim.
//!
//! On any other target the same macro yields a `&'static str` and [`Text`] is a
//! plain slice reference, so the domain code and its tests are identical.
//!
//! # Verifying it worked
//!
//! The point of this module is an absence, so it is easy to silently lose:
//!
//! ```text
//! avr-nm -C --size-sort -td target/avr-none/release/gowired-firmware.elf | grep -i ' [dD] '
//! ```
//!
//! No presentation string should appear. `tools/gowired-rs.py build --report`
//! checks this and fails the build if one does.

#![allow(unsafe_code)] // the whole point of the module: one lpm read

use core::marker::PhantomData;

/// A string literal held in program memory.
///
/// Construct with [`crate::gw_text!`]; there is deliberately no way to make one from a
/// runtime `&str`, because on AVR the pointer would then be in the wrong
/// address space and [`Text::copy_to`] would read whatever `lpm` found at the
/// same numeric offset in flash.
#[derive(Copy, Clone)]
pub struct Text {
    ptr: *const u8,
    len: u8,
    /// `Text` is only ever read, never written through, and must not be `Send`:
    /// on AVR the pointer is meaningless to anything but `lpm`.
    _not_send: PhantomData<*const u8>,
}

impl Text {
    /// # Safety
    ///
    /// `ptr` must point at `len` bytes in program memory on AVR (that is, at a
    /// static placed in `.progmem.data`), or at `len` readable bytes anywhere
    /// on other targets. Use [`crate::gw_text!`] rather than calling this.
    #[must_use]
    pub const unsafe fn from_raw(ptr: *const u8, len: u8) -> Self {
        Self {
            ptr,
            len,
            _not_send: PhantomData,
        }
    }

    /// Length in bytes. Never longer than a MySensors payload.
    #[must_use]
    pub const fn len(&self) -> u8 {
        self.len
    }

    /// Whether the literal is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Compares the literal against a runtime string.
    ///
    /// Byte-wise, so it never needs a RAM copy of the literal. This is what
    /// lets the configuration commands (`cmd1`..`cmd4`) live in flash and still
    /// be matched against an inbound payload.
    #[must_use]
    pub fn equals(&self, other: &str) -> bool {
        let other = other.as_bytes();
        if other.len() != self.len as usize {
            return false;
        }
        for (i, &b) in other.iter().enumerate() {
            // SAFETY: `i < len`, and the constructor's contract covers `len`
            // bytes of program memory.
            if unsafe { read_flash_byte(self.ptr.add(i)) } != b {
                return false;
            }
        }
        true
    }

    /// Copies the literal into `out`.
    ///
    /// Returns the number of bytes written, which is `min(len, out.len())` --
    /// a literal too long for a MySensors payload is truncated rather than
    /// dropped, matching what the C++ library did with an over-long
    /// `sendSketchInfo`.
    pub fn copy_to(&self, out: &mut [u8]) -> usize {
        let n = core::cmp::min(self.len as usize, out.len());
        for (i, slot) in out.iter_mut().enumerate().take(n) {
            // SAFETY: the constructor's contract is that `ptr` addresses `len`
            // bytes of program memory, and `i < n <= len`.
            *slot = unsafe { read_flash_byte(self.ptr.add(i)) };
        }
        n
    }
}

/// Reads one byte from program memory.
///
/// # Safety
///
/// `ptr` must point into program memory (AVR) or be a valid readable pointer
/// (elsewhere).
#[cfg(target_arch = "avr")]
unsafe fn read_flash_byte(ptr: *const u8) -> u8 {
    let out: u8;
    // `lpm Rd, Z` is the only way to reach flash. 16-bit Z register, which is
    // why this is correct on the 32 KB 328P/328PB and would need RAMPZ (elpm)
    // on a part with more than 64 KB of flash.
    core::arch::asm!(
        "lpm {out}, Z",
        out = out(reg) out,
        in("Z") ptr,
        options(pure, readonly, nostack, preserves_flags),
    );
    out
}

/// Host fallback: `.rodata` is directly addressable everywhere but AVR.
///
/// # Safety
///
/// `ptr` must be valid for a one-byte read.
#[cfg(not(target_arch = "avr"))]
unsafe fn read_flash_byte(ptr: *const u8) -> u8 {
    *ptr
}

/// Places a string literal in flash and yields a [`Text`] referring to it.
///
/// ```
/// # use gowired_core::gw_text;
/// let name = gw_text!("Roller Shutter");
/// let mut buf = [0u8; 25];
/// assert_eq!(name.copy_to(&mut buf), 14);
/// assert_eq!(&buf[..14], b"Roller Shutter");
/// ```
///
/// Literals longer than 255 bytes are a compile error, because [`Text`] stores
/// the length in a `u8`. A MySensors payload caps out at 25 anyway.
#[macro_export]
macro_rules! gw_text {
    ($literal:literal) => {{
        const LEN: usize = $literal.len();
        const _: () = assert!(LEN <= 255, "gw_text! literal is too long for a u8 length");

        // A `static` rather than a `const`: the address has to be real, and on
        // AVR it has to be *this* address, in .progmem.data.
        //
        // `link_section` is an unsafe attribute -- placing a static in a section
        // the linker treats differently is exactly the kind of thing that can go
        // wrong silently -- hence both the `unsafe(..)` wrapper and the allow.
        #[allow(unsafe_code)]
        #[cfg_attr(target_arch = "avr", unsafe(link_section = ".progmem.data"))]
        static BYTES: [u8; LEN] = {
            let src = $literal.as_bytes();
            let mut dst = [0u8; LEN];
            let mut i = 0;
            while i < LEN {
                dst[i] = src[i];
                i += 1;
            }
            dst
        };

        // SAFETY: BYTES is LEN bytes long and, on AVR, in program memory.
        //
        // The allow is on the statement rather than on the module, because the
        // block expands at the call site -- which is in code that (rightly)
        // denies unsafe.
        #[allow(unsafe_code)]
        let text = unsafe { $crate::text::Text::from_raw(BYTES.as_ptr(), LEN as u8) };
        text
    }};
}

#[cfg(test)]
mod tests {
    // `gw_text!` is macro_rules and defined above, so it is already in textual
    // scope here; no import needed.

    #[test]
    fn copies_the_literal() {
        let t = gw_text!("Relay 1");
        let mut buf = [0u8; 8];
        assert_eq!(t.copy_to(&mut buf), 7);
        assert_eq!(&buf[..7], b"Relay 1");
        assert_eq!(t.len(), 7);
    }

    #[test]
    fn truncates_rather_than_overflowing() {
        let t = gw_text!("OVERCURRENT ERROR");
        let mut buf = [0u8; 4];
        assert_eq!(t.copy_to(&mut buf), 4);
        assert_eq!(&buf, b"OVER");
    }

    #[test]
    fn empty_literal_is_empty() {
        let t = gw_text!("");
        assert!(t.is_empty());
        assert_eq!(t.copy_to(&mut [0u8; 4]), 0);
    }

    /// Two `gw_text!` uses of the same spelling are separate statics; nothing
    /// in the code relies on them being merged, but neither may they alias into
    /// one another's bytes.
    #[test]
    fn distinct_literals_do_not_alias() {
        let a = gw_text!("Input 1");
        let b = gw_text!("Input 2");
        let mut ba = [0u8; 7];
        let mut bb = [0u8; 7];
        a.copy_to(&mut ba);
        b.copy_to(&mut bb);
        assert_eq!(&ba, b"Input 1");
        assert_eq!(&bb, b"Input 2");
    }
}
