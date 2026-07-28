//! Optional external temperature/humidity probe.
//!
//! The C++ build reached these through third-party Arduino libraries
//! (`arduino-sht` and `DHTlib`), which is why `external_probe.h` was the one file
//! left with a behavioural `#ifdef` in it: selecting a probe meant selecting which
//! header to include, and a header that is not installed cannot be included. Here
//! both drivers are always compiled and a cargo feature picks which one the
//! firmware names, so the conditional is a type alias rather than a preprocessor
//! branch.
//!
//! # Verification status
//!
//! Neither driver has been run against a real sensor. Of the two, the SHT30 path
//! is the one to trust further: I2C is a clocked protocol with an explicit
//! acknowledge at every step and a CRC on the reading, so it either works or
//! returns [`ProbeStatus::ChecksumError`]. The DHT22 path is bit-banged and
//! depends on pulse widths, and its threshold is derived from the datasheet
//! rather than measured -- see [`dht22`].

use gowired_core::hal::{self, ProbeReading, ProbeStatus};

/// The probe this firmware was built with.
#[cfg(feature = "probe-sht30")]
pub type ExternalProbe = sht30::Sht30;
/// The probe this firmware was built with.
#[cfg(all(feature = "probe-dht22", not(feature = "probe-sht30")))]
pub type ExternalProbe = dht22::Dht22;
/// The probe this firmware was built with: none.
#[cfg(not(any(feature = "probe-sht30", feature = "probe-dht22")))]
pub type ExternalProbe = hal::stub::NoHygrometer;

#[cfg(all(feature = "probe-sht30", feature = "probe-dht22"))]
compile_error!("enable at most one of probe-sht30 and probe-dht22");

/// SHT30 over I2C.
pub mod sht30 {
    use super::{hal, ProbeReading, ProbeStatus};
    use crate::clock::delay_ms;
    use crate::regs::{rd, wr, TWBR, TWCR, TWDR, TWSR};

    /// The SHT3x family's default address, with `ADDR` tied low.
    const ADDRESS: u8 = 0x44;

    /// Single shot, medium repeatability, clock stretching disabled.
    ///
    /// Medium is what the C++ build asked `arduino-sht` for. High repeatability
    /// would take 15 ms instead of 6 and buy accuracy this application has no use
    /// for.
    const MEASURE_MEDIUM: [u8; 2] = [0x2C, 0x0D];

    /// `TWINT`: operation complete.
    const TWINT: u8 = 1 << 7;
    /// `TWEA`: acknowledge.
    const TWEA: u8 = 1 << 6;
    /// `TWSTA`: send a start condition.
    const TWSTA: u8 = 1 << 5;
    /// `TWSTO`: send a stop condition.
    const TWSTO: u8 = 1 << 4;
    /// `TWEN`: TWI enable.
    const TWEN: u8 = 1 << 2;

    /// Bit-rate divisor for 100 kHz at 8 MHz: `SCL = F_CPU / (16 + 2 * TWBR)`.
    const BITRATE_100KHZ: u8 = 32;

    /// How many polls to wait for `TWINT` before giving up.
    ///
    /// A bus with no device on it, or one held low by a stuck slave, never sets
    /// `TWINT`. Without a bound this would hang the node until the watchdog reset
    /// it -- every 8 seconds, forever.
    const TWI_TIMEOUT: u16 = 2000;

    fn wait() -> bool {
        for _ in 0..TWI_TIMEOUT {
            // SAFETY: reading TWCR has no side effects.
            if unsafe { rd(TWCR) } & TWINT != 0 {
                return true;
            }
        }
        false
    }

    fn start() -> bool {
        // SAFETY: the documented start sequence.
        unsafe { wr(TWCR, TWINT | TWSTA | TWEN) };
        wait()
    }

    fn stop() {
        // SAFETY: TWSTO clears itself once the stop condition is out.
        unsafe { wr(TWCR, TWINT | TWSTO | TWEN) };
    }

    fn send(byte: u8) -> bool {
        // SAFETY: TWDR may only be written while TWINT is set, which `wait`
        // guarantees for every caller.
        unsafe {
            wr(TWDR, byte);
            wr(TWCR, TWINT | TWEN);
        }
        wait()
    }

