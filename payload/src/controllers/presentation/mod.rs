//! The render-rate presentation layer: the player's upper body re-posed every rendered frame from
//! the latest tracking, on top of the pose the simulation wrote a tick before. The design, and what
//! is drawn at the render rate and what at the simulation's, is in
//! `docs/mod/rendering/presentation-layer.md`.
//!
//! The simulation poses Rico at 30 Hz: the procedural body and the hand IK solve at the pass's
//! solve gate, the head override writes the head after it, and the skinning draws each frame
//! between the last two ticks' poses. The headset and the late update's hands move at the display's
//! rate, so the torso, the shoulders, and the arms under them trail the player by a tick. Here:
//!
//! - [`after_solve`] records, at the end of each tick's pose pass, the upper body's joints as the
//!   tick left them (model space) and the head pose the tick posed it from;
//! - [`begin_frame`] snapshots the last two ticks' records, the render-time head pose pair, and the
//!   settings, once per rendered frame;
//! - [`draw_edit`], called for every draw of the local player's body, interpolates the two ticks'
//!   joints by the frame's interpolation fraction (as the skinning does), measures the head's motion
//!   since them and takes each hand's correction from the late update
//!   ([`late::hand_correction`]), and solves the upper body again ([`solve::solve`]). The result
//!   is a rigid edit per bone group that the draw pre-multiplies onto its copy of the palette, so
//!   the simulation's pose, the hit shapes, and every other reader of the skeleton stay as the
//!   tick left them, and other characters are never touched.
//!
//! The legs and the hips are not re-posed: they stand on the world, and the root's interpolation
//! carries them. When the layer has nothing to pose from (it is off, outside the headset, or no
//! tick has been recorded), the draw falls back to the late update's own edit of the hands and the
//! forearms ([`late::palette_edit`]).
//!
//! Threading: [`after_solve`] runs on whichever thread finalizes the player's pose, [`begin_frame`]
//! on the game thread at the top of the render update, and [`draw_edit`] on the draw workers. Each
//! shares state only through leaf locks it holds briefly, and none takes the VR runtime lock
//! (`docs/mod/threading.md`).

mod config;
mod edit;
pub mod rig;
pub mod solve;

use std::{
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Vec3};
use jc3gi::character::character::{AnimationController, Character, Joint};
use parking_lot::Mutex;
use smol_str::{SmolStr, ToSmolStr};

pub use crate::controllers::presentation::{
    config::PresentationConfig,
    edit::{BodyEdit, DrawEdit},
};
use crate::{
    config::Config,
    controllers::{
        body::{
            procedural::{
                self,
                skeleton::{self, SkeletonKey},
            },
            rig::torso::EYES_TO_NECK_PIVOT,
        },
        frame::{Hand, Pose},
        hands::{late, pose as hand_pose},
        presentation::{
            rig::{Group, GroupMap, PresentationRig},
            solve::{ArmJoints, Motion, UpperBody, head_motion, solve},
        },
    },
    frame_timing::{self, Slot},
    headpose, hooks,
};

/// Why the layer is or is not re-posing the body.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum Idle {
    /// It re-poses the body.
    #[default]
    Active,
    /// Switched off in the settings.
    Off,
    /// Not in the headset, or not in gameplay.
    NotInHeadset,
    /// No recent tick recorded the body: the player's pose pass has not run lately.
    NoTick,
    /// The skeleton has none of the bones the layer re-poses ([`PresentationStatus::rig_error`]).
    NoRig,
}

impl Idle {
    const ALL: [Self; 5] = [
        Self::Active,
        Self::Off,
        Self::NotInHeadset,
        Self::NoTick,
        Self::NoRig,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Off => "off",
            Self::NotInHeadset => "not in the headset",
            Self::NoTick => "no recent tick",
            Self::NoRig => "no rig",
        }
    }
}

/// The layer's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PresentationStatus {
    pub idle: Idle,
    /// Whether the layer has re-posed the body in any frame since the payload loaded; the counts
    /// and the costs below are from the last such frame.
    pub measured: bool,
    /// Draws of the player's body the layer re-posed in the last active frame.
    pub draws: u32,
    /// The layer's own time in the last active frame, microseconds: the frame's snapshot and every
    /// draw's solve, on whichever threads they ran.
    pub frame_cost_us: f32,
    /// The same, smoothed over about a second of active frames.
    pub mean_cost_us: f32,
    /// The costliest active frame since the payload loaded, microseconds.
    pub peak_cost_us: f32,
    /// Ticks recorded since the payload loaded.
    pub ticks: u64,
    /// Why the skeleton has no rig, when it has none.
    pub rig_error: Option<SmolStr>,
}

