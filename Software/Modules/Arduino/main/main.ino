/*
 * GoWired is an open source project for WIRED home automation. It aims at making wired
 * home automation easy and affordable for every home automation enthusiast. GoWired provides
 * hardware, software, enclosures and instructions necessary to build your own bus communicating
 * smart home installation.
 *
 * GoWired is based on RS485 industrial communication standard. The software uses MySensors
 * communication protocol (http://www.mysensors.org).
 *
 * Created by feanor-anglin
 * Copyright (C) 2018-2022 feanor-anglin
 *
 * This program is free software; you can redistribute it and/or
 * modify it under the terms of the GNU General Public License
 * version 3 as published by the Free Software Foundation.
 *
 * ******************************
 * Source code for GoWired MCU working with 2SSR, RGBW & 4RelayDin shields.
 *
 * This sketch is only wiring. All behaviour lives in src/domain, which knows
 * nothing about Arduino or MySensors and is unit tested on the host -- see
 * test/README.md. src/platform holds the ATmega328P and MySensors adapters.
 *
 * NOTE: requires -std=gnu++17. See platform.local.txt in this folder.
 */

#include "Configuration.h"

#include <GoWired.h>

#include "src/domain/dimmer_device.h"
#include "src/domain/input_bank.h"
#include "src/domain/module.h"
#include "src/domain/relay_bank_device.h"
#include "src/domain/roller_shutter_device.h"
#include "src/platform/avr_hal.h"
#include "src/platform/avr_sensors.h"
#include "src/platform/external_probe.h"
#include "src/platform/mysensors_bus.h"

// ---------------------------------------------------------------------------
// Hardware
// ---------------------------------------------------------------------------

gw::AvrGpio Gpio;
gw::AvrPwm Pwm;
gw::AvrClock Clock;
gw::AvrStore Store;
gw::AvrWatchdog Watchdog;
gw::AvrVoltageReference Vref;
gw::MySensorsBus Bus;

// ---------------------------------------------------------------------------
// Device selection -- the ONE compile-time branch in the sketch.
//
// The six board variants are mutually exclusive, so only the selected one is
// named here. That keeps the unselected devices, their vtables and their
// dependencies out of the binary: the IDevice abstraction is paid for in
// source, not in flash.
//
// Note DOUBLE_RELAY/FOUR_RELAY share RelayBankDevice and DIMMER/RGB/RGBW share
// DimmerDevice -- six variants, three implementations.
// ---------------------------------------------------------------------------

#if GW_DEVICE == GW_DOUBLE_RELAY || GW_DEVICE == GW_FOUR_RELAY

constexpr gw::RelayBankSpec kDeviceSpec = {
    /* relay_count     */ gw::output_count(cfg::kDevice),
    /* relay_pins      */ {cfg::relay_pin(0), cfg::relay_pin(1), cfg::relay_pin(2),
                           cfg::relay_pin(3)},
    /* button_count    */ gw::button_count(cfg::kDevice),
    /* button_pins     */ {cfg::kButtonPin1, cfg::kButtonPin2},
    /* off_level       */ cfg::kRelayOffLevel,
    /* per_relay_power */ cfg::kDevice == gw::DeviceKind::FourRelay,
};

gw::RelayBankDevice Device(Gpio, Clock, kDeviceSpec, cfg::kButtons, cfg::kFeatures.special_button);

#elif GW_DEVICE == GW_ROLLER_SHUTTER

constexpr gw::RollerShutterDevice::Spec kDeviceSpec = {
    /* pins */ {cfg::relay_pin(0), cfg::relay_pin(1), cfg::kRelayOffLevel},
    /* button_pins           */ {cfg::kButtonPin1, cfg::kButtonPin2},
    /* current_floor         */ cfg::kShutter.calibration_current_floor,
    /* calibration_samples   */ cfg::kShutter.calibration_samples,
    /* default_up_time_s     */ cfg::kShutter.up_time_s,
    /* default_down_time_s   */ cfg::kShutter.down_time_s,
    /* current_sensing       */ cfg::kPowerSensor,
};

gw::RollerShutterDevice Device(Gpio, Clock, Store, kDeviceSpec, cfg::kStore, cfg::kButtons,
                              cfg::kFeatures.special_button);

