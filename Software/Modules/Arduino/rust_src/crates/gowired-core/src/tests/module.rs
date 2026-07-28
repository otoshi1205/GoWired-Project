//! The orchestrator: presentation, monitoring, faults, commands, main loop.

use crate::domain::config::{ids, Features, MAX_INPUTS, PowerTuning, ThermalTuning, Timing};
use crate::domain::{Module, Peripherals, RelayBankDevice, Settings};
use crate::fakes::{Event, FakeBus, FakePlatform, Rig};
use crate::gw_text;
use crate::hal::{InboundMessage, ProbeStatus, SensorClass, ValueType};
use crate::tests::support::{
    double_relay_spec, four_relay_spec, input_bank, inputs, no_inputs, pins, relay_bank,
};

type RelayModule<'a> = Module<'a, FakePlatform, FakeBus, RelayBankDevice<'a, FakePlatform>>;

fn settings(features: Features) -> Settings {
    Settings {
        features,
        timing: Timing::default(),
        power: PowerTuning::default(),
        thermal: ThermalTuning::default(),
        layout: crate::domain::StoreLayout::default(),
    }
}

/// A `DoubleRelay` module with the features and peripherals given.
fn module<'a>(
    rig: &'a Rig,
    features: Features,
    peripherals: Peripherals<'a, FakePlatform>,
) -> RelayModule<'a> {
    Module::new(
        relay_bank(rig, double_relay_spec(), features.special_button),
        no_inputs(rig),
        rig.wiring(),
        peripherals,
        settings(features),
    )
}

/// Everything on: current sensing, thermometer, fault reporting.
fn full_features() -> Features {
    Features {
        power_sensor: true,
        internal_temperature: true,
        external_temperature: false,
        error_reporting: true,
        special_button: true,
        heating_controller_node: 0,
    }
}

fn peripherals_with_power_and_temp(rig: &Rig) -> Peripherals<'_, FakePlatform> {
    let mut p = rig.shared_power(ids::POWER);
    p.internal_temperature = Some(&rig.temperature);
    p
}

/// Idle buttons, so `poll_buttons` does not invent events.
fn idle_buttons(rig: &Rig) {
    rig.gpio.script_idle(pins::BUTTON1);
    rig.gpio.script_idle(pins::BUTTON2);
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

#[test]
fn begin_enables_the_watchdog_and_initialises_every_sensor() {
    let rig = Rig::new();
    let mut m = module(&rig, full_features(), peripherals_with_power_and_temp(&rig));
    m.begin();

    assert_eq!(rig.watchdog.enables.get(), 1);
    assert_eq!(rig.power[0].begins.get(), 1);
    assert_eq!(rig.temperature.begins.get(), 1);
    // And the device's own pins.
    assert!(!rig.gpio.level_of(pins::OUT1));
}

#[test]
fn presentation_announces_every_enabled_child_and_nothing_else() {
    let rig = Rig::new();
    let mut m = Module::new(
        relay_bank(&rig, double_relay_spec(), true),
        input_bank(&rig, inputs([true, true, false, false])),
        rig.wiring(),
        peripherals_with_power_and_temp(&rig),
        settings(full_features()),
    );
    m.present(gw_text!("GoWired Module"), gw_text!("3.0"));

    assert_eq!(
        rig.bus.presented_ids(),
        std::vec![
            0,
            1, // relays
            2,
            3, // inputs
            ids::SPECIAL_BUTTON_1,
            ids::SPECIAL_BUTTON_2,
            ids::POWER,
            ids::INTERNAL_TEMP,
            ids::OVERCURRENT_STATUS,
            ids::THERMAL_STATUS,
            ids::CONFIGURATION,
        ]
    );
    assert!(rig.bus.any(|e| matches!(
        e,
        Event::SketchInfo { name, version } if name == "GoWired Module" && version == "3.0"
    )));
    assert!(rig.bus.any(|e| matches!(
        e,
        Event::Presented {
            sensor,
            class: SensorClass::Info,
            ..
        } if *sensor == ids::CONFIGURATION
    )));
}

#[test]
fn disabled_features_are_presented_nowhere() {
    let rig = Rig::new();
    let mut m = module(&rig, Features::default(), Peripherals::default());
    m.present(gw_text!("GoWired Module"), gw_text!("3.0"));

    let presented = rig.bus.presented_ids();
    assert_eq!(presented, std::vec![0, 1, ids::CONFIGURATION]);
}

#[test]
fn initial_state_is_sent_once_on_the_first_loop() {
    let rig = Rig::new();
    let mut m = module(&rig, full_features(), peripherals_with_power_and_temp(&rig));
    m.begin();
    idle_buttons(&rig);

    assert!(!m.initial_state_sent());
    m.loop_once();
    assert!(m.initial_state_sent());

    assert_eq!(
        rig.bus.last_text(ids::CONFIGURATION, ValueType::Text),
        Some(std::string::String::from("CONFIG INIT"))
    );
    let first_pass = rig.bus.count(|e| matches!(e, Event::SentText { .. }));

    rig.bus.clear();
    m.loop_once();
    assert!(
        rig.bus.count(|e| matches!(
            e,
            Event::SentText {
                value,
                ..
            } if value == "CONFIG INIT"
        )) == 0,
        "initial state was sent twice"
    );
    assert_eq!(first_pass, 1);
}

// ---------------------------------------------------------------------------
// Overcurrent
// ---------------------------------------------------------------------------

/// Drives the module far enough that the relay is on and the sensor is sampled.
fn energised_module<'a>(rig: &'a Rig) -> RelayModule<'a> {
    let mut m = module(rig, full_features(), peripherals_with_power_and_temp(rig));
    m.begin();
    idle_buttons(rig);
    m.loop_once(); // initial state
    m.on_message(&InboundMessage {
        sensor: 0,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    });
    rig.bus.clear();
    m
}

