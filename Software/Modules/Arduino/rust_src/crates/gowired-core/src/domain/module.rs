//! The whole node, minus the hardware.
//!
//! This is what used to be `setup()`, `presentation()`, `InitConfirmation()`,
//! `receive()`, `UpdateIO()`, `PSUpdate()`, `ETUpdate()` and `loop()` in
//! `main.ino`, written once against [`Device`] instead of six times behind
//! `#ifdef`s.

use crate::domain::config::{
    ids, Features, PowerTuning, StoreLayout, ThermalTuning, Timing,
};
use crate::domain::device::{Device, SafetyState};
use crate::domain::input_bank::InputBank;
use crate::domain::monitors::{LatchedFault, PowerMonitor, ThermalMonitor};
use crate::gw_text;
use crate::hal::{
    Bus, Clock, CurrentSensor, Hygrometer, InboundMessage, Milliamps, Millivolts, Platform,
    ProbeStatus, SensorClass, SensorId, Store, TemperatureSensor, ValueType, VoltageReference,
    Watchdog, NO_SENSOR,
};
use crate::text::Text;

const EEPROM_BLANK: u8 = 0xFF;

/// How long to stall so the 8 s watchdog fires and restarts the node.
const WATCHDOG_TRIGGER_MS: u32 = 10_000;

fn power_name(channel: usize) -> Text {
    match channel {
        0 => gw_text!("Power Sensor 1"),
        1 => gw_text!("Power Sensor 2"),
        2 => gw_text!("Power Sensor 3"),
        _ => gw_text!("Power Sensor 4"),
    }
}

/// The current sensors, and the child ids their readings are published under.
pub struct PowerChannels<'a, P: Platform> {
    /// One sensor per channel; `None` means the channel is not sampled.
    pub sensor: [Option<&'a P::CurrentSensor>; 4],
    /// Child id each channel publishes under.
    pub id: [SensorId; 4],
    /// How many channels are live.
    pub count: u8,
}

impl<P: Platform> Default for PowerChannels<'_, P> {
    fn default() -> Self {
        Self {
            sensor: [None; 4],
            id: [NO_SENSOR; 4],
            count: 0,
        }
    }
}

/// Optional peripherals. `None` means "this board does not have one".
pub struct Peripherals<'a, P: Platform> {
    /// Current sensing.
    pub power: PowerChannels<'a, P>,
    /// On-board thermometer.
    pub internal_temperature: Option<&'a P::TemperatureSensor>,
    /// External temperature/humidity probe.
    pub external_probe: Option<&'a P::Hygrometer>,
}

impl<P: Platform> Default for Peripherals<'_, P> {
    fn default() -> Self {
        Self {
            power: PowerChannels::default(),
            internal_temperature: None,
            external_probe: None,
        }
    }
}

/// The shared hardware the module drives directly.
pub struct Wiring<'a, P: Platform, B: Bus> {
    /// Controller link.
    pub bus: &'a B,
    /// Time base.
    pub clock: &'a P::Clock,
    /// EEPROM.
    pub store: &'a P::Store,
    /// Watchdog.
    pub watchdog: &'a P::Watchdog,
    /// Supply voltage measurement.
    pub vref: &'a P::Vref,
}

/// Everything configurable that is not a pin.
#[derive(Copy, Clone, Debug, Default)]
pub struct Settings {
    /// Which optional peripherals are fitted.
    pub features: Features,
    /// Loop and reporting periods.
    pub timing: Timing,
    /// Current limit and load parameters.
    pub power: PowerTuning,
    /// Temperature limit and sensor calibration.
    pub thermal: ThermalTuning,
    /// EEPROM layout.
    pub layout: StoreLayout,
}

/// Text commands accepted on the configuration child.
///
/// Held as [`Text`], so on AVR the four command words stay in flash and are
/// matched against the inbound payload byte by byte.
#[derive(Copy, Clone)]
pub struct ConfigCommands {
    /// Run a calibration cycle.
    pub calibrate: Text,
    /// Unused, reserved by the original firmware.
    pub reserved: Text,
    /// Stall until the watchdog restarts the node.
    pub watchdog_test: Text,
    /// Erase the EEPROM and restart.
    pub clear_store: Text,
}

