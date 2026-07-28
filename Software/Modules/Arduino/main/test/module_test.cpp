/// Module orchestration: what used to be setup(), presentation(),
/// InitConfirmation(), receive(), PSUpdate(), ETUpdate() and loop().
///
/// The headline test here is OvercurrentProtection.TripsWhenTheLimitIsExceeded,
/// which is the regression test for the defect that left the shipped default
/// configuration with no overcurrent protection at all.
#include "domain/module.h"

#include "domain/relay_bank_device.h"
#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeBus;
using test::FakeClock;
using test::FakeCurrentSensor;
using test::FakeGpio;
using test::FakeHygrometer;
using test::FakeStore;
using test::FakeTemperatureSensor;
using test::FakeVoltageReference;
using test::FakeWatchdog;
using test::inbound;

constexpr Pin kRelay1 = 5;
constexpr Pin kRelay2 = 9;
constexpr Pin kButton1 = 2;
constexpr Pin kButton2 = 3;
constexpr Pin kInputPin1 = 4;

struct Fixture {
    FakeGpio gpio;
    FakeClock clock;
    FakeStore store;
    FakeBus bus;
    FakeWatchdog watchdog;
    FakeVoltageReference vref;
    FakeCurrentSensor current;
    FakeTemperatureSensor temperature;
    FakeHygrometer probe;

    ButtonTiming button_timing;
    Timing timing;
    PowerTuning power;
    ThermalTuning thermal;
    StoreLayout layout;
    Features features;

    InputPin input_pins[kMaxInputs] = {
        {2, kInputPin1, true, true, false},
        {3, 14, true, true, false},
        {4, 15, true, true, false},
        {5, 16, true, true, false},
    };

    void enable_inputs(uint8_t n)
    {
        for (uint8_t i = 0; i < kMaxInputs; ++i) {
            input_pins[i].enabled = i < n;
        }
    }

    RelayBankSpec relay_spec()
    {
        RelayBankSpec s;
        s.relay_count = 2;
        s.relay_pins[0] = kRelay1;
        s.relay_pins[1] = kRelay2;
        s.button_count = 2;
        s.button_pins[0] = kButton1;
        s.button_pins[1] = kButton2;
        return s;
    }

    Fixture()
    {
        features.power_sensor = true;
        features.internal_temperature = true;
        features.error_reporting = true;
        features.special_button = true;
        idle_all();
    }

    /// Nothing pressed, so poll_buttons() and the input bank stay quiet.
    void idle_all()
    {
        gpio.script_idle(kButton1);
        gpio.script_idle(kButton2);
        for (uint8_t i = 0; i < kMaxInputs; ++i) {
            gpio.script_idle(input_pins[i].pin);
        }
    }

    Peripherals peripherals(uint8_t power_channels = 1)
    {
        Peripherals p;
        if (features.power_sensor) {
            p.power.count = power_channels;
            for (uint8_t ch = 0; ch < power_channels; ++ch) {
                p.power.sensor[ch] = &current;
                p.power.id[ch] = power_channels > 1 ? ids::kPowerPerRelay[ch] : ids::kPower;
            }
        }
        if (features.internal_temperature) {
            p.internal_temperature = &temperature;
        }
        if (features.external_temperature) {
            p.external_probe = &probe;
        }
        return p;
    }
};

/// Owns a device plus a Module wired to it.
struct Node {
    Fixture& f;
    RelayBankDevice device;
    InputBank inputs;
    Module module;

    /// Enable/disable inputs on the Fixture *before* constructing a Node --
    /// InputBank snapshots the pin table in its constructor.
    explicit Node(Fixture& fx, uint8_t power_channels = 1)
        : f(fx), device(fx.gpio, fx.clock, fx.relay_spec(), fx.button_timing, true),
          inputs(fx.gpio, fx.clock, fx.input_pins, fx.button_timing.debounce_ms),
          module(device, inputs, fx.bus, fx.clock, fx.store, fx.watchdog, fx.vref,
                 fx.peripherals(power_channels), fx.features, fx.timing, fx.power, fx.thermal,
                 fx.layout)
    {
    }
};

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

