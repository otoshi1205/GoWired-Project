/**
 * @file roller_shutter_device.h
 * @brief ROLLER_SHUTTER variant: one cover child driven by two relays.
 */
#pragma once

#include "device.h"
#include "input.h"
#include "shutter.h"

namespace gw {

class RollerShutterDevice final : public IDevice {
public:
    struct Spec {
        ShutterPins pins;
        Pin button_pins[2] = {kNoPin, kNoPin};
        /// Amps below which the motor is considered to have reached an end stop.
        float current_floor = 0.2f;
        uint8_t calibration_samples = 1;
        uint8_t default_up_time_s = 21;
        uint8_t default_down_time_s = 20;
        /// End-stop detection needs a current sensor. Without one the shutter
        /// runs purely on the configured travel times.
        bool current_sensing = false;
    };

    RollerShutterDevice(IGpio& gpio, IClock& clock, IStore& store, const Spec& spec,
                        const StoreLayout& layout, const ButtonTiming& button_timing,
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

    bool calibrate(IBus& bus, ICurrentSensor& sensor, IWatchdog& watchdog, float vcc_mv) override;
    void prepare_for_maintenance() override;

    const Shutter& shutter() const { return shutter_; }

private:
    void start_movement(IBus& bus);
    void finish_movement(IBus& bus, uint32_t stopped_at_ms);

    Shutter shutter_;
    Button buttons_[2];
    IClock& clock_;
    Spec spec_;
    bool special_button_enabled_;

    uint32_t movement_time_ms_ = 0;
    uint32_t started_at_ms_ = 0;
    /// Direction to resume in after a reversal has braked to a stop.
    ShutterMotion resume_ = ShutterMotion::Stopped;
};

} // namespace gw
