/**
 * @file config.h
 * @brief Compile-time configuration types and the sensor-id layout.
 *
 * This replaces the macro cascade that used to live in Configuration.h. Two
 * things are worth knowing:
 *
 *  1. The sensor ids below are a *wire protocol*. A deployed controller (Home
 *     Assistant, Domoticz, ...) addresses children by these numbers, so they
 *     are pinned to the values the pre-refactor firmware used. Do not renumber
 *     them to make the table look tidier.
 *
 *  2. Config objects are consumed as constant expressions at the single wiring
 *     point in main.ino and their values are passed to constructors as
 *     scalars. Nothing takes their address. That matters on AVR, where a
 *     `const` object whose address escapes is copied into SRAM at startup
 *     rather than living in flash.
 */
#pragma once

#include "../hal/hal.h"

namespace gw {

enum class DeviceKind : uint8_t {
    DoubleRelay,
    RollerShutter,
    FourRelay,
    Dimmer,
    Rgb,
    Rgbw,
};

/// Colour model of a dimmer device; drives channel count and message types.
enum class ColorModel : uint8_t {
    White, // single brightness value, V_PERCENTAGE only
    Rgb,   // 3 channels, V_RGB
    Rgbw,  // 4 channels, V_RGBW
};

// ---------------------------------------------------------------------------
// Fixed sensor ids -- shared by every device kind. Wire protocol; pinned.
// ---------------------------------------------------------------------------
namespace ids {

constexpr SensorId kSpecialButton1 = 8;
constexpr SensorId kSpecialButton2 = 9;
constexpr SensorId kPower = 10;          ///< single-channel power sensor
constexpr SensorId kInternalTemp = 11;
constexpr SensorId kExternalTemp = 12;
constexpr SensorId kExternalHumidity = 13;
constexpr SensorId kOvercurrentStatus = 15;
constexpr SensorId kThermalStatus = 16;
constexpr SensorId kExternalTempStatus = 17;
constexpr SensorId kConfiguration = 20;

/// Per-relay power sensors, used only by FOUR_RELAY.
constexpr SensorId kPowerPerRelay[4] = {4, 5, 6, 7};

} // namespace ids

/// First id of the generic digital inputs (INPUT_1..4).
///
/// FOUR_RELAY is the odd one out: ids 4..7 are taken by its per-relay power
/// sensors, so its inputs start above kConfiguration. Nothing was ever
/// deployed with FOUR_RELAY inputs -- that variant did not compile before this
/// refactor -- so there is no wire compatibility to preserve here.
constexpr SensorId first_input_id(DeviceKind kind)
{
    return kind == DeviceKind::FourRelay ? 21 : 2;
}

/// Number of relay-ish outputs a device kind drives.
constexpr uint8_t output_count(DeviceKind kind)
{
    switch (kind) {
    case DeviceKind::DoubleRelay:
        return 2;
    case DeviceKind::RollerShutter:
        return 2; // up + down, driven as one logical cover
    case DeviceKind::FourRelay:
        return 4;
    case DeviceKind::Dimmer:
    case DeviceKind::Rgb:
    case DeviceKind::Rgbw:
        return 1;
    }
    return 0;
}

/// Wall-switch buttons wired to the device (FOUR_RELAY has none).
constexpr uint8_t button_count(DeviceKind kind)
{
    return kind == DeviceKind::FourRelay ? 0 : 2;
}

constexpr uint8_t channel_count(ColorModel model)
{
    switch (model) {
    case ColorModel::White:
        return 4; // legacy: single-colour dimmer drives all four pins together
    case ColorModel::Rgb:
        return 3;
    case ColorModel::Rgbw:
        return 4;
    }
    return 0;
}

// ---------------------------------------------------------------------------
// Configuration aggregates
// ---------------------------------------------------------------------------

struct ButtonTiming {
    uint16_t longpress_ms = 1000;
    uint8_t debounce_ms = 50;
};

struct DimmerTuning {
    uint8_t step = 1;         ///< brightness units per interval
    uint8_t interval_ms = 1;  ///< delay between steps
    uint8_t toggle_step = 20; ///< wall-switch brightness increment
};

struct ShutterTuning {
    uint8_t up_time_s = 21;
    uint8_t down_time_s = 20;
    float calibration_current_floor = 0.2f; ///< amps below which the motor is idle
    uint8_t calibration_samples = 1;
};

struct PowerTuning {
    uint8_t max_current_a = 3;
    uint8_t receiver_voltage = 230;
    float cos_phi = 1.0f;
    uint8_t measuring_time_ms = 20;
    uint8_t mv_per_amp = 185;
};

struct ThermalTuning {
    uint8_t max_temperature_c = 85;
    float mv_per_celsius = 10.0f;
    float zero_voltage_mv = 500.0f;
};

/// EEPROM layout. Offset 0..511 is used by MySensors itself.
struct StoreLayout {
    uint16_t shutter_down_time = 512;
    uint16_t shutter_up_time = 513;
    uint16_t shutter_position = 514;
    uint16_t size = 1024;
};

/// Number of generic digital inputs the board can expose.
constexpr uint8_t kMaxInputs = 4;

/// One generic digital input (a door/window contact, a motion sensor, ...).
///
/// Mirrors the documented INPUT_n / PULLUP_n / INVERT_n settings. Each slot
/// keeps its own child id whether or not it is enabled, so disabling INPUT_2
/// leaves a gap rather than renumbering INPUT_3 and INPUT_4 underneath a
/// controller that is already bound to them.
struct InputPin {
    SensorId id = kNoSensor;
    Pin pin = kNoPin;
    bool enabled = false;
    /// INPUT_PULLUP (a dry contact to ground) vs INPUT (a driven sensor
    /// output). Was: commenting out PULLUP_n.
    bool pullup = true;
    /// Reverses the active level. Was: INVERT_n.
    bool invert = false;
};

/// Bit i set means INPUT_(i+1) is enabled.
constexpr uint8_t enabled_input_mask(const InputPin (&inputs)[kMaxInputs])
{
    uint8_t mask = 0;
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        if (inputs[i].enabled) {
            mask = static_cast<uint8_t>(mask | (1u << i));
        }
    }
    return mask;
}