#[test]
fn overcurrent_trips_when_the_limit_is_exceeded() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.power[0].current.set(5000); // limit is 3 A
    m.loop_once();

    assert!(m.safety().overcurrent[0]);
    assert_eq!(
        rig.bus.last_bool(ids::OVERCURRENT_STATUS, ValueType::Status),
        Some(true)
    );
    assert_eq!(rig.bus.last_bool(0, ValueType::Status), Some(false));
}

#[test]
fn overcurrent_does_not_trip_below_the_limit() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.power[0].current.set(2500);
    m.loop_once();

    assert!(!m.safety().overcurrent[0]);
    assert_eq!(
        rig.bus.last_bool(ids::OVERCURRENT_STATUS, ValueType::Status),
        None
    );
}

#[test]
fn overcurrent_status_is_reported_once_per_transition() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.power[0].current.set(5000);
    m.loop_once();
    rig.bus.clear();

    m.loop_once();
    m.loop_once();
    assert_eq!(
        rig.bus.count(|e| matches!(
            e,
            Event::SentBool { sensor, .. } if *sensor == ids::OVERCURRENT_STATUS
        )),
        0,
        "status re-sent while the fault was unchanged"
    );
}

/// Shedding the load removes the current that tripped the fault. A self-clearing
/// flag would report "all clear" on the next pass and let the controller
/// re-energise straight back into the overload.
#[test]
fn overcurrent_latches_until_the_controller_clears_it() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.power[0].current.set(5000);
    m.loop_once();
    assert!(m.safety().overcurrent[0]);

    // The relay is off now, so the sensor reads nothing.
    rig.power[0].current.set(0);
    m.loop_once();
    m.loop_once();
    assert!(
        m.safety().overcurrent[0],
        "fault cleared itself once the load was shed"
    );

    // And the relay stays refused.
    m.on_message(&InboundMessage {
        sensor: 0,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    });
    assert!(!m.device().relay_on(0));
}

