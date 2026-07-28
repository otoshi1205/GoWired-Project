//! MySensors 2.x message encoding.
//!
//! The wire format is the V2 header, seven bytes, followed by up to 25 bytes of
//! payload:
//!
//! ```text
//! byte 0  last            id of the last node this message passed
//! byte 1  sender          id of the originating node
//! byte 2  destination     id of the target node
//! byte 3  version_length  bits 0-1 protocol version (2)
//!                         bit  2   signed
//!                         bits 3-7 payload length
//! byte 4  command_echo_payload
//!                         bits 0-2 command
//!                         bit  3   echo requested
//!                         bit  4   is an echo
//!                         bits 5-7 payload type
//! byte 5  type            V_* for C_SET/C_REQ, S_* for C_PRESENTATION,
//!                         I_* for C_INTERNAL
//! byte 6  sensor          child id
//! byte 7+ payload
//! ```
//!
//! Numbers are little-endian, which on AVR means a `f32` payload is a straight
//! copy of the register pair -- and is why `V_TEMP` costs 5 bytes on the wire
//! rather than the 6 or 7 a decimal string would take.
//!
//! Everything in this file is checked byte-for-byte against frames captured
//! from the C++ library in `tests/message_test.rs`.

/// Total message size, header included.
pub const MAX_MESSAGE_SIZE: usize = 32;

/// V2 header size.
pub const HEADER_SIZE: usize = 7;

/// Largest payload that fits in a message.
pub const MAX_PAYLOAD_SIZE: usize = MAX_MESSAGE_SIZE - HEADER_SIZE;

/// Protocol version carried in every header.
pub const PROTOCOL_VERSION: u8 = 2;

/// The gateway is always node 0.
pub const GATEWAY_ADDRESS: u8 = 0;

/// Destination that every node accepts.
pub const BROADCAST_ADDRESS: u8 = 255;

/// Child id that means "the node itself".
pub const NODE_SENSOR_ID: u8 = 255;

/// Node id meaning "not assigned yet"; ask the controller for one.
pub const AUTO_NODE_ID: u8 = 255;

/// Message class.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Command {
    /// A node announcing a child.
    Presentation = 0,
    /// A value being set.
    Set = 1,
    /// A value being asked for.
    Req = 2,
    /// Library-level housekeeping.
    Internal = 3,
    /// Bulk data: OTA firmware. Not implemented here.
    Stream = 4,
}

impl Command {
    /// Decodes the 3-bit command field.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            0 => Some(Self::Presentation),
            1 => Some(Self::Set),
            2 => Some(Self::Req),
            3 => Some(Self::Internal),
            4 => Some(Self::Stream),
            _ => None,
        }
    }
}

/// How the payload bytes are to be read.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum PayloadType {
    /// ASCII, not NUL-terminated on the wire.
    String = 0,
    /// One unsigned byte.
    Byte = 1,
    /// Little-endian `i16`.
    Int16 = 2,
    /// Little-endian `u16`.
    UInt16 = 3,
    /// Little-endian `i32`.
    Long32 = 4,
    /// Little-endian `u32`.
    ULong32 = 5,
    /// Opaque bytes.
    Custom = 6,
    /// Little-endian `f32` followed by a precision byte.
    Float32 = 7,
}

impl PayloadType {
    /// Decodes the 3-bit payload-type field.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Byte,
            2 => Self::Int16,
            3 => Self::UInt16,
            4 => Self::Long32,
            5 => Self::ULong32,
            6 => Self::Custom,
            7 => Self::Float32,
            _ => Self::String,
        }
    }
}

/// `V_*` variable types, as sent on the wire.
///
/// Only the ones this firmware uses. The numbers are MySensors'.
pub mod v {
    /// `V_TEMP`
    pub const TEMP: u8 = 0;
    /// `V_HUM`
    pub const HUM: u8 = 1;
    /// `V_STATUS`
    pub const STATUS: u8 = 2;
    /// `V_PERCENTAGE`
    pub const PERCENTAGE: u8 = 3;
    /// `V_WATT`
    pub const WATT: u8 = 17;
    /// `V_UP`
    pub const UP: u8 = 29;
    /// `V_DOWN`
    pub const DOWN: u8 = 30;
    /// `V_STOP`
    pub const STOP: u8 = 31;
    /// `V_RGB`
    pub const RGB: u8 = 40;
    /// `V_RGBW`
    pub const RGBW: u8 = 41;
    /// `V_TEXT`
    pub const TEXT: u8 = 47;
}

