//! Roller-shutter position model.

use crate::domain::shutter::{Motion, Shutter, ShutterPins};
use crate::domain::StoreLayout;
use crate::fakes::{FakePlatform, Rig};

const UP_PIN: u8 = 5;
const DOWN_PIN: u8 = 9;

fn shutter(rig: &Rig) -> Shutter<'_, FakePlatform> {
    Shutter::new(
        &rig.gpio,
        &rig.clock,
        &rig.store,
        ShutterPins {
            up: UP_PIN,
            down: DOWN_PIN,
            off_level: false,
        },
        StoreLayout::default(),
    )
}

/// A shutter that has been calibrated to 21 s up / 20 s down at position 0.
fn calibrated(rig: &Rig) -> Shutter<'_, FakePlatform> {
    let layout = StoreLayout::default();
    rig.store.preset(layout.shutter_up_time, 21);
    rig.store.preset(layout.shutter_down_time, 20);
    rig.store.preset(layout.shutter_position, 0);
    let mut s = shutter(rig);
    s.begin(21, 20);
    s
}

#[test]
fn blank_eeprom_falls_back_to_configured_times_and_stays_uncalibrated() {
    let rig = Rig::new();
    let mut s = shutter(&rig);
    s.begin(21, 20);

    assert!(!s.calibrated());
    assert_eq!(s.up_time_s(), 21);
    assert_eq!(s.down_time_s(), 20);
    assert_eq!(s.position(), 0);
}

#[test]
fn persisted_values_are_restored() {
    let rig = Rig::new();
    let layout = StoreLayout::default();
    rig.store.preset(layout.shutter_up_time, 30);
    rig.store.preset(layout.shutter_down_time, 28);
    rig.store.preset(layout.shutter_position, 45);

    let mut s = shutter(&rig);
    s.begin(21, 20);

    assert!(s.calibrated());
    assert_eq!(s.up_time_s(), 30);
    assert_eq!(s.down_time_s(), 28);
    assert_eq!(s.position(), 45);
}

#[test]
fn both_relays_start_de_energised() {
    let rig = Rig::new();
    shutter(&rig).begin(21, 20);
    assert!(!rig.gpio.level_of(UP_PIN));
    assert!(!rig.gpio.level_of(DOWN_PIN));
}

#[test]
fn full_traverse_uses_the_direction_time() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    assert_eq!(s.request(Motion::Down), 20_000);
    assert_eq!(s.pending(), Motion::Down);
    assert_eq!(s.request(Motion::Up), 21_000);
    assert_eq!(s.request(Motion::Stopped), 0);
}

#[test]
fn position_request_scales_by_the_remaining_distance() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    // Fully open to 50% closed: half of the 20 s down travel.
    assert_eq!(s.request_position(50), 10_000);
    assert_eq!(s.pending(), Motion::Down);
}

#[test]
fn position_request_picks_upward_when_opening() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);
    s.set_position(80);

    // 80% closed to 30%: 50% of the 21 s up travel.
    assert_eq!(s.request_position(30), 10_500);
    assert_eq!(s.pending(), Motion::Up);
}

#[test]
fn position_request_is_clamped_and_noops_when_already_there() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    assert_eq!(s.request_position(0), 0); // already fully open
    assert_eq!(s.pending(), Motion::Stopped);

    // Beyond the end stops, clamped rather than rejected.
    assert_eq!(s.request_position(150), 20_000);
    s.set_position(100);
    assert_eq!(s.request_position(-20), 21_000);
}

#[test]
fn button_for_the_running_direction_stops() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    s.request(Motion::Down);
    s.apply();
    assert_eq!(s.motion(), Motion::Down);

    assert_eq!(s.request_button(1), 0); // button 1 is down
    assert_eq!(s.pending(), Motion::Stopped);
}

#[test]
fn button_for_the_other_direction_reverses() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    s.request(Motion::Down);
    s.apply();

    assert_eq!(s.request_button(0), 21_000); // button 0 is up
    assert_eq!(s.pending(), Motion::Up);
}