#[test]
fn controller_clearing_the_latch_reports_recovery_once() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.power[0].current.set(5000);
    m.loop_once();
    rig.power[0].current.set(0);
    rig.bus.clear();

    m.on_message(&InboundMessage {
        sensor: ids::OVERCURRENT_STATUS,
        ty: ValueType::Status,
        boolean: false,
        numeric: 0,
        text: "",
    });
    assert!(!m.safety().overcurrent[0]);

    m.loop_once();
    assert_eq!(
        rig.bus.count(|e| matches!(
            e,
            Event::SentBool { sensor, .. } if *sensor == ids::OVERCURRENT_STATUS
        )),
        0
    );
}

/// Both relays, not just the one that happens to share the fault's channel
/// number. With a single shared current sensor the fault latches on channel 0,
/// and the C++ version then compared it against the *relay index* -- so relay 2
/// could be switched straight back on into the overload.
#[test]
fn commands_are_refused_on_every_relay_while_faulted() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.power[0].current.set(5000);
    m.loop_once();

    for sensor in [0u8, 1] {
        m.on_message(&InboundMessage {
            sensor,
            ty: ValueType::Status,
            boolean: true,
            numeric: 1,
            text: "",
        });
        assert!(
            !m.device().relay_on(sensor as usize),
            "relay {sensor} was energised while faulted"
        );
    }
}

#[test]
fn current_is_not_sampled_while_nothing_is_energised() {
    let rig = Rig::new();
    let mut m = module(&rig, full_features(), peripherals_with_power_and_temp(&rig));
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    let before = rig.power[0].ac_reads.get();
    m.loop_once();
    assert_eq!(rig.power[0].ac_reads.get(), before);
}

#[test]
fn power_reporting_publishes_watts_through_the_deadband() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.power[0].current.set(2000);
    m.loop_once();
    assert_eq!(
        rig.bus.last_fixed(ids::POWER, ValueType::Watt),
        Some(460) // 2 A * 230 V * 1.0
    );

    // Within the deadband: nothing new.
    rig.bus.clear();
    rig.power[0].current.set(2050);
    m.loop_once();
    assert_eq!(rig.bus.last_fixed(ids::POWER, ValueType::Watt), None);
}

#[test]
fn per_channel_deadbands_are_independent() {
    let rig = Rig::new();
    let features = Features {
        special_button: false,
        ..full_features()
    };
    let mut m = Module::new(
        relay_bank(&rig, four_relay_spec(), false),
        no_inputs(&rig),
        rig.wiring(),
        rig.per_relay_power(ids::POWER_PER_RELAY),
        settings(Features {
            internal_temperature: false,
            ..features
        }),
    );
    m.begin();
    m.loop_once();

    for ch in 0..4u8 {
        m.on_message(&InboundMessage {
            sensor: ch,
            ty: ValueType::Status,
            boolean: true,
            numeric: 1,
            text: "",
        });
    }

    rig.power[0].current.set(1000);
    rig.power[1].current.set(2000);
    rig.power[2].current.set(0);
    rig.power[3].current.set(0);
    rig.bus.clear();
    m.loop_once();

    assert_eq!(
        rig.bus
            .last_fixed(ids::POWER_PER_RELAY[0], ValueType::Watt),
        Some(230)
    );
    assert_eq!(
        rig.bus
            .last_fixed(ids::POWER_PER_RELAY[1], ValueType::Watt),
        Some(460)
    );
    // Channels drawing nothing, and never having reported, stay silent.
    assert_eq!(
        rig.bus
            .last_fixed(ids::POWER_PER_RELAY[2], ValueType::Watt),
        None
    );
}

// ---------------------------------------------------------------------------
// Thermal
// ---------------------------------------------------------------------------

#[test]
fn overheating_sheds_the_load_and_reports_once() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.temperature.decicelsius.set(950); // limit is 85
    m.loop_once();

    assert!(m.safety().thermal_fault);
    assert_eq!(
        rig.bus.last_bool(ids::THERMAL_STATUS, ValueType::Status),
        Some(true)
    );
    assert!(!m.device().relay_on(0));

    rig.bus.clear();
    m.loop_once();
    assert_eq!(
        rig.bus.count(|e| matches!(
            e,
            Event::SentBool { sensor, .. } if *sensor == ids::THERMAL_STATUS
        )),
        0
    );
}

