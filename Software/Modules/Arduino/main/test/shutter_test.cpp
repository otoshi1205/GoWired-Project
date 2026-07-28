/// Shutter position arithmetic, travel-time persistence and relay interlock.
#include "domain/shutter.h"

#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeClock;
using test::FakeGpio;
using test::FakeStore;

constexpr Pin kUp = 5;
constexpr Pin kDown = 9;
constexpr bool kOff = false;

struct Fixture {
    FakeGpio gpio;
    FakeClock clock;
    FakeStore store;
    ShutterPins pins{kUp, kDown, kOff};
    StoreLayout layout;

    Shutter make() { return Shutter(gpio, clock, store, pins, layout); }

    void persist(uint8_t up_s, uint8_t down_s, uint8_t position)
    {
        store.cells[layout.shutter_up_time] = up_s;
        store.cells[layout.shutter_down_time] = down_s;
        store.cells[layout.shutter_position] = position;
    }
};

TEST(ShutterBegin, BlankEepromFallsBackToConfiguredTimesAndStaysUncalibrated)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);

    EXPECT_FALSE(s.calibrated());
    EXPECT_EQ(21, s.up_time_s());
    EXPECT_EQ(20, s.down_time_s());
    EXPECT_EQ(0, s.position());
}

TEST(ShutterBegin, PersistedValuesAreRestored)
{
    Fixture f;
    f.persist(30, 28, 45);
    Shutter s = f.make();
    s.begin(21, 20);

    EXPECT_TRUE(s.calibrated());
    EXPECT_EQ(30, s.up_time_s());
    EXPECT_EQ(28, s.down_time_s());
    EXPECT_EQ(45, s.position());
}

TEST(ShutterBegin, BothRelaysStartDeEnergised)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);

    EXPECT_EQ(PinMode::Output, f.gpio.mode.at(kUp));
    EXPECT_EQ(PinMode::Output, f.gpio.mode.at(kDown));
    EXPECT_EQ(kOff, f.gpio.level.at(kUp));
    EXPECT_EQ(kOff, f.gpio.level.at(kDown));
    EXPECT_EQ(ShutterMotion::Stopped, s.motion());
}

// ---------------------------------------------------------------------------
// Commands -> movement time (milliseconds)
// ---------------------------------------------------------------------------

TEST(ShutterCommands, FullTraverseUsesTheDirectionTime)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);

    EXPECT_EQ(21000u, s.request(ShutterMotion::Up));
    EXPECT_EQ(ShutterMotion::Up, s.pending());

    EXPECT_EQ(20000u, s.request(ShutterMotion::Down));
    EXPECT_EQ(0u, s.request(ShutterMotion::Stopped));
}

TEST(ShutterCommands, PositionRequestScalesByTheRemainingDistance)
{
    Fixture f;
    f.persist(20, 20, 0);
    Shutter s = f.make();
    s.begin(21, 20);

    // 0 -> 50 % of a 20 s downward traverse is 10 s.
    EXPECT_EQ(10000u, s.request_position(50));
    EXPECT_EQ(ShutterMotion::Down, s.pending());
}

TEST(ShutterCommands, PositionRequestPicksUpwardWhenOpening)
{
    Fixture f;
    f.persist(20, 20, 80);
    Shutter s = f.make();
    s.begin(21, 20);

    EXPECT_EQ(6000u, s.request_position(50)); // 30 % of 20 s
    EXPECT_EQ(ShutterMotion::Up, s.pending());
}

TEST(ShutterCommands, PositionRequestIsClampedAndNoOpsWhenAlreadyThere)
{
    Fixture f;
    f.persist(20, 20, 40);
    Shutter s = f.make();
    s.begin(21, 20);

    EXPECT_EQ(0u, s.request_position(40));
    EXPECT_EQ(ShutterMotion::Stopped, s.pending());

    s.request_position(500); // clamped to 100
    EXPECT_EQ(ShutterMotion::Down, s.pending());

    s.set_position(40);
    s.request_position(-20); // clamped to 0
    EXPECT_EQ(ShutterMotion::Up, s.pending());
}

TEST(ShutterCommands, ButtonForTheRunningDirectionStops)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);

    ASSERT_EQ(21000u, s.request_button(0)); // up
    s.apply();
    ASSERT_EQ(ShutterMotion::Up, s.motion());

    EXPECT_EQ(0u, s.request_button(0)); // same direction -> stop
    EXPECT_EQ(ShutterMotion::Stopped, s.pending());
}

TEST(ShutterCommands, ButtonForTheOtherDirectionReverses)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);

    s.request_button(0);
    s.apply();
    EXPECT_EQ(20000u, s.request_button(1));
    EXPECT_EQ(ShutterMotion::Down, s.pending());
}

// ---------------------------------------------------------------------------
// Relay driving
// ---------------------------------------------------------------------------

