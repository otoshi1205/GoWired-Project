/// DimmerDevice covers DIMMER, RGB and RGBW -- three former classes, one now.
#include "domain/dimmer_device.h"

#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeBus;
using test::FakeClock;
using test::FakeGpio;
using test::FakePwm;
using test::inbound;

constexpr Pin kButton1 = 2;
constexpr Pin kButton2 = 3;

DimmerDevice::Spec spec_for(ColorModel model)
{
    DimmerDevice::Spec s;
    s.model = model;
    s.led_pins[0] = 10;
    s.led_pins[1] = 5;
    s.led_pins[2] = 9;
    s.led_pins[3] = 6;
    s.button_pins[0] = kButton1;
    s.button_pins[1] = kButton2;
    return s;
}

struct Fixture {
    FakePwm pwm;
    FakeGpio gpio;
    FakeClock clock;
    FakeBus bus;
    DimmerTuning tuning{1, 0, 20};
    ButtonTiming buttons;
    SafetyState safety;

    DimmerDevice make(ColorModel model, bool special = true)
    {
        return DimmerDevice(pwm, gpio, clock, spec_for(model), tuning, buttons, special);
    }
};

TEST(DimmerDevicePresentation, WhiteDimmerPresentsAsSDimmer)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::White);
    d.present(f.bus, 10);

    ASSERT_EQ(1u, f.bus.presented.size());
    EXPECT_EQ(0, f.bus.presented[0].sensor);
    EXPECT_EQ(SensorClass::Dimmer, f.bus.presented[0].type);
    EXPECT_EQ("Dimmer", f.bus.presented[0].name);
}

TEST(DimmerDevicePresentation, RgbAndRgbwPresentTheirOwnClasses)
{
    Fixture f;
    DimmerDevice rgb = f.make(ColorModel::Rgb);
    rgb.present(f.bus, 10);
    EXPECT_EQ(SensorClass::RgbLight, f.bus.presented[0].type);
    EXPECT_EQ("RGB", f.bus.presented[0].name);

    f.bus.clear();
    DimmerDevice rgbw = f.make(ColorModel::Rgbw);
    rgbw.present(f.bus, 10);
    EXPECT_EQ(SensorClass::RgbwLight, f.bus.presented[0].type);
    EXPECT_EQ("RGBW", f.bus.presented[0].name);
}

TEST(DimmerDeviceInitialState, WhiteAdvertisesNoColourChild)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::White);
    d.send_initial_state(f.bus, 2000);

    EXPECT_EQ(0u, f.bus.count(0, ValueType::Rgb));
    EXPECT_EQ(0u, f.bus.count(0, ValueType::Rgbw));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Status));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Percentage));
}

TEST(DimmerDeviceInitialState, RgbAdvertisesSixHexDigits)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.send_initial_state(f.bus, 2000);

    ASSERT_EQ(1u, f.bus.count(0, ValueType::Rgb));
    EXPECT_EQ("ffffff", f.bus.matching(0, ValueType::Rgb)[0].text);
}

TEST(DimmerDeviceInitialState, RgbwAdvertisesEightHexDigits)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgbw);
    d.send_initial_state(f.bus, 2000);

    ASSERT_EQ(1u, f.bus.count(0, ValueType::Rgbw));
    EXPECT_EQ("ffffffff", f.bus.matching(0, ValueType::Rgbw)[0].text);
}

TEST(DimmerDeviceMessages, StatusSwitchesTheStrip)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();

    EXPECT_TRUE(d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety));
    EXPECT_TRUE(d.dimmer().is_on());

    EXPECT_TRUE(d.handle(inbound(0, ValueType::Status, false), f.bus, f.safety));
    EXPECT_FALSE(d.dimmer().is_on());
}

TEST(DimmerDeviceMessages, PercentageSetsAndClampsBrightness)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();

    d.handle(inbound(0, ValueType::Percentage, false, 65), f.bus, f.safety);
    EXPECT_EQ(65, d.dimmer().target_level());

    d.handle(inbound(0, ValueType::Percentage, false, 900), f.bus, f.safety);
    EXPECT_EQ(100, d.dimmer().target_level());

    d.handle(inbound(0, ValueType::Percentage, false, -5), f.bus, f.safety);
    EXPECT_EQ(0, d.dimmer().target_level());
}

