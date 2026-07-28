#include "roller_shutter_device.h"

namespace gw {

namespace {

/// Settling time after a direction change, and after telling the controller a
/// movement started, before the next command is acted on.
constexpr uint32_t kMovementSettleMs = 500;

constexpr SensorId kShutterId = 0;

} // namespace

RollerShutterDevice::RollerShutterDevice(IGpio& gpio, IClock& clock, IStore& store,
                                         const Spec& spec, const StoreLayout& layout,
                                         const ButtonTiming& button_timing,
                                         bool special_button_enabled)
    : shutter_(gpio, clock, store, spec.pins, layout),
      buttons_{{gpio, clock, spec.button_pins[0], false, button_timing.longpress_ms,
                button_timing.debounce_ms},
               {gpio, clock, spec.button_pins[1], false, button_timing.longpress_ms,
                button_timing.debounce_ms}},
      clock_(clock), spec_(spec), special_button_enabled_(special_button_enabled)
{
}

void RollerShutterDevice::begin()
{
    shutter_.begin(spec_.default_up_time_s, spec_.default_down_time_s);
    buttons_[0].begin();
    buttons_[1].begin();
}

void RollerShutterDevice::present(IBus& bus, uint16_t presentation_delay_ms)
{
    bus.present(kShutterId, SensorClass::Cover, GW_TEXT("Roller Shutter"));
    bus.wait(presentation_delay_ms);
}

void RollerShutterDevice::send_initial_state(IBus& bus, uint16_t echo_timeout_ms)
{
    bus.send_bool(kShutterId, ValueType::Up, false);
    bus.request(kShutterId, ValueType::Up);
    bus.wait_for_set(echo_timeout_ms, ValueType::Up);

    bus.send_bool(kShutterId, ValueType::Down, false);
    bus.request(kShutterId, ValueType::Down);
    bus.wait_for_set(echo_timeout_ms, ValueType::Down);

    bus.send_bool(kShutterId, ValueType::Stop, false);
    bus.request(kShutterId, ValueType::Stop);
    bus.wait_for_set(echo_timeout_ms, ValueType::Stop);

    bus.send_uint(kShutterId, ValueType::Percentage, shutter_.position());
    bus.request(kShutterId, ValueType::Percentage);
    bus.wait_for_set(echo_timeout_ms, ValueType::Percentage);
}

bool RollerShutterDevice::handle(const InboundMessage& msg, IBus& bus, const SafetyState& safety)
{
    (void)bus;
    (void)safety;

    if (msg.sensor != kShutterId) {
        return false;
    }

    switch (msg.type) {
    case ValueType::Percentage:
        movement_time_ms_ = shutter_.request_position(static_cast<int>(msg.numeric));
        return true;
    case ValueType::Up:
        movement_time_ms_ = shutter_.request(ShutterMotion::Up);
        return true;
    case ValueType::Down:
        movement_time_ms_ = shutter_.request(ShutterMotion::Down);
        return true;
    case ValueType::Stop:
        movement_time_ms_ = shutter_.request(ShutterMotion::Stopped);
        return true;
    default:
        return false;
    }
}

void RollerShutterDevice::poll_buttons(IBus& bus, const SafetyState& safety)
{
    for (uint8_t i = 0; i < 2; ++i) {
        switch (buttons_[i].poll()) {
        case ButtonEvent::Toggle:
            if (safety.blocks(0)) {
                break;
            }
            movement_time_ms_ = shutter_.request_button(i);
            break;

        case ButtonEvent::LongPress:
            if (special_button_enabled_) {
                bus.send_bool(static_cast<SensorId>(ids::kSpecialButton1 + i), ValueType::Status,
                              true);
            }
            break;

        case ButtonEvent::None:
            break;
        }
    }
}

void RollerShutterDevice::start_movement(IBus& bus)
{
    shutter_.apply();
    started_at_ms_ = clock_.now_ms();
    bus.send_bool(kShutterId,
                  shutter_.motion() == ShutterMotion::Up ? ValueType::Up : ValueType::Down, true);
    bus.wait(kMovementSettleMs);
}

void RollerShutterDevice::finish_movement(IBus& bus, uint32_t stopped_at_ms)
{
    const ShutterMotion direction = shutter_.motion();

    shutter_.set_pending(ShutterMotion::Stopped);
    shutter_.apply();
    bus.send_bool(kShutterId, ValueType::Stop, true);

    // Unsigned subtraction is rollover-correct, which is why the original's
    // explicit 0xFFFFFFFF fix-up branch is gone.
    shutter_.advance(direction, stopped_at_ms - started_at_ms_);
    shutter_.persist_position();
    bus.send_uint(kShutterId, ValueType::Percentage, shutter_.position());

    if (resume_ != ShutterMotion::Stopped) {
        bus.wait(kMovementSettleMs);
        shutter_.set_pending(resume_);
        resume_ = ShutterMotion::Stopped;
        start_movement(bus);
    }
}

void RollerShutterDevice::tick(IBus& bus, float current_a)
{
    const uint32_t now = clock_.now_ms();
    bool stop_now = false;
    uint32_t stopped_at = now;

    if (shutter_.motion() != ShutterMotion::Stopped) {
        if (now - started_at_ms_ >= movement_time_ms_) {
            stop_now = true;
        } else if (spec_.current_sensing && current_a < spec_.current_floor) {
            // Motor stopped drawing: the shutter reached an end stop early.
            stop_now = true;
        }
    }

    if (shutter_.motion() != shutter_.pending()) {
        if (shutter_.pending() == ShutterMotion::Stopped) {
            stop_now = true;
        } else if (shutter_.motion() == ShutterMotion::Stopped) {
            start_movement(bus);
            return;
        } else {
            // Reversal: brake first, then resume the other way.
            resume_ = shutter_.pending();
            stop_now = true;
        }
    }

    if (stop_now) {
        finish_movement(bus, stopped_at);
    }
}

void RollerShutterDevice::shed_load(IBus& bus, const SafetyState& safety)
{
    (void)safety; // one motor: any fault stops it
    if (shutter_.motion() == ShutterMotion::Stopped &&
        shutter_.pending() == ShutterMotion::Stopped) {
        return;
    }
    resume_ = ShutterMotion::Stopped;
    shutter_.set_pending(ShutterMotion::Stopped);
    finish_movement(bus, clock_.now_ms());
}

bool RollerShutterDevice::draws_current(uint8_t channel) const
{
    (void)channel;
    return shutter_.motion() != ShutterMotion::Stopped;
}

void RollerShutterDevice::prepare_for_maintenance()
{
    resume_ = ShutterMotion::Stopped;
    shutter_.set_pending(ShutterMotion::Stopped);
    shutter_.apply();
}

bool RollerShutterDevice::calibrate(IBus& bus, ICurrentSensor& sensor, IWatchdog& watchdog,
                                    float vcc_mv)
{
    if (!spec_.current_sensing) {
        return false; // no way to detect the end stops
    }

    // Drive fully open first so both directions are measured from a known end.
    shutter_.set_pending(ShutterMotion::Up);
    shutter_.apply();
    do {
        clock_.delay_ms(500);
        watchdog.pet();
    } while (sensor.measure_ac(vcc_mv) > spec_.current_floor);

    shutter_.set_pending(ShutterMotion::Stopped);
    shutter_.apply();
    clock_.delay_ms(1000);

    uint32_t down_total_s = 0;
    uint32_t up_total_s = 0;
    const uint8_t samples = spec_.calibration_samples == 0 ? 1 : spec_.calibration_samples;

    for (uint8_t i = 0; i < samples; ++i) {
        // Down first, then up, so each pass ends back at fully open.
        const ShutterMotion order[2] = {ShutterMotion::Down, ShutterMotion::Up};
        for (uint8_t j = 0; j < 2; ++j) {
            shutter_.set_pending(order[j]);
            shutter_.apply();
            const uint32_t start = clock_.now_ms();
            uint32_t stop = start;

            do {
                clock_.delay_ms(250);
                stop = clock_.now_ms();
                watchdog.pet();
            } while (sensor.measure_ac(vcc_mv) > spec_.current_floor);

            shutter_.set_pending(ShutterMotion::Stopped);
            shutter_.apply();

            const uint32_t measured_s = (stop - start) / 1000u;
            if (order[j] == ShutterMotion::Down) {
                down_total_s += measured_s;
            } else {
                up_total_s += measured_s;
            }

            clock_.delay_ms(1000);
        }
    }

    // +1 second of margin, as in the original, so a commanded full traverse
    // definitely reaches the end stop.
    const uint8_t up_s = static_cast<uint8_t>(up_total_s / samples + 1);
    const uint8_t down_s = static_cast<uint8_t>(down_total_s / samples + 1);

    shutter_.set_travel_times(up_s, down_s);
    shutter_.set_position(0);
    shutter_.persist_position();

    bus.send_bool(kShutterId, ValueType::Stop, true);
    bus.send_uint(kShutterId, ValueType::Percentage, shutter_.position());
    return true;
}

} // namespace gw
