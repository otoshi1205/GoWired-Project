#include "light_dimmer.h"

void CommonDimmer::setup(CommonIOPins& io) {
    io[0].SetValues(0, false, 3, BUTTON_1);
    io[1].SetValues(0, false, 3, BUTTON_2);
}

void ActiveDimmer::setup(CommonIOPins& io) {
    CommonDimmer::setup(io);
    dimmer.SetValues(NUMBER_OF_CHANNELS, DIMMING_STEP, DIMMING_INTERVAL, LED_PIN_1, LED_PIN_2, LED_PIN_3, LED_PIN_4);
}

void RgbDimmer::setup(CommonIOPins& io) {
    CommonDimmer::setup(io);
    dimmer.SetValues(NUMBER_OF_CHANNELS, DIMMING_STEP, DIMMING_INTERVAL, LED_PIN_1, LED_PIN_2, LED_PIN_3);
}

void RgbwDimmer::setup(CommonIOPins& io) {
    CommonDimmer::setup(io);
    dimmer.SetValues(NUMBER_OF_CHANNELS, DIMMING_STEP, DIMMING_INTERVAL, LED_PIN_1, LED_PIN_2, LED_PIN_3, LED_PIN_4);
}

bool ActiveDimmer::present() const {
    return ::present(dimmer_id_, S_DIMMER, "Dimmer");
}

bool RgbDimmer::present() const {
    return ::present(dimmer_id_, S_RGB_LIGHT, "RGB");
}

bool RgbwDimmer::present() const {
    return ::present(dimmer_id_, S_RGBW_LIGHT, "RGBW");
}

void CommonDimmer::init_confirmation() {
    send(MsgSTATUS.setSensor(dimmer_id_).set(false));
    request(dimmer_id_, V_STATUS);
    wait(2000, C_SET, V_STATUS);
    
    send(MsgPERCENTAGE.setSensor(dimmer_id_).set(dimmer.NewDimmingLevel));
    request(dimmer_id_, V_PERCENTAGE);
    wait(2000, C_SET, V_PERCENTAGE);
}

void RgbDimmer::init_confirmation() {
    CommonDimmer::init_confirmation();
    send(MsgRGB.setSensor(dimmer_id_).set("ffffff"));
    request(dimmer_id_, V_RGB);
    wait(2000, C_SET, V_RGB);
}

void RgbwDimmer::init_confirmation() {
    CommonDimmer::init_confirmation();
    send(MsgRGBW.setSensor(dimmer_id_).set("ffffffff"));
    request(dimmer_id_, V_RGBW);
    wait(2000, C_SET, V_RGBW);
}

bool CommonDimmer::handle_msg(const MyMessage& message) {
    if(message.sensor != dimmer_id_) {
        return false;
    }

    switch (message.type)
    {
    case V_STATUS:
        dimmer.ChangeState(message.getBool());
        return true;
    case V_PERCENTAGE:
        dimmer.NewDimmingLevel = atoi(message.data);
        dimmer.NewDimmingLevel = dimmer.NewDimmingLevel > 100 ? 100 : dimmer.NewDimmingLevel;
        return true;
    case V_RGB:
    case V_RGBW: {
        const char *rgbvalues = message.getString();
        dimmer.NewColorValues(rgbvalues);
        return true;
    }
    default:
        return false;
    }
}

bool CommonDimmer::update_io(CommonIO& io_pin, size_t idx) {
    if(idx == 0)  {
        if(io_pin.NewState != 2) {
            // Change dimmer state
            dimmer.ChangeState(!dimmer.CurrentState);
            send(MsgSTATUS.setSensor(dimmer_id_).set(dimmer.CurrentState));
            io_pin.State = io_pin.NewState;
        }
        if(io_pin.NewState == 2) {
            #ifdef SPECIAL_BUTTON
            send(MsgSTATUS.setSensor(SPECIAL_BUTTON_ID).set(true));
            #endif
            io_pin.NewState = io_pin.State;
        }
        }
        else if(idx == 1) {
        if(io_pin.NewState != 2)  {
            if(!dimmer.CurrentState) {
                return false;
            }
                
            // Toggle dimming level by DIMMING_TOGGLE_STEP
            dimmer.NewDimmingLevel += DIMMING_TOGGLE_STEP;
            dimmer.NewDimmingLevel = dimmer.NewDimmingLevel > 100 ? DIMMING_TOGGLE_STEP : dimmer.NewDimmingLevel;
            send(MsgPERCENTAGE.setSensor(dimmer_id_).set(dimmer.NewDimmingLevel));
            io_pin.NewState = io_pin.State;
        }
    }
    return true;
}

float CommonDimmer::measure_current(float Vcc, PowerSensor& power_sensor) {
    if (dimmer.CurrentState)  {
        return power_sensor.MeasureDC(Vcc);
    }
    return 0;
}

void CommonDimmer::update() {
    dimmer.UpdateDimmer();
}

void CommonDimmer::alert() {
    dimmer.ChangeState(false);
    send(MsgSTATUS.setSensor(dimmer_id_).set(dimmer.CurrentState));

}