/// The layer's readout. A leaf lock and atomics, so any thread may read it.
pub fn status() -> PresentationStatus {
    PresentationStatus {
        idle: Idle::ALL
            .get(IDLE.load(Ordering::Relaxed) as usize)
            .copied()
            .unwrap_or_default(),
        measured: COST_SEEDED.load(Ordering::Relaxed),
        draws: LAST_DRAWS.load(Ordering::Relaxed),
        frame_cost_us: f32::from_bits(LAST_COST_US.load(Ordering::Relaxed)),
        mean_cost_us: f32::from_bits(MEAN_COST_US.load(Ordering::Relaxed)),
        peak_cost_us: f32::from_bits(PEAK_COST_US.load(Ordering::Relaxed)),
        ticks: TICK_COUNT.load(Ordering::Relaxed),
        rig_error: RIG
            .lock()
            .as_ref()
            .and_then(|(_, rig)| rig.as_ref().err().cloned()),
    }
}

/// Record the upper body as this tick's pose pass left it, and the head pose it was posed from.
/// Called at the end of the pose-finalization pass, after the head override and the post-solve
/// fix-ups.
///
/// # Safety
///
/// `character` must be null or a live character; called from the post-call `UpdatePropEffects`
/// detour, on the thread finalizing its pose.
pub(crate) unsafe fn after_solve(character: *mut Character) {
    let Some(character) = (unsafe { character.as_ref() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() })
    else {
        return;
    };
    // SAFETY: the live controller, on the thread finalizing its character's pose.
    let Some(key) = (unsafe { SkeletonKey::of(controller) }) else {
        return;
    };
    let Some(rig) = (unsafe { rig_for(controller, key) }) else {
        return;
    };
    // SAFETY: as above; every root index is below the skeleton's bone count.
    let body = unsafe { read_body(controller, &rig, key) };
    let Some(body) = body.filter(UpperBody::is_finite) else {
        return;
    };
    let head = headpose::query();
    let pose = TickPose {
        body,
        head: Pose {
            position: head.position,
            orientation: head.orientation,
        },
        torso: procedural::torso_weight(),
        key,
        groups: rig.groups,
        at: Instant::now(),
    };
    let mut ticks = TICKS.lock();
    ticks.previous = ticks.current.filter(|previous| previous.key == key);
    ticks.current = Some(pose);
    TICK_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Snapshot the frame's inputs: the last two ticks, the render-time head pose pair, and the
/// settings. Called once per rendered frame from `Game::UpdateRender`, after the head pose pair is
/// published and before the engine's render update and the draws.
pub(crate) fn begin_frame() {
    let start = Instant::now();
    let _timing = frame_timing::scope(Slot::Presentation);
    roll_cost();
    let (config, reach, world_scale) = Config::lock_query(|c| {
        (
            c.controllers.hands.presentation,
            c.controllers.hands.reach,
            c.vr.world_scale,
        )
    });
    let rig_failed = RIG.lock().as_ref().is_some_and(|(_, rig)| rig.is_err());
    let ticks = *TICKS.lock();
    let fresh = ticks
        .current
        .filter(|t| start.saturating_duration_since(t.at) <= STALE_AFTER);
    let (idle, frame) = if !config.enabled {
        (Idle::Off, None)
    } else if headpose::source() != headpose::Source::Vr || !hooks::in_gameplay() {
        (Idle::NotInHeadset, None)
    } else if rig_failed {
        (Idle::NoRig, None)
    } else if let Some(current) = fresh {
        let (previous, current) = (ticks.previous.unwrap_or(current), current);
        let scale = if world_scale.is_finite() && world_scale > 0.0 {
            world_scale
        } else {
            1.0
        };
        let head = |pose: headpose::HeadPose| Pose {
            position: pose.position,
            orientation: pose.orientation,
        };
        let frame = Frame {
            config,
            stretch_forearm: reach.detach && reach.stretch_forearm,
            pivot_offset: EYES_TO_NECK_PIVOT * scale,
            head: [head(headpose::query_prev()), head(headpose::query())],
            ticks: [previous, current],
            sequence: SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1,
        };
        (Idle::Active, Some(frame))
    } else {
        (Idle::NoTick, None)
    };
    IDLE.store(idle as u8, Ordering::Relaxed);
    *FRAME.lock() = frame;
    add_cost(start);
}

/// The palette edit for a draw of the local player's body whose instance matrix is `instance`, at
/// the interpolation fraction `dtf`: the upper body re-posed when the layer is active, or else the
/// late update's hands; `None` when nothing is re-posed.
pub(crate) fn draw_edit(instance: Mat4, dtf: f32) -> Option<DrawEdit> {
    let start = Instant::now();
    let _timing = frame_timing::scope(Slot::Presentation);
    let body = body_edit(instance, dtf);
    if body.is_some() {
        DRAWS.fetch_add(1, Ordering::Relaxed);
    }
    let edit = body
        .map(DrawEdit::Body)
        .or_else(|| late::palette_edit(instance, dtf).map(DrawEdit::Hands));
    add_cost(start);
    edit
}

/// One tick's record.
#[derive(Debug, Copy, Clone, PartialEq)]
struct TickPose {
    body: UpperBody,
    /// The head pose the tick posed the body from, world space.
    head: Pose,
    /// The procedural torso's weight at the tick.
    torso: f32,
    key: SkeletonKey,
    groups: GroupMap,
    at: Instant,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Ticks {
    previous: Option<TickPose>,
    current: Option<TickPose>,
}

/// The frame's snapshot.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Frame {
    config: PresentationConfig,
    stretch_forearm: bool,
    /// The neck pivot from the eyes, in the head's frame, at the world scale.
    pivot_offset: Vec3,
    /// The render-time head pose pair, previous then current, as the camera interpolates it.
    head: [Pose; 2],
    /// The last two ticks, previous then current.
    ticks: [TickPose; 2],
    /// Which frame this is, for the draws' cache.
    sequence: u64,
}

/// The rig of the skeleton `key` names, built from the skeleton the first time it is seen. A
/// skeleton without the bones is logged once and refused.
///
/// # Safety
///
/// `controller` must be the live animation controller `key` was read from, on the thread
/// finalizing its character's pose.
unsafe fn rig_for(controller: &AnimationController, key: SkeletonKey) -> Option<PresentationRig> {
    let mut cached = RIG.lock();
    if let Some((cached_key, rig)) = cached.as_ref()
        && *cached_key == key
    {
        return rig.as_ref().ok().copied();
    }
    // SAFETY: forwarded from the caller.
    let rig = match unsafe { skeleton::read(controller) } {
        Ok(snapshot) => {
            let names: Vec<SmolStr> = snapshot.bones.iter().map(|b| b.name.clone()).collect();
            let parents: Vec<Option<usize>> = snapshot.bones.iter().map(|b| b.parent).collect();
            PresentationRig::build(&names, &parents).map_err(|e| e.to_smolstr())
        }
        Err(e) => Err(e.to_smolstr()),
    };
    match &rig {
        Ok(_) => tracing::info!(
            target: "body",
            bones = key.bones(),
            "presentation: rigged the player's skeleton for the render-rate body",
        ),
        Err(error) => tracing::warn!(target: "body", "presentation: {error}"),
    }
    *cached = Some((key, rig.clone()));
    rig.ok()
}

/// The upper body's joints as `controller` holds them, model space.
///
/// # Safety
///
/// As [`rig_for`], with `rig` built for `key`.
unsafe fn read_body(
    controller: &AnimationController,
    rig: &PresentationRig,
    key: SkeletonKey,
) -> Option<UpperBody> {
    let read = |group: Group| {
        let bone = rig.roots[group.index()];
        (bone < key.bones()).then(|| {
            let mut joint = Joint::default();
            // SAFETY: forwarded from the caller; the index is within the skeleton.
            unsafe { controller.GetJoint(bone as u32, &mut joint) };
            Vec3::from_array(joint.m_Translation.data)
        })
    };
    let arm = |side: usize| {
        let [upper, fore, hand] = Group::arm(side);
        Some(ArmJoints {
            shoulder: read(upper)?,
            elbow: read(fore)?,
            wrist: read(hand)?,
        })
    };
    Some(UpperBody {
        spine: [
            read(Group::Spine1)?,
            read(Group::Spine2)?,
            read(Group::Sternum)?,
            read(Group::Neck)?,
        ],
        head: read(Group::Head)?,
        arms: [arm(0)?, arm(1)?],
    })
}

/// The upper body's edit for a draw, from the frame's snapshot; cached for the frame's other draws
/// with the same instance matrix and fraction.
fn body_edit(instance: Mat4, dtf: f32) -> Option<BodyEdit> {
    let frame = (*FRAME.lock())?;
    let key = DrawKey {
        sequence: frame.sequence,
        dtf: dtf.to_bits(),
        instance: instance.to_cols_array().map(f32::to_bits),
    };
    if let Some((cached, edit)) = *CACHE.lock()
        && cached == key
    {
        return edit;
    }
    let edit = solve_frame(&frame, instance, dtf);
    *CACHE.lock() = Some((key, edit));
    edit
}

/// Solve the frame's upper body for a draw whose instance matrix is `instance` at `dtf`.
fn solve_frame(frame: &Frame, instance: Mat4, dtf: f32) -> Option<BodyEdit> {
    let to_model = instance.inverse();
    if !(to_model.is_finite() && dtf.is_finite()) {
        return None;
    }
    let t = dtf.clamp(0.0, 1.0);
    let model = |world: Mat4| to_model * world * instance;
    let [previous, current] = &frame.ticks;
    let body = previous.body.lerp(&current.body, t);
    let head = frame
        .config
        .torso
        .then(|| {
            let pivot = |pose: Pose| Pose {
                position: pose.position + pose.orientation * frame.pivot_offset,
                orientation: pose.orientation,
            };
            let now = pivot(hand_pose::interp(frame.head[0], frame.head[1], t));
            let reference = pivot(hand_pose::interp(previous.head, current.head, t));
            late::plausible(now, reference).then(|| {
                model(head_motion(
                    reference.position,
                    reference.orientation,
                    now.position,
                    now.orientation,
                ))
            })
        })
        .flatten();
    let motion = Motion {
        head,
        torso: previous.torso + (current.torso - previous.torso) * t,
        hands: Hand::BOTH.map(|hand| late::hand_correction(hand, t).map(model)),
        arms: frame.config.arms,
        stretch_forearm: frame.stretch_forearm,
    };
    let edits = solve(&body, &motion);
    edits
        .iter()
        .all(|e| e.is_finite())
        .then(|| BodyEdit::new(current.groups, edits))
        .flatten()
}

/// What a draw's cached edit was solved for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct DrawKey {
    sequence: u64,
    dtf: u32,
    instance: [u32; 16],
}