TEST(ShutterApply, EnergisesOnlyTheRequestedDirection)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);

    s.set_pending(ShutterMotion::Up);
    s.apply();
    EXPECT_EQ(!kOff, f.gpio.level.at(kUp));
    EXPECT_EQ(kOff, f.gpio.level.at(kDown));
    EXPECT_EQ(ShutterMotion::Up, s.motion());
}

/// Energising both directions at once would short the motor windings.
TEST(ShutterApply, BreaksBeforeMakingOnAReversal)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);

    s.set_pending(ShutterMotion::Up);
    s.apply();
    f.gpio.writes.clear();
    f.clock.delays.clear();

    s.set_pending(ShutterMotion::Down);
    s.apply();

    // The up relay must be released, with a settling delay, before down closes.
    ASSERT_GE(f.gpio.writes.size(), 2u);
    EXPECT_EQ(kUp, f.gpio.writes.front().pin);
    EXPECT_EQ(kOff, f.gpio.writes.front().high);
    EXPECT_EQ(kDown, f.gpio.writes.back().pin);
    EXPECT_EQ(!kOff, f.gpio.writes.back().high);
    ASSERT_EQ(1u, f.clock.delays.size());
    EXPECT_EQ(50u, f.clock.delays.front());

    EXPECT_EQ(kOff, f.gpio.level.at(kUp));
    EXPECT_EQ(!kOff, f.gpio.level.at(kDown));
}

TEST(ShutterApply, StopReleasesBothRelays)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);
    s.set_pending(ShutterMotion::Down);
    s.apply();

    s.set_pending(ShutterMotion::Stopped);
    s.apply();
    EXPECT_EQ(kOff, f.gpio.level.at(kUp));
    EXPECT_EQ(kOff, f.gpio.level.at(kDown));
}

// ---------------------------------------------------------------------------
// Position integration
// ---------------------------------------------------------------------------

TEST(ShutterPosition, FullDownwardTraverseClosesCompletely)
{
    Fixture f;
    f.persist(20, 20, 0);
    Shutter s = f.make();
    s.begin(21, 20);

    s.advance(ShutterMotion::Down, 20000);
    EXPECT_EQ(100, s.position());
}

TEST(ShutterPosition, PartialTraverseIsProportional)
{
    Fixture f;
    f.persist(20, 20, 0);
    Shutter s = f.make();
    s.begin(21, 20);

    s.advance(ShutterMotion::Down, 5000); // a quarter of 20 s
    EXPECT_EQ(25, s.position());
}

TEST(ShutterPosition, UpwardMovementOpens)
{
    Fixture f;
    f.persist(20, 20, 100);
    Shutter s = f.make();
    s.begin(21, 20);

    s.advance(ShutterMotion::Up, 10000);
    EXPECT_EQ(50, s.position());
}

TEST(ShutterPosition, IsClampedAtBothEndStops)
{
    Fixture f;
    f.persist(20, 20, 90);
    Shutter s = f.make();
    s.begin(21, 20);

    s.advance(ShutterMotion::Down, 60000); // far past closed
    EXPECT_EQ(100, s.position());

    s.advance(ShutterMotion::Up, 60000);
    EXPECT_EQ(0, s.position());
}

TEST(ShutterPosition, PersistenceRoundTrips)
{
    Fixture f;
    f.persist(20, 20, 0);
    {
        Shutter s = f.make();
        s.begin(21, 20);
        s.advance(ShutterMotion::Down, 10000);
        s.persist_position();
    }

    Shutter reloaded = f.make();
    reloaded.begin(21, 20);
    EXPECT_EQ(50, reloaded.position());
}

/// EEPROM endurance: re-persisting an unchanged position must not burn a cycle.
TEST(ShutterPosition, UnchangedPositionIsNotRewritten)
{
    Fixture f;
    f.persist(20, 20, 50);
    Shutter s = f.make();
    s.begin(21, 20);

    f.store.write_count = 0;
    s.persist_position();
    s.persist_position();
    EXPECT_EQ(0u, f.store.write_count);
}

TEST(ShutterCalibration, StoringTravelTimesMarksItCalibratedAndPersists)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(21, 20);
    ASSERT_FALSE(s.calibrated());

    s.set_travel_times(25, 24);
    EXPECT_TRUE(s.calibrated());
    EXPECT_EQ(25, f.store.read(f.layout.shutter_up_time));
    EXPECT_EQ(24, f.store.read(f.layout.shutter_down_time));
}

/// An uncalibrated shutter has no basis for position arithmetic; it must not
/// divide by a zero travel time.
TEST(ShutterPosition, ZeroTravelTimeLeavesPositionUntouched)
{
    Fixture f;
    Shutter s = f.make();
    s.begin(0, 0);
    s.advance(ShutterMotion::Down, 5000);
    EXPECT_EQ(0, s.position());
}

} // namespace
} // namespace gw
