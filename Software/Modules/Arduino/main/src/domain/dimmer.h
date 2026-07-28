/**
 * @file dimmer.h
 * @brief PWM dimmer with white / RGB / RGBW colour models.
 *
 * Port of GoWired-lib's Dimmer class. Two defects in the original are fixed
 * here and covered by tests:
 *
 *  - Hex colour parsing only accepted upper-case digits. `_StringHexToByte()`
 *    did `c -= 7` for anything above '9', which is correct for 'A'-'F' but
 *    off by 32 for 'a'-'f', so "ff0000" decoded to garbage. Controllers
 *    generally send lower case -- and init_confirmation() itself advertised
 *    "ffffff". Parsing is now case-insensitive and accepts an optional '#'.
 *
 *  - The ramp loops compared `current != target` while stepping by
 *    `DimmerTuning::step`, so any step size that did not exactly divide the
 *    distance overshot and spun forever until the watchdog fired. The final
 *    step is now clamped to the remaining distance.
 */
#pragma once

#include "../hal/hal.h"
#include "config.h"

namespace gw {

/// Maximum channels a dimmer can drive (R, G, B, W).
constexpr uint8_t kMaxDimmerChannels = 4;

class Dimmer {
public:
    Dimmer(IPwm& pwm, IClock& clock, const Pin (&pins)[kMaxDimmerChannels], uint8_t channels,
           const DimmerTuning& tuning);

    void begin();

    bool is_on() const { return on_; }
    /// Brightness the dimmer is ramping towards, 0..100.
    uint8_t target_level() const { return target_level_; }
    /// Brightness currently applied to the outputs, 0..100.
    uint8_t level() const { return level_; }
    uint8_t channel_value(uint8_t channel) const;

    /// Turns the dimmer on or off, ramping the brightness.
    void set_on(bool on);

    /// V_PERCENTAGE from the controller. Values above 100 are clamped.
    void set_target_level(uint8_t percent);

    /// Wall-switch brightness step. Wraps back to `step` once past 100.
    void bump_level(uint8_t step);

    /// Parses "RRGGBB", "RRGGBBWW", optionally '#'-prefixed, case-insensitive.
    /// @return false if the payload was not valid hex, leaving colours untouched
    bool set_colors_from_hex(const char* hex);

    /// Advances the brightness/colour ramp to the target. Blocking, as in the
    /// original: returns once the outputs have reached the requested values.
    void update();

private:
    void write_outputs();
    /// Steps `current` towards `target` by at most `step`. @return true if moved
    static bool step_towards(uint8_t& current, uint8_t target, uint8_t step);

    IPwm& pwm_;
    IClock& clock_;
    Pin pins_[kMaxDimmerChannels] = {kNoPin, kNoPin, kNoPin, kNoPin};
    uint8_t channels_;
    DimmerTuning tuning_;

    bool on_ = false;
    uint8_t level_ = 20;
    uint8_t target_level_ = 20;
    uint8_t values_[kMaxDimmerChannels] = {0, 0, 0, 0};
    uint8_t target_values_[kMaxDimmerChannels] = {255, 255, 255, 255};
};

} // namespace gw
