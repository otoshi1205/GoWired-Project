//! Sensor-id layout and the collision detector.

use crate::domain::config::{
    all_distinct, button_count, channel_count, enabled_input_mask, first_input_id, ids,
    ids_are_valid, output_count, presented_ids, ColorModel, DeviceKind, Features, InputPin, IdSet,
    MAX_INPUTS,
};

const KINDS: [DeviceKind; 6] = [
    DeviceKind::DoubleRelay,
    DeviceKind::RollerShutter,
    DeviceKind::FourRelay,
    DeviceKind::Dimmer,
    DeviceKind::Rgb,
    DeviceKind::Rgbw,
];

fn every_feature_combination() -> impl Iterator<Item = Features> {
    (0u8..32).map(|bits| Features {
        power_sensor: bits & 1 != 0,
        internal_temperature: bits & 2 != 0,
        external_temperature: bits & 4 != 0,
        error_reporting: bits & 8 != 0,
        special_button: bits & 16 != 0,
        heating_controller_node: 0,
    })
}

/// The exhaustive check the firmware's `const` assertion relies on: 6 kinds x
/// 16 input masks x 32 feature combinations.
#[test]
fn no_collisions_for_any_kind_or_input_combination() {
    for kind in KINDS {
        for mask in 0u8..16 {
            for features in every_feature_combination() {
                assert!(
                    ids_are_valid(kind, mask, &features),
                    "collision for {kind:?} mask={mask:#06b} {features:?}: {:?}",
                    presented_ids(kind, mask, &features)
                );
            }
        }
    }
}

/// The bug this check exists for. `FourRelay`'s per-relay power sensors sit on
/// ids 4..7, which is where the generic inputs would land if they started at 2.
#[test]
fn four_relay_inputs_do_not_overlap_per_relay_power_sensors() {
    let features = Features {
        power_sensor: true,
        ..Features::default()
    };
    let set = presented_ids(DeviceKind::FourRelay, 0b1111, &features);

    for id in ids::POWER_PER_RELAY {
        assert!(set.contains(id), "power sensor {id} missing");
    }
    for i in 0..4u8 {
        assert!(set.contains(first_input_id(DeviceKind::FourRelay) + i));
    }
    assert!(all_distinct(&set));
    assert_eq!(first_input_id(DeviceKind::FourRelay), 21);
}

/// These numbers are on the wire. A controller bound to them must keep working
/// across the rewrite, so they are asserted literally rather than derived.
#[test]
fn ids_are_pinned_for_wire_compatibility() {
    assert_eq!(ids::SPECIAL_BUTTON_1, 8);
    assert_eq!(ids::SPECIAL_BUTTON_2, 9);
    assert_eq!(ids::POWER, 10);
    assert_eq!(ids::INTERNAL_TEMP, 11);
    assert_eq!(ids::EXTERNAL_TEMP, 12);
    assert_eq!(ids::EXTERNAL_HUMIDITY, 13);
    assert_eq!(ids::OVERCURRENT_STATUS, 15);
    assert_eq!(ids::THERMAL_STATUS, 16);
    assert_eq!(ids::EXTERNAL_TEMP_STATUS, 17);
    assert_eq!(ids::CONFIGURATION, 20);
    assert_eq!(ids::POWER_PER_RELAY, [4, 5, 6, 7]);
    assert_eq!(first_input_id(DeviceKind::DoubleRelay), 2);
}

#[test]
fn output_and_button_counts() {
    assert_eq!(output_count(DeviceKind::DoubleRelay), 2);
    assert_eq!(output_count(DeviceKind::RollerShutter), 2);
    assert_eq!(output_count(DeviceKind::FourRelay), 4);
    assert_eq!(output_count(DeviceKind::Dimmer), 1);
    assert_eq!(output_count(DeviceKind::Rgb), 1);
    assert_eq!(output_count(DeviceKind::Rgbw), 1);

    assert_eq!(button_count(DeviceKind::FourRelay), 0);
    for kind in KINDS.iter().filter(|k| **k != DeviceKind::FourRelay) {
        assert_eq!(button_count(*kind), 2, "{kind:?}");
    }
}

#[test]
fn channel_count_per_color_model() {
    // White drives all four pins together, which is the legacy behaviour of the
    // single-colour dimmer shield.
    assert_eq!(channel_count(ColorModel::White), 4);
    assert_eq!(channel_count(ColorModel::Rgb), 3);
    assert_eq!(channel_count(ColorModel::Rgbw), 4);
}

#[test]
fn any_input_combination_is_representable() {
    for mask in 0u8..16 {
        let features = Features::default();
        let set = presented_ids(DeviceKind::DoubleRelay, mask, &features);
        let expected = u32::from(mask.count_ones());
        let inputs = (0..4u8)
            .filter(|i| set.contains(first_input_id(DeviceKind::DoubleRelay) + i))
            .count() as u32;
        assert_eq!(inputs, expected, "mask={mask:#06b}");
    }
}

/// Turning off `INPUT_2` must not slide `INPUT_3` and `INPUT_4` down into a
/// controller that is already bound to them.
#[test]
fn disabling_an_input_does_not_renumber_the_others() {
    let all = presented_ids(DeviceKind::DoubleRelay, 0b1111, &Features::default());
    let without_second = presented_ids(DeviceKind::DoubleRelay, 0b1101, &Features::default());

    assert!(all.contains(3));
    assert!(!without_second.contains(3));
    // 4 and 5 are INPUT_3 and INPUT_4; they keep their ids.
    assert!(without_second.contains(4));
    assert!(without_second.contains(5));
}

#[test]
fn input_mask_is_derived_from_the_enabled_flags() {
    let mut pins = [InputPin::none(); MAX_INPUTS];
    assert_eq!(enabled_input_mask(&pins), 0);

    pins[0].enabled = true;
    pins[3].enabled = true;
    assert_eq!(enabled_input_mask(&pins), 0b1001);

    for pin in &mut pins {
        pin.enabled = true;
    }
    assert_eq!(enabled_input_mask(&pins), 0b1111);
}

/// A detector that never fires would pass every other test in this file.
#[test]
fn collision_detector_actually_detects() {
    let set = IdSet::new().add(7).add(9);
    assert!(all_distinct(&set));
    assert!(!all_distinct(&set.add(7)));
}
