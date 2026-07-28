/// RelayBankDevice covers both DOUBLE_RELAY and FOUR_RELAY.
#include "domain/relay_bank_device.h"

#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeBus;
using test::FakeClock;
using test::FakeGpio;
using test::inbound;

constexpr Pin kRelay1 = 5;
constexpr Pin kRelay2 = 9;
constexpr Pin kRelay3 = 6;
constexpr Pin kRelay4 = 10;
constexpr Pin kButton1 = 2;
constexpr Pin kButton2 = 3;

RelayBankSpec double_relay_spec()
{
    RelayBankSpec s;
    s.relay_count = 2;
    s.relay_pins[0] = kRelay1;
    s.relay_pins[1] = kRelay2;
    s.button_count = 2;
    s.button_pins[0] = kButton1;
    s.button_pins[1] = kButton2;
    s.off_level = false;
    s.per_relay_power = false;
    return s;
}

RelayBankSpec four_relay_spec()
{
    RelayBankSpec s;
    s.relay_count = 4;
    s.relay_pins[0] = kRelay1;
    s.relay_pins[1] = kRelay2;
    s.relay_pins[2] = kRelay3;
    s.relay_pins[3] = kRelay4;
    s.button_count = 0;
    s.off_level = false;
    s.per_relay_power = true;
    return s;
}

struct Fixture {
    FakeGpio gpio;
    FakeClock clock;
    FakeBus bus;
    ButtonTiming timing;
    SafetyState safety;

    RelayBankDevice make(const RelayBankSpec& spec, bool special = true)
    {
        return RelayBankDevice(gpio, clock, spec, timing, special);
    }
};

TEST(RelayBank, PresentsOneBinaryChildPerRelay)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.present(f.bus, 10);

    ASSERT_EQ(2u, f.bus.presented.size());
    EXPECT_EQ(0, f.bus.presented[0].sensor);
    EXPECT_EQ(SensorClass::Binary, f.bus.presented[0].type);
    EXPECT_EQ("Relay 1", f.bus.presented[0].name);
    EXPECT_EQ(1, f.bus.presented[1].sensor);
    EXPECT_EQ("Relay 2", f.bus.presented[1].name);
}

TEST(RelayBank, FourRelayPresentsFourChildren)
{
    Fixture f;
    RelayBankDevice d = f.make(four_relay_spec());
    d.present(f.bus, 10);
    EXPECT_EQ(4u, f.bus.presented.size());
    EXPECT_EQ("Relay 4", f.bus.presented[3].name);
}

TEST(RelayBank, BeginDeEnergisesEveryRelay)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();

    EXPECT_EQ(PinMode::Output, f.gpio.mode.at(kRelay1));
    EXPECT_FALSE(f.gpio.level.at(kRelay1));
    EXPECT_FALSE(f.gpio.level.at(kRelay2));
    EXPECT_FALSE(d.relay_on(0));
}

/// FOUR_RELAY has no wall switches. The old UpdateIO() looped CheckInput() over
/// its relay channels anyway and debounced whatever pin _SensorPin happened to
/// hold.
TEST(RelayBank, FourRelayNeverConfiguresAButtonInput)
{
    Fixture f;
    RelayBankDevice d = f.make(four_relay_spec());
    d.begin();

    for (const auto& entry : f.gpio.mode) {
        EXPECT_EQ(PinMode::Output, entry.second) << "pin " << static_cast<int>(entry.first);
    }
}

TEST(RelayBank, StatusMessageSwitchesTheAddressedRelay)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();

    EXPECT_TRUE(d.handle(inbound(1, ValueType::Status, true), f.bus, f.safety));
    EXPECT_TRUE(d.relay_on(1));
    EXPECT_FALSE(d.relay_on(0));

    EXPECT_TRUE(d.handle(inbound(1, ValueType::Status, false), f.bus, f.safety));
    EXPECT_FALSE(d.relay_on(1));
}

TEST(RelayBank, MessagesForOtherChildrenAreNotClaimed)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();

    EXPECT_FALSE(d.handle(inbound(ids::kOvercurrentStatus, ValueType::Status, true), f.bus,
                          f.safety));
    EXPECT_FALSE(d.handle(inbound(0, ValueType::Percentage, false, 50), f.bus, f.safety));
}

TEST(RelayBank, AFaultBlocksSwitchingOnButStillConsumesTheMessage)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();
    f.safety.thermal_fault = true;

    EXPECT_TRUE(d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety));
    EXPECT_FALSE(d.relay_on(0));
}

TEST(RelayBank, ShortPressTogglesAndReportsTheNewState)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();

    f.gpio.script_short_press(kButton1);
    f.gpio.script_idle(kButton2);
    d.poll_buttons(f.bus, f.safety);

    EXPECT_TRUE(d.relay_on(0));
    ASSERT_EQ(1u, f.bus.count(0, ValueType::Status));
    EXPECT_TRUE(f.bus.matching(0, ValueType::Status)[0].boolean);
}

