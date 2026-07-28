/**
 * @file avr_hal.h
 * @brief ATmega328P implementations of the HAL interfaces.
 *
 * Everything here is a pass-through. If a method in this file grows a
 * conditional, it probably belongs in the domain layer instead -- this is the
 * one part of the sketch that cannot be unit tested.
 */
#pragma once

#include "../hal/hal.h"

#include <Arduino.h>
#include <EEPROM.h>
#include <avr/wdt.h>

namespace gw {

class AvrGpio final : public IGpio {
public:
    void configure(Pin pin, PinMode mode) override
    {
        switch (mode) {
        case PinMode::Input:
            pinMode(pin, INPUT);
            break;
        case PinMode::InputPullup:
            pinMode(pin, INPUT_PULLUP);
            break;
        case PinMode::Output:
            pinMode(pin, OUTPUT);
            break;
        }
    }

    bool read(Pin pin) override { return digitalRead(pin) == HIGH; }

    void write(Pin pin, bool high) override { digitalWrite(pin, high ? HIGH : LOW); }
};

class AvrPwm final : public IPwm {
public:
    void write_duty(Pin pin, uint8_t duty) override { analogWrite(pin, duty); }
};

class AvrClock final : public IClock {
public:
    uint32_t now_ms() const override { return millis(); }
    void delay_ms(uint32_t ms) override { delay(ms); }
};

class AvrStore final : public IStore {
public:
    uint8_t read(uint16_t address) const override { return EEPROM.read(address); }

    /// EEPROM.update, not EEPROM.write: an unchanged byte costs no erase cycle.
    void write(uint16_t address, uint8_t value) override { EEPROM.update(address, value); }
};

class AvrWatchdog final : public IWatchdog {
public:
    void enable() override { wdt_enable(WDTO_8S); }
    void disable() override { wdt_disable(); }
    void pet() override { wdt_reset(); }
};

/// Measures Vcc by comparing the internal 1.1 V bandgap against AVcc, which
/// needs no external components and no spare pin.
class AvrVoltageReference final : public IVoltageReference {
public:
    float vcc_mv() override
    {
        ADMUX = _BV(REFS0) | _BV(MUX3) | _BV(MUX2) | _BV(MUX1);
        delay(2); // let the reference settle

        ADCSRA |= _BV(ADSC);
        while (bit_is_set(ADCSRA, ADSC)) {
        }

        uint16_t raw = ADCL;
        raw |= static_cast<uint16_t>(ADCH) << 8;
        if (raw == 0) {
            return 0.0f;
        }
        return 1126400.0f / static_cast<float>(raw);
    }
};

} // namespace gw
