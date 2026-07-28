//! `DoubleRelay` and `FourRelay`.

use crate::domain::config::ids;
use crate::domain::device::{Device, SafetyState};
use crate::fakes::{Event, Rig};
use crate::hal::{InboundMessage, PinMode, SensorClass, ValueType};
use crate::tests::support::{double_relay_spec, four_relay_spec, pins, relay_bank};

fn set_status(sensor: u8, on: bool) -> InboundMessage<'static> {
    InboundMessage {
        sensor,
        ty: ValueType::Status,
        boolean: on,
        numeric: i32::from(on),
        text: "",
    }
}

fn clear() -> SafetyState {
    SafetyState::default()
}

#[test]
fn presents_one_binary_child_per_relay() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.present(&rig.bus, 10);

    assert_eq!(rig.bus.presented_ids(), std::vec![0, 1]);
    assert!(rig.bus.any(|e| matches!(
        e,
        Event::Presented {
            sensor: 0,
            class: SensorClass::Binary,
            name
        } if name == "Relay 1"
    )));
}

#[test]
fn four_relay_presents_four_children() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, four_relay_spec(), false);
    d.present(&rig.bus, 10);
    assert_eq!(rig.bus.presented_ids(), std::vec![0, 1, 2, 3]);
}

#[test]
fn begin_de_energises_every_relay() {
    let rig = Rig::new();
    relay_bank(&rig, double_relay_spec(), true).begin();

    assert_eq!(rig.gpio.mode_of(pins::OUT1), Some(PinMode::Output));
    assert_eq!(rig.gpio.mode_of(pins::OUT2), Some(PinMode::Output));
    assert!(!rig.gpio.level_of(pins::OUT1));
    assert!(!rig.gpio.level_of(pins::OUT2));
}

/// The pre-refactor `UpdateIO()` ran `CheckInput()` over `FourRelay`'s outputs
/// and debounced an uninitialised pin.
#[test]
fn four_relay_never_configures_a_button_input() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, four_relay_spec(), false);
    d.begin();
    d.poll_buttons(&rig.bus, &clear());

    assert_eq!(rig.gpio.mode_of(pins::BUTTON1), None);
    assert_eq!(rig.gpio.mode_of(pins::BUTTON2), None);
    assert!(rig.bus.events().is_empty());
}

#[test]
fn status_message_switches_the_addressed_relay() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();

    assert!(d.handle(&set_status(1, true), &rig.bus, &clear()));
    assert!(!d.relay_on(0));
    assert!(d.relay_on(1));
    assert!(rig.gpio.level_of(pins::OUT2));
}

#[test]
fn messages_for_other_children_are_not_claimed() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();

    assert!(!d.handle(&set_status(7, true), &rig.bus, &clear()));

    let wrong_type = InboundMessage {
        sensor: 0,
        ty: ValueType::Percentage,
        numeric: 50,
        ..InboundMessage::default()
    };
    assert!(!d.handle(&wrong_type, &rig.bus, &clear()));
}

/// The message *is* for us, so it is consumed -- we simply refuse to act.
#[test]
fn a_fault_blocks_switching_on_but_still_consumes_the_message() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();

    let mut safety = SafetyState::default();
    safety.overcurrent[0] = true;

    assert!(d.handle(&set_status(0, true), &rig.bus, &safety));
    assert!(!d.relay_on(0));
}

#[test]
fn short_press_toggles_and_reports_the_new_state() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();

    rig.gpio.script_short_press(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());

    assert!(d.relay_on(0));
    assert_eq!(rig.bus.last_bool(0, ValueType::Status), Some(true));
}

#[test]
fn long_press_notifies_the_matching_special_button_child() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();

    rig.gpio.script_idle(pins::BUTTON1);
    rig.gpio.script_hold(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());

    assert_eq!(
        rig.bus.last_bool(ids::SPECIAL_BUTTON_2, ValueType::Status),
        Some(true)
    );
    // A long press must not also switch the relay.
    assert!(!d.relay_on(1));
}

#[test]
fn long_press_is_silent_when_the_special_button_is_disabled() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), false);
    d.begin();

    rig.gpio.script_hold(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());

    assert_eq!(
        rig.bus.last_bool(ids::SPECIAL_BUTTON_1, ValueType::Status),
        None
    );
}

