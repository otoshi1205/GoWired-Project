#pragma once

#include <ArduinoSTL.h>
#include <array>
#include <core/CommonIO.h>
#include "Configuration.h"

using CommonIOPins = std::array<CommonIO, NUMBER_OF_RELAYS + NUMBER_OF_INPUTS>;