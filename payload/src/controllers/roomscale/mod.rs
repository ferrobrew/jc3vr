//! Posture and roomscale: the plan of record's "Posture", "Body scale", "Roomscale design", "Crouch,
//! in stages", and "Seat transitions" (`docs/mod/input/controllers-and-roomscale.md`). The body
//! following the head about the room is the rig's ([`crate::controllers::body::rig::root`],
//! `docs/mod/body/rig.md`).
//!
//! - [`posture`]: the posture setting resolved against the head height, and the head height it
//!   implies (the body remap onto the character's eye height);
//! - [`seated`]: the seated eye reference, which follows how the player actually sits;
//! - [`teleport`]: the teleport epoch, from the game's camera reset;
//! - [`fade`]: the comfort fade the VR blit applies;
//! - [`crouch`]: whether the character counts as crouched, and the capsule's proxy-state rules;
//! - [`seat`]: the seat-transition phases and modes.
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
pub mod seat;
pub mod seated;
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
            rig::{
                collision::probe::{self, ProbeResult},
                grounded,
                root::{self as rig_root, FrameSourceSnapshot, RootStatus, capsule_enabled},
            },
        },
        config::{Posture, RoomscaleConfig},
        frame as controllers_frame, locomotion,
        roomscale::{
            crouch::{CROUCH_EXIT_DROP_M, CrouchTracker},
            fade::{FADE_TIMING, FadeSource},
            posture::{
                AUTO_POSTURE, ClassifierInput, ClassifierStatus, DetectedPosture, EffectivePosture,
                HeadHeight, HeadHeightInput, PostureClassifier, SEATED_CROUCH_DROP_M,
            },
            seat::{SEAT_FADE_HOLD_S, SeatEvent, SeatTracker},
            seated::{SEATED_REFERENCE, SeatedReference, SeatedReferenceStatus},
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
    pub(crate) source: FrameSourceSnapshot,
}

