//! Everything you edit. The replacement for `Configuration.h`.
//!
//! Two kinds of setting live here and they are not interchangeable:
//!
//! - **Which board this is** is a cargo feature, because it decides which of the
//!   three device implementations gets compiled in at all. Pass exactly one
//!   `device-*` feature; `tools/gowired-rs.py` does it for you.
//!
//! - **Everything else** is a `const` below. These are ordinary Rust values, so
//!   the compiler type-checks them, the unit tests can construct alternatives,
//!   and a mistake such as two children sharing an id is a build error rather
//!   than a device that silently misbehaves -- see [`ID_CHECK`].
//!
//! Every documented setting from the author's *Programowanie MCU* instructions
//! has a home here; `BUILDING.md` maps the old names onto the new ones.

// Every variant reads a different subset of this file -- a relay build has no use
// for DIMMER or led_pin, a dimmer build none for SHUTTER -- so `dead_code` would
// fire on whichever half is not in play. Deleting the unused half is not an
// option: this is the one file a user edits, and it has to describe the whole
// board.
#![allow(dead_code)]

use gowired_core::domain::config::{
    ids, ButtonTiming, ColorModel, DeviceKind, DimmerTuning, Features, InputPin, PowerTuning,
    ShutterTuning, StoreLayout, ThermalTuning, Timing, MAX_INPUTS,
};
use gowired_core::domain::config::{enabled_input_mask, ids_are_valid};
use gowired_core::hal::{Pin, SensorId};
use gowired_core::proto::AUTO_NODE_ID;
use gowired_core::text::Text;
use gowired_core::gw_text;

/* ===========================================================================
 * 1. Identity and transport
 * ======================================================================== */

/// Node id. Unique per module; two modules with the same id must not share a
/// gateway.
///
/// [`AUTO_NODE_ID`] asks the controller to assign one, which is then remembered
/// in EEPROM so it survives a power cut. The author's instructions recommend
/// setting it explicitly (1, 2, 3, ...) so the id survives a re-pairing too.
pub const NODE_ID: u8 = AUTO_NODE_ID;

/// Sketch name, as the controller displays it. Was `SN`.
///
/// A function rather than a `const` because the string has to stay in flash:
/// see [`gowired_core::text`]. Edit the literal.
#[must_use]
pub fn sketch_name() -> Text {
    gw_text!("GoWired Module")
}

/// Firmware version. Was `SV`.
#[must_use]
pub fn sketch_version() -> Text {
    gw_text!("3.0")
}

/// RS485 line rate. Both ends must agree.
pub const RS485_BAUD: u32 = 57600;

/// Driver-enable pin on the RS485 transceiver.
pub const RS485_DE_PIN: Pin = 7;

/// How many `SOH` bytes precede a frame.
///
/// More of them makes it likelier that a receiver which joined mid-collision
/// still finds the frame boundary. Was `MY_RS485_SOH_COUNT`.
pub const RS485_SOH_COUNT: u8 = 3;

/// How long to keep trying to reach the gateway at startup, in milliseconds.
///
/// Was `MY_TRANSPORT_WAIT_READY_MS`. After this the node carries on regardless
/// and keeps retrying from the main loop, so a module that boots before its
/// gateway recovers on its own.
pub const TRANSPORT_WAIT_READY_MS: u32 = 60_000;

/* ===========================================================================
 * 2. Optional peripherals
 * ======================================================================== */

/// A current sensor is fitted. Was `POWER_SENSOR`.
pub const POWER_SENSOR: bool = true;

/// The on-board thermometer is fitted. Was `INTERNAL_TEMP`.
///
/// The 4RelayDin shield has none: its analog pins are taken by the four current
/// sensors. Asking for one anyway is a build error -- see [`THERMOMETER_CHECK`].
pub const INTERNAL_TEMPERATURE: bool = true;

/// An external probe is fitted. Was `EXTERNAL_TEMP`.
///
/// Also enable exactly one of the `probe-sht30` / `probe-dht22` cargo features.
pub const EXTERNAL_TEMPERATURE: bool = false;

/// Publish the fault children (`OVERCURRENT ERROR`, `THERMAL ERROR`, ...).
pub const ERROR_REPORTING: bool = true;

