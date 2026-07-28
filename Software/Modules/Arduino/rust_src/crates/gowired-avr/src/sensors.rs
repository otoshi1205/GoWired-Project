//! Current and temperature sampling.
//!
//! Ports of GoWired-lib's `PowerSensor::MeasureAC` / `MeasureDC` and
//! `AnalogTemp::MeasureT`. The sampling loops are unchanged -- they are field
//! proven and the constants are tied to the ACS712 and the thermistor actually
//! fitted -- but the *decisions* those classes also made
//! (`CalculatePower`, `ElectricalStatus`, `ThermalStatus`) stayed in
//! [`gowired_core::domain::monitors`], where they can be tested.
//!
//! # Where the arithmetic went
//!
//! Only the sampling loops are here. The scaling -- counts to milliamps, counts to
//! tenths of a degree -- is in [`gowired_core::domain::sensing`], because it needs
//! no hardware to check and getting it wrong is dangerous in a specific way: an
//! intermediate that overflows reports a *small* current in the middle of an
//! overload. In this crate that would be untestable. Over there it is a dozen
//! tests, one of which caught exactly that.
//!
//! The originals worked in `float`; everything is integer now, because `f32`
//! arithmetic on AVR costs 6 kB of flash -- see [`gowired_core::hal::Milliamps`].
//! Nothing measurable is lost: one ADC count at 185 mV/A and 5 V is about 26 mA,
//! so a milliamp is already forty times finer than the hardware can resolve.
//!
//! Porting these 80 lines is also what removes three of the C++ build's four
//! library dependencies: `GoWired-lib` was pulled in for exactly this, and
//! `ADCTouch` and `PCF8575-lib` only because Arduino compiles every source file
//! of a library it can see.

use core::cell::Cell;

use gowired_core::domain::config::{PowerTuning, ThermalTuning};
use gowired_core::domain::sensing;
use gowired_core::hal::{self, DeciCelsius, Milliamps, Millivolts, Pin};

use crate::adc;
use crate::clock::millis;

/// Peak-to-peak counts at or below this are ADC noise rather than a load.
///
/// Carried over verbatim from `PowerSensor::MeasureAC`. Fifteen counts at
/// 185 mV/A is roughly 0.2 A, which is about the noise floor of an ACS712 read by
/// a 10-bit ADC.
const NOISE_FLOOR_COUNTS: u16 = 15;

/// A hall-effect current sensor (ACS712) on one analog pin.
pub struct CurrentSensor {
    pin: Pin,
    tuning: PowerTuning,
    /// Quiescent ADC reading, sampled by `begin`.
    ///
    /// A `Cell` because the HAL traits take `&self` -- the ADC is shared
    /// hardware, and pretending otherwise would mean no two things could read it.
    zero_offset: Cell<u16>,
}

impl CurrentSensor {
    /// Binds a sensor to a pin.
    pub const fn new(pin: Pin, tuning: PowerTuning) -> Self {
        Self {
            pin,
            tuning,
            zero_offset: Cell::new(0),
        }
    }

}

impl hal::CurrentSensor for CurrentSensor {
    fn begin(&self, _vcc_mv: Millivolts) {
        // Ten samples averaged, as the original did. The pin needs no `pinMode`:
        // on this board the current sensors are on A6/A7 and the I2C pins, and
        // the analog-only pins have no digital hardware to configure.
        let mut total: u16 = 0;
        for _ in 0..10 {
            total += adc::read(self.pin) / 10;
        }
        self.zero_offset.set(total);
    }

    fn measure_ac(&self, vcc_mv: Millivolts) -> Milliamps {
        let mut max: u16 = 0;
        let mut min: u16 = 1024;

        let start = millis();
        while millis().wrapping_sub(start) < u32::from(self.tuning.measuring_time_ms) {
            let value = adc::read(self.pin);
            max = max.max(value);
            min = min.min(value);
        }

        let mut peak_to_peak = max.saturating_sub(min);
        if peak_to_peak <= NOISE_FLOOR_COUNTS {
            peak_to_peak = 0;
        }

        sensing::ac_counts_to_ma(peak_to_peak, vcc_mv, self.tuning.mv_per_amp)
    }

    fn measure_dc(&self, vcc_mv: Millivolts) -> Milliamps {
        let mut sum: u32 = 0;
        let mut samples: u16 = 0;

        let start = millis();
        while millis().wrapping_sub(start) < u32::from(self.tuning.measuring_time_ms) {
            let value = i32::from(adc::read(self.pin)) - i32::from(self.zero_offset.get());
            sum += value.unsigned_abs();
            samples += 1;
        }

        if samples == 0 {
            return 0;
        }
        let average = (sum / u32::from(samples)) as u16;
        sensing::dc_counts_to_ma(average, vcc_mv, self.tuning.mv_per_amp)
    }
}

/// The on-board analog thermometer (an MCP9700-style linear sensor).
pub struct TemperatureSensor {
    pin: Pin,
    tuning: ThermalTuning,
}

impl TemperatureSensor {
    /// Binds a thermometer to a pin.
    pub const fn new(pin: Pin, tuning: ThermalTuning) -> Self {
        Self { pin, tuning }
    }

}

impl hal::TemperatureSensor for TemperatureSensor {
    fn begin(&self) {
        // Nothing to do: the thermometer sits on an ADC-only pin.
    }

    fn measure_decicelsius(&self, vcc_mv: Millivolts) -> DeciCelsius {
        sensing::counts_to_decicelsius(
            adc::read(self.pin),
            vcc_mv,
            self.tuning.zero_voltage_mv,
            self.tuning.mv_per_celsius_x100,
        )
    }
}
