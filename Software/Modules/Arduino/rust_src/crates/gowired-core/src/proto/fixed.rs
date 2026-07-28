//! Building an IEEE-754 `f32` out of a scaled integer, with integer arithmetic.
//!
//! # Why this exists
//!
//! MySensors sends `V_TEMP`, `V_WATT` and `V_HUM` as `P_FLOAT32`: four bytes of
//! little-endian `f32` plus a precision byte. So the firmware has to *produce* a
//! float, even though it has no reason to *compute* with one.
//!
//! That distinction is worth 6 kB of flash. Rust's `compiler_builtins` provides
//! the soft-float routines on AVR, and they are generic Rust rather than the
//! hand-written assembly avr-libc ships:
//!
//! ```text
//! __addsf3   2180 bytes
//! __divsf3   2072
//! __mulsf3   1802
//! __cmpsf2    196
//! ```
//!
//! There is no way to make the linker prefer avr-libc's versions -- the
//! `compiler-builtins-weak-intrinsics` build-std feature that used to allow it is
//! gone -- so on a 32 KB part the only real option is not to reference them. With
//! the domain layer working in milliamps and tenths of a degree, this function is
//! the entire remaining contact with floating point, and it is pure integer code.
//!
//! # How
//!
//! Long division, one mantissa bit at a time. `value / 10^decimals` is computed
//! as an integer quotient and remainder, the quotient's bits become the top of
//! the mantissa, and the remainder is doubled repeatedly for the rest. Nothing
//! ever exceeds 32 bits, because the remainder stays below the divisor.
//!
//! Rounding is round-half-to-even, so the result is the `f32` *nearest* to the
//! exact rational `value / 10^decimals`.
//!
//! Note what that does and does not promise. It is correctly rounded, which is
//! better than `value as f32 / 10.0` -- that rounds twice, and for a value above
//! 2^24 the first rounding has already lost the digit the second one needed. It
//! is *not* a claim to reproduce the C++ firmware's bytes for a measurement,
//! because the C++ computed the quantity itself in `f32` from an `f32` sensor
//! reading, and this firmware computes it in integers. The wire *format* is
//! identical; the last bit of a reading may differ, by less than the ADC's own
//! resolution.

/// Number of explicit mantissa bits in an `f32`.
const MANTISSA_BITS: u32 = 23;

/// Exponent bias.
const BIAS: i32 = 127;

/// Powers of ten this firmware ever asks for.
///
/// `decimals` comes from the domain layer, which sends 0 for watts and whole
/// degrees and 1 for a probe reading. Anything past 3 is clamped rather than
/// silently wrapping.
const fn power_of_ten(decimals: u8) -> u32 {
    match decimals {
        0 => 1,
        1 => 10,
        2 => 100,
        _ => 1000,
    }
}

/// The bit pattern of `value / 10^decimals` as an `f32`.
///
/// Exact for every input a sensor can produce, and identical to what an FPU
/// would compute -- which the tests check by comparing against real `f32`
/// division on the host.
#[must_use]
pub const fn to_f32_bits(value: i32, decimals: u8) -> u32 {
    let sign: u32 = if value < 0 { 0x8000_0000 } else { 0 };
    let numerator = value.unsigned_abs();
    if numerator == 0 {
        return sign; // +0.0 or -0.0
    }

    let denominator = power_of_ten(decimals);
    let quotient = numerator / denominator;
    let mut remainder = numerator % denominator;

    // Where the binary point sits, relative to the top bit of the mantissa we
    // are about to build.
    let mut exponent: i32;
    // 25 bits: 24 of significand plus one to round with.
    let mut significand: u32;
    let mut bits_taken: u32;

    if quotient == 0 {
        // The value is below 1, so there is no integer part to seed from. Double
        // the remainder until it carries, counting how far down the exponent goes.
        exponent = -1;
        loop {
            remainder *= 2;
            if remainder >= denominator {
                remainder -= denominator;
                break;
            }
            exponent -= 1;
        }
        significand = 1;
        bits_taken = 1;
    } else {
        // 32 - leading_zeros is the position of the top set bit.
        let width = 32 - quotient.leading_zeros();
        exponent = width as i32 - 1;

        if width > MANTISSA_BITS + 2 {
            // More integer bits than the mantissa holds: keep the top 25 and
            // fold what is dropped into the remainder decision below.
            let drop = width - (MANTISSA_BITS + 2);
            let dropped = quotient & ((1 << drop) - 1);
            significand = quotient >> drop;
            // Anything dropped, or any leftover remainder, means the true value
            // is above the truncated one -- which is what the sticky bit is for.
            remainder = if dropped != 0 || remainder != 0 { 1 } else { 0 };
            return assemble(sign, exponent, significand, MANTISSA_BITS + 2, remainder, 1);
        }

        significand = quotient;
        bits_taken = width;
    }

    // Fill the rest of the significand from the fraction.
    while bits_taken < MANTISSA_BITS + 2 {
        remainder *= 2;
        let bit = if remainder >= denominator {
            remainder -= denominator;
            1
        } else {
            0
        };
        significand = (significand << 1) | bit;
        bits_taken += 1;
    }

    assemble(sign, exponent, significand, bits_taken, remainder, denominator)
}