    /// Reads one byte, acknowledging it unless it is the last.
    fn receive(last: bool) -> Option<u8> {
        // SAFETY: as `send`.
        unsafe {
            wr(TWCR, if last { TWINT | TWEN } else { TWINT | TWEA | TWEN });
        }
        if !wait() {
            return None;
        }
        // SAFETY: TWDR holds the received byte once TWINT is set.
        Some(unsafe { rd(TWDR) })
    }

    /// Sensirion's CRC-8: polynomial 0x31, initial value 0xFF.
    fn crc8(data: &[u8]) -> u8 {
        let mut crc = 0xFFu8;
        for &byte in data {
            crc ^= byte;
            for _ in 0..8 {
                crc = if crc & 0x80 != 0 {
                    (crc << 1) ^ 0x31
                } else {
                    crc << 1
                };
            }
        }
        crc
    }

    /// An SHT30 on the I2C pins (A4/A5).
    pub struct Sht30;

    impl Sht30 {
        /// Configures the TWI peripheral.
        ///
        /// # Safety
        ///
        /// Call once, at startup.
        pub unsafe fn begin() {
            wr(TWSR, 0); // prescaler 1
            wr(TWBR, BITRATE_100KHZ);
            wr(TWCR, TWEN);
        }

        fn transfer() -> Option<[u8; 6]> {
            if !start() || !send(ADDRESS << 1) {
                stop();
                return None;
            }
            if !send(MEASURE_MEDIUM[0]) || !send(MEASURE_MEDIUM[1]) {
                stop();
                return None;
            }
            stop();

            // Medium repeatability takes 6 ms; 10 leaves margin over the part's
            // spread without making the main loop noticeably longer.
            delay_ms(10);

            if !start() || !send((ADDRESS << 1) | 1) {
                stop();
                return None;
            }

            let mut raw = [0u8; 6];
            for i in 0..raw.len() {
                // The last byte must not be acknowledged, or the sensor keeps
                // holding the bus.
                let Some(byte) = receive(i + 1 == raw.len()) else {
                    stop();
                    return None;
                };
                raw[i] = byte;
            }
            stop();
            Some(raw)
        }
    }

    impl hal::Hygrometer for Sht30 {
        fn read(&self) -> ProbeReading {
            let Some(raw) = Self::transfer() else {
                return ProbeReading {
                    status: ProbeStatus::TimeoutError,
                    ..ProbeReading::default()
                };
            };

            if crc8(&raw[0..2]) != raw[2] || crc8(&raw[3..5]) != raw[5] {
                return ProbeReading {
                    status: ProbeStatus::ChecksumError,
                    ..ProbeReading::default()
                };
            }

            let t_raw = u32::from(u16::from_be_bytes([raw[0], raw[1]]));
            let h_raw = u32::from(u16::from_be_bytes([raw[3], raw[4]]));

            // Datasheet conversions, section 4.13, in tenths:
            //   t_dc = -450 + 1750 * raw / 65535
            //   h_dp =         1000 * raw / 65535
            // Widest intermediate is 1750 * 65535 = 1.15e8, inside u32.
            ProbeReading {
                status: ProbeStatus::Ok,
                temperature_dc: (1750 * t_raw / 65535) as i16 - 450,
                humidity_dp: (1000 * h_raw / 65535) as i16,
            }
        }
    }
}

/// DHT22 (AM2302) on a single data line.
pub mod dht22 {
    use super::{hal, ProbeReading, ProbeStatus};
    use crate::clock::{delay_ms, delay_us};
    use crate::pins::port_of;
    use crate::regs::{clear_bits, rd, set_bits};
    use gowired_core::hal::Pin;

    /// How many polls to wait for an edge before calling it a timeout.
    ///
    /// The longest legitimate pulse is the sensor's 80 microsecond response, and
    /// each iteration of the wait loop is a handful of cycles, so this is roughly
    /// a 300 microsecond ceiling at 8 MHz.
    const EDGE_TIMEOUT: u16 = 400;