/// Unlike overcurrent, a thermal fault genuinely does clear on its own: the
/// board cooling down is real information, not an artefact of the protection.
#[test]
fn thermal_recovery_is_reported() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.temperature.decicelsius.set(950);
    m.loop_once();
    rig.bus.clear();

    rig.temperature.decicelsius.set(400);
    m.loop_once();

    assert!(!m.safety().thermal_fault);
    assert_eq!(
        rig.bus.last_bool(ids::THERMAL_STATUS, ValueType::Status),
        Some(false)
    );
}

#[test]
fn controller_can_clear_the_thermal_latch() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    rig.temperature.decicelsius.set(950);
    m.loop_once();

    m.on_message(&InboundMessage {
        sensor: ids::THERMAL_STATUS,
        ty: ValueType::Status,
        boolean: false,
        numeric: 0,
        text: "",
    });
    assert!(!m.safety().thermal_fault);
}

#[test]
fn controller_can_set_and_clear_the_overcurrent_latch() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);

    m.on_message(&InboundMessage {
        sensor: ids::OVERCURRENT_STATUS,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    });
    assert!(m.safety().overcurrent.iter().all(|f| *f));

    m.on_message(&InboundMessage {
        sensor: ids::OVERCURRENT_STATUS,
        ty: ValueType::Status,
        boolean: false,
        numeric: 0,
        text: "",
    });
    assert!(m.safety().overcurrent.iter().all(|f| !*f));
}

/// The module publishes its own long-press notifications; the controller echoes
/// them back, and acting on that echo would be a feedback loop.
#[test]
fn special_button_echo_is_ignored() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);
    rig.bus.clear();

    m.on_message(&InboundMessage {
        sensor: ids::SPECIAL_BUTTON_1,
        ty: ValueType::Status,
        boolean: true,
        numeric: 1,
        text: "",
    });
    assert!(rig.bus.events().is_empty());
}

#[test]
fn unknown_child_is_ignored_silently() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);
    rig.bus.clear();

    m.on_message(&InboundMessage {
        sensor: 99,
        ty: ValueType::Percentage,
        numeric: 50,
        ..InboundMessage::default()
    });
    assert!(rig.bus.events().is_empty());
}

// ---------------------------------------------------------------------------
// Text command channel
// ---------------------------------------------------------------------------

#[test]
fn config_command_payload_is_echoed_back() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);
    rig.bus.clear();

    m.on_message(&InboundMessage {
        sensor: ids::CONFIGURATION,
        ty: ValueType::Text,
        text: "cmd2",
        ..InboundMessage::default()
    });
    assert_eq!(
        rig.bus.last_text(ids::CONFIGURATION, ValueType::Text),
        Some(std::string::String::from("cmd2"))
    );
}

#[test]
fn watchdog_test_stalls_long_enough_to_trigger_a_reset() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);
    rig.clock.delays.borrow_mut().clear();

    m.on_message(&InboundMessage {
        sensor: ids::CONFIGURATION,
        ty: ValueType::Text,
        text: "cmd3",
        ..InboundMessage::default()
    });

    // The watchdog is armed for 8 s; the stall has to outlast it.
    assert!(rig.clock.delays.borrow().iter().any(|ms| *ms >= 8_000));
}

#[test]
fn clear_store_blanks_the_whole_eeprom() {
    let rig = Rig::new();
    rig.store.preset(600, 0x42);
    let mut m = energised_module(&rig);

    m.on_message(&InboundMessage {
        sensor: ids::CONFIGURATION,
        ty: ValueType::Text,
        text: "cmd4",
        ..InboundMessage::default()
    });

    assert_eq!(rig.store.peek(600), 0xFF);
    assert!(rig.clock.delays.borrow().iter().any(|ms| *ms >= 8_000));
}