TEST(ModuleBegin, EnablesTheWatchdogAndInitialisesEverySensor)
{
    Fixture f;
    Node n(f);
    n.module.begin();

    EXPECT_TRUE(f.watchdog.enabled);
    EXPECT_TRUE(f.current.began);
    EXPECT_TRUE(f.temperature.began);
    EXPECT_EQ(PinMode::Output, f.gpio.mode.at(kRelay1));
    EXPECT_EQ(PinMode::InputPullup, f.gpio.mode.at(kButton1));
    EXPECT_EQ(PinMode::InputPullup, f.gpio.mode.at(kInputPin1));
}

TEST(ModulePresentation, AnnouncesEveryEnabledChildAndNothingElse)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.present("GoWired Module", "3.0");

    EXPECT_EQ("GoWired Module", f.bus.sketch_name);
    EXPECT_EQ("3.0", f.bus.sketch_version);

    EXPECT_TRUE(f.bus.was_presented(0));                       // relay 1
    EXPECT_TRUE(f.bus.was_presented(1));                       // relay 2
    EXPECT_TRUE(f.bus.was_presented(2));                       // input 1
    EXPECT_TRUE(f.bus.was_presented(5));                       // input 4
    EXPECT_TRUE(f.bus.was_presented(ids::kSpecialButton1));
    EXPECT_TRUE(f.bus.was_presented(ids::kSpecialButton2));
    EXPECT_TRUE(f.bus.was_presented(ids::kPower));
    EXPECT_TRUE(f.bus.was_presented(ids::kInternalTemp));
    EXPECT_TRUE(f.bus.was_presented(ids::kOvercurrentStatus));
    EXPECT_TRUE(f.bus.was_presented(ids::kThermalStatus));
    EXPECT_TRUE(f.bus.was_presented(ids::kConfiguration));

    // External probe is disabled in this configuration.
    EXPECT_FALSE(f.bus.was_presented(ids::kExternalTemp));
    EXPECT_FALSE(f.bus.was_presented(ids::kExternalHumidity));
    EXPECT_FALSE(f.bus.was_presented(ids::kExternalTempStatus));
}

TEST(ModulePresentation, DisabledFeaturesArePresentedNowhere)
{
    Fixture f;
    f.features.power_sensor = false;
    f.features.internal_temperature = false;
    f.features.error_reporting = false;
    f.features.special_button = false;
    f.enable_inputs(0);
    Node n(f);
    n.module.begin();
    n.module.present("x", "1");

    EXPECT_FALSE(f.bus.was_presented(ids::kPower));
    EXPECT_FALSE(f.bus.was_presented(ids::kInternalTemp));
    EXPECT_FALSE(f.bus.was_presented(ids::kOvercurrentStatus));
    EXPECT_FALSE(f.bus.was_presented(ids::kSpecialButton1));
    EXPECT_FALSE(f.bus.was_presented(2));
    EXPECT_TRUE(f.bus.was_presented(ids::kConfiguration)); // always present
}

TEST(ModuleInitialState, IsSentOnceOnTheFirstLoop)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    ASSERT_FALSE(n.module.initial_state_sent());

    n.module.loop();
    EXPECT_TRUE(n.module.initial_state_sent());
    EXPECT_EQ(1u, f.bus.count(ids::kConfiguration, ValueType::Text));
    EXPECT_EQ(1u, f.bus.count(ids::kOvercurrentStatus, ValueType::Status));

    f.bus.clear();
    n.module.loop();
    EXPECT_EQ(0u, f.bus.count(ids::kConfiguration, ValueType::Text));
}

// ---------------------------------------------------------------------------
// Overcurrent -- the regression this refactor exists to fix
// ---------------------------------------------------------------------------

