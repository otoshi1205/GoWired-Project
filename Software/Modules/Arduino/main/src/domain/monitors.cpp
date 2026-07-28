#include "monitors.h"

namespace gw {

namespace {

float absolute(float v)
{
    return v < 0.0f ? -v : v;
}

} // namespace

bool PowerMonitor::should_report(float amps, float last_reported) const
{
    if (amps == 0.0f && last_reported == 0.0f) {
        return false;
    }
    const float delta = absolute(last_reported - amps);
    if (amps < 1.0f) {
        return delta >= 0.1f;
    }
    return delta >= 0.1f * last_reported;
}

} // namespace gw
