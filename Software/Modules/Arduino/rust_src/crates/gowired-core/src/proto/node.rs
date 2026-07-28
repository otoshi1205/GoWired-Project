//! The MySensors node: addressing, housekeeping, and the inbound queue.
//!
//! # Reentrancy, and why messages are queued
//!
//! The C++ library calls the sketch's `receive()` from inside `wait()`, which is
//! itself called from inside `presentation()` and from the middle of the main
//! loop. That is a reentrant call into application state, and it is only not a
//! bug there because C++ let it happen quietly.
//!
//! Rust will not: the module cannot be `&mut` borrowed while a call it made is
//! still on the stack. So inbound application messages are pushed onto a short
//! queue instead, and the firmware drains it at the top of each loop pass:
//!
//! ```text
//! loop {
//!     while let Some(msg) = bus.take_inbound() { node.on_message(&msg); }
//!     node.loop_once();
//! }
//! ```
//!
//! The one behavioural consequence is in the startup handshake. `send_initial_state`
//! does `send`, `request`, `wait_for_set` per child so the controller can push
//! the pre-reboot state back; with a queue those replies are applied at the top
//! of the next loop pass rather than inside the wait. The end state is the same
//! -- the outputs still adopt the echoed values, a few milliseconds later --
//! and `wait_for_set` still returns early when the reply lands, so the handshake
//! is no slower.
//!
//! # What is implemented
//!
//! Enough for a leaf node on a wired bus: addressing (static or controller
//! assigned), presentation, `C_SET`/`C_REQ` in both directions, echo requests,
//! and the `I_PING`/`I_HEARTBEAT`/`I_DISCOVER` housekeeping a controller expects
//! an answer to.
//!
//! # What is not
//!
//! - **Message signing** (`MY_SIGNING_*`). A signed inbound message is rejected
//!   rather than trusted; see [`super::message::Message::decode`].
//! - **OTA firmware update** (`MY_OTA_FIRMWARE_FEATURE`, `C_STREAM`). The C++
//!   build had this enabled. It needs DualOptiboot and the firmware-block
//!   protocol, and a half-working implementation of it bricks nodes, so it is
//!   absent rather than approximate. Flashing is over ISP either way.
//! - **Routing and repeating.** RS485 is a single collision domain: every node
//!   hears every other, so there is no parent to find and nothing to relay.

use core::cell::{Cell, RefCell};

use crate::hal::{Clock, Platform, Store};
use crate::proto::message::{
    i, s, Command, Message, AUTO_NODE_ID, GATEWAY_ADDRESS, MAX_MESSAGE_SIZE, NODE_SENSOR_ID,
};
use crate::proto::rs485::Transport;
use crate::text::Text;

/// EEPROM cell MySensors keeps the assigned node id in.
///
/// Address 0, same as the C++ library, so a node that was already paired keeps
/// its id across the switch to this firmware.
pub const EEPROM_NODE_ID_ADDRESS: u16 = 0;

/// How many inbound application messages can be waiting at once.
///
/// Three is one more than the deepest burst this firmware can provoke (a
/// `request` answered while a previous reply is still queued). Each slot is a
/// whole [`Message`], so this is ~100 bytes of SRAM -- the largest single
/// allocation in the firmware, and the reason it is 3 and not 8.
pub const INBOUND_QUEUE_LEN: usize = 3;

/// Library version reported when presenting the node itself.
const LIBRARY_VERSION: &str = "2.4.0";

/// Retry period while asking the controller for a node id.
const ID_REQUEST_RETRY_MS: u32 = 2000;

/// How long to wait for the controller's `I_CONFIG` reply during presentation.
const CONFIG_REPLY_TIMEOUT_MS: u32 = 2000;

/// A short FIFO of whole messages.
struct Queue {
    slots: [Message; INBOUND_QUEUE_LEN],
    head: usize,
    len: usize,
    /// Messages dropped because the queue was full. Never resets; a non-zero
    /// value here means the main loop is not draining fast enough.
    overflows: u16,
}

impl Queue {
    const fn new() -> Self {
        Self {
            slots: [Message::new(); INBOUND_QUEUE_LEN],
            head: 0,
            len: 0,
            overflows: 0,
        }
    }

