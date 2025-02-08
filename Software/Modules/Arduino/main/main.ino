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
 * This is source code for GoWired MCU working with 2SSR, RGBW & 4RelayDin Shields.
 * 
 * 
 */

/***** INCLUDES *****/
#include "Configuration.h"
#include <GoWired.h>
#include "roller_shutter.h"
#ifdef SHT30
  #include <SHTSensor.h>
#elif defined(DHT22)
  #include <dht.h>
#endif

/***** Globals *****/

// Timer
uint32_t LastUpdate = 0;               // Time of last update of interval sensors
bool CheckNow = false;

// Module Safety Indicators
bool THERMAL_ERROR = false;                 // Thermal error status
bool InformControllerTS = false;            // Was controller informed about error?
bool OVERCURRENT_ERROR[4] = {false, false, false, false};             // Overcurrent error status
bool InformControllerES = false;            // Was controller informed about error?
uint8_t ET_ERROR = 3;                       // External thermometer status (0 - ok, 1 - checksum error, 2 - timeout error, 3 - default/initialization)

// Initialization
bool InitConfirm = false;

/***** Constructors *****/
// CommonIo constructor
#if (NUMBER_OF_RELAYS + NUMBER_OF_INPUTS > 0)
  CommonIOPins common_io;
#endif

MyMessage MsgSTATUS(0, V_STATUS);
MyMessage MsgPERCENTAGE(0, V_PERCENTAGE);
MyMessage MsgWATT(0, V_WATT);
MyMessage MsgTEMP(0, V_TEMP);
MyMessage MsgHUM(0, V_HUM);
MyMessage MsgTEXT(0, V_TEXT);

RollerShutter roller_shutter;

// Dimmer
#if defined(DIMMER) || defined(RGB) || defined(RGBW)
  Dimmer Dimmer;
  MyMessage MsgRGB(DIMMER_ID, V_RGB);
  MyMessage MsgRGBW(DIMMER_ID, V_RGBW);
#endif

// Power sensor constructor
#if defined(POWER_SENSOR) && !defined(FOUR_RELAY)
  PowerSensor PS;
#elif defined(POWER_SENSOR) && defined(FOUR_RELAY)
  PowerSensor PS[NUMBER_OF_RELAYS];
#endif

// Internal thermometer constructor
#ifdef INTERNAL_TEMP
  AnalogTemp AnalogTemp(IT_PIN, MAX_TEMPERATURE, MVPERC, ZEROVOLTAGE);
#endif

// External thermometer constructor
#ifdef EXTERNAL_TEMP
  #ifdef DHT22
    dht DHT;
  #endif
  #ifdef SHT30
    SHTSensor sht;
  #endif
#endif

#ifdef RS485_DEBUG
  MyMessage MsgDEBUG(DEBUG_ID, V_TEXT);
  MyMessage MsgDEBUG2(DEBUG_ID, V_WATT);
  MyMessage MsgCUSTOM(0, V_CUSTOM);
#endif

/**
 * @brief Function called before setup(); resets wdt
 * 
 */
void before() {
  #ifdef ENABLE_WATCHDOG
    wdt_reset();
    MCUSR = 0;
    wdt_disable();
  #endif
}

/**
 * @brief Setups software components: wdt, expander, inputs, outputs
 * 
 */