impl Default for ConfigCommands {
    fn default() -> Self {
        Self {
            calibrate: gw_text!("cmd1"),
            reserved: gw_text!("cmd2"),
            watchdog_test: gw_text!("cmd3"),
            clear_store: gw_text!("cmd4"),
        }
    }
}

/// The node.
pub struct Module<'a, P: Platform, B: Bus, D: Device<P>> {
    device: D,
    inputs: InputBank<'a, P>,
    wiring: Wiring<'a, P, B>,
    peripherals: Peripherals<'a, P>,
    settings: Settings,
    commands: ConfigCommands,

    power_monitor: PowerMonitor,
    thermal_monitor: ThermalMonitor,
    overcurrent: LatchedFault,
    thermal: LatchedFault,

    safety: SafetyState,
    /// Deadband state is per channel; `FourRelay` reports four independent loads.
    last_reported_current: [Milliamps; 4],
    latest_current: Milliamps,

    external_status: ProbeStatus,

    initial_state_sent: bool,
    report_now: bool,
    last_report_ms: u32,
    vcc_mv: Millivolts,
}

impl<'a, P: Platform, B: Bus, D: Device<P>> Module<'a, P, B, D> {
    /// Assembles the node from a device, its inputs, the shared hardware and the
    /// configuration.
    pub fn new(
        device: D,
        inputs: InputBank<'a, P>,
        wiring: Wiring<'a, P, B>,
        peripherals: Peripherals<'a, P>,
        settings: Settings,
    ) -> Self {
        Self {
            device,
            inputs,
            wiring,
            peripherals,
            settings,
            commands: ConfigCommands::default(),
            power_monitor: PowerMonitor::new(
                settings.power.max_current_a,
                settings.power.receiver_voltage,
                settings.power.cos_phi_percent,
            ),
            thermal_monitor: ThermalMonitor::new(settings.thermal.max_temperature_c),
            overcurrent: LatchedFault::new(),
            thermal: LatchedFault::new(),
            safety: SafetyState::default(),
            last_reported_current: [0; 4],
            latest_current: 0,
            external_status: ProbeStatus::Uninitialised,
            initial_state_sent: false,
            report_now: false,
            last_report_ms: 0,
            vcc_mv: 0,
        }
    }

    /// Configures every pin. Call from the firmware's startup.
    pub fn begin(&mut self) {
        self.wiring.watchdog.enable();
        self.vcc_mv = self.wiring.vref.vcc_mv();

        for ch in 0..self.power_count() {
            if let Some(sensor) = self.peripherals.power.sensor[ch] {
                sensor.begin(self.vcc_mv);
            }
        }
        if let Some(thermometer) = self.peripherals.internal_temperature {
            thermometer.begin();
        }

        self.device.begin();
        self.inputs.begin();
    }

    fn power_count(&self) -> usize {
        (self.peripherals.power.count as usize).min(4)
    }

