/**
 * @file monitors.h
 * @brief Decisions taken on top of raw sensor readings.
 *
 * PowerSensor::CalculatePower/ElectricalStatus and AnalogTemp::ThermalStatus
 * used to live in GoWired-lib next to the ADC sampling code, which made them
 * untestable. The sampling stays in the library; the judgement moved here.
 */
#pragma once

#include "../hal/hal.h"

namespace gw {

/// Converts current to power and decides when a change is worth a message.
class PowerMonitor {
public:
    PowerMonitor(uint8_t max_current_a, uint8_t receiver_voltage, float cos_phi)
        : max_current_a_(max_current_a), receiver_voltage_(receiver_voltage), cos_phi_(cos_phi)
    {
    }

    bool over_limit(float amps) const { return amps > static_cast<float>(max_current_a_); }

    float power_w(float amps) const
    {
        return amps * static_cast<float>(receiver_voltage_) * cos_phi_;
    }

    /// Deadband so a noisy ADC does not flood the bus: absolute below 1 A,
    /// relative above it.
    ///
    /// Stateless in the previous reading so one monitor can serve FOUR_RELAY's
    /// four independent loads; the caller keeps the per-channel history.
    bool should_report(float amps, float last_reported) const;

private:
    uint8_t max_current_a_;
    uint8_t receiver_voltage_;
    float cos_phi_;
};

class ThermalMonitor {
public:
    explicit ThermalMonitor(uint8_t max_temperature_c) : max_temperature_c_(max_temperature_c) {}

    bool over_limit(float celsius) const
    {
        return celsius > static_cast<float>(max_temperature_c_);
    }

private:
    uint8_t max_temperature_c_;
};

/// A fault that must be reported to the controller once when it appears and
/// once when it clears.
///
/// The pre-refactor sketch handled the two faults inconsistently: the thermal
/// path was guarded by `&& !InformControllerTS` and so reported once, while the
/// overcurrent path had no such guard and re-sent the same status on every
/// iteration of loop() for as long as the fault lasted. Both now report once.
/// The protective action itself is still re-applied every iteration while the
/// fault is active -- only the message is suppressed.
class LatchedFault {
public:
    /// @return true when the controller needs to be told about a transition
    bool update(bool active)
    {
        if (active != reported_) {
            reported_ = active;
            active_ = active;
            return true;
        }
        active_ = active;
        return false;
    }

    bool active() const { return active_; }

    /// The controller wrote to the status child. Adopt its value and re-arm
    /// reporting so the next real transition is sent again.
    void override_from_controller(bool active)
    {
        active_ = active;
        reported_ = active;
    }

private:
    bool active_ = false;
    bool reported_ = false;
};

} // namespace gw
