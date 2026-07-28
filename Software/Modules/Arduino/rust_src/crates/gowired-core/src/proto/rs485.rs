//! The RS485 link: ICSC framing over a half-duplex serial line.
//!
//! MySensors' RS485 transport is not a MySensors invention -- it is the ICSC
//! protocol from Majenko Technologies' library, carrying one MySensors message
//! per `SYS_PACK` frame:
//!
//! ```text
//! SOH x N   collision-avoidance preamble (MY_RS485_SOH_COUNT of them)
//! dest      destination node id
//! sender    our node id
//! 0x58      ICSC_SYS_PACK, the only command used
//! len       payload length
//! STX
//! payload   len bytes: an encoded MySensors message
//! ETX
//! cs        sum of dest + sender + command + len + payload, mod 256
//! EOT
//! ```
//!
//! The receiver is a byte-at-a-time state machine that shifts a six-byte window
//! looking for `SOH .. STX`, which is what lets a node resynchronise mid-stream
//! after a collision. It is ported directly from `MyTransportRS485.cpp`,
//! including the rejections that matter for correctness on a shared bus: a frame
//! whose sender is us (our own echo, since RS485 is a single pair) and a frame
//! addressed to neither us nor broadcast.
//!
//! What is *not* ported: the C++ version's `rand()`-based collision backoff uses
//! the C library's global PRNG state. Here it is a small LCG seeded from the node
//! id, so two nodes that collide do not then back off in lockstep -- which the
//! shared `rand()` sequence made possible when several nodes booted together.

use core::cell::{Cell, RefCell};

use crate::hal::{Clock, Gpio, Pin, PinMode, Platform, NO_PIN};
use crate::proto::message::BROADCAST_ADDRESS;

/// Largest frame payload the transport will accept.
///
/// MySensors' `MY_RS485_MAX_MESSAGE_LENGTH`. Kept at its default so that the
/// accept/reject decision on a malformed length byte matches the C++ node's
/// exactly.
pub const MAX_FRAME_LENGTH: usize = 40;

const SOH: u8 = 1;
const STX: u8 = 2;
const ETX: u8 = 3;
const EOT: u8 = 4;

/// The only ICSC command MySensors uses.
const ICSC_SYS_PACK: u8 = 0x58;

/// A half-duplex byte pipe.
pub trait SerialPort {
    /// Takes the next received byte, if one is waiting.
    fn read_byte(&self) -> Option<u8>;

    /// Whether at least one byte is waiting.
    fn available(&self) -> bool;

    /// Queues a byte for transmission.
    fn write_byte(&self, byte: u8);

    /// Blocks until every queued byte has left the shift register.
    ///
    /// Load bearing on RS485: dropping DE before the stop bit is out truncates
    /// the last character.
    fn flush(&self);
}

/// What a transport has to do for [`super::node::Node`].
pub trait Transport {
    /// Brings the link up.
    fn init(&self) -> bool;

    /// Sets the address this node answers to.
    fn set_address(&self, address: u8);

    /// The address this node answers to.
    fn address(&self) -> u8;

    /// Sends `data` to node `to`. Returns `false` if the bus stayed busy.
    fn send(&self, to: u8, data: &[u8]) -> bool;

    /// Pumps the receiver; returns whether a complete frame is waiting.
    fn data_available(&self) -> bool;

    /// Takes the waiting frame into `out`; returns its length, or 0 if none.
    fn receive(&self, out: &mut [u8]) -> usize;
}

#[derive(Copy, Clone)]
struct Rx {
    header: [u8; 6],
    phase: u8,
    pos: usize,
    len: usize,
    station: u8,
    sender: u8,
    command: u8,
    checksum: u8,
    calculated: u8,
    data: [u8; MAX_FRAME_LENGTH],
    packet_len: usize,
    packet_from: u8,
    received: bool,
}

