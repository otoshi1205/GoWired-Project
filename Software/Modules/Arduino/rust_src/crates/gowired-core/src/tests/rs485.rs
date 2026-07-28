//! ICSC framing over the RS485 link.

use crate::fakes::{FakePlatform, FakeSerial, Rig};
use crate::proto::rs485::{Rs485Transport, Transport, MAX_FRAME_LENGTH};

const SOH: u8 = 1;
const STX: u8 = 2;
const ETX: u8 = 3;
const EOT: u8 = 4;
const SYS_PACK: u8 = 0x58;

const DE_PIN: u8 = 7;
const NODE: u8 = 12;

struct Link {
    rig: Rig,
    port: FakeSerial,
}

impl Link {
    fn new() -> Self {
        Self {
            rig: Rig::new(),
            port: FakeSerial::new(),
        }
    }

    fn transport(&self) -> Rs485Transport<'_, FakePlatform, FakeSerial> {
        let t = Rs485Transport::new(&self.port, &self.rig.gpio, &self.rig.clock, DE_PIN, 3);
        t.init();
        t.set_address(NODE);
        t
    }
}

/// Builds a well-formed frame, the way another node would.
fn frame(dest: u8, sender: u8, payload: &[u8]) -> std::vec::Vec<u8> {
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

#[test]
fn init_drives_de_low_as_an_output() {
    let link = Link::new();
    let _t = link.transport();
    assert_eq!(
        link.rig.gpio.mode_of(DE_PIN),
        Some(crate::hal::PinMode::Output)
    );
    assert!(!link.rig.gpio.level_of(DE_PIN));
}

#[test]
fn sending_produces_the_icsc_frame() {
    let link = Link::new();
    let t = link.transport();
    assert!(t.send(0, &[0xAA, 0xBB]));

    assert_eq!(link.port.sent(), frame(0, NODE, &[0xAA, 0xBB]));
}

/// `MY_RS485_SOH_COUNT`: repeating the start byte is how a receiver that joined
/// mid-collision still finds a frame boundary.
#[test]
fn soh_count_is_honoured() {
    let link = Link::new();
    let t = Rs485Transport::<FakePlatform, _>::new(
        &link.port,
        &link.rig.gpio,
        &link.rig.clock,
        DE_PIN,
        1,
    );
    t.init();
    t.set_address(NODE);
    t.send(0, &[0x01]);

    assert_eq!(link.port.sent()[0], SOH);
    assert_eq!(link.port.sent()[1], 0); // destination, so only one SOH
}

/// Dropping DE before the stop bit is out truncates the last character, which
/// makes every frame this node sends fail its checksum at the far end.
#[test]
fn de_is_released_only_after_the_last_byte_is_flushed() {
    let link = Link::new();
    let t = link.transport();
    link.rig.gpio.clear_writes();

    t.send(0, &[0x42]);

    let de_writes = link.rig.gpio.writes_to(DE_PIN);
    assert_eq!(de_writes, std::vec![true, false], "DE was not cycled once");
    assert_eq!(link.port.flushes.get(), 1);
    // Everything queued was flushed, i.e. flush() came last.
    assert_eq!(link.port.flushed_at.get(), link.port.sent().len());
}

#[test]
fn a_frame_addressed_to_us_is_received() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&frame(NODE, 0, &[1, 2, 3]));

    assert!(t.data_available());
    let mut buf = [0u8; MAX_FRAME_LENGTH];
    assert_eq!(t.receive(&mut buf), 3);
    assert_eq!(&buf[..3], &[1, 2, 3]);

    // Taken once only.
    assert_eq!(t.receive(&mut buf), 0);
}

#[test]
fn a_broadcast_frame_is_received() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&frame(255, 0, &[9]));

    assert!(t.data_available());
    let mut buf = [0u8; MAX_FRAME_LENGTH];
    assert_eq!(t.receive(&mut buf), 1);
}

#[test]
fn a_frame_for_another_node_is_ignored() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&frame(99, 0, &[1, 2, 3]));
    assert!(!t.data_available());
}

/// RS485 is one pair: a node hears its own transmissions.
#[test]
fn our_own_echo_is_ignored() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&frame(0, NODE, &[1, 2, 3]));
    assert!(!t.data_available());
}

#[test]
fn a_bad_checksum_is_rejected() {
    let link = Link::new();
    let t = link.transport();
    let mut bad = frame(NODE, 0, &[1, 2, 3]);
    let cs = bad.len() - 2;
    bad[cs] = bad[cs].wrapping_add(1);
    link.port.feed(&bad);

    assert!(!t.data_available());
}

#[test]
fn a_missing_eot_is_rejected() {
    let link = Link::new();
    let t = link.transport();
    let mut bad = frame(NODE, 0, &[7]);
    *bad.last_mut().unwrap() = 0xFF;
    link.port.feed(&bad);

    assert!(!t.data_available());
}

