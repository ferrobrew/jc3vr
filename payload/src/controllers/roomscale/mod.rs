//! Posture and roomscale: the plan of record's "Posture", "Body scale", "Roomscale design", "Crouch,
//! in stages", and "Seat transitions" (`docs/mod/input/controllers-and-roomscale.md`). The body
//! following the head about the room is the rig's ([`crate::controllers::body::rig::root`],
//! `docs/mod/body/rig.md`).
//!
//! - [`posture`]: the posture setting resolved against the head height, and the head height it
//!   implies (the body remap onto the character's eye height);
//! - [`seated`]: the seated eye reference, which follows how the player actually sits;
//! - [`teleport`]: the teleport epoch, from the game's camera reset;
//! - [`probe`]: the head-through-geometry probe;
//! - [`fade`]: the comfort fade the VR blit applies;
//! - [`crouch`]: the visual crouch's IK targets and the capsule's proxy-state rules;
//! - [`seat`]: the seat-transition phases and modes;
//! - [`stance`]: the standing stance: the relaxed idle and the squared stance.
//!
//! The frame half runs once per rendered VR frame from the pose composition
//! ([`compose_cockpit`], called by [`crate::vr::begin_render_frame`] on the game thread, after the
//! simulation tick and before the render update): it resolves the posture, remaps the head height,
//! hands the cockpit to the rig's frame half for the shift, tracks the seat phase, and advances the
//! fade. It publishes what the simulation half needs ([`FrameShared`]).
//!
//! Everything here needs the headset: it runs only while the VR source owns the head pose, and each
//! part is behind its own toggle in [`RoomscaleConfig`] (the head height behind
//! [`BodyRemapConfig::remap_head`]). None of it depends on the controller scheme, because a gamepad
//! player standing in a room has the same head.
//!
//! [`BodyRemapConfig::remap_head`]: crate::controllers::config::BodyRemapConfig::remap_head

pub mod crouch;
pub mod fade;
pub mod posture;
pub mod probe;
pub mod seat;
pub mod seated;
pub mod stance;
pub mod teleport;

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use glam::{Quat, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
pub use seat::SeatPhase;

use crate::{
    config::Config,
    controllers::{
        body::{
            self, BodyDimensions,
            alignment::estimate as alignment_estimate,
            rig::root::{self as rig_root, RootStatus, capsule_enabled},
        },
        config::{Posture, RoomscaleConfig},
        roomscale::{
            crouch::CrouchTracker,
            fade::FadeSource,
            posture::{
                ClassifierInput, ClassifierStatus, DetectedPosture, EffectivePosture, HeadHeight,
                HeadHeightInput, PostureClassifier,
            },
            probe::ProbeResult,
            seat::{SeatEvent, SeatTracker},
            seated::{SeatedReference, SeatedReferenceStatus},
        },
        vehicles::driving_view,
    },
    headpose,
    hooks::{self, animation::active_state_type_id},
    vr::{self, RecenterCause, RecenterKind, stage::StageInfo},
};

/// The roomscale hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_hook_library(teleport::hook_library())
}

/// One rendered frame's inputs to the pose composition.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameInputs {
    /// The head's cockpit position (relative to the recenter baseline), metres.
    pub cockpit: Vec3,
    /// The head's cockpit orientation.
    pub orientation: Quat,
    /// The body rotation the cockpit composes with.
    pub body: Quat,
    pub world_scale: f32,
}

/// The cockpit positions to compose the two sides of the pose pair with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ComposedCockpit {
    pub prev: Vec3,
    pub cur: Vec3,
}