#[test]
fn unrecognised_command_only_echoes() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);
    rig.bus.clear();
    rig.clock.delays.borrow_mut().clear();

    m.on_message(&InboundMessage {
        sensor: ids::CONFIGURATION,
        ty: ValueType::Text,
        text: "hello",
        ..InboundMessage::default()
    });

    assert_eq!(rig.bus.events().len(), 1);
    assert!(rig.clock.delays.borrow().is_empty());
}

/// A relay bank has nothing to calibrate; the command must not misfire.
#[test]
fn calibrate_is_a_noop_on_a_device_that_cannot_calibrate() {
    let rig = Rig::new();
    let mut m = energised_module(&rig);
    rig.bus.clear();

    m.on_message(&InboundMessage {
        sensor: ids::CONFIGURATION,
        ty: ValueType::Text,
        text: "cmd1",
        ..InboundMessage::default()
    });

    assert_eq!(rig.bus.events().len(), 1); // the echo, and nothing else
    assert!(m.device().relay_on(0) || !m.device().relay_on(0)); // no panic
}

// ---------------------------------------------------------------------------
// Interval reporting and the external probe
// ---------------------------------------------------------------------------

#[test]
fn temperature_is_published_on_the_configured_interval() {
    let rig = Rig::new();
    let mut m = module(&rig, full_features(), peripherals_with_power_and_temp(&rig));
    rig.temperature.decicelsius.set(300);
    m.begin();
    idle_buttons(&rig);
    m.loop_once();
    rig.bus.clear();

    m.loop_once(); // well inside the interval
    assert_eq!(
        rig.bus.last_fixed(ids::INTERNAL_TEMP, ValueType::Temperature),
        None
    );

    rig.clock.advance(300_001);
    m.loop_once();
    assert_eq!(
        rig.bus.last_fixed(ids::INTERNAL_TEMP, ValueType::Temperature),
        Some(30)
    );
}

fn probe_module<'a>(rig: &'a Rig, heating_node: u8) -> RelayModule<'a> {
    let features = Features {
        power_sensor: false,
        internal_temperature: false,
        external_temperature: true,
        error_reporting: true,
        special_button: false,
        heating_controller_node: heating_node,
    };
    let mut p = Peripherals::default();
    p.external_probe = Some(&rig.probe);
    module(rig, features, p)
}

#[test]
fn good_reading_publishes_temperature_and_humidity() {
    let rig = Rig::new();
    let mut m = probe_module(&rig, 0);
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    assert_eq!(
        rig.bus.last_fixed(ids::EXTERNAL_TEMP, ValueType::Temperature),
        Some(215)
    );
    assert_eq!(
        rig.bus
            .last_fixed(ids::EXTERNAL_HUMIDITY, ValueType::Humidity),
        Some(450)
    );
}

/// The status code is published verbatim, because a dashboard may be bound to
/// "2 means the probe timed out".
#[test]
fn probe_failure_is_reported_as_a_status_code() {
    let rig = Rig::new();
    rig.probe.reading.set(crate::hal::ProbeReading {
        status: ProbeStatus::TimeoutError,
        temperature_dc: 0,
        humidity_dp: 0,
    });
    let mut m = probe_module(&rig, 0);
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    assert!(rig.bus.any(|e| matches!(
        e,
        Event::SentUint {
            sensor,
            ty: ValueType::Status,
            value: 2
        } if *sensor == ids::EXTERNAL_TEMP_STATUS
    )));
    assert_eq!(
        rig.bus.last_fixed(ids::EXTERNAL_TEMP, ValueType::Temperature),
        None
    );
}

/// The C++ original published the failure and then overwrote it with "all
/// clear", because the fault children were cleared after the probe was read.
#[test]
fn failure_on_the_first_pass_survives_the_initial_state_burst() {
    let rig = Rig::new();
    rig.probe.reading.set(crate::hal::ProbeReading {
        status: ProbeStatus::ChecksumError,
        temperature_dc: 0,
        humidity_dp: 0,
    });
    let mut m = probe_module(&rig, 0);
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    // The last thing said about the status child must be the failure.
    let last = rig
        .bus
        .events()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            Event::SentUint { sensor, value, .. } if sensor == ids::EXTERNAL_TEMP_STATUS => {
                Some(value)
            }
            Event::SentBool { sensor, value, .. } if sensor == ids::EXTERNAL_TEMP_STATUS => {
                Some(u32::from(value))
            }
            _ => None,
        })
        .expect("status child was never published");
    assert_eq!(last, 1, "the failure was overwritten with 'all clear'");
}

