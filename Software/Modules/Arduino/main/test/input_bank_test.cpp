/// The generic digital inputs, which used to be four copy-pasted #ifdef blocks
/// in each of setup(), presentation(), InitConfirmation() and UpdateIO().
#include "domain/input_bank.h"

#include "fakes.h"

#include <gtest/gtest.h>

namespace gw {
namespace {

using test::FakeBus;
using test::FakeClock;
using test::FakeGpio;

struct Fixture {
    FakeGpio gpio;
    FakeClock clock;
    FakeBus bus;

    InputPin pins[kMaxInputs] = {
        {2, 4, true, true, false},
        {3, 17, true, true, false},
        {4, 15, true, true, false},
        {5, 16, true, true, false},
    };

    /// Enables the first `n` slots and disables the rest.
    InputBank make(uint8_t n)
    {
        for (uint8_t i = 0; i < kMaxInputs; ++i) {
            pins[i].enabled = i < n;
        }
        return InputBank(gpio, clock, pins, 50);
    }

    /// Enables exactly the slots named in `mask`.
    InputBank make_mask(uint8_t mask)
    {
        for (uint8_t i = 0; i < kMaxInputs; ++i) {
            pins[i].enabled = (mask & (1u << i)) != 0;
        }
        return InputBank(gpio, clock, pins, 50);
    }