/// The frame half: resolve the posture, remap the head height, hand the cockpit to the rig's
/// frame half for the shift, track the seat phase, advance the fade, and add the driving view's
/// correction, returning the cockpit positions to compose with. Called once per rendered VR frame
/// on the game thread, outside the world update.
pub fn compose_cockpit(inputs: &FrameInputs) -> ComposedCockpit {
    let (config, rig, remap_head, driving_view_config) = Config::lock_query(|c| {
        (
            c.controllers.roomscale,
            c.controllers.body.rig,
            c.controllers.body.remap_head,
            c.controllers.vehicles.driving_view,
        )
    });
    let stage = vr::stage::stage();
    let player = body::player();
    let now = Instant::now();
    let in_gameplay = hooks::in_gameplay();
    let torso_yaw = alignment_estimate().map(|e| e.yaw);
    let on_foot = headpose::sim::mode() == headpose::sim::HeadMode::OnFoot;

    let mut frame = FRAME.lock();
    let dt = frame
        .last_at
        .map_or(0.0, |t| now.duration_since(t).as_secs_f32());
    if dt > FRAME_GAP.as_secs_f32() {
        fade::reset();
    }
    frame.last_at = Some(now);

    let classifier = if config.posture == Posture::Auto {
        Some(
            frame
                .classifier
                .update(&classifier_input(&stage, &player), dt, &config.auto_posture),
        )
    } else {
        frame.classifier.reset();
        None
    };
    if let Some(c) = classifier {
        let seen = (c.detected, c.floor_level);
        if frame.classified != Some(seen) {
            frame.classified = Some(seen);
            tracing::info!(
                target: "controllers",
                detected = ?c.detected,
                reason = c.reason.label(),
                floor_level = c.floor_level,
                head_height_m = ?c.head_height_m,
                floor_below_m = c.thresholds.floor_below_m,
                sit_below_m = c.thresholds.sit_below_m,
                "roomscale: the automatic posture's detection changes",
            );
        }
    } else {
        frame.classified = None;
    }
    let detected = classifier.map_or(DetectedPosture::Seated, |c| c.detected);
    let posture = posture::resolve(config.posture, detected);
    let seated_reference = if posture == EffectivePosture::Seated {
        frame.seated_reference.update(
            stage.head_height,
            player.seated_eye_height,
            dt,
            &config.seated_reference,
        )
    } else {
        frame.seated_reference.current()
    };
    // The seated crouch is Rico crouching on foot; in a vehicle seat it would drop the view into
    // the footwell. The toggle keeps its state for when he is out.
    let seated_crouch = frame.seated_crouch && frame.seat.phase() == SeatPhase::Out;

    let head_height = remap_head.then(|| {
        posture::head_height(
            &HeadHeightInput {
                posture,
                cockpit_y: inputs.cockpit.y,
                stage_height: stage.head_height,
                seated_reference: (posture == EffectivePosture::Seated)
                    .then_some(seated_reference)
                    .flatten(),
                seated_crouch,
                seated_drop_m: config.crouch.seated_drop_m,
            },
            &player,
            &BodyDimensions::CHARACTER_ESTIMATE,
        )
    });
    // Without the head remap the seated crouch still lowers the eyes.
    let vertical = head_height.map_or_else(
        || {
            inputs.cockpit.y
                - posture::seated_crouch_drop(posture, seated_crouch, config.crouch.seated_drop_m)
        },
        |h| h.cockpit_y,
    );
    let root = rig_root::frame(&rig_root::FrameInputs {
        raw: Vec3::new(inputs.cockpit.x, vertical, inputs.cockpit.z),
        head: inputs.orientation,
        view: inputs.body,
        world_scale: inputs.world_scale,
        posture,
        seat: frame.seat.phase(),
        on_foot,
        floor_jumps: stage.floor_jumps,
        dt,
        torso_yaw,
        config: rig,
    });
    let (mut prev, mut cur) = (root.prev, root.cur);
    prev.y = vertical;
    cur.y = vertical;

    let eye_drop_m = if (vertical * inputs.world_scale).is_finite() {
        (-vertical * inputs.world_scale).max(0.0)
    } else {
        0.0
    };
    let crouch = config.crouch;
    let crouched = frame
        .crouch
        .update(eye_drop_m, crouch.enter_drop_m, crouch.exit_drop_m);

    if in_gameplay {
        // SAFETY: the game thread, outside the world update.
        unsafe { seat_tick(&mut frame, &config, now) };
    } else {
        frame.seat = SeatTracker::default();
        frame.seat_fade_release = None;
        fade::set_target(FadeSource::Seat, 0.0);
    }
    if let Some((target, at)) = frame.seat_fade_release
        && now >= at
    {
        fade::set_target(FadeSource::Seat, target);
        frame.seat_fade_release = None;
    }

    let sim = rig_root::status().sim;
    let probe = sim
        .probe
        .filter(|p| now.duration_since(p.at) < probe::STALE_AFTER)
        .map(|p| p.result);
    let head_fade = match probe {
        Some(result) if config.head_fade.enabled && in_gameplay => {
            probe::fade_for_depth(result.depth(), &config.head_fade)
        }
        _ => 0.0,
    };
    fade::set_target(FadeSource::HeadInGeometry, head_fade);
    fade::advance(dt, &config.fade);

    let sim_stale = sim
        .at
        .is_none_or(|at| now.duration_since(at) > probe::STALE_AFTER);
    if sim.capsule_owned && (!capsule_enabled(&config) || sim_stale) {
        // SAFETY: the game thread, outside the world update.
        unsafe { rig_root::release_capsule() };
    }

    let seat = frame.seat.phase();
    // Added last, so the posture, the seated reference, the crouch, and the chase never see it.
    // A driving state missing from the seat tables reads as out, so only a transition holds the
    // correction back.
    let driving_view = driving_view::advance(
        &driving_view_config,
        driving_view::FrameInputs {
            seated: !matches!(
                seat,
                SeatPhase::Entering | SeatPhase::Shuffle | SeatPhase::Exiting
            ),
            dt,
        },
        now,
    );
    frame.shared = Some(FrameShared {
        posture,
        crouched,
        eye_drop_m,
        seat,
        at: now,
    });
    frame.status = FrameStatus {
        setting: config.posture,
        posture,
        classifier,
        seated_reference: frame.seated_reference.status(),
        stage,
        head_height,
        cockpit_y: vertical,
        world_scale: inputs.world_scale,
        eye_drop_m,
        crouched,
        seated_crouch: frame.seated_crouch,
        seat,
        last_seat_event: frame.last_seat_event,
        probe,
    };
    ComposedCockpit {
        prev: prev + driving_view,
        cur: cur + driving_view,
    }
}

