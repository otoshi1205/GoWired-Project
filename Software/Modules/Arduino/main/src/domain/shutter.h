/**
 * @file shutter.h
 * @brief Roller-shutter position model and relay driving.
 *
 * Port of GoWired-lib's Shutters class. The position arithmetic and the
 * travel-time bookkeeping are pure and testable; the two relays and the
 * EEPROM are reached through HAL interfaces.
 *
 * All public methods that return a duration return *milliseconds*. The
 * original returned seconds and left each caller to multiply by 1000 (or by
 * 10, for the percentage case), which is where the magic numbers in the old
 * roller_shutter.cpp came from.
 */
#pragma once

#include "../hal/hal.h"
#include "config.h"

namespace gw {

enum class ShutterMotion : uint8_t {
    Up = 0,
    Down = 1,
    Stopped = 2,
};

struct ShutterPins {
    Pin up = kNoPin;
    Pin down = kNoPin;
    bool off_level = false;
};

class Shutter {
public:
    Shutter(IGpio& gpio, IClock& clock, IStore& store, const ShutterPins& pins,
            const StoreLayout& layout);

    /// Reads persisted travel times and position; falls back to the configured
    /// defaults when the EEPROM is blank (0xFF).
    void begin(uint8_t default_up_s, uint8_t default_down_s);

    bool calibrated() const { return calibrated_; }
    uint8_t position() const { return position_; }
    ShutterMotion motion() const { return motion_; }
    uint8_t up_time_s() const { return up_time_s_; }
    uint8_t down_time_s() const { return down_time_s_; }

    /// Stores new travel times and persists them.
    void set_travel_times(uint8_t up_s, uint8_t down_s);

    // -- command entry points; each sets the pending motion and returns the
    //    time the shutter should travel for, in milliseconds ------------------

    /// Direct V_UP / V_DOWN / V_STOP command from the controller.
    uint32_t request(ShutterMotion motion);

    /// V_PERCENTAGE command. 0 is fully open, 100 fully closed.
    uint32_t request_position(int percent);

    /// Wall-switch press. @param button 0 = up, 1 = down. Pressing the button
    /// for the direction already in progress stops the shutter.
    uint32_t request_button(uint8_t button);

    ShutterMotion pending() const { return pending_; }
    void set_pending(ShutterMotion motion) { pending_ = motion; }

    /// Drives the relays to match the pending motion, breaking before making.
    void apply();

    /// Integrates a completed movement into the stored position.
    void advance(ShutterMotion direction, uint32_t elapsed_ms);

    void set_position(uint8_t percent);
    void persist_position();

private:
    uint8_t travel_time_s(ShutterMotion motion) const;

    IGpio& gpio_;
    IClock& clock_;
    IStore& store_;
    ShutterPins pins_;
    StoreLayout layout_;

    uint8_t up_time_s_ = 0;
    uint8_t down_time_s_ = 0;
    uint8_t position_ = 0;
    bool calibrated_ = false;
    ShutterMotion motion_ = ShutterMotion::Stopped;
    ShutterMotion pending_ = ShutterMotion::Stopped;
};

} // namespace gw
