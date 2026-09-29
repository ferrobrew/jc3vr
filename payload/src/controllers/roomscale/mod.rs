//! Posture and roomscale: the plan of record's "Posture", "Body scale", "Roomscale design", "The
//! chase loop", "Crouch, in stages", and "Seat transitions" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`posture`]: the posture setting resolved against the play area, and the head height it
//!   implies (the body remap onto the character's eye height);
//! - [`rebase`]: the cockpit shift that keeps the camera on the physical head while the body
//!   catches up;
//! - [`chase`]: the chase loop's arithmetic;
//! - [`sim`]: the simulation-phase half: the chase velocity, the warp test, the head probe, and the
//!   crouch capsule, from the on-foot locomotion task;
//! - [`teleport`]: the teleport epoch, from the game's camera reset;
//! - [`probe`]: the head-through-geometry probe;
//! - [`fade`]: the comfort fade the VR blit applies;
//! - [`crouch`]: the visual crouch's IK targets and the capsule's proxy-state rules;
//! - [`seat`]: the seat-transition phases and modes.
//!
//! The frame half runs once per rendered VR frame from the pose composition
//! ([`compose_cockpit`], called by [`crate::vr::begin_render_frame`] on the game thread, after the
//! simulation tick and before the render update): it resolves the posture, remaps the head height,
//! folds the chase into the cockpit shift, tracks the seat phase, and advances the fade. It
//! publishes what the simulation half needs ([`FrameShared`]).
//!
//! Everything here needs the headset: it runs only while the VR source owns the head pose, and each
//! part is behind its own toggle in [`RoomscaleConfig`] (the head height behind
//! [`BodyRemapConfig::remap_head`]), all off by default, so the mod behaves as before until the
//! player opts in. None of it depends on the controller scheme, because a gamepad player standing
//! in a room has the same head.
//!
//! [`BodyRemapConfig::remap_head`]: crate::controllers::config::BodyRemapConfig::remap_head

pub mod chase;
pub mod crouch;
pub mod fade;
pub mod posture;
pub mod probe;
pub mod rebase;
pub mod seat;
pub mod sim;
pub mod teleport;

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use glam::{Quat, Vec2, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
pub use seat::SeatPhase;

use crate::{
    config::Config,
    controllers::{
        body::{self, BodyDimensions},
        config::RoomscaleConfig,
        roomscale::{
            crouch::CrouchTracker,
            fade::FadeSource,
            posture::{EffectivePosture, HeadHeight, HeadHeightInput},
            probe::ProbeResult,
            rebase::{CockpitShift, TickAnchors},
            seat::{SeatEvent, SeatTracker},
        },
    },
    headpose,
    hooks::{self, animation::active_state_type_id},
    vr::{self, RecenterKind, stage::StageInfo},
};

/// The roomscale hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(sim::hook_library())
        .with_hook_library(teleport::hook_library())
}

/// One rendered frame's inputs to the pose composition.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameInputs {
    /// The head's cockpit position (relative to the recenter baseline), metres.
    pub cockpit: Vec3,
    /// The animated head anchor this tick and the previous tick.
    pub anchor: Vec3,
    pub anchor_prev: Vec3,
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

