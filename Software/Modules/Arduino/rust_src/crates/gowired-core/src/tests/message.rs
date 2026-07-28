//! Message encoding, checked against the C++ library's own output.
//!
//! The `GOLDEN` table below is not hand-derived from the protocol
//! specification. It was produced by compiling MySensors' `MyMessage.cpp` on
//! the host and dumping the packed struct byte by byte -- see
//! `tools/golden/README.md`, which also explains how to regenerate it.
//!
//! This is the test that decides whether a controller that is already paired
//! with a GoWired module keeps working after the rewrite.

use crate::proto::message::{i, s, v, Command, Message, PayloadType, NODE_SENSOR_ID};

/// Bytes the C++ library produces, for a node with id 12 talking to gateway 0.
const GOLDEN: &[(&str, &[u8])] = &[
    ("relay0_status_true", &[0x0C, 0x0C, 0x00, 0x0A, 0x21, 0x02, 0x00, 0x01]),
    ("relay1_status_false", &[0x0C, 0x0C, 0x00, 0x0A, 0x21, 0x02, 0x01, 0x00]),
    ("percentage_63", &[0x0C, 0x0C, 0x00, 0x22, 0xA1, 0x03, 0x00, 0x3F, 0x00, 0x00, 0x00]),
    ("watt_460", &[0x0C, 0x0C, 0x00, 0x2A, 0xE1, 0x11, 0x0A, 0x00, 0x00, 0xE6, 0x43, 0x00]),
    ("temp_21_5", &[0x0C, 0x0C, 0x00, 0x2A, 0xE1, 0x00, 0x0C, 0x00, 0x00, 0xAC, 0x41, 0x01]),
    (
        "present_relay1",
        &[0x0C, 0x0C, 0x00, 0x3A, 0x00, 0x03, 0x00, 0x52, 0x65, 0x6C, 0x61, 0x79, 0x20, 0x31],
    ),
    (
        "present_cover",
        &[
            0x0C, 0x0C, 0x00, 0x72, 0x00, 0x05, 0x00, 0x52, 0x6F, 0x6C, 0x6C, 0x65, 0x72, 0x20,
            0x53, 0x68, 0x75, 0x74, 0x74, 0x65, 0x72,
        ],
    ),
    (
        "sketch_name",
        &[
            0x0C, 0x0C, 0x00, 0x72, 0x03, 0x0B, 0xFF, 0x47, 0x6F, 0x57, 0x69, 0x72, 0x65, 0x64,
            0x20, 0x4D, 0x6F, 0x64, 0x75, 0x6C, 0x65,
        ],
    ),
    ("sketch_version", &[0x0C, 0x0C, 0x00, 0x1A, 0x03, 0x0C, 0xFF, 0x33, 0x2E, 0x30]),
    ("request_status", &[0x0C, 0x0C, 0x00, 0x02, 0x02, 0x02, 0x00]),
    (
        "rgb_ffffff",
        &[0x0C, 0x0C, 0x00, 0x32, 0x01, 0x28, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66],
    ),
    ("id_request", &[0xFF, 0xFF, 0x00, 0x02, 0x03, 0x03, 0xFF]),
    (
        "node_presentation",
        &[0x0C, 0x0C, 0x00, 0x2A, 0x00, 0x11, 0xFF, 0x32, 0x2E, 0x34, 0x2E, 0x30],
    ),
    (
        "config_text",
        &[
            0x0C, 0x0C, 0x00, 0x5A, 0x01, 0x2F, 0x14, 0x43, 0x4F, 0x4E, 0x46, 0x49, 0x47, 0x20,
            0x49, 0x4E, 0x49, 0x54,
        ],
    ),
    ("heartbeat_response", &[0x0C, 0x0C, 0x00, 0x22, 0xA3, 0x16, 0xFF, 0x40, 0xE2, 0x01, 0x00]),
    ("pong", &[0x0C, 0x0C, 0x00, 0x0A, 0x23, 0x19, 0xFF, 0x01]),
];

fn golden(name: &str) -> &'static [u8] {
    GOLDEN
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, bytes)| *bytes)
        .expect("unknown fixture")
}

fn encode(msg: &Message) -> std::vec::Vec<u8> {
    let mut buf = [0u8; 64];
    let n = msg.encode(&mut buf);
    buf[..n].to_vec()
}

fn assert_matches(name: &str, msg: &Message) {
    assert_eq!(
        encode(msg),
        golden(name),
        "{name} does not match the C++ library"
    );
}

const NODE: u8 = 12;
const GW: u8 = 0;

#[test]
fn bool_payloads_match_the_library() {
    let mut m = Message::build(NODE, GW, 0, Command::Set, v::STATUS);
    m.set_bool(true);
    assert_matches("relay0_status_true", &m);

    let mut m = Message::build(NODE, GW, 1, Command::Set, v::STATUS);
    m.set_bool(false);
    assert_matches("relay1_status_false", &m);
}