    fn push(&mut self, msg: Message) {
        if self.len == INBOUND_QUEUE_LEN {
            // Drop the newest rather than the oldest: the oldest is the one the
            // controller has been waiting longest for an effect from, and for
            // V_STATUS-style commands a stale value applied late is worse than a
            // repeat the controller will re-send.
            self.overflows = self.overflows.saturating_add(1);
            return;
        }
        let tail = (self.head + self.len) % INBOUND_QUEUE_LEN;
        self.slots[tail] = msg;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<Message> {
        if self.len == 0 {
            return None;
        }
        let msg = self.slots[self.head];
        self.head = (self.head + 1) % INBOUND_QUEUE_LEN;
        self.len -= 1;
        Some(msg)
    }
}

/// A MySensors leaf node.
pub struct Node<'a, P: Platform, T: Transport> {
    transport: &'a T,
    clock: &'a P::Clock,
    store: &'a P::Store,

    /// What the firmware asked for: a fixed id, or [`AUTO_NODE_ID`].
    configured_id: u8,
    address: Cell<u8>,
    /// How long to keep trying to reach the gateway at startup.
    wait_ready_ms: u32,

    inbound: RefCell<Queue>,
    /// Set when a `C_SET` of the awaited type arrives, so `wait_for_set` can
    /// return early.
    awaited: Cell<Option<(Command, u8)>>,
    awaited_seen: Cell<bool>,
    /// The controller asked us to present ourselves again.
    presentation_requested: Cell<bool>,
}

impl<'a, P: Platform, T: Transport> Node<'a, P, T> {
    /// Builds a node over a transport.
    ///
    /// `configured_id` is [`AUTO_NODE_ID`] to have the controller assign one.
    pub fn new(
        transport: &'a T,
        clock: &'a P::Clock,
        store: &'a P::Store,
        configured_id: u8,
        wait_ready_ms: u32,
    ) -> Self {
        Self {
            transport,
            clock,
            store,
            configured_id,
            address: Cell::new(AUTO_NODE_ID),
            wait_ready_ms,
            inbound: RefCell::new(Queue::new()),
            awaited: Cell::new(None),
            awaited_seen: Cell::new(false),
            presentation_requested: Cell::new(false),
        }
    }

    /// The id this node answers to, or [`AUTO_NODE_ID`] if it has none yet.
    pub fn node_id(&self) -> u8 {
        self.address.get()
    }

    /// Whether the node has an address and can be talked to.
    pub fn ready(&self) -> bool {
        self.address.get() != AUTO_NODE_ID
    }

    /// Whether the controller has asked for a fresh presentation.
    ///
    /// Clears the flag.
    pub fn take_presentation_request(&self) -> bool {
        self.presentation_requested.replace(false)
    }

    /// Messages dropped because the inbound queue was full.
    pub fn inbound_overflows(&self) -> u16 {
        self.inbound.borrow().overflows
    }

    fn set_node_id(&self, id: u8) {
        self.address.set(id);
        self.transport.set_address(id);
    }

    /// Brings the link up and settles on an address.
    ///
    /// Returns whether the node ended up addressable. A `false` return is not
    /// fatal: [`Node::service`] keeps retrying, so a node that boots before its
    /// gateway recovers on its own.
    pub fn begin(&self) -> bool {
        self.transport.init();

        if self.configured_id != AUTO_NODE_ID {
            self.set_node_id(self.configured_id);
            return true;
        }

        // A previously assigned id is remembered, so re-pairing is not needed
        // after every power cut.
        let stored = self.store.read(EEPROM_NODE_ID_ADDRESS);
        if stored != 0xFF && stored != GATEWAY_ADDRESS {
            self.set_node_id(stored);
            return true;
        }

        self.request_node_id()
    }

    /// Asks the controller for an address, retrying until `wait_ready_ms`.
    pub fn request_node_id(&self) -> bool {
        self.request_node_id_for(self.wait_ready_ms)
    }

    /// Asks the controller for an address, giving up after `patience_ms`.
    ///
    /// The bounded form exists for the main loop: a node that booted before its
    /// gateway has to keep asking, but the watchdog is armed by then, so it
    /// cannot sit in here for the full startup patience.
    pub fn request_node_id_for(&self, patience_ms: u32) -> bool {
        // The transport must answer to the broadcast address while we have no id
        // of our own, or the reply is filtered out before we see it.
        self.transport.set_address(AUTO_NODE_ID);

        let start = self.clock.now_ms();
        loop {
            let mut msg = Message::build(
                AUTO_NODE_ID,
                GATEWAY_ADDRESS,
                NODE_SENSOR_ID,
                Command::Internal,
                i::ID_REQUEST,
            );
            msg.set_str("");
            self.send_raw(&msg);

            self.wait_for(ID_REQUEST_RETRY_MS, None);
            if self.ready() {
                return true;
            }
            if self.clock.now_ms().wrapping_sub(start) >= patience_ms {
                return false;
            }
        }
    }

