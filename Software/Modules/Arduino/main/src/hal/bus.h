/**
 * @file bus.h
 * @brief Controller-facing transport seam.
 *
 * The domain layer never includes a MySensors header. It speaks in terms of
 * ValueType / SensorClass / InboundMessage, and the single MySensors
 * translation lives in platform/mysensors_bus.h.
 *
 * That split is not only for testability: MySensors pulls its entire transport
 * implementation in through <MySensors.h> and may therefore only be included
 * from one translation unit -- the .ino. Keeping it behind IBus means the rest
 * of the sketch is free of that constraint.
 */
#pragma once

#include "hal.h"
#include "text.h"

namespace gw {

/// Maps onto the MySensors V_* variable types.
enum class ValueType : uint8_t {
    Status,      // V_STATUS
    Percentage,  // V_PERCENTAGE
    Watt,        // V_WATT
    Temperature, // V_TEMP
    Humidity,    // V_HUM
    Text,        // V_TEXT
    Up,          // V_UP
    Down,        // V_DOWN
    Stop,        // V_STOP
    Rgb,         // V_RGB
    Rgbw,        // V_RGBW
};

/// Maps onto the MySensors S_* sensor types.
enum class SensorClass : uint8_t {
    Binary,
    Cover,
    Dimmer,
    RgbLight,
    RgbwLight,
    Power,
    Temperature,
    Humidity,
    Info,
};

/// A decoded inbound message.
///
/// All three payload views are filled by the transport adapter so that the
/// domain does not have to know how MySensors encodes a payload. `numeric` is
/// specifically atoi() over the raw buffer, which is what the original sketch
/// used for V_PERCENTAGE.
struct InboundMessage {
    SensorId sensor = kNoSensor;
    ValueType type = ValueType::Status;
    bool boolean = false;
    long numeric = 0;
    const char* text = "";
};

class IBus {
public:
    virtual void send_sketch_info(TextRef name, TextRef version) = 0;
    virtual bool present(SensorId sensor, SensorClass type, TextRef name) = 0;

    virtual void send_bool(SensorId sensor, ValueType type, bool value) = 0;
    virtual void send_uint(SensorId sensor, ValueType type, uint32_t value) = 0;
    virtual void send_float(SensorId sensor, ValueType type, float value, uint8_t decimals) = 0;

    /// Sends a string held in RAM -- used to echo a received payload back.
    virtual void send_text(SensorId sensor, ValueType type, const char* value) = 0;

    /// Sends a compile-time literal, which on AVR stays in flash. Distinct name
    /// rather than an overload because TextRef and const char* are the same
    /// type on a host build.
    virtual void send_literal(SensorId sensor, ValueType type, TextRef value) = 0;

    /// Routes a reading to another node (used by HEATING_SECTION_SENSOR).
    virtual void send_float_to(uint8_t node, SensorId sensor, ValueType type, float value,
                               uint8_t decimals) = 0;

    virtual void request(SensorId sensor, ValueType type) = 0;

    /// Waits while still servicing the transport.
    virtual void wait(uint32_t ms) = 0;

    /// Waits for an inbound C_SET of the given type, or until the timeout.
    virtual void wait_for_set(uint32_t ms, ValueType type) = 0;

protected:
    ~IBus() = default;
};

} // namespace gw