/// Re-base the cockpit shift after the recenter baseline was re-based
/// ([`crate::controllers::body::rig::root::on_recenter`]). Called by the recenter at the top of the
/// frame ([`crate::vr::auto_recenter_tick`]).
///
/// A manual recenter is also the player saying "this is how I am": in the automatic posture it
/// classifies seated or standing at once from the head height, and while seated it captures the
/// head height as the seated eye reference ([`SeatedReference::capture`]). A seat transition's
/// re-base does neither, because it happens on the game's schedule rather than the player's.
pub fn on_recenter(kind: RecenterKind, turn: Quat, cause: RecenterCause) {
    let setting = Config::lock_query(|c| c.controllers.roomscale.posture);
    let stage = vr::stage::stage();
    let player = body::player();
    rig_root::on_recenter(kind, turn);
    if cause != RecenterCause::Manual {
        return;
    }
    let mut frame = FRAME.lock();
    let seated = match setting {
        Posture::Seated => true,
        Posture::Auto => {
            frame
                .classifier
                .reclassify(&classifier_input(&stage, &player));
            frame.classifier.detected() == DetectedPosture::Seated
        }
        Posture::Standing => false,
    };
    if seated
        && let Some(head_height) = stage.head_height
        && frame.seated_reference.capture(head_height)
    {
        tracing::info!(
            target: "vr",
            head_height_m = head_height,
            "roomscale: captured the seated eye reference at the recenter"
        );
    }
}

