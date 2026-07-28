/// The movement state machine that used to live in ActiveRollerShutter.
#include "domain/roller_shutter_device.h"

#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeBus;
using test::FakeClock;
using test::FakeCurrentSensor;
using test::FakeGpio;
using test::FakeStore;
using test::FakeWatchdog;
using test::inbound;

constexpr Pin kUp = 5;
constexpr Pin kDown = 9;
constexpr Pin kButton1 = 2;
constexpr Pin kButton2 = 3;
constexpr float kIdle = 0.0f;   // motor drawing nothing
constexpr float kMoving = 1.5f; // motor under load

struct Fixture {
    FakeGpio gpio;
    FakeClock clock;
    FakeStore store;
    FakeBus bus;
    FakeCurrentSensor sensor;
    FakeWatchdog watchdog;
    ButtonTiming buttons;
    StoreLayout layout;
    SafetyState safety;

    RollerShutterDevice::Spec spec(bool current_sensing = true)
    {
        RollerShutterDevice::Spec s;
        s.pins = {kUp, kDown, false};
        s.button_pins[0] = kButton1;
        s.button_pins[1] = kButton2;
        s.current_floor = 0.2f;
        s.calibration_samples = 1;
        s.default_up_time_s = 20;
        s.default_down_time_s = 20;
        s.current_sensing = current_sensing;
        return s;
    }

    RollerShutterDevice make(bool current_sensing = true)
    {
        return RollerShutterDevice(gpio, clock, store, spec(current_sensing), layout, buttons, true);
    }

    void persist(uint8_t up_s, uint8_t down_s, uint8_t position)
    {
        store.cells[layout.shutter_up_time] = up_s;
        store.cells[layout.shutter_down_time] = down_s;
        store.cells[layout.shutter_position] = position;
    }

    void idle_buttons()
    {
        gpio.script_idle(kButton1);
        gpio.script_idle(kButton2);
    }
};

TEST(RollerShutterDevice, PresentsASingleCoverChild)
{
    Fixture f;
    RollerShutterDevice d = f.make();
    d.present(f.bus, 10);

    ASSERT_EQ(1u, f.bus.presented.size());
    EXPECT_EQ(0, f.bus.presented[0].sensor);
    EXPECT_EQ(SensorClass::Cover, f.bus.presented[0].type);
    EXPECT_EQ("Roller Shutter", f.bus.presented[0].name);
}

TEST(RollerShutterDevice, InitialStateCoversUpDownStopAndPosition)
{
    Fixture f;
    f.persist(20, 20, 30);
    RollerShutterDevice d = f.make();
    d.begin();
    d.send_initial_state(f.bus, 2000);

    EXPECT_EQ(1u, f.bus.count(0, ValueType::Up));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Down));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Stop));
    ASSERT_EQ(1u, f.bus.count(0, ValueType::Percentage));
    EXPECT_EQ(30u, f.bus.matching(0, ValueType::Percentage)[0].uint_value);
}

TEST(RollerShutterDevice, DownCommandStartsTheMotorAndAnnouncesIt)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();

    ASSERT_TRUE(d.handle(inbound(0, ValueType::Down), f.bus, f.safety));
    d.tick(f.bus, kMoving);

    EXPECT_EQ(ShutterMotion::Down, d.shutter().motion());
    EXPECT_EQ(!false, f.gpio.level.at(kDown));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Down));
}

/// This is the behaviour that bug #1 destroyed. With the current reading
/// clobbered to zero, `current < current_floor` was true on the very next
/// iteration and the shutter braked immediately after starting.
TEST(RollerShutterDevice, KeepsMovingWhileTheMotorDrawsCurrent)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving); // starts
    ASSERT_EQ(ShutterMotion::Down, d.shutter().motion());

    d.tick(f.bus, kMoving);
    d.tick(f.bus, kMoving);
    EXPECT_EQ(ShutterMotion::Down, d.shutter().motion());
}

TEST(RollerShutterDevice, CurrentDroppingToZeroMeansAnEndStopWasReached)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving);
    f.bus.clear();

    d.tick(f.bus, kIdle);

    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
    EXPECT_EQ(false, f.gpio.level.at(kDown));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Stop));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Percentage));
}

/// Without a current sensor there is no end-stop signal, so travel must be
/// governed purely by the configured times.
TEST(RollerShutterDevice, WithoutCurrentSensingZeroCurrentDoesNotStopIt)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make(/* current_sensing */ false);
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kIdle);
    ASSERT_EQ(ShutterMotion::Down, d.shutter().motion());

    d.tick(f.bus, kIdle);
    EXPECT_EQ(ShutterMotion::Down, d.shutter().motion());
}

TEST(RollerShutterDevice, StopsOnceTheMovementTimeElapses)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make(/* current_sensing */ false);
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving);
    ASSERT_EQ(ShutterMotion::Down, d.shutter().motion());

    f.clock.now += 21000; // past the 20 s traverse
    d.tick(f.bus, kMoving);

    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
    EXPECT_EQ(100, d.shutter().position());
}