void setup() {

  #ifdef ENABLE_WATCHDOG
    wdt_enable(WDTO_8S);
  #endif

  float Vcc = ReadVcc();  // mV

  // POWER SENSOR
  #if defined(POWER_SENSOR) && !defined(FOUR_RELAY)
    PS.SetValues(PS_PIN, MVPERAMP, RECEIVER_VOLTAGE, MAX_CURRENT, POWER_MEASURING_TIME, Vcc);
  #elif defined(POWER_SENSOR) && defined(FOUR_RELAY)
    PS[RELAY_ID_1].SetValues(PS_PIN_1, MVPERAMP, RECEIVER_VOLTAGE, MAX_CURRENT, POWER_MEASURING_TIME, Vcc);
    PS[RELAY_ID_2].SetValues(PS_PIN_2, MVPERAMP, RECEIVER_VOLTAGE, MAX_CURRENT, POWER_MEASURING_TIME, Vcc);
    PS[RELAY_ID_3].SetValues(PS_PIN_3, MVPERAMP, RECEIVER_VOLTAGE, MAX_CURRENT, POWER_MEASURING_TIME, Vcc);
    PS[RELAY_ID_4].SetValues(PS_PIN_4, MVPERAMP, RECEIVER_VOLTAGE, MAX_CURRENT, POWER_MEASURING_TIME, Vcc);
  #endif

  // OUTPUT
  #ifdef DOUBLE_RELAY
    common_io[RELAY_ID_1].SetValues(RELAY_OFF, false, 4, BUTTON_1, RELAY_1);
    common_io[RELAY_ID_2].SetValues(RELAY_OFF, false, 4, BUTTON_2, RELAY_2);
  #endif

    roller_shutter.setup(common_io);

  #ifdef FOUR_RELAY
    common_io[RELAY_ID_1].SetValues(RELAY_OFF, 2, RELAY_1);
    common_io[RELAY_ID_2].SetValues(RELAY_OFF, 2, RELAY_2);
    common_io[RELAY_ID_3].SetValues(RELAY_OFF, 2, RELAY_3);
    common_io[RELAY_ID_4].SetValues(RELAY_OFF, 2, RELAY_4);
  #endif

  #if defined(DIMMER) || defined(RGB) || defined(RGBW)
    common_io[0].SetValues(0, false, 3, BUTTON_1);
    common_io[1].SetValues(0, false, 3, BUTTON_2);
  #endif

  #ifdef DIMMER
    Dimmer.SetValues(NUMBER_OF_CHANNELS, DIMMING_STEP, DIMMING_INTERVAL, LED_PIN_1, LED_PIN_2, LED_PIN_3, LED_PIN_4);
  #elif defined(RGB)
    Dimmer.SetValues(NUMBER_OF_CHANNELS, DIMMING_STEP, DIMMING_INTERVAL, LED_PIN_1, LED_PIN_2, LED_PIN_3);
  #elif defined(RGBW)
    Dimmer.SetValues(NUMBER_OF_CHANNELS, DIMMING_STEP, DIMMING_INTERVAL, LED_PIN_1, LED_PIN_2, LED_PIN_3, LED_PIN_4);
  #endif

  // INPUT
  #ifdef INPUT_1
    #ifdef PULLUP_1
      common_io[INPUT_ID_1].SetValues(RELAY_OFF, INVERT_1, 0, PIN_1);
    #else
      common_io[INPUT_ID_1].SetValues(RELAY_OFF, INVERT_1, 1, PIN_1);
    #endif
  #endif

  #ifdef INPUT_2
    #ifdef PULLUP_2
      common_io[INPUT_ID_2].SetValues(RELAY_OFF, INVERT_2, 0, PIN_2);
    #else
      common_io[INPUT_ID_2].SetValues(RELAY_OFF, INVERT_2, 1, PIN_2);
    #endif
  #endif

  #ifdef INPUT_3
    #ifdef PULLUP_3
      common_io[INPUT_ID_3].SetValues(RELAY_OFF, INVERT_3, 0, PIN_3);
    #else
      common_io[INPUT_ID_3].SetValues(RELAY_OFF, INVERT_3, 1, PIN_3);
    #endif
  #endif

  #ifdef INPUT_4
    #ifdef PULLUP_4
      common_io[INPUT_ID_4].SetValues(RELAY_OFF, INVERT_4, 0, PIN_4);
    #else
      common_io[INPUT_ID_4].SetValues(RELAY_OFF, INVERT_4, 1, PIN_4);
    #endif
  #endif

  // EXTERNAL THERMOMETER
  #ifdef EXTERNAL_TEMP
    #ifdef DHT22
      pinMode(ET_PIN, INPUT);
    #endif
    #ifdef SHT30
      Wire.begin();
      sht.init();
      sht.setAccuracy(SHTSensor::SHT_ACCURACY_MEDIUM);
    #endif
  #endif

}

/**
 * @brief Presents module to the controller, send name, software version, info about sensors
 * 
 */
