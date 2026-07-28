//! The node: addressing, housekeeping replies, and the inbound queue.
//!
//! Driven against a scripted gateway: frames are fed in through a fake serial
//! port and the node's answers are decoded back out of it, so these exercise the
//! whole stack from ICSC framing up to the [`crate::hal::Bus`] trait.

use crate::fakes::{FakePlatform, FakeSerial, Rig};
use crate::gw_text;
use crate::hal::{Bus, SensorClass, ValueType};
use crate::proto::message::{i, v, Command, Message, AUTO_NODE_ID, NODE_SENSOR_ID};
use crate::proto::node::EEPROM_NODE_ID_ADDRESS;
use crate::proto::rs485::Rs485Transport;
use crate::proto::{MySensorsBus, Node};

const DE_PIN: u8 = 7;
const SOH: u8 = 1;
const STX: u8 = 2;
const ETX: u8 = 3;
const EOT: u8 = 4;
const SYS_PACK: u8 = 0x58;

/// A node, its transport, and the port both ends see.
struct Bench {
    rig: Rig,
    port: FakeSerial,
}

impl Bench {
    fn new() -> Self {
        Self {
            rig: Rig::new(),
            port: FakeSerial::new(),
        }
    }

    fn transport(&self) -> Rs485Transport<'_, FakePlatform, FakeSerial> {
        Rs485Transport::new(&self.port, &self.rig.gpio, &self.rig.clock, DE_PIN, 3)
    }

    fn node<'a>(
        &'a self,
        transport: &'a Rs485Transport<'a, FakePlatform, FakeSerial>,
        configured_id: u8,
    ) -> Node<'a, FakePlatform, Rs485Transport<'a, FakePlatform, FakeSerial>> {
        Node::new(
            transport,
            &self.rig.clock,
            &self.rig.store,
            configured_id,
            60_000,
        )
    }

    /// Feeds a message in as the gateway would send it.
    fn gateway_sends(&self, to: u8, msg: &Message) {
        let mut payload = [0u8; 32];
        let n = msg.encode(&mut payload);
        self.port.feed(&icsc(to, msg.sender, &payload[..n]));
    }

    /// Every message the node has transmitted, decoded.
    fn transmitted(&self) -> std::vec::Vec<Message> {
        decode_stream(&self.port.sent())
    }
}

fn icsc(dest: u8, sender: u8, payload: &[u8]) -> std::vec::Vec<u8> {
    let mut out = std::vec![SOH, SOH, SOH];
    let mut cs = 0u8;
    for b in [dest, sender, SYS_PACK, payload.len() as u8] {
        out.push(b);
        cs = cs.wrapping_add(b);
    }
    out.push(STX);
    for &b in payload {
        out.push(b);
        cs = cs.wrapping_add(b);
    }
    out.push(ETX);
    out.push(cs);
    out.push(EOT);
    out
}

/// Pulls MySensors messages back out of a stream of ICSC frames.
fn decode_stream(bytes: &[u8]) -> std::vec::Vec<Message> {
    let mut out = std::vec::Vec::new();
    let mut i = 0;
    while i + 6 < bytes.len() {
        if bytes[i] != SOH {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && bytes[j] == SOH {
            j += 1;
        }
        if j + 4 >= bytes.len() {
            break;
        }
        let len = bytes[j + 3] as usize;
        let stx = j + 4;
        if bytes[stx] != STX || stx + 1 + len + 2 >= bytes.len() + 1 {
            i += 1;
            continue;
        }
        let payload = &bytes[stx + 1..stx + 1 + len];
        if let Some(msg) = Message::decode(payload) {
            out.push(msg);
        }
        i = stx + 1 + len + 3; // ETX, cs, EOT
    }
    out
}

fn built_from_gateway(sensor: u8, command: Command, ty: u8) -> Message {
    Message::build(0, 12, sensor, command, ty)
}

// ---------------------------------------------------------------------------
// Addressing
// ---------------------------------------------------------------------------

#[test]
fn a_static_id_is_adopted_without_asking() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);

    assert!(node.begin());
    assert_eq!(node.node_id(), 12);
    assert!(node.ready());
    // Nothing was transmitted: no id request needed.
    assert!(b.port.sent().is_empty());
}