/// The classifier's input from one frame's STAGE information and the player's body.
fn classifier_input(stage: &StageInfo, player: &BodyDimensions) -> ClassifierInput {
    ClassifierInput {
        head_height: stage.head_height,
        standing_eye_m: player.eye_height,
        calibrated_seated_eye_m: player.seated_eye_height,
    }
}

/// What a recenter outside a seat transition (the F7 key, the debug UI's button, and the
/// gameplay-resume recenter) re-bases: yaw and height while the body follows the head on foot, and
/// the full re-base otherwise ([`recenter_kind`]).
pub fn manual_recenter_kind() -> RecenterKind {
    let chase = Config::lock_query(|c| c.controllers.body.rig.chase.enabled);
    let frame = frame_shared().filter(|f| f.fresh(Instant::now()));
    recenter_kind(chase, frame.map(|f| (f.posture, f.seat)))
}

/// The recenter for a chase toggle and the latest frame's posture and seat phase: the chase is
/// active in the standing posture out of a seat, where it keeps the body under the head, so only
/// the yaw and the height need re-basing. With the chase off nothing else brings the body back
/// under the head, and without a recent frame nothing is known: the full re-base.
pub fn recenter_kind(
    chase_enabled: bool,
    frame: Option<(EffectivePosture, SeatPhase)>,
) -> RecenterKind {
    match frame {
        Some((EffectivePosture::Standing, SeatPhase::Out)) if chase_enabled => {
            RecenterKind::YawAndHeight
        }
        _ => RecenterKind::Full,
    }
}

/// Toggle the seated crouch. Acts only in the seated posture, where the eyes have a crouched preset;
/// standing players crouch physically.
pub fn toggle_seated_crouch() {
    let mut frame = FRAME.lock();
    let seated = frame
        .shared
        .is_some_and(|f| f.posture == EffectivePosture::Seated);
    if seated {
        frame.seated_crouch = !frame.seated_crouch;
        tracing::debug!(target: "controllers", crouched = frame.seated_crouch, "roomscale: seated crouch toggled");
    }
}

/// Queue the visual crouch's IK targets for the local player at the entry of its pose-finalization
/// pass, when the head has dropped. Called by the character hook after the body IK and the hands.
///
/// # Safety
///
/// `character` must be null or a live character, at the entry of its pose-finalization pass.
pub(crate) unsafe fn queue_crouch_ik(character: *mut Character) {
    let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
    if !local {
        return;
    }
    let config = Config::lock_query(|c| c.controllers.roomscale.crouch);
    let now = Instant::now();
    let frame = frame_shared().filter(|f| f.fresh(now) && f.seat == SeatPhase::Out);
    let on_foot = headpose::sim::mode() == headpose::sim::HeadMode::OnFoot;
    let queued = match frame {
        Some(frame)
            if config.visual
                && on_foot
                && headpose::source() == headpose::Source::Vr
                && hooks::in_gameplay() =>
        {
            let drop = crouch::hips_drop(frame.eye_drop_m, config.hips_share);
            // SAFETY: forwarded from the caller.
            drop >= MIN_HIPS_DROP_M && unsafe { crouch::queue_ik(character, drop) }
        }
        _ => false,
    };
    FRAME.lock().visual_crouch = queued;
}

/// Whether the visual crouch queued its targets on the latest pose finalization; read after
/// [`queue_crouch_ik`] in the same pass, this tick's.
pub(crate) fn visual_crouch_queued() -> bool {
    FRAME.lock().visual_crouch
}

/// What the frame half publishes for the simulation half.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameShared {
    pub posture: EffectivePosture,
    /// Whether the character counts as crouched.
    pub crouched: bool,
    /// How far the eyes sit below the character's standing eye height, metres.
    pub eye_drop_m: f32,
    pub seat: SeatPhase,
    pub at: Instant,
}

