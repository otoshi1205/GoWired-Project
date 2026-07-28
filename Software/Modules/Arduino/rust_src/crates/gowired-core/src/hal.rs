//! What the hardware has to provide.
//!
//! These are the seams the old C++ `IGpio` / `IPwm` / `IClock` / `IStore` /
//! `IBus` interfaces described, with two differences that both come from Rust
//! rather than from a change of design:
//!
//! 1. **No vtables.** Every trait here is used through a generic parameter, so
//!    the firmware's single choice of implementation is resolved at compile
//!    time and inlined. The C++ version reached the same end (only one device
//!    is linked in) by relying on the linker to drop the vtables of the five
//!    variants it did not build; here there is nothing to drop.
//!
//! 2. **`&self`, not `&mut self`.** The GPIO port and the ADC are global,
//!    shared hardware: a `Relay`, a `Button` and the `Shutter` all drive pins,
//!    and in C++ they all held an `IGpio&`. Rust will not hand out two `&mut`
//!    to one object, so the traits take `&self` and implementations that need
//!    to remember something (the current sensor's zero offset, the transport's
//!    receive state) use a `Cell` internally. That is honest about the
//!    hardware: writing `PORTB` is not an exclusive operation.
//!
//! [`Platform`] bundles the lot, so domain types carry one type parameter
//! instead of nine.

use crate::text::Text;

/// Arduino-style pin number: 0..7 = PORTD, 8..13 = PORTB, 14..21 = PORTC/A0-A7.
///
/// Kept as the Arduino numbering rather than a port/bit pair because that is
/// what the pin map in the firmware's `config.rs` is written in, and what the
/// board's documentation and silkscreen use.
pub type Pin = u8;

/// MySensors child id.
pub type SensorId = u8;

/// Current, in milliamps.
///
/// # Why not `f32`
///
/// Because `f32` arithmetic costs 6 kB of flash on this part, and there is 32 kB
/// of it. Rust's `compiler_builtins` supplies the soft-float routines on AVR and
/// they are generic Rust rather than avr-libc's hand-written assembly:
/// `__addsf3` alone is 2180 bytes. There is no supported way to make the linker
/// prefer avr-libc's, so the firmware simply does not reference them -- see
/// [`crate::proto::fixed`], which is the one place a float is *constructed*, for
/// the wire, with integer arithmetic.
///
/// The units are not a compromise either. A milliamp is well below what an
/// ACS712 on a 10-bit ADC can resolve (~26 mA per count at 185 mV/A), so nothing
/// measurable is lost, and integer arithmetic is exactly reproducible in tests.
pub type Milliamps = u16;

/// Voltage, in millivolts. Vcc is around 5000 or 3300.
pub type Millivolts = u16;

/// Temperature, in tenths of a degree Celsius. Also the wire precision.
pub type DeciCelsius = i16;

/// Relative humidity, in tenths of a percent.
pub type DeciPercent = i16;

/// Power, in watts.
pub type Watts = u32;

/// Sentinel for "no pin wired".
pub const NO_PIN: Pin = 0xFF;

/// Sentinel for "no child".
pub const NO_SENSOR: SensorId = 0xFF;

/// How a pin is driven.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PinMode {
    /// High-impedance input, no pull-up. For a sensor that drives the line.
    Input,
    /// Input with the internal pull-up enabled. For a dry contact to ground.
    InputPullup,
    /// Push-pull output.
    Output,
}

/// Digital input and output.
pub trait Gpio {
    /// Sets the direction and pull-up of a pin.
    fn configure(&self, pin: Pin, mode: PinMode);
    /// Reads the pin level. `true` is high.
    fn read(&self, pin: Pin) -> bool;
    /// Drives the pin. `true` is high.
    fn write(&self, pin: Pin, high: bool);
}

/// PWM output, used by the dimmer.
pub trait Pwm {
    /// Sets the duty cycle: 0 is off, 255 is full on.
    fn write_duty(&self, pin: Pin, duty: u8);
}

/// Millisecond time base.
pub trait Clock {
    /// Milliseconds since boot, wrapping every ~49.7 days.
    ///
    /// The domain code compares these with wrapping arithmetic, so the wrap is
    /// not a special case anywhere -- see [`crate::domain::module`].
    fn now_ms(&self) -> u32;

    /// Plain blocking delay.
    ///
    /// Does **not** service the transport. Where inbound messages must still be
    /// processed, [`Bus::wait`] is the one to call. The distinction is load
    /// bearing: using this one during presentation loses messages.
    fn delay_ms(&self, ms: u32);

    /// Short blocking delay, for bus turnaround timing.
    ///
    /// The RS485 driver needs a few microseconds between asserting DE and the
    /// first bit going out, which is far below the resolution of
    /// [`Clock::delay_ms`].
    fn delay_us(&self, us: u16);
}

/// Byte-addressable non-volatile storage (EEPROM).
pub trait Store {
    /// Reads one byte. A never-written cell reads as `0xFF`.
    fn read(&self, address: u16) -> u8;

