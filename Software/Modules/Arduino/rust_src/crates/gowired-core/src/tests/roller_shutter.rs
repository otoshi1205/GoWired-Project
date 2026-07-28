//! `ROLLER_SHUTTER`.

use crate::domain::config::{ids, StoreLayout};
use crate::domain::device::{Device, SafetyState};
use crate::domain::shutter::Motion;
use crate::fakes::{Event, Rig};
use crate::hal::{InboundMessage, SensorClass, ValueType};
use crate::tests::support::{pins, roller_shutter};

fn clear() -> SafetyState {
    SafetyState::default()
}

fn command(ty: ValueType, numeric: i32) -> InboundMessage<'static> {
    InboundMessage {
        sensor: 0,
        ty,
        boolean: true,
        numeric,
        text: "",
    }
}

/// A calibrated cover: 21 s up, 20 s down, fully open.
fn calibrated(rig: &Rig) {
    let layout = StoreLayout::default();
    rig.store.preset(layout.shutter_up_time, 21);
    rig.store.preset(layout.shutter_down_time, 20);
    rig.store.preset(layout.shutter_position, 0);
}

#[test]
fn presents_a_single_cover_child() {
    let rig = Rig::new();
    let mut d = roller_shutter(&rig, true);
    d.present(&rig.bus, 10);

    assert!(rig.bus.any(|e| matches!(
        e,
        Event::Presented {
            sensor: 0,
            class: SensorClass::Cover,
            name
        } if name == "Roller Shutter"
    )));
}

#[test]
fn initial_state_covers_up_down_stop_and_position() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, true);
    d.begin();
    d.send_initial_state(&rig.bus, 2000);

    for ty in [ValueType::Up, ValueType::Down, ValueType::Stop] {
        assert_eq!(rig.bus.last_bool(0, ty), Some(false), "{ty:?}");
        assert!(rig.bus.any(|e| matches!(
            e,
            Event::Requested { sensor: 0, ty: t } if *t == ty
        )));
    }
    assert_eq!(rig.bus.last_uint(0, ValueType::Percentage), Some(0));
}

#[test]
fn down_command_starts_the_motor_and_announces_it() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, true);
    d.begin();

    assert!(d.handle(&command(ValueType::Down, 0), &rig.bus, &clear()));
    d.tick(&rig.bus, 0);

    assert_eq!(d.shutter().motion(), Motion::Down);
    assert!(rig.gpio.level_of(pins::OUT2));
    assert_eq!(rig.bus.last_bool(0, ValueType::Down), Some(true));
}

#[test]
fn keeps_moving_while_the_motor_draws_current() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, true);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500); // start
    rig.bus.clear();

    d.tick(&rig.bus, 500); // motor still loaded
    assert_eq!(d.shutter().motion(), Motion::Down);
    assert_eq!(rig.bus.last_bool(0, ValueType::Stop), None);
}

#[test]
fn current_dropping_to_zero_means_an_end_stop_was_reached() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, true);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500);
    rig.bus.clear();

    d.tick(&rig.bus, 0); // motor no longer drawing
    assert_eq!(d.shutter().motion(), Motion::Stopped);
    assert_eq!(rig.bus.last_bool(0, ValueType::Stop), Some(true));
}

#[test]
fn without_current_sensing_zero_current_does_not_stop_it() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, false);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 0);
    rig.bus.clear();

    d.tick(&rig.bus, 0);
    assert_eq!(d.shutter().motion(), Motion::Down);
}

#[test]
fn stops_once_the_movement_time_elapses() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, false);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500); // starts, 20 s of travel requested
    rig.bus.clear();

    rig.clock.advance(20_500);
    d.tick(&rig.bus, 500);

    assert_eq!(d.shutter().motion(), Motion::Stopped);
    assert_eq!(rig.bus.last_bool(0, ValueType::Stop), Some(true));
    assert_eq!(d.shutter().position(), 100);
}

#[test]
fn explicit_stop_command_brakes() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, false);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500);
    rig.clock.advance(5_000);

    d.handle(&command(ValueType::Stop, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500);

    assert_eq!(d.shutter().motion(), Motion::Stopped);
    assert!(!rig.gpio.level_of(pins::OUT1));
    assert!(!rig.gpio.level_of(pins::OUT2));
}

#[test]
fn reversing_brakes_then_resumes_the_other_way() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, false);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500);
    rig.clock.advance(5_000);

    d.handle(&command(ValueType::Up, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500); // brakes, then resumes upward in the same pass

    assert_eq!(d.shutter().motion(), Motion::Up);
    assert!(rig.gpio.level_of(pins::OUT1));
    assert!(!rig.gpio.level_of(pins::OUT2));
    assert_eq!(rig.bus.last_bool(0, ValueType::Up), Some(true));
}

