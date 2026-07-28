//! GoWired module firmware, hardware-independent half.
//!
//! Nothing in this crate touches a register, so all of it -- the device
//! behaviour *and* the MySensors protocol encoding -- runs under `cargo test`
//! on the host. The ATmega328P/PB register layer is `gowired-avr`, and the
//! wiring that names one device variant is `gowired-firmware`.
//!
//! # Layout
//!
//! - [`hal`] -- traits the hardware must implement. The Rust equivalent of the
//!   old `IGpio` / `IClock` / `IStore` / `IBus` interfaces, except that they are
//!   resolved statically: exactly one implementation of each is named by the
//!   firmware, so there are no vtables and no dynamic dispatch in the binary.
//! - [`text`] -- string literals that stay in flash. On AVR `.rodata` is copied
//!   into SRAM at startup, so a plain `&'static str` costs RAM the module does
//!   not have. This is the `PROGMEM` shim.
//! - [`domain`] -- all behaviour. Six board variants over three device
//!   implementations.
//! - [`proto`] -- MySensors 2.x message encoding and the RS485 link. Written
//!   from the protocol, not bound to the C++ library; see the module docs for
//!   exactly which parts of MySensors are implemented and which are not.
//!
//! # Dispatch
//!
//! Everything is generic over [`hal::Platform`], a bundle of associated types.
//! One `Platform` impl in the firmware fixes every HAL type at compile time,
//! which is what replaces the C++ `IDevice*` indirection. The tests supply a
//! second impl made of fakes.

#![no_std]
// Reading a byte out of flash needs `lpm`, and inline asm for tier-3
// architectures is still feature-gated. AVR only; the host build of this crate
// compiles on a stable-shaped nightly.
#![cfg_attr(target_arch = "avr", feature(asm_experimental_arch))]
#![cfg_attr(test, allow(clippy::float_cmp))]

#[cfg(test)]
extern crate std;

pub mod domain;
pub mod hal;
pub mod proto;
pub mod text;

#[cfg(any(test, feature = "testing"))]
pub mod fakes;

#[cfg(test)]
mod tests;

pub use text::Text;