    /// Writes one byte, skipping the write if the value is unchanged.
    ///
    /// Implementations **must** compare first. Shutter position is persisted on
    /// every movement and the cell is rated for ~100k erase cycles.
    fn write(&self, address: u16, value: u8);
}

/// Hardware watchdog.
pub trait Watchdog {
    /// Arms the watchdog.
    fn enable(&self);
    /// Disarms it.
    fn disable(&self);
    /// Resets the countdown.
    fn pet(&self);
}

/// Supply-voltage reference.
///
/// ADC readings are ratiometric to Vcc, so every measurement helper takes the
/// supply voltage as an explicit parameter rather than hiding a second
/// conversion inside itself.
pub trait VoltageReference {
    /// Measured Vcc.
    fn vcc_mv(&self) -> Millivolts;
}

/// Current transformer / hall sensor on one output channel.
pub trait CurrentSensor {
    /// Configures the pin and samples the quiescent ADC offset.
    fn begin(&self, vcc_mv: Millivolts);
    /// Peak-to-peak sampling, for mains loads.
    fn measure_ac(&self, vcc_mv: Millivolts) -> Milliamps;
    /// Averaged sampling, for DC loads such as LED strips.
    fn measure_dc(&self, vcc_mv: Millivolts) -> Milliamps;
}

/// On-board analog thermometer.
pub trait TemperatureSensor {
    /// Configures the pin.
    fn begin(&self);
    /// Reads the temperature.
    fn measure_decicelsius(&self, vcc_mv: Millivolts) -> DeciCelsius;
}

/// Why an external probe read failed.
///
/// The numeric values are a wire protocol: they are published verbatim on the
/// `ET STATUS` child, so a controller with a dashboard bound to "2 means the
/// probe timed out" keeps working.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ProbeStatus {
    /// Reading is good.
    Ok = 0,
    /// The probe answered but the checksum did not match.
    ChecksumError = 1,
    /// The probe did not answer in time.
    TimeoutError = 2,
    /// No reading has been attempted yet.
    Uninitialised = 3,
}

/// One reading from an external temperature/humidity probe.
#[derive(Copy, Clone, Debug)]
pub struct ProbeReading {
    /// Whether the two readings mean anything.
    pub status: ProbeStatus,
    /// Temperature.
    pub temperature_dc: DeciCelsius,
    /// Relative humidity.
    pub humidity_dp: DeciPercent,
}

impl Default for ProbeReading {
    fn default() -> Self {
        Self {
            status: ProbeStatus::Uninitialised,
            temperature_dc: 0,
            humidity_dp: 0,
        }
    }
}

/// Optional external temperature + humidity probe (SHT30 / DHT22).
pub trait Hygrometer {
    /// Takes a reading.
    fn read(&self) -> ProbeReading;
}

// ---------------------------------------------------------------------------
// Controller-facing transport
// ---------------------------------------------------------------------------

/// Maps onto the MySensors `V_*` variable types.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ValueType {
    /// `V_STATUS`
    Status,
    /// `V_PERCENTAGE`
    Percentage,
    /// `V_WATT`
    Watt,
    /// `V_TEMP`
    Temperature,
    /// `V_HUM`
    Humidity,
    /// `V_TEXT`
    Text,
    /// `V_UP`
    Up,
    /// `V_DOWN`
    Down,
    /// `V_STOP`
    Stop,
    /// `V_RGB`
    Rgb,
    /// `V_RGBW`
    Rgbw,
}

/// Maps onto the MySensors `S_*` sensor types.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SensorClass {
    /// `S_BINARY`
    Binary,
    /// `S_COVER`
    Cover,
    /// `S_DIMMER`
    Dimmer,
    /// `S_RGB_LIGHT`
    RgbLight,
    /// `S_RGBW_LIGHT`
    RgbwLight,
    /// `S_POWER`
    Power,
    /// `S_TEMP`
    Temperature,
    /// `S_HUM`
    Humidity,
    /// `S_INFO`
    Info,
}

/// A decoded inbound message.
///
/// All three payload views are filled in by the transport, so the domain never
/// has to know how MySensors encodes a payload. `numeric` is specifically the
/// integer parse of the raw payload, which is what the original sketch used for
/// `V_PERCENTAGE`.
#[derive(Copy, Clone, Debug)]
pub struct InboundMessage<'a> {
    /// Child the message is addressed to.
    pub sensor: SensorId,
    /// Variable type.
    pub ty: ValueType,
    /// Payload as a truth value.
    pub boolean: bool,
    /// Payload as an integer.
    pub numeric: i32,
    /// Payload as text. Empty when the payload was not a string.
    pub text: &'a str,
}

impl Default for InboundMessage<'_> {
    fn default() -> Self {
        Self {
            sensor: NO_SENSOR,
            ty: ValueType::Status,
            boolean: false,
            numeric: 0,
            text: "",
        }
    }
}