/// `S_*` sensor types, as sent on the wire.
pub mod s {
    /// `S_BINARY`
    pub const BINARY: u8 = 3;
    /// `S_DIMMER`
    pub const DIMMER: u8 = 4;
    /// `S_COVER`
    pub const COVER: u8 = 5;
    /// `S_TEMP`
    pub const TEMP: u8 = 6;
    /// `S_HUM`
    pub const HUM: u8 = 7;
    /// `S_POWER`
    pub const POWER: u8 = 13;
    /// `S_ARDUINO_NODE`
    pub const ARDUINO_NODE: u8 = 17;
    /// `S_RGB_LIGHT`
    pub const RGB_LIGHT: u8 = 26;
    /// `S_RGBW_LIGHT`
    pub const RGBW_LIGHT: u8 = 27;
    /// `S_INFO`
    pub const INFO: u8 = 36;
}

/// `I_*` internal message types, as sent on the wire.
pub mod i {
    /// `I_TIME`
    pub const TIME: u8 = 1;
    /// `I_VERSION`
    pub const VERSION: u8 = 2;
    /// `I_ID_REQUEST`
    pub const ID_REQUEST: u8 = 3;
    /// `I_ID_RESPONSE`
    pub const ID_RESPONSE: u8 = 4;
    /// `I_CONFIG`
    pub const CONFIG: u8 = 6;
    /// `I_LOG_MESSAGE`
    pub const LOG_MESSAGE: u8 = 9;
    /// `I_SKETCH_NAME`
    pub const SKETCH_NAME: u8 = 11;
    /// `I_SKETCH_VERSION`
    pub const SKETCH_VERSION: u8 = 12;
    /// `I_HEARTBEAT_REQUEST`
    pub const HEARTBEAT_REQUEST: u8 = 18;
    /// `I_PRESENTATION`
    pub const PRESENTATION: u8 = 19;
    /// `I_DISCOVER_REQUEST`
    pub const DISCOVER_REQUEST: u8 = 20;
    /// `I_DISCOVER_RESPONSE`
    pub const DISCOVER_RESPONSE: u8 = 21;
    /// `I_HEARTBEAT_RESPONSE`
    pub const HEARTBEAT_RESPONSE: u8 = 22;
    /// `I_PING`
    pub const PING: u8 = 24;
    /// `I_PONG`
    pub const PONG: u8 = 25;
}

/// One MySensors message, header and payload.
#[derive(Copy, Clone, Debug)]
pub struct Message {
    /// Id of the last node this message passed.
    pub last: u8,
    /// Id of the originating node.
    pub sender: u8,
    /// Id of the target node.
    pub destination: u8,
    /// Message class.
    pub command: Command,
    /// Whether the sender wants the message echoed back.
    pub echo_request: bool,
    /// Whether this message *is* an echo.
    pub echo: bool,
    /// How to read the payload.
    pub payload_type: PayloadType,
    /// `V_*`, `S_*` or `I_*` depending on `command`.
    pub ty: u8,
    /// Child id.
    pub sensor: u8,
    payload: [u8; MAX_PAYLOAD_SIZE],
    length: u8,
}

impl Default for Message {
    fn default() -> Self {
        Self::new()
    }
}