    /// Announces every child to the controller.
    pub fn present(&mut self, sketch_name: Text, sketch_version: Text) {
        let bus = self.wiring.bus;
        let delay = self.settings.timing.presentation_delay_ms;
        let features = self.settings.features;

        bus.send_sketch_info(sketch_name, sketch_version);

        self.device.present(bus, delay);
        self.inputs.present(bus, delay);

        if features.special_button {
            bus.present(
                ids::SPECIAL_BUTTON_1,
                SensorClass::Binary,
                gw_text!("Longpress-1"),
            );
            bus.wait(u32::from(delay));
            bus.present(
                ids::SPECIAL_BUTTON_2,
                SensorClass::Binary,
                gw_text!("Longpress-2"),
            );
            bus.wait(u32::from(delay));
        }

        if features.power_sensor {
            let multi = self.peripherals.power.count > 1;
            for ch in 0..self.power_count() {
                bus.present(
                    self.peripherals.power.id[ch],
                    SensorClass::Power,
                    if multi {
                        power_name(ch)
                    } else {
                        gw_text!("Power Sensor")
                    },
                );
                bus.wait(u32::from(delay));
            }
        }

        if features.internal_temperature {
            bus.present(
                ids::INTERNAL_TEMP,
                SensorClass::Temperature,
                gw_text!("Internal Thermometer"),
            );
            bus.wait(u32::from(delay));
        }

        if features.external_temperature {
            bus.present(
                ids::EXTERNAL_TEMP,
                SensorClass::Temperature,
                gw_text!("External Thermometer"),
            );
            bus.wait(u32::from(delay));
            bus.present(
                ids::EXTERNAL_HUMIDITY,
                SensorClass::Humidity,
                gw_text!("External Hygrometer"),
            );
            bus.wait(u32::from(delay));
        }

        if features.error_reporting {
            if features.power_sensor {
                bus.present(
                    ids::OVERCURRENT_STATUS,
                    SensorClass::Binary,
                    gw_text!("OVERCURRENT ERROR"),
                );
                bus.wait(u32::from(delay));
            }
            if features.internal_temperature {
                bus.present(
                    ids::THERMAL_STATUS,
                    SensorClass::Binary,
                    gw_text!("THERMAL ERROR"),
                );
                bus.wait(u32::from(delay));
            }
            if features.external_temperature {
                bus.present(
                    ids::EXTERNAL_TEMP_STATUS,
                    SensorClass::Binary,
                    gw_text!("ET STATUS"),
                );
                bus.wait(u32::from(delay));
            }
        }

        bus.present(ids::CONFIGURATION, SensorClass::Info, gw_text!("TEXT Msg"));
    }

    fn send_initial_state(&mut self) {
        let bus = self.wiring.bus;
        let features = self.settings.features;

        self.device
            .send_initial_state(bus, self.settings.timing.init_echo_timeout_ms);
        self.inputs.send_initial_state(bus);

        if features.special_button {
            bus.send_bool(ids::SPECIAL_BUTTON_1, ValueType::Status, false);
            bus.send_bool(ids::SPECIAL_BUTTON_2, ValueType::Status, false);
        }

        if features.power_sensor {
            for ch in 0..self.power_count() {
                bus.send_fixed(self.peripherals.power.id[ch], ValueType::Watt, 0, 0);
            }
        }

        if features.internal_temperature {
            if let Some(thermometer) = self.peripherals.internal_temperature {
                // Whole degrees, as the original sent: the on-board sensor is
                // a thermal-fault detector, not a thermometer for display.
                bus.send_fixed(
                    ids::INTERNAL_TEMP,
                    ValueType::Temperature,
                    i32::from(thermometer.measure_decicelsius(self.vcc_mv) / 10),
                    0,
                );
            }
        }

        // Fault children are cleared before the probe is read, so that if the
        // probe fails on this first pass its real status is what the controller
        // is left holding. The original order published the failure and then
        // overwrote it with "all clear".
        if features.error_reporting {
            if features.power_sensor {
                bus.send_bool(ids::OVERCURRENT_STATUS, ValueType::Status, false);
            }
            if features.internal_temperature {
                bus.send_bool(ids::THERMAL_STATUS, ValueType::Status, false);
            }
            if features.external_temperature {
                bus.send_bool(ids::EXTERNAL_TEMP_STATUS, ValueType::Status, false);
            }
        }

        if features.external_temperature {
            self.read_external_probe();
        }

        bus.send_literal(ids::CONFIGURATION, ValueType::Text, gw_text!("CONFIG INIT"));
        self.initial_state_sent = true;
    }

