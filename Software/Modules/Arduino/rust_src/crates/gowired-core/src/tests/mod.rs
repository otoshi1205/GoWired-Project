//! The test suite.
//!
//! A port of the C++ suite in `../main/test/`, test for test, plus the protocol
//! coverage that suite had no reason to have (the C++ build got its protocol from
//! the MySensors library; this one implements it).
//!
//! Everything runs on the host against [`crate::fakes`]. No hardware, no
//! toolchain, no gateway.

mod config;
mod dimmer;
mod dimmer_device;
mod input;
mod input_bank;
mod message;
mod module;
mod monitors;
mod node;
mod relay_bank;
mod roller_shutter;
mod rs485;
mod sensing;
mod shutter;

/// Shared setup helpers.
pub mod support;