void presentation() {

  sendSketchInfo(SN, SV);

  // OUTPUT
  #ifdef DOUBLE_RELAY
    present(RELAY_ID_1, S_BINARY, "Relay 1");   wait(PRESENTATION_DELAY);
    present(RELAY_ID_2, S_BINARY, "Relay 2");   wait(PRESENTATION_DELAY);
  #endif

    if (roller_shutter.present()) {
        wait(PRESENTATION_DELAY);
    }  

  #ifdef FOUR_RELAY
    present(RELAY_ID_1, S_BINARY, "Relay 1");   wait(PRESENTATION_DELAY);
    present(RELAY_ID_2, S_BINARY, "Relay 2");   wait(PRESENTATION_DELAY);
    present(RELAY_ID_3, S_BINARY, "Relay 3");   wait(PRESENTATION_DELAY);
    present(RELAY_ID_4, S_BINARY, "Relay 4");   wait(PRESENTATION_DELAY);
  #endif

  #ifdef DIMMER
    present(DIMMER_ID, S_DIMMER, "Dimmer"); wait(PRESENTATION_DELAY);
  #endif

  #ifdef RGB
    present(DIMMER_ID, S_RGB_LIGHT, "RGB"); wait(PRESENTATION_DELAY);
  #endif

  #ifdef RGBW
    present(DIMMER_ID, S_RGBW_LIGHT, "RGBW");   wait(PRESENTATION_DELAY);
  #endif

  // DIGITAL INPUT
  #ifdef INPUT_1
    present(INPUT_ID_1, S_BINARY, "Input 1");   wait(PRESENTATION_DELAY);
  #endif

  #ifdef INPUT_2
    present(INPUT_ID_2, S_BINARY, "Input 2");   wait(PRESENTATION_DELAY);
  #endif

  #ifdef INPUT_3
    present(INPUT_ID_3, S_BINARY, "Input 3");   wait(PRESENTATION_DELAY);
  #endif

  #ifdef INPUT_4
    present(INPUT_ID_4, S_BINARY, "Input 4");   wait(PRESENTATION_DELAY);
  #endif

  #ifdef SPECIAL_BUTTON
    present(SPECIAL_BUTTON_ID, S_BINARY, "Longpress-1"); wait(PRESENTATION_DELAY);
    present(SPECIAL_BUTTON_ID+1, S_BINARY, "Longpress-2"); wait(PRESENTATION_DELAY);
  #endif

  // POWER SENSOR
  #if defined(POWER_SENSOR) && !defined(FOUR_RELAY)
    present(PS_ID, S_POWER, "Power Sensor");    wait(PRESENTATION_DELAY);
  #elif defined(POWER_SENSOR) && defined(FOUR_RELAY)
    present(PS_ID_1, S_POWER, "Power Sensor 1");    wait(PRESENTATION_DELAY);
    present(PS_ID_2, S_POWER, "Power Sensor 2");    wait(PRESENTATION_DELAY);
    present(PS_ID_3, S_POWER, "Power Sensor 3");    wait(PRESENTATION_DELAY);
    present(PS_ID_4, S_POWER, "Power Sensor 4");    wait(PRESENTATION_DELAY);
  #endif

  // Internal Thermometer
  #ifdef INTERNAL_TEMP
    present(IT_ID, S_TEMP, "Internal Thermometer"); wait(PRESENTATION_DELAY);
  #endif

  // External Thermometer
  #ifdef EXTERNAL_TEMP
    present(ETT_ID, S_TEMP, "External Thermometer"); wait(PRESENTATION_DELAY);
    present(ETH_ID, S_HUM, "External Hygrometer");  wait(PRESENTATION_DELAY);
  #endif

  // I2C


  // Error Reporting
  #ifdef ERROR_REPORTING
    #ifdef POWER_SENSOR
      present(ES_ID, S_BINARY, "OVERCURRENT ERROR");    wait(PRESENTATION_DELAY);
    #endif
    #ifdef INTERNAL_TEMP
      present(TS_ID, S_BINARY, "THERMAL ERROR");    wait(PRESENTATION_DELAY);
    #endif
    #ifdef EXTERNAL_TEMP
      present(ETS_ID, S_BINARY, "ET STATUS");   wait(PRESENTATION_DELAY);
    #endif
  #endif

  #ifdef RS485_DEBUG
    present(DEBUG_ID, S_INFO, "DEBUG INFO");
  #endif

  // Configuration sensor
  present(CONFIGURATION_SENSOR_ID, S_INFO, "TEXT Msg");

}

/**
 * @brief Sends initial value of sensors as required by Home Assistant
 * 
 */
