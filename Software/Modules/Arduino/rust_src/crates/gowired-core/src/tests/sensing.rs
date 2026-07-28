//! ADC scaling, including the overflow orderings.
//!
//! These are the tests the C++ build could not have: the arithmetic lived in
//! GoWired-lib next to the sampling loop, so exercising it needed an ADC.

use crate::domain::sensing::{
    ac_counts_to_ma, bandgap_to_millivolts, counts_to_decicelsius, dc_counts_to_ma,
};

/// The board's own sensor: an ACS712-05B at 185 mV/A, read at 5 V.
const MV_PER_AMP: u8 = 185;
const VCC: u16 = 5000;

#[test]
fn no_swing_means_no_current() {
    assert_eq!(ac_counts_to_ma(0, VCC, MV_PER_AMP), 0);
    assert_eq!(dc_counts_to_ma(0, VCC, MV_PER_AMP), 0);
}

/// A misconfigured `mv_per_amp` of zero would be a division by zero.
#[test]
fn a_zero_coefficient_does_not_divide_by_zero() {
    assert_eq!(ac_counts_to_ma(500, VCC, 0), 0);
    assert_eq!(dc_counts_to_ma(500, VCC, 0), 0);
    assert_eq!(counts_to_decicelsius(500, VCC, 500, 0), 0);
}

/// 1 A RMS through an ACS712-05B swings 185 mV RMS, so 523 mV peak-to-peak,
/// which at 5 V over 1024 counts is about 107 counts.
#[test]
fn ac_scaling_matches_the_sensor() {
    let ma = ac_counts_to_ma(107, VCC, MV_PER_AMP);
    assert!(
        (950..=1050).contains(&ma),
        "107 counts should be about 1 A, got {ma} mA"
    );
}

#[test]
fn ac_scaling_is_linear() {
    let one = ac_counts_to_ma(107, VCC, MV_PER_AMP);
    let three = ac_counts_to_ma(321, VCC, MV_PER_AMP);
    // Within a percent of three times, allowing for integer truncation.
    assert!(
        (three as i32 - 3 * one as i32).abs() < 40,
        "{one} mA and {three} mA are not proportional"
    );
}

/// The reason the intermediates are ordered the way they are. Left to right,
/// `1023 * 5500 * 3535` is 2.0e10 and wraps; the actual expression peaks at
/// 1.1e8. A wrapped result would read as a *small* current in the middle of an
/// overload, which is the worst possible direction for the error to go.
#[test]
fn ac_scaling_does_not_wrap_at_full_scale() {
    let ma = ac_counts_to_ma(1023, u16::MAX, 1);
    // The true value is far beyond a u16, so it must saturate, not wrap.
    assert_eq!(ma, u16::MAX);

    // And at a plausible full-scale reading it is a believable number.
    let ma = ac_counts_to_ma(1023, VCC, MV_PER_AMP);
    assert!(
        (9000..=10_500).contains(&ma),
        "full swing should be about 9.5 A, got {ma} mA"
    );
}

#[test]
fn dc_scaling_matches_the_sensor() {
    // A 1 A DC load deflects the sensor 185 mV, about 38 counts at 5 V.
    let ma = dc_counts_to_ma(38, VCC, MV_PER_AMP);
    assert!(
        (950..=1060).contains(&ma),
        "38 counts should be about 1 A, got {ma} mA"
    );
}

#[test]
fn dc_scaling_does_not_wrap_at_full_scale() {
    assert_eq!(dc_counts_to_ma(1023, u16::MAX, 1), u16::MAX);
}

/// The supply voltage scales every reading, so the same swing at 3.3 V is a
/// smaller current than at 5 V.
#[test]
fn scaling_follows_the_supply_voltage() {
    let at_5v = ac_counts_to_ma(200, 5000, MV_PER_AMP);
    let at_3v3 = ac_counts_to_ma(200, 3300, MV_PER_AMP);
    assert!(at_3v3 < at_5v);
    // Proportionally: 3300/5000 of the 5 V figure.
    let expected = u32::from(at_5v) * 3300 / 5000;
    assert!((i32::from(at_3v3) - expected as i32).abs() < 20);
}

// ---------------------------------------------------------------------------
// Temperature
// ---------------------------------------------------------------------------

/// MCP9700: 500 mV at 0 °C, 10 mV per degree.
const ZERO_MV: u16 = 500;
const MV_PER_C_X100: u16 = 1000;

#[test]
fn zero_degrees_is_the_offset_voltage() {
    // 500 mV at 5 V over 1024 counts is 102.4 counts.
    let dc = counts_to_decicelsius(102, VCC, ZERO_MV, MV_PER_C_X100);
    assert!((-10..=10).contains(&dc), "should be about 0 °C, got {dc}");
}

#[test]
fn room_temperature_reads_as_expected() {
    // 21.5 °C is 500 + 215 = 715 mV, which is 146.4 counts at 5 V.
    let dc = counts_to_decicelsius(146, VCC, ZERO_MV, MV_PER_C_X100);
    assert!(
        (200..=230).contains(&dc),
        "should be about 21.5 °C, got {dc}"
    );
}

/// An outdoor or unheated module reads below zero, and the original's `float`
/// handled that silently. Integers have to be signed for it.
#[test]
fn below_zero_is_negative_not_wrapped() {
    // -10 °C is 400 mV, about 82 counts.
    let dc = counts_to_decicelsius(82, VCC, ZERO_MV, MV_PER_C_X100);
    assert!(dc < 0, "should be below zero, got {dc}");
    assert!((-120..=-80).contains(&dc), "should be about -10 °C, got {dc}");
}

#[test]
fn thermal_fault_temperatures_are_representable() {
    // 90 °C is 1400 mV, about 287 counts.
    let dc = counts_to_decicelsius(287, VCC, ZERO_MV, MV_PER_C_X100);
    assert!((880..=920).contains(&dc), "should be about 90 °C, got {dc}");
}

#[test]
fn temperature_saturates_rather_than_wrapping() {
    // A shorted sensor with an absurd coefficient: the result must clamp into an
    // i16 rather than wrap into a plausible reading.
    let dc = counts_to_decicelsius(1023, u16::MAX, 0, 1);
    assert_eq!(dc, i16::MAX);
}

// ---------------------------------------------------------------------------
// Supply voltage
// ---------------------------------------------------------------------------

#[test]
fn bandgap_reads_a_plausible_supply() {
    // At 5.0 V the 1.1 V bandgap reads 1.1/5.0 * 1024 = 225 counts.
    let mv = bandgap_to_millivolts(225);
    assert!((4900..=5100).contains(&mv), "should be about 5 V, got {mv}");

    // At 3.3 V it reads 341.
    let mv = bandgap_to_millivolts(341);
    assert!((3250..=3350).contains(&mv), "should be about 3.3 V, got {mv}");
}

/// A raw zero means the ADC never ran. Dividing by it would be a hang or a trap.
#[test]
fn bandgap_of_zero_is_not_a_division_by_zero() {
    assert_eq!(bandgap_to_millivolts(0), 0);
}

#[test]
fn bandgap_saturates_rather_than_wrapping() {
    // A raw reading of 1 implies 1.1 MV, which does not fit in a u16.
    assert_eq!(bandgap_to_millivolts(1), u16::MAX);
}
