/// Threshold, deadband and fault-latching logic that used to live inside
/// GoWired-lib's PowerSensor/AnalogTemp where it could not be reached.
#include "domain/monitors.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

PowerMonitor make_monitor()
{
    return PowerMonitor(/* max_current_a */ 3, /* receiver_voltage */ 230, /* cos_phi */ 1.0f);
}

TEST(PowerMonitor, OverLimitIsStrictlyAboveTheConfiguredMaximum)
{
    const PowerMonitor m = make_monitor();
    EXPECT_FALSE(m.over_limit(0.0f));
    EXPECT_FALSE(m.over_limit(2.9f));
    EXPECT_FALSE(m.over_limit(3.0f)); // at the limit is still acceptable
    EXPECT_TRUE(m.over_limit(3.01f));
    EXPECT_TRUE(m.over_limit(12.0f));
}

TEST(PowerMonitor, PowerUsesVoltageAndPowerFactor)
{
    EXPECT_FLOAT_EQ(230.0f, make_monitor().power_w(1.0f));

    const PowerMonitor led(3, 24, 0.5f);
    EXPECT_FLOAT_EQ(24.0f, led.power_w(2.0f));
}

TEST(PowerMonitor, SilentWhenNothingIsDrawnAndNothingWasReported)
{
    EXPECT_FALSE(make_monitor().should_report(0.0f, 0.0f));
}

TEST(PowerMonitor, ReportsTheTransitionToZero)
{
    // A load being switched off must be published even though the new value is 0.
    EXPECT_TRUE(make_monitor().should_report(0.0f, 1.5f));
}

TEST(PowerMonitor, AbsoluteDeadbandBelowOneAmp)
{
    const PowerMonitor m = make_monitor();
    EXPECT_FALSE(m.should_report(0.52f, 0.50f)); // 20 mA of ADC noise
    EXPECT_TRUE(m.should_report(0.65f, 0.50f));  // 150 mA is real
}

TEST(PowerMonitor, RelativeDeadbandAboveOneAmp)
{
    const PowerMonitor m = make_monitor();
    // 10 % of 2 A is 200 mA, so 100 mA is noise here but would be signal below 1 A.
    EXPECT_FALSE(m.should_report(2.1f, 2.0f));
    EXPECT_TRUE(m.should_report(2.5f, 2.0f));
}

TEST(ThermalMonitor, TripsOnlyAboveTheLimit)
{
    const ThermalMonitor m(85);
    EXPECT_FALSE(m.over_limit(20.0f));
    EXPECT_FALSE(m.over_limit(85.0f));
    EXPECT_TRUE(m.over_limit(85.5f));
}

TEST(LatchedFault, ReportsEachTransitionExactlyOnce)
{
    LatchedFault f;
    EXPECT_FALSE(f.active());

    EXPECT_TRUE(f.update(true)); // fault appears
    EXPECT_TRUE(f.active());

    // The pre-refactor overcurrent path re-sent this on every loop iteration.
    EXPECT_FALSE(f.update(true));
    EXPECT_FALSE(f.update(true));

    EXPECT_TRUE(f.update(false)); // fault clears
    EXPECT_FALSE(f.active());
    EXPECT_FALSE(f.update(false));
}

TEST(LatchedFault, ControllerOverrideReArmsReporting)
{
    LatchedFault f;
    ASSERT_TRUE(f.update(true));

    // Controller clears the status child by hand.
    f.override_from_controller(false);
    EXPECT_FALSE(f.active());

    // The next genuine trip must be reported again.
    EXPECT_TRUE(f.update(true));
}

} // namespace
} // namespace gw
