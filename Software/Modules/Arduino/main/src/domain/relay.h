/**
 * @file relay.h
 * @brief A single on/off output.
 */
#pragma once

#include "../hal/hal.h"

namespace gw {

class Relay {
public:
    /// @param off_level the pin level that de-energises the relay
    Relay(IGpio& gpio, Pin pin, bool off_level) : gpio_(gpio), pin_(pin), off_level_(off_level) {}

    void begin()
    {
        gpio_.configure(pin_, PinMode::Output);
        gpio_.write(pin_, off_level_);
        on_ = false;
    }

    void set(bool on)
    {
        gpio_.write(pin_, on ? !off_level_ : off_level_);
        on_ = on;
    }

    void toggle() { set(!on_); }

    bool is_on() const { return on_; }
    Pin pin() const { return pin_; }

private:
    IGpio& gpio_;
    Pin pin_;
    bool off_level_;
    bool on_ = false;
};

} // namespace gw
