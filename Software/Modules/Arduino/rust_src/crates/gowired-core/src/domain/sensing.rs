//! Turning ADC counts into physical quantities.
//!
//! This is the arithmetic half of GoWired-lib's `PowerSensor` and `AnalogTemp`.
//! The sampling loops stay in `gowired-avr` -- they need the ADC and the clock --
//! but the scaling does not need hardware to be checked, so it lives here where
//! the host tests can reach it.
//!
//! That split matters more than it looks. Every function below has an ordering of
//! its intermediates chosen so nothing exceeds `u32`, and getting one wrong would
//! wrap silently and report a *small* current during an overload. In
//! `gowired-avr` that would be untestable; here it is four tests.

use crate::hal::{DeciCelsius, Milliamps, Millivolts};

/// Peak-to-peak to RMS for a sinusoid, times 10000.
///
/// `1 / (2 * sqrt(2))` is 0.35355. The original used 0.3535 and this keeps that,
/// rather than the more accurate value, so readings do not shift against a
/// controller's history for no reason.
const RMS_SCALE_X10000: u32 = 3535;

/// Peak-to-peak ADC swing to milliamps, for a mains load.
///
/// ```text
/// mA = p2p * vcc_mv * 0.3535 / (mv_per_amp * 1024) * 1000
/// ```
///
/// Evaluated as `(p2p * vcc / mv_per_amp) * 3535 / 10240`. For the sensors
/// actually fitted the widest intermediate is `1023 * 5500 / 185 * 3535`, about
/// 1.1e8; the obvious left-to-right order would overflow at
/// `1023 * 5500 * 3535`.
///
/// The multiply saturates rather than wrapping, because the bound above assumes a
/// sane `mv_per_amp`. A mistyped 1 in the configuration makes the intermediate
/// 2.4e11, and with `overflow-checks` off -- which is how the firmware is built --
/// a wrap would report a *small* current in the middle of an overload. Saturating
/// gives an obviously wrong large reading instead, which trips the protection.
#[must_use]
pub fn ac_counts_to_ma(peak_to_peak: u16, vcc_mv: Millivolts, mv_per_amp: u8) -> Milliamps {
    if peak_to_peak == 0 || mv_per_amp == 0 {
        return 0;
    }
    let scaled = u32::from(peak_to_peak) * u32::from(vcc_mv) / u32::from(mv_per_amp);
    saturate(scaled.saturating_mul(RMS_SCALE_X10000) / 10_240)
}

/// Averaged ADC deflection to milliamps, for a DC load.
///
/// ```text
/// mA = counts * vcc_mv / (mv_per_amp * 1024) * 1000
/// ```
///
/// Evaluated as `(counts * vcc / mv_per_amp) * 1000 / 1024`; widest intermediate
/// about 3.0e7. Saturating for the same reason as [`ac_counts_to_ma`].
#[must_use]
pub fn dc_counts_to_ma(counts: u16, vcc_mv: Millivolts, mv_per_amp: u8) -> Milliamps {
    if counts == 0 || mv_per_amp == 0 {
        return 0;
    }
    let scaled = u32::from(counts) * u32::from(vcc_mv) / u32::from(mv_per_amp);
    saturate(scaled.saturating_mul(1000) / 1024)
}

/// ADC counts to tenths of a degree, for a linear analog thermometer.
///
/// ```text
/// mV = counts * vcc_mv / 1024
/// dC = (mV - zero_mv) * 1000 / mv_per_celsius_x100
/// ```
///
/// The `* 1000` is `* 100` to undo the coefficient's scaling and `* 10` for
/// tenths. Signed throughout, because a module in an unheated space reads below
/// zero and the original's `float` handled that silently.
#[must_use]
pub fn counts_to_decicelsius(
    counts: u16,
    vcc_mv: Millivolts,
    zero_voltage_mv: Millivolts,
    mv_per_celsius_x100: u16,
) -> DeciCelsius {
    if mv_per_celsius_x100 == 0 {
        return 0;
    }
    let millivolts = (u32::from(counts) * u32::from(vcc_mv) / 1024) as i32;
    let above_zero = millivolts - i32::from(zero_voltage_mv);
    let decicelsius = above_zero.saturating_mul(1000) / i32::from(mv_per_celsius_x100);
    decicelsius.clamp(i32::from(DeciCelsius::MIN), i32::from(DeciCelsius::MAX)) as DeciCelsius
}

/// Supply voltage from a conversion of the internal 1.1 V bandgap against AVcc.
///
/// `1126400 = 1.1 * 1024 * 1000`, so the quotient is millivolts. Integer
/// division is exact enough: at 5 V the raw reading is about 225, and one count
/// either way is 22 mV -- the bandgap's own tolerance is wider than that.
#[must_use]
pub fn bandgap_to_millivolts(raw: u16) -> Millivolts {
    if raw == 0 {
        return 0; // ADC not running; do not divide by it
    }
    (1_126_400 / u32::from(raw)).min(u32::from(Millivolts::MAX)) as Millivolts
}

fn saturate(value: u32) -> Milliamps {
    value.min(u32::from(Milliamps::MAX)) as Milliamps
}
