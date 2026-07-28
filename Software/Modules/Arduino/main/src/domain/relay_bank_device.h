/**
 * @file relay_bank_device.h
 * @brief Covers both DOUBLE_RELAY and FOUR_RELAY.
 *
 * The two used to be separate #ifdef blocks repeated across setup(),
 * presentation(), InitConfirmation(), receive(), UpdateIO() and loop(). They
 * differ only in relay count, whether wall switches are wired, and whether
 * there is one shared current sensor or one per relay -- so they are one class
 * with three parameters.
 */
#pragma once

#include "device.h"
#include "input.h"
#include "relay.h"

namespace gw {

constexpr uint8_t kMaxRelays = 4;

struct RelayBankSpec {
    uint8_t relay_count = 0;
    Pin relay_pins[kMaxRelays] = {kNoPin, kNoPin, kNoPin, kNoPin};

    /// Wall switches, paired one-to-one with the first `button_count` relays.
    /// FOUR_RELAY has none, which is also why its relays must not be polled as
    /// inputs -- the old UpdateIO() ran CheckInput() over them and debounced an
    /// uninitialised pin.
    uint8_t button_count = 0;
    Pin button_pins[2] = {kNoPin, kNoPin};

    bool off_level = false;
    /// FOUR_RELAY has an ACS712 per output; the others share one sensor.
    bool per_relay_power = false;
};

class RelayBankDevice final : public IDevice {
public:
    RelayBankDevice(IGpio& gpio, IClock& clock, const RelayBankSpec& spec,
                    const ButtonTiming& button_timing, bool special_button_enabled);

    void begin() override;
    void present(IBus& bus, uint16_t presentation_delay_ms) override;
    void send_initial_state(IBus& bus, uint16_t echo_timeout_ms) override;
    bool handle(const InboundMessage& msg, IBus& bus, const SafetyState& safety) override;
    void poll_buttons(IBus& bus, const SafetyState& safety) override;
    void tick(IBus& bus, float current_a) override;
    void shed_load(IBus& bus, const SafetyState& safety) override;

    uint8_t power_channel_count() const override { return spec_.per_relay_power ? 4 : 1; }
    bool draws_current(uint8_t channel) const override;

    /// Test/observability accessor.
    bool relay_on(uint8_t index) const;

private:
    RelayBankSpec spec_;
    bool special_button_enabled_;

    // Fixed-capacity storage: no allocator on this part, and only the first
    // `spec_.relay_count` entries are live.
    Relay relays_[kMaxRelays];
    Button buttons_[2];
    uint8_t button_count_;
};

} // namespace gw
