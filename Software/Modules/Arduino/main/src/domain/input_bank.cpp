#include "input_bank.h"

namespace gw {

namespace {

TextRef input_name(uint8_t index)
{
    switch (index) {
    case 0:
        return GW_TEXT("Input 1");
    case 1:
        return GW_TEXT("Input 2");
    case 2:
        return GW_TEXT("Input 3");
    default:
        return GW_TEXT("Input 4");
    }
}

} // namespace

InputBank::InputBank(IGpio& gpio, IClock& clock, const InputPin (&pins)[kMaxInputs],
                     uint8_t debounce_ms)
    : sensors_{{gpio, clock, pins[0].pin, pins[0].invert, pins[0].pullup, debounce_ms},
               {gpio, clock, pins[1].pin, pins[1].invert, pins[1].pullup, debounce_ms},
               {gpio, clock, pins[2].pin, pins[2].invert, pins[2].pullup, debounce_ms},
               {gpio, clock, pins[3].pin, pins[3].invert, pins[3].pullup, debounce_ms}}
{
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        pins_[i] = pins[i];
    }
}

uint8_t InputBank::enabled_count() const
{
    uint8_t n = 0;
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        if (pins_[i].enabled) {
            ++n;
        }
    }
    return n;
}

void InputBank::begin()
{
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        if (pins_[i].enabled) {
            sensors_[i].begin();
        }
    }
}

void InputBank::present(IBus& bus, uint16_t presentation_delay_ms)
{
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        if (!pins_[i].enabled) {
            continue;
        }
        bus.present(pins_[i].id, SensorClass::Binary, input_name(i));
        bus.wait(presentation_delay_ms);
    }
}

void InputBank::send_initial_state(IBus& bus)
{
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        if (pins_[i].enabled) {
            bus.send_bool(pins_[i].id, ValueType::Status, sensors_[i].level());
        }
    }
}

void InputBank::poll(IBus& bus)
{
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        if (!pins_[i].enabled) {
            continue;
        }
        bool level = false;
        if (sensors_[i].poll(level)) {
            bus.send_bool(pins_[i].id, ValueType::Status, level);
        }
    }
}

} // namespace gw
