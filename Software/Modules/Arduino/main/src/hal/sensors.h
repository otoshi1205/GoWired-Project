/**
 * @file sensors.h
 * @brief Analog measurement seams.
 *
 * These return raw physical quantities only. Everything judgemental --
 * comparing against a limit, deciding whether a change is worth reporting,
 * converting amps to watts -- lives in domain/monitors.h where it can be
 * tested.
 */
#pragma once

#include "hal.h"

namespace gw {

class ICurrentSensor {
public:
    /// Configures the pin and samples the quiescent ADC offset.
    virtual void begin(float vcc_mv) = 0;

    /// Peak-to-peak sampling, for mains loads. @return amps
    virtual float measure_ac(float vcc_mv) = 0;

    /// Averaged sampling, for DC loads such as LED strips. @return amps
    virtual float measure_dc(float vcc_mv) = 0;

protected:
    ~ICurrentSensor() = default;
};

class ITemperatureSensor {
public:
    virtual void begin() = 0;

    /// @return degrees Celsius
    virtual float measure_celsius(float vcc_mv) = 0;

protected:
    ~ITemperatureSensor() = default;
};

/// Optional external temperature + humidity probe (DHT22 / SHT30).
class IHygrometer {
public:
    enum class Status : uint8_t {
        Ok = 0,
        ChecksumError = 1,
        TimeoutError = 2,
        Uninitialised = 3,
    };

    struct Reading {
        Status status = Status::Uninitialised;
        float temperature_c = 0.0f;
        float humidity_pct = 0.0f;
    };

    virtual Reading read() = 0;

protected:
    ~IHygrometer() = default;
};

} // namespace gw