TEST(RelayBank, LongPressNotifiesTheMatchingSpecialButtonChild)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();

    f.gpio.script_idle(kButton1);
    f.gpio.script_hold(kButton2);
    d.poll_buttons(f.bus, f.safety);

    EXPECT_FALSE(d.relay_on(1)); // a hold must not switch the load
    ASSERT_EQ(1u, f.bus.count(ids::kSpecialButton2, ValueType::Status));
    EXPECT_TRUE(f.bus.matching(ids::kSpecialButton2, ValueType::Status)[0].boolean);
}

TEST(RelayBank, LongPressIsSilentWhenTheSpecialButtonIsDisabled)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec(), /* special */ false);
    d.begin();

    f.gpio.script_idle(kButton1);
    f.gpio.script_hold(kButton2);
    d.poll_buttons(f.bus, f.safety);
    EXPECT_TRUE(f.bus.sent.empty());
}

TEST(RelayBank, ButtonIsIgnoredWhileAFaultIsActive)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();
    f.safety.overcurrent[0] = true;

    f.gpio.script_short_press(kButton1);
    f.gpio.script_idle(kButton2);
    d.poll_buttons(f.bus, f.safety);

    EXPECT_FALSE(d.relay_on(0));
    EXPECT_TRUE(f.bus.sent.empty());
}

// ---------------------------------------------------------------------------
// Load shedding
// ---------------------------------------------------------------------------

TEST(RelayBankShedLoad, SharedSensorSwitchesEverythingOff)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();
    d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety);
    d.handle(inbound(1, ValueType::Status, true), f.bus, f.safety);
    f.bus.clear();

    f.safety.overcurrent[0] = true;
    d.shed_load(f.bus, f.safety);

    EXPECT_FALSE(d.relay_on(0));
    EXPECT_FALSE(d.relay_on(1));
    EXPECT_EQ(2u, f.bus.sent.size());
}

/// With one sensor per output, only the output that actually tripped should be
/// dropped -- killing all four would be needlessly disruptive.
TEST(RelayBankShedLoad, PerRelaySensingDropsOnlyTheFaultedChannel)
{
    Fixture f;
    RelayBankDevice d = f.make(four_relay_spec());
    d.begin();
    for (uint8_t i = 0; i < 4; ++i) {
        d.handle(inbound(i, ValueType::Status, true), f.bus, f.safety);
    }
    f.bus.clear();

    f.safety.overcurrent[2] = true;
    d.shed_load(f.bus, f.safety);

    EXPECT_TRUE(d.relay_on(0));
    EXPECT_TRUE(d.relay_on(1));
    EXPECT_FALSE(d.relay_on(2));
    EXPECT_TRUE(d.relay_on(3));
    ASSERT_EQ(1u, f.bus.sent.size());
    EXPECT_EQ(2, f.bus.sent[0].sensor);
}

/// shed_load runs on every iteration while the fault persists, so it must not
/// re-announce an already-open relay.
TEST(RelayBankShedLoad, IsIdempotent)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();
    d.handle(inbound(0, ValueType::Status, true), f.bus, f.safety);
    f.bus.clear();

    f.safety.thermal_fault = true;
    d.shed_load(f.bus, f.safety);
    const size_t after_first = f.bus.sent.size();
    d.shed_load(f.bus, f.safety);
    d.shed_load(f.bus, f.safety);

    EXPECT_EQ(after_first, f.bus.sent.size());
}

// ---------------------------------------------------------------------------
// Current sensing
// ---------------------------------------------------------------------------

TEST(RelayBankCurrent, SharedSensorSamplesWheneverAnythingIsEnergised)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();

    EXPECT_EQ(1, d.power_channel_count());
    EXPECT_FALSE(d.draws_current(0));

    d.handle(inbound(1, ValueType::Status, true), f.bus, f.safety);
    EXPECT_TRUE(d.draws_current(0));
}

TEST(RelayBankCurrent, PerRelaySensingTracksEachChannel)
{
    Fixture f;
    RelayBankDevice d = f.make(four_relay_spec());
    d.begin();

    EXPECT_EQ(4, d.power_channel_count());
    d.handle(inbound(2, ValueType::Status, true), f.bus, f.safety);

    EXPECT_FALSE(d.draws_current(0));
    EXPECT_TRUE(d.draws_current(2));
}

TEST(RelayBankCurrent, RelaysAreAcLoads)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    EXPECT_FALSE(d.uses_dc_measurement());
}

TEST(RelayBank, InitialStateSendsAndRequestsEveryChild)
{
    Fixture f;
    RelayBankDevice d = f.make(double_relay_spec());
    d.begin();
    d.send_initial_state(f.bus, 2000);

    EXPECT_EQ(2u, f.bus.sent.size());
    ASSERT_EQ(2u, f.bus.requests.size());
    EXPECT_EQ(0, f.bus.requests[0].first);
    EXPECT_EQ(1, f.bus.requests[1].first);
}

} // namespace
} // namespace gw
