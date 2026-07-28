/**
 * @file device.h
 * @brief The polymorphic seam between Module and the six board variants.
 *
 * Module is written once against IDevice. Exactly one implementation is named
 * at the single compile-time selection point in main.ino, so the linker
 * discards the vtables and code of the variants that are not built -- the
 * abstraction is paid for in source, not in flash. The unit tests instantiate
 * all of them.
 */
#pragma once

#include "../hal/bus.h"
#include "../hal/sensors.h"
#include "config.h"

namespace gw {

/// Aggregate of the protective states that suppress load-switching commands.
struct SafetyState {
    bool thermal_fault = false;
    bool overcurrent[4] = {false, false, false, false};

    /// @return true when channel `channel` must not be energised
    bool blocks(uint8_t channel) const
    {
        return thermal_fault || overcurrent[channel < 4 ? channel : 0];
    }
};

class IDevice {
public:
    // -- lifecycle ----------------------------------------------------------
    virtual void begin() = 0;
    virtual void present(IBus& bus, uint16_t presentation_delay_ms) = 0;
    virtual void send_initial_state(IBus& bus, uint16_t echo_timeout_ms) = 0;

    // -- runtime ------------------------------------------------------------

    /// @return true if the message was addressed to this device and consumed
    virtual bool handle(const InboundMessage& msg, IBus& bus, const SafetyState& safety) = 0;

    /// Samples the wall switches and acts on them.
    virtual void poll_buttons(IBus& bus, const SafetyState& safety) = 0;

    /// Periodic update. @param current_a most recent current reading, in amps
    virtual void tick(IBus& bus, float current_a) = 0;

    /// De-energises the faulted load and tells the controller. Called on every
    /// iteration while a thermal or overcurrent fault is active, so it must be
    /// idempotent and must not re-send an unchanged state.
    ///
    /// `safety` is passed so a board with per-output current sensing can shed
    /// only the offending channel; a thermal fault blocks every channel and so
    /// sheds everything.
    virtual void shed_load(IBus& bus, const SafetyState& safety) = 0;

    // -- current measurement ------------------------------------------------
    //
    // Module owns the sensors and the reporting; the device only says which
    // channels are live and how they should be sampled. This is what lets one
    // loop cover both the single shared sensor and FOUR_RELAY's four.

    /// 1 for a shared sensor, 4 for one sensor per relay.
    virtual uint8_t power_channel_count() const = 0;

    /// True when `channel` may be drawing current and is worth sampling.
    virtual bool draws_current(uint8_t channel) const = 0;

    /// DC loads (LED strips) need averaged sampling, not peak-to-peak.
    virtual bool uses_dc_measurement() const { return false; }

    // -- optional ------------------------------------------------------------

    /// Runs a self-calibration cycle. @return false if unsupported.
    virtual bool calibrate(IBus& bus, ICurrentSensor& sensor, IWatchdog& watchdog, float vcc_mv)
    {
        (void)bus;
        (void)sensor;
        (void)watchdog;
        (void)vcc_mv;
        return false;
    }

    /// Called before the touch-field/other long blocking maintenance actions.
    virtual void prepare_for_maintenance() {}

protected:
    ~IDevice() = default;
};

} // namespace gw