/// Pre-refactor, loop() measured the current correctly and then immediately
/// overwrote it with the stubbed dimmer's return value of 0.0, so
/// ElectricalStatus() was always handed zero. In the shipped default
/// (DOUBLE_RELAY) that silently disabled overcurrent protection entirely.
TEST(OvercurrentProtection, TripsWhenTheLimitIsExceeded)
{
    Fixture f;
    f.power.max_current_a = 3;
    Node n(f);
    n.module.begin();
    n.module.loop(); // initial state

    // Energise a relay so the sensor is sampled, then overload it.
    n.module.on_message(inbound(0, ValueType::Status, true));
    ASSERT_TRUE(n.device.relay_on(0));
    f.current.ac = 12.0f;
    f.bus.clear();

    n.module.loop();

    EXPECT_TRUE(n.module.safety().overcurrent[0]);
    EXPECT_FALSE(n.device.relay_on(0)) << "the overloaded relay must be dropped";
    ASSERT_EQ(1u, f.bus.count(ids::kOvercurrentStatus, ValueType::Status));
    EXPECT_TRUE(f.bus.matching(ids::kOvercurrentStatus, ValueType::Status)[0].boolean);
}

TEST(OvercurrentProtection, DoesNotTripBelowTheLimit)
{
    Fixture f;
    f.power.max_current_a = 3;
    Node n(f);
    n.module.begin();
    n.module.loop();

    n.module.on_message(inbound(0, ValueType::Status, true));
    f.current.ac = 2.0f;
    f.bus.clear();
    n.module.loop();

    EXPECT_FALSE(n.module.safety().overcurrent[0]);
    EXPECT_TRUE(n.device.relay_on(0));
}

TEST(OvercurrentProtection, StatusIsReportedOncePerTransition)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    n.module.on_message(inbound(0, ValueType::Status, true));

    f.current.ac = 12.0f;
    f.bus.clear();
    n.module.loop();
    n.module.loop();
    n.module.loop();
    EXPECT_EQ(1u, f.bus.count(ids::kOvercurrentStatus, ValueType::Status));
}

/// The fault must not clear itself. Shedding the load removes the current that
/// caused it, so a self-clearing flag would let the controller re-energise
/// straight back into the overload, oscillating indefinitely.
TEST(OvercurrentProtection, LatchesUntilTheControllerClearsIt)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    n.module.on_message(inbound(0, ValueType::Status, true));

    f.current.ac = 12.0f;
    n.module.loop();
    ASSERT_TRUE(n.module.safety().overcurrent[0]);

    // The load is off now, so nothing is drawing. The fault must persist.
    f.current.ac = 0.0f;
    f.bus.clear();
    n.module.loop();
    n.module.loop();
    EXPECT_TRUE(n.module.safety().overcurrent[0]);
    EXPECT_EQ(0u, f.bus.count(ids::kOvercurrentStatus, ValueType::Status));
    EXPECT_FALSE(n.device.relay_on(0)) << "load must stay shed while latched";
}

TEST(OvercurrentProtection, ControllerClearingTheLatchReportsRecoveryOnce)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    n.module.on_message(inbound(0, ValueType::Status, true));
    f.current.ac = 12.0f;
    n.module.loop();
    ASSERT_TRUE(n.module.safety().overcurrent[0]);

    f.current.ac = 0.0f;
    n.module.on_message(inbound(ids::kOvercurrentStatus, ValueType::Status, false));
    f.bus.clear();

    n.module.loop();
    n.module.loop();
    EXPECT_FALSE(n.module.safety().overcurrent[0]);
    EXPECT_EQ(0u, f.bus.count(ids::kOvercurrentStatus, ValueType::Status));

    // And the load can be switched on again.
    n.module.on_message(inbound(0, ValueType::Status, true));
    EXPECT_TRUE(n.device.relay_on(0));
}

TEST(OvercurrentProtection, CommandsAreRefusedWhileFaulted)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    n.module.on_message(inbound(0, ValueType::Status, true));
    f.current.ac = 12.0f;
    n.module.loop();
    ASSERT_TRUE(n.module.safety().overcurrent[0]);

    n.module.on_message(inbound(0, ValueType::Status, true));
    EXPECT_FALSE(n.device.relay_on(0));
}