    /// Presents the node itself, then asks the controller for its configuration.
    ///
    /// The child presentations are the firmware's job -- this is the part the
    /// library used to do before calling `presentation()`.
    pub fn present_node(&self) {
        let id = self.address.get();

        let mut msg = Message::build(
            id,
            GATEWAY_ADDRESS,
            NODE_SENSOR_ID,
            Command::Presentation,
            s::ARDUINO_NODE,
        );
        msg.set_str(LIBRARY_VERSION);
        self.send_raw(&msg);

        // Node sends its parent; the controller answers with the latest node
        // configuration. Nothing here uses the answer -- it carries the
        // metric/imperial preference -- but the controller expects to be asked,
        // and swallowing the reply keeps it out of the application queue.
        let mut cfg = Message::build(
            id,
            GATEWAY_ADDRESS,
            NODE_SENSOR_ID,
            Command::Internal,
            i::CONFIG,
        );
        cfg.set_byte(GATEWAY_ADDRESS);
        self.send_raw(&cfg);

        self.wait_for(
            CONFIG_REPLY_TIMEOUT_MS,
            Some((Command::Internal, i::CONFIG)),
        );
    }

    /// Sends a message as it stands.
    ///
    /// `inline(never)` deliberately. Inlined, every call site gets its own
    /// 32-byte encode buffer in the caller's frame, and LLVM does not overlap
    /// them -- on a 2 KB part, thirty presentation calls turn into most of a
    /// kilobyte of stack. One shared frame is both smaller and less code.
    #[inline(never)]
    pub fn send_raw(&self, msg: &Message) -> bool {
        let mut buf = [0u8; MAX_MESSAGE_SIZE];
        let n = msg.encode(&mut buf);
        if n == 0 {
            return false;
        }
        self.transport.send(msg.destination, &buf[..n])
    }

    /// Builds and sends a message from this node.
    pub fn send(&self, destination: u8, sensor: u8, command: Command, ty: u8) -> Message {
        Message::build(self.address.get(), destination, sensor, command, ty)
    }

    /// Pumps the transport once, handling housekeeping and queueing the rest.
    ///
    /// `inline(never)` for the same reason as [`Node::send_raw`]: it holds a
    /// 40-byte frame buffer.
    #[inline(never)]
    pub fn service(&self) {
        while self.transport.data_available() {
            let mut buf = [0u8; crate::proto::rs485::MAX_FRAME_LENGTH];
            let n = self.transport.receive(&mut buf);
            if n == 0 {
                break;
            }
            let Some(msg) = Message::decode(&buf[..n]) else {
                continue; // malformed, signed, or a protocol version we do not speak
            };
            self.dispatch(&msg);
        }
    }

    fn dispatch(&self, msg: &Message) {
        // Note the awaited message before anything else decides to drop it, so
        // that an internal reply we do not otherwise use still releases a wait.
        if let Some((cmd, ty)) = self.awaited.get() {
            if msg.command == cmd && msg.ty == ty {
                self.awaited_seen.set(true);
            }
        }

        // Echo first: the sender is waiting for confirmation that we saw it, and
        // acting on the message may block for a second or more.
        if msg.echo_request && !msg.echo {
            let mut reply = *msg;
            reply.destination = msg.sender;
            reply.sender = self.address.get();
            reply.last = self.address.get();
            reply.echo_request = false;
            reply.echo = true;
            self.send_raw(&reply);
        }

        match msg.command {
            Command::Internal => self.handle_internal(msg),
            Command::Set | Command::Req => self.inbound.borrow_mut().push(*msg),
            // Presentation messages from elsewhere are none of our business, and
            // C_STREAM is OTA, which this firmware does not implement.
            Command::Presentation | Command::Stream => {}
        }
    }

