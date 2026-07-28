/**
 * @file dimmer_device.h
 * @brief Covers DIMMER, RGB and RGBW.
 *
 * The three used to be three classes (ActiveDimmer / RgbDimmer / RgbwDimmer)
 * that differed only in channel count, the S_* class they present as, and
 * whether they advertise V_RGB or V_RGBW. That is data, not behaviour, so they
 * are one class parameterised by ColorModel.
 */
#pragma once

#include "device.h"
#include "dimmer.h"
#include "input.h"

namespace gw {

class DimmerDevice final : public IDevice {
public:
    struct Spec {
        ColorModel model = ColorModel::White;
        Pin led_pins[kMaxDimmerChannels] = {kNoPin, kNoPin, kNoPin, kNoPin};
        Pin button_pins[2] = {kNoPin, kNoPin};
    };

    DimmerDevice(IPwm& pwm, IGpio& gpio, IClock& clock, const Spec& spec,
                 const DimmerTuning& tuning, const ButtonTiming& button_timing,
                 bool special_button_enabled);

    void begin() override;
    void present(IBus& bus, uint16_t presentation_delay_ms) override;
    void send_initial_state(IBus& bus, uint16_t echo_timeout_ms) override;
    bool handle(const InboundMessage& msg, IBus& bus, const SafetyState& safety) override;
    void poll_buttons(IBus& bus, const SafetyState& safety) override;
    void tick(IBus& bus, float current_a) override;
    void shed_load(IBus& bus, const SafetyState& safety) override;

    uint8_t power_channel_count() const override { return 1; }
    bool draws_current(uint8_t channel) const override;
    bool uses_dc_measurement() const override { return true; }

    const Dimmer& dimmer() const { return dimmer_; }

private:
    /// V_RGB for a 3-channel strip, V_RGBW for 4, nothing for plain white.
    bool color_value_type(ValueType& out) const;

    Dimmer dimmer_;
    Button buttons_[2];
    Spec spec_;
    DimmerTuning tuning_;
    bool special_button_enabled_;
};

} // namespace gw
