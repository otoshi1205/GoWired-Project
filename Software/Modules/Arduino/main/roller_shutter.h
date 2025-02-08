#ifndef ROLLER_SHUTTER_H
#define ROLLER_SHUTTER_H

#include "custom_types.h"

#include <core/Shutters.h>
#include <core/PowerSensor.h>
#include <core/MySensorsCore.h>

class RollerShutter
{

public:
    RollerShutter();
    void setup(CommonIOPins &io);
    bool present() const;
    void init_confirmation() const;
    bool handle_msg(const MyMessage& message);
    void calibrate(float Vcc, PowerSensor& power_sensor);
    void update(float Current);
    void update_io(CommonIO& io_pin, size_t idx);
    void start();
    void stop();

    enum class State {
        UP,
        DOWN,
        STOP
    };

private:
    uint32_t MovementTime = 0;
    uint32_t StartTime = 0;
    Shutters Shutter;
    MyMessage MsgUP;
    MyMessage MsgDOWN;
    MyMessage MsgSTOP;
    MyMessage MsgPERCENTAGE;
    MyMessage MsgSTATUS;
};

#endif