/// Close the last frame's cost: its total, the smoothed mean, and the peak, kept from the last
/// frame the layer re-posed the body in, so the readout still shows them while the pause menu
/// stops the gameplay.
fn roll_cost() {
    let ns = FRAME_NS.swap(0, Ordering::Relaxed);
    let draws = DRAWS.swap(0, Ordering::Relaxed);
    if IDLE.load(Ordering::Relaxed) != Idle::Active as u8 {
        return;
    }
    LAST_DRAWS.store(draws, Ordering::Relaxed);
    let us = ns as f32 / 1000.0;
    LAST_COST_US.store(us.to_bits(), Ordering::Relaxed);
    let mean = f32::from_bits(MEAN_COST_US.load(Ordering::Relaxed));
    let mean = if COST_SEEDED.swap(true, Ordering::Relaxed) {
        mean + (us - mean) * COST_SMOOTHING
    } else {
        us
    };
    MEAN_COST_US.store(mean.to_bits(), Ordering::Relaxed);
    let peak = f32::from_bits(PEAK_COST_US.load(Ordering::Relaxed));
    if us > peak {
        PEAK_COST_US.store(us.to_bits(), Ordering::Relaxed);
    }
}

fn add_cost(start: Instant) {
    let ns = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    FRAME_NS.fetch_add(ns, Ordering::Relaxed);
}

