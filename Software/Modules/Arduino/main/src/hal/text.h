/**
 * @file text.h
 * @brief Portable "this string lives in flash" reference.
 *
 * On AVR, a plain string literal is copied into SRAM at startup (.data), so the
 * child names this sketch presents would permanently occupy a few hundred bytes
 * of a 2 KB budget. PSTR keeps them in program memory instead, and MySensors
 * has __FlashStringHelper overloads of present(), sendSketchInfo() and
 * MyMessage::set() to consume them.
 *
 * On a host build TextRef degrades to const char*, so the domain layer and its
 * tests are unaffected.
 *
 * GW_TEXT expands to a GCC statement expression and so may only be used inside
 * a function body, not in a namespace-scope initialiser.
 */
#pragma once

#if defined(__AVR__)

#include <WString.h>
#include <avr/pgmspace.h>

namespace gw {
using TextRef = const __FlashStringHelper*;
}

#define GW_TEXT(literal) (reinterpret_cast<gw::TextRef>(PSTR(literal)))

#else

namespace gw {
using TextRef = const char*;
}

#define GW_TEXT(literal) (literal)

#endif
