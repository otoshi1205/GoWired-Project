//! GoWired module firmware: wiring only.
//!
//! All behaviour is in `gowired-core`, which knows nothing about ATmega
//! registers and is unit tested on the host. All register access is in
//! `gowired-avr`. This file names one device implementation, hands it the
//! hardware, and runs the loop.
//!
//! # Startup order
//!
//! It matters, and not obviously:
//!
//! 1. [`gowired_avr::clear_watchdog_reset`] -- the bootloader can leave `WDRF`
//!    set with a short timeout armed, which turns one watchdog reset into an
//!    endless series.
//! 2. [`gowired_avr::init`] -- time base, PWM timers, ADC, USART, interrupts.
//!    Nothing else works before this.
//! 3. `node.begin()` -- transport up, node id settled. This can block for up to
//!    a minute waiting for the gateway, which is why it happens *before* the
//!    watchdog is armed.
//! 4. `module.begin()` -- arms the watchdog, then configures every pin.
//! 5. `node.present_node()` then `module.present()` -- announce the node, then
//!    its children.
//!
//! # The main loop
//!
//! ```text
//! drain the inbound queue -> module.loop_once()
//! ```
//!
//! The C++ library called the sketch's `receive()` from inside `wait()`, from
//! inside `loop()`. Rust will not allow that reentrancy, so inbound messages are
//! queued by the transport and drained here instead. See
//! [`gowired_core::proto::node`] for what that changes (very little) and why.

#![no_std]
#![no_main]

mod config;

use core::panic::PanicInfo;

use config as cfg;
use gowired_avr as avr;
// Each `make_device` below is behind a `cfg`, so only one variant's types are
// named in any build. Importing all of them keeps the three functions readable
// and costs nothing; the allow is for the two that this build does not use.
#[allow(unused_imports)]
use gowired_core::domain::{
    Device, DimmerDevice, DimmerSpec, InputBank, Module, Peripherals, RelayBankDevice,
    RelayBankSpec, RollerShutterDevice, RollerShutterSpec, Settings, ShutterPins,
};
use gowired_core::hal::{Platform, Watchdog};
use gowired_core::proto::{MySensorsBus, Node, Rs485Transport};

/// The hardware this firmware runs on.
///
/// One impl, so every generic in `gowired-core` monomorphises exactly once and
/// nothing is dispatched dynamically.
struct Board;

impl Platform for Board {
    type Gpio = avr::Gpio;
    type Pwm = avr::Pwm;
    type Clock = avr::Clock;
    type Store = avr::Store;
    type Watchdog = avr::Watchdog;
    type Vref = avr::VoltageReference;
    type CurrentSensor = avr::CurrentSensor;
    type TemperatureSensor = avr::TemperatureSensor;
    type Hygrometer = avr::probe::ExternalProbe;
}

/// Nothing can be reported and nothing can be recovered, so stop and let the
/// watchdog restart the node.
///
/// This is the only sane response on a part with no console: a panic here means a
/// bug, and eight seconds later the node is back up presenting itself again. The
/// firmware is built with `panic_immediate_abort`, so in practice this is never
/// reached -- the panicking paths compile to a trap.
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Device selection -- the ONE compile-time branch in the firmware.
//
// Six board variants, three implementations. Exactly one `make_device` exists in
// any given build, so the other two devices are not merely unreachable, they are
// not compiled.
// ---------------------------------------------------------------------------

/// Builds the relay bank for `DOUBLE_RELAY` and `FOUR_RELAY`.
#[cfg(any(feature = "device-double-relay", feature = "device-four-relay"))]
fn make_device<'a>(
    gpio: &'a avr::Gpio,
    clock: &'a avr::Clock,
    _pwm: &'a avr::Pwm,
    _store: &'a avr::Store,
) -> RelayBankDevice<'a, Board> {
    use gowired_core::domain::config::{button_count, output_count};

    let spec = RelayBankSpec {
        relay_count: output_count(cfg::DEVICE),
        relay_pins: [
            cfg::relay_pin(0),
            cfg::relay_pin(1),
            cfg::relay_pin(2),
            cfg::relay_pin(3),
        ],
        button_count: button_count(cfg::DEVICE),
        button_pins: [cfg::BUTTON_PIN_1, cfg::BUTTON_PIN_2],
        off_level: cfg::RELAY_OFF_LEVEL,
        per_relay_power: matches!(
            cfg::DEVICE,
            gowired_core::domain::config::DeviceKind::FourRelay
        ),
    };
    RelayBankDevice::new(gpio, clock, spec, cfg::BUTTONS, cfg::FEATURES.special_button)
}