    fn handle_internal(&self, msg: &Message) {
        match msg.ty {
            i::ID_RESPONSE => {
                if !self.ready() {
                    let assigned = msg.as_i32();
                    if (1..=254).contains(&assigned) {
                        let id = assigned.unsigned_abs() as u8;
                        self.store.write(EEPROM_NODE_ID_ADDRESS, id);
                        self.set_node_id(id);
                    }
                }
            }

            i::PING => {
                // Payload is a hop counter; a leaf node is always one hop away.
                let mut pong = self.send(msg.sender, NODE_SENSOR_ID, Command::Internal, i::PONG);
                pong.set_byte(1);
                self.send_raw(&pong);
            }

            i::HEARTBEAT_REQUEST => {
                let mut beat = self.send(
                    msg.sender,
                    NODE_SENSOR_ID,
                    Command::Internal,
                    i::HEARTBEAT_RESPONSE,
                );
                beat.set_u32(self.clock.now_ms());
                self.send_raw(&beat);
            }

            i::DISCOVER_REQUEST => {
                let mut found = self.send(
                    msg.sender,
                    NODE_SENSOR_ID,
                    Command::Internal,
                    i::DISCOVER_RESPONSE,
                );
                found.set_byte(GATEWAY_ADDRESS); // our parent is the gateway
                self.send_raw(&found);
            }

            i::PRESENTATION => self.presentation_requested.set(true),

            // I_CONFIG carries the metric/imperial preference, which nothing in
            // this firmware formats for display. I_TIME, I_VERSION and
            // I_LOG_MESSAGE are equally uninteresting to a node with no clock
            // and no display.
            _ => {}
        }
    }

    /// Takes the next application message, if one is waiting.
    pub fn take_inbound(&self) -> Option<Message> {
        self.inbound.borrow_mut().pop()
    }

    /// Services the transport for `ms`, or until `awaited` arrives.
    pub fn wait_for(&self, ms: u32, awaited: Option<(Command, u8)>) {
        self.awaited.set(awaited);
        self.awaited_seen.set(false);

        let start = self.clock.now_ms();
        loop {
            self.service();
            if awaited.is_some() && self.awaited_seen.get() {
                break;
            }
            if self.clock.now_ms().wrapping_sub(start) >= ms {
                break;
            }
        }

        self.awaited.set(None);
    }
}

// ---------------------------------------------------------------------------
// The Bus implementation
// ---------------------------------------------------------------------------

use crate::hal::{Bus, InboundMessage, SensorClass, SensorId, ValueType};
use crate::proto::message::v;

const fn raw_value_type(ty: ValueType) -> u8 {
    match ty {
        ValueType::Status => v::STATUS,
        ValueType::Percentage => v::PERCENTAGE,
        ValueType::Watt => v::WATT,
        ValueType::Temperature => v::TEMP,
        ValueType::Humidity => v::HUM,
        ValueType::Text => v::TEXT,
        ValueType::Up => v::UP,
        ValueType::Down => v::DOWN,
        ValueType::Stop => v::STOP,
        ValueType::Rgb => v::RGB,
        ValueType::Rgbw => v::RGBW,
    }
}

const fn raw_sensor_class(class: SensorClass) -> u8 {
    match class {
        SensorClass::Binary => s::BINARY,
        SensorClass::Cover => s::COVER,
        SensorClass::Dimmer => s::DIMMER,
        SensorClass::RgbLight => s::RGB_LIGHT,
        SensorClass::RgbwLight => s::RGBW_LIGHT,
        SensorClass::Power => s::POWER,
        SensorClass::Temperature => s::TEMP,
        SensorClass::Humidity => s::HUM,
        SensorClass::Info => s::INFO,
    }
}

/// Recovers a [`ValueType`] from a raw `V_*` code.
#[must_use]
pub const fn value_type_from_raw(raw: u8) -> Option<ValueType> {
    match raw {
        v::STATUS => Some(ValueType::Status),
        v::PERCENTAGE => Some(ValueType::Percentage),
        v::WATT => Some(ValueType::Watt),
        v::TEMP => Some(ValueType::Temperature),
        v::HUM => Some(ValueType::Humidity),
        v::TEXT => Some(ValueType::Text),
        v::UP => Some(ValueType::Up),
        v::DOWN => Some(ValueType::Down),
        v::STOP => Some(ValueType::Stop),
        v::RGB => Some(ValueType::Rgb),
        v::RGBW => Some(ValueType::Rgbw),
        _ => None,
    }
}

/// A received message, kept alive so the domain layer can borrow its payload.
///
/// [`InboundMessage`] holds a `&str` into the payload, so the owner has to
/// outlive it. That is what this wrapper is for.
pub struct Inbound(Message);

