/**
 * @file mysensors_bus.h
 * @brief The one place that knows about MySensors.
 *
 * Include this from the .ino only. <MySensors.h> drags the entire transport
 * implementation in as source, so it may appear in exactly one translation
 * unit; that constraint is the reason IBus exists in the first place.
 *
 * One reusable MyMessage is shared by every send. The pre-refactor sketch kept
 * ten of them alive as globals (six in main.ino, plus five in the shutter and
 * four in the dimmer, several of which were never sent in a given build). At
 * 33 bytes each on a 2 KB part that was worth reclaiming.
 */
#pragma once

#include "../hal/bus.h"

namespace gw {

namespace detail {

inline uint8_t to_data_type(ValueType type)
{
    switch (type) {
    case ValueType::Status:
        return V_STATUS;
    case ValueType::Percentage:
        return V_PERCENTAGE;
    case ValueType::Watt:
        return V_WATT;
    case ValueType::Temperature:
        return V_TEMP;
    case ValueType::Humidity:
        return V_HUM;
    case ValueType::Text:
        return V_TEXT;
    case ValueType::Up:
        return V_UP;
    case ValueType::Down:
        return V_DOWN;
    case ValueType::Stop:
        return V_STOP;
    case ValueType::Rgb:
        return V_RGB;
    case ValueType::Rgbw:
        return V_RGBW;
    }
    return V_STATUS;
}

inline mysensors_sensor_t to_sensor_type(SensorClass type)
{
    switch (type) {
    case SensorClass::Binary:
        return S_BINARY;
    case SensorClass::Cover:
        return S_COVER;
    case SensorClass::Dimmer:
        return S_DIMMER;
    case SensorClass::RgbLight:
        return S_RGB_LIGHT;
    case SensorClass::RgbwLight:
        return S_RGBW_LIGHT;
    case SensorClass::Power:
        return S_POWER;
    case SensorClass::Temperature:
        return S_TEMP;
    case SensorClass::Humidity:
        return S_HUM;
    case SensorClass::Info:
        return S_INFO;
    }
    return S_BINARY;
}

/// Recovers a ValueType from a raw V_* code. Unknown codes are reported as
/// Status; on_message() then discards them because no child claims them.
inline bool from_data_type(uint8_t raw, ValueType& out)
{
    switch (raw) {
    case V_STATUS:
        out = ValueType::Status;
        return true;
    case V_PERCENTAGE:
        out = ValueType::Percentage;
        return true;
    case V_WATT:
        out = ValueType::Watt;
        return true;
    case V_TEMP:
        out = ValueType::Temperature;
        return true;
    case V_HUM:
        out = ValueType::Humidity;
        return true;
    case V_TEXT:
        out = ValueType::Text;
        return true;
    case V_UP:
        out = ValueType::Up;
        return true;
    case V_DOWN:
        out = ValueType::Down;
        return true;
    case V_STOP:
        out = ValueType::Stop;
        return true;
    case V_RGB:
        out = ValueType::Rgb;
        return true;
    case V_RGBW:
        out = ValueType::Rgbw;
        return true;
    default:
        return false;
    }
}

} // namespace detail

/// Translates a received MyMessage into the domain representation.
inline bool decode(const MyMessage& in, InboundMessage& out)
{
    ValueType type = ValueType::Status;
    if (!detail::from_data_type(in.type, type)) {
        return false;
    }
    out.sensor = in.sensor;
    out.type = type;
    out.boolean = in.getBool();
    // atoi() over the raw buffer, which is what the original used for
    // V_PERCENTAGE. getLong() would decode a P_BYTE payload differently.
    out.numeric = atoi(in.data);
    out.text = in.getString();
    return true;
}

class MySensorsBus final : public IBus {
public:
    void send_sketch_info(TextRef name, TextRef version) override
    {
        sendSketchInfo(name, version);
    }

    bool present(SensorId sensor, SensorClass type, TextRef name) override
    {
        return ::present(sensor, detail::to_sensor_type(type), name);
    }

    void send_bool(SensorId sensor, ValueType type, bool value) override
    {
        ::send(prepare(sensor, type).set(value));
    }

    void send_uint(SensorId sensor, ValueType type, uint32_t value) override
    {
        ::send(prepare(sensor, type).set(value));
    }

    void send_float(SensorId sensor, ValueType type, float value, uint8_t decimals) override
    {
        ::send(prepare(sensor, type).set(value, decimals));
    }

    void send_text(SensorId sensor, ValueType type, const char* value) override
    {
        ::send(prepare(sensor, type).set(value));
    }

    void send_literal(SensorId sensor, ValueType type, TextRef value) override
    {
        ::send(prepare(sensor, type).set(value));
    }

    void send_float_to(uint8_t node, SensorId sensor, ValueType type, float value,
                       uint8_t decimals) override
    {
        MyMessage& msg = prepare(sensor, type);
        msg.setDestination(node);
        ::send(msg.set(value, decimals));
        msg.setDestination(0); // never leak the destination into the next send
    }

    void request(SensorId sensor, ValueType type) override
    {
        ::request(sensor, detail::to_data_type(type));
    }

    void wait(uint32_t ms) override { ::wait(ms); }

    void wait_for_set(uint32_t ms, ValueType type) override
    {
        ::wait(ms, C_SET, detail::to_data_type(type));
    }

private:
    MyMessage& prepare(SensorId sensor, ValueType type)
    {
        msg_.setSensor(sensor);
        msg_.setType(detail::to_data_type(type));
        return msg_;
    }

    MyMessage msg_;
};

} // namespace gw