    /// A DHT22 on one digital pin.
    pub struct Dht22 {
        pin: Pin,
    }

    impl Dht22 {
        /// Binds the driver to a pin.
        pub const fn new(pin: Pin) -> Self {
            Self { pin }
        }

        fn level(&self) -> bool {
            port_of(self.pin).is_some_and(|p| {
                // SAFETY: reading PINx has no side effects.
                unsafe { rd(p.input) & p.mask() != 0 }
            })
        }

        fn drive_low(&self) {
            if let Some(p) = port_of(self.pin) {
                // SAFETY: `p` came from the pin table.
                unsafe {
                    clear_bits(p.out, p.mask());
                    set_bits(p.ddr, p.mask());
                }
            }
        }

        fn release(&self) {
            if let Some(p) = port_of(self.pin) {
                // SAFETY: as `drive_low`. Input with the pull-up on: the sensor
                // is open-drain and needs the line held high between pulses.
                unsafe {
                    clear_bits(p.ddr, p.mask());
                    set_bits(p.out, p.mask());
                }
            }
        }

        /// Counts loop iterations until the line reaches `target`.
        ///
        /// Iterations rather than microseconds: the count is only ever compared
        /// against another count taken by the same loop, so the units cancel and
        /// nothing depends on how many cycles an iteration actually takes. That is
        /// also how `DHTlib` did it, and it is what makes the driver survive a
        /// compiler that inlines differently than expected.
        fn wait_for(&self, target: bool) -> Option<u16> {
            let mut count = 0u16;
            while self.level() != target {
                count += 1;
                if count >= EDGE_TIMEOUT {
                    return None;
                }
            }
            Some(count)
        }

        fn read_frame(&self) -> Result<[u8; 5], ProbeStatus> {
            // Start: hold the line low for at least 1 ms, then let it go.
            self.drive_low();
            delay_ms(2);
            self.release();
            delay_us(30);

            // The sensor answers with 80 us low, then 80 us high.
            self.wait_for(false).ok_or(ProbeStatus::TimeoutError)?;
            self.wait_for(true).ok_or(ProbeStatus::TimeoutError)?;
            self.wait_for(false).ok_or(ProbeStatus::TimeoutError)?;

            let mut bytes = [0u8; 5];
            for i in 0..40 {
                // Each bit: 50 us low, then 26 us (zero) or 70 us (one) high.
                self.wait_for(true).ok_or(ProbeStatus::TimeoutError)?;
                let high = self.wait_for(false).ok_or(ProbeStatus::TimeoutError)?;
                let low = self.wait_for(true).ok_or(ProbeStatus::TimeoutError)?;

                // Comparing the two halves of the same bit rather than against a
                // fixed threshold: a long high half means a one, whatever the
                // absolute loop speed turns out to be.
                bytes[i / 8] <<= 1;
                if high > low {
                    bytes[i / 8] |= 1;
                }
            }

            let sum = bytes[0]
                .wrapping_add(bytes[1])
                .wrapping_add(bytes[2])
                .wrapping_add(bytes[3]);
            if sum != bytes[4] {
                return Err(ProbeStatus::ChecksumError);
            }
            Ok(bytes)
        }
    }

    impl hal::Hygrometer for Dht22 {
        fn read(&self) -> ProbeReading {
            match self.read_frame() {
                Err(status) => ProbeReading {
                    status,
                    ..ProbeReading::default()
                },
                Ok(bytes) => {
                    // The DHT22 reports in tenths already, which is exactly the
                    // firmware's unit -- no conversion at all.
                    let humidity_dp = i16::from_be_bytes([bytes[0], bytes[1]]) & 0x7FFF;
                    let raw_t = u16::from_be_bytes([bytes[2], bytes[3]]);
                    // Bit 15 is the sign, not part of the magnitude.
                    let magnitude = (raw_t & 0x7FFF) as i16;
                    ProbeReading {
                        status: ProbeStatus::Ok,
                        temperature_dc: if raw_t & 0x8000 != 0 {
                            -magnitude
                        } else {
                            magnitude
                        },
                        humidity_dp,
                    }
                }
            }
        }
    }
}
