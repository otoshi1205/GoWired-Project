/*
 * Configuration.h
 *
 * Quick and easy configuration of your GoWired module. Edit the marked
 * sections; everything below them is derived.
 *
 * Two kinds of setting live here and they are not interchangeable:
 *
 *   - MySensors transport settings must be #defines. They are that library's
 *     configuration API: it reads them while <MySensors.h> is being included.
 *
 *   - Everything else is `constexpr`. These are ordinary C++ values, so the
 *     compiler type-checks them, the unit tests can construct alternatives,
 *     and a mistake such as two children sharing an id is a build error rather
 *     than a device that silently misbehaves.
 */

#ifndef Configuration_h
#define Configuration_h

#include <Arduino.h> // for the A0..A7 pin names used in the pin map below

#include "src/domain/config.h"

/* ===========================================================================
 * 1. MySensors protocol settings  (must stay macros)
 * ======================================================================== */

// MY_NODE_ID -- unique per module. Two modules with the same id must not share a
// gateway. AUTO lets the gateway assign one; the official instructions
// recommend assigning it explicitly (e.g. 1) so the id survives a re-pairing.
#define MY_NODE_ID AUTO

#define SN "GoWired Module" // Sketch name presented to the controller
#define SV "3.0"            // Sketch (firmware) version

#define MY_RS485                         // Enable RS485 transport layer
#define MY_RS485_DE_PIN 7                // DE pin
#define MY_RS485_BAUD_RATE 57600
#define MY_RS485_HWSERIAL Serial
#define MY_RS485_SOH_COUNT 3             // Collision avoidance

#define MY_OTA_FIRMWARE_FEATURE          // FOTA updates
#define MY_TRANSPORT_WAIT_READY_MS 60000  // Startup wait for the gateway

/* ===========================================================================
 * 2. Which board is this?  --  set GW_DEVICE to exactly one of these
 * ======================================================================== */

#define GW_DOUBLE_RELAY 1   // 2SSR shield, two independent relays
#define GW_ROLLER_SHUTTER 2 // 2SSR shield driving one cover
#define GW_FOUR_RELAY 3     // 4RelayDin shield
#define GW_DIMMER 4         // single-colour dimmable LED strip
#define GW_RGB 5            // RGB strip
#define GW_RGBW 6           // RGBW strip

#define GW_DEVICE GW_DOUBLE_RELAY

/* ===========================================================================
 * 3. Optional peripherals
 * ======================================================================== */

namespace cfg {

constexpr bool kPowerSensor = true;
constexpr bool kInternalTemperature = true;
constexpr bool kExternalTemperature = false;
constexpr bool kErrorReporting = true;
constexpr bool kWatchdog = true;

} // namespace cfg

// Which external probe is fitted. Uncomment exactly one when
// kExternalTemperature is true, and install the matching library
// (arduino-sht or DHTlib). These have to be macros: they select which
// third-party header src/platform/external_probe.h includes.
//#define GW_PROBE_SHT30
//#define GW_PROBE_DHT22

