//! Compile-time configuration types and the sensor-id layout.
//!
//! Two things are worth knowing before changing anything here:
//!
//! 1. The sensor ids below are a **wire protocol**. A deployed controller (Home
//!    Assistant, Domoticz, ...) addresses children by these numbers, so they are
//!    pinned to the values the pre-refactor firmware used. Do not renumber them
//!    to make the table look tidier.
//!
//! 2. Every type here is `Copy` and every constructor is a `const fn`, so the
//!    firmware's configuration is a set of constants evaluated at compile time.
//!    On AVR that keeps it in flash: a `static` whose address is taken would be
//!    copied into SRAM at startup instead.

use crate::hal::{Milliamps, Millivolts, Pin, SensorId, NO_PIN, NO_SENSOR};

/// Which board this firmware is for.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    /// 2SSR shield, two independent relays.
    DoubleRelay,
    /// 2SSR shield driving one cover.
    RollerShutter,
    /// 4RelayDin shield.
    FourRelay,
    /// Single-colour dimmable LED strip.
    Dimmer,
    /// RGB strip.
    Rgb,
    /// RGBW strip.
    Rgbw,
}

/// Colour model of a dimmer device; drives channel count and message types.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ColorModel {
    /// Single brightness value, `V_PERCENTAGE` only.
    White,
    /// Three channels, `V_RGB`.
    Rgb,
    /// Four channels, `V_RGBW`.
    Rgbw,
}

/// Fixed sensor ids, shared by every device kind. Wire protocol; pinned.
pub mod ids {
    use crate::hal::SensorId;

    /// First wall-switch long-press child.
    pub const SPECIAL_BUTTON_1: SensorId = 8;
    /// Second wall-switch long-press child.
    pub const SPECIAL_BUTTON_2: SensorId = 9;
    /// Single-channel power sensor.
    pub const POWER: SensorId = 10;
    /// On-board thermometer.
    pub const INTERNAL_TEMP: SensorId = 11;
    /// External probe temperature.
    pub const EXTERNAL_TEMP: SensorId = 12;
    /// External probe humidity.
    pub const EXTERNAL_HUMIDITY: SensorId = 13;
    /// Overcurrent fault child.
    pub const OVERCURRENT_STATUS: SensorId = 15;
    /// Thermal fault child.
    pub const THERMAL_STATUS: SensorId = 16;
    /// External probe fault child.
    pub const EXTERNAL_TEMP_STATUS: SensorId = 17;
    /// Text command child.
    pub const CONFIGURATION: SensorId = 20;

    /// Per-relay power sensors, used only by `FourRelay`.
    pub const POWER_PER_RELAY: [SensorId; 4] = [4, 5, 6, 7];
}

/// First id of the generic digital inputs (`INPUT_1..4`).
///
/// `FourRelay` is the odd one out: ids 4..7 are taken by its per-relay power
/// sensors, so its inputs start above [`ids::CONFIGURATION`]. Nothing was ever
/// deployed with `FourRelay` inputs -- that variant did not compile before the
/// C++ refactor -- so there is no wire compatibility to preserve there.
#[must_use]
pub const fn first_input_id(kind: DeviceKind) -> SensorId {
    match kind {
        DeviceKind::FourRelay => 21,
        _ => 2,
    }
}

/// Number of relay-ish outputs a device kind drives.
#[must_use]
pub const fn output_count(kind: DeviceKind) -> u8 {
    match kind {
        // Up + down, driven as one logical cover.
        DeviceKind::DoubleRelay | DeviceKind::RollerShutter => 2,
        DeviceKind::FourRelay => 4,
        DeviceKind::Dimmer | DeviceKind::Rgb | DeviceKind::Rgbw => 1,
    }
}

/// Wall-switch buttons wired to the device. `FourRelay` has none.
#[must_use]
pub const fn button_count(kind: DeviceKind) -> u8 {
    match kind {
        DeviceKind::FourRelay => 0,
        _ => 2,
    }
}

/// PWM channels a colour model drives.
#[must_use]
pub const fn channel_count(model: ColorModel) -> u8 {
    match model {
        // Legacy: a single-colour dimmer drives all four pins together.
        ColorModel::White | ColorModel::Rgbw => 4,
        ColorModel::Rgb => 3,
    }
}

/// Wall-switch timing.
#[derive(Copy, Clone, Debug)]
pub struct ButtonTiming {
    /// Hold time that counts as a long press.
    pub longpress_ms: u16,
    /// Settling time before a level is believed.
    pub debounce_ms: u8,
}

impl Default for ButtonTiming {
    fn default() -> Self {
        Self {
            longpress_ms: 1000,
            debounce_ms: 50,
        }
    }
}

/// Dimmer ramp tuning.
#[derive(Copy, Clone, Debug)]
pub struct DimmerTuning {
    /// Brightness units per interval.
    pub step: u8,
    /// Delay between steps.
    pub interval_ms: u8,
    /// Wall-switch brightness increment.
    pub toggle_step: u8,
}

impl Default for DimmerTuning {
    fn default() -> Self {
        Self {
            step: 1,
            interval_ms: 1,
            toggle_step: 20,
        }
    }
}