void InitConfirmation() {

  // OUTPUT
  #ifdef DOUBLE_RELAY
    send(MsgSTATUS.setSensor(RELAY_ID_1).set(common_io[RELAY_ID_1].NewState));
    request(RELAY_ID_1, V_STATUS);
    wait(2000, C_SET, V_STATUS);

    send(MsgSTATUS.setSensor(RELAY_ID_2).set(common_io[RELAY_ID_2].NewState));
    request(RELAY_ID_2, V_STATUS);
    wait(2000, C_SET, V_STATUS);
  #endif

    roller_shutter.init_confirmation();

  #ifdef FOUR_RELAY
    send(MsgSTATUS.setSensor(RELAY_ID_1).set(common_io[RELAY_ID_1].NewState));
    request(RELAY_ID_1, V_STATUS);
    wait(2000, C_SET, V_STATUS);
    
    send(MsgSTATUS.setSensor(RELAY_ID_2).set(common_io[RELAY_ID_2].NewState));
    request(RELAY_ID_2, V_STATUS);
    wait(2000, C_SET, V_STATUS);
    
    send(MsgSTATUS.setSensor(RELAY_ID_3).set(common_io[RELAY_ID_3].NewState));
    request(RELAY_ID_3, V_STATUS);
    wait(2000, C_SET, V_STATUS);
    
    send(MsgSTATUS.setSensor(RELAY_ID_4).set(common_io[RELAY_ID_4].NewState));
    request(RELAY_ID_4, V_STATUS);
    wait(2000, C_SET, V_STATUS);
  #endif

  #if defined(DIMMER) || defined(RGB) || defined(RGBW)
    send(MsgSTATUS.setSensor(DIMMER_ID).set(false));
    request(DIMMER_ID, V_STATUS);
    wait(2000, C_SET, V_STATUS);
    
    send(MsgPERCENTAGE.setSensor(DIMMER_ID).set(Dimmer.NewDimmingLevel));
    request(DIMMER_ID, V_PERCENTAGE);
    wait(2000, C_SET, V_PERCENTAGE);
  #endif

  #ifdef RGB
    send(MsgRGB.setSensor(DIMMER_ID).set("ffffff"));
    request(DIMMER_ID, V_RGB);
    wait(2000, C_SET, V_RGB);
  #elif defined(RGBW)
    send(MsgRGBW.setSensor(DIMMER_ID).set("ffffffff"));
    request(DIMMER_ID, V_RGBW);
    wait(2000, C_SET, V_RGBW);
  #endif

  // DIGITAL INPUT
  #ifdef INPUT_1
    send(MsgSTATUS.setSensor(INPUT_ID_1).set(common_io[INPUT_ID_1].NewState));
  #endif

  #ifdef INPUT_2
    send(MsgSTATUS.setSensor(INPUT_ID_2).set(common_io[INPUT_ID_2].NewState));
  #endif

  #ifdef INPUT_3
    send(MsgSTATUS.setSensor(INPUT_ID_3).set(common_io[INPUT_ID_3].NewState));
  #endif

  #ifdef INPUT_4
    send(MsgSTATUS.setSensor(INPUT_ID_4).set(common_io[INPUT_ID_4].NewState));
  #endif

  #ifdef SPECIAL_BUTTON
    send(MsgSTATUS.setSensor(SPECIAL_BUTTON_ID).set(0));
    send(MsgSTATUS.setSensor(SPECIAL_BUTTON_ID+1).set(0));
  #endif

  // Built-in sensors
  #ifdef POWER_SENSOR
    #if !defined(FOUR_RELAY)
      send(MsgWATT.setSensor(PS_ID).set("0"));
    #elif defined(FOUR_RELAY)
      for(int i=PS_ID_1; i<=PS_ID_4; i++)  {
        send(MsgWATT.setSensor(i).set("0"));
      }
    #endif
  #endif

  #ifdef INTERNAL_TEMP
    send(MsgTEMP.setSensor(IT_ID).set((int)AnalogTemp.MeasureT(ReadVcc())));
  #endif

  // External sensors
  #ifdef EXTERNAL_TEMP
    ETUpdate();
  #endif

  // Error Reporting
  #ifdef ERROR_REPORTING
    #ifdef POWER_SENSOR
      send(MsgSTATUS.setSensor(ES_ID).set(0));
    #endif
    #ifdef INTERNAL_TEMP
      send(MsgSTATUS.setSensor(TS_ID).set(0));
    #endif
    #ifdef EXTERNAL_TEMP
      send(MsgSTATUS.setSensor(ETS_ID).set(0));
    #endif
  #endif

  #ifdef RS485_DEBUG
    send(MsgDEBUG.setSensor(DEBUG_ID).set("DEBUG MESSAGE"));
  #endif

  send(MsgTEXT.setSensor(CONFIGURATION_SENSOR_ID).set("CONFIG INIT"));

  InitConfirm = true;
}