TEST(RollerShutterDevice, ExplicitStopCommandBrakes)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving);
    f.bus.clear();

    ASSERT_TRUE(d.handle(inbound(0, ValueType::Stop), f.bus, f.safety));
    d.tick(f.bus, kMoving);

    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Stop));
}

TEST(RollerShutterDevice, ReversingBrakesThenResumesTheOtherWay)
{
    Fixture f;
    f.persist(20, 20, 50);
    RollerShutterDevice d = f.make();
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving);
    ASSERT_EQ(ShutterMotion::Down, d.shutter().motion());
    f.bus.clear();

    d.handle(inbound(0, ValueType::Up), f.bus, f.safety);
    d.tick(f.bus, kMoving);

    // One tick brakes and restarts upward; both relays were never on together.
    EXPECT_EQ(ShutterMotion::Up, d.shutter().motion());
    EXPECT_EQ(false, f.gpio.level.at(kDown));
    EXPECT_EQ(true, f.gpio.level.at(kUp));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Stop));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Up));
}

TEST(RollerShutterDevice, PositionIsPersistedWhenMovementEnds)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make(/* current_sensing */ false);
    d.begin();
    d.handle(inbound(0, ValueType::Percentage, false, 50), f.bus, f.safety);
    d.tick(f.bus, kMoving);

    f.clock.now += 10000;
    d.tick(f.bus, kMoving);

    EXPECT_EQ(50, d.shutter().position());
    EXPECT_EQ(50, f.store.read(f.layout.shutter_position));
}

TEST(RollerShutterDevice, ButtonStartsMovementAndSecondPressStops)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();

    f.gpio.script_short_press(kButton1);
    f.gpio.script_idle(kButton2);
    d.poll_buttons(f.bus, f.safety);
    d.tick(f.bus, kMoving);
    ASSERT_EQ(ShutterMotion::Up, d.shutter().motion());

    // Release, then press the same button again.
    f.idle_buttons();
    d.poll_buttons(f.bus, f.safety);
    f.gpio.script_short_press(kButton1);
    f.gpio.script_idle(kButton2);
    d.poll_buttons(f.bus, f.safety);
    d.tick(f.bus, kMoving);

    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
}

TEST(RollerShutterDevice, LongPressNotifiesTheSpecialButtonInsteadOfMoving)
{
    Fixture f;
    RollerShutterDevice d = f.make();
    d.begin();

    f.gpio.script_hold(kButton1);
    f.gpio.script_idle(kButton2);
    d.poll_buttons(f.bus, f.safety);

    EXPECT_EQ(1u, f.bus.count(ids::kSpecialButton1, ValueType::Status));
    d.tick(f.bus, kMoving);
    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
}

TEST(RollerShutterDevice, FaultStopsTheMotorAndReportsPosition)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving);
    f.bus.clear();

    f.safety.thermal_fault = true;
    d.shed_load(f.bus, f.safety);

    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Stop));

    // Idempotent while the fault persists.
    d.shed_load(f.bus, f.safety);
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Stop));
}

TEST(RollerShutterDevice, DrawsCurrentOnlyWhileMoving)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();
    EXPECT_FALSE(d.draws_current(0));

    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving);
    EXPECT_TRUE(d.draws_current(0));
}

TEST(RollerShutterDevice, CalibrationMeasuresBothDirectionsAndPersistsThem)
{
    Fixture f;
    RollerShutterDevice d = f.make();
    d.begin();
    ASSERT_FALSE(d.shutter().calibrated());

    // The motor draws current for a while in each leg, then falls idle.
    f.sensor.ac_script = {kMoving, kMoving, kIdle};
    ASSERT_TRUE(d.calibrate(f.bus, f.sensor, f.watchdog, 5000.0f));

    EXPECT_TRUE(d.shutter().calibrated());
    EXPECT_GT(f.watchdog.pets, 0u); // long blocking walk must pet the watchdog
    EXPECT_EQ(0, d.shutter().position());
    EXPECT_NE(0xFF, f.store.read(f.layout.shutter_up_time));
    EXPECT_NE(0xFF, f.store.read(f.layout.shutter_down_time));
    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
}

TEST(RollerShutterDevice, CalibrationIsRefusedWithoutACurrentSensor)
{
    Fixture f;
    RollerShutterDevice d = f.make(/* current_sensing */ false);
    d.begin();
    EXPECT_FALSE(d.calibrate(f.bus, f.sensor, f.watchdog, 5000.0f));
}

TEST(RollerShutterDevice, MaintenanceStopsTheMotorFirst)
{
    Fixture f;
    f.persist(20, 20, 0);
    RollerShutterDevice d = f.make();
    d.begin();
    d.handle(inbound(0, ValueType::Down), f.bus, f.safety);
    d.tick(f.bus, kMoving);

    d.prepare_for_maintenance();
    EXPECT_EQ(ShutterMotion::Stopped, d.shutter().motion());
    EXPECT_EQ(false, f.gpio.level.at(kDown));
}

} // namespace
} // namespace gw
