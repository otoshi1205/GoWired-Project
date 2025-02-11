#ifndef ROLLER_SHUTTER_H
#define ROLLER_SHUTTER_H

#include "custom_types.h"

#include <core/Shutters.h>
#include <core/PowerSensor.h>
#include <core/MySensorsCore.h>

template <typename Derived>
class RollerShutterBase {

public:
    RollerShutterBase(int shutter_id)
        : shutter_id_(shutter_id),
          Shutter(EEA_SHUTTER_TIME_DOWN, EEA_SHUTTER_TIME_UP, EEA_SHUTTER_POSITION),
          MsgUP(shutter_id_, V_UP),
          MsgDOWN(shutter_id_, V_DOWN),
          MsgSTOP(shutter_id_, V_STOP),
          MsgPERCENTAGE(0, V_PERCENTAGE),
          MsgSTATUS(0, V_STATUS) {}

    void setup(CommonIOPins& io) {
        static_cast<Derived*>(this)->setup_impl(io);
    }
    bool present() const {
        return static_cast<const Derived*>(this)->present_impl();
    }
    void init_confirmation() const {
        static_cast<const Derived*>(this)->init_confirmation_impl();
    }
    bool handle_msg(const MyMessage& message) {
        return static_cast<Derived*>(this)->handle_msg_impl(message);
    }
    void calibrate(float Vcc, PowerSensor& power_sensor) {
        static_cast<Derived*>(this)->calibrate_impl(Vcc, power_sensor);
    }
    void update(float Current) {
        static_cast<Derived*>(this)->update_impl(Current);
    }
    void start() {
        static_cast<Derived*>(this)->start_impl();
    }
    void stop() {
        static_cast<Derived*>(this)->stop_impl();
    }
    void update_io(CommonIO& io_pin, size_t idx) {
        static_cast<Derived*>(this)->update_io_impl(io_pin, idx);
    }

    enum class State {
        UP,
        DOWN,
        STOP,
    };


protected:
    const uint16_t shutter_id_;
    uint32_t MovementTime = 0;
    uint32_t StartTime = 0;
    Shutters Shutter;
    MyMessage MsgUP;
    MyMessage MsgDOWN;
    MyMessage MsgSTOP;
    MyMessage MsgPERCENTAGE;
    MyMessage MsgSTATUS;
};

class ActiveRollerShutter : public RollerShutterBase<ActiveRollerShutter> {
public:
    ActiveRollerShutter(int shutterId) : RollerShutterBase(shutterId) {}
    void setup_impl(CommonIOPins& io);
    bool present_impl() const;
    void init_confirmation_impl() const;
    bool handle_msg_impl(const MyMessage& message);
    void calibrate_impl(float Vcc, PowerSensor& power_sensor);
    void update_impl(float Current);
    void start_impl();
    void stop_impl();
    void update_io_impl(CommonIO& io_pin, size_t idx);
};

class StubRollerShutter : public RollerShutterBase<StubRollerShutter> {
public:
    StubRollerShutter(int shutterId) : RollerShutterBase(shutterId) {}
    void setup_impl(CommonIOPins&) {}
    bool present_impl() const { return false; }
    void init_confirmation_impl() const {}
    bool handle_msg_impl(const MyMessage&) { return false; }
    void calibrate_impl(float, PowerSensor&) {}
    void update_impl(float) {}
    void start_impl() {}
    void stop_impl() {}
    void update_io_impl(CommonIO&, size_t) {}
};

#ifdef ROLLER_SHUTTER
using RollerShutter = ActiveRollerShutter;
#else
using RollerShutter = StubRollerShutter;
#endif

#endif