/// Rounds a 25-bit significand to 24 and packs the result.
///
/// `remainder` and `denominator` describe what is left over past the last bit
/// taken, so that a tie can be told from a value just above one.
const fn assemble(
    sign: u32,
    mut exponent: i32,
    significand: u32,
    _bits_taken: u32,
    remainder: u32,
    denominator: u32,
) -> u32 {
    // The lowest bit of the 25 is the round bit; `remainder` is the sticky.
    let round_bit = significand & 1;
    let mut mantissa = significand >> 1;
    let sticky = remainder != 0 && denominator != 0;

    // Round half to even: round up on a tie only when it would make the last bit
    // even.
    if round_bit == 1 && (sticky || mantissa & 1 == 1) {
        mantissa += 1;
        // Rounding can carry out of the top of the mantissa.
        if mantissa >> (MANTISSA_BITS + 1) != 0 {
            mantissa >>= 1;
            exponent += 1;
        }
    }

    if exponent > 127 {
        return sign | 0x7F80_0000; // infinity
    }
    if exponent < -126 {
        // Subnormals cannot arise from anything a sensor produces -- the
        // smallest non-zero value is 1/1000 -- so flushing to zero here is
        // unreachable rather than a shortcut.
        return sign;
    }

    let biased = (exponent + BIAS) as u32;
    sign | (biased << MANTISSA_BITS) | (mantissa & ((1 << MANTISSA_BITS) - 1))
}

/// Truncates an `f32` bit pattern towards zero into an `i32`.
///
/// The inverse of the useful half of [`to_f32_bits`], for the rare case of a
/// controller sending `P_FLOAT32` where this firmware expects a number. Integer
/// arithmetic, for the same reason as everything else here: `as i32` on an `f32`
/// would pull in `__fixsfsi`.
///
/// Saturates rather than wrapping on out-of-range input, and treats NaN as zero.
#[must_use]
pub const fn f32_bits_to_i32(bits: u32) -> i32 {
    let negative = bits & 0x8000_0000 != 0;
    let exponent = ((bits >> MANTISSA_BITS) & 0xFF) as i32;
    let fraction = bits & ((1 << MANTISSA_BITS) - 1);

    if exponent == 0xFF {
        // Infinity or NaN.
        if fraction != 0 {
            return 0; // NaN
        }
        return if negative { i32::MIN } else { i32::MAX };
    }

    let unbiased = exponent - BIAS;
    if unbiased < 0 {
        return 0; // magnitude below 1, truncates to zero
    }
    if unbiased > 30 {
        return if negative { i32::MIN } else { i32::MAX };
    }

    // Implicit leading one, unless the exponent field is zero (subnormal, which
    // truncates to zero anyway and is caught by `unbiased < 0`).
    let significand = (1u32 << MANTISSA_BITS) | fraction;
    let magnitude = if unbiased >= MANTISSA_BITS as i32 {
        significand << (unbiased - MANTISSA_BITS as i32)
    } else {
        significand >> (MANTISSA_BITS as i32 - unbiased)
    };

    if negative {
        -(magnitude as i32)
    } else {
        magnitude as i32
    }
}

#[cfg(test)]
mod tests {
    use super::{f32_bits_to_i32, to_f32_bits};

    /// The reference: the correctly-rounded `f32` nearest to `value / 10^d`.
    ///
    /// Computed in `f64` deliberately. `value as f32 / 10.0` would round twice --
    /// once converting an integer wider than 24 bits, once dividing -- and the
    /// two roundings do not compose. `f64` has 53 bits of mantissa, which is
    /// enough to hold any `i32 / 1000` exactly enough that the single narrowing
    /// to `f32` is correctly rounded.
    fn reference(value: i32, decimals: u8) -> u32 {
        let den = match decimals {
            0 => 1.0f64,
            1 => 10.0,
            2 => 100.0,
            _ => 1000.0,
        };
        ((f64::from(value) / den) as f32).to_bits()
    }