#[test]
fn a_missing_etx_is_rejected() {
    let link = Link::new();
    let t = link.transport();
    let mut bad = frame(NODE, 0, &[7]);
    let etx = bad.len() - 3;
    bad[etx] = 0xFF;
    link.port.feed(&bad);

    assert!(!t.data_available());
}

/// A length byte past the buffer size must be dropped, not trusted.
#[test]
fn an_overlong_length_is_rejected() {
    let link = Link::new();
    let t = link.transport();
    let mut bad = std::vec![SOH, SOH, SOH, NODE, 0, SYS_PACK, MAX_FRAME_LENGTH as u8, STX];
    bad.extend(core::iter::repeat_n(0xAA, MAX_FRAME_LENGTH));
    bad.push(ETX);
    bad.push(0);
    bad.push(EOT);
    link.port.feed(&bad);

    assert!(!t.data_available());
}

/// A frame with a zero-length payload skips the data phase entirely.
#[test]
fn an_empty_payload_is_a_valid_frame() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&frame(NODE, 0, &[]));

    assert!(t.data_available());
    let mut buf = [0u8; MAX_FRAME_LENGTH];
    assert_eq!(t.receive(&mut buf), 0);
}

/// The header window is what lets a node resynchronise after a collision: junk
/// ahead of a good frame must not lose it.
#[test]
fn leading_junk_is_resynchronised_past() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&[0xFF, 0x00, SOH, 0x12, 0x34]); // truncated nonsense
    link.port.feed(&frame(NODE, 0, &[5, 6]));

    assert!(t.data_available());
    let mut buf = [0u8; MAX_FRAME_LENGTH];
    assert_eq!(t.receive(&mut buf), 2);
    assert_eq!(&buf[..2], &[5, 6]);
}

/// A frame whose destination equals its sender is malformed: the C++ receiver
/// uses that as its header-sync sanity check, so this firmware must too or the
/// two disagree about where a frame starts.
#[test]
fn a_frame_whose_sender_is_its_destination_is_not_a_header() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&frame(NODE, NODE, &[1]));
    assert!(!t.data_available());
}

#[test]
fn two_frames_back_to_back_are_both_delivered() {
    let link = Link::new();
    let t = link.transport();
    let mut both = frame(NODE, 0, &[1]);
    both.extend(frame(NODE, 0, &[2]));
    link.port.feed(&both);

    let mut buf = [0u8; MAX_FRAME_LENGTH];
    assert!(t.data_available());
    assert_eq!(t.receive(&mut buf), 1);
    assert_eq!(buf[0], 1);

    assert!(t.data_available());
    assert_eq!(t.receive(&mut buf), 1);
    assert_eq!(buf[0], 2);
}

/// The transmit path backs off while the bus is busy, and gives up rather than
/// blocking forever.
#[test]
fn a_permanently_busy_bus_fails_the_send() {
    let link = Link::new();
    let t = link.transport();

    // A finite queue always drains, and then the bus is quiet by definition, so
    // the port is jammed instead: it reports traffic forever.
    link.port.jam();

    assert!(!t.send(0, &[1]));
    // Nothing was transmitted into the collision.
    assert!(link.port.sent().is_empty());
    // And it backed off rather than spinning.
    assert!(!link.rig.clock.delays.borrow().is_empty());
}

/// Traffic that stops must not leave the transmitter permanently convinced the
/// bus is busy.
#[test]
fn a_send_succeeds_once_the_bus_goes_quiet() {
    let link = Link::new();
    let t = link.transport();
    link.port.feed(&std::vec![0xAA; 32]); // junk, then silence

    assert!(t.send(0, &[1]));
    assert_eq!(link.port.sent(), frame(0, NODE, &[1]));
}

#[test]
fn address_changes_take_effect_immediately() {
    let link = Link::new();
    let t = link.transport();
    assert_eq!(t.address(), NODE);

    t.set_address(33);
    assert_eq!(t.address(), 33);

    // Now addressed to 33, so a frame for 12 is somebody else's.
    link.port.feed(&frame(NODE, 0, &[1]));
    assert!(!t.data_available());

    link.port.feed(&frame(33, 0, &[1]));
    assert!(t.data_available());
}

/// A board that keeps its driver permanently enabled has no DE pin to drive.
#[test]
fn a_transport_without_a_de_pin_touches_no_pins() {
    let link = Link::new();
    let t = Rs485Transport::<FakePlatform, _>::new(
        &link.port,
        &link.rig.gpio,
        &link.rig.clock,
        crate::hal::NO_PIN,
        3,
    );
    t.init();
    t.set_address(NODE);
    link.rig.gpio.clear_writes();

    t.send(0, &[1]);
    assert!(link.rig.gpio.writes.borrow().is_empty());
    assert_eq!(link.port.sent(), frame(0, NODE, &[1]));
}
