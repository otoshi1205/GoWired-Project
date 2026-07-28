#include "relay_bank_device.h"

namespace gw {

namespace {

TextRef relay_name(uint8_t index)
{
    switch (index) {
    case 0:
        return GW_TEXT("Relay 1");
    case 1:
        return GW_TEXT("Relay 2");
    case 2:
        return GW_TEXT("Relay 3");
    default:
        return GW_TEXT("Relay 4");
    }
}

} // namespace

RelayBankDevice::RelayBankDevice(IGpio& gpio, IClock& clock, const RelayBankSpec& spec,
                                 const ButtonTiming& button_timing, bool special_button_enabled)
    : spec_(spec), special_button_enabled_(special_button_enabled),
      relays_{{gpio, spec.relay_pins[0], spec.off_level},
              {gpio, spec.relay_pins[1], spec.off_level},
              {gpio, spec.relay_pins[2], spec.off_level},
              {gpio, spec.relay_pins[3], spec.off_level}},
      buttons_{{gpio, clock, spec.button_pins[0], false, button_timing.longpress_ms,
                button_timing.debounce_ms},
               {gpio, clock, spec.button_pins[1], false, button_timing.longpress_ms,
                button_timing.debounce_ms}},
      button_count_(spec.button_count > 2 ? 2 : spec.button_count)
{
}

void RelayBankDevice::begin()
{
    for (uint8_t i = 0; i < spec_.relay_count; ++i) {
        relays_[i].begin();
    }
    for (uint8_t i = 0; i < button_count_; ++i) {
        buttons_[i].begin();
    }
}

void RelayBankDevice::present(IBus& bus, uint16_t presentation_delay_ms)
{
    for (uint8_t i = 0; i < spec_.relay_count; ++i) {
        bus.present(i, SensorClass::Binary, relay_name(i));
        bus.wait(presentation_delay_ms);
    }
}

void RelayBankDevice::send_initial_state(IBus& bus, uint16_t echo_timeout_ms)
{
    for (uint8_t i = 0; i < spec_.relay_count; ++i) {
        bus.send_bool(i, ValueType::Status, relays_[i].is_on());
        bus.request(i, ValueType::Status);
        bus.wait_for_set(echo_timeout_ms, ValueType::Status);
    }
}

bool RelayBankDevice::handle(const InboundMessage& msg, IBus& bus, const SafetyState& safety)
{
    (void)bus;
    if (msg.type != ValueType::Status || msg.sensor >= spec_.relay_count) {
        return false;
    }

    // Addressed to us either way, so the message is consumed even when a fault
    // means we refuse to act on it.
    if (!safety.blocks(msg.sensor)) {
        relays_[msg.sensor].set(msg.boolean);
    }
    return true;
}

void RelayBankDevice::poll_buttons(IBus& bus, const SafetyState& safety)
{
    for (uint8_t i = 0; i < button_count_; ++i) {
        switch (buttons_[i].poll()) {
        case ButtonEvent::Toggle:
            if (safety.blocks(i)) {
                break;
            }
            relays_[i].toggle();
            bus.send_bool(i, ValueType::Status, relays_[i].is_on());
            break;

        case ButtonEvent::LongPress:
            if (special_button_enabled_) {
                bus.send_bool(static_cast<SensorId>(ids::kSpecialButton1 + i), ValueType::Status,
                              true);
            }
            break;

        case ButtonEvent::None:
            break;
        }
    }
}

void RelayBankDevice::tick(IBus& bus, float current_a)
{
    (void)bus;
    (void)current_a;
}

void RelayBankDevice::shed_load(IBus& bus, const SafetyState& safety)
{
    for (uint8_t i = 0; i < spec_.relay_count; ++i) {
        // With one shared sensor every channel is implicated; with per-relay
        // sensing only the channel that actually tripped is.
        const uint8_t fault_channel = spec_.per_relay_power ? i : 0;
        if (!safety.blocks(fault_channel) || !relays_[i].is_on()) {
            continue;
        }
        relays_[i].set(false);
        bus.send_bool(i, ValueType::Status, false);
    }
}

bool RelayBankDevice::draws_current(uint8_t channel) const
{
    if (spec_.per_relay_power) {
        return channel < spec_.relay_count && relays_[channel].is_on();
    }
    // One shared sensor: sample whenever anything is energised.
    for (uint8_t i = 0; i < spec_.relay_count; ++i) {
        if (relays_[i].is_on()) {
            return true;
        }
    }
    return false;
}

bool RelayBankDevice::relay_on(uint8_t index) const
{
    return index < spec_.relay_count && relays_[index].is_on();
}

} // namespace gw
