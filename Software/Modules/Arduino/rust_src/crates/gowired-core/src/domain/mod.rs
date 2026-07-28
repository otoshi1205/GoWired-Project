//! All behaviour. No registers, no protocol bytes, no `#[cfg(target_arch)]`.
//!
//! Six board variants map onto three [`Device`] implementations:
//!
//! | Variant                      | Type                    |
//! | ---------------------------- | ----------------------- |
//! | `DoubleRelay`, `FourRelay`   | [`RelayBankDevice`]     |
//! | `RollerShutter`              | [`RollerShutterDevice`] |
//! | `Dimmer`, `Rgb`, `Rgbw`      | [`DimmerDevice`]        |
//!
//! [`Module`] is written once against [`Device`] and does everything that is
//! common to all six: presentation, the initial-state burst Home Assistant
//! needs, current and temperature monitoring, the fault latches, the text
//! command channel, and the main loop.

pub mod config;
pub mod device;
pub mod dimmer;
pub mod dimmer_device;
pub mod input;
pub mod input_bank;
pub mod module;
pub mod monitors;
pub mod relay;
pub mod relay_bank;
pub mod roller_shutter;
pub mod sensing;
pub mod shutter;

pub use config::{
    ButtonTiming, ColorModel, DeviceKind, DimmerTuning, Features, InputPin, PowerTuning,
    ShutterTuning, StoreLayout, ThermalTuning, Timing, MAX_INPUTS,
};
pub use device::{Device, SafetyState};
pub use dimmer::Dimmer;
pub use dimmer_device::{DimmerDevice, DimmerSpec};
pub use input::{Button, ButtonEvent, DebouncedInput, DigitalSensor};
pub use input_bank::InputBank;
pub use module::{ConfigCommands, Module, Peripherals, PowerChannels, Settings, Wiring};
pub use monitors::{LatchedFault, PowerMonitor, ThermalMonitor};
pub use relay::Relay;
pub use relay_bank::{RelayBankDevice, RelayBankSpec};
pub use roller_shutter::{RollerShutterDevice, RollerShutterSpec};
pub use shutter::{Motion, Shutter, ShutterPins};
