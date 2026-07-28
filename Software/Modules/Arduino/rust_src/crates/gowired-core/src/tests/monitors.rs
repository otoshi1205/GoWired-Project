//! Power, thermal and fault-latch decisions.

use crate::domain::monitors::{LatchedFault, PowerMonitor, ThermalMonitor};

fn monitor() -> PowerMonitor {
    PowerMonitor::new(3, 230, 100)
}

#[test]
fn over_limit_is_strictly_above_the_configured_maximum() {
    let m = monitor();
    assert!(!m.over_limit(2900));
    assert!(!m.over_limit(3000)); // exactly at the limit is not over it
    assert!(m.over_limit(3001));
}

#[test]
fn power_uses_voltage_and_power_factor() {
    assert_eq!(monitor().power_w(2000), 460);

    let led = PowerMonitor::new(3, 24, 50);
    assert_eq!(led.power_w(2000), 24);
}

/// The widest intermediate the calculation can be handed is
/// `65535 * 230 * 100`, which is 1.5e9 -- inside `u32`, but only just. A `u16`
/// or `i32` anywhere in that expression would wrap.
#[test]
fn power_does_not_overflow_at_full_scale() {
    let m = PowerMonitor::new(255, 230, 100);
    // 65.535 A at 230 V.
    assert_eq!(m.power_w(u16::MAX), 15_073);
}

#[test]
fn silent_when_nothing_is_drawn_and_nothing_was_reported() {
    assert!(!monitor().should_report(0, 0));
}

#[test]
fn reports_the_transition_to_zero() {
    // A load that switched off has to be published, or the controller shows the
    // last non-zero reading forever.
    assert!(monitor().should_report(0, 2000));
}

#[test]
fn absolute_deadband_below_one_amp() {
    let m = monitor();
    assert!(!m.should_report(550, 500)); // 50 mA of noise
    assert!(m.should_report(650, 500)); // 150 mA is real
}

#[test]
fn relative_deadband_above_one_amp() {
    let m = monitor();
    // 10% of the last reported value, not an absolute 100 mA.
    assert!(!m.should_report(2100, 2000));
    assert!(m.should_report(2300, 2000));
}

/// The deadband is symmetric: a load that drops has to be reported as readily as
/// one that rises.
#[test]
fn deadband_is_symmetric() {
    let m = monitor();
    assert!(m.should_report(1700, 2000));
    assert!(!m.should_report(1900, 2000));
}

#[test]
fn thermal_monitor_trips_only_above_the_limit() {
    let t = ThermalMonitor::new(85);
    assert!(!t.over_limit(849));
    assert!(!t.over_limit(850));
    assert!(t.over_limit(851));
}

/// Below freezing is a legitimate reading for an outdoor module.
#[test]
fn thermal_monitor_accepts_negative_temperatures() {
    let t = ThermalMonitor::new(85);
    assert!(!t.over_limit(-150));
}

/// The pre-refactor overcurrent path re-sent the same status on every loop
/// iteration for as long as the fault lasted; the thermal path reported once.
#[test]
fn latched_fault_reports_each_transition_exactly_once() {
    let mut f = LatchedFault::new();

    assert!(!f.update(false)); // nothing has happened
    assert!(f.update(true)); // fault appears
    assert!(!f.update(true)); // still faulted; stay quiet
    assert!(!f.update(true));
    assert!(f.active());
    assert!(f.update(false)); // fault clears
    assert!(!f.update(false));
    assert!(!f.active());
}

#[test]
fn controller_override_rearms_reporting() {
    let mut f = LatchedFault::new();
    assert!(f.update(true));

    // The controller writes "no fault" to the status child. The next genuine
    // transition must be reported again rather than swallowed as unchanged.
    f.override_from_controller(false);
    assert!(!f.active());
    assert!(f.update(true));
}