impl Message {
    /// An empty `C_SET` message to the gateway.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: 0,
            sender: 0,
            destination: GATEWAY_ADDRESS,
            command: Command::Set,
            echo_request: false,
            echo: false,
            payload_type: PayloadType::String,
            ty: 0,
            sensor: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
            length: 0,
        }
    }

    /// Starts a message from `sender` to `destination`.
    #[must_use]
    pub const fn build(sender: u8, destination: u8, sensor: u8, command: Command, ty: u8) -> Self {
        Self {
            last: sender,
            sender,
            destination,
            command,
            echo_request: false,
            echo: false,
            payload_type: PayloadType::String,
            ty,
            sensor,
            payload: [0; MAX_PAYLOAD_SIZE],
            length: 0,
        }
    }

    /// Payload length in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length as usize
    }

    /// Whether the payload is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// The raw payload bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.length as usize]
    }

    /// Sets a `bool` payload, as `P_BYTE`. Matches `MyMessage::set(bool)`.
    pub fn set_bool(&mut self, value: bool) -> &mut Self {
        self.set_byte(u8::from(value))
    }

    /// Sets a `u8` payload, as `P_BYTE`.
    pub fn set_byte(&mut self, value: u8) -> &mut Self {
        self.payload_type = PayloadType::Byte;
        self.payload[0] = value;
        self.length = 1;
        self
    }

    /// Sets a `u32` payload, as `P_ULONG32`.
    pub fn set_u32(&mut self, value: u32) -> &mut Self {
        self.payload_type = PayloadType::ULong32;
        self.payload[..4].copy_from_slice(&value.to_le_bytes());
        self.length = 4;
        self
    }

    /// Sets a fixed-point payload as `P_FLOAT32`.
    ///
    /// Five bytes: a little-endian `f32`, then the number of decimals the
    /// controller should display. This is what `MyMessage::set(float, decimals)`
    /// sends -- the value is *not* formatted into a decimal string, which is why
    /// this firmware needs no float-to-text conversion at all.
    ///
    /// `value` is the quantity scaled by `10^decimals`, and the conversion to
    /// `f32` is done with integer arithmetic; see [`crate::proto::fixed`] for why
    /// that is worth 6 kB of flash.
    pub fn set_fixed(&mut self, value: i32, decimals: u8) -> &mut Self {
        self.payload_type = PayloadType::Float32;
        let bits = crate::proto::fixed::to_f32_bits(value, decimals);
        self.payload[..4].copy_from_slice(&bits.to_le_bytes());
        self.payload[4] = decimals;
        self.length = 5;
        self
    }

    /// Sets a raw `f32` bit pattern with a precision byte.
    ///
    /// For tests and for anything that already has the bits. Prefer
    /// [`Message::set_fixed`], which is what the firmware uses.
    pub fn set_f32_bits(&mut self, bits: u32, decimals: u8) -> &mut Self {
        self.payload_type = PayloadType::Float32;
        self.payload[..4].copy_from_slice(&bits.to_le_bytes());
        self.payload[4] = decimals;
        self.length = 5;
        self
    }

    /// Sets a string payload, as `P_STRING`, truncating past 25 bytes.
    pub fn set_str(&mut self, value: &str) -> &mut Self {
        self.payload_type = PayloadType::String;
        let bytes = value.as_bytes();
        let n = bytes.len().min(MAX_PAYLOAD_SIZE);
        self.payload[..n].copy_from_slice(&bytes[..n]);
        self.length = n as u8;
        self
    }

    /// Sets a string payload from a flash literal.
    pub fn set_text(&mut self, value: crate::text::Text) -> &mut Self {
        self.payload_type = PayloadType::String;
        let n = value.copy_to(&mut self.payload);
        self.length = n as u8;
        self
    }

    /// Sets raw payload bytes without changing the payload type.
    pub fn set_raw(&mut self, bytes: &[u8], payload_type: PayloadType) -> &mut Self {
        self.payload_type = payload_type;
        let n = bytes.len().min(MAX_PAYLOAD_SIZE);
        self.payload[..n].copy_from_slice(&bytes[..n]);
        self.length = n as u8;
        self
    }

    // -- payload views ------------------------------------------------------

    /// The payload as a truth value.
    ///
    /// Controllers send `V_STATUS` as the string `"1"` or `"0"`; the library
    /// itself sends it as a byte. Both are accepted.
    #[must_use]
    pub fn as_bool(&self) -> bool {
        self.as_i32() != 0
    }

    /// The payload as an integer.
    ///
    /// Mirrors what the C++ sketch did with `atoi(msg.data)`, extended to the
    /// binary payload types so a library-sent value decodes correctly too.
    #[must_use]
    pub fn as_i32(&self) -> i32 {
        let p = self.payload();
        match self.payload_type {
            PayloadType::String => parse_i32(p),
            PayloadType::Byte => i32::from(p.first().copied().unwrap_or(0)),
            PayloadType::Int16 => i32::from(i16::from_le_bytes([
                p.first().copied().unwrap_or(0),
                p.get(1).copied().unwrap_or(0),
            ])),
            PayloadType::UInt16 => i32::from(u16::from_le_bytes([
                p.first().copied().unwrap_or(0),
                p.get(1).copied().unwrap_or(0),
            ])),
            PayloadType::Long32 | PayloadType::ULong32 => {
                let mut b = [0u8; 4];
                for (i, slot) in b.iter_mut().enumerate() {
                    *slot = p.get(i).copied().unwrap_or(0);
                }
                i32::from_le_bytes(b)
            }
            // Decoded from the bit pattern rather than by converting to `f32`
            // and back, so that a controller sending a float does not drag the
            // soft-float library into the firmware.
            PayloadType::Float32 => crate::proto::fixed::f32_bits_to_i32(self.f32_bits()),
            PayloadType::Custom => 0,
        }
    }

    /// The payload's raw `f32` bits, for `P_FLOAT32` only. Zero otherwise.
    #[must_use]
    pub fn f32_bits(&self) -> u32 {
        if self.payload_type != PayloadType::Float32 || self.length < 4 {
            return 0;
        }
        u32::from_le_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ])
    }

    /// The payload as a float, for `P_FLOAT32` only. Zero otherwise.
    ///
    /// Only for host code -- tests and tooling. The firmware never calls it,
    /// because touching an `f32` value on AVR is what this design avoids.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn as_f32(&self) -> f32 {
        f32::from_bits(self.f32_bits())
    }

    /// The payload as text, for `P_STRING` only. Empty otherwise.
    ///
    /// ASCII only: a payload with the high bit set anywhere yields `""`.
    ///
    /// That is deliberate, and it is worth 256 bytes of SRAM. Every string this
    /// firmware reads is a decimal number, a hex colour or a `cmdN`, all of them
    /// ASCII -- but `core::str::from_utf8` links a 256-byte character-width table
    /// to validate the multi-byte forms, and on a 2 KB part that table is an
    /// eighth of the available RAM. The check below is a loop.
    #[must_use]
    pub fn as_str(&self) -> &str {
        if self.payload_type != PayloadType::String {
            return "";
        }
        let bytes = self.payload();
        if !bytes.is_ascii() {
            return "";
        }
        // SAFETY: `is_ascii` just established that every byte is below 0x80, and
        // any sequence of such bytes is valid UTF-8 by definition.
        #[allow(unsafe_code)]
        unsafe {
            core::str::from_utf8_unchecked(bytes)
        }
    }

    // -- the wire -----------------------------------------------------------

    /// Serialises the message into `out`; returns the number of bytes written.
    ///
    /// Returns 0 if `out` is too small, which cannot happen for a buffer of
    /// [`MAX_MESSAGE_SIZE`].
    pub fn encode(&self, out: &mut [u8]) -> usize {
        let total = HEADER_SIZE + self.length as usize;
        if out.len() < total {
            return 0;
        }

        out[0] = self.last;
        out[1] = self.sender;
        out[2] = self.destination;
        out[3] = PROTOCOL_VERSION | (self.length << 3);
        out[4] = (self.command as u8)
            | (u8::from(self.echo_request) << 3)
            | (u8::from(self.echo) << 4)
            | ((self.payload_type as u8) << 5);
        out[5] = self.ty;
        out[6] = self.sensor;
        out[HEADER_SIZE..total].copy_from_slice(self.payload());
        total
    }

    /// Parses a message out of `bytes`.
    ///
    /// Returns `None` when the frame is too short, carries an unknown command,
    /// or claims a payload longer than it actually has. The signed flag is
    /// preserved in neither direction: message signing is not implemented, so a
    /// signed message is rejected rather than silently trusted.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < HEADER_SIZE {
            return None;
        }

        let vsl = bytes[3];
        if vsl & 0x03 != PROTOCOL_VERSION {
            return None;
        }
        if vsl & 0x04 != 0 {
            return None; // signed; unsupported, so not accepted
        }
        let length = (vsl >> 3) as usize;
        if length > MAX_PAYLOAD_SIZE || bytes.len() < HEADER_SIZE + length {
            return None;
        }

        let cep = bytes[4];
        let command = Command::from_bits(cep & 0x07)?;

        let mut msg = Self {
            last: bytes[0],
            sender: bytes[1],
            destination: bytes[2],
            command,
            echo_request: cep & 0x08 != 0,
            echo: cep & 0x10 != 0,
            payload_type: PayloadType::from_bits(cep >> 5),
            ty: bytes[5],
            sensor: bytes[6],
            payload: [0; MAX_PAYLOAD_SIZE],
            length: length as u8,
        };
        msg.payload[..length].copy_from_slice(&bytes[HEADER_SIZE..HEADER_SIZE + length]);
        Some(msg)
    }
}

/// `atoi`, near enough: leading spaces and sign, then digits until something
/// that is not one.
fn parse_i32(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }

    let mut negative = false;
    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
        negative = bytes[i] == b'-';
        i += 1;
    }

    let mut value: i32 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        // Saturating, so a hostile payload cannot wrap the accumulator into a
        // plausible-looking small number.
        value = value
            .saturating_mul(10)
            .saturating_add(i32::from(bytes[i] - b'0'));
        i += 1;
    }

    if negative {
        -value
    } else {
        value
    }
}