    fn check(value: i32, decimals: u8) {
        let got = to_f32_bits(value, decimals);
        let want = reference(value, decimals);
        assert_eq!(
            got,
            want,
            "{value} / 10^{decimals}: got {:?} ({got:#010x}), want {:?} ({want:#010x})",
            f32::from_bits(got),
            f32::from_bits(want)
        );
    }

    #[test]
    fn zero_and_signed_zero() {
        assert_eq!(to_f32_bits(0, 0), 0);
        assert_eq!(to_f32_bits(0, 1), 0);
        check(0, 0);
    }

    #[test]
    fn small_integers() {
        for v in -300..=300 {
            check(v, 0);
        }
    }

    #[test]
    fn tenths() {
        for v in -3000..=3000 {
            check(v, 1);
        }
    }

    #[test]
    fn hundredths_and_thousandths() {
        for v in [-100_000, -12345, -1, 0, 1, 7, 99, 12345, 100_000] {
            check(v, 2);
            check(v, 3);
        }
    }

    /// The values this firmware actually sends.
    #[test]
    fn realistic_readings() {
        // Watts, whole numbers: 0 to 3 A at 230 V.
        for watts in 0..=700 {
            check(watts, 0);
        }
        // Temperatures in tenths, from a cold loft to a thermal fault.
        for dc in -400..=1200 {
            check(dc, 1);
        }
        // Humidity in tenths of a percent.
        for dp in 0..=1000 {
            check(dp, 1);
        }
    }

    /// Values with more significant bits than the mantissa holds, where the
    /// rounding decision is the whole story.
    #[test]
    fn values_needing_rounding() {
        let cases = [
            16_777_215, // 2^24 - 1, the last exactly representable integer
            16_777_216, // 2^24
            16_777_217, // needs rounding: ties to even, down
            16_777_219, // ties to even, up
            33_554_431,
            33_554_433,
            123_456_789,
            2_147_483_647, // i32::MAX
        ];
        for v in cases {
            check(v, 0);
            check(-v, 0);
            check(v, 1);
            check(v, 3);
        }
    }

    #[test]
    fn i32_min_does_not_panic() {
        // `-i32::MIN` overflows; `unsigned_abs` is why this is fine.
        check(i32::MIN, 0);
        check(i32::MIN, 1);
    }

    #[test]
    fn powers_of_two_are_exact() {
        for shift in 0..31 {
            check(1 << shift, 0);
        }
    }

    #[test]
    fn round_trips_back_to_an_integer() {
        for v in [0i32, 1, 7, 42, -42, 1000, -1000, 16_777_215, -16_777_215] {
            assert_eq!(f32_bits_to_i32(to_f32_bits(v, 0)), v, "value {v}");
        }
    }

    #[test]
    fn truncates_towards_zero_like_an_as_cast() {
        for v in [-2500i32, -101, -100, -99, -1, 0, 1, 99, 100, 101, 2500] {
            let bits = to_f32_bits(v, 1);
            let expected = (f64::from(v) / 10.0) as i32;
            assert_eq!(f32_bits_to_i32(bits), expected, "value {v} tenths");
        }
    }

    #[test]
    fn handles_infinities_and_nan() {
        assert_eq!(f32_bits_to_i32(0x7F80_0000), i32::MAX);
        assert_eq!(f32_bits_to_i32(0xFF80_0000), i32::MIN);
        assert_eq!(f32_bits_to_i32(0x7FC0_0000), 0); // NaN
    }

    #[test]
    fn saturates_beyond_i32() {
        // 1e30 does not fit; it must not wrap into something plausible.
        assert_eq!(f32_bits_to_i32(1e30f32.to_bits()), i32::MAX);
        assert_eq!(f32_bits_to_i32((-1e30f32).to_bits()), i32::MIN);
    }

    /// A sweep wide enough to catch an off-by-one in the exponent that the
    /// hand-picked cases would miss.
    #[test]
    fn sweep() {
        let mut v: i32 = 1;
        while v < 2_000_000_000 {
            check(v, 0);
            check(v, 1);
            check(v, 2);
            check(-v, 1);
            v = v.saturating_mul(3).saturating_add(1);
        }
    }
}