/// Roller-shutter travel tuning.
#[derive(Copy, Clone, Debug)]
pub struct ShutterTuning {
    /// Seconds for a full open.
    pub up_time_s: u8,
    /// Seconds for a full close.
    pub down_time_s: u8,
    /// Current below which the motor is considered idle.
    pub calibration_current_floor_ma: Milliamps,
    /// How many travel measurements to average.
    pub calibration_samples: u8,
}

impl Default for ShutterTuning {
    fn default() -> Self {
        Self {
            up_time_s: 21,
            down_time_s: 20,
            calibration_current_floor_ma: 200,
            calibration_samples: 1,
        }
    }
}

/// Current measurement and limit.
#[derive(Copy, Clone, Debug)]
pub struct PowerTuning {
    /// Trip current.
    pub max_current_a: u8,
    /// Load voltage, for the watts calculation.
    pub receiver_voltage: u8,
    /// Power factor as a percentage: 100 is resistive, 40..99 for LED drivers.
    ///
    /// A percentage rather than a fraction because the whole measurement chain is
    /// integer -- see [`crate::hal::Milliamps`].
    pub cos_phi_percent: u8,
    /// Sampling window.
    pub measuring_time_ms: u8,
    /// Sensor sensitivity: 2SSR 185, 4RelayDin 73, RGBW 100.
    pub mv_per_amp: u8,
}

impl Default for PowerTuning {
    fn default() -> Self {
        Self {
            max_current_a: 3,
            receiver_voltage: 230,
            cos_phi_percent: 100,
            measuring_time_ms: 20,
            mv_per_amp: 185,
        }
    }
}

/// On-board thermometer calibration and limit.
#[derive(Copy, Clone, Debug)]
pub struct ThermalTuning {
    /// Trip temperature.
    pub max_temperature_c: u8,
    /// Sensor slope, in hundredths of a millivolt per degree.
    ///
    /// 1000 is the 10.0 mV/°C of an MCP9700. Hundredths so that a sensor with a
    /// fractional coefficient is still expressible.
    pub mv_per_celsius_x100: u16,
    /// Sensor output at 0 °C, in millivolts.
    pub zero_voltage_mv: Millivolts,
}

impl Default for ThermalTuning {
    fn default() -> Self {
        Self {
            max_temperature_c: 85,
            mv_per_celsius_x100: 1000,
            zero_voltage_mv: 500,
        }
    }
}

/// EEPROM layout. Offsets 0..511 belong to the MySensors stack.
#[derive(Copy, Clone, Debug)]
pub struct StoreLayout {
    /// Cell holding the measured down-travel time, in seconds.
    pub shutter_down_time: u16,
    /// Cell holding the measured up-travel time, in seconds.
    pub shutter_up_time: u16,
    /// Cell holding the last known shutter position, in percent.
    pub shutter_position: u16,
    /// Total EEPROM size, used by the erase command.
    pub size: u16,
}

impl Default for StoreLayout {
    fn default() -> Self {
        Self {
            shutter_down_time: 512,
            shutter_up_time: 513,
            shutter_position: 514,
            size: 1024,
        }
    }
}

/// Number of generic digital inputs the board can expose.
pub const MAX_INPUTS: usize = 4;

/// One generic digital input: a door/window contact, a motion sensor, ...
///
/// Mirrors the documented `INPUT_n` / `PULLUP_n` / `INVERT_n` settings. Each
/// slot keeps its own child id whether or not it is enabled, so disabling
/// `INPUT_2` leaves a gap rather than renumbering `INPUT_3` and `INPUT_4`
/// underneath a controller that is already bound to them.
#[derive(Copy, Clone, Debug)]
pub struct InputPin {
    /// Child id, kept even when disabled.
    pub id: SensorId,
    /// Which pin it is wired to.
    pub pin: Pin,
    /// Was: `#define INPUT_n`.
    pub enabled: bool,
    /// `InputPullup` for a dry contact to ground, `Input` for a driven line.
    /// Was: commenting out `PULLUP_n`.
    pub pullup: bool,
    /// Reverses the active level. Was: `INVERT_n`.
    pub invert: bool,
}

impl InputPin {
    /// An unwired, disabled slot.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            id: NO_SENSOR,
            pin: NO_PIN,
            enabled: false,
            pullup: true,
            invert: false,
        }
    }
}

impl Default for InputPin {
    fn default() -> Self {
        Self::none()
    }
}

/// Bit *i* set means `INPUT_(i+1)` is enabled.
#[must_use]
pub const fn enabled_input_mask(inputs: &[InputPin; MAX_INPUTS]) -> u8 {
    let mut mask = 0u8;
    let mut i = 0;
    while i < MAX_INPUTS {
        if inputs[i].enabled {
            mask |= 1 << i;
        }
        i += 1;
    }
    mask
}

