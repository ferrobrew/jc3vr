//! The controller-to-game bindings: on every engine input tick, map the latest controller frame
//! and its edges to the game's actions for the current mode, and inject them.
//!
//! - [`controls`](jc3vr_body::controllers::intent::controls) reduces the frame to logical controls (deadzones, edges, what each grip would
//!   take);
//! - [`intents`] reads the tick's semantic actions and masks what the mod claims of each hand;
//! - [`tables`] maps the semantic actions to the wanted game actions per mode;
//! - `bail` chooses, once per press, between the vehicle exit and the parachute bail for the
//!   exit's long press;
//! - [`hold`](jc3vr_body::controllers::intent::hold) turns the wanted actions into effector writes with the game's hold timing;
//! - the grip model ([`jc3vr_body::controllers::grab`]) gathers the targets the weapon handling, the
//!   vehicle handling, and the physical hands provide, resolves each hand among them, and hands
//!   each grab to the subsystem that owns its target;
//! - [`grapple_button`](jc3vr_grapple::controllers::grapple_button) reads the grapple button's gestures while tethers or a line are attached:
//!   a tap, a double tap that releases the tethers, a hold that tethers or tightens, and a sharp
//!   pull that zips;
//! - [`yank`](jc3vr_grapple::controllers::yank) detects the yank gesture on the grapple hand, a sharp pull while the grapple button is
//!   held, and the snatch's own yank while the grip holds a remote target
//!   ([`jc3vr_grapple::controllers::snatch`]);
//! - `recovery` masks a hand after a loss of its pose or of injection until its controls rest
//!   ([`HandRecovery`]): its raw controls, and every semantic action on it;
//! - [`inject`] applies the writes to the player and UI action maps;
//! - [`stage`] splits the tick between the input job and the game thread, and hands the writes and
//!   the body turn between them;
//! - [`policy_a`] arbitrates the shared system chords before raw, semantic, and pointer consumers,
//!   retaining ownership while gameplay stands down.
//!
//! The tick runs in two stages ([`stage`]). [`game_tick`], the decisions and the subsystems' acts
//! in the order `tick` lays out, runs in the game stage, on the game thread at the top of the
//! world's pre-simulation queue in play (inline in the input update in the load state and the
//! frontend's loading). Its writes land in the next poll stage, in the `InputDeviceManager::Update`
//! post-call detour ([`crate::hooks::input::look`]), after the device poll and before the game
//! reads its actions. It injects only while the bindings are enabled, the controller scheme is in
//! effect, a controller is active, the game's device poll runs, and the mod's panel is down;
//! otherwise it writes nothing, so a real gamepad under the gamepad scheme is never fought. The
//! right stick's body turn is not an action: it goes to the look detour's next poll stage, which
//! feeds it to the headpose's VR turn scheme.
//!
//! Whenever injection stops, the bindings undo what they wrote rather than leave it to the device
//! poll ([`HoldTracker::release_all`]): the poll does not run while the device manager is disabled,
//! and an effector left clicked there reads as a fresh click on every frame. What else stops, and
//! in which order, is one set of steps per reason (`stand_down`).

pub(crate) mod bail;
mod buttons;
mod capture_wait;
mod flight;
mod gun_fire;
mod heli_hang;
pub mod inject;
pub mod intents;
mod loss;
pub mod pointer;
pub(crate) mod policy_a;
mod rail;
mod recovery;
mod report;
pub(crate) mod settings;
pub(crate) mod stage;
mod stand_down;
mod summary;
pub mod tables;
mod tick;
mod trigger_ownership;

use std::{sync::LazyLock, time::Instant};

