//! The seam between [`Module`](crate::domain::Module) and the six board variants.
//!
//! `Module` is written once against [`Device`]. Exactly one implementation is
//! named at the single compile-time selection point in the firmware, so the
//! other two are never instantiated and never reach the binary. Unlike the C++
//! version -- which relied on the linker discarding unused vtables -- there is
//! no indirection to discard: every call is static and inlinable.
//!
//! The unit tests instantiate all three.

use crate::hal::{Bus, InboundMessage, Milliamps, Millivolts, Platform};

/// The protective states that suppress load-switching commands.
#[derive(Copy, Clone, Debug, Default)]
pub struct SafetyState {
    /// The board is too hot. Blocks every channel.
    pub thermal_fault: bool,
    /// Per-channel overcurrent latches.
    pub overcurrent: [bool; 4],
}

impl SafetyState {
    /// Whether `channel` must not be energised.
    #[must_use]
    pub fn blocks(&self, channel: u8) -> bool {
        let idx = if channel < 4 { channel as usize } else { 0 };
        self.thermal_fault || self.overcurrent[idx]
    }
}

/// What one board variant does.
pub trait Device<P: Platform> {
    // -- lifecycle ----------------------------------------------------------

    /// Configures every pin the device owns.
    fn begin(&mut self);

    /// Announces the device's own children.
    fn present<B: Bus>(&mut self, bus: &B, presentation_delay_ms: u16);

    /// Publishes an initial value for each of them.
    fn send_initial_state<B: Bus>(&mut self, bus: &B, echo_timeout_ms: u16);

    // -- runtime ------------------------------------------------------------

    /// Handles an inbound message.
    ///
    /// Returns `true` if the message was addressed to this device and consumed.
    fn handle<B: Bus>(&mut self, msg: &InboundMessage<'_>, bus: &B, safety: &SafetyState) -> bool;

    /// Samples the wall switches and acts on them.
    fn poll_buttons<B: Bus>(&mut self, bus: &B, safety: &SafetyState);

    /// Periodic update. `current` is the most recent reading.
    fn tick<B: Bus>(&mut self, bus: &B, current: Milliamps);

    /// De-energises the faulted load and tells the controller.
    ///
    /// Called on every iteration while a thermal or overcurrent fault is active,
    /// so it must be idempotent and must not re-send an unchanged state.
    ///
    /// `safety` is passed so a board with per-output current sensing can shed
    /// only the offending channel; a thermal fault blocks every channel and so
    /// sheds everything.
    fn shed_load<B: Bus>(&mut self, bus: &B, safety: &SafetyState);

    // -- current measurement ------------------------------------------------
    //
    // Module owns the sensors and the reporting; the device only says which
    // channels are live and how they should be sampled. That is what lets one
    // loop cover both the single shared sensor and FourRelay's four.

    /// 1 for a shared sensor, 4 for one sensor per relay.
    fn power_channel_count(&self) -> u8;

    /// Whether `channel` may be drawing current and is worth sampling.
    fn draws_current(&self, channel: u8) -> bool;

    /// DC loads (LED strips) need averaged sampling, not peak-to-peak.
    fn uses_dc_measurement(&self) -> bool {
        false
    }

    // -- optional -----------------------------------------------------------

    /// Runs a self-calibration cycle. Returns `false` if unsupported.
    fn calibrate<B: Bus>(
        &mut self,
        bus: &B,
        sensor: &P::CurrentSensor,
        watchdog: &P::Watchdog,
        vcc_mv: Millivolts,
    ) -> bool {
        let _ = (bus, sensor, watchdog, vcc_mv);
        false
    }

    /// Called before long blocking maintenance actions.
    fn prepare_for_maintenance(&mut self) {}
}
