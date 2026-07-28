/// Debounce and longpress classification, ported from CommonIO::CheckInput().
#include "domain/input.h"

#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeClock;
using test::FakeGpio;

constexpr Pin kPin = 4;
constexpr uint16_t kLongpressMs = 1000;
constexpr uint8_t kDebounceMs = 50;

struct ButtonFixture {
    FakeGpio gpio;
    FakeClock clock;
    Button button{gpio, clock, kPin, false, kLongpressMs, kDebounceMs};

    ButtonFixture() { button.begin(); }
};

TEST(Button, ConfiguresItsPinAsPulledUpInput)
{
    ButtonFixture f;
    EXPECT_EQ(PinMode::InputPullup, f.gpio.mode.at(kPin));
}

TEST(Button, IdleInputProducesNoEvent)
{
    ButtonFixture f;
    f.gpio.script_idle(kPin);
    EXPECT_EQ(ButtonEvent::None, f.button.poll());
}

TEST(Button, ShortPressToggles)
{
    ButtonFixture f;
    f.gpio.script_short_press(kPin);
    EXPECT_EQ(ButtonEvent::Toggle, f.button.poll());
}

TEST(Button, HoldingPastTheThresholdIsALongPress)
{
    ButtonFixture f;
    f.gpio.script_hold(kPin);
    EXPECT_EQ(ButtonEvent::LongPress, f.button.poll());
}

/// The release latch: holding the button must not emit a Toggle on every pass
/// of the main loop.
TEST(Button, SecondPressIsIgnoredUntilAReleaseIsObserved)
{
    ButtonFixture f;

    f.gpio.script_short_press(kPin);
    ASSERT_EQ(ButtonEvent::Toggle, f.button.poll());

    // Still held down: no new event, and the latch stays closed.
    f.gpio.script_hold(kPin);
    EXPECT_EQ(ButtonEvent::None, f.button.poll());

    // Released: still no event, but the latch re-arms.
    f.gpio.script_idle(kPin);
    EXPECT_EQ(ButtonEvent::None, f.button.poll());

    f.gpio.script_short_press(kPin);
    EXPECT_EQ(ButtonEvent::Toggle, f.button.poll());
}

TEST(Button, ShortSpikeIsDebouncedAway)
{
    ButtonFixture f;
    // A single active sample followed by release never satisfies the debounce
    // window, so nothing is reported.
    f.gpio.script[kPin] = {false, true, true, true};
    EXPECT_EQ(ButtonEvent::None, f.button.poll());
}

TEST(Button, SurvivesMillisRollover)
{
    ButtonFixture f;
    f.clock.now = 0xFFFFFF00; // wraps during the poll
    f.gpio.script_short_press(kPin);
    EXPECT_EQ(ButtonEvent::Toggle, f.button.poll());
}

// ---------------------------------------------------------------------------
// DigitalSensor
// ---------------------------------------------------------------------------

struct SensorFixture {
    FakeGpio gpio;
    FakeClock clock;
    DigitalSensor sensor{gpio, clock, kPin, false, true, kDebounceMs};

    SensorFixture() { sensor.begin(); }
};

TEST(DigitalSensor, PulledUpVariantConfiguresPullup)
{
    SensorFixture f;
    EXPECT_EQ(PinMode::InputPullup, f.gpio.mode.at(kPin));
}

TEST(DigitalSensor, FloatingVariantDoesNotConfigurePullup)
{
    FakeGpio gpio;
    FakeClock clock;
    DigitalSensor motion{gpio, clock, kPin, false, /* pullup */ false, kDebounceMs};
    motion.begin();
    EXPECT_EQ(PinMode::Input, gpio.mode.at(kPin));
}

TEST(DigitalSensor, ReportsOnlyOnChange)
{
    SensorFixture f;
    bool level = false;

    f.gpio.script_idle(kPin);
    EXPECT_FALSE(f.sensor.poll(level)); // already inactive

    f.gpio.script[kPin] = {false}; // held active
    ASSERT_TRUE(f.sensor.poll(level));
    EXPECT_TRUE(level);
    EXPECT_TRUE(f.sensor.level());

    EXPECT_FALSE(f.sensor.poll(level)); // unchanged

    f.gpio.script_idle(kPin);
    ASSERT_TRUE(f.sensor.poll(level));
    EXPECT_FALSE(level);
}

TEST(DigitalSensor, InvertedPolarityFlipsTheActiveLevel)
{
    FakeGpio gpio;
    FakeClock clock;
    DigitalSensor inverted{gpio, clock, kPin, /* invert */ true, true, kDebounceMs};
    inverted.begin();

    bool level = false;
    gpio.script[kPin] = {true}; // HIGH is active when inverted
    ASSERT_TRUE(inverted.poll(level));
    EXPECT_TRUE(level);
}

} // namespace
} // namespace gw