/// The frame half: resolve the posture, remap the head height, hand the cockpit to the rig's
/// frame half for the shift, track the seat phase, advance the fade, and add the driving view's
/// correction, returning the cockpit positions to compose with. Called once per rendered VR frame
/// on the game thread, outside the world update.
pub fn compose_cockpit(inputs: &FrameInputs) -> ComposedCockpit {
    let (config, rig, remap_head, driving_view_on) = Config::lock_query(|c| {
        (
            c.controllers.roomscale,
            c.controllers.body.rig,
            c.controllers.body.remap_head,
            c.controllers.vehicles.driving_view.enabled,
        )
    });
    let stage = vr::stage::stage();
    let player = body::player();
    let now = Instant::now();
    let in_gameplay = hooks::in_gameplay();
    let on_foot = headpose::sim::mode() == headpose::sim::HeadMode::OnFoot;
    let moving = {
        let stick = locomotion::status();
        stick.fresh(now) && stick.stick.length() > MOVING_STICK
    };

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
                .update(&classifier_input(&stage, &player), dt, &AUTO_POSTURE),
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
            &SEATED_REFERENCE,
        )
    } else {
        frame.seated_reference.current()
    };
    // The seated crouch is Rico crouching on foot; in a vehicle seat it would drop the view into
    // the footwell, and under a canopy, in the wingsuit, or swimming into his torso. The toggle
    // keeps its state for when he is back on foot.
    let seated_crouch = frame.seated_crouch && on_foot && frame.seat.phase() == SeatPhase::Out;

    // Standing, the remap measures the player's eyes from the standing eye height the rig has
    // measured rather than the calibration's, which stood this player's eyes several centimetres
    // under Rico's.
    let remap_player = BodyDimensions {
        eye_height: measured_eye_height(&player, posture),
        ..player
    };
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
                seated_drop_m: SEATED_CROUCH_DROP_M,
            },
            &remap_player,
            &BodyDimensions::CHARACTER_ESTIMATE,
        )
    });
    // Without the head remap the seated crouch still lowers the eyes.
    let vertical = head_height.map_or_else(
        || {
            inputs.cockpit.y
                - posture::seated_crouch_drop(posture, seated_crouch, SEATED_CROUCH_DROP_M)
        },
        |h| h.cockpit_y,
    );
    // The rig's room frame: the cockpit frame with the floor at zero, in the player's metres. The
    // eyes' height is over the STAGE floor standing, the seated reference's travel over the
    // standing eye height seated, and the travel from the recenter's otherwise.
    let eye_height = match (posture, stage.head_height, seated_reference) {
        (EffectivePosture::Standing, Some(height), _) => height,
        (EffectivePosture::Seated, Some(height), Some(reference)) => {
            player.eye_height + height - reference.height_m
        }
        _ => player.eye_height + inputs.cockpit.y,
    };
    let lift = eye_height - inputs.cockpit.y;
    let hands = room_hands(lift);
    let root = rig_root::frame(&rig_root::FrameInputs {
        raw: Vec3::new(inputs.cockpit.x, vertical, inputs.cockpit.z),
        head: inputs.orientation,
        view: inputs.body,
        world_scale: inputs.world_scale,
        posture,
        seat: frame.seat.phase(),
        eyes: Vec3::new(inputs.cockpit.x, eye_height, inputs.cockpit.z),
        floor: -lift,
        hands,
        moving,
        dimensions: player,
        dt,
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
    // Standing on the ground, a crouch is the head's drop that no bend at the waist explains, so a
    // look at the floor or a lean over a table does not lower the capsule.
    let eye_drop_m = match (posture, rig_crouch()) {
        (EffectivePosture::Standing, Some(crouch_m)) => {
            let ratio = BodyDimensions::CHARACTER_ESTIMATE.eye_height / player.eye_height;
            let drop = crouch_m * ratio * inputs.world_scale;
            if drop.is_finite() {
                drop.max(0.0)
            } else {
                eye_drop_m
            }
        }
        _ => eye_drop_m,
    };
    let crouched = frame
        .crouch
        .update(eye_drop_m, config.crouch.enter_drop_m, CROUCH_EXIT_DROP_M);

    if in_gameplay {
        // SAFETY: the game thread, outside the world update.
        unsafe { seat_tick(&mut frame, &config, now) };
        let take_off = Config::lock_query(|c| c.controllers.vehicles.take_off_to_head);
        // SAFETY: as above.
        unsafe { crate::controllers::vehicles::take_off::tick(take_off) };
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
        Some(result) if config.head_fade.enabled && in_gameplay => probe::fade_for_depth(
            result.depth(),
            probe::HEAD_FADE_START_DEPTH_M,
            probe::HEAD_FADE_FULL_DEPTH_M,
        ),
        _ => 0.0,
    };
    fade::set_target(FadeSource::HeadInGeometry, head_fade);
    fade::advance(dt, &FADE_TIMING);

    let sim_stale = sim
        .at
        .is_none_or(|at| now.duration_since(at) > probe::STALE_AFTER);
    if sim.capsule_owned && (!capsule_enabled(&config) || sim_stale) {
        // SAFETY: the game thread, outside the world update.
        unsafe { rig_root::release_capsule() };
    }

    let seat = frame.seat.phase();
    // Added last, so the posture, the seated reference, the crouch, and the rig never see it.
    // A driving state missing from the seat tables reads as out, so only a transition holds the
    // correction back.
    let driving_view = driving_view::advance(
        driving_view_on,
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
        source: root.source,
    }
}

/// The standing eye height the head remap measures the player's eyes from: the grounded rig's
/// measure standing, and the calibration's otherwise (seated, the seated reference carries the
/// height instead).
fn measured_eye_height(player: &BodyDimensions, posture: EffectivePosture) -> f32 {
    let measured = grounded::standing_eye_height();
    match posture {
        EffectivePosture::Standing if measured.is_finite() && measured > 0.0 => measured,
        _ => player.eye_height,
    }
}

/// The crouch the grounded rig measured on this frame, player metres, while Rico is grounded.
fn rig_crouch() -> Option<f32> {
    rig_root::status()
        .frame
        .rig
        .map(|rig| rig.output.solve.crouch_m)
}

/// The tracked grips in the rig's room frame: the cockpit grips lifted by `lift`, the room height
/// of the cockpit's origin.
fn room_hands(lift: f32) -> [Option<Vec3>; 2] {
    controllers_frame::with_latest(|frame| {
        frame.hands.each_ref().map(|hand| {
            (frame.focused && hand.active && hand.grip.is_tracked() && hand.grip.is_valid())
                .then(|| hand.grip.pose.position + Vec3::Y * lift)
                .filter(|p| p.is_finite())
        })
    })
    .unwrap_or([None, None])
}

/// The move stick past this deflection moves the body, for the yaw's deadzone.
const MOVING_STICK: f32 = 0.2;

/// Re-base the cockpit shift after the recenter baseline was re-based
/// ([`crate::controllers::body::rig::root::on_recenter`]). Called by the recenter at the top of the
/// frame ([`crate::vr::auto_recenter_tick`]).
///
/// A manual recenter is also the player saying "this is how I am": in the automatic posture it
/// classifies seated or standing at once from the head height, and while seated it captures the
/// head height as the seated eye reference ([`SeatedReference::capture`]). A seat transition's
/// re-base and a follow bank do neither, because they happen on the game's schedule rather than the
/// player's.
pub fn on_recenter(kind: RecenterKind, turn: Quat, cause: RecenterCause) {
    let setting = Config::lock_query(|c| c.controllers.roomscale.posture);
    let stage = vr::stage::stage();
    let player = body::player();
    rig_root::on_recenter(kind, turn, cause);
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
/// gameplay-resume recenter) re-bases: yaw and height out of a seat, where the body rig then snaps
/// the pelvis under the neck by re-basing the cockpit shift, and the full re-base otherwise
/// ([`recenter_kind`]).
pub fn manual_recenter_kind() -> RecenterKind {
    let frame = frame_shared().filter(|f| f.fresh(Instant::now()));
    recenter_kind(frame.map(|f| f.seat))
}

/// The recenter for the latest frame's seat phase: out of a seat, only the yaw and the height need
/// re-basing, since the rig's snap puts the body under the head; in a seat, or without a recent
/// frame where nothing is known, the full re-base.
pub fn recenter_kind(seat: Option<SeatPhase>) -> RecenterKind {
    match seat {
        Some(SeatPhase::Out) => RecenterKind::YawAndHeight,
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
    /// The rig's root: the owner, the step drive, the shift, the probe, and the capsule.
    pub root: RootStatus,
    pub fade: fade::FadeStatus,
    /// Teleports seen, and how many came from the distance test.
    pub teleports: u64,
    pub warps: u64,
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
    let (frame, active) = {
        let f = FRAME.lock();
        let active = f.shared.is_some_and(|s| s.fresh(Instant::now()));
        (f.status, active)
    };
    RoomscaleStatus {
        active,
        frame,
        root: rig_root::status(),
        fade: fade::status(),
        teleports: teleport::epoch(),
        warps: teleport::warps(),
    }
}

/// A gap between rendered frames longer than this means the headset stopped rendering: the fade
/// resets, and the frame half's values stop holding for the simulation half.
const FRAME_GAP: Duration = Duration::from_millis(500);

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
    let action = seat::action(config.seat_transition, event, SEAT_FADE_HOLD_S);
    if let Some(kind) = action.recenter {
        vr::request_seat_recenter(kind);
    }
    if action.rebase_yaw {
        vr::request_seat_yaw_rebase();
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
        assert_eq!(
            recenter_kind(Some(SeatPhase::Out)),
            RecenterKind::YawAndHeight
        );
        for seat in [
            SeatPhase::Entering,
            SeatPhase::Locked,
            SeatPhase::Shuffle,
            SeatPhase::Exiting,
        ] {
            assert_eq!(recenter_kind(Some(seat)), RecenterKind::Full);
        }
        assert_eq!(recenter_kind(None), RecenterKind::Full);
    }
}
