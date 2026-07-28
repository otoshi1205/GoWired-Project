#include "module.h"

namespace gw {

namespace {

bool equals(const char* a, const char* b)
{
    if (a == nullptr || b == nullptr) {
        return false;
    }
    while (*a != '\0' && *b != '\0') {
        if (*a != *b) {
            return false;
        }
        ++a;
        ++b;
    }
    return *a == *b;
}

constexpr uint8_t kEepromBlank = 0xFF;

/// How long to stall so the 8 s watchdog fires and restarts the node.
constexpr uint32_t kWatchdogTriggerMs = 10000;

TextRef power_name(uint8_t channel)
{
    switch (channel) {
    case 0:
        return GW_TEXT("Power Sensor 1");
    case 1:
        return GW_TEXT("Power Sensor 2");
    case 2:
        return GW_TEXT("Power Sensor 3");
    default:
        return GW_TEXT("Power Sensor 4");
    }
}

} // namespace

Module::Module(IDevice& device, InputBank& inputs, IBus& bus, IClock& clock, IStore& store,
               IWatchdog& watchdog, IVoltageReference& vref, const Peripherals& peripherals,
               const Features& features, const Timing& timing, const PowerTuning& power,
               const ThermalTuning& thermal, const StoreLayout& layout)
    : device_(device), inputs_(inputs), bus_(bus), clock_(clock), store_(store),
      watchdog_(watchdog), vref_(vref), peripherals_(peripherals), features_(features),
      timing_(timing), layout_(layout),
      power_monitor_(power.max_current_a, power.receiver_voltage, power.cos_phi),
      thermal_monitor_(thermal.max_temperature_c)
{
}

void Module::begin()
{
    watchdog_.enable();
    vcc_mv_ = vref_.vcc_mv();

    for (uint8_t ch = 0; ch < peripherals_.power.count; ++ch) {
        if (peripherals_.power.sensor[ch] != nullptr) {
            peripherals_.power.sensor[ch]->begin(vcc_mv_);
        }
    }
    if (peripherals_.internal_temperature != nullptr) {
        peripherals_.internal_temperature->begin();
    }

    device_.begin();
    inputs_.begin();
}

void Module::present(TextRef sketch_name, TextRef sketch_version)
{
    bus_.send_sketch_info(sketch_name, sketch_version);

    device_.present(bus_, timing_.presentation_delay_ms);
    inputs_.present(bus_, timing_.presentation_delay_ms);

    if (features_.special_button) {
        bus_.present(ids::kSpecialButton1, SensorClass::Binary, GW_TEXT("Longpress-1"));
        bus_.wait(timing_.presentation_delay_ms);
        bus_.present(ids::kSpecialButton2, SensorClass::Binary, GW_TEXT("Longpress-2"));
        bus_.wait(timing_.presentation_delay_ms);
    }

    if (features_.power_sensor) {
        const bool multi = peripherals_.power.count > 1;
        for (uint8_t ch = 0; ch < peripherals_.power.count; ++ch) {
            bus_.present(peripherals_.power.id[ch], SensorClass::Power,
                         multi ? power_name(ch) : GW_TEXT("Power Sensor"));
            bus_.wait(timing_.presentation_delay_ms);
        }
    }

    if (features_.internal_temperature) {
        bus_.present(ids::kInternalTemp, SensorClass::Temperature, GW_TEXT("Internal Thermometer"));
        bus_.wait(timing_.presentation_delay_ms);
    }

    if (features_.external_temperature) {
        bus_.present(ids::kExternalTemp, SensorClass::Temperature, GW_TEXT("External Thermometer"));
        bus_.wait(timing_.presentation_delay_ms);
        bus_.present(ids::kExternalHumidity, SensorClass::Humidity, GW_TEXT("External Hygrometer"));
        bus_.wait(timing_.presentation_delay_ms);
    }

    if (features_.error_reporting) {
        if (features_.power_sensor) {
            bus_.present(ids::kOvercurrentStatus, SensorClass::Binary, GW_TEXT("OVERCURRENT ERROR"));
            bus_.wait(timing_.presentation_delay_ms);
        }
        if (features_.internal_temperature) {
            bus_.present(ids::kThermalStatus, SensorClass::Binary, GW_TEXT("THERMAL ERROR"));
            bus_.wait(timing_.presentation_delay_ms);
        }
        if (features_.external_temperature) {
            bus_.present(ids::kExternalTempStatus, SensorClass::Binary, GW_TEXT("ET STATUS"));
            bus_.wait(timing_.presentation_delay_ms);
        }
    }

    bus_.present(ids::kConfiguration, SensorClass::Info, GW_TEXT("TEXT Msg"));
}

void Module::send_initial_state()
{
    device_.send_initial_state(bus_, timing_.init_echo_timeout_ms);
    inputs_.send_initial_state(bus_);

    if (features_.special_button) {
        bus_.send_bool(ids::kSpecialButton1, ValueType::Status, false);
        bus_.send_bool(ids::kSpecialButton2, ValueType::Status, false);
    }

    if (features_.power_sensor) {
        for (uint8_t ch = 0; ch < peripherals_.power.count; ++ch) {
            bus_.send_float(peripherals_.power.id[ch], ValueType::Watt, 0.0f, 0);
        }
    }

    if (features_.internal_temperature && peripherals_.internal_temperature != nullptr) {
        bus_.send_float(ids::kInternalTemp, ValueType::Temperature,
                        peripherals_.internal_temperature->measure_celsius(vcc_mv_), 0);
    }

    // Fault children are cleared before the probe is read, so that if the probe
    // fails on this first pass its real status is what the controller is left
    // holding. The original order published the failure and then overwrote it
    // with "all clear".
    if (features_.error_reporting) {
        if (features_.power_sensor) {
            bus_.send_bool(ids::kOvercurrentStatus, ValueType::Status, false);
        }
        if (features_.internal_temperature) {
            bus_.send_bool(ids::kThermalStatus, ValueType::Status, false);
        }
        if (features_.external_temperature) {
            bus_.send_bool(ids::kExternalTempStatus, ValueType::Status, false);
        }
    }

    if (features_.external_temperature) {
        read_external_probe();
    }

    bus_.send_literal(ids::kConfiguration, ValueType::Text, GW_TEXT("CONFIG INIT"));
    initial_state_sent_ = true;
}

void Module::on_message(const InboundMessage& msg)
{
    if (device_.handle(msg, bus_, safety_)) {
        return;
    }

    if (msg.type == ValueType::Status) {
        if (features_.error_reporting && features_.power_sensor &&
            msg.sensor == ids::kOvercurrentStatus) {
            for (uint8_t ch = 0; ch < 4; ++ch) {
                safety_.overcurrent[ch] = msg.boolean;
            }
            overcurrent_.override_from_controller(msg.boolean);
            return;
        }
        if (features_.error_reporting && features_.internal_temperature &&
            msg.sensor == ids::kThermalStatus) {
            safety_.thermal_fault = msg.boolean;
            thermal_.override_from_controller(msg.boolean);
            return;
        }
        if (features_.special_button &&
            (msg.sensor == ids::kSpecialButton1 || msg.sensor == ids::kSpecialButton2)) {
            return; // echo of our own longpress notification
        }
    }

    if (msg.type == ValueType::Text && msg.sensor == ids::kConfiguration) {
        handle_config_command(msg.text);
    }
}

void Module::handle_config_command(const char* payload)
{
    // Echo the command back so the controller can see it was received. The
    // original round-tripped this through an Arduino String; on a 2 KB part
    // that is a heap allocation for no reason.
    bus_.send_text(ids::kConfiguration, ValueType::Text, payload);

    if (equals(payload, commands_.calibrate)) {
        device_.prepare_for_maintenance();
        if (peripherals_.power.sensor[0] != nullptr) {
            device_.calibrate(bus_, *peripherals_.power.sensor[0], watchdog_, vref_.vcc_mv());
        }
        return;
    }

    if (equals(payload, commands_.watchdog_test)) {
        clock_.delay_ms(kWatchdogTriggerMs);
        return;
    }

    if (equals(payload, commands_.clear_store)) {
        for (uint16_t address = 0; address < layout_.size; ++address) {
            store_.write(address, kEepromBlank);
        }
        clock_.delay_ms(kWatchdogTriggerMs);
    }
}

void Module::update_power()
{
    if (!features_.power_sensor) {
        return;
    }

    for (uint8_t ch = 0; ch < peripherals_.power.count; ++ch) {
        ICurrentSensor* sensor = peripherals_.power.sensor[ch];
        if (sensor == nullptr) {
            continue;
        }

        float amps = 0.0f;
        if (device_.draws_current(ch)) {
            amps = device_.uses_dc_measurement() ? sensor->measure_dc(vcc_mv_)
                                                 : sensor->measure_ac(vcc_mv_);
        }

        // The pre-refactor loop() measured this correctly and then immediately
        // overwrote it with the (stubbed, always-zero) dimmer reading, which
        // disabled overcurrent protection and made the roller shutter believe
        // its motor had stopped. Nothing clobbers it now.
        latest_current_ = amps;

        // Latching, not tracking. Shedding the load removes the very current
        // that tripped the fault, so a self-clearing flag would report the
        // fault gone on the next iteration and let the controller re-energise
        // straight back into the overload. The fault stays set until the
        // controller explicitly clears the status child -- which is what the
        // inbound handler for kOvercurrentStatus has always been for.
        if (features_.error_reporting && power_monitor_.over_limit(amps)) {
            safety_.overcurrent[ch] = true;
        }

        if (power_monitor_.should_report(amps, last_reported_current_[ch])) {
            bus_.send_float(peripherals_.power.id[ch], ValueType::Watt,
                            power_monitor_.power_w(amps), 0);
            last_reported_current_[ch] = amps;
        }
    }
}

void Module::enforce_overcurrent_limits()
{
    if (!features_.error_reporting || !features_.power_sensor) {
        return;
    }

    bool any = false;
    for (uint8_t ch = 0; ch < peripherals_.power.count; ++ch) {
        any = any || safety_.overcurrent[ch];
    }

    if (any) {
        device_.shed_load(bus_, safety_);
    }
    if (overcurrent_.update(any)) {
        bus_.send_bool(ids::kOvercurrentStatus, ValueType::Status, any);
    }
}

void Module::update_thermal()
{
    if (!features_.error_reporting || !features_.internal_temperature ||
        peripherals_.internal_temperature == nullptr) {
        return;
    }

    const float celsius = peripherals_.internal_temperature->measure_celsius(vcc_mv_);
    const bool fault = thermal_monitor_.over_limit(celsius);
    safety_.thermal_fault = fault;

    if (fault) {
        device_.shed_load(bus_, safety_);
    }
    if (thermal_.update(fault)) {
        bus_.send_bool(ids::kThermalStatus, ValueType::Status, fault);
        if (fault) {
            report_now_ = true; // get a temperature reading out immediately
        }
    }
}

void Module::read_external_probe()
{
    if (!features_.external_temperature || peripherals_.external_probe == nullptr) {
        return;
    }

    const IHygrometer::Reading reading = peripherals_.external_probe->read();

    if (reading.status != IHygrometer::Status::Ok) {
        if (features_.error_reporting) {
            external_status_ = reading.status;
            bus_.send_uint(ids::kExternalTempStatus, ValueType::Status,
                           static_cast<uint32_t>(reading.status));
        }
        return;
    }

    bus_.send_float(ids::kExternalTemp, ValueType::Temperature, reading.temperature_c, 1);
    bus_.send_float(ids::kExternalHumidity, ValueType::Humidity, reading.humidity_pct, 1);

    if (features_.heating_controller_node != 0) {
        bus_.send_float_to(features_.heating_controller_node, ids::kExternalTemp,
                           ValueType::Temperature, reading.temperature_c, 1);
    }

    if (features_.error_reporting && external_status_ != IHygrometer::Status::Ok) {
        external_status_ = IHygrometer::Status::Ok;
        bus_.send_uint(ids::kExternalTempStatus, ValueType::Status, 0);
    }
}

void Module::report_interval_sensors(float vcc_mv)
{
    if (features_.internal_temperature && peripherals_.internal_temperature != nullptr) {
        bus_.send_float(ids::kInternalTemp, ValueType::Temperature,
                        peripherals_.internal_temperature->measure_celsius(vcc_mv), 0);
    }
    if (features_.external_temperature) {
        read_external_probe();
    }
}

void Module::loop()
{
    vcc_mv_ = vref_.vcc_mv();

    // Home Assistant needs an initial value for every child before it will
    // show the entity, and the transport has to be up for that to land.
    if (!initial_state_sent_) {
        send_initial_state();
    }

    update_power();
    enforce_overcurrent_limits();
    update_thermal();

    device_.poll_buttons(bus_, safety_);
    inputs_.poll(bus_);
    device_.tick(bus_, latest_current_);

    // Unsigned subtraction makes the rollover check the original open-coded
    // unnecessary.
    const uint32_t now = clock_.now_ms();
    if (now - last_report_ms_ >= timing_.report_interval_ms || report_now_) {
        report_interval_sensors(vcc_mv_);
        last_report_ms_ = now;
        report_now_ = false;
    }

    bus_.wait(timing_.loop_time_ms);
}

} // namespace gw
