/// Sensor-id layout. The old macro chain computed ids by addition, so a
/// collision was invisible; these tests plus the static_assert in
/// Configuration.h make it a build failure instead.
#include "domain/config.h"

#include <gtest/gtest.h>

#include <set>

namespace gw {
namespace {

Features all_features()
{
    Features f;
    f.power_sensor = true;
    f.internal_temperature = true;
    f.external_temperature = true;
    f.error_reporting = true;
    f.special_button = true;
    return f;
}

const DeviceKind kAllKinds[] = {
    DeviceKind::DoubleRelay, DeviceKind::RollerShutter, DeviceKind::FourRelay,
    DeviceKind::Dimmer,      DeviceKind::Rgb,           DeviceKind::Rgbw,
};

TEST(SensorIds, NoCollisionsForAnyKindOrInputCombination)
{
    for (DeviceKind kind : kAllKinds) {
        // Every subset of the four inputs, not just the contiguous prefixes.
        for (uint8_t inputs = 0; inputs < (1u << kMaxInputs); ++inputs) {
            Features f = all_features();
            // FOUR_RELAY has no thermistor and no wall switches.
            if (kind == DeviceKind::FourRelay) {
                f.internal_temperature = false;
                f.special_button = false;
            }
            EXPECT_TRUE(ids_are_valid(kind, inputs, f))
                << "kind=" << static_cast<int>(kind) << " inputs=" << static_cast<int>(inputs);
        }
    }
}

/// The specific regression: FOUR_RELAY publishes per-relay power on 4..7, and
/// the old FIRST_INPUT_ID put its digital inputs on 4..7 as well.
TEST(SensorIds, FourRelayInputsDoNotOverlapPerRelayPowerSensors)
{
    std::set<SensorId> power(std::begin(ids::kPowerPerRelay), std::end(ids::kPowerPerRelay));
    for (uint8_t i = 0; i < kMaxInputs; ++i) {
        const SensorId input = static_cast<SensorId>(first_input_id(DeviceKind::FourRelay) + i);
        EXPECT_EQ(0u, power.count(input)) << "input id " << static_cast<int>(input);
    }
}

/// Ids the pre-refactor firmware used are a deployed wire protocol.
TEST(SensorIds, PinnedForWireCompatibility)
{
    EXPECT_EQ(8, ids::kSpecialButton1);
    EXPECT_EQ(9, ids::kSpecialButton2);
    EXPECT_EQ(10, ids::kPower);
    EXPECT_EQ(11, ids::kInternalTemp);
    EXPECT_EQ(12, ids::kExternalTemp);
    EXPECT_EQ(13, ids::kExternalHumidity);
    EXPECT_EQ(15, ids::kOvercurrentStatus);
    EXPECT_EQ(16, ids::kThermalStatus);
    EXPECT_EQ(17, ids::kExternalTempStatus);
    EXPECT_EQ(20, ids::kConfiguration);

    // The five variants that worked before must keep their input ids.
    EXPECT_EQ(2, first_input_id(DeviceKind::DoubleRelay));
    EXPECT_EQ(2, first_input_id(DeviceKind::RollerShutter));
    EXPECT_EQ(2, first_input_id(DeviceKind::Dimmer));
    EXPECT_EQ(2, first_input_id(DeviceKind::Rgb));
    EXPECT_EQ(2, first_input_id(DeviceKind::Rgbw));
}

TEST(DeviceShape, OutputAndButtonCounts)
{
    EXPECT_EQ(2, output_count(DeviceKind::DoubleRelay));
    EXPECT_EQ(4, output_count(DeviceKind::FourRelay));
    EXPECT_EQ(2, button_count(DeviceKind::DoubleRelay));

    // FOUR_RELAY has no wall switches, which is why its relays must never be
    // polled as inputs.
    EXPECT_EQ(0, button_count(DeviceKind::FourRelay));
}

TEST(DeviceShape, ChannelCountPerColorModel)
{
    EXPECT_EQ(4, channel_count(ColorModel::White));
    EXPECT_EQ(3, channel_count(ColorModel::Rgb));
    EXPECT_EQ(4, channel_count(ColorModel::Rgbw));
}

/// Any number of generic inputs is legal. Reducing the count below four used to
/// break the build, because NUMBER_OF_INPUTS summed possibly-undefined macros
/// and was then used as a C++ array bound.
TEST(SensorIds, AnyInputCombinationIsRepresentable)
{
    Features f;
    for (uint8_t mask = 0; mask < (1u << kMaxInputs); ++mask) {
        const IdSet s = presented_ids(DeviceKind::DoubleRelay, mask, f);
        uint8_t enabled = 0;
        for (uint8_t i = 0; i < kMaxInputs; ++i) {
            enabled = static_cast<uint8_t>(enabled + ((mask >> i) & 1u));
        }
        // two relays + the enabled inputs + the configuration child
        EXPECT_EQ(3u + enabled, s.n) << "mask=" << static_cast<int>(mask);
        EXPECT_TRUE(all_distinct(s));
    }
}

/// Disabling a middle input must not renumber the ones after it: a controller
/// is already bound to those child ids.
TEST(SensorIds, DisablingAnInputDoesNotRenumberTheOthers)
{
    Features f;
    const IdSet all = presented_ids(DeviceKind::DoubleRelay, 0b1111, f);
    const IdSet without_second = presented_ids(DeviceKind::DoubleRelay, 0b1101, f);

    // INPUT_4 keeps id 5 either way.
    EXPECT_EQ(5, all.v[5]);
    EXPECT_EQ(5, without_second.v[4]);
    EXPECT_TRUE(all_distinct(without_second));
}

TEST(InputMask, IsDerivedFromTheEnabledFlags)
{
    InputPin pins[kMaxInputs] = {
        {2, 4, true, true, false},
        {3, 5, false, true, false},
        {4, 6, true, true, false},
        {5, 7, false, true, false},
    };
    EXPECT_EQ(0b0101, enabled_input_mask(pins));
}

TEST(SensorIds, CollisionDetectorActuallyDetects)
{
    IdSet s;
    s.add(4);
    s.add(7);
    s.add(4);
    EXPECT_FALSE(all_distinct(s));
}

} // namespace
} // namespace gw
