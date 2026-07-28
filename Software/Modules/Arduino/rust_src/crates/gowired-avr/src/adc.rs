//! Analog input, and measuring the supply voltage without a spare pin.

use gowired_core::hal::{self, Millivolts};

use crate::clock::delay_ms;
use crate::pins::adc_channel;
use crate::regs::{rd, set_bits, wr, ADCH, ADCL, ADCSRA, ADMUX};

/// `ADEN`: ADC enable.
const ADEN: u8 = 1 << 7;
/// `ADSC`: start conversion.
const ADSC: u8 = 1 << 6;
/// `ADPS2 | ADPS1`: prescaler 64, giving 8000000/64 = 125 kHz.
///
/// The ADC wants 50--200 kHz for full 10-bit accuracy.
const PRESCALER_64: u8 = (1 << 2) | (1 << 1);

/// `REFS0`: AVcc as the reference.
const REFS_AVCC: u8 = 1 << 6;

/// MUX value that selects the internal 1.1 V bandgap instead of a pin.
const MUX_BANDGAP: u8 = 0b1110;

/// Enables the ADC.
///
/// # Safety
///
/// Call once, at startup.
pub unsafe fn init() {
    wr(ADCSRA, ADEN | PRESCALER_64);
}

/// One 10-bit conversion on an analog pin, referenced to AVcc.
///
/// Accepts either analog numbering or the `A`-pin's digital number -- see
/// [`adc_channel`].
pub fn read(pin: u8) -> u16 {
    // SAFETY: writing ADMUX selects a channel; ADSC starts a conversion; ADCL
    // must be read before ADCH, which is what latches the pair.
    unsafe {
        wr(ADMUX, REFS_AVCC | (adc_channel(pin) & 0x0F));
        set_bits(ADCSRA, ADSC);
        while rd(ADCSRA) & ADSC != 0 {
            core::hint::spin_loop();
        }
        let low = rd(ADCL);
        let high = rd(ADCH);
        u16::from(low) | (u16::from(high) << 8)
    }
}

/// Measures Vcc by comparing the internal 1.1 V bandgap against AVcc.
///
/// Needs no external components and no spare pin: the bandgap is a known voltage,
/// so a conversion of it referenced to AVcc gives AVcc.
///
/// `1126400 = 1.1 V * 1024 * 1000`, so the quotient is millivolts. Integer
/// division, which is exact enough: at a plausible 5 V the raw reading is around
/// 225, and one count either way is 22 mV -- the bandgap's own tolerance is
/// wider than that.
pub fn vcc_mv() -> Millivolts {
    // SAFETY: as `read`. The bandgap needs time to settle after being selected,
    // which is what the delay is for -- without it the first conversion reads
    // whatever the previous channel left in the sample-and-hold.
    unsafe {
        wr(ADMUX, REFS_AVCC | MUX_BANDGAP);
        delay_ms(2);

        set_bits(ADCSRA, ADSC);
        while rd(ADCSRA) & ADSC != 0 {
            core::hint::spin_loop();
        }
        let low = rd(ADCL);
        let high = rd(ADCH);
        let raw = u16::from(low) | (u16::from(high) << 8);

        gowired_core::domain::sensing::bandgap_to_millivolts(raw)
    }
}

/// The bandgap measurement, behind [`hal::VoltageReference`].
pub struct VoltageReference;

impl hal::VoltageReference for VoltageReference {
    fn vcc_mv(&self) -> Millivolts {
        vcc_mv()
    }
}
