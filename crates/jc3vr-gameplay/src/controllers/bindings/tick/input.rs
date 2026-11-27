//! The input stages: the debug panel's capture of the controls, the tick's frames and the policy's
//! routes, and the masks over the frame.

use std::time::Instant;

use jc3vr_body::controllers::{
    haptics,
    intent::hold::DevicePoll,
    mode,
    mode::{Detection, Mode},
    roomscale, scheme,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{
    capture::{
        CaptureUpdate, DebugUiInput, OwnershipBlocker, debug_ui_capture, desktop_input_observation,
        update_debug_ui_capture,
    },
    frame::{self, ControllerFrame, Edges, Hand},
    haptics::cues::Event,
    semantic::vocabulary::MenuAction,
};
use jc3vr_view::{
    vr,
    vr::input::semantic::{
        frame as semantic_frame,
        frame::{SemanticEdges, SemanticFrame},
        history,
    },
};
use parking_lot::Mutex;

use crate::controllers::bindings::{
    INPUT_POLICY, Idle, PANEL_RECOVERY, STATE,
    capture_wait::CaptureWait,
    intents::Intent,
    pointer::{toggle_panel, update_controller, update_egui_controller},
    policy_a,
    policy_a::{
        SystemCommand,
        runtime::{GroupBlocker, Tick as PolicyTick, TickInput},
    },
    recovery::{Blocker, NeutralPolicy},
    settings::Settings,
    stage::Segment,
    stand_down,
    stand_down::{Context, Reason},
    tick::Live,
};

/// What the capture stage read.
pub(super) struct Capture {
    /// When each hand's controls are at rest.
    pub(super) neutral_policy: NeutralPolicy,
    /// The policy's input group for the latest frame is at rest.
    pub(super) source_neutral: bool,
    /// What the capture's update read.
    pub(super) input: DebugUiInput,
    /// The capture cancelled the bindings' routes this tick.
    pub(super) cancelled: bool,
}

/// Step the debug panel's capture of the controls, and cancel the bindings' routes when it says to.
pub(super) fn sample(
    game: &GameTick<'_>,
    settings: &Settings,
    poll: DevicePoll,
    sink: &mut Vec<Segment>,
) -> Capture {
    let neutral_policy = NeutralPolicy::new(&settings.bindings, &settings.vr_turn, settings.acting);
    let desktop = desktop_input_observation();
    let raw_frame = frame::latest();
    let source_neutral = raw_frame
        .as_ref()
        .and_then(|frame| history::original_group(frame.sequence))
        .is_some_and(|group| {
            policy_a::runtime::group_neutral(&group, &settings.bindings, settings.acting)
        });
    let controller_neutral = !settings.enabled
        || !scheme::is_controllers()
        || raw_frame
            .as_ref()
            .is_some_and(|frame| neutral_policy.frame_neutral(frame) && source_neutral);
    let input = DebugUiInput {
        focused: desktop.focused() == Some(true)
            && raw_frame.as_ref().is_none_or(|frame| frame.focused),
        tracking_ready: !settings.enabled
            || !scheme::is_controllers()
            || raw_frame.as_ref().is_some_and(|frame| {
                frame.any_active()
                    && frame
                        .hands
                        .iter()
                        .filter(|hand| hand.active)
                        .all(|hand| hand.grip.is_valid())
            }),
        // An unavailable desktop sample (the window unfocused, or an inline tick off the window's
        // thread) cannot show a held key, and it cannot block gameplay in the headset:
        // the editing owner is gated on desktop focus, under which the sample is observed.
        controls_neutral: controller_neutral && desktop.controls_neutral() != Some(false),
    };
    let update = update_debug_ui_capture(input);
    log_capture_wait(update.as_ref(), input, || CaptureEvidence {
        hands: raw_frame
            .as_ref()
            .map(|frame| neutral_policy.blockers(frame)),
        group: raw_frame
            .as_ref()
            .and_then(|frame| history::original_group(frame.sequence))
            .map(|group| {
                policy_a::runtime::group_blocker(&group, &settings.bindings, settings.acting)
            }),
        desktop_neutral: desktop.controls_neutral(),
    });
    let cancellation = update.and_then(|update| capture_cancellation(&update, poll));
    if let Some(poll_after) = cancellation {
        cancel_capture_routes(game, settings, poll_after, sink);
    }
    Capture {
        neutral_policy,
        source_neutral,
        input,
        cancelled: cancellation.is_some(),
    }
}

/// Stand the bindings down for a capture that cancelled their routes ([`Reason::CaptureCancel`]),
/// undoing their writes for `poll`.
fn cancel_capture_routes(
    game: &GameTick<'_>,
    settings: &Settings,
    poll: DevicePoll,
    sink: &mut Vec<Segment>,
) {
    stand_down::run(
        Reason::CaptureCancel,
        &mut Context {
            game,
            poll,
            settings,
            state: &mut STATE.lock(),
            sink,
        },
    );
}

/// The tick's frames and the policy's routes.
pub(super) struct Routed {
    /// The latest controller frame, while a controller is active and the frame is focused.
    pub(super) frame: Option<ControllerFrame>,
    /// Why the bindings do not inject this tick, if they do not.
    pub(super) idle: Option<Idle>,
    pub(super) semantic: Option<SemanticFrame>,
    pub(super) edges: Edges,
    pub(super) semantic_edges: SemanticEdges,
    /// The policy's routes, empty when the capture or the panel's toggle cancelled them.
    pub(super) policy: PolicyTick,
}

/// Take the tick's frames and edges, let the policy arbitrate the system chords and dispatch
/// them, and drive the panel pointer. A toggle of the panel steps the capture again and detects
/// the mode again into `detection`.
pub(super) fn route(
    game: &GameTick<'_>,
    settings: &Settings,
    poll: DevicePoll,
    sink: &mut Vec<Segment>,
    capture: &Capture,
    detection: &mut Detection,
) -> Routed {
    let frame = frame::latest()
        .filter(ControllerFrame::any_active)
        .filter(|frame| frame.focused);
    let mut idle = idle(frame.as_ref(), poll);
    // The edges are taken even while idle, so that a press from before injection resumed is not
    // replayed then.
    let semantic_frame::Taken {
        semantic,
        edges,
        semantic_edges,
    } = semantic_frame::take();
    let publications = history::take_gameplay();
    let mut policy = INPUT_POLICY.lock().step(TickInput {
        publications,
        frame: frame.as_ref(),
        context: policy_context(detection.mode),
        gameplay: idle.is_none(),
        usable: scheme::is_controllers(),
        config: &settings.bindings,
        dominant: settings.acting,
    });
    crate::controllers::summary::hold::publish(false, false);
    let mut cancelled = capture.cancelled;
    for &command in &policy.commands {
        system_command(command);
        if command == SystemCommand::TogglePanel {
            let cancellation = update_debug_ui_capture(DebugUiInput {
                controls_neutral: false,
                ..capture.input
            })
            .and_then(|update| capture_cancellation(&update, poll));
            if let Some(poll_after) = cancellation {
                cancel_capture_routes(game, settings, poll_after, sink);
            }
            *detection = mode::tick(game);
            if debug_ui_capture().is_some_and(|snapshot| !snapshot.gameplay_effective) {
                idle = Some(Idle::ModPanel);
            }
            cancelled = true;
            break;
        }
    }
    if cancelled {
        policy = Default::default();
    }
    if frame.is_none() {
        PANEL_RECOVERY.lock().suspend();
    }
    panel_pointer(
        settings,
        frame.as_ref(),
        edges,
        &policy,
        &capture.neutral_policy,
    );
    Routed {
        frame,
        idle,
        semantic,
        edges,
        semantic_edges,
        policy,
    }
}

/// The frame and its edges after the policy's and the recovery's masks, beside the frame as it
/// came.
pub(super) struct Filtered {
    pub(super) frame: ControllerFrame,
    pub(super) edges: Edges,
    pub(super) original: ControllerFrame,
    pub(super) original_edges: Edges,
}

/// Mask `frame` by the policy's routes and each hand's recovery, publish the recovery, and drive
/// the game pointer.
pub(super) fn filter(
    live: &mut Live<'_, '_>,
    frame: ControllerFrame,
    routed: &Routed,
    neutral_policy: &NeutralPolicy,
) -> Filtered {
    let original = frame.clone();
    let original_edges = routed.edges;
    let mut frame = frame;
    let mut edges = routed.edges;
    routed.policy.filter_raw(&mut frame, &mut edges);
    live.state
        .recovery
        .filter_observed(&original, &mut frame, &mut edges, neutral_policy);
    live.state.publish_recovery();
    let acting = live.settings.acting;
    update_controller(
        Some(&frame),
        if live.state.recovery.ready(acting) {
            routed.policy.intent(MenuAction::PointerSelect.id(), acting)
        } else {
            Intent::default()
        },
        live.detection.mode == Mode::Ui,
        acting,
        live.state.recovery.ready(acting),
    );
    Filtered {
        frame,
        edges,
        original,
        original_edges,
    }
}

/// Why the bindings do not inject this tick, before the panel's toggle.
fn idle(frame: Option<&ControllerFrame>, poll: DevicePoll) -> Option<Idle> {
    if !scheme::is_controllers() {
        Some(Idle::GamepadScheme)
    } else if frame.is_none() {
        Some(Idle::NoControllers)
    } else if poll == DevicePoll::Disabled {
        Some(Idle::PollDisabled)
    } else if debug_ui_capture().is_some_and(|snapshot| !snapshot.gameplay_effective) {
        Some(Idle::ModPanel)
    } else {
        None
    }
}

fn policy_context(mode: Mode) -> u64 {
    let detail = match mode {
        Mode::OnFoot(variant) => variant as u64,
        Mode::Airborne(variant) => 2 + variant as u64,
        Mode::Vehicle(seat) => 5 + (seat.class as u64) * 2 + u64::from(seat.driver),
        Mode::Ui => 32,
    };
    (detail << 8) | mode.action_set() as u64
}

/// Dispatch accepted system commands even while gameplay injection is stopped.
fn system_command(command: SystemCommand) {
    match command {
        SystemCommand::Recenter => {
            let kind = roomscale::manual_recenter_kind();
            vr::request_recenter(kind);
            tracing::info!(target: "controllers", ?kind, "controller input: recentering the view");
            haptics::play_hands(Hand::BOTH, Event::Chord);
        }
        SystemCommand::TogglePanel => {
            let enabled = toggle_panel();
            tracing::info!(target: "controllers", enabled, "controller input: toggled the debug panel");
            haptics::play(Hand::Right, Event::Chord);
        }
        SystemCommand::Mark => {
            tracing::info!(target: "controllers", "controller input: take marker");
            jc3vr_core::session::request_mark();
            haptics::play_hands(Hand::BOTH, Event::Chord);
        }
    }
}

/// Whether a capture update cancels the bindings' routes, and if so the device poll their
/// stand-down must undo its writes for: the poll the next poll stage runs, not the one this tick
/// saw. A panel that takes the game's device clears its enabled flag before that poll, so a
/// stand-down for a running poll would leave the raised values (the move stick among them) for a
/// poll that never comes, and Rico would keep walking under the panel.
fn capture_cancellation(update: &CaptureUpdate, poll: DevicePoll) -> Option<DevicePoll> {
    let cancels = update.effects.consume_until_neutral
        || update.effects.clear_pending_events
        || update.effects.cancel_pointer_and_drag;
    cancels.then_some(if update.snapshot.intended_game_device_enabled {
        poll
    } else {
        DevicePoll::Disabled
    })
}

/// What the capture's neutral test read, for the log of a wait that does not end.
struct CaptureEvidence {
    /// Each hand's control that is not at rest, or `None` without a controller frame.
    hands: Option<[Option<Blocker>; 2]>,
    /// Why the policy's input group is not neutral, or `None` without a group for the frame.
    group: Option<Option<GroupBlocker>>,
    /// The desktop's keys and mouse, when the window's thread observed them.
    desktop_neutral: Option<bool>,
}

/// Log a debug panel that is up while neither editing nor gameplay owns the controls, naming what
/// the wait is on. `evidence` is read only when a log is due.
fn log_capture_wait(
    update: Option<&CaptureUpdate>,
    input: DebugUiInput,
    evidence: impl FnOnce() -> CaptureEvidence,
) {
    let blocker = update
        .map(|update| update.snapshot)
        .filter(|snapshot| snapshot.visible)
        .and_then(|snapshot| snapshot.blocker)
        .filter(|blocker| *blocker != OwnershipBlocker::Editing);
    let Some(waited) = CAPTURE_WAIT.lock().step(blocker.is_some(), Instant::now()) else {
        return;
    };
    let evidence = evidence();
    tracing::info!(
        target: "controllers",
        ?waited,
        ?blocker,
        focused = input.focused,
        tracking_ready = input.tracking_ready,
        controls_neutral = input.controls_neutral,
        hands = ?evidence.hands,
        group = ?evidence.group,
        desktop_neutral = ?evidence.desktop_neutral,
        "debug panel: neither editing nor gameplay owns the controls",
    );
}

static CAPTURE_WAIT: Mutex<CaptureWait> = Mutex::new(CaptureWait::new());

/// Drive the panel pointer from the frame after the policy's routes and the panel recovery's
/// masks, while the panel's capture takes the pointer.
fn panel_pointer(
    settings: &Settings,
    frame: Option<&ControllerFrame>,
    edges: Edges,
    policy: &PolicyTick,
    neutral_policy: &NeutralPolicy,
) {
    let panel_input = frame.cloned().map(|mut consumer| {
        let mut edges = edges;
        policy.filter_raw(&mut consumer, &mut edges);
        let mut recovery = PANEL_RECOVERY.lock();
        if let Some(original) = frame {
            recovery.observe(original);
            recovery.filter_observed(original, &mut consumer, &mut edges, neutral_policy);
        }
        (consumer, edges)
    });
    update_egui_controller(
        panel_input
            .as_ref()
            .filter(|_| debug_ui_capture().is_some_and(|snapshot| snapshot.edit_capture.pointer))
            .map(|(frame, edges)| (frame, edges)),
        settings.acting,
        Hand::BOTH.map(|hand| {
            if panel_input.as_ref().is_some_and(|(frame, _)| {
                frame.hand(hand).active && frame.hand(hand).grip.is_tracked()
            }) && PANEL_RECOVERY.lock().ready(hand)
            {
                policy.intent(MenuAction::PointerSelect.id(), hand)
            } else {
                Intent::default()
            }
        }),
    );
}
