//! Diagnostic snapshots and write outcomes for the controller bindings.

use jc3gi::input::input_action_map::Action;
use parking_lot::Mutex;

use crate::controllers::{
    bindings::{
        hold::{ActionWrite, MapKind, Phase},
        inject::{Landing, Outcome},
        recovery::HandRecovery,
    },
    frame::Hand,
    mode::Detection,
};

/// Why the bindings did not inject on a tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Idle {
    /// The bindings toggle is off.
    Disabled,
    /// The gamepad scheme is in effect.
    GamepadScheme,
    /// No controller frame, or no active controller.
    NoControllers,
    /// The game's device manager is disabled, so its device poll does not run.
    PollDisabled,
    /// The mod's panel is up.
    ModPanel,
}

/// One injected write, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReportedWrite {
    pub map: MapKind,
    pub action: Action,
    pub value: f32,
    pub phase: Phase,
    pub outcome: Outcome,
}

/// The last input tick, for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickReport {
    pub detection: Detection,
    /// Why nothing was injected, when nothing was.
    pub idle: Option<Idle>,
    pub body_turn: f32,
    pub writes: Vec<ReportedWrite>,
}

/// Each hand's input recovery at the last input tick, in [`Hand::index`] order. Safe to call with
/// the config locked: it takes only a lock that is never held while taking
/// another.
pub fn recovery_status() -> [HandRecovery; 2] {
    *RECOVERY_STATUS.lock()
}

pub(super) fn publish_recovery_status(status: [HandRecovery; 2]) {
    *RECOVERY_STATUS.lock() = status;
}

/// Pair each write with what became of it, for the report.
pub(super) fn report_writes(writes: &[ActionWrite], landings: &[Landing]) -> Vec<ReportedWrite> {
    writes
        .iter()
        .zip(landings)
        .map(|(w, landing)| ReportedWrite {
            map: w.target.map,
            action: w.target.action,
            value: w.value,
            phase: w.phase,
            outcome: landing.outcome,
        })
        .collect()
}

static RECOVERY_STATUS: Mutex<[HandRecovery; 2]> = Mutex::new([HandRecovery::Unusable; 2]);