/// Which optional peripherals this board actually has.
#[derive(Copy, Clone, Debug, Default)]
pub struct Features {
    /// A current sensor is fitted.
    pub power_sensor: bool,
    /// The on-board thermometer is fitted.
    pub internal_temperature: bool,
    /// An external probe is fitted.
    pub external_temperature: bool,
    /// Publish fault children.
    pub error_reporting: bool,
    /// Report wall-switch long presses as their own children.
    pub special_button: bool,
    /// Node id to mirror external temperature to, or 0 for none.
    pub heating_controller_node: u8,
}

/// Loop and reporting periods.
#[derive(Copy, Clone, Debug)]
pub struct Timing {
    /// How often the slow sensors are published regardless of change.
    pub report_interval_ms: u32,
    /// Gap between presentation messages, so the gateway keeps up.
    pub presentation_delay_ms: u16,
    /// Nominal main-loop period.
    pub loop_time_ms: u16,
    /// Timeout when waiting for the controller to echo an initial value back.
    pub init_echo_timeout_ms: u16,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            report_interval_ms: 300_000,
            presentation_delay_ms: 10,
            loop_time_ms: 80,
            init_echo_timeout_ms: 2000,
        }
    }
}

// ---------------------------------------------------------------------------
// Sensor-id collision check
// ---------------------------------------------------------------------------
//
// The pre-refactor macro chain computed ids by addition (`#define TS_ID
// ES_ID+1`), which made collisions invisible: FOUR_RELAY silently aliased its
// power sensors onto its digital inputs. `ids_are_valid` in a `const {}` block
// turns that class of mistake into a compile error.

/// The set of child ids a configuration presents.
#[derive(Copy, Clone, Debug)]
pub struct IdSet {
    /// Ids, `n` of them live.
    pub v: [SensorId; 32],
    /// How many entries of `v` are used.
    pub n: u8,
}

impl IdSet {
    /// An empty set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            v: [NO_SENSOR; 32],
            n: 0,
        }
    }

    /// Adds an id. Takes and returns `self` so it can be used in a `const fn`.
    #[must_use]
    pub const fn add(mut self, id: SensorId) -> Self {
        self.v[self.n as usize] = id;
        self.n += 1;
        self
    }

    /// Whether the set contains `id`.
    #[must_use]
    pub const fn contains(&self, id: SensorId) -> bool {
        let mut i = 0;
        while i < self.n as usize {
            if self.v[i] == id {
                return true;
            }
            i += 1;
        }
        false
    }
}

impl Default for IdSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether every id in the set is distinct.
#[must_use]
pub const fn all_distinct(s: &IdSet) -> bool {
    let mut i = 0;
    while i < s.n as usize {
        let mut j = i + 1;
        while j < s.n as usize {
            if s.v[i] == s.v[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// Enumerates every child id a given configuration will present.
///
/// `enabled_inputs` is the bitmask from [`enabled_input_mask`].
#[must_use]
pub const fn presented_ids(kind: DeviceKind, enabled_inputs: u8, f: &Features) -> IdSet {
    let mut s = IdSet::new();

    // Device children occupy the low ids.
    match kind {
        DeviceKind::DoubleRelay => {
            s = s.add(0);
            s = s.add(1);
        }
        DeviceKind::FourRelay => {
            s = s.add(0);
            s = s.add(1);
            s = s.add(2);
            s = s.add(3);
        }
        DeviceKind::RollerShutter | DeviceKind::Dimmer | DeviceKind::Rgb | DeviceKind::Rgbw => {
            s = s.add(0);
        }
    }

    let mut i = 0u8;
    while (i as usize) < MAX_INPUTS {
        if enabled_inputs & (1 << i) != 0 {
            s = s.add(first_input_id(kind) + i);
        }
        i += 1;
    }

    if f.special_button {
        s = s.add(ids::SPECIAL_BUTTON_1);
        s = s.add(ids::SPECIAL_BUTTON_2);
    }

    if f.power_sensor {
        if matches!(kind, DeviceKind::FourRelay) {
            let mut ch = 0;
            while ch < 4 {
                s = s.add(ids::POWER_PER_RELAY[ch]);
                ch += 1;
            }
        } else {
            s = s.add(ids::POWER);
        }
    }

    if f.internal_temperature {
        s = s.add(ids::INTERNAL_TEMP);
    }
    if f.external_temperature {
        s = s.add(ids::EXTERNAL_TEMP);
        s = s.add(ids::EXTERNAL_HUMIDITY);
    }

    if f.error_reporting {
        if f.power_sensor {
            s = s.add(ids::OVERCURRENT_STATUS);
        }
        if f.internal_temperature {
            s = s.add(ids::THERMAL_STATUS);
        }
        if f.external_temperature {
            s = s.add(ids::EXTERNAL_TEMP_STATUS);
        }
    }

    s.add(ids::CONFIGURATION)
}

/// Whether a configuration presents any child id twice.
///
/// Evaluate this in a `const` block against the real configuration -- see the
/// firmware's `config.rs` -- and it becomes a build error rather than a device
/// that silently misbehaves. The tests exercise it over every kind and every
/// input combination.
#[must_use]
pub const fn ids_are_valid(kind: DeviceKind, enabled_inputs: u8, f: &Features) -> bool {
    all_distinct(&presented_ids(kind, enabled_inputs, f))
}