/**
 * @brief Handles incoming messages
 * 
 * @param message incoming message data
 */
void receive(const MyMessage &message)  {
  if (roller_shutter.handle_msg(message)) {
    return;
  }
  if (message.type == V_STATUS) {
    #if defined(POWER_SENSOR) && defined(ERROR_REPORTING)
      if (message.sensor == ES_ID)  {
        for (int i = 0; i < 4; i++)  {
          OVERCURRENT_ERROR[i] = message.getBool();
        }
        InformControllerES = false;
      }
    #endif
    #if defined(INTERNAL_TEMP) && defined(ERROR_REPORTING)
      if (message.sensor == TS_ID)  {
        THERMAL_ERROR = message.getBool();
        if (THERMAL_ERROR == false)  {
          InformControllerTS = false;
        }
      }
    #endif
    #ifdef SPECIAL_BUTTON
      if (message.sensor == SPECIAL_BUTTON_ID || message.sensor == SPECIAL_BUTTON_ID+1)  {
        // Ignore this message
      }
    #endif
    #if defined(DIMMER) || defined(RGB) || defined(RGBW)
      if (message.sensor == DIMMER_ID) {
        Dimmer.ChangeState(message.getBool());
      }
    #endif
    #if defined(DOUBLE_RELAY)
      if (message.sensor == RELAY_ID_1 || message.sensor == RELAY_ID_2)  {
        if (!OVERCURRENT_ERROR[0] && !THERMAL_ERROR) {
          common_io[message.sensor].SetState(message.getBool());
          common_io[message.sensor].SetRelay();
        }
      }
    #endif
    #ifdef FOUR_RELAY
      if (message.sensor >= RELAY_ID_1 && message.sensor < NUMBER_OF_RELAYS) {
        for (int i = RELAY_ID_1; i < RELAY_ID_1 + NUMBER_OF_RELAYS; i++) {
          if (message.sensor == i) {
            if (!OVERCURRENT_ERROR[i] && !THERMAL_ERROR) {
              common_io[message.sensor].NewState = message.getBool();
              common_io[message.sensor].SetRelay();
            }
          }
        }
      }
    #endif
  }
  else if (message.type == V_PERCENTAGE) {
    #if defined(DIMMER) || defined(RGB) || defined(RGBW)
      if(message.sensor == DIMMER_ID) {
        Dimmer.NewDimmingLevel = atoi(message.data);
        Dimmer.NewDimmingLevel = Dimmer.NewDimmingLevel > 100 ? 100 : Dimmer.NewDimmingLevel;
        Dimmer.NewDimmingLevel = Dimmer.NewDimmingLevel < 0 ? 0 : Dimmer.NewDimmingLevel;
      }
    #endif
  }
  else if (message.type == V_RGB || message.type == V_RGBW) {
    #if defined(RGB) || defined(RGBW)
      if(message.sensor == DIMMER_ID) {
        const char *rgbvalues = message.getString();

        Dimmer.NewColorValues(rgbvalues);
      }
    #endif
  }
  else if(message.type == V_TEXT) {
    // Configuration by message
    if(message.sensor == CONFIGURATION_SENSOR_ID)  {
      
      // Initialize strings and pointers
      char ReceivedPayload[10];
      char *RPaddr = ReceivedPayload;
      String RPstr = String(message.getString());

      // Turn String payload to char array and send back to the controller
      RPstr.toCharArray(ReceivedPayload, 10);
      send(MsgTEXT.setSensor(CONFIGURATION_SENSOR_ID).set(RPaddr));

      if(RPstr.equals(CONF_MSG_1)) {
          // Roller shutter: calibration
          float Vcc = ReadVcc();
          roller_shutter.calibrate(Vcc, PS);
      }
      else if(RPstr.equals(CONF_MSG_2)) {
        // No effect
      }
      else if(RPstr.equals(CONF_MSG_3)) {
        // Watchdog test procedure / module restart
        delay(10000);
      }
      else if(RPstr.equals(CONF_MSG_4)) {
        // Clear EEPROM and restart
        for (int i=0;i<1024;i++) {
          EEPROM.write(i,0xFF);
        }
        delay(10000);
      }
    }
  }
}

/**
 * @brief Reads temperature & humidity from an optional, external thermometer 
 * 
 */
