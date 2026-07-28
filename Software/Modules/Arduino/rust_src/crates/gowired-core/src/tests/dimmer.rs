//! PWM dimmer: colour parsing, brightness ramp, output scaling.

use crate::domain::config::DimmerTuning;
use crate::domain::dimmer::Dimmer;
use crate::fakes::{FakePlatform, Rig};

const PINS: [u8; 4] = [5, 9, 6, 10];

fn dimmer(rig: &Rig, channels: u8, tuning: DimmerTuning) -> Dimmer<'_, FakePlatform> {
    Dimmer::new(&rig.pwm, &rig.clock, PINS, channels, tuning)
}

fn rgb(rig: &Rig) -> Dimmer<'_, FakePlatform> {
    dimmer(rig, 3, DimmerTuning::default())
}

// ---------------------------------------------------------------------------
// Colour parsing. The original accepted upper case only.
// ---------------------------------------------------------------------------

#[test]
fn lower_case_hex_is_parsed() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    assert!(d.set_colors_from_hex("ff8000"));
    d.set_on(true);
    d.set_target_level(100);
    d.update();
    assert_eq!(d.channel_value(0), 0xFF);
    assert_eq!(d.channel_value(1), 0x80);
    assert_eq!(d.channel_value(2), 0x00);
}

#[test]
fn upper_case_hex_still_parses() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    assert!(d.set_colors_from_hex("FF8000"));
    d.set_on(true);
    d.update();
    assert_eq!(d.channel_value(0), 0xFF);
    assert_eq!(d.channel_value(1), 0x80);
}

#[test]
fn mixed_case_hex_parses() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    assert!(d.set_colors_from_hex("Ff8A0b"));
    d.set_on(true);
    d.update();
    assert_eq!(d.channel_value(0), 0xFF);
    assert_eq!(d.channel_value(1), 0x8A);
    assert_eq!(d.channel_value(2), 0x0B);
}

#[test]
fn hash_prefix_accepted_for_both_lengths() {
    let rig = Rig::new();
    assert!(rgb(&rig).set_colors_from_hex("#ff8000"));
    assert!(dimmer(&rig, 4, DimmerTuning::default()).set_colors_from_hex("#ff800040"));
}

#[test]
fn rgbw_consumes_the_fourth_byte() {
    let rig = Rig::new();
    let mut d = dimmer(&rig, 4, DimmerTuning::default());
    assert!(d.set_colors_from_hex("01020304"));
    d.set_on(true);
    d.set_target_level(100);
    d.update();
    assert_eq!(d.channel_value(3), 4);
}

/// A payload that fails halfway must leave the previous colour intact rather
/// than applying the bytes it managed to read.
#[test]
fn malformed_payload_is_rejected_without_partial_application() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.set_colors_from_hex("112233");
    d.set_on(true);
    d.set_target_level(100);
    d.update();

    assert!(!d.set_colors_from_hex("44zz66"));
    d.update();
    assert_eq!(d.channel_value(0), 0x11);
    assert_eq!(d.channel_value(1), 0x22);
    assert_eq!(d.channel_value(2), 0x33);
}

#[test]
fn wrong_length_is_rejected() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    for bad in ["", "f", "fff", "fffff", "fffffff", "fffffffff"] {
        assert!(!d.set_colors_from_hex(bad), "accepted {bad:?}");
    }
}

// ---------------------------------------------------------------------------
// Brightness
// ---------------------------------------------------------------------------

#[test]
fn target_is_clamped_to_one_hundred() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.set_target_level(250);
    assert_eq!(d.target_level(), 100);
}

#[test]
fn wall_switch_step_wraps_back_to_the_step_size() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.set_target_level(90);
    d.bump_level(20);
    assert_eq!(d.target_level(), 20);
}

/// The original compared `current != target` while stepping by `step`, so a step
/// that did not divide the distance overshot and span until the watchdog fired.
#[test]
fn ramp_terminates_when_the_step_does_not_divide_the_distance() {
    let rig = Rig::new();
    let mut d = dimmer(
        &rig,
        3,
        DimmerTuning {
            step: 7,
            interval_ms: 0,
            toggle_step: 20,
        },
    );
    d.set_target_level(100);
    d.set_on(true); // ramps 0 -> 100 in steps of 7
    assert_eq!(d.level(), 100);
}

#[test]
fn ramp_reaches_the_target_exactly() {
    let rig = Rig::new();
    let mut d = dimmer(
        &rig,
        3,
        DimmerTuning {
            step: 3,
            interval_ms: 0,
            toggle_step: 20,
        },
    );
    d.set_target_level(50);
    d.set_on(true);
    assert_eq!(d.level(), 50);
}

#[test]
fn switching_off_then_on_restores_the_requested_brightness() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.set_target_level(70);
    d.set_on(true);
    assert_eq!(d.level(), 70);

    d.set_on(false);
    assert!(!d.is_on());
    assert_eq!(d.target_level(), 70);

    d.set_on(true);
    assert_eq!(d.level(), 70);
}

#[test]
fn redundant_state_change_is_a_noop() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.set_on(true);
    let writes = rig.pwm.writes.borrow().len();
    d.set_on(true);
    assert_eq!(rig.pwm.writes.borrow().len(), writes);
}

#[test]
fn update_does_nothing_while_off() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.update();
    assert!(rig.pwm.writes.borrow().is_empty());
}

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

#[test]
fn duty_is_brightness_scaled_colour() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.set_colors_from_hex("c80000"); // 200, 0, 0
    d.set_target_level(50);
    d.set_on(true);
    d.update();

    assert_eq!(rig.pwm.duty_of(PINS[0]), 100); // 200 * 50 / 100
    assert_eq!(rig.pwm.duty_of(PINS[1]), 0);
}

#[test]
fn only_configured_channels_are_driven() {
    let rig = Rig::new();
    let mut d = rgb(&rig); // 3 channels
    d.set_on(true);
    d.update();
    assert!(
        !rig.pwm
            .writes
            .borrow()
            .iter()
            .any(|(pin, _)| *pin == PINS[3]),
        "fourth channel was driven on a 3-channel strip"
    );
}

#[test]
fn begin_zeroes_every_channel() {
    let rig = Rig::new();
    let mut d = dimmer(&rig, 4, DimmerTuning::default());
    d.begin();
    for pin in PINS {
        assert_eq!(rig.pwm.duty_of(pin), 0);
    }
    assert!(!d.is_on());
}

/// `level * value / 100` reaches 255 at full brightness on a full channel; it
/// must not wrap past it.
#[test]
fn duty_saturates_rather_than_wrapping() {
    let rig = Rig::new();
    let mut d = rgb(&rig);
    d.set_colors_from_hex("ffffff");
    d.set_target_level(100);
    d.set_on(true);
    d.update();
    assert_eq!(rig.pwm.duty_of(PINS[0]), 255);
}