#else // GW_DIMMER / GW_RGB / GW_RGBW

constexpr gw::DimmerDevice::Spec kDeviceSpec = {
    /* model     */ cfg::color_model(),
    /* led_pins  */ {cfg::led_pin(0), cfg::led_pin(1), cfg::led_pin(2), cfg::led_pin(3)},
    /* button_pins */ {cfg::kButtonPin1, cfg::kButtonPin2},
};

gw::DimmerDevice Device(Pwm, Gpio, Clock, kDeviceSpec, cfg::kDimmer, cfg::kButtons,
                        cfg::kFeatures.special_button);

#endif

// ---------------------------------------------------------------------------
// Generic digital inputs
// ---------------------------------------------------------------------------

gw::InputBank Inputs(Gpio, Clock, cfg::kInputs, cfg::kButtons.debounce_ms);

// ---------------------------------------------------------------------------
// Optional peripherals
// ---------------------------------------------------------------------------

// Only the channels this board actually has: four ACS712s on 4RelayDin, one
// everywhere else. Sizing this unconditionally at 4 wasted 66 bytes of SRAM.
#if GW_DEVICE == GW_FOUR_RELAY
gw::AvrCurrentSensor CurrentSensors[4] = {
    {cfg::current_sense_pin(0), cfg::kPower},
    {cfg::current_sense_pin(1), cfg::kPower},
    {cfg::current_sense_pin(2), cfg::kPower},
    {cfg::current_sense_pin(3), cfg::kPower},
};
#else
gw::AvrCurrentSensor CurrentSensors[1] = {
    {cfg::current_sense_pin(0), cfg::kPower},
};
#endif

gw::AvrInternalTemperature InternalTemperature(cfg::kInternalTempPin, cfg::kThermal);

#if defined(GW_PROBE_SHT30)
gw::ExternalProbe ExternalTemperatureProbe;
#elif defined(GW_PROBE_DHT22)
gw::ExternalProbe ExternalTemperatureProbe(cfg::kOneWire);
#endif

gw::Peripherals make_peripherals()
{
    gw::Peripherals p;

    if (cfg::kPowerSensor) {
        p.power.count = Device.power_channel_count();
        for (uint8_t ch = 0; ch < p.power.count; ++ch) {
            p.power.sensor[ch] = &CurrentSensors[ch];
            p.power.id[ch] = p.power.count > 1 ? gw::ids::kPowerPerRelay[ch] : gw::ids::kPower;
        }
    }

    if (cfg::kInternalTemperature) {
        p.internal_temperature = &InternalTemperature;
    }

#if defined(GW_PROBE_SHT30) || defined(GW_PROBE_DHT22)
    p.external_probe = &ExternalTemperatureProbe;
#endif

    return p;
}

gw::Module Node(Device, Inputs, Bus, Clock, Store, Watchdog, Vref, make_peripherals(),
                cfg::kFeatures, cfg::kTiming, cfg::kPower, cfg::kThermal, cfg::kStore);

// ---------------------------------------------------------------------------
// MySensors entry points
// ---------------------------------------------------------------------------

/// Runs before setup(); clears a watchdog reset left over from the last boot.
void before()
{
    if (cfg::kWatchdog) {
        wdt_reset();
        MCUSR = 0;
        wdt_disable();
    }
}

void setup()
{
#if defined(GW_PROBE_SHT30) || defined(GW_PROBE_DHT22)
    ExternalTemperatureProbe.begin();
#endif
    Node.begin();
}

void presentation()
{
    Node.present(GW_TEXT(SN), GW_TEXT(SV));
}

void receive(const MyMessage& message)
{
    gw::InboundMessage decoded;
    if (gw::decode(message, decoded)) {
        Node.on_message(decoded);
    }
}

void loop()
{
    Node.loop();
}

/// Required because the sketch has pure virtual functions and the AVR toolchain
/// does not provide this. Spinning lets the watchdog restart the node, which is
/// the only sane response to a call through an uninitialised vtable.
extern "C" void __cxa_pure_virtual()
{
    while (true) {
    }
}

/*
 * EOF
 */