void ETUpdate()  {

  #ifdef EXTERNAL_TEMP
    #ifdef DHT22
      int chk = DHT.read22(ET_PIN);
      switch (chk)  {
        case DHTLIB_OK:
          send(MsgTEMP.setSensor(ETT_ID).setDestination(0).set(DHT.temperature, 1));
          send(MsgHUM.setSensor(ETH_ID).set(DHT.humidity, 1));
          #ifdef HEATING_SECTION_SENSOR
            send(MsgTEMP.setSensor(ETT_ID).setDestination(MY_HEATING_CONTROLLER).set(DHT.temperature, 1));
          #endif
          #ifdef ERROR_REPORTING
            if (ET_ERROR != 0) {
              ET_ERROR = 0;
              send(MsgSTATUS.setSensor(ETS_ID).set(ET_ERROR));
            }
          #endif
        break;
        case DHTLIB_ERROR_CHECKSUM:
          #ifdef ERROR_REPORTING
            ET_ERROR = 1;
            send(MsgSTATUS.setSensor(ETS_ID).set(ET_ERROR));
          #endif
        break;
        case DHTLIB_ERROR_TIMEOUT:
          #ifdef ERROR_REPORTING
            ET_ERROR = 2;
            send(MsgSTATUS.setSensor(ETS_ID).set(ET_ERROR));
          #endif
        break;
        default:
          #ifdef ERROR_REPORTING
            ET_ERROR = 3;
            send(MsgSTATUS.setSensor(ETS_ID).set(ET_ERROR));
          #endif
        break;
      }
    #elif defined(SHT30)
      if(sht.readSample())  {
        send(MsgTEMP.setSensor(ETT_ID).setDestination(0).set(sht.getTemperature(), 1));
        send(MsgHUM.setSensor(ETH_ID).set(sht.getHumidity(), 1));
        #ifdef HEATING_SECTION_SENSOR
          send(MsgTEMP.setSensor(ETT_ID).setDestination(MY_HEATING_CONTROLLER).set(sht.getTemperature(), 1));
        #endif
      }
      else  {
        #ifdef ERROR_REPORTING
          ET_ERROR = 1;
          send(MsgSTATUS.setSensor(ETS_ID).set(ET_ERROR));
        #endif
      }
    #endif
  #endif
}

/**
 * @brief Updates common_io class objects; reads inputs & set outputs
 * 
 */
void UpdateIO() {

  int FirstSensor = 0;
  int Iterations = NUMBER_OF_RELAYS+NUMBER_OF_INPUTS;

  if(Iterations <= 0)  return;

  for (int i = FirstSensor; i < FirstSensor + Iterations; i++)  {
    common_io[i].CheckInput(LONGPRESS_DURATION, DEBOUNCE_VALUE);

    if (common_io[i].NewState == common_io[i].State)  continue;

    switch(common_io[i].SensorType)  {
      case 0:
        // Door/window/button
      case 1:
        // Motion sensor
        send(MsgSTATUS.setSensor(i).set(common_io[i].NewState));
        common_io[i].State = common_io[i].NewState;
        break;
      case 2:
        // Relay output
        // Nothing to do here
        break;
      case 3:
        // Button input
        #ifdef DIMMER_ID
          if(i == 0)  {
            if(common_io[i].NewState != 2) {
              // Change dimmer state
              Dimmer.ChangeState(!Dimmer.CurrentState);
              send(MsgSTATUS.setSensor(DIMMER_ID).set(Dimmer.CurrentState));
              common_io[i].State = common_io[i].NewState;
            }
            if(common_io[i].NewState == 2) {
              #ifdef SPECIAL_BUTTON
                send(MsgSTATUS.setSensor(SPECIAL_BUTTON_ID).set(true));
              #endif
              common_io[i].NewState = common_io[i].State;
            }
          }
          else if(i == 1) {
            if(common_io[i].NewState != 2)  {
              if(!Dimmer.CurrentState) continue;
                    
              // Toggle dimming level by DIMMING_TOGGLE_STEP
              Dimmer.NewDimmingLevel += DIMMING_TOGGLE_STEP;
              Dimmer.NewDimmingLevel = Dimmer.NewDimmingLevel > 100 ? DIMMING_TOGGLE_STEP : Dimmer.NewDimmingLevel;
              send(MsgPERCENTAGE.setSensor(DIMMER_ID).set(Dimmer.NewDimmingLevel));
              common_io[i].NewState = common_io[i].State;
            }
          }
        #endif
        #ifdef ROLLER_SHUTTER
          roller_shutter.update_io(common_io[i], i);
        #endif
        break;
      case 4:
        // Button input + Relay output
        if (common_io[i].NewState != 2)  {
          if (OVERCURRENT_ERROR[0] || THERMAL_ERROR)  continue;

          common_io[i].SetRelay();
          send(MsgSTATUS.setSensor(i).set(common_io[i].NewState));
        }
        else if (common_io[i].NewState == 2)  {
          #ifdef SPECIAL_BUTTON
            uint8_t SensorID = i == 0 ? SPECIAL_BUTTON_ID : SPECIAL_BUTTON_ID+1;
            send(MsgSTATUS.setSensor(SensorID).set(true));
          #endif
          
          common_io[i].NewState = common_io[i].State;
        }
        break;
      default:
        // Nothing to do here
        break;
    }
  }
}