    /// Handles one inbound message.
    pub fn on_message(&mut self, msg: &InboundMessage<'_>) {
        let bus = self.wiring.bus;
        let features = self.settings.features;

        if self.device.handle(msg, bus, &self.safety) {
            return;
        }

        if msg.ty == ValueType::Status {
            if features.error_reporting
                && features.power_sensor
                && msg.sensor == ids::OVERCURRENT_STATUS
            {
                self.safety.overcurrent = [msg.boolean; 4];
                self.overcurrent.override_from_controller(msg.boolean);
                return;
            }
            if features.error_reporting
                && features.internal_temperature
                && msg.sensor == ids::THERMAL_STATUS
            {
                self.safety.thermal_fault = msg.boolean;
                self.thermal.override_from_controller(msg.boolean);
                return;
            }
            if features.special_button
                && (msg.sensor == ids::SPECIAL_BUTTON_1 || msg.sensor == ids::SPECIAL_BUTTON_2)
            {
                return; // echo of our own longpress notification
            }
        }

        if msg.ty == ValueType::Text && msg.sensor == ids::CONFIGURATION {
            self.handle_config_command(msg.text);
        }
    }

    fn handle_config_command(&mut self, payload: &str) {
        let bus = self.wiring.bus;

        // Echo the command back so the controller can see it was received. The
        // original round-tripped this through an Arduino String; on a 2 KB part
        // that is a heap allocation for no reason.
        bus.send_text(ids::CONFIGURATION, ValueType::Text, payload);

        if self.commands.calibrate.equals(payload) {
            self.device.prepare_for_maintenance();
            if let Some(sensor) = self.peripherals.power.sensor[0] {
                let vcc = self.wiring.vref.vcc_mv();
                self.device
                    .calibrate(bus, sensor, self.wiring.watchdog, vcc);
            }
            return;
        }

        if self.commands.watchdog_test.equals(payload) {
            self.wiring.clock.delay_ms(WATCHDOG_TRIGGER_MS);
            return;
        }

        if self.commands.clear_store.equals(payload) {
            for address in 0..self.settings.layout.size {
                self.wiring.store.write(address, EEPROM_BLANK);
            }
            self.wiring.clock.delay_ms(WATCHDOG_TRIGGER_MS);
        }
    }

    fn update_power(&mut self) {
        if !self.settings.features.power_sensor {
            return;
        }
        let bus = self.wiring.bus;

        for ch in 0..self.power_count() {
            let Some(sensor) = self.peripherals.power.sensor[ch] else {
                continue;
            };

            let mut current: Milliamps = 0;
            if self.device.draws_current(ch as u8) {
                current = if self.device.uses_dc_measurement() {
                    sensor.measure_dc(self.vcc_mv)
                } else {
                    sensor.measure_ac(self.vcc_mv)
                };
            }

            // The pre-refactor loop() measured this correctly and then
            // immediately overwrote it with the (stubbed, always-zero) dimmer
            // reading, which disabled overcurrent protection and made the roller
            // shutter believe its motor had stopped. Nothing clobbers it now.
            self.latest_current = current;

            // Latching, not tracking. Shedding the load removes the very current
            // that tripped the fault, so a self-clearing flag would report the
            // fault gone on the next iteration and let the controller
            // re-energise straight back into the overload. The fault stays set
            // until the controller explicitly clears the status child -- which
            // is what the inbound handler for OVERCURRENT_STATUS has always been
            // for.
            if self.settings.features.error_reporting && self.power_monitor.over_limit(current) {
                self.safety.overcurrent[ch] = true;
            }

            if self
                .power_monitor
                .should_report(current, self.last_reported_current[ch])
            {
                bus.send_fixed(
                    self.peripherals.power.id[ch],
                    ValueType::Watt,
                    self.power_monitor.power_w(current) as i32,
                    0,
                );
                self.last_reported_current[ch] = current;
            }
        }
    }

    fn enforce_overcurrent_limits(&mut self) {
        if !self.settings.features.error_reporting || !self.settings.features.power_sensor {
            return;
        }

        let any = self.safety.overcurrent[..self.power_count()]
            .iter()
            .any(|&f| f);

        if any {
            self.device.shed_load(self.wiring.bus, &self.safety);
        }
        if self.overcurrent.update(any) {
            self.wiring
                .bus
                .send_bool(ids::OVERCURRENT_STATUS, ValueType::Status, any);
        }
    }

