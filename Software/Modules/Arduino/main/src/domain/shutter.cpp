#include "shutter.h"

namespace gw {

namespace {

/// Time the opposite relay needs to drop out before the other is energised.
constexpr uint32_t kDirectionInterlockMs = 50;

constexpr uint8_t kEepromBlank = 0xFF;

int clamp_percent(int value)
{
    if (value > 100) {
        return 100;
    }
    if (value < 0) {
        return 0;
    }
    return value;
}

} // namespace

Shutter::Shutter(IGpio& gpio, IClock& clock, IStore& store, const ShutterPins& pins,
                 const StoreLayout& layout)
    : gpio_(gpio), clock_(clock), store_(store), pins_(pins), layout_(layout)
{
}

void Shutter::begin(uint8_t default_up_s, uint8_t default_down_s)
{
    gpio_.configure(pins_.up, PinMode::Output);
    gpio_.write(pins_.up, pins_.off_level);
    gpio_.configure(pins_.down, PinMode::Output);
    gpio_.write(pins_.down, pins_.off_level);

    const uint8_t stored_down = store_.read(layout_.shutter_down_time);
    const uint8_t stored_up = store_.read(layout_.shutter_up_time);

    if (stored_up != kEepromBlank && stored_down != kEepromBlank) {
        calibrated_ = true;
        up_time_s_ = stored_up;
        down_time_s_ = stored_down;
        position_ = clamp_percent(store_.read(layout_.shutter_position));
    } else {
        // Uncalibrated: adopt the manually configured times so the shutter is
        // usable, but stay flagged so a calibration run can still be asked for.
        calibrated_ = false;
        up_time_s_ = default_up_s;
        down_time_s_ = default_down_s;
        position_ = 0;
    }

    motion_ = ShutterMotion::Stopped;
    pending_ = ShutterMotion::Stopped;
}

void Shutter::set_travel_times(uint8_t up_s, uint8_t down_s)
{
    up_time_s_ = up_s;
    down_time_s_ = down_s;
    calibrated_ = true;
    store_.write(layout_.shutter_up_time, up_s);
    store_.write(layout_.shutter_down_time, down_s);
}

uint8_t Shutter::travel_time_s(ShutterMotion motion) const
{
    switch (motion) {
    case ShutterMotion::Up:
        return up_time_s_;
    case ShutterMotion::Down:
        return down_time_s_;
    case ShutterMotion::Stopped:
        break;
    }
    return 0;
}

uint32_t Shutter::request(ShutterMotion motion)
{
    pending_ = motion;
    if (motion == ShutterMotion::Stopped) {
        return 0;
    }
    return static_cast<uint32_t>(travel_time_s(motion)) * 1000u;
}

uint32_t Shutter::request_position(int percent)
{
    const int target = clamp_percent(percent);
    const int range = target - static_cast<int>(position_);
    if (range == 0) {
        pending_ = ShutterMotion::Stopped;
        return 0;
    }

    // Positive range means further closed, i.e. downward.
    const ShutterMotion direction = range > 0 ? ShutterMotion::Down : ShutterMotion::Up;
    pending_ = direction;

    const int magnitude = range > 0 ? range : -range;
    // travel_time_s is a full 0..100 traverse, so scale by percent/100. The
    // *10 (rather than *1000/100) is the original arithmetic, kept verbatim.
    return static_cast<uint32_t>(travel_time_s(direction)) * static_cast<uint32_t>(magnitude) * 10u;
}

uint32_t Shutter::request_button(uint8_t button)
{
    const ShutterMotion requested =
        button == 0 ? ShutterMotion::Up : ShutterMotion::Down;

    // Pressing the direction that is already running means "stop".
    if (motion_ != ShutterMotion::Stopped && motion_ == requested) {
        pending_ = ShutterMotion::Stopped;
        return 0;
    }

    pending_ = requested;
    return static_cast<uint32_t>(travel_time_s(requested)) * 1000u;
}

void Shutter::apply()
{
    switch (pending_) {
    case ShutterMotion::Stopped:
        gpio_.write(pins_.up, pins_.off_level);
        gpio_.write(pins_.down, pins_.off_level);
        break;

    case ShutterMotion::Up:
        // Break before make: never energise both directions at once.
        if (gpio_.read(pins_.down) != pins_.off_level) {
            gpio_.write(pins_.down, pins_.off_level);
            clock_.delay_ms(kDirectionInterlockMs);
        }
        gpio_.write(pins_.up, !pins_.off_level);
        break;

    case ShutterMotion::Down:
        if (gpio_.read(pins_.up) != pins_.off_level) {
            gpio_.write(pins_.up, pins_.off_level);
            clock_.delay_ms(kDirectionInterlockMs);
        }
        gpio_.write(pins_.down, !pins_.off_level);
        break;
    }

    motion_ = pending_;
}

void Shutter::advance(ShutterMotion direction, uint32_t elapsed_ms)
{
    const uint8_t full_travel_s = travel_time_s(direction);
    if (full_travel_s == 0) {
        return; // not calibrated; nothing sensible to integrate
    }

    // elapsed_ms / (full_travel_s * 1000) * 100 == elapsed_ms / full_travel_s / 10
    const float change = static_cast<float>(elapsed_ms) / static_cast<float>(full_travel_s) / 10.0f;

    int next = static_cast<int>(position_);
    next += direction == ShutterMotion::Down ? static_cast<int>(change) : -static_cast<int>(change);
    position_ = static_cast<uint8_t>(clamp_percent(next));
}

void Shutter::set_position(uint8_t percent)
{
    position_ = static_cast<uint8_t>(clamp_percent(percent));
}

void Shutter::persist_position()
{
    store_.write(layout_.shutter_position, position_);
}

} // namespace gw
