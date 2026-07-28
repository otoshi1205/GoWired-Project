//! Host implementations of every HAL trait, for the tests.
//!
//! The equivalent of the C++ suite's `test/fakes.h`. Enabled by the `testing`
//! feature; the firmware never turns it on, so none of this reaches the device.
//!
//! Two things here are load bearing rather than incidental:
//!
//! - **[`FakeClock`] advances on every `now_ms()` call.** The ported input and
//!   dimmer code polls in blocking loops exactly as the original did, so a clock
//!   that stood still would hang the test suite rather than fail it.
//!
//! - **[`FakeGpio`] scripts reads, and repeats the last scripted value
//!   forever.** A button poll reads the pin an unbounded number of times; a
//!   script that ran out would deadlock. See [`FakeGpio::script_short_press`].

extern crate std;

use core::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::string::String;
use std::vec::Vec;

use crate::hal::{
    Bus, Clock, CurrentSensor, DeciCelsius, Gpio, Hygrometer, Milliamps, Millivolts,
    Pin, PinMode, Platform, ProbeReading, ProbeStatus, Pwm, SensorClass, SensorId, Store,
    TemperatureSensor, ValueType, VoltageReference, Watchdog,
};
use crate::text::Text;

/// How many pins the fakes model. Arduino numbering tops out at A7 = 21.
const PINS: usize = 32;

fn text_to_string(t: Text) -> String {
    let mut buf = [0u8; 64];
    let n = t.copy_to(&mut buf);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

// ---------------------------------------------------------------------------
// Gpio
// ---------------------------------------------------------------------------

/// One recorded pin write.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct PinWrite {
    /// Which pin.
    pub pin: Pin,
    /// The level written.
    pub high: bool,
}

/// Records writes, replays scripted reads.
#[derive(Default)]
pub struct FakeGpio {
    /// Raw pin levels. The domain treats a pulled-up input as *active* when it
    /// reads LOW, so `false` here means "button pressed".
    level: RefCell<[Option<bool>; PINS]>,
    mode: RefCell<[Option<PinMode>; PINS]>,
    script: RefCell<[Option<VecDeque<bool>>; PINS]>,
    /// Every write, in order.
    pub writes: RefCell<Vec<PinWrite>>,
}

impl FakeGpio {
    /// An all-idle port: every input reads high (pulled up).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forces a pin's level, as an external circuit would.
    pub fn set_level(&self, pin: Pin, high: bool) {
        self.level.borrow_mut()[pin as usize] = Some(high);
    }

    /// The pin's current level.
    #[must_use]
    pub fn level_of(&self, pin: Pin) -> bool {
        self.level.borrow()[pin as usize].unwrap_or(true)
    }

    /// The mode a pin was configured with, if it ever was.
    #[must_use]
    pub fn mode_of(&self, pin: Pin) -> Option<PinMode> {
        self.mode.borrow()[pin as usize]
    }

    /// Queues a sequence of raw reads for one pin.
    ///
    /// Once exhausted the last value repeats forever, which is what keeps the
    /// blocking poll loops terminating.
    pub fn script(&self, pin: Pin, values: &[bool]) {
        self.script.borrow_mut()[pin as usize] = Some(values.iter().copied().collect());
    }

    /// Scripts a press short enough to be a toggle.
    ///
    /// `false` is pressed. One low read gets past debounce, then the line is
    /// released so the poll loop exits before the long-press threshold.
    pub fn script_short_press(&self, pin: Pin) {
        self.script(pin, &[false, false, false, true]);
    }

    /// Scripts a press held down forever, so the poll loop exits on the
    /// long-press timeout instead of on release.
    pub fn script_hold(&self, pin: Pin) {
        self.script(pin, &[false]);
    }

    /// Scripts an untouched, released button.
    pub fn script_idle(&self, pin: Pin) {
        self.script(pin, &[true]);
    }

    /// Forgets the recorded writes.
    pub fn clear_writes(&self) {
        self.writes.borrow_mut().clear();
    }

    /// Every write recorded for one pin, in order.
    #[must_use]
    pub fn writes_to(&self, pin: Pin) -> Vec<bool> {
        self.writes
            .borrow()
            .iter()
            .filter(|w| w.pin == pin)
            .map(|w| w.high)
            .collect()
    }
}

impl Gpio for FakeGpio {
    fn configure(&self, pin: Pin, mode: PinMode) {
        self.mode.borrow_mut()[pin as usize] = Some(mode);
    }

