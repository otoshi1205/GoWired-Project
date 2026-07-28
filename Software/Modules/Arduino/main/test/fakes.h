/**
 * @file fakes.h
 * @brief Host implementations of every HAL interface, for the unit tests.
 */
#pragma once

#include "hal/bus.h"
#include "hal/hal.h"
#include "hal/sensors.h"

#include <cstring>
#include <deque>
#include <map>
#include <string>
#include <vector>

namespace gw {
namespace test {

class FakeGpio final : public IGpio {
public:
    /// Raw pin levels. Remember the domain treats a pulled-up input as active
    /// when it reads LOW, so `false` here means "button pressed".
    std::map<Pin, bool> level;
    std::map<Pin, PinMode> mode;

    /// Scripted read sequence per pin, consumed in order. Once exhausted the
    /// last value is returned forever, which keeps the blocking poll loops
    /// terminating.
    std::map<Pin, std::deque<bool>> script;

    struct Write {
        Pin pin;
        bool high;
    };
    std::vector<Write> writes;

    void configure(Pin pin, PinMode m) override { mode[pin] = m; }

    bool read(Pin pin) override
    {
        auto it = script.find(pin);
        if (it != script.end() && !it->second.empty()) {
            const bool v = it->second.front();
            if (it->second.size() > 1) {
                it->second.pop_front();
            }
            level[pin] = v;
            return v;
        }
        auto lv = level.find(pin);
        return lv == level.end() ? true : lv->second; // idle high (pulled up)
    }

    void write(Pin pin, bool high) override
    {
        level[pin] = high;
        writes.push_back({pin, high});
    }

    /// Scripts one complete short press on a pulled-up, non-inverted input.
    ///
    /// DebouncedInput needs two consecutive active samples to confirm the press,
    /// then Button samples once more and must see a release to classify it as a
    /// short press rather than a hold.
    void script_short_press(Pin pin) { script[pin] = {false, false, true}; }

    /// Scripts a hold: the input never releases, so Button reports LongPress.
    void script_hold(Pin pin) { script[pin] = {false}; }

    /// Scripts an idle (released) input.
    void script_idle(Pin pin) { script[pin] = {true}; }
};

class FakePwm final : public IPwm {
public:
    std::map<Pin, uint8_t> duty;
    std::vector<std::pair<Pin, uint8_t>> writes;

    void write_duty(Pin pin, uint8_t d) override
    {
        duty[pin] = d;
        writes.push_back({pin, d});
    }
};

class FakeClock final : public IClock {
public:
    /// Advanced by this much on every now_ms() call, so the blocking debounce
    /// and ramp loops in the domain make progress and terminate.
    uint32_t auto_advance_ms = 30;
    mutable uint32_t now = 0;
    std::vector<uint32_t> delays;

    uint32_t now_ms() const override
    {
        now += auto_advance_ms;
        return now;
    }

    void delay_ms(uint32_t ms) override
    {
        now += ms;
        delays.push_back(ms);
    }
};

class FakeStore final : public IStore {
public:
    /// Blank EEPROM reads as 0xFF, as on the real part.
    std::map<uint16_t, uint8_t> cells;
    uint32_t write_count = 0;

    uint8_t read(uint16_t address) const override
    {
        auto it = cells.find(address);
        return it == cells.end() ? 0xFF : it->second;
    }

    void write(uint16_t address, uint8_t value) override
    {
        // Mirrors EEPROM.update: an unchanged byte costs nothing.
        if (read(address) == value) {
            return;
        }
        cells[address] = value;
        ++write_count;
    }
};

class FakeWatchdog final : public IWatchdog {
public:
    bool enabled = false;
    uint32_t pets = 0;

    void enable() override { enabled = true; }
    void disable() override { enabled = false; }
    void pet() override { ++pets; }
};

class FakeVoltageReference final : public IVoltageReference {
public:
    float value = 5000.0f;
    uint32_t reads = 0;

    float vcc_mv() override
    {
        ++reads;
        return value;
    }
};

class FakeCurrentSensor final : public ICurrentSensor {
public:
    bool began = false;
    float ac = 0.0f;
    float dc = 0.0f;
    uint32_t ac_reads = 0;
    uint32_t dc_reads = 0;
    /// Optional scripted AC sequence, for the shutter calibration walk.
    std::deque<float> ac_script;

    void begin(float) override { began = true; }

    float measure_ac(float) override
    {
        ++ac_reads;
        if (!ac_script.empty()) {
            const float v = ac_script.front();
            if (ac_script.size() > 1) {
                ac_script.pop_front();
            }
            return v;
        }
        return ac;
    }

    float measure_dc(float) override
    {
        ++dc_reads;
        return dc;
    }
};

class FakeTemperatureSensor final : public ITemperatureSensor {
public:
    bool began = false;
    float celsius = 25.0f;