impl Rx {
    const fn new() -> Self {
        Self {
            header: [0; 6],
            phase: 0,
            pos: 0,
            len: 0,
            station: 0,
            sender: 0,
            command: 0,
            checksum: 0,
            calculated: 0,
            data: [0; MAX_FRAME_LENGTH],
            packet_len: 0,
            packet_from: 0,
            received: false,
        }
    }

    /// Resets the state machine without dropping an already-received frame.
    fn reset(&mut self) {
        self.phase = 0;
        self.pos = 0;
        self.len = 0;
        self.command = 0;
        self.checksum = 0;
        self.calculated = 0;
    }
}

/// ICSC over a serial port, with a driver-enable pin.
pub struct Rs485Transport<'a, P: Platform, S: SerialPort> {
    port: &'a S,
    gpio: &'a P::Gpio,
    clock: &'a P::Clock,
    /// Driver enable. [`NO_PIN`] for a board that keeps the driver always on.
    de_pin: Pin,
    soh_count: u8,
    node_id: Cell<u8>,
    rx: RefCell<Rx>,
    rng: Cell<u32>,
}

impl<'a, P: Platform, S: SerialPort> Rs485Transport<'a, P, S> {
    /// Wires the transport to a port and a DE pin.
    ///
    /// `soh_count` is MySensors' `MY_RS485_SOH_COUNT`: repeating the start byte
    /// makes it far more likely that a receiver which joined mid-collision still
    /// finds a frame boundary. The GoWired configuration uses 3.
    #[must_use]
    pub fn new(
        port: &'a S,
        gpio: &'a P::Gpio,
        clock: &'a P::Clock,
        de_pin: Pin,
        soh_count: u8,
    ) -> Self {
        Self {
            port,
            gpio,
            clock,
            de_pin,
            soh_count: soh_count.max(1),
            node_id: Cell::new(BROADCAST_ADDRESS),
            rx: RefCell::new(Rx::new()),
            rng: Cell::new(0x1234_5678),
        }
    }

    fn assert_de(&self) {
        if self.de_pin != NO_PIN {
            self.gpio.write(self.de_pin, true);
            self.clock.delay_us(5);
        }
    }

    fn deassert_de(&self) {
        if self.de_pin != NO_PIN {
            self.gpio.write(self.de_pin, false);
        }
    }

    /// Next pseudo-random value. A 32-bit xorshift: three shifts, no division,
    /// and unlike an LCG its low bits are not almost-periodic -- which matters
    /// because the backoff only uses `% 20`.
    fn next_random(&self) -> u32 {
        let mut x = self.rng.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng.set(x);
        x
    }

