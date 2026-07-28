#include "dimmer.h"

namespace gw {

namespace {

bool hex_nibble(char c, uint8_t& out)
{
    if (c >= '0' && c <= '9') {
        out = static_cast<uint8_t>(c - '0');
        return true;
    }
    if (c >= 'a' && c <= 'f') {
        out = static_cast<uint8_t>(c - 'a' + 10);
        return true;
    }
    if (c >= 'A' && c <= 'F') {
        out = static_cast<uint8_t>(c - 'A' + 10);
        return true;
    }
    return false;
}

bool hex_byte(const char* s, uint8_t& out)
{
    uint8_t hi = 0;
    uint8_t lo = 0;
    if (!hex_nibble(s[0], hi) || !hex_nibble(s[1], lo)) {
        return false;
    }
    out = static_cast<uint8_t>((hi << 4) | lo);
    return true;
}

uint8_t length_of(const char* s)
{
    uint8_t n = 0;
    while (s != nullptr && s[n] != '\0' && n < 255) {
        ++n;
    }
    return n;
}

} // namespace

Dimmer::Dimmer(IPwm& pwm, IClock& clock, const Pin (&pins)[kMaxDimmerChannels], uint8_t channels,
               const DimmerTuning& tuning)
    : pwm_(pwm), clock_(clock), channels_(channels > kMaxDimmerChannels ? kMaxDimmerChannels
                                                                       : channels),
      tuning_(tuning)
{
    for (uint8_t i = 0; i < kMaxDimmerChannels; ++i) {
        pins_[i] = pins[i];
    }
}

void Dimmer::begin()
{
    for (uint8_t i = 0; i < channels_; ++i) {
        pwm_.write_duty(pins_[i], 0);
    }
    on_ = false;
}

uint8_t Dimmer::channel_value(uint8_t channel) const
{
    return channel < kMaxDimmerChannels ? values_[channel] : 0;
}

void Dimmer::write_outputs()
{
    for (uint8_t i = 0; i < channels_; ++i) {
        const uint16_t scaled =
            static_cast<uint16_t>(static_cast<uint32_t>(level_) * values_[i] / 100u);
        pwm_.write_duty(pins_[i], static_cast<uint8_t>(scaled > 255 ? 255 : scaled));
    }
}

bool Dimmer::step_towards(uint8_t& current, uint8_t target, uint8_t step)
{
    if (current == target) {
        return false;
    }
    const uint8_t effective = step == 0 ? 1 : step;
    if (current < target) {
        const uint8_t room = static_cast<uint8_t>(target - current);
        current = static_cast<uint8_t>(current + (effective < room ? effective : room));
    } else {
        const uint8_t room = static_cast<uint8_t>(current - target);
        current = static_cast<uint8_t>(current - (effective < room ? effective : room));
    }
    return true;
}

void Dimmer::set_target_level(uint8_t percent)
{
    target_level_ = percent > 100 ? 100 : percent;
}

void Dimmer::bump_level(uint8_t step)
{
    const uint16_t next = static_cast<uint16_t>(target_level_) + step;
    target_level_ = next > 100 ? step : static_cast<uint8_t>(next);
}

void Dimmer::set_on(bool on)
{
    if (on == on_) {
        return;
    }
    on_ = on;

    // The requested brightness must survive an off/on cycle, so it is restored
    // after the ramp rather than being used as the ramp target directly.
    const uint8_t requested = target_level_;

    if (on) {
        level_ = 0;
        target_level_ = requested;
        // Colours are unknown at switch-on; adopt the targets so the ramp has
        // something to scale.
        for (uint8_t i = 0; i < channels_; ++i) {
            if (values_[i] == 0) {
                values_[i] = target_values_[i];
            }
        }
        update();
    } else {
        target_level_ = 0;
        update();
        level_ = requested; // remember brightness for the next switch-on
    }

    target_level_ = requested;
}

bool Dimmer::set_colors_from_hex(const char* hex)
{
    if (hex == nullptr) {
        return false;
    }

    const char* p = hex;
    uint8_t len = length_of(p);
    if (len > 0 && p[0] == '#') {
        ++p;
        --len;
    }
    if (len != 6 && len != 8) {
        return false;
    }

    const uint8_t available = static_cast<uint8_t>(len / 2);
    uint8_t parsed[kMaxDimmerChannels] = {0, 0, 0, 0};
    for (uint8_t i = 0; i < available && i < kMaxDimmerChannels; ++i) {
        if (!hex_byte(p + 2 * i, parsed[i])) {
            return false; // reject atomically; do not half-apply a bad payload
        }
    }

    for (uint8_t i = 0; i < channels_ && i < available; ++i) {
        target_values_[i] = parsed[i];
    }
    return true;
}

void Dimmer::update()
{
    if (!on_) {
        return;
    }

    uint32_t next_step_at = clock_.now_ms();

    for (;;) {
        const uint32_t now = clock_.now_ms();
        if (now < next_step_at) {
            // millis() rollover: resynchronise rather than stalling for 49 days.
            next_step_at = now;
            continue;
        }
        if (now - next_step_at < tuning_.interval_ms) {
            continue;
        }

        bool moved = step_towards(level_, target_level_, tuning_.step);
        for (uint8_t i = 0; i < channels_; ++i) {
            moved = step_towards(values_[i], target_values_[i], tuning_.step) || moved;
        }

        write_outputs();
        next_step_at = clock_.now_ms();

        if (!moved) {
            return;
        }
    }
}

} // namespace gw