impl Inbound {
    /// Translates into the domain representation.
    ///
    /// `None` when the message carries a `V_*` type no child of this node uses,
    /// which is where the C++ `decode()` returned false.
    #[must_use]
    pub fn as_domain(&self) -> Option<InboundMessage<'_>> {
        let ty = value_type_from_raw(self.0.ty)?;
        Some(InboundMessage {
            sensor: self.0.sensor,
            ty,
            boolean: self.0.as_bool(),
            numeric: self.0.as_i32(),
            text: self.0.as_str(),
        })
    }

    /// The underlying message.
    #[must_use]
    pub const fn message(&self) -> &Message {
        &self.0
    }
}

/// [`Bus`] over a MySensors [`Node`].
///
/// The one place in the firmware that knows both the protocol and the domain
/// vocabulary.
pub struct MySensorsBus<'a, P: Platform, T: Transport> {
    node: Node<'a, P, T>,
}

impl<'a, P: Platform, T: Transport> MySensorsBus<'a, P, T> {
    /// Wraps a node.
    pub const fn new(node: Node<'a, P, T>) -> Self {
        Self { node }
    }

    /// The node underneath, for startup and for draining the inbound queue.
    pub const fn node(&self) -> &Node<'a, P, T> {
        &self.node
    }

    /// Takes the next inbound application message.
    pub fn take_inbound(&self) -> Option<Inbound> {
        self.node.take_inbound().map(Inbound)
    }

    /// Builds and sends one `C_SET`.
    ///
    /// `inline(never)`: a whole [`Message`] lives here, and there are a dozen
    /// call sites.
    #[inline(never)]
    fn emit(&self, sensor: SensorId, ty: ValueType, f: impl FnOnce(&mut Message)) {
        let mut msg = self.node.send(
            GATEWAY_ADDRESS,
            sensor,
            Command::Set,
            raw_value_type(ty),
        );
        f(&mut msg);
        self.node.send_raw(&msg);
    }
}

impl<P: Platform, T: Transport> Bus for MySensorsBus<'_, P, T> {
    fn send_sketch_info(&self, name: Text, version: Text) {
        let mut msg = self.node.send(
            GATEWAY_ADDRESS,
            NODE_SENSOR_ID,
            Command::Internal,
            i::SKETCH_NAME,
        );
        msg.set_text(name);
        self.node.send_raw(&msg);

        let mut ver = self.node.send(
            GATEWAY_ADDRESS,
            NODE_SENSOR_ID,
            Command::Internal,
            i::SKETCH_VERSION,
        );
        ver.set_text(version);
        self.node.send_raw(&ver);
    }

    fn present(&self, sensor: SensorId, class: SensorClass, name: Text) -> bool {
        let mut msg = self.node.send(
            GATEWAY_ADDRESS,
            sensor,
            Command::Presentation,
            raw_sensor_class(class),
        );
        msg.set_text(name);
        self.node.send_raw(&msg)
    }

    fn send_bool(&self, sensor: SensorId, ty: ValueType, value: bool) {
        self.emit(sensor, ty, |m| {
            m.set_bool(value);
        });
    }

    fn send_uint(&self, sensor: SensorId, ty: ValueType, value: u32) {
        self.emit(sensor, ty, |m| {
            m.set_u32(value);
        });
    }

    fn send_fixed(&self, sensor: SensorId, ty: ValueType, value: i32, decimals: u8) {
        self.emit(sensor, ty, |m| {
            m.set_fixed(value, decimals);
        });
    }

    fn send_text(&self, sensor: SensorId, ty: ValueType, value: &str) {
        self.emit(sensor, ty, |m| {
            m.set_str(value);
        });
    }

    fn send_literal(&self, sensor: SensorId, ty: ValueType, value: Text) {
        self.emit(sensor, ty, |m| {
            m.set_text(value);
        });
    }

    fn send_fixed_to(&self, node: u8, sensor: SensorId, ty: ValueType, value: i32, decimals: u8) {
        let mut msg = self
            .node
            .send(node, sensor, Command::Set, raw_value_type(ty));
        msg.set_fixed(value, decimals);
        self.node.send_raw(&msg);
    }

    fn request(&self, sensor: SensorId, ty: ValueType) {
        let mut msg = self.node.send(
            GATEWAY_ADDRESS,
            sensor,
            Command::Req,
            raw_value_type(ty),
        );
        msg.set_str("");
        self.node.send_raw(&msg);
    }

    fn wait(&self, ms: u32) {
        self.node.wait_for(ms, None);
    }

    fn wait_for_set(&self, ms: u32, ty: ValueType) {
        self.node
            .wait_for(ms, Some((Command::Set, raw_value_type(ty))));
    }
}
