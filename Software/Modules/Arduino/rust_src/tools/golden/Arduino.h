// Minimal stand-in so MyMessage.cpp can be compiled on the host purely to
// generate reference frames. Nothing here affects the wire format.
#pragma once
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
class __FlashStringHelper;
#define PROGMEM
#define PSTR(s) (s)
static inline char* dtostrf(double v, signed char w, unsigned char p, char* s) {
    char fmt[16]; snprintf(fmt, sizeof fmt, "%%%d.%df", (int)w, (int)p);
    sprintf(s, fmt, v); return s;
}
static inline char* itoa_(int v, char* s, int b){ (void)b; sprintf(s, "%d", v); return s; }
#define itoa(v,s,b) itoa_((int)(v),(s),(b))
static inline char* ltoa_(long v, char* s, int b){ (void)b; sprintf(s, "%ld", v); return s; }
#define ltoa(v,s,b) ltoa_((long)(v),(s),(b))
static inline char* utoa_(unsigned v, char* s, int b){ (void)b; sprintf(s, "%u", v); return s; }
#define utoa(v,s,b) utoa_((unsigned)(v),(s),(b))
static inline char* ultoa_(unsigned long v, char* s, int b){ (void)b; sprintf(s, "%lu", v); return s; }
#define ultoa(v,s,b) ultoa_((unsigned long)(v),(s),(b))
#define GATEWAY_ADDRESS ((uint8_t)0)
#define NODE_SENSOR_ID  ((uint8_t)255)
#define BROADCAST_ADDRESS ((uint8_t)255)
template <class T> static inline T min(T a, T b) { return a < b ? a : b; }