/// Builds the cover for `ROLLER_SHUTTER`.
#[cfg(feature = "device-roller-shutter")]
fn make_device<'a>(
    gpio: &'a avr::Gpio,
    clock: &'a avr::Clock,
    _pwm: &'a avr::Pwm,
    store: &'a avr::Store,
) -> RollerShutterDevice<'a, Board> {
    let spec = RollerShutterSpec {
        pins: ShutterPins {
            up: cfg::relay_pin(0),
            down: cfg::relay_pin(1),
            off_level: cfg::RELAY_OFF_LEVEL,
        },
        button_pins: [cfg::BUTTON_PIN_1, cfg::BUTTON_PIN_2],
        current_floor_ma: cfg::SHUTTER.calibration_current_floor_ma,
        calibration_samples: cfg::SHUTTER.calibration_samples,
        default_up_time_s: cfg::SHUTTER.up_time_s,
        default_down_time_s: cfg::SHUTTER.down_time_s,
        current_sensing: cfg::POWER_SENSOR,
    };
    RollerShutterDevice::new(
        gpio,
        clock,
        store,
        spec,
        cfg::STORE,
        cfg::BUTTONS,
        cfg::FEATURES.special_button,
    )
}

/// Builds the strip for `DIMMER`, `RGB` and `RGBW`.
#[cfg(any(
    feature = "device-dimmer",
    feature = "device-rgb",
    feature = "device-rgbw"
))]
fn make_device<'a>(
    gpio: &'a avr::Gpio,
    clock: &'a avr::Clock,
    pwm: &'a avr::Pwm,
    _store: &'a avr::Store,
) -> DimmerDevice<'a, Board> {
    let spec = DimmerSpec {
        model: cfg::COLOR_MODEL,
        led_pins: [
            cfg::led_pin(0),
            cfg::led_pin(1),
            cfg::led_pin(2),
            cfg::led_pin(3),
        ],
        button_pins: [cfg::BUTTON_PIN_1, cfg::BUTTON_PIN_2],
    };
    DimmerDevice::new(
        pwm,
        gpio,
        clock,
        spec,
        cfg::DIMMER,
        cfg::BUTTONS,
        cfg::FEATURES.special_button,
    )
}

/// Builds the external probe, if this build has one.
#[cfg(feature = "probe-sht30")]
fn make_probe() -> avr::probe::ExternalProbe {
    avr::probe::sht30::Sht30
}

/// Builds the external probe, if this build has one.
#[cfg(all(feature = "probe-dht22", not(feature = "probe-sht30")))]
fn make_probe() -> avr::probe::ExternalProbe {
    avr::probe::dht22::Dht22::new(cfg::ONE_WIRE)
}

/// No probe in this build; the stub is never called.
#[cfg(not(any(feature = "probe-sht30", feature = "probe-dht22")))]
fn make_probe() -> avr::probe::ExternalProbe {
    gowired_core::hal::stub::NoHygrometer
}