    void idle()
    {
        for (uint8_t i = 0; i < kMaxInputs; ++i) {
            gpio.script_idle(pins[i].pin);
        }
    }
};

TEST(InputBank, PresentsOneBinaryChildPerEnabledInput)
{
    Fixture f;
    InputBank bank = f.make(4);
    bank.present(f.bus, 10);

    ASSERT_EQ(4u, f.bus.presented.size());
    EXPECT_EQ(2, f.bus.presented[0].sensor);
    EXPECT_EQ("Input 1", f.bus.presented[0].name);
    EXPECT_EQ(SensorClass::Binary, f.bus.presented[0].type);
    EXPECT_EQ(5, f.bus.presented[3].sensor);
    EXPECT_EQ("Input 4", f.bus.presented[3].name);
}

/// Reducing the input count used to be impossible: NUMBER_OF_INPUTS was a sum of
/// possibly-undefined PULLUP_n macros used as a C++ array bound, so commenting
/// out INPUT_3 broke the build.
TEST(InputBank, HonoursAReducedInputCount)
{
    Fixture f;
    InputBank bank = f.make(2);
    bank.present(f.bus, 10);

    ASSERT_EQ(2u, f.bus.presented.size());
    EXPECT_EQ(2, f.bus.presented[0].sensor);
    EXPECT_EQ(3, f.bus.presented[1].sensor);
}

TEST(InputBank, ZeroInputsIsLegalAndSilent)
{
    Fixture f;
    InputBank bank = f.make(0);
    bank.begin();
    bank.present(f.bus, 10);
    bank.send_initial_state(f.bus);
    f.idle();
    bank.poll(f.bus);

    EXPECT_TRUE(f.bus.presented.empty());
    EXPECT_TRUE(f.bus.sent.empty());
    EXPECT_TRUE(f.gpio.mode.empty());
}

TEST(InputBank, ConfiguresOnlyTheEnabledPins)
{
    Fixture f;
    InputBank bank = f.make(2);
    bank.begin();

    EXPECT_EQ(1u, f.gpio.mode.count(f.pins[0].pin));
    EXPECT_EQ(1u, f.gpio.mode.count(f.pins[1].pin));
    EXPECT_EQ(0u, f.gpio.mode.count(f.pins[2].pin));
    EXPECT_EQ(0u, f.gpio.mode.count(f.pins[3].pin));
}

TEST(InputBank, InitialStateReportsEveryEnabledChild)
{
    Fixture f;
    InputBank bank = f.make(3);
    bank.begin();
    bank.send_initial_state(f.bus);

    ASSERT_EQ(3u, f.bus.sent.size());
    EXPECT_EQ(2, f.bus.sent[0].sensor);
    EXPECT_EQ(ValueType::Status, f.bus.sent[0].type);
    EXPECT_FALSE(f.bus.sent[0].boolean);
}

TEST(InputBank, PublishesChangesOnlyOnce)
{
    Fixture f;
    InputBank bank = f.make(4);
    bank.begin();
    f.idle();
    bank.poll(f.bus);
    ASSERT_TRUE(f.bus.sent.empty());

    f.gpio.script[f.pins[1].pin] = {false}; // input 2 closes
    bank.poll(f.bus);
    ASSERT_EQ(1u, f.bus.sent.size());
    EXPECT_EQ(3, f.bus.sent[0].sensor);
    EXPECT_TRUE(f.bus.sent[0].boolean);

    f.bus.clear();
    bank.poll(f.bus); // unchanged
    EXPECT_TRUE(f.bus.sent.empty());

    f.gpio.script_idle(f.pins[1].pin); // opens again
    bank.poll(f.bus);
    ASSERT_EQ(1u, f.bus.sent.size());
    EXPECT_FALSE(f.bus.sent[0].boolean);
}

/// PULLUP_n per input: a dry contact needs the pullup, a sensor that drives the
/// line itself must not have it. The official instructions describe this as
/// commenting out PULLUP_n.
TEST(InputBank, PullupIsConfiguredPerInput)
{
    Fixture f;
    f.pins[0].pullup = true;
    f.pins[1].pullup = false;
    InputBank bank = f.make(2);
    bank.begin();

    EXPECT_EQ(PinMode::InputPullup, f.gpio.mode.at(f.pins[0].pin));
    EXPECT_EQ(PinMode::Input, f.gpio.mode.at(f.pins[1].pin));
}

/// INVERT_n per input.
TEST(InputBank, InvertIsConfiguredPerInput)
{
    Fixture f;
    f.pins[0].invert = true;
    InputBank bank = f.make(1);
    bank.begin();

    // Inverted: a HIGH reading is the active state.
    f.gpio.script[f.pins[0].pin] = {true};
    bank.poll(f.bus);
    ASSERT_EQ(1u, f.bus.sent.size());
    EXPECT_TRUE(f.bus.sent[0].boolean);
}

/// Enabled slots need not be contiguous, and a disabled slot in the middle must
/// not shift the child ids of the ones after it.
TEST(InputBank, NonContiguousInputsKeepTheirOwnIds)
{
    Fixture f;
    InputBank bank = f.make_mask(0b1001); // INPUT_1 and INPUT_4 only
    bank.begin();
    bank.present(f.bus, 10);

    ASSERT_EQ(2u, f.bus.presented.size());
    EXPECT_EQ(2, f.bus.presented[0].sensor);
    EXPECT_EQ("Input 1", f.bus.presented[0].name);
    EXPECT_EQ(5, f.bus.presented[1].sensor); // still 5, not 3
    EXPECT_EQ("Input 4", f.bus.presented[1].name);

    // The disabled middle slots are not even configured as pins.
    EXPECT_EQ(0u, f.gpio.mode.count(f.pins[1].pin));
    EXPECT_EQ(0u, f.gpio.mode.count(f.pins[2].pin));
    EXPECT_EQ(2, bank.enabled_count());
}

TEST(InputBank, DisabledInputsAreNeverPolled)
{
    Fixture f;
    InputBank bank = f.make_mask(0b0001);
    bank.begin();
    f.idle();
    bank.poll(f.bus);
    f.bus.clear();

    // Slot 2 goes active, but it is disabled, so nothing is reported.
    f.gpio.script[f.pins[1].pin] = {false};
    bank.poll(f.bus);
    EXPECT_TRUE(f.bus.sent.empty());
}

TEST(InputBank, InputsAreIndependent)
{
    Fixture f;
    InputBank bank = f.make(4);
    bank.begin();
    f.idle();
    bank.poll(f.bus);
    f.bus.clear();

    f.gpio.script[f.pins[0].pin] = {false};
    f.gpio.script[f.pins[3].pin] = {false};
    bank.poll(f.bus);

    ASSERT_EQ(2u, f.bus.sent.size());
    EXPECT_EQ(2, f.bus.sent[0].sensor);
    EXPECT_EQ(5, f.bus.sent[1].sensor);
}

} // namespace
} // namespace gw