use jc3vr_body::controllers::{
    intent,
    intent::{
        controls::ClosedHand,
        hold::{ActionWrite, DevicePoll, HoldTracker},
    },
    mode,
    mode::Detection,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_grapple::controllers::yank::{LineYank, Yank};
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::controllers::bindings::{
    bail::BailLatch,
    heli_hang::HeliHang,
    policy_a::runtime::Runtime,
    recovery::{MaskEvent, MaskLog, Recovery},
    report::{publish_recovery_status, report_writes},
    stage::{Delivery, GameStageOutput, LandedSegment, PollRecord, Segment},
    tables::Bindings,
    trigger_ownership::TriggerOwnership,
};
pub use crate::controllers::bindings::{
    recovery::{Blocker, HandRecovery},
    report::{Idle, ReportedWrite, TickReport, recovery_status},
};

/// The bindings' game stage ([`stage::game_stage`]): take in where the last writes landed, detect
/// the mode (which selects the action set), evaluate the bindings, run the subsystems' acts, and
/// produce this tick's writes. Under [`Delivery::Inline`] the writes land before this returns;
/// under [`Delivery::Deferred`] they are returned for the next poll stage to land.
pub(crate) fn game_tick(
    game: &GameTick<'_>,
    record: PollRecord,
    landed: Vec<LandedSegment>,
    delivery: Delivery,
) -> GameStageOutput {
    let mut reported = absorb_landed(landed);
    let mut sink = Vec::new();
    let body_turn = tick::tick(game, record.dt, record.poll, &mut sink);
    let deferred = match delivery {
        Delivery::Inline => {
            // SAFETY: an inline game stage runs inside the input update right after the device
            // poll, outside the input job.
            reported.extend(absorb_landed(unsafe { stage::land(sink) }));
            Vec::new()
        }
        Delivery::Deferred => sink,
    };
    STATE.lock().report.writes = reported;
    GameStageOutput {
        deferred,
        body_turn,
    }
}

/// Tell the hold tracker where its tracked writes landed, and report every write's outcome.
fn absorb_landed(landed: Vec<LandedSegment>) -> Vec<ReportedWrite> {
    let mut state = STATE.lock();
    let mut reported = Vec::new();
    for LandedSegment { segment, landings } in landed {
        if segment.tracked {
            let effectors: Vec<_> = landings.iter().map(|l| l.effector).collect();
            state.tracker.landed(&segment.writes, &effectors);
        }
        reported.extend(report_writes(&segment.writes, &landings));
    }
    reported
}

/// The last input tick's report.
pub fn last_report() -> TickReport {
    STATE.lock().report.clone()
}

/// The yank gesture's readout: how many yanks were detected, and the last one.
pub fn yank_status() -> (u64, Option<Yank>) {
    let state = STATE.lock();
    (state.yank.detector.count, state.yank.detector.last)
}

/// System ownership survives gameplay stand-down and editor open/close transitions.
static INPUT_POLICY: LazyLock<Mutex<Runtime>> = LazyLock::new(|| Mutex::new(Runtime::default()));
static PANEL_RECOVERY: Mutex<Recovery> = Mutex::new(Recovery::new());

static STATE: Mutex<State> = Mutex::new(State {
    poll: DevicePoll::Running,
    recovery: Recovery::new(),
    mask_log: MaskLog::new(),
    bindings: Bindings::new(),
    closed: [ClosedHand::new(); 2],
    trigger_ownership: TriggerOwnership::new(),
    tracker: HoldTracker::new(),
    flight: None,
    rail: rail::RailRelease::new(),
    yank: LineYank::new(),
    bail: BailLatch::new(),
    heli_hang: HeliHang::new(),
    report: TickReport {
        detection: Detection {
            mode: mode::Mode::OnFoot(mode::OnFootVariant::Ground),
            traversal: None,
            held: None,
        },
        idle: None,
        body_turn: 0.0,
        writes: Vec::new(),
    },
});

struct State {
    /// The device poll as of the last tick, to log its changes.
    poll: DevicePoll,
    recovery: Recovery,
    mask_log: MaskLog,
    bindings: Bindings,
    /// Whether each hand is still closed on what its grip took, by [`Hand::index`].
    closed: [ClosedHand; 2],
    trigger_ownership: TriggerOwnership,
    tracker: HoldTracker,
    flight: Option<flight::FlightState>,
    rail: rail::RailRelease,
    yank: LineYank,
    /// The bail's choice for the vehicle exit's long press.
    bail: BailLatch,
    heli_hang: HeliHang,
    report: TickReport,
}

impl State {
    /// Stop injecting: forget every hold and latch, and return the writes that undo what the last
    /// tick wrote ([`HoldTracker::release_all`]).
    fn reset(&mut self, poll: DevicePoll) -> Vec<ActionWrite> {
        self.recovery.disarm();
        self.bindings.reset_preserving_snatch();
        self.closed = [ClosedHand::new(); 2];
        intent::clear_hands_closed();
        self.trigger_ownership.suspend();
        if let Some(flight) = &mut self.flight {
            flight.reset();
        }
        self.rail = rail::RailRelease::new();
        self.yank = LineYank::new();
        self.bail.reset();
        self.heli_hang = HeliHang::new();
        self.tracker.release_all(poll)
    }

    /// [`reset`](Self::reset), pushing the writes that undo the last tick's to `sink`.
    fn stand_down(&mut self, poll: DevicePoll, sink: &mut Vec<Segment>) {
        sink.push(Segment {
            writes: self.reset(poll),
            tracked: false,
        });
    }

    /// Log a change of the device poll: while it is disabled the bindings stand down, which
    /// otherwise looks like broken bindings.
    fn observe_poll(&mut self, poll: DevicePoll) {
        if std::mem::replace(&mut self.poll, poll) == poll {
            return;
        }
        match poll {
            DevicePoll::Disabled => tracing::info!(
                target: "controllers",
                "the game's device poll is disabled; the bindings stand down and release their writes"
            ),
            DevicePoll::Running => tracing::info!(
                target: "controllers",
                "the game's device poll runs again; the bindings resume"
            ),
        }
    }

    /// Publish each hand's recovery for the debug UI, and log a hand that keeps waiting for its
    /// controls to rest and its rearm, rate-limited by [`MaskLog`]: a masked hand otherwise looks
    /// like a broken binding.
    fn publish_recovery(&mut self) {
        let status = self.recovery.status();
        publish_recovery_status(status);
        let events = self.mask_log.step(status, Instant::now());
        for (hand, event) in Hand::BOTH.into_iter().zip(events) {
            match event {
                Some(MaskEvent::StillMasked { waited, blocker }) => tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    ?waited,
                    ?blocker,
                    "hand input masked until its controls rest"
                ),
                Some(MaskEvent::Rearmed { waited }) => tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    ?waited,
                    "hand input rearmed"
                ),
                None => {}
            }
        }
    }
}