/// Which optional peripherals this board actually has.
struct Features {
    bool power_sensor = false;
    bool internal_temperature = false;
    bool external_temperature = false;
    bool error_reporting = false;
    bool special_button = false;
    /// Node id to mirror external temperature to, or 0 for none.
    uint8_t heating_controller_node = 0;
};

struct Timing {
    uint32_t report_interval_ms = 300000;
    uint16_t presentation_delay_ms = 10;
    uint16_t loop_time_ms = 80;
    /// Timeout when waiting for the controller to echo an initial value back.
    uint16_t init_echo_timeout_ms = 2000;
};

// ---------------------------------------------------------------------------
// Sensor-id collision check
// ---------------------------------------------------------------------------
//
// The old macro chain computed ids by addition (`#define TS_ID ES_ID+1`), which
// made collisions invisible -- FOUR_RELAY silently aliased its power sensors
// onto its digital inputs. This turns that class of mistake into a build error.

struct IdSet {
    SensorId v[32] = {};
    uint8_t n = 0;

    constexpr void add(SensorId id)
    {
        v[n] = id;
        ++n;
    }
};

constexpr bool all_distinct(const IdSet& s)
{
    for (uint8_t i = 0; i < s.n; ++i) {
        for (uint8_t j = static_cast<uint8_t>(i + 1); j < s.n; ++j) {
            if (s.v[i] == s.v[j]) {
                return false;
            }
        }
    }
    return true;
}

/// Enumerates every child id a given configuration will present.
/// @param enabled_inputs bitmask, bit i for INPUT_(i+1)
constexpr IdSet presented_ids(DeviceKind kind, uint8_t enabled_inputs, const Features& f)
{
    IdSet s;

    // Device children occupy the low ids.
    switch (kind) {
    case DeviceKind::DoubleRelay:
        s.add(0);
        s.add(1);
        break;
    case DeviceKind::FourRelay:
        s.add(0);
        s.add(1);
        s.add(2);
        s.add(3);
        break;
    case DeviceKind::RollerShutter:
    case DeviceKind::Dimmer:
    case DeviceKind::Rgb:
    case DeviceKind::Rgbw:
        s.add(0);
        break;
    }

    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        if (enabled_inputs & (1u << i)) {
            s.add(static_cast<SensorId>(first_input_id(kind) + i));
        }
    }

    if (f.special_button) {
        s.add(ids::kSpecialButton1);
        s.add(ids::kSpecialButton2);
    }

    if (f.power_sensor) {
        if (kind == DeviceKind::FourRelay) {
            for (uint8_t i = 0; i < 4; ++i) {
                s.add(ids::kPowerPerRelay[i]);
            }
        } else {
            s.add(ids::kPower);
        }
    }

    if (f.internal_temperature) {
        s.add(ids::kInternalTemp);
    }
    if (f.external_temperature) {
        s.add(ids::kExternalTemp);
        s.add(ids::kExternalHumidity);
    }

    if (f.error_reporting) {
        if (f.power_sensor) {
            s.add(ids::kOvercurrentStatus);
        }
        if (f.internal_temperature) {
            s.add(ids::kThermalStatus);
        }
        if (f.external_temperature) {
            s.add(ids::kExternalTempStatus);
        }
    }

    s.add(ids::kConfiguration);
    return s;
}

/// Instantiate once per build against the real configuration; see
/// Configuration.h. Also exercised over every kind and every input combination
/// by the unit tests.
constexpr bool ids_are_valid(DeviceKind kind, uint8_t enabled_inputs, const Features& f)
{
    return all_distinct(presented_ids(kind, enabled_inputs, f));
}

} // namespace gw