/// The controller link.
///
/// Implemented by [`crate::proto::MySensorsBus`] on hardware and by a recording
/// fake in the tests. Nothing in the domain layer knows that MySensors exists.
pub trait Bus {
    /// Announces the sketch name and version.
    fn send_sketch_info(&self, name: Text, version: Text);

    /// Announces one child. Returns whether the message was sent.
    fn present(&self, sensor: SensorId, class: SensorClass, name: Text) -> bool;

    /// Sends a truth value.
    fn send_bool(&self, sensor: SensorId, ty: ValueType, value: bool);

    /// Sends an unsigned integer.
    fn send_uint(&self, sensor: SensorId, ty: ValueType, value: u32);

    /// Sends a fixed-point value with `decimals` places of precision.
    ///
    /// `value` is the quantity scaled by `10^decimals`: 21.5 °C is
    /// `send_fixed(id, Temperature, 215, 1)`, and 460 W is
    /// `send_fixed(id, Watt, 460, 0)`. The transport turns that into the `f32`
    /// the protocol wants -- see [`crate::proto::fixed`].
    fn send_fixed(&self, sensor: SensorId, ty: ValueType, value: i32, decimals: u8);

    /// Sends a string held in RAM, used to echo a received payload back.
    fn send_text(&self, sensor: SensorId, ty: ValueType, value: &str);

    /// Sends a compile-time literal, which on AVR stays in flash.
    ///
    /// Distinct from [`Bus::send_text`] because [`Text`] cannot be dereferenced
    /// like a `&str` -- see [`crate::text`].
    fn send_literal(&self, sensor: SensorId, ty: ValueType, value: Text);

    /// Routes a reading to another node, for the heating-controller mirror.
    fn send_fixed_to(&self, node: u8, sensor: SensorId, ty: ValueType, value: i32, decimals: u8);

    /// Asks the controller for a child's current value.
    fn request(&self, sensor: SensorId, ty: ValueType);

    /// Waits, while still servicing the transport.
    fn wait(&self, ms: u32);

    /// Waits for an inbound `C_SET` of the given type, or until the timeout.
    fn wait_for_set(&self, ms: u32, ty: ValueType);
}

// ---------------------------------------------------------------------------
// Platform bundle
// ---------------------------------------------------------------------------

/// Every hardware type, in one place.
///
/// A firmware writes exactly one of these and the whole domain layer
/// monomorphises against it. The tests write a second one out of fakes.
///
/// A board that lacks a peripheral still has to name a type for it -- use the
/// stubs in [`crate::hal::stub`] -- but it passes `None` to
/// [`crate::domain::Peripherals`], so nothing calls it.
///
/// [`Bus`] is deliberately *not* here, and that is not an oversight. The
/// MySensors bus is generic over the platform (it reaches the clock and the
/// EEPROM through it), so making it an associated type would give
/// `type Bus = MySensorsBus<Self, ..>` -- a cycle whose only escape is to put the
/// transport in a `static`, and the transport has interior mutability, so that
/// means `static mut`. Threading the bus as its own parameter keeps every object
/// in the firmware on the stack and every lifetime checked.
pub trait Platform {
    /// Digital I/O.
    type Gpio: Gpio;
    /// PWM outputs.
    type Pwm: Pwm;
    /// Time base.
    type Clock: Clock;
    /// EEPROM.
    type Store: Store;
    /// Watchdog.
    type Watchdog: Watchdog;
    /// Supply voltage measurement.
    type Vref: VoltageReference;
    /// Current sensor.
    type CurrentSensor: CurrentSensor;
    /// On-board thermometer.
    type TemperatureSensor: TemperatureSensor;
    /// External probe.
    type Hygrometer: Hygrometer;
}

/// Do-nothing implementations, for peripherals a board does not have.
///
/// A `Platform` must name a type for every associated type even when the board
/// has no such hardware. These are zero-sized and never called -- the
/// `Option<&_>` in [`crate::domain::Peripherals`] is `None` -- but naming them
/// is what keeps [`Platform`] free of `Option`al associated types.
pub mod stub {
    use super::{CurrentSensor, Hygrometer, ProbeReading, TemperatureSensor};

    /// Stands in for an absent current sensor.
    pub struct NoCurrentSensor;

    impl CurrentSensor for NoCurrentSensor {
        fn begin(&self, _vcc_mv: super::Millivolts) {}
        fn measure_ac(&self, _vcc_mv: super::Millivolts) -> super::Milliamps {
            0
        }
        fn measure_dc(&self, _vcc_mv: super::Millivolts) -> super::Milliamps {
            0
        }
    }

    /// Stands in for an absent thermometer.
    pub struct NoTemperatureSensor;

    impl TemperatureSensor for NoTemperatureSensor {
        fn begin(&self) {}
        fn measure_decicelsius(&self, _vcc_mv: super::Millivolts) -> super::DeciCelsius {
            0
        }
    }

    /// Stands in for an absent external probe.
    pub struct NoHygrometer;

    impl Hygrometer for NoHygrometer {
        fn read(&self) -> ProbeReading {
            ProbeReading::default()
        }
    }
}