#[test]
fn apply_energises_only_the_requested_direction() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    s.request(Motion::Down);
    s.apply();
    assert!(!rig.gpio.level_of(UP_PIN));
    assert!(rig.gpio.level_of(DOWN_PIN));
}

/// Both relays energised at once shorts the motor windings.
#[test]
fn apply_breaks_before_making_on_a_reversal() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    s.request(Motion::Down);
    s.apply();
    rig.gpio.clear_writes();
    rig.clock.delays.borrow_mut().clear();

    s.request(Motion::Up);
    s.apply();

    // Down must be released, then a settling delay, then up energised.
    let writes = rig.gpio.writes.borrow().clone();
    let down_off = writes
        .iter()
        .position(|w| w.pin == DOWN_PIN && !w.high)
        .expect("down relay released");
    let up_on = writes
        .iter()
        .position(|w| w.pin == UP_PIN && w.high)
        .expect("up relay energised");
    assert!(down_off < up_on, "made before breaking: {writes:?}");
    assert_eq!(rig.clock.delays.borrow().as_slice(), &[50]);
}

#[test]
fn apply_stop_releases_both_relays() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    s.request(Motion::Down);
    s.apply();
    s.request(Motion::Stopped);
    s.apply();

    assert!(!rig.gpio.level_of(UP_PIN));
    assert!(!rig.gpio.level_of(DOWN_PIN));
    assert_eq!(s.motion(), Motion::Stopped);
}

#[test]
fn full_downward_traverse_closes_completely() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);
    s.advance(Motion::Down, 20_000);
    assert_eq!(s.position(), 100);
}

#[test]
fn partial_traverse_is_proportional() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);
    s.advance(Motion::Down, 5_000); // a quarter of 20 s
    assert_eq!(s.position(), 25);
}

#[test]
fn upward_movement_opens() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);
    s.set_position(100);
    s.advance(Motion::Up, 10_500); // half of 21 s
    assert_eq!(s.position(), 50);
}

#[test]
fn position_is_clamped_at_both_end_stops() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);

    s.advance(Motion::Down, 60_000); // three full traverses
    assert_eq!(s.position(), 100);

    s.advance(Motion::Up, 60_000);
    assert_eq!(s.position(), 0);
}

#[test]
fn persistence_round_trips() {
    let rig = Rig::new();
    let layout = StoreLayout::default();
    {
        let mut s = calibrated(&rig);
        s.set_position(63);
        s.persist_position();
    }
    assert_eq!(rig.store.peek(layout.shutter_position), 63);

    let mut s = shutter(&rig);
    s.begin(21, 20);
    assert_eq!(s.position(), 63);
}

/// The cell is rated for ~100k erases and is written on every movement.
#[test]
fn unchanged_position_is_not_rewritten() {
    let rig = Rig::new();
    let mut s = calibrated(&rig);
    s.set_position(40);
    s.persist_position();

    let after_first = rig.store.effective_writes.get();
    s.persist_position();
    s.persist_position();
    assert_eq!(rig.store.effective_writes.get(), after_first);
}

#[test]
fn storing_travel_times_marks_it_calibrated_and_persists() {
    let rig = Rig::new();
    let layout = StoreLayout::default();
    let mut s = shutter(&rig);
    s.begin(21, 20);
    assert!(!s.calibrated());

    s.set_travel_times(33, 31);

    assert!(s.calibrated());
    assert_eq!(s.up_time_s(), 33);
    assert_eq!(s.down_time_s(), 31);
    assert_eq!(rig.store.peek(layout.shutter_up_time), 33);
    assert_eq!(rig.store.peek(layout.shutter_down_time), 31);
}

/// Nothing sensible can be integrated without a travel time, and dividing by it
/// would be worse than doing nothing.
#[test]
fn zero_travel_time_leaves_position_untouched() {
    let rig = Rig::new();
    let mut s = shutter(&rig);
    s.begin(0, 0);
    s.set_position(50);
    s.advance(Motion::Down, 5_000);
    assert_eq!(s.position(), 50);
}