/// How old the latest tick may be for the layer to pose from it: a few ticks. Past it the pose
/// pass has stopped running for the player (a cutscene, a load), and the body is left as drawn.
const STALE_AFTER: Duration = Duration::from_millis(250);

/// The share of each frame's cost the smoothed mean takes: about a second's worth of frames.
const COST_SMOOTHING: f32 = 1.0 / 90.0;

static TICKS: Mutex<Ticks> = Mutex::new(Ticks {
    previous: None,
    current: None,
});
static RIG: Mutex<Option<(SkeletonKey, Result<PresentationRig, SmolStr>)>> = Mutex::new(None);
static FRAME: Mutex<Option<Frame>> = Mutex::new(None);
static CACHE: Mutex<Option<(DrawKey, Option<BodyEdit>)>> = Mutex::new(None);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static IDLE: AtomicU8 = AtomicU8::new(Idle::NoTick as u8);
static TICK_COUNT: AtomicU64 = AtomicU64::new(0);
static FRAME_NS: AtomicU64 = AtomicU64::new(0);
static DRAWS: AtomicU32 = AtomicU32::new(0);
static LAST_DRAWS: AtomicU32 = AtomicU32::new(0);
static LAST_COST_US: AtomicU32 = AtomicU32::new(0);
static MEAN_COST_US: AtomicU32 = AtomicU32::new(0);
static PEAK_COST_US: AtomicU32 = AtomicU32::new(0);
static COST_SEEDED: AtomicBool = AtomicBool::new(false);
