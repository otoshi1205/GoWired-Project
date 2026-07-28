#include "dimmer_device.h"

namespace gw {

namespace {

constexpr SensorId kDimmerId = 0;

SensorClass sensor_class_for(ColorModel model)
{
    switch (model) {
    case ColorModel::Rgb:
        return SensorClass::RgbLight;
    case ColorModel::Rgbw:
        return SensorClass::RgbwLight;
    case ColorModel::White:
        break;
    }
    return SensorClass::Dimmer;
}

TextRef name_for(ColorModel model)
{
    switch (model) {
    case ColorModel::Rgb:
        return GW_TEXT("RGB");
    case ColorModel::Rgbw:
        return GW_TEXT("RGBW");
    case ColorModel::White:
        break;
    }
    return GW_TEXT("Dimmer");
}

} // namespace

DimmerDevice::DimmerDevice(IPwm& pwm, IGpio& gpio, IClock& clock, const Spec& spec,
                           const DimmerTuning& tuning, const ButtonTiming& button_timing,
                           bool special_button_enabled)
    : dimmer_(pwm, clock, spec.led_pins, channel_count(spec.model), tuning),
      buttons_{{gpio, clock, spec.button_pins[0], false, button_timing.longpress_ms,
                button_timing.debounce_ms},
               {gpio, clock, spec.button_pins[1], false, button_timing.longpress_ms,
                button_timing.debounce_ms}},
      spec_(spec), tuning_(tuning), special_button_enabled_(special_button_enabled)
{
}

void DimmerDevice::begin()
{
    dimmer_.begin();
    buttons_[0].begin();
    buttons_[1].begin();
}

void DimmerDevice::present(IBus& bus, uint16_t presentation_delay_ms)
{
    bus.present(kDimmerId, sensor_class_for(spec_.model), name_for(spec_.model));
    bus.wait(presentation_delay_ms);
}

bool DimmerDevice::color_value_type(ValueType& out) const
{
    switch (spec_.model) {
    case ColorModel::Rgb:
        out = ValueType::Rgb;
        return true;
    case ColorModel::Rgbw:
        out = ValueType::Rgbw;
        return true;
    case ColorModel::White:
        break;
    }
    return false;
}

void DimmerDevice::send_initial_state(IBus& bus, uint16_t echo_timeout_ms)
{
    bus.send_bool(kDimmerId, ValueType::Status, false);
    bus.request(kDimmerId, ValueType::Status);
    bus.wait_for_set(echo_timeout_ms, ValueType::Status);

    bus.send_uint(kDimmerId, ValueType::Percentage, dimmer_.target_level());
    bus.request(kDimmerId, ValueType::Percentage);
    bus.wait_for_set(echo_timeout_ms, ValueType::Percentage);

    ValueType color_type = ValueType::Rgb;
    if (color_value_type(color_type)) {
        bus.send_literal(kDimmerId, color_type,
                         color_type == ValueType::Rgb ? GW_TEXT("ffffff")
                                                      : GW_TEXT("ffffffff"));
        bus.request(kDimmerId, color_type);
        bus.wait_for_set(echo_timeout_ms, color_type);
    }
}

bool DimmerDevice::handle(const InboundMessage& msg, IBus& bus, const SafetyState& safety)
{
    (void)bus;
    (void)safety;

    if (msg.sensor != kDimmerId) {
        return false;
    }

    switch (msg.type) {
    case ValueType::Status:
        dimmer_.set_on(msg.boolean);
        return true;

    case ValueType::Percentage:
        dimmer_.set_target_level(
            static_cast<uint8_t>(msg.numeric < 0 ? 0 : (msg.numeric > 100 ? 100 : msg.numeric)));
        return true;

    case ValueType::Rgb:
    case ValueType::Rgbw:
        dimmer_.set_colors_from_hex(msg.text);
        return true;

    default:
        return false;
    }
}

void DimmerDevice::poll_buttons(IBus& bus, const SafetyState& safety)
{
    // Button 0 switches the strip; button 1 steps the brightness.
    switch (buttons_[0].poll()) {
    case ButtonEvent::Toggle:
        if (!safety.blocks(0)) {
            dimmer_.set_on(!dimmer_.is_on());
            bus.send_bool(kDimmerId, ValueType::Status, dimmer_.is_on());
        }
        break;
    case ButtonEvent::LongPress:
        if (special_button_enabled_) {
            bus.send_bool(ids::kSpecialButton1, ValueType::Status, true);
        }
        break;
    case ButtonEvent::None:
        break;
    }

    switch (buttons_[1].poll()) {
    case ButtonEvent::Toggle:
        // Stepping the brightness of a strip that is off would be invisible.
        if (dimmer_.is_on() && !safety.blocks(0)) {
            dimmer_.bump_level(tuning_.toggle_step);
            bus.send_uint(kDimmerId, ValueType::Percentage, dimmer_.target_level());
        }
        break;
    case ButtonEvent::LongPress:
        if (special_button_enabled_) {
            bus.send_bool(ids::kSpecialButton2, ValueType::Status, true);
        }
        break;
    case ButtonEvent::None:
        break;
    }
}

void DimmerDevice::tick(IBus& bus, float current_a)
{
    (void)bus;
    (void)current_a;
    dimmer_.update();
}

void DimmerDevice::shed_load(IBus& bus, const SafetyState& safety)
{
    (void)safety; // a single LED channel: any fault sheds it
    if (!dimmer_.is_on()) {
        return;
    }
    dimmer_.set_on(false);
    bus.send_bool(kDimmerId, ValueType::Status, dimmer_.is_on());
}

bool DimmerDevice::draws_current(uint8_t channel) const
{
    (void)channel;
    return dimmer_.is_on();
}

} // namespace gw