impl FrameShared {
    /// Whether the frame half ran recently enough for its values to hold.
    pub fn fresh(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.at) < FRAME_GAP
    }
}

/// The latest frame half's output, `None` before the first VR frame.
pub fn frame_shared() -> Option<FrameShared> {
    FRAME.lock().shared
}

/// The roomscale readout, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RoomscaleStatus {
    /// Whether the frame half ran recently (a VR frame is rendering).
    pub active: bool,
    pub frame: FrameStatus,
    /// The rig's root: the owner, the chase, the shift, the probe, and the capsule.
    pub root: RootStatus,
    pub fade: fade::FadeStatus,
    /// Teleports seen, and how many came from the distance test.
    pub teleports: u64,
    pub warps: u64,
    /// Whether the visual crouch queued its targets on the last pose finalization.
    pub visual_crouch: bool,
}

/// The frame half's readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameStatus {
    /// The posture setting, before the automatic setting is resolved.
    pub setting: Posture,
    pub posture: EffectivePosture,
    /// The automatic posture's detection, while the setting is automatic.
    pub classifier: Option<ClassifierStatus>,
    /// The seated eye reference and the adaptive baseline's state.
    pub seated_reference: SeatedReferenceStatus,
    pub stage: StageInfo,
    /// The remapped head height, while the head remap is on.
    pub head_height: Option<HeadHeight>,
    /// The composed cockpit position's vertical component, metres before the world scale: the
    /// remapped head height's offset while the remap is on, and the head's own height from the
    /// recenter baseline (less the seated crouch) otherwise.
    pub cockpit_y: f32,
    /// The world scale the cockpit composed with.
    pub world_scale: f32,
    pub eye_drop_m: f32,
    pub crouched: bool,
    /// Whether the seated crouch toggle is down; it lowers the eyes only out of a vehicle seat.
    pub seated_crouch: bool,
    pub seat: SeatPhase,
    pub last_seat_event: Option<SeatEvent>,
    /// The head probe's current result, while it runs.
    pub probe: Option<ProbeResult>,
}

pub fn status() -> RoomscaleStatus {
    let (frame, active, visual_crouch) = {
        let f = FRAME.lock();
        let active = f.shared.is_some_and(|s| s.fresh(Instant::now()));
        (f.status, active, f.visual_crouch)
    };
    RoomscaleStatus {
        active,
        frame,
        root: rig_root::status(),
        fade: fade::status(),
        teleports: teleport::epoch(),
        warps: teleport::warps(),
        visual_crouch,
    }
}

/// A gap between rendered frames longer than this means the headset stopped rendering: the fade
/// resets, and the frame half's values stop holding for the simulation half.
const FRAME_GAP: Duration = Duration::from_millis(500);

/// The smallest hips drop worth queueing a crouch for, metres; below it the standing pose stands.
const MIN_HIPS_DROP_M: f32 = 0.02;

/// How many unknown vehicle states are logged, so that a long session cannot fill the log.
const UNKNOWN_STATE_LOG_LIMIT: usize = 32;

struct FrameState {
    last_at: Option<Instant>,
    seated_crouch: bool,
    classifier: PostureClassifier,
    /// The automatic posture's last logged detection, and whether the head was at floor level.
    classified: Option<(DetectedPosture, bool)>,
    seated_reference: SeatedReference,
    crouch: CrouchTracker,
    seat: SeatTracker,
    last_seat_event: Option<SeatEvent>,
    /// A seat fade target waiting out its hold, and when it applies.
    seat_fade_release: Option<(f32, Instant)>,
    /// Vehicle states seen in none of the seat tables, already logged.
    unknown_states: Option<HashSet<u32>>,
    visual_crouch: bool,
    shared: Option<FrameShared>,
    status: FrameStatus,
}