/// The frame half: resolve the posture, remap the head height, fold the chase into the cockpit
/// shift, track the seat phase, and advance the fade, returning the cockpit positions to compose
/// with. Called once per rendered VR frame on the game thread, outside the world update.
pub fn compose_cockpit(inputs: &FrameInputs) -> ComposedCockpit {
    let (config, remap_head) =
        Config::lock_query(|c| (c.controllers.roomscale, c.controllers.body.remap_head));
    let stage = vr::stage::stage();
    let posture = posture::resolve(config.posture, stage.bounds, config.roomscale_min_extent_m);
    let now = Instant::now();
    let in_gameplay = hooks::in_gameplay();

    let mut frame = FRAME.lock();
    let dt = frame
        .last_at
        .map_or(0.0, |t| now.duration_since(t).as_secs_f32());
    if dt > FRAME_GAP.as_secs_f32() {
        fade::reset();
    }
    frame.last_at = Some(now);

    let chase_on = config.chase.enabled && posture == EffectivePosture::Roomscale;
    let epoch = teleport::epoch();
    if epoch != frame.seen_epoch {
        frame.seen_epoch = epoch;
        if chase_on {
            frame.shift.rebase_horizontal(inputs.cockpit);
            frame.teleport_rebases += 1;
        }
    }
    if chase_on {
        frame.shift.observe(
            &TickAnchors {
                anchor: inputs.anchor,
                anchor_prev: inputs.anchor_prev,
                body: inputs.body,
                world_scale: inputs.world_scale,
            },
            sim::take_share,
        );
    } else if frame.shift.current() != Vec3::ZERO {
        frame.shift.clear();
    }

    let head_height = remap_head.then(|| {
        posture::head_height(
            &HeadHeightInput {
                posture,
                cockpit_y: inputs.cockpit.y,
                stage_height: stage.head_height,
                seated_crouch: frame.seated_crouch,
                seated_drop_m: config.crouch.seated_drop_m,
            },
            &body::player(),
            &BodyDimensions::CHARACTER_ESTIMATE,
        )
    });
    let vertical = head_height.map_or(inputs.cockpit.y, |h| h.cockpit_y);
    let (mut prev, mut cur) = frame.shift.apply(inputs.cockpit);
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
    let offset = inputs.body * (Vec3::new(cur.x, 0.0, cur.z) * inputs.world_scale);
    let chase_offset = if offset.is_finite() {
        Vec2::new(offset.x, offset.z)
    } else {
        Vec2::ZERO
    };

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

    let sim = sim::status();
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
    if sim.capsule_owned && (!(crouch.capsule && crouch.visual) || sim_stale) {
        // SAFETY: the game thread, outside the world update.
        unsafe { sim::release_capsule() };
    }

    let seat = frame.seat.phase();
    frame.shared = Some(FrameShared {
        posture,
        chase_offset,
        crouched,
        eye_drop_m,
        seat,
        at: now,
    });
    frame.status = FrameStatus {
        posture,
        stage,
        head_height,
        eye_drop_m,
        chase_offset,
        cockpit_shift: frame.shift.current(),
        teleport_rebases: frame.teleport_rebases,
        crouched,
        seated_crouch: frame.seated_crouch,
        seat,
        last_seat_event: frame.last_seat_event,
        probe,
    };
    ComposedCockpit { prev, cur }
}

/// Re-base the cockpit shift after the recenter baseline was re-based: a full recenter makes the
/// head the cockpit origin, so the shift is spent; a yaw-and-height recenter keeps the horizontal
/// position and turns the cockpit frame by `turn` (new frame from old). Called by
/// [`crate::vr::recenter`].
pub fn on_recenter(kind: RecenterKind, turn: Quat) {
    let mut frame = FRAME.lock();
    match kind {
        RecenterKind::Full => frame.shift.clear(),
        RecenterKind::YawAndHeight => frame.shift.rotate(turn),
    }
}

/// Toggle the seated crouch. Acts only in the seated posture, where the eyes have a crouched preset;
/// standing and roomscale players crouch physically.
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

/// What the frame half publishes for the simulation half.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameShared {
    pub posture: EffectivePosture,
    /// The physical head's horizontal offset from the body, world `(x, z)`, metres.
    pub chase_offset: Vec2,
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
    pub sim: sim::SimStatus,
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
    pub posture: EffectivePosture,
    pub stage: StageInfo,
    /// The remapped head height, while the head remap is on.
    pub head_height: Option<HeadHeight>,
    pub eye_drop_m: f32,
    pub chase_offset: Vec2,
    /// The cockpit shift, cockpit frame, metres.
    pub cockpit_shift: Vec3,
    /// Teleports that re-based the horizontal offset.
    pub teleport_rebases: u64,
    pub crouched: bool,
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
        sim: sim::status(),
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
    seen_epoch: u64,
    shift: CockpitShift,
    teleport_rebases: u64,
    seated_crouch: bool,
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
    seen_epoch: 0,
    shift: CockpitShift::new(),
    teleport_rebases: 0,
    seated_crouch: false,
    crouch: CrouchTracker::new(),
    seat: SeatTracker::new(),
    last_seat_event: None,
    seat_fade_release: None,
    unknown_states: None,
    visual_crouch: false,
    shared: None,
    status: FrameStatus {
        posture: EffectivePosture::Standing,
        stage: StageInfo {
            available: false,
            bounds: None,
            head_height: None,
        },
        head_height: None,
        eye_drop_m: 0.0,
        chase_offset: Vec2::ZERO,
        cockpit_shift: Vec3::ZERO,
        teleport_rebases: 0,
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
        vr::request_recenter(kind);
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