TEST(OvercurrentProtection, IsNotSampledWhileNothingIsEnergised)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();

    const uint32_t before = f.current.ac_reads;
    n.module.loop();
    EXPECT_EQ(before, f.current.ac_reads);
}

TEST(PowerReporting, PublishesWattsThroughTheDeadband)
{
    Fixture f;
    f.power.receiver_voltage = 230;
    f.power.cos_phi = 1.0f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    n.module.on_message(inbound(0, ValueType::Status, true));

    f.current.ac = 1.0f;
    f.bus.clear();
    n.module.loop();
    ASSERT_EQ(1u, f.bus.count(ids::kPower, ValueType::Watt));
    EXPECT_FLOAT_EQ(230.0f, f.bus.matching(ids::kPower, ValueType::Watt)[0].float_value);

    // A change inside the deadband is not worth a message.
    f.current.ac = 1.02f;
    f.bus.clear();
    n.module.loop();
    EXPECT_EQ(0u, f.bus.count(ids::kPower, ValueType::Watt));
}

TEST(PowerReporting, PerChannelDeadbandsAreIndependent)
{
    Fixture f;
    Node n(f, /* power_channels */ 4);
    n.module.begin();
    n.module.loop();

    // Channels are addressed under their own children.
    n.module.on_message(inbound(0, ValueType::Status, true));
    f.current.ac = 1.0f;
    f.bus.clear();
    n.module.loop();
    EXPECT_EQ(1u, f.bus.count(ids::kPowerPerRelay[0], ValueType::Watt));
}

// ---------------------------------------------------------------------------
// Thermal
// ---------------------------------------------------------------------------

TEST(ThermalProtection, OverheatingShedsTheLoadAndReportsOnce)
{
    Fixture f;
    f.thermal.max_temperature_c = 85;
    Node n(f);
    n.module.begin();
    n.module.loop();
    n.module.on_message(inbound(0, ValueType::Status, true));
    ASSERT_TRUE(n.device.relay_on(0));

    f.temperature.celsius = 95.0f;
    f.bus.clear();
    n.module.loop();

    EXPECT_TRUE(n.module.safety().thermal_fault);
    EXPECT_FALSE(n.device.relay_on(0));
    EXPECT_EQ(1u, f.bus.count(ids::kThermalStatus, ValueType::Status));

    n.module.loop();
    EXPECT_EQ(1u, f.bus.count(ids::kThermalStatus, ValueType::Status));
}

TEST(ThermalProtection, RecoveryIsReported)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    f.temperature.celsius = 95.0f;
    n.module.loop();

    f.temperature.celsius = 30.0f;
    f.bus.clear();
    n.module.loop();

    EXPECT_FALSE(n.module.safety().thermal_fault);
    ASSERT_EQ(1u, f.bus.count(ids::kThermalStatus, ValueType::Status));
    EXPECT_FALSE(f.bus.matching(ids::kThermalStatus, ValueType::Status)[0].boolean);
}

TEST(ThermalProtection, ControllerCanClearTheLatch)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    f.temperature.celsius = 95.0f;
    n.module.loop();
    ASSERT_TRUE(n.module.safety().thermal_fault);

    n.module.on_message(inbound(ids::kThermalStatus, ValueType::Status, false));
    EXPECT_FALSE(n.module.safety().thermal_fault);
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

TEST(ModuleMessages, ControllerCanSetAndClearTheOvercurrentLatch)
{
    Fixture f;
    Node n(f);
    n.module.begin();

    n.module.on_message(inbound(ids::kOvercurrentStatus, ValueType::Status, true));
    EXPECT_TRUE(n.module.safety().overcurrent[0]);

    n.module.on_message(inbound(ids::kOvercurrentStatus, ValueType::Status, false));
    EXPECT_FALSE(n.module.safety().overcurrent[0]);
}