static FRAME: Mutex<FrameState> = Mutex::new(FrameState {
    last_at: None,
    seated_crouch: false,
    classifier: PostureClassifier::new(),
    classified: None,
    seated_reference: SeatedReference::new(),
    crouch: CrouchTracker::new(),
    seat: SeatTracker::new(),
    last_seat_event: None,
    seat_fade_release: None,
    unknown_states: None,
    visual_crouch: false,
    shared: None,
    status: FrameStatus {
        setting: Posture::Auto,
        posture: EffectivePosture::Seated,
        classifier: None,
        seated_reference: SeatedReferenceStatus {
            reference: None,
            median_m: None,
            samples: 0,
        },
        stage: StageInfo {
            available: false,
            bounds: None,
            head_height: None,
            floor_shift_m: 0.0,
            floor_jumps: 0,
        },
        head_height: None,
        cockpit_y: 0.0,
        world_scale: 1.0,
        eye_drop_m: 0.0,
        crouched: false,
        seated_crouch: false,
        seat: SeatPhase::Out,
        last_seat_event: None,
        probe: None,
    },
});

/// Track the seat phase from the body rule system's state and act on its edges.
///
/// # Safety
///
/// Must run on the game thread outside the world update: it reads the rule system's current state
/// and the interaction graph.
unsafe fn seat_tick(frame: &mut FrameState, config: &RoomscaleConfig, now: Instant) {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() };
    let state = character.and_then(|c| unsafe { active_state_type_id(c) });
    let previous = frame.seat.phase();
    let mut phase = seat::classify(state);
    let seated = matches!(previous, SeatPhase::Locked | SeatPhase::Shuffle);
    if phase == SeatPhase::Locked && !seated {
        let has_vehicle = character.is_some_and(|c| !unsafe { c.GetVehiclePtr() }.is_null());
        if !has_vehicle {
            phase = previous;
        }
    }
    if phase == SeatPhase::Out
        && previous != SeatPhase::Out
        && let (Some(state), Some(character)) = (state, character)
        && !unsafe { character.GetVehiclePtr() }.is_null()
    {
        let logged = frame.unknown_states.get_or_insert_with(HashSet::new);
        if logged.len() < UNKNOWN_STATE_LOG_LIMIT && logged.insert(state) {
            tracing::debug!(
                target: "controllers",
                state = format_args!("{state:#010x}"),
                "roomscale: a vehicle state in none of the seat tables",
            );
        }
    }
    let Some(event) = frame.seat.observe(phase) else {
        return;
    };
    frame.last_seat_event = Some(event);
    tracing::debug!(target: "controllers", ?event, ?phase, "roomscale: seat transition");
    let action = seat::action(config.seat_transition, event, config.seat_fade_hold_s);
    if let Some(kind) = action.recenter {
        vr::request_seat_recenter(kind);
    }
    if let Some(target) = action.fade {
        if action.hold_s > 0.0 {
            let hold = Duration::try_from_secs_f32(action.hold_s).unwrap_or(Duration::ZERO);
            frame.seat_fade_release = Some((target, now + hold));
        } else {
            frame.seat_fade_release = None;
            fade::set_target(FadeSource::Seat, target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recenter_on_foot_keeps_the_position() {
        let standing_out = Some((EffectivePosture::Standing, SeatPhase::Out));
        assert_eq!(
            recenter_kind(true, standing_out),
            RecenterKind::YawAndHeight
        );
        assert_eq!(recenter_kind(false, standing_out), RecenterKind::Full);
        assert_eq!(
            recenter_kind(true, Some((EffectivePosture::Seated, SeatPhase::Out))),
            RecenterKind::Full
        );
        assert_eq!(
            recenter_kind(true, Some((EffectivePosture::Standing, SeatPhase::Locked))),
            RecenterKind::Full
        );
        assert_eq!(recenter_kind(true, None), RecenterKind::Full);
    }
}