    fn read(&self, pin: Pin) -> bool {
        let mut script = self.script.borrow_mut();
        if let Some(queue) = script[pin as usize].as_mut() {
            if !queue.is_empty() {
                let v = if queue.len() > 1 {
                    queue.pop_front().unwrap()
                } else {
                    queue[0]
                };
                self.level.borrow_mut()[pin as usize] = Some(v);
                return v;
            }
        }
        drop(script);
        self.level_of(pin) // idle high (pulled up)
    }

    fn write(&self, pin: Pin, high: bool) {
        self.level.borrow_mut()[pin as usize] = Some(high);
        self.writes.borrow_mut().push(PinWrite { pin, high });
    }
}

// ---------------------------------------------------------------------------
// Pwm
// ---------------------------------------------------------------------------

/// Records duty cycles.
#[derive(Default)]
pub struct FakePwm {
    duty: RefCell<[u8; PINS]>,
    /// Every write, in order.
    pub writes: RefCell<Vec<(Pin, u8)>>,
}

impl FakePwm {
    /// A fresh set of outputs, all at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The last duty written to a pin.
    #[must_use]
    pub fn duty_of(&self, pin: Pin) -> u8 {
        self.duty.borrow()[pin as usize]
    }
}

impl Pwm for FakePwm {
    fn write_duty(&self, pin: Pin, duty: u8) {
        self.duty.borrow_mut()[pin as usize] = duty;
        self.writes.borrow_mut().push((pin, duty));
    }
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

/// A clock that ticks whenever it is read.
pub struct FakeClock {
    now: Cell<u32>,
    /// Milliseconds added by each `now_ms()` call.
    ///
    /// Non-zero by default, because the ported code polls in blocking loops that
    /// only terminate once time passes. Set to 0 to freeze time for a test that
    /// needs exact arithmetic.
    pub auto_advance_ms: Cell<u32>,
    /// Every `delay_ms` argument, in order.
    pub delays: RefCell<Vec<u32>>,
}

impl Default for FakeClock {
    fn default() -> Self {
        Self {
            now: Cell::new(0),
            auto_advance_ms: Cell::new(30),
            delays: RefCell::new(Vec::new()),
        }
    }
}

impl FakeClock {
    /// A clock at zero that advances 30 ms per read.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A clock that does not move unless told to.
    #[must_use]
    pub fn frozen() -> Self {
        let c = Self::default();
        c.auto_advance_ms.set(0);
        c
    }

    /// Moves the clock forward.
    pub fn advance(&self, ms: u32) {
        self.now.set(self.now.get().wrapping_add(ms));
    }

    /// Sets the clock, for rollover tests.
    pub fn set(&self, ms: u32) {
        self.now.set(ms);
    }

