/**
 * @file avr_sensors.h
 * @brief Analog sensor adapters over GoWired-lib's ADC sampling.
 *
 * GoWired-lib's PowerSensor and AnalogTemp are kept as the low-level drivers:
 * their sampling loops are hardware specific and field proven, and this sketch
 * has no business reimplementing them. Only the *decisions* they also happened
 * to make -- CalculatePower(), ElectricalStatus(), ThermalStatus() -- were
 * moved out, into domain/monitors.h where they can be tested.
 */
#pragma once

#include "../domain/config.h"
#include "../hal/sensors.h"

#include <core/AnalogTemp.h>
#include <core/PowerSensor.h>

namespace gw {

class AvrCurrentSensor final : public ICurrentSensor {
public:
    AvrCurrentSensor(Pin pin, const PowerTuning& tuning) : pin_(pin), tuning_(tuning) {}

    void begin(float vcc_mv) override
    {
        sensor_.SetValues(pin_, tuning_.mv_per_amp, tuning_.receiver_voltage,
                          tuning_.max_current_a, tuning_.measuring_time_ms, vcc_mv);
    }

    float measure_ac(float vcc_mv) override { return sensor_.MeasureAC(vcc_mv); }
    float measure_dc(float vcc_mv) override { return sensor_.MeasureDC(vcc_mv); }

private:
    Pin pin_;
    PowerTuning tuning_;
    PowerSensor sensor_;
};

class AvrInternalTemperature final : public ITemperatureSensor {
public:
    AvrInternalTemperature(Pin pin, const ThermalTuning& tuning)
        : sensor_(pin, tuning.max_temperature_c, tuning.mv_per_celsius, tuning.zero_voltage_mv)
    {
    }

    /// AnalogTemp configures its pin in its constructor.
    void begin() override {}

    float measure_celsius(float vcc_mv) override { return sensor_.MeasureT(vcc_mv); }

private:
    AnalogTemp sensor_;
};

} // namespace gw