/// The node publishes its own longpress notifications; the controller echoing
/// them back must not be mistaken for a command.
TEST(ModuleMessages, SpecialButtonEchoIsIgnored)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    f.bus.clear();

    n.module.on_message(inbound(ids::kSpecialButton1, ValueType::Status, true));
    n.module.on_message(inbound(ids::kSpecialButton2, ValueType::Status, true));
    EXPECT_TRUE(f.bus.sent.empty());
}

TEST(ModuleMessages, UnknownChildIsIgnoredSilently)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    f.bus.clear();

    n.module.on_message(inbound(99, ValueType::Status, true));
    EXPECT_TRUE(f.bus.sent.empty());
}

TEST(ConfigCommands, PayloadIsEchoedBack)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    f.bus.clear();

    n.module.on_message(inbound(ids::kConfiguration, ValueType::Text, false, 0, "cmd2"));
    ASSERT_EQ(1u, f.bus.count(ids::kConfiguration, ValueType::Text));
    EXPECT_EQ("cmd2", f.bus.matching(ids::kConfiguration, ValueType::Text)[0].text);
}

TEST(ConfigCommands, WatchdogTestStallsLongEnoughToTriggerAReset)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    f.clock.delays.clear();

    n.module.on_message(inbound(ids::kConfiguration, ValueType::Text, false, 0, "cmd3"));
    ASSERT_EQ(1u, f.clock.delays.size());
    EXPECT_GE(f.clock.delays[0], 8000u); // the watchdog period is 8 s
}

TEST(ConfigCommands, ClearStoreBlanksTheWholeEeprom)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    f.store.cells[600] = 0x42;
    f.store.cells[1000] = 0x43;

    n.module.on_message(inbound(ids::kConfiguration, ValueType::Text, false, 0, "cmd4"));

    EXPECT_EQ(0xFF, f.store.read(600));
    EXPECT_EQ(0xFF, f.store.read(1000));
}

TEST(ConfigCommands, UnrecognisedCommandOnlyEchoes)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    f.clock.delays.clear();
    f.store.cells[600] = 0x42;

    n.module.on_message(inbound(ids::kConfiguration, ValueType::Text, false, 0, "nope"));

    EXPECT_TRUE(f.clock.delays.empty());
    EXPECT_EQ(0x42, f.store.read(600));
}

/// A relay bank cannot calibrate; the command must be a harmless no-op rather
/// than reaching into a device that does not support it.
TEST(ConfigCommands, CalibrateIsANoOpOnADeviceThatCannotCalibrate)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    f.bus.clear();

    n.module.on_message(inbound(ids::kConfiguration, ValueType::Text, false, 0, "cmd1"));
    EXPECT_EQ(1u, f.bus.count(ids::kConfiguration, ValueType::Text)); // just the echo
}

// ---------------------------------------------------------------------------
// Interval reporting and the external probe
// ---------------------------------------------------------------------------

TEST(IntervalReporting, TemperatureIsPublishedOnTheConfiguredInterval)
{
    Fixture f;
    f.timing.report_interval_ms = 300000;
    Node n(f);
    n.module.begin();
    n.module.loop(); // initial state includes one temperature reading
    f.bus.clear();

    n.module.loop();
    EXPECT_EQ(0u, f.bus.count(ids::kInternalTemp, ValueType::Temperature));

    f.clock.now += 300001;
    n.module.loop();
    EXPECT_EQ(1u, f.bus.count(ids::kInternalTemp, ValueType::Temperature));
}

TEST(ExternalProbe, GoodReadingPublishesTemperatureAndHumidity)
{
    Fixture f;
    f.features.external_temperature = true;
    f.probe.next.status = IHygrometer::Status::Ok;
    f.probe.next.temperature_c = 21.5f;
    f.probe.next.humidity_pct = 48.0f;

    Node n(f);
    n.module.begin();
    n.module.loop();

    ASSERT_EQ(1u, f.bus.count(ids::kExternalTemp, ValueType::Temperature));
    EXPECT_FLOAT_EQ(21.5f, f.bus.matching(ids::kExternalTemp, ValueType::Temperature)[0].float_value);
    ASSERT_EQ(1u, f.bus.count(ids::kExternalHumidity, ValueType::Humidity));
    EXPECT_FLOAT_EQ(48.0f, f.bus.matching(ids::kExternalHumidity, ValueType::Humidity)[0].float_value);
}

