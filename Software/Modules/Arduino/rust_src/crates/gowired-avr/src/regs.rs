//! Register addresses and the two primitives that reach them.
//!
//! # Why raw addresses rather than a PAC
//!
//! Every register this firmware touches is at the same address on the ATmega328P
//! and the ATmega328PB. Only the C *names* differ, and only for the TWI block:
//! avr-libc calls it `TWBR` on the 328P and `TWBR0` on the 328PB, both at 0xB8.
//! Verified against the toolchain's own `iom328p.h` and `iom328pb.h`:
//!
//! ```text
//! PORTB  ADMUX  UCSR0A  UDR0  TCCR0A  OCR0A  OCR1AL  WDTCSR  EECR   all identical
//! TWBR/TWBR0  TWSR/TWSR0  TWDR/TWDR0  TWCR/TWCR0     same address, different name
//! ```
//!
//! So a single table covers both parts, and the `variant=modelP` /
//! `variant=modelPB` distinction that the C++ build needed disappears from the
//! source: it survives only as the `-C target-cpu` the linker is given.
//!
//! `_SFR_IO8(x)` in avr-libc means data address `x + 0x20`; the constants here
//! are data addresses, already offset.

use core::ptr::{read_volatile, write_volatile};

/// Reads a peripheral register.
///
/// # Safety
///
/// `addr` must be a valid I/O register address. Reading some registers has side
/// effects -- `UDR0` consumes a received byte, `ADCL` latches `ADCH`.
#[inline(always)]
pub unsafe fn rd(addr: u16) -> u8 {
    read_volatile(addr as *const u8)
}

/// Writes a peripheral register.
///
/// # Safety
///
/// `addr` must be a valid I/O register address, and the value must make sense
/// for it: writing nonsense to `WDTCSR` or `TCCR0B` misconfigures the part.
#[inline(always)]
pub unsafe fn wr(addr: u16, value: u8) {
    write_volatile(addr as *mut u8, value);
}

/// Sets the given bits, leaving the rest.
///
/// # Safety
///
/// As [`rd`] and [`wr`]. Not atomic: an interrupt between the read and the write
/// loses whatever it wrote.
#[inline(always)]
pub unsafe fn set_bits(addr: u16, mask: u8) {
    wr(addr, rd(addr) | mask);
}

/// Clears the given bits, leaving the rest.
///
/// # Safety
///
/// As [`set_bits`].
#[inline(always)]
pub unsafe fn clear_bits(addr: u16, mask: u8) {
    wr(addr, rd(addr) & !mask);
}

// -- ports ------------------------------------------------------------------

/// Data direction, port B.
pub const DDRB: u16 = 0x24;
/// Output, port B.
pub const PORTB: u16 = 0x25;
/// Input, port B.
pub const PINB: u16 = 0x23;
/// Data direction, port C.
pub const DDRC: u16 = 0x27;
/// Output, port C.
pub const PORTC: u16 = 0x28;
/// Input, port C.
pub const PINC: u16 = 0x26;
/// Data direction, port D.
pub const DDRD: u16 = 0x2A;
/// Output, port D.
pub const PORTD: u16 = 0x2B;
/// Input, port D.
pub const PIND: u16 = 0x29;

// -- timer 0 (millis, and PWM on pins 5 and 6) ------------------------------

/// Timer 0 control A.
pub const TCCR0A: u16 = 0x44;
/// Timer 0 control B.
pub const TCCR0B: u16 = 0x45;
/// Timer 0 compare A -- pin 6.
pub const OCR0A: u16 = 0x47;
/// Timer 0 compare B -- pin 5.
pub const OCR0B: u16 = 0x48;
/// Timer 0 interrupt mask.
pub const TIMSK0: u16 = 0x6E;

// -- timer 1 (PWM on pins 9 and 10) -----------------------------------------

/// Timer 1 control A.
pub const TCCR1A: u16 = 0x80;
/// Timer 1 control B.
pub const TCCR1B: u16 = 0x81;
/// Timer 1 compare A low byte -- pin 9.
pub const OCR1AL: u16 = 0x88;
/// Timer 1 compare A high byte.
pub const OCR1AH: u16 = 0x89;
/// Timer 1 compare B low byte -- pin 10.
pub const OCR1BL: u16 = 0x8A;
/// Timer 1 compare B high byte.
pub const OCR1BH: u16 = 0x8B;

// -- ADC --------------------------------------------------------------------

/// ADC multiplexer and reference select.
pub const ADMUX: u16 = 0x7C;
/// ADC control and status A.
pub const ADCSRA: u16 = 0x7A;
/// ADC result, low byte. Read before `ADCH`.
pub const ADCL: u16 = 0x78;
/// ADC result, high byte.
pub const ADCH: u16 = 0x79;
/// Digital input disable, for the analog-only pins.
pub const DIDR0: u16 = 0x7E;

// -- USART0 -----------------------------------------------------------------

/// USART control and status A.
pub const UCSR0A: u16 = 0xC0;
/// USART control and status B.
pub const UCSR0B: u16 = 0xC1;
/// USART control and status C.
pub const UCSR0C: u16 = 0xC2;
/// Baud rate, low byte.
pub const UBRR0L: u16 = 0xC4;
/// Baud rate, high byte.
pub const UBRR0H: u16 = 0xC5;
/// USART data.
pub const UDR0: u16 = 0xC6;

// -- EEPROM -----------------------------------------------------------------

/// EEPROM control.
pub const EECR: u16 = 0x3F;
/// EEPROM data.
pub const EEDR: u16 = 0x40;
/// EEPROM address, low byte.
pub const EEARL: u16 = 0x41;
/// EEPROM address, high byte.
pub const EEARH: u16 = 0x42;

// -- system -----------------------------------------------------------------

/// Watchdog control.
pub const WDTCSR: u16 = 0x60;
/// MCU status: reset cause flags.
pub const MCUSR: u16 = 0x54;

// -- TWI (I2C), for the SHT30 probe -----------------------------------------
//
// `TWBR` on the 328P, `TWBR0` on the 328PB. Same address.

/// TWI bit rate.
pub const TWBR: u16 = 0xB8;
/// TWI status and prescaler.
pub const TWSR: u16 = 0xB9;
/// TWI data.
pub const TWDR: u16 = 0xBB;
/// TWI control.
pub const TWCR: u16 = 0xBC;