/// Entry point.
///
/// Never returns. Every object lives in this frame, which is what lets the whole
/// firmware avoid `static mut`: the borrows are checked, and the frame outlives
/// everything because the function does not end.
#[no_mangle]
pub extern "C" fn main() -> ! {
    // Referencing the checks is what forces them to be evaluated.
    let () = cfg::DEVICE_FEATURE_CHECK;
    let () = cfg::ID_CHECK;
    let () = cfg::THERMOMETER_CHECK;
    let () = cfg::PROBE_CHECK;

    // SAFETY: first thing in main, exactly once, before anything else runs.
    unsafe {
        if cfg::WATCHDOG {
            avr::clear_watchdog_reset();
        }
        avr::init(cfg::RS485_BAUD);
        #[cfg(feature = "probe-sht30")]
        avr::probe::sht30::Sht30::begin();
    }

    let gpio = avr::Gpio;
    let pwm = avr::Pwm;
    let clock = avr::Clock;
    let store = avr::Store;
    let watchdog = avr::Watchdog;
    let vref = avr::VoltageReference;
    let serial = avr::Serial;

    // -- transport ----------------------------------------------------------

    let transport = Rs485Transport::<Board, _>::new(
        &serial,
        &gpio,
        &clock,
        cfg::RS485_DE_PIN,
        cfg::RS485_SOH_COUNT,
    );
    // Built in one expression rather than as two locals: `MySensorsBus` takes
    // the `Node` by value, and a separate `node` binding leaves the compiler
    // holding a second ~125-byte copy of it in this frame for the whole run.
    let bus = MySensorsBus::new(Node::<Board, _>::new(
        &transport,
        &clock,
        &store,
        cfg::NODE_ID,
        cfg::TRANSPORT_WAIT_READY_MS,
    ));

    // Before the watchdog is armed: this can wait a full minute for a gateway.
    bus.node().begin();

    // -- peripherals --------------------------------------------------------

    let current_sensors: [avr::CurrentSensor; cfg::POWER_CHANNELS] =
        core::array::from_fn(|ch| avr::CurrentSensor::new(cfg::current_sense_pin(ch), cfg::POWER));
    let thermometer = avr::TemperatureSensor::new(cfg::INTERNAL_TEMP_PIN, cfg::THERMAL);
    let probe = make_probe();

    let mut peripherals = Peripherals::<Board>::default();
    if cfg::POWER_SENSOR {
        peripherals.power.count = cfg::POWER_CHANNELS as u8;
        for ch in 0..cfg::POWER_CHANNELS {
            peripherals.power.sensor[ch] = Some(&current_sensors[ch]);
            peripherals.power.id[ch] = cfg::power_child_id(ch);
        }
    }
    if cfg::INTERNAL_TEMPERATURE {
        peripherals.internal_temperature = Some(&thermometer);
    }
    if cfg::EXTERNAL_TEMPERATURE {
        peripherals.external_probe = Some(&probe);
    }

    // -- the node -----------------------------------------------------------

    let device = make_device(&gpio, &clock, &pwm, &store);
    let inputs = InputBank::<Board>::new(&gpio, &clock, cfg::INPUTS, cfg::BUTTONS.debounce_ms);

    let mut module = Module::new(
        device,
        inputs,
        gowired_core::domain::Wiring {
            bus: &bus,
            clock: &clock,
            store: &store,
            watchdog: &watchdog,
            vref: &vref,
        },
        peripherals,
        Settings {
            features: cfg::FEATURES,
            timing: cfg::TIMING,
            power: cfg::POWER,
            thermal: cfg::THERMAL,
            layout: cfg::STORE,
        },
    );

    module.begin();
    if !cfg::WATCHDOG {
        // `Module::begin` arms it unconditionally, as the C++ version did. This
        // is the escape hatch for bench work, where a debugger pause is not a
        // reason to reboot.
        watchdog.disable();
    }

    bus.node().present_node();
    module.present(cfg::sketch_name(), cfg::sketch_version());

    loop {
        // Inbound first: the transport queued these while we were busy.
        while let Some(inbound) = bus.take_inbound() {
            if let Some(msg) = inbound.as_domain() {
                module.on_message(&msg);
            }
        }

        // The controller can ask a node to introduce itself again, typically
        // after the controller itself restarts.
        if bus.node().take_presentation_request() {
            module.present(cfg::sketch_name(), cfg::sketch_version());
        }

        module.loop_once();

        // A node that booted before its gateway keeps asking. Bounded, because
        // the watchdog is armed by now.
        if !bus.node().ready() {
            watchdog.pet();
            bus.node().request_node_id_for(2000);
        }
    }
}

/// Keeps the device trait in scope for `make_device`'s return types.
///
/// Without a use of `Device`, the import is dead and the compiler says so; the
/// trait is what `Module` requires of whatever `make_device` returns.
const _: fn() = || {
    fn assert_device<D: Device<Board>>() {}
    #[cfg(any(feature = "device-double-relay", feature = "device-four-relay"))]
    assert_device::<RelayBankDevice<'static, Board>>();
    #[cfg(feature = "device-roller-shutter")]
    assert_device::<RollerShutterDevice<'static, Board>>();
    #[cfg(any(
        feature = "device-dimmer",
        feature = "device-rgb",
        feature = "device-rgbw"
    ))]
    assert_device::<DimmerDevice<'static, Board>>();
};