/// Arm the hardware watchdog.
pub const WATCHDOG: bool = true;

/// Node id to mirror external temperature to, or 0 for none.
///
/// Was `HEATING_SECTION_SENSOR` / `MY_HEATING_CONTROLLER`.
pub const HEATING_CONTROLLER_NODE: u8 = 0;

/* ===========================================================================
 * 3. Tuning
 * ======================================================================== */

/// Wall-switch timing. Was `LONGPRESS_DURATION` / the debounce constant.
pub const BUTTONS: ButtonTiming = ButtonTiming {
    longpress_ms: 1000,
    debounce_ms: 50,
};

/// Dimmer ramp. Was `DIMMING_STEP` / `DIMMING_INTERVAL` / `DIMMING_TOGGLE_STEP`.
pub const DIMMER: DimmerTuning = DimmerTuning {
    step: 1,
    interval_ms: 1,
    toggle_step: 20,
};

/// Roller-shutter travel. Was `UP_TIME` / `DOWN_TIME` / `PS_OFFSET` /
/// `CALIBRATION_SAMPLES`.
pub const SHUTTER: ShutterTuning = ShutterTuning {
    up_time_s: 21,
    down_time_s: 20,
    calibration_current_floor_ma: 200,
    calibration_samples: 1,
};

/// Current limit and load. Was `MAX_CURRENT` / `RECEIVER_VOLTAGE` / `COSFI` /
/// `POWER_MEASURING_TIME` / `MVPERAMP`.
pub const POWER: PowerTuning = PowerTuning {
    max_current_a: 3,      // 2SSR 3 A; 4RelayDin 10 or 16 A
    receiver_voltage: 230, // 230 / 24 / 12, per the load
    cos_phi_percent: 100,  // resistive 100; LED 40..99
    measuring_time_ms: 20,
    mv_per_amp: 185, // 2SSR 185; 4RelayDin 73; RGBW 100
};

/// Temperature limit and sensor calibration. Was `MAX_TEMPERATURE`.
pub const THERMAL: ThermalTuning = ThermalTuning {
    max_temperature_c: 85,
    mv_per_celsius_x100: 1000, // 10.00 mV/°C
    zero_voltage_mv: 500,
};

/// Loop and reporting periods. Was `INTERVAL` / `LOOP_TIME`.
pub const TIMING: Timing = Timing {
    report_interval_ms: 300_000,
    presentation_delay_ms: 10,
    loop_time_ms: 80,
    init_echo_timeout_ms: 2000,
};

/// EEPROM layout. 0..511 belongs to the MySensors stack, including the node id.
pub const STORE: StoreLayout = StoreLayout {
    shutter_down_time: 512,
    shutter_up_time: 513,
    shutter_position: 514,
    size: 1024,
};

/* ===========================================================================
 * 4. Pin map -- GoWired MCU v1.0
 * ======================================================================== */

/// Relay / PWM output 1.
pub const OUT1: Pin = 5;
/// Relay / PWM output 2.
pub const OUT2: Pin = 9;
/// Relay / PWM output 3.
pub const OUT3: Pin = 6;
/// Relay / PWM output 4.
pub const OUT4: Pin = 10;

/// Digital input 1, also wall switch 1.
pub const IN1: Pin = 2;
/// Digital input 2, also wall switch 2.
pub const IN2: Pin = 3;
/// Digital input 3.
pub const IN3: Pin = 4;
/// Digital input 4. `A3` in Arduino terms.
pub const IN4: Pin = 17;

/// Analog input 5. `A1`.
pub const IN5: Pin = 15;
/// Analog input 6. `A2`.
pub const IN6: Pin = 16;
/// Analog input 7. `A6` -- ADC only.
pub const IN7: Pin = 20;
/// Analog input 8. `A7` -- ADC only.
pub const IN8: Pin = 21;

/// One-wire bus. `A0`.
pub const ONE_WIRE: Pin = 14;
/// I2C data. `A4`.
pub const I2C_SDA: Pin = 18;
/// I2C clock. `A5`.
pub const I2C_SCL: Pin = 19;

/// Pin level that de-energises a relay.
pub const RELAY_OFF_LEVEL: bool = false;