/**
 * @brief Informs controller about power sensor readings
 * 
 * @param Current current measured by sensor 
 * @param Sensor sensor ID if more than one sensor is attached
 */
void PSUpdate(float Current, uint8_t Sensor = 0)  {

  if(Current == 0 && PS.OldValue == 0)  return;
  else if(Current < 1 && (abs(PS.OldValue - Current) < 0.1)) return;
  else if(Current >= 1 && (abs(PS.OldValue - Current) < (0.1 * PS.OldValue))) return;
  
  #if defined(POWER_SENSOR) && !defined(FOUR_RELAY)
    send(MsgWATT.setSensor(PS_ID).set(PS.CalculatePower(Current, COSFI), 0));
    PS.OldValue = Current;
  #elif defined(POWER_SENSOR) && defined(FOUR_RELAY)
    send(MsgWATT.setSensor(Sensor+4).set(PS[Sensor].CalculatePower(Current, COSFI), 0));
    PS[Sensor].OldValue = Current;
  #endif

}

/**
 * @brief Measures uC supply voltage
 * 
 * @return long measured voltage in mV
 */
long ReadVcc() {
  
  long result;
  
  // Read 1.1V reference against AVcc
  ADMUX = _BV(REFS0) | _BV(MUX3) | _BV(MUX2) | _BV(MUX1);
  
  delay(2);
  
  ADCSRA |= _BV(ADSC); // Convert
  
  while (bit_is_set(ADCSRA,ADSC));
  
  result = ADCL;
  result |= ADCH<<8;
  result = 1126400L / result; // Back-calculate AVcc in mV
  result = result;
  
  return result;
}

/**
 * @brief main loop: calls all 'Update' functions, runs all measurements, checks if safety parameters are within limits
 * 
 */