#[test]
fn position_is_persisted_when_movement_ends() {
    let rig = Rig::new();
    calibrated(&rig);
    let layout = StoreLayout::default();
    let mut d = roller_shutter(&rig, false);
    d.begin();

    d.handle(&command(ValueType::Percentage, 50), &rig.bus, &clear());
    d.tick(&rig.bus, 500); // 0% -> 50% is 10 s of the 20 s down travel

    // Freeze the clock so the elapsed time is exactly the requested movement
    // time; otherwise the reads inside tick() add their own milliseconds and the
    // integrated position lands a percent or two past the target.
    rig.clock.auto_advance_ms.set(0);
    rig.clock.advance(10_000);
    d.tick(&rig.bus, 500);

    assert_eq!(d.shutter().position(), 50);
    assert_eq!(rig.store.peek(layout.shutter_position), 50);
    assert_eq!(rig.bus.last_uint(0, ValueType::Percentage), Some(50));
}

#[test]
fn button_starts_movement_and_second_press_stops() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, false);
    d.begin();

    rig.gpio.script_short_press(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());
    d.tick(&rig.bus, 500);
    assert_eq!(d.shutter().motion(), Motion::Up);

    // A press is not accepted until a release has been *observed*, which takes
    // one poll of its own. Without this the second press is swallowed -- which is
    // the same rule that stops a held button streaming toggles.
    rig.gpio.script_idle(pins::BUTTON1);
    d.poll_buttons(&rig.bus, &clear());

    rig.gpio.script_short_press(pins::BUTTON1);
    d.poll_buttons(&rig.bus, &clear());
    d.tick(&rig.bus, 500);
    assert_eq!(d.shutter().motion(), Motion::Stopped);
}

#[test]
fn long_press_notifies_the_special_button_instead_of_moving() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, false);
    d.begin();

    rig.gpio.script_hold(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());
    d.tick(&rig.bus, 500);

    assert_eq!(
        rig.bus.last_bool(ids::SPECIAL_BUTTON_1, ValueType::Status),
        Some(true)
    );
    assert_eq!(d.shutter().motion(), Motion::Stopped);
}

#[test]
fn fault_stops_the_motor_and_reports_position() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, false);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500);
    rig.clock.advance(5_000);
    rig.bus.clear();

    let safety = SafetyState {
        thermal_fault: true,
        ..SafetyState::default()
    };
    d.shed_load(&rig.bus, &safety);

    assert_eq!(d.shutter().motion(), Motion::Stopped);
    assert_eq!(rig.bus.last_bool(0, ValueType::Stop), Some(true));
    assert!(rig.bus.any(|e| matches!(
        e,
        Event::SentUint {
            sensor: 0,
            ty: ValueType::Percentage,
            ..
        }
    )));

    // Idempotent: the module calls this every pass while the fault lasts.
    rig.bus.clear();
    d.shed_load(&rig.bus, &safety);
    assert!(rig.bus.events().is_empty());
}

#[test]
fn draws_current_only_while_moving() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, true);
    d.begin();
    assert!(!d.draws_current(0));
    assert_eq!(d.power_channel_count(), 1);

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500);
    assert!(d.draws_current(0));
}

#[test]
fn calibration_measures_both_directions_and_persists_them() {
    let rig = Rig::new();
    let layout = StoreLayout::default();
    let mut d = roller_shutter(&rig, true);
    d.begin();

    // The sensor reports a loaded motor until the caller has waited long enough;
    // Below the floor from the start, so each wait ends on its first check.
    rig.power[0].current.set(0);

    assert!(d.calibrate(&rig.bus, &rig.power[0], &rig.watchdog, 5000));

    assert!(d.shutter().calibrated());
    assert_eq!(rig.store.peek(layout.shutter_up_time), d.shutter().up_time_s());
    assert_eq!(
        rig.store.peek(layout.shutter_down_time),
        d.shutter().down_time_s()
    );
    assert_eq!(d.shutter().position(), 0);
    // The watchdog must be petted, or an eight-second traverse resets the node.
    assert!(rig.watchdog.pets.get() > 0);
    assert_eq!(rig.bus.last_bool(0, ValueType::Stop), Some(true));
}

#[test]
fn calibration_is_refused_without_a_current_sensor() {
    let rig = Rig::new();
    let mut d = roller_shutter(&rig, false);
    d.begin();
    assert!(!d.calibrate(&rig.bus, &rig.power[0], &rig.watchdog, 5000));
}

#[test]
fn maintenance_stops_the_motor_first() {
    let rig = Rig::new();
    calibrated(&rig);
    let mut d = roller_shutter(&rig, true);
    d.begin();

    d.handle(&command(ValueType::Down, 0), &rig.bus, &clear());
    d.tick(&rig.bus, 500);
    assert_eq!(d.shutter().motion(), Motion::Down);

    d.prepare_for_maintenance();
    assert_eq!(d.shutter().motion(), Motion::Stopped);
    assert!(!rig.gpio.level_of(pins::OUT1));
    assert!(!rig.gpio.level_of(pins::OUT2));
}