/* ===========================================================================
 * 5. Digital inputs INPUT_1 .. INPUT_4
 *
 * Replaces the INPUT_n / PULLUP_n / INVERT_n macro triplets. All four may be
 * active at once, and any combination is legal including none.
 *
 *   enabled  was: #define INPUT_n
 *   pullup   was: #define PULLUP_n -- true for a dry contact switching to
 *            ground, false for a sensor that drives the line itself
 *   invert   was: #define INVERT_n -- reverses the active level
 *
 * Each slot keeps its own child id whether enabled or not, so turning INPUT_2
 * off does not renumber INPUT_3 and INPUT_4 under a controller already bound to
 * them.
 * ======================================================================== */

/// One row of the input table.
#[derive(Copy, Clone)]
pub struct InputSetting {
    /// Whether the slot is used at all.
    pub enabled: bool,
    /// Enable the internal pull-up.
    pub pullup: bool,
    /// Reverse the active level.
    pub invert: bool,
}

/// The four input slots.
pub const INPUT_SETTINGS: [InputSetting; MAX_INPUTS] = [
    // INPUT_1
    InputSetting { enabled: true, pullup: true, invert: false },
    // INPUT_2
    InputSetting { enabled: true, pullup: true, invert: false },
    // INPUT_3
    InputSetting { enabled: true, pullup: true, invert: false },
    // INPUT_4
    InputSetting { enabled: true, pullup: true, invert: false },
];

/* ===========================================================================
 * 6. Derived -- no need to edit below here
 * ======================================================================== */

/// Which board this firmware is for, from the cargo feature.
pub const DEVICE: DeviceKind = device_kind();

const fn device_kind() -> DeviceKind {
    #[cfg(feature = "device-roller-shutter")]
    return DeviceKind::RollerShutter;
    #[cfg(feature = "device-four-relay")]
    return DeviceKind::FourRelay;
    #[cfg(feature = "device-dimmer")]
    return DeviceKind::Dimmer;
    #[cfg(feature = "device-rgb")]
    return DeviceKind::Rgb;
    #[cfg(feature = "device-rgbw")]
    return DeviceKind::Rgbw;
    #[cfg(feature = "device-double-relay")]
    return DeviceKind::DoubleRelay;
}

/// Colour model, for the dimmer variants.
pub const COLOR_MODEL: ColorModel = match DEVICE {
    DeviceKind::Rgb => ColorModel::Rgb,
    DeviceKind::Rgbw => ColorModel::Rgbw,
    _ => ColorModel::White,
};

/// Relay pin for output `index`.
///
/// The ordering differs per shield because of how the boards are routed.
#[must_use]
pub const fn relay_pin(index: usize) -> Pin {
    if matches!(DEVICE, DeviceKind::FourRelay) {
        return match index {
            0 => OUT3,
            1 => OUT2,
            2 => OUT1,
            _ => OUT4,
        };
    }
    // 2SSR: relay 1 / shutter-up on OUT1, relay 2 / shutter-down on OUT2.
    if index == 0 {
        OUT1
    } else {
        OUT2
    }
}

/// PWM pin for channel `index`.
///
/// The RGB(W) shield routes the white channel to OUT4 and R/G/B to OUT1..3.
#[must_use]
pub const fn led_pin(index: usize) -> Pin {
    if matches!(DEVICE, DeviceKind::Dimmer) {
        return match index {
            0 => OUT1,
            1 => OUT2,
            2 => OUT3,
            _ => OUT4,
        };
    }
    match index {
        0 => OUT4,
        1 => OUT1,
        2 => OUT2,
        _ => OUT3,
    }
}

/// Wall switch 1.
pub const BUTTON_PIN_1: Pin = IN1;
/// Wall switch 2.
pub const BUTTON_PIN_2: Pin = IN2;

/// Current-sense pin for channel `channel`.
#[must_use]
pub const fn current_sense_pin(channel: usize) -> Pin {
    if matches!(DEVICE, DeviceKind::FourRelay) {
        return match channel {
            0 => IN7,
            1 => I2C_SCL,
            2 => I2C_SDA,
            _ => IN8,
        };
    }
    // The dimmer shields use the other spare analog pin, because their
    // thermistor sits on A6.
    if matches!(DEVICE, DeviceKind::DoubleRelay | DeviceKind::RollerShutter) {
        IN7
    } else {
        IN8
    }
}

