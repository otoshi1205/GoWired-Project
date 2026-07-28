/// Dimmer brightness/colour logic, including the two library defects this
/// refactor fixes: case-sensitive hex parsing and the non-terminating ramp.
#include "domain/dimmer.h"

#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeClock;
using test::FakePwm;

constexpr Pin kR = 5;
constexpr Pin kG = 9;
constexpr Pin kB = 6;
constexpr Pin kW = 10;

struct Fixture {
    FakePwm pwm;
    FakeClock clock;
    Pin pins[kMaxDimmerChannels] = {kR, kG, kB, kW};
    DimmerTuning tuning{/* step */ 1, /* interval_ms */ 1, /* toggle_step */ 20};

    Dimmer make(uint8_t channels = 3) { return Dimmer(pwm, clock, pins, channels, tuning); }
};

// ---------------------------------------------------------------------------
// Hex colour parsing
// ---------------------------------------------------------------------------

/// The original _StringHexToByte() did `c -= 7` for any character above '9',
/// which is right for 'A'-'F' and 32 off for 'a'-'f'. Controllers send lower
/// case, and init_confirmation() advertised "ffffff" itself.
TEST(DimmerColors, LowerCaseHexIsParsed)
{
    Fixture f;
    Dimmer d = f.make(3);

    ASSERT_TRUE(d.set_colors_from_hex("ff8000"));
    d.set_on(true);

    EXPECT_EQ(255, d.channel_value(0));
    EXPECT_EQ(128, d.channel_value(1));
    EXPECT_EQ(0, d.channel_value(2));
}

TEST(DimmerColors, UpperCaseHexStillParses)
{
    Fixture f;
    Dimmer d = f.make(3);
    ASSERT_TRUE(d.set_colors_from_hex("FF8000"));
    d.set_on(true);
    EXPECT_EQ(255, d.channel_value(0));
    EXPECT_EQ(128, d.channel_value(1));
}

TEST(DimmerColors, MixedCaseHexParses)
{
    Fixture f;
    Dimmer d = f.make(3);
    ASSERT_TRUE(d.set_colors_from_hex("aB12Cd"));
    d.set_on(true);
    EXPECT_EQ(0xAB, d.channel_value(0));
    EXPECT_EQ(0x12, d.channel_value(1));
    EXPECT_EQ(0xCD, d.channel_value(2));
}

/// "#RRGGBB" is seven characters; the original only special-cased the
/// nine-character "#RRGGBBWW" form and silently ignored this one.
TEST(DimmerColors, HashPrefixAcceptedForBothLengths)
{
    Fixture f;
    Dimmer rgb = f.make(3);
    EXPECT_TRUE(rgb.set_colors_from_hex("#ff8000"));

    Dimmer rgbw = f.make(4);
    EXPECT_TRUE(rgbw.set_colors_from_hex("#ff800040"));
}

TEST(DimmerColors, RgbwConsumesTheFourthByte)
{
    Fixture f;
    Dimmer d = f.make(4);
    ASSERT_TRUE(d.set_colors_from_hex("01020304"));
    d.set_on(true);
    EXPECT_EQ(4, d.channel_value(3));
}

TEST(DimmerColors, MalformedPayloadIsRejectedWithoutPartialApplication)
{
    Fixture f;
    Dimmer d = f.make(3);
    ASSERT_TRUE(d.set_colors_from_hex("112233"));
    d.set_on(true);
    ASSERT_EQ(0x11, d.channel_value(0));

    // Second byte is not hex: the whole payload must be refused, leaving the
    // previous colour intact rather than half-updated.
    EXPECT_FALSE(d.set_colors_from_hex("44zz66"));
    d.update();
    EXPECT_EQ(0x11, d.channel_value(0));
}

TEST(DimmerColors, WrongLengthIsRejected)
{
    Fixture f;
    Dimmer d = f.make(3);
    EXPECT_FALSE(d.set_colors_from_hex(""));
    EXPECT_FALSE(d.set_colors_from_hex("fff"));
    EXPECT_FALSE(d.set_colors_from_hex("fffffffff"));
    EXPECT_FALSE(d.set_colors_from_hex(nullptr));
}

