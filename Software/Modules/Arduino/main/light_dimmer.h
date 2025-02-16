#pragma once

#include "custom_types.h"

#include <core/Dimmer.h>
#include <core/PowerSensor.h>
#include <core/MySensorsCore.h>

class DimmerBase {
public:
    virtual void setup(CommonIOPins& io) = 0;
    virtual bool present() const = 0;
    virtual void init_confirmation() = 0;
    virtual bool handle_msg(const MyMessage& message) = 0;
    virtual void update() = 0;
    virtual bool update_io(CommonIO& io_pin, size_t idx) = 0;
    virtual float measure_current(float Vcc, PowerSensor& power_sensor) = 0;
    virtual void alert() = 0;  
protected:
    DimmerBase() = default;
    virtual ~DimmerBase() = default;
};

class CommonDimmer : public DimmerBase {
public:
    void setup(CommonIOPins& io) override;
    void init_confirmation() override;
    bool handle_msg(const MyMessage& message) override;
    void update() override;
    bool update_io(CommonIO& io_pin, size_t idx) override;
    float measure_current(float Vcc, PowerSensor& power_sensor) override;
    void alert() override;
protected:
    CommonDimmer(int dimmer_id)
    : dimmer_id_(dimmer_id)
    , MsgRGB(dimmer_id_, V_RGB)
    , MsgRGBW(dimmer_id_, V_RGBW)
    , MsgPERCENTAGE(0, V_PERCENTAGE)
    , MsgSTATUS(0, V_STATUS) {}

    const uint16_t dimmer_id_;
    Dimmer dimmer;
    MyMessage MsgRGB;
    MyMessage MsgRGBW;
    MyMessage MsgPERCENTAGE;
    MyMessage MsgSTATUS;
};

class ActiveDimmer final : public CommonDimmer {
public:
    ActiveDimmer(int shutterId) : CommonDimmer(shutterId) {}
    void setup(CommonIOPins& io) override;
    bool present() const override;
};

class RgbDimmer final : public CommonDimmer {
public:
    RgbDimmer(int shutterId) : CommonDimmer(shutterId) {}
    void setup(CommonIOPins& io) override;
    bool present() const override;
    void init_confirmation() override;
};

class RgbwDimmer final : public CommonDimmer {
public:
    RgbwDimmer(int shutterId) : CommonDimmer(shutterId) {}
    void setup(CommonIOPins& io) override;
    bool present() const override;
    void init_confirmation() override;
};

class StubDimmer : public DimmerBase {
public:
    StubDimmer(int) {}
    void setup(CommonIOPins&) override {}
    bool present() const override { return false; }
    void init_confirmation() override {}
    bool handle_msg(const MyMessage&) override { return false; }
    void update() override {}
    bool update_io(CommonIO&, size_t) override {return true;}
    void alert() override {}
    float measure_current(float, PowerSensor&) override {return 0.0;}
};

#if defined(DIMMER)
using LightDimmer = ActiveDimmer;
#elif defined(RGB)
using LightDimmer = RgbDimmer;
#elif defined(RGBW)
using LightDimmer = RgbwDimmer;
#else
using LightDimmer = StubDimmer;
#define DIMMER_ID 0
#define LED_PIN_1 OUTPUT_PIN_1
#define LED_PIN_2 OUTPUT_PIN_2
#define LED_PIN_3 OUTPUT_PIN_3
#define LED_PIN_4 OUTPUT_PIN_4
#define BUTTON_1 INPUT_PIN_1
#define BUTTON_2 INPUT_PIN_2
#define NUMBER_OF_CHANNELS 4
#endif
