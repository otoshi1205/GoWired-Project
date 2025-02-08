#include "roller_shutter.h"
#include <avr/wdt.h>


#ifndef SHUTTER_ID
#define SHUTTER_ID 0
#endif

RollerShutter::RollerShutter()
: Shutter(EEA_SHUTTER_TIME_DOWN, EEA_SHUTTER_TIME_UP, EEA_SHUTTER_POSITION)
, MsgUP(SHUTTER_ID, V_UP)
, MsgDOWN(SHUTTER_ID, V_DOWN)
, MsgSTOP(SHUTTER_ID, V_STOP)
, MsgPERCENTAGE(0, V_PERCENTAGE)
, MsgSTATUS(0, V_STATUS) {
}

#ifdef ROLLER_SHUTTER

void RollerShutter::setup(CommonIOPins& io) {
    Shutter.SetOutputs( RELAY_OFF, RELAY_1, RELAY_2);
    io[SHUTTER_ID].SetValues(RELAY_OFF, false, 3, BUTTON_1);
    io[SHUTTER_ID + 1].SetValues(RELAY_OFF, false, 3, BUTTON_2);
    if(!Shutter.Calibrated) {
        Shutter.Calibration(UP_TIME, DOWN_TIME);
    }
}

bool RollerShutter::present() const {
    return ::present(SHUTTER_ID, S_COVER, "Roller Shutter");
}

void RollerShutter::init_confirmation() const {
    send(MsgUP.set(0));
    request(SHUTTER_ID, V_UP);
    wait(2000, C_SET, V_UP);

    send(MsgDOWN.set(0));
    request(SHUTTER_ID, V_DOWN);
    wait(2000, C_SET, V_DOWN);

    send(MsgSTOP.set(0));
    request(SHUTTER_ID, V_STOP);
    wait(2000, C_SET, V_STOP);

    send(MsgPERCENTAGE.setSensor(SHUTTER_ID).set(Shutter.Position));
    request(SHUTTER_ID, V_PERCENTAGE);
    wait(2000, C_SET, V_PERCENTAGE);
}

bool RollerShutter::handle_msg(const MyMessage& message) {

    if(message.sensor != SHUTTER_ID) {
        return false;
    }

    switch (message.type)
    {
    case V_PERCENTAGE:
        int NewPosition = atoi(message.data);
        NewPosition = NewPosition > 100 ? 100 : NewPosition;
        NewPosition = NewPosition < 0 ? 0 : NewPosition;
        Shutter.NewState = (uint8_t)State::STOP;
        //ShutterUpdate(0);
        MovementTime = Shutter.ReadNewPosition(NewPosition) * 10;
        return true;
    case V_UP:
        MovementTime = Shutter.ReadMessage(0) * 1000;
        return true;
    case V_DOWN:
        MovementTime = Shutter.ReadMessage(1) * 1000;
        return true;
    case V_STOP:
        MovementTime = Shutter.ReadMessage(2);
        return true;
    default:
        return false;
    }
}

/**
 * @brief Measures shutter movement duration; calls class Calibration() function to save measured durations
 * 
 * @param Vcc current uC voltage
 */