/// A node that was already paired must keep its id across a power cut, or
/// re-pairing becomes a chore after every outage.
#[test]
fn a_remembered_id_is_reused() {
    let b = Bench::new();
    b.rig.store.preset(EEPROM_NODE_ID_ADDRESS, 37);
    let t = b.transport();
    let node = b.node(&t, AUTO_NODE_ID);

    assert!(node.begin());
    assert_eq!(node.node_id(), 37);
    assert!(b.port.sent().is_empty());
}

#[test]
fn an_unassigned_node_asks_the_controller() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, AUTO_NODE_ID);

    // Queue the answer before begin(), so the first wait finds it.
    let mut response = built_from_gateway(NODE_SENSOR_ID, Command::Internal, i::ID_RESPONSE);
    response.set_str("44");
    response.sender = 0;
    b.gateway_sends(AUTO_NODE_ID, &response);

    assert!(node.begin());
    assert_eq!(node.node_id(), 44);
    // And it is remembered.
    assert_eq!(b.rig.store.peek(EEPROM_NODE_ID_ADDRESS), 44);

    let sent = b.transmitted();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].command, Command::Internal);
    assert_eq!(sent[0].ty, i::ID_REQUEST);
    assert_eq!(sent[0].sender, AUTO_NODE_ID);
}

#[test]
fn a_node_with_no_gateway_gives_up_without_hanging() {
    let b = Bench::new();
    let t = b.transport();
    // A one-second patience, so the test does not sit through 60 s of fake time.
    let node = Node::<FakePlatform, _>::new(&t, &b.rig.clock, &b.rig.store, AUTO_NODE_ID, 1_000);

    assert!(!node.begin());
    assert!(!node.ready());
    assert_eq!(node.node_id(), AUTO_NODE_ID);
    // It kept asking rather than asking once and giving up silently.
    assert!(!b.transmitted().is_empty());
}

/// A gateway that answers with 0 or 255 is answering with a reserved address.
#[test]
fn an_out_of_range_id_response_is_refused() {
    for bad in ["0", "255", "-1", "hello"] {
        let b = Bench::new();
        let t = b.transport();
        let node = Node::<FakePlatform, _>::new(&t, &b.rig.clock, &b.rig.store, AUTO_NODE_ID, 500);

        let mut response = built_from_gateway(NODE_SENSOR_ID, Command::Internal, i::ID_RESPONSE);
        response.set_str(bad);
        response.sender = 0;
        b.gateway_sends(AUTO_NODE_ID, &response);

        assert!(!node.begin(), "accepted id {bad:?}");
        assert_eq!(b.rig.store.peek(EEPROM_NODE_ID_ADDRESS), 0xFF);
    }
}

// ---------------------------------------------------------------------------
// Housekeeping
// ---------------------------------------------------------------------------

#[test]
fn a_ping_is_answered_with_a_pong() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    b.port.clear_tx();

    let ping = built_from_gateway(NODE_SENSOR_ID, Command::Internal, i::PING);
    b.gateway_sends(12, &ping);
    node.service();

    let sent = b.transmitted();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].ty, i::PONG);
    assert_eq!(sent[0].destination, 0);
    assert_eq!(sent[0].sender, 12);
}

#[test]
fn a_heartbeat_request_is_answered() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    b.port.clear_tx();

    let beat = built_from_gateway(NODE_SENSOR_ID, Command::Internal, i::HEARTBEAT_REQUEST);
    b.gateway_sends(12, &beat);
    node.service();

    let sent = b.transmitted();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].ty, i::HEARTBEAT_RESPONSE);
}

#[test]
fn a_discover_request_is_answered_with_the_parent() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    b.port.clear_tx();

    let discover = built_from_gateway(NODE_SENSOR_ID, Command::Internal, i::DISCOVER_REQUEST);
    b.gateway_sends(255, &discover);
    node.service();

    let sent = b.transmitted();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].ty, i::DISCOVER_RESPONSE);
    assert_eq!(sent[0].as_i32(), 0); // the gateway is our parent
}

#[test]
fn a_presentation_request_is_reported_to_the_firmware() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();

    assert!(!node.take_presentation_request());

    let req = built_from_gateway(NODE_SENSOR_ID, Command::Internal, i::PRESENTATION);
    b.gateway_sends(12, &req);
    node.service();

    assert!(node.take_presentation_request());
    assert!(!node.take_presentation_request(), "flag was not cleared");
}

