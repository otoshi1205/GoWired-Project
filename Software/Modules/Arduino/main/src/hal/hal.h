/**
 * @file hal.h
 * @brief Hardware seams for the GoWired module.
 *
 * Everything in this header is an interface. No implementation here touches
 * <Arduino.h>, which is what lets the whole domain layer be unit tested on a
 * host compiler against the fakes in test/fakes.h.
 *
 * Destructors are deliberately protected and non-virtual: the module never
 * destroys anything through a base pointer, and omitting the virtual
 * destructor keeps the deleting-destructor slots (and operator delete) out of
 * the vtables. On a 32 KB part that is worth the small loss of generality.
 */
#pragma once

#include <stdint.h>

namespace gw {

using Pin = uint8_t;
using SensorId = uint8_t;

constexpr Pin kNoPin = 0xFF;
constexpr SensorId kNoSensor = 0xFF;

enum class PinMode : uint8_t { Input, InputPullup, Output };

/// Digital input/output.
class IGpio {
public:
    virtual void configure(Pin pin, PinMode mode) = 0;
    virtual bool read(Pin pin) = 0;
    virtual void write(Pin pin, bool high) = 0;

protected:
    ~IGpio() = default;
};

/// PWM output, used by the dimmer.
class IPwm {
public:
    /// @param duty 0 (off) .. 255 (full)
    virtual void write_duty(Pin pin, uint8_t duty) = 0;

protected:
    ~IPwm() = default;
};

class IClock {
public:
    virtual uint32_t now_ms() const = 0;

    /// Plain blocking delay. Does *not* service the transport; use
    /// IBus::wait() where incoming messages must still be processed. The two
    /// are not interchangeable and the distinction is load bearing.
    virtual void delay_ms(uint32_t ms) = 0;

protected:
    ~IClock() = default;
};

/// Byte-addressable non-volatile storage (EEPROM).
class IStore {
public:
    virtual uint8_t read(uint16_t address) const = 0;

    /// Implementations must skip the write when the value is unchanged --
    /// shutter position is persisted on every movement and the part is only
    /// rated for ~100k erase cycles.
    virtual void write(uint16_t address, uint8_t value) = 0;

protected:
    ~IStore() = default;
};

class IWatchdog {
public:
    virtual void enable() = 0;
    virtual void disable() = 0;
    virtual void pet() = 0;

protected:
    ~IWatchdog() = default;
};

/// Supply-voltage reference. ADC readings are ratiometric to Vcc, so the
/// measurement helpers below take it as an explicit parameter rather than
/// hiding a second ADC conversion inside every read.
class IVoltageReference {
public:
    /// @return measured Vcc in millivolts
    virtual float vcc_mv() = 0;

protected:
    ~IVoltageReference() = default;
};

} // namespace gw