// ---------------------------------------------------------------------------
// Brightness
// ---------------------------------------------------------------------------

TEST(DimmerLevel, TargetIsClampedToOneHundred)
{
    Fixture f;
    Dimmer d = f.make();
    d.set_target_level(140);
    EXPECT_EQ(100, d.target_level());
}

TEST(DimmerLevel, WallSwitchStepWrapsBackToTheStepSize)
{
    Fixture f;
    Dimmer d = f.make();
    d.set_target_level(90);

    d.bump_level(20);
    EXPECT_EQ(20, d.target_level()); // 110 wraps to the step, not to 100

    d.bump_level(20);
    EXPECT_EQ(40, d.target_level());
}

/// The original ramp loops spun on `current != target` while stepping by a
/// fixed amount, so a step size that did not divide the distance overshot and
/// looped until the watchdog fired. Anything but step 1 could hang the node.
TEST(DimmerLevel, RampTerminatesWhenTheStepDoesNotDivideTheDistance)
{
    FakePwm pwm;
    FakeClock clock;
    Pin pins[kMaxDimmerChannels] = {kR, kG, kB, kW};
    DimmerTuning coarse{/* step */ 7, /* interval_ms */ 0, /* toggle_step */ 20};

    Dimmer d(pwm, clock, pins, 3, coarse);
    d.set_target_level(25); // 7 does not divide 25
    d.set_on(true);         // ramps; must return

    EXPECT_EQ(25, d.level());
}

TEST(DimmerLevel, RampReachesTheTargetExactly)
{
    Fixture f;
    Dimmer d = f.make();
    d.set_target_level(37);
    d.set_on(true);
    EXPECT_EQ(37, d.level());
}

TEST(DimmerState, SwitchingOffThenOnRestoresTheRequestedBrightness)
{
    Fixture f;
    Dimmer d = f.make();
    d.set_target_level(60);
    d.set_on(true);
    ASSERT_EQ(60, d.level());

    d.set_on(false);
    EXPECT_FALSE(d.is_on());
    EXPECT_EQ(60, d.target_level());

    d.set_on(true);
    EXPECT_TRUE(d.is_on());
    EXPECT_EQ(60, d.level());
}

TEST(DimmerState, RedundantStateChangeIsANoOp)
{
    Fixture f;
    Dimmer d = f.make();
    d.set_on(false);
    EXPECT_TRUE(f.pwm.writes.empty());
}

TEST(DimmerState, UpdateDoesNothingWhileOff)
{
    Fixture f;
    Dimmer d = f.make();
    d.set_target_level(80);
    d.update();
    EXPECT_TRUE(f.pwm.writes.empty());
}

TEST(DimmerOutputs, DutyIsBrightnessScaledColour)
{
    Fixture f;
    Dimmer d = f.make(3);
    ASSERT_TRUE(d.set_colors_from_hex("ff0000"));
    d.set_target_level(50);
    d.set_on(true);

    // 50 % of 255 on the red channel, nothing on the others.
    EXPECT_EQ(127, f.pwm.duty.at(kR));
    EXPECT_EQ(0, f.pwm.duty.at(kG));
    EXPECT_EQ(0, f.pwm.duty.at(kB));
}

TEST(DimmerOutputs, OnlyConfiguredChannelsAreDriven)
{
    Fixture f;
    Dimmer d = f.make(3); // RGB: the white pin must stay untouched
    d.set_on(true);
    EXPECT_EQ(0u, f.pwm.duty.count(kW));
}

TEST(DimmerOutputs, BeginZeroesEveryChannel)
{
    Fixture f;
    Dimmer d = f.make(3);
    d.begin();
    EXPECT_EQ(0, f.pwm.duty.at(kR));
    EXPECT_EQ(0, f.pwm.duty.at(kG));
    EXPECT_EQ(0, f.pwm.duty.at(kB));
    EXPECT_FALSE(d.is_on());
}

} // namespace
} // namespace gw
