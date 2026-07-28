#include "input.h"

namespace gw {

bool DebouncedInput::read()
{
    // Hardcoded overall timeout, carried over from CommonIO::_ReadDigital.
    constexpr uint32_t kSettleTimeoutMs = 255;

    bool active = false;
    bool previous = false;
    bool result = false;
    uint32_t start = clock_.now_ms();

    do {
        // An unmodified input idles high (pulled up), so the raw read is
        // inverted unless the caller asked for the opposite polarity.
        active = invert_ ? gpio_.read(pin_) : !gpio_.read(pin_);

        if (active && !previous) {
            start = clock_.now_ms();
        }

        if (clock_.now_ms() - start > debounce_ms_ && active) {
            result = true;
            break;
        }

        // Second test catches a millis() rollover mid-read.
        if (clock_.now_ms() - start > kSettleTimeoutMs || clock_.now_ms() < start) {
            break;
        }

        previous = active;
    } while (active);

    return result;
}

ButtonEvent Button::poll()
{
    bool active = false;
    bool toggled = false;
    bool first_pass = true;
    ButtonEvent event = ButtonEvent::None;
    uint32_t start = clock_.now_ms();

    do {
        active = input_.read();

        if (first_pass) {
            // Require a release before accepting another press. Without this a
            // held button would re-trigger on every main-loop iteration.
            if (!release_seen_) {
                release_seen_ = !active;
                break;
            }
            first_pass = false;
        }

        if (!toggled && active) {
            event = ButtonEvent::Toggle;
            toggled = true;
            release_seen_ = false;
        }

        if (clock_.now_ms() - start > longpress_ms_) {
            event = ButtonEvent::LongPress;
            break;
        }

        if (clock_.now_ms() < start) {
            start = clock_.now_ms(); // millis() rollover
        }
    } while (active);

    return event;
}

bool DigitalSensor::poll(bool& level)
{
    const bool reading = input_.read();
    if (reading == level_) {
        return false;
    }
    level_ = reading;
    level = reading;
    return true;
}

} // namespace gw
