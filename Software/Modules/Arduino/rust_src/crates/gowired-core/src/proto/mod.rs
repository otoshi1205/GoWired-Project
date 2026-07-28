//! MySensors 2.x, from the protocol rather than from the C++ library.
//!
//! # Why this is written out rather than linked
//!
//! The C++ firmware used the MySensors Arduino library, all 51000 lines of it.
//! There is no Rust port of it, and binding to it from Rust would mean keeping
//! the Arduino core, the C++ toolchain and the single-translation-unit
//! restriction that `IBus` existed to work around -- which is most of what a
//! rewrite was for.
//!
//! What this firmware actually needs is a small, well-specified subset: a leaf
//! node on a wired bus, talking to a gateway it can always reach directly. That
//! is about 900 lines, all of it testable on the host, and it is what these
//! modules are.
//!
//! # Confidence, honestly
//!
//! The encoding is verified byte-for-byte against frames the C++ library
//! produces -- not against the specification; see `tools/golden/`. The framing is
//! exercised over every rejection path, and the node's exchanges are driven
//! end-to-end against a scripted gateway. All of that is in `src/tests/`.
//!
//! What has **not** happened is a conversation with a real gateway on real wire:
//! there was no RS485 hardware. The byte-level agreement is strong evidence about
//! the *format* and says nothing about the timing -- the driver turnaround and the
//! baud divisor are derived, not measured.
//!
//! Deliberately absent, and listed in [`node`]: message signing, OTA firmware
//! update, routing/repeating.

pub mod fixed;
pub mod message;
pub mod node;
pub mod rs485;

pub use message::{Command, Message, PayloadType, AUTO_NODE_ID, GATEWAY_ADDRESS, NODE_SENSOR_ID};
pub use node::{Inbound, MySensorsBus, Node};
pub use rs485::{Rs485Transport, SerialPort, Transport};