void loop() {

  float Vcc = ReadVcc(); // mV
  float Current = 0;

  // Sending out states for the first time (as required by Home Assistant)
  if (!InitConfirm)  {
    InitConfirmation();
  }

  // Reading power sensor(s)
  #if defined(POWER_SENSOR) && !defined(FOUR_RELAY)
    #if defined(DOUBLE_RELAY) || defined(ROLLER_SHUTTER)
      if (digitalRead(RELAY_1) == RELAY_ON || digitalRead(RELAY_2) == RELAY_ON)  {
        Current = PS.MeasureAC(Vcc);
      }
    #elif defined(DIMMER) || defined(RGB) || defined(RGBW)
      if (Dimmer.CurrentState)  {
        Current = PS.MeasureDC(Vcc);
      }
    #endif
      
    #ifdef ERROR_REPORTING
      OVERCURRENT_ERROR[0] = PS.ElectricalStatus(Current);
    #endif
    
    PSUpdate(Current);

  #elif defined(POWER_SENSOR) && defined(FOUR_RELAY)
    for (int i = RELAY_ID_1; i < RELAY_ID_1 + NUMBER_OF_RELAYS; i++) {
      if (common_io[i].State == RELAY_ON)  {
        Current = PS[i].MeasureAC(Vcc);
      }
      else  {
        Current = 0;
      }
      #ifdef ERROR_REPORTING
        OVERCURRENT_ERROR[i] = PS[i].ElectricalStatus(Current);
      #endif

      PSUpdate(Current, i);
    }
  #endif

  // Current safety
  #if defined(ERROR_REPORTING) && defined(POWER_SENSOR)
    #ifdef FOUR_RELAY
      for (int i = RELAY_ID_1; i < RELAY_ID_1 + NUMBER_OF_RELAYS; i++)  {
        if (OVERCURRENT_ERROR[i]) {
          // Current to high
          common_io[i].NewState = RELAY_OFF;
          common_io[i].SetRelay();
          send(MsgSTATUS.setSensor(i).set(common_io[i].NewState));
          send(MsgSTATUS.setSensor(ES_ID).set(OVERCURRENT_ERROR[i]));
          InformControllerES = true;
        }
        else if(!OVERCURRENT_ERROR[i] && InformControllerES) {
          // Current normal (only after reporting error)
          send(MsgSTATUS.setSensor(ES_ID).set(OVERCURRENT_ERROR[i]));
          InformControllerES = false;
        }
      }
    #else
      if(OVERCURRENT_ERROR[0])  {
        // Current to high
        #ifdef DOUBLE_RELAY
          for (int i = RELAY_ID_1; i < RELAY_ID_1 + NUMBER_OF_RELAYS; i++)  {
            common_io[i].NewState = RELAY_OFF;
            common_io[i].SetRelay();
            send(MsgSTATUS.setSensor(i).set(common_io[i].NewState));
          }
        #elif defined(ROLLER_SHUTTER)
          roller_shutter.stop();
        #elif defined(DIMMER) || defined(RGB) || defined(RGBW)
          //Dimmer.NewState = false;
          Dimmer.ChangeState(false);
          send(MsgSTATUS.setSensor(DIMMER_ID).set(Dimmer.CurrentState));
        #endif

        send(MsgSTATUS.setSensor(ES_ID).set(OVERCURRENT_ERROR[0]));
        InformControllerES = true;
      }
      else if(!OVERCURRENT_ERROR[0] && InformControllerES)  {
        // Current normal (only after reporting error)
        send(MsgSTATUS.setSensor(ES_ID).set(OVERCURRENT_ERROR[0]));
        InformControllerES = false;
      }
    #endif
  #endif

  // Reading internal temperature sensor
  #if defined(ERROR_REPORTING) && defined(INTERNAL_TEMP)
    THERMAL_ERROR = AnalogTemp.ThermalStatus(AnalogTemp.MeasureT(Vcc));
  #endif

  // Thermal safety
  #if defined(ERROR_REPORTING) && defined(INTERNAL_TEMP)
    if (THERMAL_ERROR && !InformControllerTS) {
    // Board temperature to high
      #ifdef DOUBLE_RELAY
        for (int i = RELAY_ID_1; i < RELAY_ID_1 + NUMBER_OF_RELAYS; i++)  {
          common_io[i].NewState = RELAY_OFF;
          common_io[i].SetRelay();
          send(MsgSTATUS.setSensor(i).set(common_io[i].NewState));
        }
      #elif defined(ROLLER_SHUTTER)
        roller_shutter.stop();
      #elif defined(DIMMER) || defined(RGB) || defined(RGBW)
        //Dimmer.NewState = false;
        Dimmer.ChangeState(false);
        send(MsgSTATUS.setSensor(DIMMER_ID).set(Dimmer.CurrentState));
      #endif
      send(MsgSTATUS.setSensor(TS_ID).set(THERMAL_ERROR));
      InformControllerTS = true;
      CheckNow = true;
    }
    else if (!THERMAL_ERROR && InformControllerTS) {
      send(MsgSTATUS.setSensor(TS_ID).set(THERMAL_ERROR));
      InformControllerTS = false;
    }
  #endif

  // Reading inputs / activating outputs
  if (NUMBER_OF_RELAYS + NUMBER_OF_INPUTS > 0) {
    UpdateIO();
  }

  // Updating roller shutter
  #ifdef ROLLER_SHUTTER
    roller_shutter.update(Current);
  #endif

  #if defined(DIMMER) || defined(RGB) || defined(RGBW)
    Dimmer.UpdateDimmer();
  #endif

  // Reset LastUpdate if millis() has overflowed
  if(LastUpdate > millis()) {
    LastUpdate = millis();
  }  
  
  // Checking out sensors which report at a defined interval
  if ((millis() > LastUpdate + INTERVAL) || CheckNow == true)  {
    #ifdef INTERNAL_TEMP
      send(MsgTEMP.setSensor(IT_ID).set((int)AnalogTemp.MeasureT(Vcc)));
    #endif
    #ifdef EXTERNAL_TEMP
      ETUpdate();
    #endif
    LastUpdate = millis();
    CheckNow = false;
  }

  wait(LOOP_TIME);
}
/*

   EOF

*/