void RollerShutter::calibrate(float Vcc, PowerSensor& power_sensor) {

  float Current = 0;
  uint32_t DownTimeCumulated = 0;
  uint32_t UpTimeCumulated = 0;
  uint32_t StartTime = 0;
  uint32_t StopTime = 0;
  uint32_t MeasuredTime = 0;

  // Opening the shutter  
  Shutter.NewState = (uint8_t)State::UP;
  Shutter.Movement();

  do  {
    delay(500);
    wdt_reset();
    Current = power_sensor.MeasureAC(Vcc);
  } while(Current > PS_OFFSET);

  Shutter.NewState = (uint8_t)State::STOP;
  Shutter.Movement();

  delay(1000);

  // Calibrating
  for(int i=0; i<CALIBRATION_SAMPLES; i++) {
    for(int j=1; j>=0; j--)  {
      Shutter.NewState = j;
      Shutter.Movement();
      StartTime = millis();

      do  {
        delay(250);
        Current = power_sensor.MeasureAC(Vcc);
        StopTime = millis();
        wdt_reset();
      } while(Current > PS_OFFSET);

      Shutter.NewState = (uint8_t)State::STOP;
      Shutter.Movement();

      MeasuredTime = StopTime - StartTime;

      if(j) {
        DownTimeCumulated += (int)(MeasuredTime / 1000);
      }
      else  {
        UpTimeCumulated += (int)(MeasuredTime / 1000);
      }

      delay(1000);
    }
  }

  Shutter.Position = 0;

  uint8_t DownTime = (int)(DownTimeCumulated / CALIBRATION_SAMPLES);
  uint8_t UpTime = (int)(UpTimeCumulated / CALIBRATION_SAMPLES);

  Shutter.Calibration(UpTime+1, DownTime+1);

  EEPROM.put(EEA_SHUTTER_TIME_DOWN, DownTime);
  EEPROM.put(EEA_SHUTTER_TIME_UP, UpTime);
  EEPROM.put(EEA_SHUTTER_POSITION, Shutter.Position);

  // Inform Controller about the current state of roller shutter
  send(MsgSTOP);
  send(MsgPERCENTAGE.setSensor(SHUTTER_ID).set(Shutter.Position));
  #ifdef RS485_DEBUG
    send(MsgDEBUG.set("DownTime ; UpTime"));
    send(MsgCUSTOM.set(DownTime)); send(MsgCUSTOM.set(UpTime));
  #endif
    
}

/**
 * @brief Updates shutter condition, informs controller about shutter condition and position
 * 
 */
void RollerShutter::update(float Current) {

  uint32_t StopTime = 0;
  uint32_t MeasuredTime;
  State TempState = State::STOP;
  bool Direction;

  if(Shutter.State != 2) {
    if((millis() >= StartTime + MovementTime) || (Current < PS_OFFSET)) {
      StopTime = millis();
    }
    else if(millis() < StartTime)  {
      uint32_t Temp = 4294967295 - StartTime + millis();
      wait(MovementTime - Temp);
      StartTime = 0;
      StopTime = MovementTime;
    }
  }

  if(Shutter.State != Shutter.NewState) {
    if(Shutter.NewState != (uint8_t)State::STOP)  {
      if(Shutter.State == (uint8_t)State::STOP)  {
        start();
      }
      else  {
        TempState = (State)Shutter.NewState;
        StopTime = millis();
      }
    }
    else  {
      StopTime = millis();
    }
  }

  if(StopTime > 0)  {
    Direction = Shutter.State;
    Shutter.NewState = (uint8_t)State::STOP;
    Shutter.Movement();
    send(MsgSTOP.setSensor(SHUTTER_ID));

    MeasuredTime = StopTime - StartTime;
    Shutter.CalculatePosition(Direction, MeasuredTime);
    EEPROM.put(EEA_SHUTTER_POSITION, Shutter.Position);
  
    send(MsgPERCENTAGE.setSensor(SHUTTER_ID).set(Shutter.Position));
  
    if(TempState != State::STOP)  {
      wait(500);
      Shutter.NewState = (uint8_t)TempState;
      start();
    }
  }
}

void RollerShutter::start() {
  Shutter.Movement();
  StartTime = millis();
  Shutter.NewState == (uint8_t)State::UP ? send(MsgUP.setSensor(SHUTTER_ID)) : send(MsgDOWN.setSensor(SHUTTER_ID));
  wait(500);
}

void RollerShutter::stop() {
    Shutter.NewState = (uint8_t)State::STOP;
    update(0);
}


void RollerShutter::update_io(CommonIO& io_pin, size_t idx) {
    if(io_pin.NewState != 2) {
        MovementTime = Shutter.ReadButtons(idx) * 1000;
        io_pin.State = io_pin.NewState;
    } else {
        #ifdef SPECIAL_BUTTON
        send(MsgSTATUS.setSensor(SPECIAL_BUTTON_ID).set(true));
        #endif
        io_pin.NewState = io_pin.State;
    }
}

#else

//Stub implementations

void RollerShutter::setup(CommonIOPins& io) {}
bool RollerShutter::present() const {return false;}
void RollerShutter::init_confirmation() const {}
bool RollerShutter::handle_msg(const MyMessage& message) {return false;}
void RollerShutter::calibrate(float Vcc, PowerSensor& power_sensor) {}
void RollerShutter::update(float Current) {}
void RollerShutter::start() {}
void RollerShutter::stop() {}
void RollerShutter::update_io(CommonIO& io_pin, size_t idx) {}

#endif