/// On-board thermometer pin.
pub const INTERNAL_TEMP_PIN: Pin =
    if matches!(DEVICE, DeviceKind::DoubleRelay | DeviceKind::RollerShutter) {
        IN8
    } else {
        IN7
    };

/// Pin for generic input `index`.
#[must_use]
pub const fn input_pin(index: usize) -> Pin {
    match index {
        0 => IN3,
        1 => IN4,
        2 => IN5,
        _ => IN6,
    }
}

/// The four input slots, resolved into pins and child ids.
pub const INPUTS: [InputPin; MAX_INPUTS] = [
    generic_input(0),
    generic_input(1),
    generic_input(2),
    generic_input(3),
];

const fn generic_input(index: usize) -> InputPin {
    InputPin {
        id: gowired_core::domain::config::first_input_id(DEVICE) + index as u8,
        pin: input_pin(index),
        enabled: INPUT_SETTINGS[index].enabled,
        pullup: INPUT_SETTINGS[index].pullup,
        invert: INPUT_SETTINGS[index].invert,
    }
}

/// Which optional peripherals this build has.
pub const FEATURES: Features = Features {
    power_sensor: POWER_SENSOR,
    internal_temperature: INTERNAL_TEMPERATURE,
    external_temperature: EXTERNAL_TEMPERATURE,
    error_reporting: ERROR_REPORTING,
    special_button: gowired_core::domain::config::button_count(DEVICE) > 0,
    heating_controller_node: HEATING_CONTROLLER_NODE,
};

/// Child id each power channel publishes under.
#[must_use]
pub const fn power_child_id(channel: usize) -> SensorId {
    if matches!(DEVICE, DeviceKind::FourRelay) {
        ids::POWER_PER_RELAY[channel]
    } else {
        ids::POWER
    }
}

/// How many current sensors this board has.
pub const POWER_CHANNELS: usize = if matches!(DEVICE, DeviceKind::FourRelay) {
    4
} else {
    1
};

/* ===========================================================================
 * 7. Build-time validation
 * ======================================================================== */

/// No two children may share an id.
///
/// The old macro chain computed ids by addition, which made collisions
/// invisible: `FOUR_RELAY` silently aliased its per-relay power sensors (4..7)
/// onto its digital inputs. Now it cannot.
pub const ID_CHECK: () = assert!(
    ids_are_valid(DEVICE, enabled_input_mask(&INPUTS), &FEATURES),
    "two children share a sensor id -- check INPUT_SETTINGS and the enabled features"
);

/// The 4RelayDin shield has no thermometer.
///
/// This used to surface as `IT_PIN was not declared`.
pub const THERMOMETER_CHECK: () = assert!(
    !(matches!(DEVICE, DeviceKind::FourRelay) && INTERNAL_TEMPERATURE),
    "FOUR_RELAY has no internal thermometer; set INTERNAL_TEMPERATURE = false"
);

/// An external probe has to be selected if one is enabled.
pub const PROBE_CHECK: () = assert!(
    !EXTERNAL_TEMPERATURE || cfg!(any(feature = "probe-sht30", feature = "probe-dht22")),
    "EXTERNAL_TEMPERATURE is true but no probe feature is enabled -- \
     pass --features probe-sht30 or --features probe-dht22"
);

/// Exactly one device feature.
const DEVICE_FEATURE_COUNT: usize = cfg!(feature = "device-double-relay") as usize
    + cfg!(feature = "device-roller-shutter") as usize
    + cfg!(feature = "device-four-relay") as usize
    + cfg!(feature = "device-dimmer") as usize
    + cfg!(feature = "device-rgb") as usize
    + cfg!(feature = "device-rgbw") as usize;

/// Exactly one `device-*` feature must be enabled.
pub const DEVICE_FEATURE_CHECK: () = assert!(
    DEVICE_FEATURE_COUNT == 1,
    "enable exactly one device-* feature; cargo unions features, so \
     --no-default-features is needed when picking a non-default one"
);