#[test]
fn button_is_ignored_while_a_fault_is_active() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();

    let safety = SafetyState {
        thermal_fault: true,
        ..SafetyState::default()
    };
    rig.gpio.script_short_press(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &safety);

    assert!(!d.relay_on(0));
    assert!(rig.bus.events().is_empty());
}

// ---------------------------------------------------------------------------
// Load shedding
// ---------------------------------------------------------------------------

#[test]
fn shared_sensor_shed_switches_everything_off() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();
    d.handle(&set_status(0, true), &rig.bus, &clear());
    d.handle(&set_status(1, true), &rig.bus, &clear());
    rig.bus.clear();

    let mut safety = SafetyState::default();
    safety.overcurrent[0] = true;
    d.shed_load(&rig.bus, &safety);

    assert!(!d.relay_on(0));
    assert!(!d.relay_on(1));
    assert_eq!(rig.bus.last_bool(0, ValueType::Status), Some(false));
    assert_eq!(rig.bus.last_bool(1, ValueType::Status), Some(false));
}

#[test]
fn per_relay_sensing_drops_only_the_faulted_channel() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, four_relay_spec(), false);
    d.begin();
    for i in 0..4 {
        d.handle(&set_status(i, true), &rig.bus, &clear());
    }
    rig.bus.clear();

    let mut safety = SafetyState::default();
    safety.overcurrent[2] = true;
    d.shed_load(&rig.bus, &safety);

    assert!(d.relay_on(0));
    assert!(d.relay_on(1));
    assert!(!d.relay_on(2));
    assert!(d.relay_on(3));
    assert_eq!(rig.bus.last_bool(2, ValueType::Status), Some(false));
    assert_eq!(rig.bus.last_bool(0, ValueType::Status), None);
}

/// `shed_load` runs on every loop pass while the fault lasts.
#[test]
fn shed_load_is_idempotent() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();
    d.handle(&set_status(0, true), &rig.bus, &clear());

    let mut safety = SafetyState::default();
    safety.overcurrent[0] = true;
    d.shed_load(&rig.bus, &safety);
    rig.bus.clear();

    d.shed_load(&rig.bus, &safety);
    d.shed_load(&rig.bus, &safety);
    assert!(rig.bus.events().is_empty());
}

// ---------------------------------------------------------------------------
// Current sensing
// ---------------------------------------------------------------------------

#[test]
fn shared_sensor_samples_whenever_anything_is_energised() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();
    assert!(!d.draws_current(0));

    d.handle(&set_status(1, true), &rig.bus, &clear());
    assert!(d.draws_current(0)); // channel 0 is the shared sensor
    assert_eq!(d.power_channel_count(), 1);
}

#[test]
fn per_relay_sensing_tracks_each_channel() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, four_relay_spec(), false);
    d.begin();
    assert_eq!(d.power_channel_count(), 4);

    d.handle(&set_status(2, true), &rig.bus, &clear());
    assert!(!d.draws_current(0));
    assert!(!d.draws_current(1));
    assert!(d.draws_current(2));
    assert!(!d.draws_current(3));
}

#[test]
fn relays_are_ac_loads() {
    let rig = Rig::new();
    let d = relay_bank(&rig, double_relay_spec(), true);
    assert!(!d.uses_dc_measurement());
}

#[test]
fn initial_state_sends_and_requests_every_child() {
    let rig = Rig::new();
    let mut d = relay_bank(&rig, double_relay_spec(), true);
    d.begin();
    d.send_initial_state(&rig.bus, 2000);

    for id in [0u8, 1] {
        assert_eq!(rig.bus.last_bool(id, ValueType::Status), Some(false));
        assert!(rig.bus.any(|e| matches!(
            e,
            Event::Requested {
                sensor,
                ty: ValueType::Status
            } if *sensor == id
        )));
    }
    assert_eq!(
        rig.bus.count(|e| matches!(
            e,
            Event::WaitedForSet {
                ms: 2000,
                ty: ValueType::Status
            }
        )),
        2
    );
}