TEST(ExternalProbe, FailureIsReportedAsAStatusCode)
{
    Fixture f;
    f.features.external_temperature = true;
    f.probe.next.status = IHygrometer::Status::TimeoutError;

    Node n(f);
    n.module.begin();
    n.module.loop();

    EXPECT_EQ(0u, f.bus.count(ids::kExternalTemp, ValueType::Temperature));

    // Initial state clears the fault child first, then the failing read reports
    // the real code -- so the controller is left holding the error, not "OK".
    const auto status = f.bus.matching(ids::kExternalTempStatus, ValueType::Status);
    ASSERT_FALSE(status.empty());
    EXPECT_EQ(static_cast<uint32_t>(IHygrometer::Status::TimeoutError), status.back().uint_value);
}

TEST(ExternalProbe, RecoveryClearsTheStatus)
{
    Fixture f;
    f.features.external_temperature = true;
    f.probe.next.status = IHygrometer::Status::ChecksumError;
    Node n(f);
    n.module.begin();
    n.module.loop();

    f.probe.next.status = IHygrometer::Status::Ok;
    f.probe.next.temperature_c = 20.0f;
    f.bus.clear();
    f.clock.now += f.timing.report_interval_ms + 1;
    n.module.loop();

    ASSERT_EQ(1u, f.bus.count(ids::kExternalTempStatus, ValueType::Status));
    EXPECT_EQ(0u, f.bus.matching(ids::kExternalTempStatus, ValueType::Status)[0].uint_value);
}

TEST(ExternalProbe, TemperatureIsMirroredToAHeatingControllerWhenConfigured)
{
    Fixture f;
    f.features.external_temperature = true;
    f.features.heating_controller_node = 7;
    f.probe.next.status = IHygrometer::Status::Ok;
    f.probe.next.temperature_c = 19.0f;

    Node n(f);
    n.module.begin();
    n.module.loop();

    const auto readings = f.bus.matching(ids::kExternalTemp, ValueType::Temperature);
    ASSERT_EQ(2u, readings.size());
    EXPECT_EQ(0, readings[0].node); // the controller
    EXPECT_EQ(7, readings[1].node); // the heating controller
}

TEST(ExternalProbe, IsNotReadWhenDisabled)
{
    Fixture f;
    f.features.external_temperature = false;
    Node n(f);
    n.module.begin();
    n.module.loop();
    EXPECT_EQ(0u, f.probe.reads);
}

// ---------------------------------------------------------------------------
// Loop wiring
// ---------------------------------------------------------------------------

TEST(ModuleLoop, PacesItselfWithTheConfiguredLoopTime)
{
    Fixture f;
    f.timing.loop_time_ms = 80;
    Node n(f);
    n.module.begin();
    f.bus.clear();
    n.module.loop();

    ASSERT_FALSE(f.bus.waits.empty());
    EXPECT_EQ(80u, f.bus.waits.back());
}

TEST(ModuleLoop, GenericInputChangesArePublished)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    f.bus.clear();

    f.gpio.script[kInputPin1] = {false}; // contact closes
    n.module.loop();

    ASSERT_EQ(1u, f.bus.count(2, ValueType::Status));
    EXPECT_TRUE(f.bus.matching(2, ValueType::Status)[0].boolean);
}

TEST(ModuleLoop, WallSwitchStillWorksThroughTheModule)
{
    Fixture f;
    Node n(f);
    n.module.begin();
    n.module.loop();
    f.bus.clear();

    f.gpio.script_short_press(kButton1);
    n.module.loop();

    EXPECT_TRUE(n.device.relay_on(0));
    EXPECT_EQ(1u, f.bus.count(0, ValueType::Status));
}

} // namespace
} // namespace gw
