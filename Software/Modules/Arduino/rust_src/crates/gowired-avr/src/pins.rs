//! Arduino pin numbers to port and bit.
//!
//! The configuration, the board documentation and the silkscreen all speak in
//! Arduino numbering, so that is what the HAL takes. This is the only place that
//! knows how it maps onto the hardware:
//!
//! ```text
//!  0..7   PORTD bit 0..7
//!  8..13  PORTB bit 0..5
//! 14..19  PORTC bit 0..5   (A0..A5)
//! 20..21  no port          (A6, A7 are ADC-only on the TQFP package)
//! ```
//!
//! A6 and A7 exist as ADC channels but have no digital input buffer, no pull-up
//! and no output driver. The GoWired boards use them for the current sensor and
//! the thermistor, which is exactly what they are good for. Asking [`port_of`]
//! about them yields `None`, and every digital operation on them is a no-op
//! rather than a write to a register that does not exist.

use crate::regs;

/// Which port a pin belongs to.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Port {
    /// Data direction register.
    pub ddr: u16,
    /// Output register.
    pub out: u16,
    /// Input register.
    pub input: u16,
    /// Bit within the port.
    pub bit: u8,
}

impl Port {
    /// The bit as a mask.
    #[inline(always)]
    pub const fn mask(&self) -> u8 {
        1 << self.bit
    }
}

/// The port and bit an Arduino pin number refers to.
///
/// `None` for A6, A7 and anything out of range.
#[inline]
pub const fn port_of(pin: u8) -> Option<Port> {
    match pin {
        0..=7 => Some(Port {
            ddr: regs::DDRD,
            out: regs::PORTD,
            input: regs::PIND,
            bit: pin,
        }),
        8..=13 => Some(Port {
            ddr: regs::DDRB,
            out: regs::PORTB,
            input: regs::PINB,
            bit: pin - 8,
        }),
        14..=19 => Some(Port {
            ddr: regs::DDRC,
            out: regs::PORTC,
            input: regs::PINC,
            bit: pin - 14,
        }),
        // A6 and A7: ADC channels with no digital hardware behind them.
        _ => None,
    }
}

/// The ADC channel an analog pin selects.
///
/// Accepts either the analog numbering (0..7) or the digital pin number of an
/// `A`-pin (14..21), because the configuration writes `A1` and `A7`, which are
/// 15 and 21.
#[inline]
pub const fn adc_channel(pin: u8) -> u8 {
    if pin >= 14 {
        pin - 14
    } else {
        pin
    }
}

/// First Arduino pin number that is analog-only.
pub const FIRST_ANALOG_ONLY: u8 = 20;