#[test]
fn u32_payloads_match_the_library() {
    let mut m = Message::build(NODE, GW, 0, Command::Set, v::PERCENTAGE);
    m.set_u32(63);
    assert_matches("percentage_63", &m);
}

/// The library sends floats as a binary `f32` plus a precision byte, not as a
/// formatted decimal string. That is why this firmware needs no float-to-text
/// conversion at all -- which on AVR is worth well over a kilobyte.
#[test]
fn float_payloads_match_the_library() {
    let mut m = Message::build(NODE, GW, 10, Command::Set, v::WATT);
    m.set_fixed(460, 0);
    assert_matches("watt_460", &m);

    let mut m = Message::build(NODE, GW, 12, Command::Set, v::TEMP);
    m.set_fixed(215, 1);
    assert_matches("temp_21_5", &m);
}

#[test]
fn presentations_match_the_library() {
    let mut m = Message::build(NODE, GW, 0, Command::Presentation, s::BINARY);
    m.set_str("Relay 1");
    assert_matches("present_relay1", &m);

    let mut m = Message::build(NODE, GW, 0, Command::Presentation, s::COVER);
    m.set_str("Roller Shutter");
    assert_matches("present_cover", &m);

    let mut m = Message::build(NODE, GW, NODE_SENSOR_ID, Command::Presentation, s::ARDUINO_NODE);
    m.set_str("2.4.0");
    assert_matches("node_presentation", &m);
}

#[test]
fn internal_messages_match_the_library() {
    let mut m = Message::build(NODE, GW, NODE_SENSOR_ID, Command::Internal, i::SKETCH_NAME);
    m.set_str("GoWired Module");
    assert_matches("sketch_name", &m);

    let mut m = Message::build(NODE, GW, NODE_SENSOR_ID, Command::Internal, i::SKETCH_VERSION);
    m.set_str("3.0");
    assert_matches("sketch_version", &m);

    let mut m = Message::build(
        NODE,
        GW,
        NODE_SENSOR_ID,
        Command::Internal,
        i::HEARTBEAT_RESPONSE,
    );
    m.set_u32(123_456);
    assert_matches("heartbeat_response", &m);

    let mut m = Message::build(NODE, GW, NODE_SENSOR_ID, Command::Internal, i::PONG);
    m.set_byte(1);
    assert_matches("pong", &m);
}

/// The node id request goes out with sender 255, because we have no id yet.
#[test]
fn id_request_matches_the_library() {
    let mut m = Message::build(255, GW, NODE_SENSOR_ID, Command::Internal, i::ID_REQUEST);
    m.set_str("");
    assert_matches("id_request", &m);
}

#[test]
fn requests_and_text_match_the_library() {
    let mut m = Message::build(NODE, GW, 0, Command::Req, v::STATUS);
    m.set_str("");
    assert_matches("request_status", &m);

    let mut m = Message::build(NODE, GW, 0, Command::Set, v::RGB);
    m.set_str("ffffff");
    assert_matches("rgb_ffffff", &m);

    let mut m = Message::build(NODE, GW, 20, Command::Set, v::TEXT);
    m.set_str("CONFIG INIT");
    assert_matches("config_text", &m);
}

