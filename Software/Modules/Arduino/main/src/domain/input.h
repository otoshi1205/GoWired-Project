/**
 * @file input.h
 * @brief Debounced digital inputs: wall-switch buttons and generic sensors.
 *
 * This is a port of CommonIO::CheckInput() / CommonIO::_ReadDigital() from
 * GoWired-lib, with the pin access and the clock injected so the state
 * machines can be tested. The timing behaviour -- including the fact that a
 * button poll blocks for as long as the button is held -- is preserved
 * deliberately; see the note on Button::poll().
 */
#pragma once

#include "../hal/hal.h"

namespace gw {

/// A single debounced level read. Blocks up to 255 ms while the input settles.
class DebouncedInput {
public:
    DebouncedInput(IGpio& gpio, IClock& clock, Pin pin, bool invert, uint8_t debounce_ms)
        : gpio_(gpio), clock_(clock), pin_(pin), invert_(invert), debounce_ms_(debounce_ms)
    {
    }

    /// @return true once the input has read active continuously for the
    ///         debounce period, false if it settled inactive or timed out.
    bool read();

    Pin pin() const { return pin_; }

private:
    IGpio& gpio_;
    IClock& clock_;
    Pin pin_;
    bool invert_;
    uint8_t debounce_ms_;
};

enum class ButtonEvent : uint8_t {
    None,
    Toggle,   ///< short press: flip whatever this button controls
    LongPress ///< held past the longpress threshold
};

/// A momentary wall switch.
class Button {
public:
    Button(IGpio& gpio, IClock& clock, Pin pin, bool invert, uint16_t longpress_ms,
           uint8_t debounce_ms)
        : input_(gpio, clock, pin, invert, debounce_ms), gpio_(gpio), clock_(clock),
          longpress_ms_(longpress_ms)
    {
    }

    void begin() { gpio_.configure(input_.pin(), PinMode::InputPullup); }

    /// Samples the button and classifies the press.
    ///
    /// Blocking, exactly as the original: while the button is held this spins
    /// until either the longpress threshold elapses or the button is released.
    /// A press is not accepted until a release has been observed, so holding
    /// the button yields one LongPress rather than a stream of Toggles.
    ButtonEvent poll();

private:
    DebouncedInput input_;
    IGpio& gpio_;
    IClock& clock_;
    uint16_t longpress_ms_;
    /// Latches on release. Starts true so the first press after boot is
    /// ignored unless the button is already released -- matching CommonIO,
    /// which initialised _HighStateDetected to true.
    bool release_seen_ = true;
};

/// A generic on/off input reported to the controller as a binary child:
/// door/window contacts (pulled up) and motion sensors (floating).
class DigitalSensor {
public:
    DigitalSensor(IGpio& gpio, IClock& clock, Pin pin, bool invert, bool pullup,
                  uint8_t debounce_ms)
        : input_(gpio, clock, pin, invert, debounce_ms), gpio_(gpio), pullup_(pullup)
    {
    }

    void begin()
    {
        gpio_.configure(input_.pin(), pullup_ ? PinMode::InputPullup : PinMode::Input);
    }

    /// @param level receives the new level when the return value is true
    /// @return true when the level changed since the last poll
    bool poll(bool& level);

    bool level() const { return level_; }

private:
    DebouncedInput input_;
    IGpio& gpio_;
    bool pullup_;
    bool level_ = false;
};

} // namespace gw
