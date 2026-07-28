//! `DIMMER`, `RGB` and `RGBW`.

use crate::domain::config::{ids, ColorModel};
use crate::domain::device::{Device, SafetyState};
use crate::fakes::{Event, Rig};
use crate::hal::{InboundMessage, SensorClass, ValueType};
use crate::tests::support::{dimmer_device, pins};

fn clear() -> SafetyState {
    SafetyState::default()
}

#[test]
fn white_dimmer_presents_as_s_dimmer() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::White);
    d.present(&rig.bus, 10);

    assert!(rig.bus.any(|e| matches!(
        e,
        Event::Presented {
            sensor: 0,
            class: SensorClass::Dimmer,
            name
        } if name == "Dimmer"
    )));
}

#[test]
fn rgb_and_rgbw_present_their_own_classes() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.present(&rig.bus, 10);
    assert!(rig.bus.any(|e| matches!(
        e,
        Event::Presented {
            class: SensorClass::RgbLight,
            ..
        }
    )));

    let rig2 = Rig::new();
    let mut d2 = dimmer_device(&rig2, ColorModel::Rgbw);
    d2.present(&rig2.bus, 10);
    assert!(rig2.bus.any(|e| matches!(
        e,
        Event::Presented {
            class: SensorClass::RgbwLight,
            ..
        }
    )));
}

#[test]
fn white_advertises_no_colour_child() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::White);
    d.send_initial_state(&rig.bus, 2000);

    assert_eq!(rig.bus.last_bool(0, ValueType::Status), Some(false));
    assert_eq!(rig.bus.last_uint(0, ValueType::Percentage), Some(20));
    assert_eq!(rig.bus.last_text(0, ValueType::Rgb), None);
    assert_eq!(rig.bus.last_text(0, ValueType::Rgbw), None);
}

#[test]
fn rgb_advertises_six_hex_digits() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.send_initial_state(&rig.bus, 2000);
    assert_eq!(
        rig.bus.last_text(0, ValueType::Rgb).as_deref(),
        Some("ffffff")
    );
}

#[test]
fn rgbw_advertises_eight_hex_digits() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgbw);
    d.send_initial_state(&rig.bus, 2000);
    assert_eq!(
        rig.bus.last_text(0, ValueType::Rgbw).as_deref(),
        Some("ffffffff")
    );
}

#[test]
fn status_switches_the_strip() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    let on = InboundMessage {
        sensor: 0,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    };
    assert!(d.handle(&on, &rig.bus, &clear()));
    assert!(d.dimmer().is_on());
}

#[test]
fn percentage_sets_and_clamps_brightness() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    let pct = |n: i32| InboundMessage {
        sensor: 0,
        ty: ValueType::Percentage,
        numeric: n,
        ..InboundMessage::default()
    };

    assert!(d.handle(&pct(60), &rig.bus, &clear()));
    assert_eq!(d.dimmer().target_level(), 60);

    d.handle(&pct(500), &rig.bus, &clear());
    assert_eq!(d.dimmer().target_level(), 100);

    // A negative percentage is nonsense; clamp rather than wrap to 255.
    d.handle(&pct(-5), &rig.bus, &clear());
    assert_eq!(d.dimmer().target_level(), 0);
}

#[test]
fn rgb_payload_from_a_controller_is_applied() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    let colour = InboundMessage {
        sensor: 0,
        ty: ValueType::Rgb,
        text: "ff0000",
        ..InboundMessage::default()
    };
    assert!(d.handle(&colour, &rig.bus, &clear()));

    let on = InboundMessage {
        sensor: 0,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    };
    d.handle(&on, &rig.bus, &clear());
    assert_eq!(d.dimmer().channel_value(0), 0xFF);
}

#[test]
fn foreign_children_and_types_are_not_claimed() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    let other_child = InboundMessage {
        sensor: 3,
        ty: ValueType::Status,
        boolean: true,
        ..InboundMessage::default()
    };
    assert!(!d.handle(&other_child, &rig.bus, &clear()));

    let other_type = InboundMessage {
        sensor: 0,
        ty: ValueType::Up,
        ..InboundMessage::default()
    };
    assert!(!d.handle(&other_type, &rig.bus, &clear()));
}

#[test]
fn first_button_toggles_and_reports() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    rig.gpio.script_short_press(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());

    assert!(d.dimmer().is_on());
    assert_eq!(rig.bus.last_bool(0, ValueType::Status), Some(true));
}

#[test]
fn second_button_steps_brightness_while_on() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    // Switch on with button 1 first.
    rig.gpio.script_short_press(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());
    let before = d.dimmer().target_level();
    rig.bus.clear();

    rig.gpio.script_idle(pins::BUTTON1);
    rig.gpio.script_short_press(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());

    assert_eq!(d.dimmer().target_level(), before + 20);
    assert_eq!(
        rig.bus.last_uint(0, ValueType::Percentage),
        Some(u32::from(before + 20))
    );
}

/// Stepping the brightness of a strip that is off would be invisible.
#[test]
fn second_button_is_inert_while_off() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    rig.gpio.script_idle(pins::BUTTON1);
    rig.gpio.script_short_press(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());

    assert!(!d.dimmer().is_on());
    assert!(rig.bus.events().is_empty());
}

#[test]
fn each_button_has_its_own_longpress_child() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    rig.gpio.script_hold(pins::BUTTON1);
    rig.gpio.script_hold(pins::BUTTON2);
    d.poll_buttons(&rig.bus, &clear());

    assert_eq!(
        rig.bus.last_bool(ids::SPECIAL_BUTTON_1, ValueType::Status),
        Some(true)
    );
    assert_eq!(
        rig.bus.last_bool(ids::SPECIAL_BUTTON_2, ValueType::Status),
        Some(true)
    );
}

#[test]
fn shed_load_switches_off_and_reports_once() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    let on = InboundMessage {
        sensor: 0,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    };
    d.handle(&on, &rig.bus, &clear());
    rig.bus.clear();

    d.shed_load(&rig.bus, &clear());
    assert!(!d.dimmer().is_on());
    assert_eq!(rig.bus.last_bool(0, ValueType::Status), Some(false));

    rig.bus.clear();
    d.shed_load(&rig.bus, &clear());
    assert!(rig.bus.events().is_empty());
}

#[test]
fn led_strips_are_measured_as_dc_and_only_while_lit() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    assert!(d.uses_dc_measurement());
    assert_eq!(d.power_channel_count(), 1);
    assert!(!d.draws_current(0));

    let on = InboundMessage {
        sensor: 0,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    };
    d.handle(&on, &rig.bus, &clear());
    assert!(d.draws_current(0));
}

/// An RGB strip has three channels; the shield's white pin must stay dark.
#[test]
fn rgb_leaves_the_white_pin_alone() {
    let rig = Rig::new();
    let mut d = dimmer_device(&rig, ColorModel::Rgb);
    d.begin();

    let on = InboundMessage {
        sensor: 0,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    };
    d.handle(&on, &rig.bus, &clear());
    d.tick(&rig.bus, 0);

    // led_pins for a colour model are [W, R, G, B]; a 3-channel strip drives the
    // first three, so OUT3 -- the fourth entry -- is never touched.
    assert!(
        !rig.pwm
            .writes
            .borrow()
            .iter()
            .any(|(pin, _)| *pin == pins::OUT3)
    );
}