    fn update_thermal(&mut self) {
        if !self.settings.features.error_reporting || !self.settings.features.internal_temperature {
            return;
        }
        let Some(thermometer) = self.peripherals.internal_temperature else {
            return;
        };

        let temperature = thermometer.measure_decicelsius(self.vcc_mv);
        let fault = self.thermal_monitor.over_limit(temperature);
        self.safety.thermal_fault = fault;

        if fault {
            self.device.shed_load(self.wiring.bus, &self.safety);
        }
        if self.thermal.update(fault) {
            self.wiring
                .bus
                .send_bool(ids::THERMAL_STATUS, ValueType::Status, fault);
            if fault {
                self.report_now = true; // get a temperature reading out immediately
            }
        }
    }

    fn read_external_probe(&mut self) {
        if !self.settings.features.external_temperature {
            return;
        }
        let Some(probe) = self.peripherals.external_probe else {
            return;
        };
        let bus = self.wiring.bus;

        let reading = probe.read();

        if reading.status != ProbeStatus::Ok {
            if self.settings.features.error_reporting {
                self.external_status = reading.status;
                bus.send_uint(
                    ids::EXTERNAL_TEMP_STATUS,
                    ValueType::Status,
                    reading.status as u32,
                );
            }
            return;
        }

        bus.send_fixed(
            ids::EXTERNAL_TEMP,
            ValueType::Temperature,
            i32::from(reading.temperature_dc),
            1,
        );
        bus.send_fixed(
            ids::EXTERNAL_HUMIDITY,
            ValueType::Humidity,
            i32::from(reading.humidity_dp),
            1,
        );

        if self.settings.features.heating_controller_node != 0 {
            bus.send_fixed_to(
                self.settings.features.heating_controller_node,
                ids::EXTERNAL_TEMP,
                ValueType::Temperature,
                i32::from(reading.temperature_dc),
                1,
            );
        }

        if self.settings.features.error_reporting && self.external_status != ProbeStatus::Ok {
            self.external_status = ProbeStatus::Ok;
            bus.send_uint(ids::EXTERNAL_TEMP_STATUS, ValueType::Status, 0);
        }
    }

    fn report_interval_sensors(&mut self, vcc_mv: Millivolts) {
        if self.settings.features.internal_temperature {
            if let Some(thermometer) = self.peripherals.internal_temperature {
                self.wiring.bus.send_fixed(
                    ids::INTERNAL_TEMP,
                    ValueType::Temperature,
                    i32::from(thermometer.measure_decicelsius(vcc_mv) / 10),
                    0,
                );
            }
        }
        if self.settings.features.external_temperature {
            self.read_external_probe();
        }
    }

    /// One pass of the main loop.
    pub fn loop_once(&mut self) {
        self.vcc_mv = self.wiring.vref.vcc_mv();

        // Home Assistant needs an initial value for every child before it will
        // show the entity, and the transport has to be up for that to land.
        if !self.initial_state_sent {
            self.send_initial_state();
        }

        self.update_power();
        self.enforce_overcurrent_limits();
        self.update_thermal();

        self.device.poll_buttons(self.wiring.bus, &self.safety);
        self.inputs.poll(self.wiring.bus);
        self.device.tick(self.wiring.bus, self.latest_current);

        // Wrapping subtraction makes the rollover check the original open-coded
        // unnecessary.
        let now = self.wiring.clock.now_ms();
        if now.wrapping_sub(self.last_report_ms) >= self.settings.timing.report_interval_ms
            || self.report_now
        {
            let vcc = self.vcc_mv;
            self.report_interval_sensors(vcc);
            self.last_report_ms = now;
            self.report_now = false;
        }

        self.wiring
            .bus
            .wait(u32::from(self.settings.timing.loop_time_ms));
    }

    // -- observability, used by the tests -----------------------------------

    /// Whether the initial-state burst has been sent.
    pub const fn initial_state_sent(&self) -> bool {
        self.initial_state_sent
    }

    /// The current protective state.
    pub const fn safety(&self) -> &SafetyState {
        &self.safety
    }

    /// The device, for assertions about its state.
    pub const fn device(&self) -> &D {
        &self.device
    }
}