/// Housekeeping must not reach the application queue.
#[test]
fn internal_messages_are_not_queued_for_the_application() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();

    for ty in [i::PING, i::TIME, i::VERSION, i::CONFIG, i::LOG_MESSAGE] {
        let msg = built_from_gateway(NODE_SENSOR_ID, Command::Internal, ty);
        b.gateway_sends(12, &msg);
    }
    node.service();

    assert!(node.take_inbound().is_none());
}

#[test]
fn an_echo_request_is_echoed_back_before_being_queued() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    b.port.clear_tx();

    let mut msg = built_from_gateway(0, Command::Set, v::STATUS);
    msg.set_bool(true);
    msg.echo_request = true;
    b.gateway_sends(12, &msg);
    node.service();

    let sent = b.transmitted();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].echo);
    assert!(!sent[0].echo_request);
    assert_eq!(sent[0].destination, 0);
    // And the application still sees it.
    assert!(node.take_inbound().is_some());
}

/// An echo we sent must not itself be echoed -- that is an infinite exchange.
#[test]
fn an_echo_is_not_echoed_again() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    b.port.clear_tx();

    let mut msg = built_from_gateway(0, Command::Set, v::STATUS);
    msg.set_bool(true);
    msg.echo_request = true;
    msg.echo = true;
    b.gateway_sends(12, &msg);
    node.service();

    assert!(b.transmitted().is_empty());
}

/// OTA is not implemented, and a `C_STREAM` frame must be dropped rather than
/// handed to a device handler that would make no sense of it.
#[test]
fn stream_messages_are_dropped() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();

    let msg = built_from_gateway(NODE_SENSOR_ID, Command::Stream, 0);
    b.gateway_sends(12, &msg);
    node.service();

    assert!(node.take_inbound().is_none());
}

// ---------------------------------------------------------------------------
// The inbound queue
// ---------------------------------------------------------------------------

#[test]
fn set_messages_reach_the_application_in_order() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();

    for sensor in [0u8, 1] {
        let mut msg = built_from_gateway(sensor, Command::Set, v::STATUS);
        msg.set_bool(true);
        b.gateway_sends(12, &msg);
    }
    node.service();

    assert_eq!(node.take_inbound().unwrap().sensor, 0);
    assert_eq!(node.take_inbound().unwrap().sensor, 1);
    assert!(node.take_inbound().is_none());
}

#[test]
fn the_queue_drops_rather_than_corrupting_when_full() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();

    for sensor in 0..6u8 {
        let mut msg = built_from_gateway(sensor, Command::Set, v::STATUS);
        msg.set_bool(true);
        b.gateway_sends(12, &msg);
        node.service();
    }

    // The three oldest survive; the rest are counted as overflows.
    assert_eq!(node.take_inbound().unwrap().sensor, 0);
    assert_eq!(node.take_inbound().unwrap().sensor, 1);
    assert_eq!(node.take_inbound().unwrap().sensor, 2);
    assert!(node.take_inbound().is_none());
    assert_eq!(node.inbound_overflows(), 3);
}

// ---------------------------------------------------------------------------
// The Bus implementation
// ---------------------------------------------------------------------------

#[test]
fn bus_sends_are_addressed_to_the_gateway_from_this_node() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);
    b.port.clear_tx();

    bus.send_bool(3, ValueType::Status, true);
    bus.send_uint(0, ValueType::Percentage, 63);
    bus.send_fixed(10, ValueType::Watt, 460, 0);

    let sent = b.transmitted();
    assert_eq!(sent.len(), 3);
    for msg in &sent {
        assert_eq!(msg.sender, 12);
        assert_eq!(msg.destination, 0);
        assert_eq!(msg.command, Command::Set);
    }
    assert_eq!(sent[0].ty, v::STATUS);
    assert!(sent[0].as_bool());
    assert_eq!(sent[1].as_i32(), 63);
    assert!((sent[2].as_f32() - 460.0).abs() < 0.001);
}

#[test]
fn bus_presentation_carries_the_child_description() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);
    b.port.clear_tx();

    assert!(bus.present(0, SensorClass::Cover, gw_text!("Roller Shutter")));

    let sent = b.transmitted();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].command, Command::Presentation);
    assert_eq!(sent[0].ty, crate::proto::message::s::COVER);
    assert_eq!(sent[0].as_str(), "Roller Shutter");
}