    /// Drains the port, advancing the receive state machine.
    ///
    /// Returns `true` if any byte was consumed -- which is what the C++ version
    /// reports, and what its collision check keys on. It does *not* mean a frame
    /// completed; ask [`Transport::data_available`] for that.
    #[inline(never)]
    fn process(&self) -> bool {
        if !self.port.available() {
            return false;
        }

        let mut rx = self.rx.borrow_mut();
        let node_id = self.node_id.get();

        while let Some(inch) = self.port.read_byte() {
            match rx.phase {
                // Look for the header. Bytes shift through a six-byte window;
                // when its ends are SOH and STX we have a frame start.
                0 => {
                    rx.header.copy_within(1..6, 0);
                    rx.header[5] = inch;

                    if rx.header[0] == SOH && rx.header[5] == STX && rx.header[1] != rx.header[2] {
                        rx.calculated = 0;
                        rx.station = rx.header[1];
                        rx.sender = rx.header[2];
                        rx.command = rx.header[3];
                        rx.len = rx.header[4] as usize;

                        for i in 1..=4 {
                            rx.calculated = rx.calculated.wrapping_add(rx.header[i]);
                        }
                        rx.phase = 1;
                        rx.pos = 0;

                        // Avoid overrunning the data buffer.
                        if rx.len >= MAX_FRAME_LENGTH {
                            rx.reset();
                            continue;
                        }

                        // Reject our own echo -- on a single pair we hear
                        // ourselves -- and anything addressed elsewhere.
                        if rx.sender == node_id
                            || (rx.station != node_id && rx.station != BROADCAST_ADDRESS)
                        {
                            rx.reset();
                            continue;
                        }

                        if rx.len == 0 {
                            rx.phase = 2;
                        }
                    }
                }

                // The payload.
                1 => {
                    let pos = rx.pos;
                    rx.data[pos] = inch;
                    rx.pos += 1;
                    rx.calculated = rx.calculated.wrapping_add(inch);
                    if rx.pos == rx.len {
                        rx.phase = 2;
                    }
                }

                // A single ETX, or resynchronise.
                2 => {
                    if inch == ETX {
                        rx.phase = 3;
                    } else {
                        rx.reset();
                    }
                }

                // The checksum, to be compared once EOT lands.
                3 => {
                    rx.checksum = inch;
                    rx.phase = 4;
                }

                // EOT, and the verdict.
                _ => {
                    if inch == EOT && rx.checksum == rx.calculated && rx.command == ICSC_SYS_PACK {
                        rx.packet_from = rx.sender;
                        rx.packet_len = rx.len;
                        rx.received = true;
                    }
                    rx.reset();
                    // One frame per call, as in the C++ version: the caller gets
                    // a chance to take it before the next one overwrites it.
                    return true;
                }
            }
        }

        true
    }
}

impl<P: Platform, S: SerialPort> Transport for Rs485Transport<'_, P, S> {
    fn init(&self) -> bool {
        self.rx.borrow_mut().reset();
        if self.de_pin != NO_PIN {
            self.gpio.configure(self.de_pin, PinMode::Output);
            self.gpio.write(self.de_pin, false);
        }
        true
    }

    fn set_address(&self, address: u8) {
        self.node_id.set(address);
        // Reseed from the address so that nodes which collide do not then pick
        // the same backoff. Any non-zero seed will do for xorshift.
        self.rng.set(0x1234_5678 ^ (u32::from(address) << 16 | 1));
    }

    fn address(&self) -> u8 {
        self.node_id.get()
    }

    #[inline(never)]
    fn send(&self, to: u8, data: &[u8]) -> bool {
        // Look for a collision first: if anything has been seen on the bus,
        // wait a random time and check again.
        let mut timeout = 10;
        while self.process() {
            let backoff = self.next_random() % 20;
            for _ in 0..backoff {
                self.clock.delay_ms(1);
                self.process();
            }
            timeout -= 1;
            if timeout == 0 {
                return false; // failed to transmit
            }
        }

        self.assert_de();

        let mut cs: u8 = 0;
        for _ in 0..self.soh_count {
            self.port.write_byte(SOH);
        }
        for byte in [to, self.node_id.get(), ICSC_SYS_PACK, data.len() as u8] {
            self.port.write_byte(byte);
            cs = cs.wrapping_add(byte);
        }
        self.port.write_byte(STX);
        for &byte in data {
            self.port.write_byte(byte);
            cs = cs.wrapping_add(byte);
        }
        self.port.write_byte(ETX);
        self.port.write_byte(cs);
        self.port.write_byte(EOT);

        // Order matters: the last stop bit must be on the wire before the driver
        // is turned around, or the receiver sees a truncated EOT.
        self.port.flush();
        self.deassert_de();
        true
    }

    fn data_available(&self) -> bool {
        self.process();
        self.rx.borrow().received
    }

    fn receive(&self, out: &mut [u8]) -> usize {
        let mut rx = self.rx.borrow_mut();
        if !rx.received {
            return 0;
        }
        let n = rx.packet_len.min(out.len());
        out[..n].copy_from_slice(&rx.data[..n]);
        rx.received = false;
        n
    }
}