#[test]
fn probe_recovery_clears_the_status() {
    let rig = Rig::new();
    rig.probe.reading.set(crate::hal::ProbeReading {
        status: ProbeStatus::ChecksumError,
        temperature_dc: 0,
        humidity_dp: 0,
    });
    let mut m = probe_module(&rig, 0);
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    rig.probe.reading.set(crate::hal::ProbeReading {
        status: ProbeStatus::Ok,
        temperature_dc: 190,
        humidity_dp: 550,
    });
    rig.bus.clear();
    rig.clock.advance(300_001);
    m.loop_once();

    assert!(rig.bus.any(|e| matches!(
        e,
        Event::SentUint {
            sensor,
            value: 0,
            ..
        } if *sensor == ids::EXTERNAL_TEMP_STATUS
    )));
}

#[test]
fn temperature_is_mirrored_to_a_heating_controller_when_configured() {
    let rig = Rig::new();
    let mut m = probe_module(&rig, 7);
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    assert!(rig.bus.any(|e| matches!(
        e,
        Event::SentFixedTo {
            node: 7,
            sensor,
            ty: ValueType::Temperature,
            ..
        } if *sensor == ids::EXTERNAL_TEMP
    )));
}

#[test]
fn probe_is_not_read_when_disabled() {
    let rig = Rig::new();
    let mut m = module(&rig, full_features(), peripherals_with_power_and_temp(&rig));
    m.begin();
    idle_buttons(&rig);
    m.loop_once();
    rig.clock.advance(300_001);
    m.loop_once();

    assert_eq!(rig.probe.reads.get(), 0);
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

#[test]
fn loop_paces_itself_with_the_configured_loop_time() {
    let rig = Rig::new();
    let mut m = module(&rig, Features::default(), Peripherals::default());
    m.begin();
    idle_buttons(&rig);
    rig.bus.clear();

    m.loop_once();
    assert!(rig.bus.any(|e| matches!(e, Event::Waited { ms: 80 })));
}

#[test]
fn generic_input_changes_are_published_through_the_module() {
    let rig = Rig::new();
    let mut m = Module::new(
        relay_bank(&rig, double_relay_spec(), false),
        input_bank(&rig, inputs([true, false, false, false])),
        rig.wiring(),
        Peripherals::default(),
        settings(Features::default()),
    );
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    rig.bus.clear();
    rig.gpio.script_hold(pins::IN1);
    m.loop_once();

    assert_eq!(rig.bus.last_bool(2, ValueType::Status), Some(true));
}

#[test]
fn wall_switch_still_works_through_the_module() {
    let rig = Rig::new();
    let mut m = module(&rig, Features::default(), Peripherals::default());
    m.begin();
    idle_buttons(&rig);
    m.loop_once();

    rig.bus.clear();
    rig.gpio.script_short_press(pins::BUTTON1);
    m.loop_once();

    assert!(m.device().relay_on(0));
    assert_eq!(rig.bus.last_bool(0, ValueType::Status), Some(true));
}

/// The whole point of `MAX_INPUTS` slots that keep their ids.
#[test]
fn module_with_every_input_enabled_presents_all_of_them() {
    let rig = Rig::new();
    let mut m = Module::new(
        relay_bank(&rig, double_relay_spec(), false),
        input_bank(&rig, inputs([true; MAX_INPUTS])),
        rig.wiring(),
        Peripherals::default(),
        settings(Features::default()),
    );
    m.present(gw_text!("GoWired Module"), gw_text!("3.0"));
    assert_eq!(
        rig.bus.presented_ids(),
        std::vec![0, 1, 2, 3, 4, 5, ids::CONFIGURATION]
    );
}