#[test]
fn sketch_info_sends_both_halves() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);
    b.port.clear_tx();

    bus.send_sketch_info(gw_text!("GoWired Module"), gw_text!("3.0"));

    let sent = b.transmitted();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].ty, i::SKETCH_NAME);
    assert_eq!(sent[0].as_str(), "GoWired Module");
    assert_eq!(sent[1].ty, i::SKETCH_VERSION);
    assert_eq!(sent[1].as_str(), "3.0");
}

#[test]
fn send_float_to_reaches_another_node() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);
    b.port.clear_tx();

    bus.send_fixed_to(7, 12, ValueType::Temperature, 195, 1);

    let sent = b.transmitted();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].destination, 7);
    assert!((sent[0].as_f32() - 19.5).abs() < 0.001);
}

/// A destination set for one message must not leak into the next.
#[test]
fn a_routed_send_does_not_change_the_default_destination() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);
    b.port.clear_tx();

    bus.send_fixed_to(7, 12, ValueType::Temperature, 195, 1);
    bus.send_bool(0, ValueType::Status, true);

    let sent = b.transmitted();
    assert_eq!(sent[0].destination, 7);
    assert_eq!(sent[1].destination, 0);
}

/// `wait_for_set` exists so the startup handshake is not four full timeouts long.
#[test]
fn wait_for_set_returns_early_when_the_reply_lands() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);

    let mut reply = built_from_gateway(0, Command::Set, v::STATUS);
    reply.set_bool(true);
    b.gateway_sends(12, &reply);

    let before = b.rig.clock.peek();
    bus.wait_for_set(30_000, ValueType::Status);
    let elapsed = b.rig.clock.peek() - before;

    assert!(elapsed < 30_000, "waited the full timeout: {elapsed} ms");
    // The reply is still delivered to the application.
    assert!(bus.take_inbound().is_some());
}

#[test]
fn wait_for_set_gives_up_at_the_timeout() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);

    let before = b.rig.clock.peek();
    bus.wait_for_set(500, ValueType::Status);
    assert!(b.rig.clock.peek() - before >= 500);
}

/// A `C_SET` of the wrong type must not release a wait for a different one.
#[test]
fn wait_for_set_is_specific_to_the_type() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);

    let mut wrong = built_from_gateway(0, Command::Set, v::PERCENTAGE);
    wrong.set_u32(50);
    b.gateway_sends(12, &wrong);

    let before = b.rig.clock.peek();
    bus.wait_for_set(500, ValueType::Status);
    assert!(b.rig.clock.peek() - before >= 500);
}

// ---------------------------------------------------------------------------
// Domain translation
// ---------------------------------------------------------------------------

#[test]
fn inbound_messages_translate_into_the_domain_vocabulary() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);

    let mut msg = built_from_gateway(2, Command::Set, v::PERCENTAGE);
    msg.set_str("63");
    b.gateway_sends(12, &msg);
    bus.node().service();

    let inbound = bus.take_inbound().expect("nothing queued");
    let domain = inbound.as_domain().expect("V_PERCENTAGE is a known type");
    assert_eq!(domain.sensor, 2);
    assert_eq!(domain.ty, ValueType::Percentage);
    assert_eq!(domain.numeric, 63);
    assert_eq!(domain.text, "63");
    assert!(domain.boolean); // 63 is non-zero
}

/// A `V_*` type no child of this node uses has to be dropped, which is where the
/// C++ `decode()` returned false.
#[test]
fn unknown_value_types_do_not_translate() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    let bus = MySensorsBus::new(node);

    let mut msg = built_from_gateway(0, Command::Set, 38); // V_VOLTAGE
    msg.set_str("5");
    b.gateway_sends(12, &msg);
    bus.node().service();

    let inbound = bus.take_inbound().expect("nothing queued");
    assert!(inbound.as_domain().is_none());
}

#[test]
fn node_presentation_announces_the_node_and_asks_for_config() {
    let b = Bench::new();
    let t = b.transport();
    let node = b.node(&t, 12);
    node.begin();
    b.port.clear_tx();

    node.present_node();

    let sent = b.transmitted();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].command, Command::Presentation);
    assert_eq!(sent[0].sensor, NODE_SENSOR_ID);
    assert_eq!(sent[0].ty, crate::proto::message::s::ARDUINO_NODE);
    assert_eq!(sent[1].command, Command::Internal);
    assert_eq!(sent[1].ty, i::CONFIG);
}
