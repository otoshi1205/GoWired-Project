/**
 * @file external_probe.h
 * @brief Optional external temperature/humidity probe.
 *
 * This file holds the one behavioural #ifdef left in the sketch, and it is not
 * avoidable in C++: selecting a probe means selecting which third-party header
 * to #include, and a header that is not installed cannot be included. The
 * conditional is confined to this file -- the rest of the sketch sees only
 * IHygrometer.
 *
 * Define one of GW_PROBE_SHT30 or GW_PROBE_DHT22 in Configuration.h to enable.
 */
#pragma once

#include "../hal/sensors.h"

#if defined(GW_PROBE_SHT30)

#include <SHTSensor.h>
#include <Wire.h>

namespace gw {

class Sht30Probe final : public IHygrometer {
public:
    void begin()
    {
        Wire.begin();
        sensor_.init();
        sensor_.setAccuracy(SHTSensor::SHT_ACCURACY_MEDIUM);
    }

    Reading read() override
    {
        Reading r;
        if (!sensor_.readSample()) {
            r.status = Status::ChecksumError;
            return r;
        }
        r.status = Status::Ok;
        r.temperature_c = sensor_.getTemperature();
        r.humidity_pct = sensor_.getHumidity();
        return r;
    }

private:
    SHTSensor sensor_;
};

using ExternalProbe = Sht30Probe;

} // namespace gw

#elif defined(GW_PROBE_DHT22)

#include <dht.h>

namespace gw {

class Dht22Probe final : public IHygrometer {
public:
    explicit Dht22Probe(Pin pin) : pin_(pin) {}

    void begin() { pinMode(pin_, INPUT); }

    Reading read() override
    {
        Reading r;
        switch (sensor_.read22(pin_)) {
        case DHTLIB_OK:
            r.status = Status::Ok;
            r.temperature_c = sensor_.temperature;
            r.humidity_pct = sensor_.humidity;
            break;
        case DHTLIB_ERROR_CHECKSUM:
            r.status = Status::ChecksumError;
            break;
        case DHTLIB_ERROR_TIMEOUT:
            r.status = Status::TimeoutError;
            break;
        default:
            r.status = Status::Uninitialised;
            break;
        }
        return r;
    }

private:
    Pin pin_;
    dht sensor_;
};

using ExternalProbe = Dht22Probe;

} // namespace gw

#endif
