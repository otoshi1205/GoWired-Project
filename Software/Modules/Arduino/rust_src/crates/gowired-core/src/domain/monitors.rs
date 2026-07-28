//! Decisions taken on top of raw sensor readings.
//!
//! `PowerSensor::CalculatePower`/`ElectricalStatus` and `AnalogTemp::ThermalStatus`
//! used to live in GoWired-lib next to the ADC sampling code, which made them
//! untestable. The sampling is now in `gowired-avr`; the judgement is here.
//!
//! All of it is integer arithmetic. See [`crate::hal::Milliamps`] for why.

use crate::hal::{DeciCelsius, Milliamps, Watts};

/// Converts current to power and decides when a change is worth a message.
#[derive(Copy, Clone, Debug)]
pub struct PowerMonitor {
    limit_ma: u32,
    receiver_voltage: u8,
    cos_phi_percent: u8,
}

impl PowerMonitor {
    /// Builds a monitor from the configured limit and load parameters.
    #[must_use]
    pub const fn new(max_current_a: u8, receiver_voltage: u8, cos_phi_percent: u8) -> Self {
        Self {
            limit_ma: max_current_a as u32 * 1000,
            receiver_voltage,
            cos_phi_percent,
        }
    }

    /// Whether the reading exceeds the configured limit.
    #[must_use]
    pub fn over_limit(&self, current: Milliamps) -> bool {
        u32::from(current) > self.limit_ma
    }

    /// Apparent power for a current reading.
    ///
    /// `mA * V * percent / 100000`. The widest intermediate is
    /// `65535 * 230 * 100`, which is 1.5e9 -- inside `u32`.
    #[must_use]
    pub fn power_w(&self, current: Milliamps) -> Watts {
        u32::from(current) * u32::from(self.receiver_voltage) * u32::from(self.cos_phi_percent)
            / 100_000
    }

    /// Deadband so a noisy ADC does not flood the bus: absolute below 1 A,
    /// relative above it.
    ///
    /// Stateless in the previous reading, so one monitor serves `FourRelay`'s
    /// four independent loads; the caller keeps the per-channel history.
    #[must_use]
    pub fn should_report(&self, current: Milliamps, last_reported: Milliamps) -> bool {
        if current == 0 && last_reported == 0 {
            return false;
        }
        let delta = u32::from(current.abs_diff(last_reported));
        if current < 1000 {
            delta >= 100
        } else {
            // A tenth of the last reported value. Integer division truncates, so
            // this is very slightly more willing to report than the float version
            // was -- by at most one milliamp, well under the ADC's resolution.
            delta >= u32::from(last_reported) / 10
        }
    }
}

/// Compares a temperature against the configured limit.
#[derive(Copy, Clone, Debug)]
pub struct ThermalMonitor {
    limit_dc: DeciCelsius,
}

impl ThermalMonitor {
    /// Builds a monitor from the configured limit.
    #[must_use]
    pub const fn new(max_temperature_c: u8) -> Self {
        Self {
            limit_dc: max_temperature_c as DeciCelsius * 10,
        }
    }

    /// Whether the reading exceeds the configured limit.
    #[must_use]
    pub fn over_limit(&self, temperature: DeciCelsius) -> bool {
        temperature > self.limit_dc
    }
}

/// A fault that must be reported to the controller once when it appears and
/// once when it clears.
///
/// The pre-refactor sketch handled the two faults inconsistently: the thermal
/// path was guarded by `&& !InformControllerTS` and so reported once, while the
/// overcurrent path had no such guard and re-sent the same status on every
/// iteration of `loop()` for as long as the fault lasted. Both report once now.
/// The protective action itself is still re-applied every iteration while the
/// fault is active -- only the message is suppressed.
#[derive(Copy, Clone, Debug, Default)]
pub struct LatchedFault {
    active: bool,
    reported: bool,
}

impl LatchedFault {
    /// A cleared fault.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active: false,
            reported: false,
        }
    }

    /// Records the current state; returns whether the controller needs telling.
    pub fn update(&mut self, active: bool) -> bool {
        let changed = active != self.reported;
        self.reported = active;
        self.active = active;
        changed
    }

    /// Whether the fault is currently active.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.active
    }

    /// The controller wrote to the status child.
    ///
    /// Adopt its value and re-arm reporting, so the next real transition is
    /// sent again.
    pub fn override_from_controller(&mut self, active: bool) {
        self.active = active;
        self.reported = active;
    }
}