namespace cfg {

#if defined(GW_PROBE_SHT30) || defined(GW_PROBE_DHT22)
constexpr bool kProbeSelected = true;
#else
constexpr bool kProbeSelected = false;
#endif

/// Node id to mirror external temperature to, or 0 to disable. Was
/// HEATING_SECTION_SENSOR / MY_HEATING_CONTROLLER.
constexpr uint8_t kHeatingControllerNode = 0;

/* ===========================================================================
 * 4. Tuning
 * ======================================================================== */

constexpr gw::ButtonTiming kButtons = {
    /* longpress_ms */ 1000,
    /* debounce_ms  */ 50,
};

constexpr gw::DimmerTuning kDimmer = {
    /* step        */ 1,  // brightness units per interval; larger is faster, coarser
    /* interval_ms */ 1,  // larger is slower
    /* toggle_step */ 20, // wall-switch brightness increment
};

constexpr gw::ShutterTuning kShutter = {
    /* up_time_s                 */ 21,
    /* down_time_s               */ 20,
    /* calibration_current_floor */ 0.2f,
    /* calibration_samples       */ 1,
};

constexpr gw::PowerTuning kPower = {
    /* max_current_a      */ 3,   // 2SSR 3 A; 4RelayDin 10 or 16 A
    /* receiver_voltage   */ 230, // 230 / 24 / 12, per the load
    /* cos_phi            */ 1.0f, // resistive 1; LED 0.4..0.99
    /* measuring_time_ms  */ 20,
    /* mv_per_amp         */ 185, // 2SSR 185; 4RelayDin 73; RGBW 100
};

constexpr gw::ThermalTuning kThermal = {
    /* max_temperature_c */ 85,
    /* mv_per_celsius    */ 10.0f,
    /* zero_voltage_mv   */ 500.0f,
};

constexpr gw::Timing kTiming = {
    /* report_interval_ms   */ 300000,
    /* presentation_delay_ms*/ 10,
    /* loop_time_ms         */ 80,
    /* init_echo_timeout_ms */ 2000,
};

/// First EEPROM address this sketch may use; 0..511 belongs to MySensors.
constexpr gw::StoreLayout kStore = {
    /* shutter_down_time */ 512,
    /* shutter_up_time   */ 513,
    /* shutter_position  */ 514,
    /* size              */ 1024,
};

/* ===========================================================================
 * 5. Pin map  --  GoWired MCU v1.0 / ATmega328P
 * ======================================================================== */

// Outputs (relay / PWM)
constexpr gw::Pin kOutput1 = 5;
constexpr gw::Pin kOutput2 = 9;
constexpr gw::Pin kOutput3 = 6;
constexpr gw::Pin kOutput4 = 10;

// Digital inputs
constexpr gw::Pin kInput1 = 2;
constexpr gw::Pin kInput2 = 3;
constexpr gw::Pin kInput3 = 4;
constexpr gw::Pin kInput4 = A3;

// Analog inputs
constexpr gw::Pin kInput5 = A1;
constexpr gw::Pin kInput6 = A2;
constexpr gw::Pin kInput7 = A6;
constexpr gw::Pin kInput8 = A7;

constexpr gw::Pin kOneWire = A0;
constexpr gw::Pin kI2cSda = A4;
constexpr gw::Pin kI2cScl = A5;

/// Pin level that de-energises a relay.
constexpr bool kRelayOffLevel = false; // LOW

/* ===========================================================================
 * 6. Derived  --  no need to edit below here
 * ======================================================================== */

constexpr gw::DeviceKind device_kind()
{
    switch (GW_DEVICE) {
    case GW_ROLLER_SHUTTER:
        return gw::DeviceKind::RollerShutter;
    case GW_FOUR_RELAY:
        return gw::DeviceKind::FourRelay;
    case GW_DIMMER:
        return gw::DeviceKind::Dimmer;
    case GW_RGB:
        return gw::DeviceKind::Rgb;
    case GW_RGBW:
        return gw::DeviceKind::Rgbw;
    default:
        return gw::DeviceKind::DoubleRelay;
    }
}

constexpr gw::DeviceKind kDevice = device_kind();

constexpr gw::ColorModel color_model()
{
    switch (kDevice) {
    case gw::DeviceKind::Rgb:
        return gw::ColorModel::Rgb;
    case gw::DeviceKind::Rgbw:
        return gw::ColorModel::Rgbw;
    default:
        return gw::ColorModel::White;
    }
}

/// Relay pin ordering differs per shield because of how the boards are routed.
constexpr gw::Pin relay_pin(uint8_t index)
{
    if (kDevice == gw::DeviceKind::FourRelay) {
        switch (index) {
        case 0:
            return kOutput3;
        case 1:
            return kOutput2;
        case 2:
            return kOutput1;
        default:
            return kOutput4;
        }
    }
    // 2SSR: relay 1 / shutter-up on OUT1, relay 2 / shutter-down on OUT2.
    return index == 0 ? kOutput1 : kOutput2;
}

/// The RGB(W) shield routes the white channel to OUT4 and R/G/B to OUT1..3.
constexpr gw::Pin led_pin(uint8_t index)
{
    if (kDevice == gw::DeviceKind::Dimmer) {
        switch (index) {
        case 0:
            return kOutput1;
        case 1:
            return kOutput2;
        case 2:
            return kOutput3;
        default:
            return kOutput4;
        }
    }
    switch (index) {
    case 0:
        return kOutput4;
    case 1:
        return kOutput1;
    case 2:
        return kOutput2;
    default:
        return kOutput3;
    }
}

constexpr gw::Pin kButtonPin1 = kInput1;
constexpr gw::Pin kButtonPin2 = kInput2;

constexpr gw::Pin current_sense_pin(uint8_t channel)
{
    if (kDevice == gw::DeviceKind::FourRelay) {
        switch (channel) {
        case 0:
            return kInput7;
        case 1:
            return kI2cScl;
        case 2:
            return kI2cSda;
        default:
            return kInput8;
        }
    }
    // The dimmer shields use the other spare analog pin for current sensing,
    // because their thermistor sits on A6.
    return (kDevice == gw::DeviceKind::DoubleRelay || kDevice == gw::DeviceKind::RollerShutter)
               ? kInput7
               : kInput8;
}

constexpr gw::Pin kInternalTempPin =
    (kDevice == gw::DeviceKind::DoubleRelay || kDevice == gw::DeviceKind::RollerShutter) ? kInput8
                                                                                        : kInput7;

constexpr gw::Pin input_pin(uint8_t index)
{
    switch (index) {
    case 0:
        return kInput3;
    case 1:
        return kInput4;
    case 2:
        return kInput5;
    default:
        return kInput6;
    }
}

/* ---------------------------------------------------------------------------
 * Digital inputs INPUT_1 .. INPUT_4
 *
 * Replaces the INPUT_n / PULLUP_n / INVERT_n macro triplets. All four may be
 * active at once.
 *
 *   enabled  was: #define INPUT_n
 *   pullup   was: #define PULLUP_n -- true for a dry contact switching to
 *                 ground, false for a sensor that drives the line itself
 *                 (the old "comment out PULLUP_n" variant)
 *   invert    was: #define INVERT_n -- reverses the active level
 *
 * Each slot keeps its own child id whether enabled or not, so turning INPUT_2
 * off does not renumber INPUT_3 and INPUT_4 under a controller already bound
 * to them. Any combination is legal, including none.
 * ------------------------------------------------------------------------ */

struct InputSetting {
    bool enabled;
    bool pullup;
    bool invert;
};

constexpr InputSetting kInputSettings[gw::kMaxInputs] = {
    /* INPUT_1 */ {true, true, false},
    /* INPUT_2 */ {true, true, false},
    /* INPUT_3 */ {true, true, false},
    /* INPUT_4 */ {true, true, false},
};

constexpr gw::InputPin generic_input(uint8_t index)
{
    return gw::InputPin{static_cast<gw::SensorId>(gw::first_input_id(kDevice) + index),
                        input_pin(index),
                        kInputSettings[index].enabled,
                        kInputSettings[index].pullup,
                        kInputSettings[index].invert};
}

constexpr gw::InputPin kInputs[gw::kMaxInputs] = {
    generic_input(0),
    generic_input(1),
    generic_input(2),
    generic_input(3),
};

constexpr gw::Features kFeatures = {
    /* power_sensor           */ kPowerSensor,
    /* internal_temperature   */ kInternalTemperature,
    /* external_temperature   */ kExternalTemperature,
    /* error_reporting        */ kErrorReporting,
    /* special_button         */ gw::button_count(kDevice) > 0,
    /* heating_controller_node*/ kHeatingControllerNode,
};

} // namespace cfg

/* ===========================================================================
 * 7. Build-time validation
 * ======================================================================== */

// The old macro chain silently aliased FOUR_RELAY's per-relay power sensors
// (ids 4..7) onto its digital inputs. Now it cannot.
static_assert(gw::ids_are_valid(cfg::kDevice, gw::enabled_input_mask(cfg::kInputs),
                                cfg::kFeatures),
              "Two children share a sensor id -- check kInputSettings and the enabled features");

// The 4RelayDin shield has no thermistor: its analog pins are taken by the four
// current sensors. This used to surface as 'IT_PIN was not declared'.
static_assert(!(cfg::kDevice == gw::DeviceKind::FourRelay && cfg::kInternalTemperature),
              "FOUR_RELAY has no internal thermometer; set kInternalTemperature = false");

static_assert(!cfg::kExternalTemperature || cfg::kProbeSelected,
              "kExternalTemperature is true but no probe is selected -- uncomment "
              "GW_PROBE_SHT30 or GW_PROBE_DHT22");

#if defined(GW_PROBE_SHT30) && defined(GW_PROBE_DHT22)
#error "Define at most one of GW_PROBE_SHT30 / GW_PROBE_DHT22"
#endif

#endif // Configuration_h