/// The colour path is the one the upper/lower-case hex defect broke.
TEST(DimmerDeviceMessages, RgbPayloadFromAControllerIsApplied)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();

    ASSERT_TRUE(d.handle(inbound(0, ValueType::Rgb, false, 0, "0080ff"), f.bus, f.safety));
    d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety);

    EXPECT_EQ(0x00, d.dimmer().channel_value(0));
    EXPECT_EQ(0x80, d.dimmer().channel_value(1));
    EXPECT_EQ(0xFF, d.dimmer().channel_value(2));
}

TEST(DimmerDeviceMessages, ForeignChildrenAndTypesAreNotClaimed)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();

    EXPECT_FALSE(d.handle(inbound(7, ValueType::Status, true), f.bus, f.safety));
    EXPECT_FALSE(d.handle(inbound(0, ValueType::Watt, false, 10), f.bus, f.safety));
}

TEST(DimmerDeviceButtons, FirstButtonTogglesAndReports)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();

    f.gpio.script_short_press(kButton1);
    f.gpio.script_idle(kButton2);
    d.poll_buttons(f.bus, f.safety);

    EXPECT_TRUE(d.dimmer().is_on());
    ASSERT_EQ(1u, f.bus.count(0, ValueType::Status));
    EXPECT_TRUE(f.bus.matching(0, ValueType::Status)[0].boolean);
}

TEST(DimmerDeviceButtons, SecondButtonStepsBrightnessWhileOn)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();
    d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety);
    const uint8_t before = d.dimmer().target_level();
    f.bus.clear();

    f.gpio.script_idle(kButton1);
    f.gpio.script_short_press(kButton2);
    d.poll_buttons(f.bus, f.safety);

    EXPECT_EQ(before + f.tuning.toggle_step, d.dimmer().target_level());
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Percentage));
}

TEST(DimmerDeviceButtons, SecondButtonIsInertWhileOff)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();
    const uint8_t before = d.dimmer().target_level();

    f.gpio.script_idle(kButton1);
    f.gpio.script_short_press(kButton2);
    d.poll_buttons(f.bus, f.safety);

    EXPECT_EQ(before, d.dimmer().target_level());
    EXPECT_TRUE(f.bus.sent.empty());
}

TEST(DimmerDeviceButtons, EachButtonHasItsOwnLongpressChild)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();

    f.gpio.script_hold(kButton1);
    f.gpio.script_idle(kButton2);
    d.poll_buttons(f.bus, f.safety);
    EXPECT_EQ(1u, f.bus.count(ids::kSpecialButton1, ValueType::Status));

    f.bus.clear();
    f.gpio.script_idle(kButton1);
    f.gpio.script_hold(kButton2);
    d.poll_buttons(f.bus, f.safety);
    EXPECT_EQ(1u, f.bus.count(ids::kSpecialButton2, ValueType::Status));
}

TEST(DimmerDeviceSafety, ShedLoadSwitchesOffAndReportsOnce)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();
    d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety);
    f.bus.clear();

    d.shed_load(f.bus, f.safety);
    EXPECT_FALSE(d.dimmer().is_on());
    ASSERT_EQ(1u, f.bus.count(0, ValueType::Status));
    EXPECT_FALSE(f.bus.matching(0, ValueType::Status)[0].boolean);

    // Runs every iteration while the fault holds; must stay quiet.
    d.shed_load(f.bus, f.safety);
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Status));
}

TEST(DimmerDeviceCurrent, LedStripsAreMeasuredAsDcAndOnlyWhileLit)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();

    EXPECT_TRUE(d.uses_dc_measurement());
    EXPECT_EQ(1, d.power_channel_count());
    EXPECT_FALSE(d.draws_current(0));

    d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety);
    EXPECT_TRUE(d.draws_current(0));
}

TEST(DimmerDeviceChannels, RgbLeavesTheWhitePinAlone)
{
    Fixture f;
    DimmerDevice d = f.make(ColorModel::Rgb);
    d.begin();
    d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety);

    // led_pins[3] is the fourth channel, unused by a 3-channel model.
    EXPECT_EQ(0u, f.pwm.duty.count(6));
}

} // namespace
} // namespace gw