    void begin() override { began = true; }
    float measure_celsius(float) override { return celsius; }
};

class FakeHygrometer final : public IHygrometer {
public:
    Reading next;
    uint32_t reads = 0;

    Reading read() override
    {
        ++reads;
        return next;
    }
};

// ---------------------------------------------------------------------------
// Bus
// ---------------------------------------------------------------------------

/// One recorded outbound message.
struct Sent {
    enum class Kind { Bool, Uint, Float, Text, Literal };

    Kind kind = Kind::Bool;
    SensorId sensor = kNoSensor;
    ValueType type = ValueType::Status;
    bool boolean = false;
    uint32_t uint_value = 0;
    float float_value = 0.0f;
    uint8_t decimals = 0;
    std::string text;
    uint8_t node = 0; ///< non-zero only for send_float_to
};

struct Presented {
    SensorId sensor;
    SensorClass type;
    std::string name;
};

class FakeBus final : public IBus {
public:
    std::vector<Sent> sent;
    std::vector<Presented> presented;
    std::vector<std::pair<SensorId, ValueType>> requests;
    std::vector<uint32_t> waits;
    std::string sketch_name;
    std::string sketch_version;

    void send_sketch_info(TextRef name, TextRef version) override
    {
        sketch_name = name == nullptr ? "" : name;
        sketch_version = version == nullptr ? "" : version;
    }

    bool present(SensorId sensor, SensorClass type, TextRef name) override
    {
        presented.push_back({sensor, type, name == nullptr ? "" : name});
        return true;
    }

    void send_bool(SensorId sensor, ValueType type, bool value) override
    {
        Sent s;
        s.kind = Sent::Kind::Bool;
        s.sensor = sensor;
        s.type = type;
        s.boolean = value;
        sent.push_back(s);
    }

    void send_uint(SensorId sensor, ValueType type, uint32_t value) override
    {
        Sent s;
        s.kind = Sent::Kind::Uint;
        s.sensor = sensor;
        s.type = type;
        s.uint_value = value;
        sent.push_back(s);
    }

    void send_float(SensorId sensor, ValueType type, float value, uint8_t decimals) override
    {
        Sent s;
        s.kind = Sent::Kind::Float;
        s.sensor = sensor;
        s.type = type;
        s.float_value = value;
        s.decimals = decimals;
        sent.push_back(s);
    }

    void send_text(SensorId sensor, ValueType type, const char* value) override
    {
        Sent s;
        s.kind = Sent::Kind::Text;
        s.sensor = sensor;
        s.type = type;
        s.text = value == nullptr ? "" : value;
        sent.push_back(s);
    }

    void send_literal(SensorId sensor, ValueType type, TextRef value) override
    {
        Sent s;
        s.kind = Sent::Kind::Literal;
        s.sensor = sensor;
        s.type = type;
        s.text = value == nullptr ? "" : value;
        sent.push_back(s);
    }

    void send_float_to(uint8_t node, SensorId sensor, ValueType type, float value,
                       uint8_t decimals) override
    {
        Sent s;
        s.kind = Sent::Kind::Float;
        s.sensor = sensor;
        s.type = type;
        s.float_value = value;
        s.decimals = decimals;
        s.node = node;
        sent.push_back(s);
    }

    void request(SensorId sensor, ValueType type) override { requests.push_back({sensor, type}); }

    void wait(uint32_t ms) override { waits.push_back(ms); }

    void wait_for_set(uint32_t ms, ValueType) override { waits.push_back(ms); }

    // -- helpers ------------------------------------------------------------

    void clear()
    {
        sent.clear();
        presented.clear();
        requests.clear();
        waits.clear();
    }

    /// All messages matching a child and value type, in order.
    std::vector<Sent> matching(SensorId sensor, ValueType type) const
    {
        std::vector<Sent> out;
        for (const Sent& s : sent) {
            if (s.sensor == sensor && s.type == type) {
                out.push_back(s);
            }
        }
        return out;
    }

    size_t count(SensorId sensor, ValueType type) const { return matching(sensor, type).size(); }

    bool was_presented(SensorId sensor) const
    {
        for (const Presented& p : presented) {
            if (p.sensor == sensor) {
                return true;
            }
        }
        return false;
    }
};

/// Builds an inbound message the way the MySensors adapter would.
inline InboundMessage inbound(SensorId sensor, ValueType type, bool boolean = false,
                              long numeric = 0, const char* text = "")
{
    InboundMessage m;
    m.sensor = sensor;
    m.type = type;
    m.boolean = boolean;
    m.numeric = numeric;
    m.text = text;
    return m;
}

} // namespace test
} // namespace gw
