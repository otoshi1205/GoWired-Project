//! Debounced buttons and generic digital inputs.

use crate::domain::input::{Button, ButtonEvent, DigitalSensor};
use crate::fakes::{FakePlatform, Rig};
use crate::hal::{Gpio, PinMode};

const PIN: u8 = 2;

fn button(rig: &Rig) -> Button<'_, FakePlatform> {
    Button::new(&rig.gpio, &rig.clock, PIN, false, 1000, 50)
}

#[test]
fn configures_its_pin_as_pulled_up_input() {
    let rig = Rig::new();
    button(&rig).begin();
    assert_eq!(rig.gpio.mode_of(PIN), Some(PinMode::InputPullup));
}

#[test]
fn idle_input_produces_no_event() {
    let rig = Rig::new();
    rig.gpio.script_idle(PIN);
    assert_eq!(button(&rig).poll(), ButtonEvent::None);
}

#[test]
fn short_press_toggles() {
    let rig = Rig::new();
    rig.gpio.script_short_press(PIN);
    assert_eq!(button(&rig).poll(), ButtonEvent::Toggle);
}

#[test]
fn holding_past_the_threshold_is_a_long_press() {
    let rig = Rig::new();
    rig.gpio.script_hold(PIN);
    assert_eq!(button(&rig).poll(), ButtonEvent::LongPress);
}

/// A held button must not stream toggles on every main-loop pass.
#[test]
fn second_press_is_ignored_until_a_release_is_observed() {
    let rig = Rig::new();
    let mut b = button(&rig);

    rig.gpio.script(PIN, &[false, false, false]); // pressed, and stays pressed
    assert_eq!(b.poll(), ButtonEvent::LongPress);

    // Still held: nothing more happens.
    assert_eq!(b.poll(), ButtonEvent::None);

    // Released, then pressed again: a fresh toggle.
    rig.gpio.script_idle(PIN);
    assert_eq!(b.poll(), ButtonEvent::None);
    rig.gpio.script_short_press(PIN);
    assert_eq!(b.poll(), ButtonEvent::Toggle);
}

#[test]
fn short_spike_is_debounced_away() {
    let rig = Rig::new();
    // One low sample, then high again: never active long enough to count.
    rig.gpio.script(PIN, &[false, true]);
    assert_eq!(button(&rig).poll(), ButtonEvent::None);
}

/// `millis()` wraps every 49.7 days; a button poll straddling the wrap must not
/// stall until it comes round again.
#[test]
fn survives_millis_rollover() {
    let rig = Rig::new();
    rig.clock.set(u32::MAX - 10);
    rig.gpio.script_short_press(PIN);
    assert_eq!(button(&rig).poll(), ButtonEvent::Toggle);
}

// ---------------------------------------------------------------------------
// DigitalSensor
// ---------------------------------------------------------------------------

fn sensor(rig: &Rig, pullup: bool, invert: bool) -> DigitalSensor<'_, FakePlatform> {
    DigitalSensor::new(&rig.gpio, &rig.clock, PIN, invert, pullup, 50)
}

#[test]
fn pulled_up_variant_configures_pullup() {
    let rig = Rig::new();
    sensor(&rig, true, false).begin();
    assert_eq!(rig.gpio.mode_of(PIN), Some(PinMode::InputPullup));
}

#[test]
fn floating_variant_does_not_configure_pullup() {
    let rig = Rig::new();
    sensor(&rig, false, false).begin();
    assert_eq!(rig.gpio.mode_of(PIN), Some(PinMode::Input));
}

#[test]
fn reports_only_on_change() {
    let rig = Rig::new();
    let mut s = sensor(&rig, true, false);

    rig.gpio.script_idle(PIN);
    assert_eq!(s.poll(), None); // starts inactive, stays inactive

    rig.gpio.script_hold(PIN); // contact closes
    assert_eq!(s.poll(), Some(true));
    assert_eq!(s.poll(), None); // still closed
    assert!(s.level());

    rig.gpio.script_idle(PIN); // contact opens
    assert_eq!(s.poll(), Some(false));
    assert_eq!(s.poll(), None);
    assert!(!s.level());
}

/// `INVERT_n`: a sensor that drives the line high when triggered.
#[test]
fn inverted_polarity_flips_the_active_level() {
    let rig = Rig::new();
    let mut s = sensor(&rig, false, true);

    rig.gpio.script(PIN, &[true]); // high == active when inverted
    assert_eq!(s.poll(), Some(true));

    rig.gpio.script(PIN, &[false]);
    assert_eq!(s.poll(), Some(false));
}

/// Reading a pin must not disturb its configured mode -- the fake would happily
/// let that slide, so it is asserted.
#[test]
fn reading_does_not_reconfigure_the_pin() {
    let rig = Rig::new();
    let s = sensor(&rig, false, false);
    s.begin();
    rig.gpio.script_idle(PIN);
    let _ = rig.gpio.read(PIN);
    assert_eq!(rig.gpio.mode_of(PIN), Some(PinMode::Input));
}