/// A literal that lives in flash has to encode identically to one in RAM.
#[test]
fn flash_literals_encode_like_ram_strings() {
    let mut m = Message::build(NODE, GW, 0, Command::Presentation, s::BINARY);
    m.set_text(crate::gw_text!("Relay 1"));
    assert_matches("present_relay1", &m);
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

#[test]
fn every_golden_frame_round_trips() {
    for (name, bytes) in GOLDEN {
        let msg = Message::decode(bytes).unwrap_or_else(|| panic!("{name} failed to decode"));
        assert_eq!(&encode(&msg), bytes, "{name} did not round-trip");
    }
}

#[test]
fn decode_recovers_the_header_fields() {
    let m = Message::decode(golden("watt_460")).unwrap();
    assert_eq!(m.sender, 12);
    assert_eq!(m.destination, 0);
    assert_eq!(m.sensor, 10);
    assert_eq!(m.ty, v::WATT);
    assert_eq!(m.command, Command::Set);
    assert_eq!(m.payload_type, PayloadType::Float32);
    assert!(!m.echo);
    assert!(!m.echo_request);
    assert_eq!(m.len(), 5);
    assert!((m.as_f32() - 460.0).abs() < 0.001);
}

/// Controllers send `V_STATUS` as the string "1" or "0"; the library sends it as
/// a byte. Both have to decode to the same truth value.
#[test]
fn status_decodes_from_both_a_byte_and_a_string() {
    let mut as_byte = Message::build(0, 12, 0, Command::Set, v::STATUS);
    as_byte.set_bool(true);
    assert!(as_byte.as_bool());
    assert_eq!(as_byte.as_i32(), 1);

    let mut as_string = Message::build(0, 12, 0, Command::Set, v::STATUS);
    as_string.set_str("1");
    assert!(as_string.as_bool());
    assert_eq!(as_string.as_i32(), 1);

    as_string.set_str("0");
    assert!(!as_string.as_bool());
}

#[test]
fn percentage_decodes_from_a_string_like_a_controller_sends() {
    let mut m = Message::build(0, 12, 0, Command::Set, v::PERCENTAGE);
    m.set_str("63");
    assert_eq!(m.as_i32(), 63);
    assert_eq!(m.as_str(), "63");
}

#[test]
fn integer_parsing_handles_signs_and_junk() {
    let cases: &[(&str, i32)] = &[
        ("0", 0),
        ("42", 42),
        ("-17", -17),
        ("+5", 5),
        ("  7", 7),
        ("100abc", 100),
        ("abc", 0),
        ("", 0),
        ("-", 0),
    ];
    for (payload, expected) in cases {
        let mut m = Message::build(0, 12, 0, Command::Set, v::PERCENTAGE);
        m.set_str(payload);
        assert_eq!(m.as_i32(), *expected, "payload {payload:?}");
    }
}

/// A hostile payload must not wrap the accumulator into a plausible small
/// number -- "4294967297" reading back as 1 would be a way to bypass a clamp.
#[test]
fn integer_parsing_saturates_on_overflow() {
    let mut m = Message::build(0, 12, 0, Command::Set, v::PERCENTAGE);
    m.set_str("999999999999999");
    assert_eq!(m.as_i32(), i32::MAX);
}

#[test]
fn short_frames_are_rejected() {
    for len in 0..7 {
        assert!(
            Message::decode(&golden("watt_460")[..len]).is_none(),
            "accepted a {len}-byte frame"
        );
    }
}

#[test]
fn truncated_payload_is_rejected() {
    let full = golden("present_cover");
    // The header claims 14 payload bytes; hand it 13.
    assert!(Message::decode(&full[..full.len() - 1]).is_none());
}

#[test]
fn wrong_protocol_version_is_rejected() {
    let mut bytes = golden("relay0_status_true").to_vec();
    bytes[3] = (bytes[3] & !0x03) | 1; // claim version 1
    assert!(Message::decode(&bytes).is_none());
}

/// Signing is not implemented. Accepting a message that claims to be signed
/// would mean trusting a signature nothing ever checked.
#[test]
fn signed_frames_are_rejected_rather_than_trusted() {
    let mut bytes = golden("relay0_status_true").to_vec();
    bytes[3] |= 0x04; // set the signed flag
    assert!(Message::decode(&bytes).is_none());
}

#[test]
fn unknown_command_is_rejected() {
    let mut bytes = golden("relay0_status_true").to_vec();
    bytes[4] = (bytes[4] & !0x07) | 7; // command 7 does not exist
    assert!(Message::decode(&bytes).is_none());
}

#[test]
fn over_long_strings_are_truncated_not_overflowed() {
    let mut m = Message::build(NODE, GW, 0, Command::Set, v::TEXT);
    m.set_str("0123456789012345678901234567890123456789");
    assert_eq!(m.len(), 25);
    assert_eq!(m.as_str(), "0123456789012345678901234");
}

/// Non-ASCII text is rejected rather than validated, which is what keeps
/// `core::str::from_utf8`'s 256-byte table out of SRAM. Nothing this firmware
/// reads is non-ASCII, so the cost of being strict is nil.
#[test]
fn non_ascii_text_reads_as_empty() {
    let mut m = Message::build(NODE, GW, 20, Command::Set, v::TEXT);
    m.set_raw(&[0x63, 0x6D, 0x64, 0xC3, 0xA9], PayloadType::String);
    assert_eq!(m.as_str(), "");

    // And plain ASCII still works.
    m.set_str("cmd1");
    assert_eq!(m.as_str(), "cmd1");
}

#[test]
fn payload_views_return_nothing_for_the_wrong_type() {
    let mut m = Message::build(NODE, GW, 0, Command::Set, v::WATT);
    m.set_fixed(15, 1);
    assert_eq!(m.as_str(), ""); // not a string
    assert!((m.as_f32() - 1.5).abs() < 0.001);

    m.set_str("hello");
    assert_eq!(m.f32_bits(), 0); // not a float
}

#[test]
fn echo_flags_survive_a_round_trip() {
    let mut m = Message::build(NODE, GW, 0, Command::Set, v::STATUS);
    m.set_bool(true);
    m.echo_request = true;
    let decoded = Message::decode(&encode(&m)).unwrap();
    assert!(decoded.echo_request);
    assert!(!decoded.echo);

    m.echo_request = false;
    m.echo = true;
    let decoded = Message::decode(&encode(&m)).unwrap();
    assert!(decoded.echo);
    assert!(!decoded.echo_request);
}
