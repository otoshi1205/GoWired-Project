/**
 * @file module.h
 * @brief The whole node, minus the hardware.
 *
 * This is what used to be setup(), presentation(), InitConfirmation(),
 * receive(), UpdateIO(), PSUpdate(), ETUpdate() and loop() in main.ino,
 * written once against IDevice instead of six times behind #ifdefs.
 */
#pragma once

#include "../hal/bus.h"
#include "../hal/sensors.h"
#include "config.h"
#include "device.h"
#include "input_bank.h"
#include "monitors.h"

namespace gw {

/// The current sensors, and the child ids their readings are published under.
struct PowerChannels {
    ICurrentSensor* sensor[4] = {nullptr, nullptr, nullptr, nullptr};
    SensorId id[4] = {kNoSensor, kNoSensor, kNoSensor, kNoSensor};
    uint8_t count = 0;
};

/// Optional peripherals. Null means "this board does not have one".
struct Peripherals {
    PowerChannels power;
    ITemperatureSensor* internal_temperature = nullptr;
    IHygrometer* external_probe = nullptr;
};

/// Text commands accepted on the configuration child.
struct ConfigCommands {
    const char* calibrate = "cmd1";
    const char* reserved = "cmd2";
    const char* watchdog_test = "cmd3";
    const char* clear_store = "cmd4";
};

class Module {
public:
    Module(IDevice& device, InputBank& inputs, IBus& bus, IClock& clock, IStore& store,
           IWatchdog& watchdog, IVoltageReference& vref, const Peripherals& peripherals,
           const Features& features, const Timing& timing, const PowerTuning& power,
           const ThermalTuning& thermal, const StoreLayout& layout);

    /// Configures every pin. Call from Arduino setup().
    void begin();

    /// Announces every child to the controller. Call from presentation().
    void present(TextRef sketch_name, TextRef sketch_version);

    /// Call from receive().
    void on_message(const InboundMessage& msg);

    /// One pass of the main loop.
    void loop();

    // -- observability, used by the tests ------------------------------------
    bool initial_state_sent() const { return initial_state_sent_; }
    const SafetyState& safety() const { return safety_; }

private:
    void send_initial_state();
    void update_power();
    void enforce_overcurrent_limits();
    void update_thermal();
    void report_interval_sensors(float vcc_mv);
    void read_external_probe();
    void handle_config_command(const char* payload);

    IDevice& device_;
    InputBank& inputs_;
    IBus& bus_;
    IClock& clock_;
    IStore& store_;
    IWatchdog& watchdog_;
    IVoltageReference& vref_;
    Peripherals peripherals_;
    Features features_;
    Timing timing_;
    ConfigCommands commands_;
    StoreLayout layout_;

    PowerMonitor power_monitor_;
    ThermalMonitor thermal_monitor_;
    LatchedFault overcurrent_;
    LatchedFault thermal_;

    SafetyState safety_;
    /// Deadband state is per channel; FOUR_RELAY reports four independent loads.
    float last_reported_current_[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    float latest_current_ = 0.0f;

    IHygrometer::Status external_status_ = IHygrometer::Status::Uninitialised;

    bool initial_state_sent_ = false;
    bool report_now_ = false;
    uint32_t last_report_ms_ = 0;
    float vcc_mv_ = 0.0f;
};

} // namespace gw
