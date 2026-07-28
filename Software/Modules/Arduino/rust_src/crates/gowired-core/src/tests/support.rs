//! Builders shared by the device and module tests.

use crate::domain::config::{ButtonTiming, ColorModel, DimmerTuning, InputPin, MAX_INPUTS};
use crate::domain::{
    DimmerDevice, DimmerSpec, InputBank, RelayBankDevice, RelayBankSpec, RollerShutterDevice,
    RollerShutterSpec, ShutterPins, StoreLayout,
};
use crate::fakes::{FakePlatform, Rig};
use crate::hal::{Pin, NO_PIN};

/// Pin map used throughout the tests. Same numbers as the shipped
/// configuration, so a test failure points at real hardware.
pub mod pins {
    use crate::hal::Pin;

    /// `OUT1`
    pub const OUT1: Pin = 5;
    /// `OUT2`
    pub const OUT2: Pin = 9;
    /// `OUT3`
    pub const OUT3: Pin = 6;
    /// `OUT4`
    pub const OUT4: Pin = 10;
    /// Wall switch 1, also `INPUT_1`'s pin on the shield.
    pub const BUTTON1: Pin = 2;
    /// Wall switch 2.
    pub const BUTTON2: Pin = 3;
    /// `INPUT_1`
    pub const IN1: Pin = 4;
    /// `INPUT_2`
    pub const IN2: Pin = 17;
    /// `INPUT_3`
    pub const IN3: Pin = 15;
    /// `INPUT_4`
    pub const IN4: Pin = 16;
}

/// A two-relay 2SSR bank with both wall switches.
pub fn double_relay_spec() -> RelayBankSpec {
    RelayBankSpec {
        relay_count: 2,
        relay_pins: [pins::OUT1, pins::OUT2, NO_PIN, NO_PIN],
        button_count: 2,
        button_pins: [pins::BUTTON1, pins::BUTTON2],
        off_level: false,
        per_relay_power: false,
    }
}

/// A four-relay 4RelayDin bank: no wall switches, one sensor per output.
pub fn four_relay_spec() -> RelayBankSpec {
    RelayBankSpec {
        relay_count: 4,
        relay_pins: [pins::OUT3, pins::OUT2, pins::OUT1, pins::OUT4],
        button_count: 0,
        button_pins: [NO_PIN, NO_PIN],
        off_level: false,
        per_relay_power: true,
    }
}

/// A relay bank over the given spec.
pub fn relay_bank<'a>(
    rig: &'a Rig,
    spec: RelayBankSpec,
    special_button: bool,
) -> RelayBankDevice<'a, FakePlatform> {
    RelayBankDevice::new(
        &rig.gpio,
        &rig.clock,
        spec,
        ButtonTiming::default(),
        special_button,
    )
}

/// A cover, optionally with end-stop current sensing.
pub fn roller_shutter(rig: &Rig, current_sensing: bool) -> RollerShutterDevice<'_, FakePlatform> {
    RollerShutterDevice::new(
        &rig.gpio,
        &rig.clock,
        &rig.store,
        RollerShutterSpec {
            pins: ShutterPins {
                up: pins::OUT1,
                down: pins::OUT2,
                off_level: false,
            },
            button_pins: [pins::BUTTON1, pins::BUTTON2],
            current_floor_ma: 200,
            calibration_samples: 1,
            default_up_time_s: 21,
            default_down_time_s: 20,
            current_sensing,
        },
        StoreLayout::default(),
        ButtonTiming::default(),
        true,
    )
}

/// A dimmer of the given colour model.
pub fn dimmer_device(rig: &Rig, model: ColorModel) -> DimmerDevice<'_, FakePlatform> {
    let led_pins = match model {
        // The RGB(W) shield routes white to OUT4 and R/G/B to OUT1..3.
        ColorModel::White => [pins::OUT1, pins::OUT2, pins::OUT3, pins::OUT4],
        ColorModel::Rgb | ColorModel::Rgbw => [pins::OUT4, pins::OUT1, pins::OUT2, pins::OUT3],
    };
    DimmerDevice::new(
        &rig.pwm,
        &rig.gpio,
        &rig.clock,
        DimmerSpec {
            model,
            led_pins,
            button_pins: [pins::BUTTON1, pins::BUTTON2],
        },
        DimmerTuning::default(),
        ButtonTiming::default(),
        true,
    )
}

/// Four input slots with the given enable flags, starting at child id 2.
pub fn inputs(enabled: [bool; MAX_INPUTS]) -> [InputPin; MAX_INPUTS] {
    let pin_of: [Pin; MAX_INPUTS] = [pins::IN1, pins::IN2, pins::IN3, pins::IN4];
    core::array::from_fn(|i| InputPin {
        id: 2 + i as u8,
        pin: pin_of[i],
        enabled: enabled[i],
        pullup: true,
        invert: false,
    })
}

/// An input bank over the given slots.
pub fn input_bank(rig: &Rig, pins: [InputPin; MAX_INPUTS]) -> InputBank<'_, FakePlatform> {
    InputBank::new(&rig.gpio, &rig.clock, pins, 50)
}

/// An input bank with nothing enabled, for devices under test that do not care.
pub fn no_inputs(rig: &Rig) -> InputBank<'_, FakePlatform> {
    input_bank(rig, inputs([false; MAX_INPUTS]))
}
