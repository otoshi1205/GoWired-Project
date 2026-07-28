//! The four generic digital inputs.

use crate::domain::config::MAX_INPUTS;
use crate::fakes::{Event, Rig};
use crate::hal::{PinMode, SensorClass, ValueType};
use crate::tests::support::{input_bank, inputs, pins};

const ALL_PINS: [u8; MAX_INPUTS] = [pins::IN1, pins::IN2, pins::IN3, pins::IN4];

#[test]
fn presents_one_binary_child_per_enabled_input() {
    let rig = Rig::new();
    let bank = input_bank(&rig, inputs([true; MAX_INPUTS]));
    bank.present(&rig.bus, 10);

    assert_eq!(rig.bus.presented_ids(), std::vec![2, 3, 4, 5]);
    assert_eq!(
        rig.bus.count(|e| matches!(
            e,
            Event::Presented {
                class: SensorClass::Binary,
                ..
            }
        )),
        4
    );
    // One inter-message pause per child, so the gateway keeps up.
    assert_eq!(rig.bus.count(|e| matches!(e, Event::Waited { ms: 10 })), 4);
}

#[test]
fn honours_a_reduced_input_count() {
    let rig = Rig::new();
    let bank = input_bank(&rig, inputs([true, true, false, false]));
    bank.present(&rig.bus, 10);
    assert_eq!(rig.bus.presented_ids(), std::vec![2, 3]);
    assert_eq!(bank.enabled_count(), 2);
}

/// Every input disabled used to be a compile error, because `NUMBER_OF_INPUTS`
/// was a sum of possibly-undefined macros used as an array bound.
#[test]
fn zero_inputs_is_legal_and_silent() {
    let rig = Rig::new();
    let mut bank = input_bank(&rig, inputs([false; MAX_INPUTS]));
    bank.begin();
    bank.present(&rig.bus, 10);
    bank.send_initial_state(&rig.bus);
    bank.poll(&rig.bus);

    assert_eq!(bank.enabled_count(), 0);
    assert!(rig.bus.events().is_empty());
}

#[test]
fn configures_only_the_enabled_pins() {
    let rig = Rig::new();
    input_bank(&rig, inputs([true, false, true, false])).begin();

    assert_eq!(rig.gpio.mode_of(pins::IN1), Some(PinMode::InputPullup));
    assert_eq!(rig.gpio.mode_of(pins::IN2), None);
    assert_eq!(rig.gpio.mode_of(pins::IN3), Some(PinMode::InputPullup));
    assert_eq!(rig.gpio.mode_of(pins::IN4), None);
}

#[test]
fn initial_state_reports_every_enabled_child() {
    let rig = Rig::new();
    let bank = input_bank(&rig, inputs([true, true, false, true]));
    bank.send_initial_state(&rig.bus);

    assert_eq!(rig.bus.last_bool(2, ValueType::Status), Some(false));
    assert_eq!(rig.bus.last_bool(3, ValueType::Status), Some(false));
    assert_eq!(rig.bus.last_bool(4, ValueType::Status), None);
    assert_eq!(rig.bus.last_bool(5, ValueType::Status), Some(false));
}

#[test]
fn publishes_changes_only_once() {
    let rig = Rig::new();
    let mut bank = input_bank(&rig, inputs([true, false, false, false]));

    rig.gpio.script_hold(pins::IN1); // contact closes
    bank.poll(&rig.bus);
    assert_eq!(rig.bus.last_bool(2, ValueType::Status), Some(true));

    rig.bus.clear();
    bank.poll(&rig.bus); // still closed
    assert!(rig.bus.events().is_empty());
}

#[test]
fn pullup_is_configured_per_input() {
    let rig = Rig::new();
    let mut pins_cfg = inputs([true; MAX_INPUTS]);
    pins_cfg[1].pullup = false;
    pins_cfg[3].pullup = false;
    input_bank(&rig, pins_cfg).begin();

    assert_eq!(rig.gpio.mode_of(pins::IN1), Some(PinMode::InputPullup));
    assert_eq!(rig.gpio.mode_of(pins::IN2), Some(PinMode::Input));
    assert_eq!(rig.gpio.mode_of(pins::IN3), Some(PinMode::InputPullup));
    assert_eq!(rig.gpio.mode_of(pins::IN4), Some(PinMode::Input));
}

#[test]
fn invert_is_configured_per_input() {
    let rig = Rig::new();
    let mut pins_cfg = inputs([true, true, false, false]);
    pins_cfg[1].invert = true;
    let mut bank = input_bank(&rig, pins_cfg);

    // Both lines high. Slot 0 reads that as inactive, slot 1 as active.
    rig.gpio.script(pins::IN1, &[true]);
    rig.gpio.script(pins::IN2, &[true]);
    bank.poll(&rig.bus);

    assert_eq!(rig.bus.last_bool(2, ValueType::Status), None);
    assert_eq!(rig.bus.last_bool(3, ValueType::Status), Some(true));
}

/// Disabling slot 1 must leave slots 2 and 3 on their own ids.
#[test]
fn non_contiguous_inputs_keep_their_own_ids() {
    let rig = Rig::new();
    let mut bank = input_bank(&rig, inputs([true, false, true, false]));

    rig.gpio.script_hold(pins::IN3);
    bank.poll(&rig.bus);

    assert_eq!(rig.bus.last_bool(4, ValueType::Status), Some(true));
    assert_eq!(rig.bus.last_bool(3, ValueType::Status), None);
}

#[test]
fn disabled_inputs_are_never_polled() {
    let rig = Rig::new();
    let mut bank = input_bank(&rig, inputs([false, false, false, false]));

    // Every line is active. Nothing should be reported, and nothing read.
    for pin in ALL_PINS {
        rig.gpio.script_hold(pin);
    }
    bank.poll(&rig.bus);
    assert!(rig.bus.events().is_empty());
}

#[test]
fn inputs_are_independent() {
    let rig = Rig::new();
    let mut bank = input_bank(&rig, inputs([true; MAX_INPUTS]));

    rig.gpio.script_hold(pins::IN2);
    rig.gpio.script_idle(pins::IN1);
    rig.gpio.script_idle(pins::IN3);
    rig.gpio.script_idle(pins::IN4);
    bank.poll(&rig.bus);

    assert_eq!(rig.bus.last_bool(3, ValueType::Status), Some(true));
    for id in [2, 4, 5] {
        assert_eq!(rig.bus.last_bool(id, ValueType::Status), None, "child {id}");
    }
}