    /// The current time without advancing it.
    #[must_use]
    pub fn peek(&self) -> u32 {
        self.now.get()
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u32 {
        let t = self.now.get();
        self.now.set(t.wrapping_add(self.auto_advance_ms.get()));
        t
    }

    fn delay_ms(&self, ms: u32) {
        self.delays.borrow_mut().push(ms);
        self.advance(ms);
    }

    fn delay_us(&self, _us: u16) {
        // Below the fake clock's resolution, and nothing in the domain layer
        // depends on microsecond timing -- only the RS485 driver turnaround does.
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// A 1 KB EEPROM that starts blank, and counts the writes it actually performs.
pub struct FakeStore {
    cells: RefCell<Vec<u8>>,
    /// Writes that changed a byte. An unchanged write must not appear here --
    /// that is the endurance guarantee [`Store::write`] documents.
    pub effective_writes: Cell<u32>,
}

impl Default for FakeStore {
    fn default() -> Self {
        Self {
            cells: RefCell::new(std::vec![0xFF; 1024]),
            effective_writes: Cell::new(0),
        }
    }
}

impl FakeStore {
    /// A blank EEPROM.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Presets a cell without counting it as a write.
    pub fn preset(&self, address: u16, value: u8) {
        self.cells.borrow_mut()[address as usize] = value;
    }

    /// Reads a cell without going through the trait.
    #[must_use]
    pub fn peek(&self, address: u16) -> u8 {
        self.cells.borrow()[address as usize]
    }
}

impl Store for FakeStore {
    fn read(&self, address: u16) -> u8 {
        self.cells.borrow()[address as usize]
    }

    fn write(&self, address: u16, value: u8) {
        let mut cells = self.cells.borrow_mut();
        if cells[address as usize] != value {
            cells[address as usize] = value;
            self.effective_writes.set(self.effective_writes.get() + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Watchdog, voltage reference
// ---------------------------------------------------------------------------

/// Counts watchdog operations.
#[derive(Default)]
pub struct FakeWatchdog {
    /// Times `enable()` was called.
    pub enables: Cell<u32>,
    /// Times `disable()` was called.
    pub disables: Cell<u32>,
    /// Times `pet()` was called.
    pub pets: Cell<u32>,
}

impl FakeWatchdog {
    /// A disarmed watchdog.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Watchdog for FakeWatchdog {
    fn enable(&self) {
        self.enables.set(self.enables.get() + 1);
    }
    fn disable(&self) {
        self.disables.set(self.disables.get() + 1);
    }
    fn pet(&self) {
        self.pets.set(self.pets.get() + 1);
    }
}

/// A settable supply voltage.
pub struct FakeVref {
    /// Millivolts reported by `vcc_mv()`.
    pub mv: Cell<Millivolts>,
}

impl Default for FakeVref {
    fn default() -> Self {
        Self {
            mv: Cell::new(5000),
        }
    }
}

impl FakeVref {
    /// A 5 V supply.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl VoltageReference for FakeVref {
    fn vcc_mv(&self) -> Millivolts {
        self.mv.get()
    }
}

// ---------------------------------------------------------------------------
// Sensors
// ---------------------------------------------------------------------------

/// A current sensor whose reading the test sets.
#[derive(Default)]
pub struct FakeCurrentSensor {
    /// Current returned by both measurement methods.
    pub current: Cell<Milliamps>,
    /// Times `begin()` was called.
    pub begins: Cell<u32>,
    /// Times `measure_ac()` was called.
    pub ac_reads: Cell<u32>,
    /// Times `measure_dc()` was called.
    pub dc_reads: Cell<u32>,
}

impl FakeCurrentSensor {
    /// A sensor reading zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A sensor reading `current` milliamps.
    #[must_use]
    pub fn at(current: Milliamps) -> Self {
        let s = Self::default();
        s.current.set(current);
        s
    }
}

impl CurrentSensor for FakeCurrentSensor {
    fn begin(&self, _vcc_mv: Millivolts) {
        self.begins.set(self.begins.get() + 1);
    }
    fn measure_ac(&self, _vcc_mv: Millivolts) -> Milliamps {
        self.ac_reads.set(self.ac_reads.get() + 1);
        self.current.get()
    }
    fn measure_dc(&self, _vcc_mv: Millivolts) -> Milliamps {
        self.dc_reads.set(self.dc_reads.get() + 1);
        self.current.get()
    }
}

/// A thermometer whose reading the test sets.
#[derive(Default)]
pub struct FakeTemperatureSensor {
    /// Temperature returned by `measure_decicelsius()`.
    pub decicelsius: Cell<DeciCelsius>,
    /// Times `begin()` was called.
    pub begins: Cell<u32>,
}

impl FakeTemperatureSensor {
    /// A thermometer reading 0 °C.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A thermometer reading `decicelsius` tenths of a degree.
    #[must_use]
    pub fn at(decicelsius: DeciCelsius) -> Self {
        let s = Self::default();
        s.decicelsius.set(decicelsius);
        s
    }
}

impl TemperatureSensor for FakeTemperatureSensor {
    fn begin(&self) {
        self.begins.set(self.begins.get() + 1);
    }
    fn measure_decicelsius(&self, _vcc_mv: Millivolts) -> DeciCelsius {
        self.decicelsius.get()
    }
}

/// An external probe whose reading the test sets.
pub struct FakeHygrometer {
    /// What `read()` returns.
    pub reading: Cell<ProbeReading>,
    /// Times `read()` was called.
    pub reads: Cell<u32>,
}

impl Default for FakeHygrometer {
    fn default() -> Self {
        Self {
            reading: Cell::new(ProbeReading {
                status: ProbeStatus::Ok,
                temperature_dc: 215,
                humidity_dp: 450,
            }),
            reads: Cell::new(0),
        }
    }
}

impl FakeHygrometer {
    /// A probe reading 21.5 °C / 45 %.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A probe that fails with `status`.
    #[must_use]
    pub fn failing(status: ProbeStatus) -> Self {
        let p = Self::default();
        p.reading.set(ProbeReading {
            status,
            temperature_dc: 0,
            humidity_dp: 0,
        });
        p
    }
}

impl Hygrometer for FakeHygrometer {
    fn read(&self) -> ProbeReading {
        self.reads.set(self.reads.get() + 1);
        self.reading.get()
    }
}

// ---------------------------------------------------------------------------
// Bus
// ---------------------------------------------------------------------------

/// Everything the domain layer can say to the controller.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Event {
    /// `send_sketch_info`
    SketchInfo {
        /// Sketch name.
        name: String,
        /// Sketch version.
        version: String,
    },
    /// `present`
    Presented {
        /// Child id.
        sensor: SensorId,
        /// Sensor class.
        class: SensorClass,
        /// Child description.
        name: String,
    },
    /// `send_bool`
    SentBool {
        /// Child id.
        sensor: SensorId,
        /// Variable type.
        ty: ValueType,
        /// Payload.
        value: bool,
    },
    /// `send_uint`
    SentUint {
        /// Child id.
        sensor: SensorId,
        /// Variable type.
        ty: ValueType,
        /// Payload.
        value: u32,
    },
    /// `send_fixed`
    SentFixed {
        /// Child id.
        sensor: SensorId,
        /// Variable type.
        ty: ValueType,
        /// Payload, scaled by `10^decimals`.
        value: i32,
        /// Requested precision.
        decimals: u8,
    },
    /// `send_text` or `send_literal`
    SentText {
        /// Child id.
        sensor: SensorId,
        /// Variable type.
        ty: ValueType,
        /// Payload.
        value: String,
    },
    /// `send_fixed_to`
    SentFixedTo {
        /// Destination node.
        node: u8,
        /// Child id.
        sensor: SensorId,
        /// Variable type.
        ty: ValueType,
        /// Payload, scaled by `10^decimals`.
        value: i32,
    },
    /// `request`
    Requested {
        /// Child id.
        sensor: SensorId,
        /// Variable type.
        ty: ValueType,
    },
    /// `wait`
    Waited {
        /// Milliseconds asked for.
        ms: u32,
    },
    /// `wait_for_set`
    WaitedForSet {
        /// Milliseconds asked for.
        ms: u32,
        /// Variable type waited on.
        ty: ValueType,
    },
}

/// Records every outbound message.
#[derive(Default)]
pub struct FakeBus {
    /// Everything sent, in order.
    pub events: RefCell<Vec<Event>>,
    /// What `present()` should return.
    pub present_result: Cell<bool>,
}

impl FakeBus {
    /// A bus that records and accepts everything.
    #[must_use]
    pub fn new() -> Self {
        let b = Self::default();
        b.present_result.set(true);
        b
    }

    /// Forgets everything recorded so far.
    pub fn clear(&self) {
        self.events.borrow_mut().clear();
    }

    /// Every recorded event, cloned.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.events.borrow().clone()
    }

    /// Whether an event matching the predicate was recorded.
    pub fn any<F: Fn(&Event) -> bool>(&self, f: F) -> bool {
        self.events.borrow().iter().any(f)
    }

    /// How many recorded events match the predicate.
    pub fn count<F: Fn(&Event) -> bool>(&self, f: F) -> usize {
        self.events.borrow().iter().filter(|e| f(e)).count()
    }

    /// The child ids presented, in order.
    #[must_use]
    pub fn presented_ids(&self) -> Vec<SensorId> {
        self.events
            .borrow()
            .iter()
            .filter_map(|e| match e {
                Event::Presented { sensor, .. } => Some(*sensor),
                _ => None,
            })
            .collect()
    }

    /// The last boolean sent on a child, if any.
    #[must_use]
    pub fn last_bool(&self, sensor: SensorId, ty: ValueType) -> Option<bool> {
        self.events.borrow().iter().rev().find_map(|e| match e {
            Event::SentBool {
                sensor: s,
                ty: t,
                value,
            } if *s == sensor && *t == ty => Some(*value),
            _ => None,
        })
    }

    /// The last unsigned integer sent on a child, if any.
    #[must_use]
    pub fn last_uint(&self, sensor: SensorId, ty: ValueType) -> Option<u32> {
        self.events.borrow().iter().rev().find_map(|e| match e {
            Event::SentUint {
                sensor: s,
                ty: t,
                value,
            } if *s == sensor && *t == ty => Some(*value),
            _ => None,
        })
    }

    /// The last fixed-point value sent on a child, if any.
    #[must_use]
    pub fn last_fixed(&self, sensor: SensorId, ty: ValueType) -> Option<i32> {
        self.events.borrow().iter().rev().find_map(|e| match e {
            Event::SentFixed {
                sensor: s,
                ty: t,
                value,
                ..
            } if *s == sensor && *t == ty => Some(*value),
            _ => None,
        })
    }

    /// The last text sent on a child, if any.
    #[must_use]
    pub fn last_text(&self, sensor: SensorId, ty: ValueType) -> Option<String> {
        self.events.borrow().iter().rev().find_map(|e| match e {
            Event::SentText {
                sensor: s,
                ty: t,
                value,
            } if *s == sensor && *t == ty => Some(value.clone()),
            _ => None,
        })
    }

    fn push(&self, e: Event) {
        self.events.borrow_mut().push(e);
    }
}

impl Bus for FakeBus {
    fn send_sketch_info(&self, name: Text, version: Text) {
        self.push(Event::SketchInfo {
            name: text_to_string(name),
            version: text_to_string(version),
        });
    }

    fn present(&self, sensor: SensorId, class: SensorClass, name: Text) -> bool {
        self.push(Event::Presented {
            sensor,
            class,
            name: text_to_string(name),
        });
        self.present_result.get()
    }

    fn send_bool(&self, sensor: SensorId, ty: ValueType, value: bool) {
        self.push(Event::SentBool { sensor, ty, value });
    }

    fn send_uint(&self, sensor: SensorId, ty: ValueType, value: u32) {
        self.push(Event::SentUint { sensor, ty, value });
    }

    fn send_fixed(&self, sensor: SensorId, ty: ValueType, value: i32, decimals: u8) {
        self.push(Event::SentFixed {
            sensor,
            ty,
            value,
            decimals,
        });
    }

    fn send_text(&self, sensor: SensorId, ty: ValueType, value: &str) {
        self.push(Event::SentText {
            sensor,
            ty,
            value: String::from(value),
        });
    }

    fn send_literal(&self, sensor: SensorId, ty: ValueType, value: Text) {
        self.push(Event::SentText {
            sensor,
            ty,
            value: text_to_string(value),
        });
    }

    fn send_fixed_to(&self, node: u8, sensor: SensorId, ty: ValueType, value: i32, _decimals: u8) {
        self.push(Event::SentFixedTo {
            node,
            sensor,
            ty,
            value,
        });
    }

    fn request(&self, sensor: SensorId, ty: ValueType) {
        self.push(Event::Requested { sensor, ty });
    }

    fn wait(&self, ms: u32) {
        self.push(Event::Waited { ms });
    }

    fn wait_for_set(&self, ms: u32, ty: ValueType) {
        self.push(Event::WaitedForSet { ms, ty });
    }
}

// ---------------------------------------------------------------------------
// Serial port
// ---------------------------------------------------------------------------

/// A byte pipe with a scriptable receive side.
///
/// `flush()` is counted rather than simulated, because what the RS485 transport
/// has to get right is the *order* -- flush before dropping DE -- and that is
/// checkable by comparing counters against the recorded DE writes.
#[derive(Default)]
pub struct FakeSerial {
    rx: RefCell<VecDeque<u8>>,
    /// Everything written, in order.
    pub tx: RefCell<Vec<u8>>,
    /// Times `flush()` was called.
    pub flushes: Cell<u32>,
    /// `tx.len()` as of the last `flush()`.
    pub flushed_at: Cell<usize>,
    /// When set, the port reports traffic forever: another burst always follows.
    ///
    /// A fake with a finite queue always drains, at which point the bus is quiet
    /// by definition -- so this is the only way to reach the transmit path's
    /// give-up branch.
    ///
    /// The traffic arrives in bursts of [`Self::JAM_BURST`] rather than as an
    /// unbroken stream, because that is what a real line does: at 57600 baud a
    /// byte takes 174 us, so a reader always catches up with the buffer before
    /// the next one lands. A fake that returned bytes without ever running dry
    /// would wedge the receive loop in a way no wire can.
    pub jammed: Cell<bool>,
    jam_remaining: Cell<u8>,
}

impl FakeSerial {
    /// How many bytes a jammed port yields before the reader catches up.
    pub const JAM_BURST: u8 = 8;

    /// An idle port.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the port permanently busy. See [`Self::jammed`].
    pub fn jam(&self) {
        self.jammed.set(true);
        self.jam_remaining.set(Self::JAM_BURST);
    }

    /// Queues bytes as if they had arrived from the bus.
    pub fn feed(&self, bytes: &[u8]) {
        self.rx.borrow_mut().extend(bytes.iter().copied());
    }

    /// Forgets what was transmitted.
    pub fn clear_tx(&self) {
        self.tx.borrow_mut().clear();
    }

    /// Everything transmitted so far.
    #[must_use]
    pub fn sent(&self) -> Vec<u8> {
        self.tx.borrow().clone()
    }
}

impl crate::proto::rs485::SerialPort for FakeSerial {
    fn read_byte(&self) -> Option<u8> {
        if self.jammed.get() {
            let left = self.jam_remaining.get();
            if left == 0 {
                self.jam_remaining.set(Self::JAM_BURST);
                return None; // reader caught up; the next burst is still coming
            }
            self.jam_remaining.set(left - 1);
            return Some(0xAA);
        }
        self.rx.borrow_mut().pop_front()
    }

    fn available(&self) -> bool {
        self.jammed.get() || !self.rx.borrow().is_empty()
    }

    fn write_byte(&self, byte: u8) {
        self.tx.borrow_mut().push(byte);
    }

    fn flush(&self) {
        self.flushes.set(self.flushes.get() + 1);
        self.flushed_at.set(self.tx.borrow().len());
    }
}

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

/// The fake platform the tests instantiate the domain layer against.
pub struct FakePlatform;

impl Platform for FakePlatform {
    type Gpio = FakeGpio;
    type Pwm = FakePwm;
    type Clock = FakeClock;
    type Store = FakeStore;
    type Watchdog = FakeWatchdog;
    type Vref = FakeVref;
    type CurrentSensor = FakeCurrentSensor;
    type TemperatureSensor = FakeTemperatureSensor;
    type Hygrometer = FakeHygrometer;
}

/// Every fake, constructed and ready to borrow.
///
/// Saves each test from declaring nine locals just to satisfy the lifetimes.
#[derive(Default)]
pub struct Rig {
    /// Digital I/O.
    pub gpio: FakeGpio,
    /// PWM.
    pub pwm: FakePwm,
    /// Clock.
    pub clock: FakeClock,
    /// EEPROM.
    pub store: FakeStore,
    /// Watchdog.
    pub watchdog: FakeWatchdog,
    /// Supply voltage.
    pub vref: FakeVref,
    /// Controller link.
    pub bus: FakeBus,
    /// Four current sensors; boards with one shared sensor use index 0.
    pub power: [FakeCurrentSensor; 4],
    /// On-board thermometer.
    pub temperature: FakeTemperatureSensor,
    /// External probe.
    pub probe: FakeHygrometer,
}

impl Rig {
    /// A rig with default fakes.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bus: FakeBus::new(),
            ..Self::default()
        }
    }

    /// Peripherals with no current sensing and no thermometer.
    #[must_use]
    pub fn no_peripherals(&self) -> crate::domain::Peripherals<'_, FakePlatform> {
        crate::domain::Peripherals::default()
    }

    /// Peripherals with one shared current sensor on the given child id.
    #[must_use]
    pub fn shared_power(&self, id: SensorId) -> crate::domain::Peripherals<'_, FakePlatform> {
        let mut p = crate::domain::Peripherals::default();
        p.power.count = 1;
        p.power.sensor[0] = Some(&self.power[0]);
        p.power.id[0] = id;
        p
    }

    /// Peripherals with one current sensor per relay.
    #[must_use]
    pub fn per_relay_power(&self, ids: [SensorId; 4]) -> crate::domain::Peripherals<'_, FakePlatform> {
        let mut p = crate::domain::Peripherals::default();
        p.power.count = 4;
        for (ch, &id) in ids.iter().enumerate() {
            p.power.sensor[ch] = Some(&self.power[ch]);
            p.power.id[ch] = id;
        }
        p
    }

    /// The wiring struct [`crate::domain::Module`] expects.
    #[must_use]
    pub fn wiring(&self) -> crate::domain::Wiring<'_, FakePlatform, FakeBus> {
        crate::domain::Wiring {
            bus: &self.bus,
            clock: &self.clock,
            store: &self.store,
            watchdog: &self.watchdog,
            vref: &self.vref,
        }
    }
}
