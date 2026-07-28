/**
 * @file input_bank.h
 * @brief The generic digital inputs (INPUT_1..4), independent of device kind.
 *
 * Replaces four copy-pasted #ifdef blocks in each of setup(), presentation(),
 * InitConfirmation() and UpdateIO(). Also removes the constraint that made
 * disabling INPUT_3 or INPUT_4 a compile error: NUMBER_OF_INPUTS was a sum of
 * possibly-undefined PULLUP_n macros used as a C++ array bound, so an
 * undefined macro leaked through as an identifier.
 */
#pragma once

#include "../hal/bus.h"
#include "config.h"
#include "input.h"

namespace gw {

class InputBank {
public:
    InputBank(IGpio& gpio, IClock& clock, const InputPin (&pins)[kMaxInputs],
              uint8_t debounce_ms);

    void begin();
    void present(IBus& bus, uint16_t presentation_delay_ms);
    void send_initial_state(IBus& bus);

    /// Reports any input whose level changed.
    void poll(IBus& bus);

    /// How many slots are enabled. Enabled slots need not be contiguous.
    uint8_t enabled_count() const;

private:
    InputPin pins_[kMaxInputs];
    DigitalSensor sensors_[kMaxInputs];
};

} // namespace gw
