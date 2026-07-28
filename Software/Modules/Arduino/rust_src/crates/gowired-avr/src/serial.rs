//! USART0, with an interrupt-driven receive ring.
//!
//! # Why receive has to be interrupt driven
//!
//! The USART holds exactly one received byte. At 57600 baud a byte arrives every
//! 174 microseconds, and this firmware routinely stops paying attention for far
//! longer than that: a wall-switch poll blocks for as long as the button is held
//! (up to a second), a dimmer ramp blocks until the strip reaches its brightness,
//! and an EEPROM write stalls the CPU for 3.4 ms. Polling would drop frames
//! throughout. The 64-byte ring is what the Arduino `HardwareSerial` provided and
//! what the RS485 transport was written against.
//!
//! Transmit is polled, unlike Arduino's. It is simpler, and the RS485 driver has
//! to be held enabled until the last stop bit is out anyway -- so there is
//! nothing to gain from queueing and then immediately waiting for the queue to
//! drain.
//!
//! # Baud rate
//!
//! 57600 at 8 MHz does not divide evenly. With `U2X0` set the divisor is
//! `8000000/(8*57600) - 1 = 16.36`, which truncates to 16 and gives 58824 baud --
//! 2.1% fast. That is inside the tolerance for 8-bit no-parity framing, and it is
//! exactly what the Arduino core computed for the C++ build, so both ends of the
//! link are wrong by the same amount as before.

use core::cell::UnsafeCell;

use gowired_core::proto::rs485::SerialPort;

use crate::regs::{rd, set_bits, wr, UBRR0H, UBRR0L, UCSR0A, UCSR0B, UCSR0C, UDR0};

/// `UDRE0`: transmit register empty.
const UDRE0: u8 = 1 << 5;
/// `TXC0`: transmit complete.
const TXC0: u8 = 1 << 6;
/// `U2X0`: double speed.
const U2X0: u8 = 1 << 1;
/// `RXEN0`: receiver enable.
const RXEN0: u8 = 1 << 4;
/// `TXEN0`: transmitter enable.
const TXEN0: u8 = 1 << 3;
/// `RXCIE0`: receive-complete interrupt enable.
const RXCIE0: u8 = 1 << 7;
/// `UCSZ01 | UCSZ00`: 8 data bits.
const EIGHT_BITS: u8 = (1 << 2) | (1 << 1);

/// Receive ring size. A power of two, so the wrap is a mask rather than a modulo.
pub const RX_BUFFER_LEN: usize = 64;

struct Ring {
    buffer: UnsafeCell<[u8; RX_BUFFER_LEN]>,
    /// Written by the interrupt handler only.
    head: UnsafeCell<u8>,
    /// Written by the main loop only.
    tail: UnsafeCell<u8>,
    /// Bytes lost because the ring was full. Never resets.
    overruns: UnsafeCell<u8>,
}

// SAFETY: single core. `head` and `tail` are single bytes, so a load or store of
// either is one instruction and cannot be observed half-done; each is written
// from one side only.
unsafe impl Sync for Ring {}

static RX: Ring = Ring {
    buffer: UnsafeCell::new([0; RX_BUFFER_LEN]),
    head: UnsafeCell::new(0),
    tail: UnsafeCell::new(0),
    overruns: UnsafeCell::new(0),
};

/// USART receive complete.
///
/// # Safety
///
/// Called by the hardware only. `USART_RX` is vector 18 on both the ATmega328P
/// and the ATmega328PB -- checked against `iom328p.h` and `iom328pb.h`, where the
/// 328P calls it `USART0_RX_vect` and the 328PB `USART_RX_vect`, both number 18.
#[no_mangle]
pub unsafe extern "avr-interrupt" fn __vector_18() {
    let byte = rd(UDR0);

    let head = RX.head.get();
    let next = (head.read_volatile() + 1) % RX_BUFFER_LEN as u8;

    if next == RX.tail.get().read_volatile() {
        // Full. Dropping the newest byte corrupts the frame in progress, which
        // the transport's checksum will reject -- better than dropping the oldest
        // and splicing two frames together.
        let o = RX.overruns.get();
        o.write_volatile(o.read_volatile().saturating_add(1));
        return;
    }

    (*RX.buffer.get())[head.read_volatile() as usize] = byte;
    head.write_volatile(next);
}

/// Configures USART0 for 8N1 at `baud` and enables the receiver.
///
/// # Safety
///
/// Call once, before interrupts are enabled.
pub unsafe fn init(baud: u32) {
    let divisor = (crate::F_CPU / (8 * baud)).saturating_sub(1) as u16;

    wr(UCSR0A, U2X0);
    wr(UBRR0H, (divisor >> 8) as u8);
    wr(UBRR0L, (divisor & 0xFF) as u8);
    wr(UCSR0C, EIGHT_BITS);
    wr(UCSR0B, RXEN0 | TXEN0 | RXCIE0);
}

/// Bytes lost to a full receive ring.
///
/// Non-zero means the main loop stopped servicing the transport for more than
/// 64 byte times (11 ms at 57600 baud).
pub fn overruns() -> u8 {
    // SAFETY: single-byte read of a counter only the handler writes.
    unsafe { RX.overruns.get().read_volatile() }
}

/// USART0, behind [`SerialPort`].
pub struct Serial;

impl SerialPort for Serial {
    fn read_byte(&self) -> Option<u8> {
        // SAFETY: `tail` is written only here, `head` only by the handler, and
        // both are single bytes.
        unsafe {
            let tail = RX.tail.get();
            let t = tail.read_volatile();
            if t == RX.head.get().read_volatile() {
                return None;
            }
            let byte = (*RX.buffer.get())[t as usize];
            tail.write_volatile((t + 1) % RX_BUFFER_LEN as u8);
            Some(byte)
        }
    }

    fn available(&self) -> bool {
        // SAFETY: as `read_byte`.
        unsafe { RX.tail.get().read_volatile() != RX.head.get().read_volatile() }
    }

    fn write_byte(&self, byte: u8) {
        // SAFETY: waiting for UDRE0 before writing UDR0 is the documented
        // sequence. TXC0 is cleared by writing a one to it, so that `flush` can
        // tell this byte's completion from the previous one's.
        unsafe {
            while rd(UCSR0A) & UDRE0 == 0 {
                core::hint::spin_loop();
            }
            set_bits(UCSR0A, TXC0);
            wr(UDR0, byte);
        }
    }

    fn flush(&self) {
        // SAFETY: reading UCSR0A has no side effects.
        //
        // TXC0, not UDRE0: UDRE0 goes high as soon as the *buffer* is free, while
        // the last byte is still being shifted out. Dropping the RS485 driver
        // then would truncate it.
        unsafe {
            while rd(UCSR0A) & TXC0 == 0 {
                core::hint::spin_loop();
            }
        }
    }
}
