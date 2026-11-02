//! The grounded rig in the game: the pure core (`body-rig`) advanced once per rendered frame from
//! the root's frame half, with the published results the rest of the mod reads (`docs/mod/body/rig.md`).
//!
//! The rig works in the room frame: the cockpit frame with its floor at zero height, in the player's
//! own metres. The frame half hands it the eyes and the hands in that frame, and the floor point
//! under Rico's pelvis, which is the root's cockpit point (the cockpit shift: the view stands the
//! eyes of a neutral head over the root) carried back from the eyes to the neck by the body's
//! heading. It publishes:
//!
//! - the step phase and the gap to the floor point under the neck, which the root's simulation
//!   half closes through the game's movement;
//! - the body's heading, which the yaw follow steers Rico to ([`body_yaw`]);
//! - the player's torso as the hands' shoulders and the divergence readout read it ([`torso`]);
//! - the neck pivot, the waist pivot, and the lean's ring, for the in-world diagnostics
//!   ([`diagnostics`]).
//!
//! A recenter asks the next frame to snap ([`request_snap`]): the rig faces the head and measures
//! the standing height afresh, and the root's cockpit point is put where the pelvis stands under
//! the neck, so the view moves to the body and the body never moves through a wall.

use std::time::{Duration, Instant};

use body_rig::{
    GroundedRig, PlayerBody, RigFrame, RigOutput, RigTuning, StepPhase,
    angle::{forward, head_heading, right},
    body::EYES_TO_NECK,
};
use glam::{Quat, Vec2, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    bindings::hold::Desired,
    body::{
        BodyDimensions,
        rig::torso::{HIP_SPACING_FRACTION, TorsoEstimate, wrap},
    },
    vehicles::axis_output::{read_move, write_move},
};

/// One frame's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GroundedInput {
    /// The eyes, room frame.
    pub eyes: Vec3,
    /// The head's cockpit orientation.
    pub head: Quat,
    /// The tracked grips, room frame.
    pub hands: [Option<Vec3>; 2],
    /// The root's cockpit point, `(x, z)`: the horizontal part of the cockpit shift.
    pub root: Vec2,
    /// The cockpit height of the room's floor: a room height is the cockpit height less this.
    pub floor: f32,
    /// Whether the stick moves the body.
    pub moving: bool,
    /// Whether the frame may sample the standing height: the standing posture.
    pub sample_height: bool,
    /// Whether Rico is grounded and the player owns his root.
    pub grounded: bool,
    pub dimensions: BodyDimensions,
    pub tuning: RigTuning,
    pub dt: f32,
    pub now: Instant,
}

/// One frame's result.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GroundedFrame {
    pub output: RigOutput,
    /// The floor point under Rico's pelvis, room `(x, z)`.
    pub pelvis: Vec2,
    /// Where the root's cockpit point should be put after a recenter, `(x, z)`: the upright eyes
    /// over the pelvis under the neck.
    pub snap_to: Option<Vec2>,
}

/// Advance the rig by one frame. `None` while Rico is not grounded.
pub fn frame(input: &GroundedInput) -> Option<GroundedFrame> {
    let mut state = STATE.lock();
    state.rig.set_body(player_body(&input.dimensions));
    let snap = std::mem::take(&mut state.snap)
        .filter(|at| input.now.saturating_duration_since(*at) <= SNAP_WAIT);
    if !input.grounded {
        state.rig.stand();
        if state
            .grounded_at
            .is_none_or(|at| input.now.saturating_duration_since(at) > RESUME_AFTER)
        {
            state.rig.forget();
        }
        state.latest = None;
        // A snap asked for during a hop waits for the ground; one asked for in a vehicle seat or
        // the air expires, since the recenter there re-based the whole view already.
        state.snap = snap;
        return None;
    }
    state.grounded_at = Some(input.now);
    let snap_to = if snap.is_some() {
        state.rig.recenter(input.eyes, input.head);
        state.snaps += 1;
        let yaw = state.rig.body_yaw().unwrap_or(0.0);
        let neck = body_rig::body::neck_pivot(input.eyes, input.head);
        let upright = Vec2::new(neck.x, neck.z) - arm(yaw);
        Some(upright)
    } else {
        None
    };
    let yaw = state
        .rig
        .body_yaw()
        .or_else(|| head_heading(input.head))
        .unwrap_or(0.0);
    let root = snap_to.unwrap_or(input.root);
    let pelvis = root + arm(yaw);
    let output = state.rig.update(
        &RigFrame {
            eyes: input.eyes,
            head: input.head,
            hands: input.hands,
            pelvis,
            moving: input.moving,
            sample_height: input.sample_height,
            dt: input.dt,
        },
        &input.tuning,
    );
    let result = GroundedFrame {
        output,
        pelvis,
        snap_to,
    };
    state.latest = Some(Published {
        frame: result,
        floor: input.floor,
        eyes: input.eyes,
        head: input.head,
        dimensions: input.dimensions,
        lean_radius_m: input.tuning.pelvis.lean_radius_m,
        at: input.now,
    });
    state.height_samples = state.rig.height_samples();
    log_phase(&mut state, output.phase, &output);
    Some(result)
}

/// Snap on the next grounded frame: face the head, measure the standing height afresh, and put the
/// pelvis under the neck. The recenter's, and the debug UI's button.
pub fn request_snap() {
    STATE.lock().snap = Some(Instant::now());
}

/// Turn the rig's room frame with the cockpit frame, by `turn` (new frame from old): a recenter's
/// yaw re-base or a follow bank. The body keeps facing where it faces in the world.
pub fn rotate(turn: Quat) {
    let yaw = body_rig::angle::yaw_of(turn * Vec3::NEG_Z).unwrap_or(0.0);
    STATE.lock().rig.rotate(yaw);
}

/// Forget the rig, for a teleport: the next frame starts facing the head.
pub fn forget() {
    let mut state = STATE.lock();
    state.rig.forget();
    state.latest = None;
}

/// Add a committed step's walk to the move stick the tables wrote this tick, so the game's
/// animation walks Rico while the step drive moves him, and publish what was added for the stick
/// locomotion to take back out of its velocity ([`step_stick`]). `walk_stick` is how far the walk
/// pushes the stick. The walk is in the frame of the head's heading, as the stick locomotion reads
/// the stick.
pub(crate) fn add_step_walk(desired: &mut Vec<Desired>, walk_stick: f32) {
    let now = Instant::now();
    let walk = fresh(now)
        .and_then(|published| walk_for(&published, walk_stick))
        .unwrap_or(Vec2::ZERO);
    if walk != Vec2::ZERO {
        let stick = read_move(desired);
        write_move(desired, (stick + walk).clamp_length_max(1.0));
    }
    *STEP_STICK.lock() = Some((walk, now));
}

/// The walk [`add_step_walk`] last added to the move stick, `(right, forward)`, while it is
/// recent.
pub(crate) fn step_stick() -> Vec2 {
    STEP_STICK
        .lock()
        .filter(|(_, at)| at.elapsed() <= FRESH_FOR)
        .map_or(Vec2::ZERO, |(walk, _)| walk)
}

/// The body's heading in the cockpit frame, while the rig is grounded and fresh.
pub fn body_yaw() -> Option<f32> {
    fresh(Instant::now()).map(|p| p.frame.output.body_yaw)
}

/// The player's torso in the cockpit frame, as the rig places it: the neck pivot, the spine down to
/// the waist pivot, and the girdles across it by the body's heading.
pub fn torso() -> Option<TorsoEstimate> {
    let published = fresh(Instant::now())?;
    let output = published.frame.output;
    let solve = output.solve;
    let lift = Vec3::Y * published.floor;
    let neck = solve.neck + lift;
    let waist = solve.waist + lift;
    let up = (neck - waist).try_normalize().unwrap_or(Vec3::Y);
    let d = published.dimensions;
    let below_neck = (d.eye_height + EYES_TO_NECK.y - d.shoulder_height).max(0.0);
    let chest = neck - up * below_neck;
    let side = right(output.body_yaw);
    let side = Vec3::new(side.x, 0.0, side.y);
    let half = d.shoulder_width * 0.5;
    let hip_half = half * HIP_SPACING_FRACTION;
    let head_yaw = head_heading(published.head).unwrap_or(output.body_yaw);
    Some(TorsoEstimate {
        yaw: output.body_yaw,
        head_yaw,
        neck_twist: wrap(head_yaw - output.body_yaw),
        hands_yaw: None,
        hands_weight: 0.0,
        neck,
        shoulders: [chest - side * half, chest + side * half],
        chest,
        pelvis: waist,
        hips: [waist - side * hip_half, waist + side * hip_half],
    })
}

/// What the in-world diagnostics draw, in the rig's room frame (the cockpit frame with its floor at
/// zero, in the player's metres). A point is drawn at the head's world position plus its offset
/// from [`eyes`](Self::eyes) carried into the world by the view's rotation and the world scale.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Diagnostics {
    pub eyes: Vec3,
    pub neck: Vec3,
    pub waist: Vec3,
    /// The floor point under the pelvis.
    pub pelvis: Vec3,
    /// The lean radius, metres: the ring on the floor around the pelvis.
    pub lean_radius_m: f32,
    /// How far out the neck may be as a lean at its height, metres.
    pub lean_reach_m: f32,
    pub phase: StepPhase,
}

/// The latest grounded frame's diagnostics, while fresh.
pub fn diagnostics() -> Option<Diagnostics> {
    let published = fresh(Instant::now())?;
    let solve = published.frame.output.solve;
    let pelvis = published.frame.pelvis;
    Some(Diagnostics {
        eyes: published.eyes,
        neck: solve.neck,
        waist: solve.waist,
        pelvis: Vec3::new(pelvis.x, 0.0, pelvis.y),
        lean_radius_m: published.lean_radius_m,
        lean_reach_m: solve.lean_reach_m,
        phase: published.frame.output.phase,
    })
}

/// What the rig did on its latest frames, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GroundedStatus {
    /// The latest grounded frame and when it ran.
    pub latest: Option<(GroundedFrame, Instant)>,
    /// How many standing-height samples are held.
    pub height_samples: usize,
    /// Recenter snaps since the payload loaded.
    pub snaps: u64,
}

pub fn status() -> GroundedStatus {
    let state = STATE.lock();
    GroundedStatus {
        latest: state.latest.map(|p| (p.frame, p.at)),
        height_samples: state.height_samples,
        snaps: state.snaps,
    }
}

/// The walk for a published frame: along the gap, in the head's heading's frame, while a step is
/// committed and far enough from arriving that the drive walks rather than shuffles.
fn walk_for(published: &Published, walk_stick: f32) -> Option<Vec2> {
    let output = published.frame.output;
    if output.phase != StepPhase::Stepping || output.gap.length() < WALK_GAP_M {
        return None;
    }
    let direction = output.gap.try_normalize()?;
    let yaw = head_heading(published.head)?;
    let walk = Vec2::new(direction.dot(right(yaw)), direction.dot(forward(yaw)))
        * walk_stick.clamp(0.0, 1.0);
    walk.is_finite().then_some(walk)
}

/// A step closer than this to arriving shuffles under the drive rather than walking, metres.
const WALK_GAP_M: f32 = 0.08;

/// The walk last added to the move stick, and when.
static STEP_STICK: Mutex<Option<(Vec2, Instant)>> = Mutex::new(None);

/// How long a snap waits for Rico to be grounded before it expires.
const SNAP_WAIT: Duration = Duration::from_secs(1);

/// A spell off the ground longer than this starts the rig again facing the head.
const RESUME_AFTER: Duration = Duration::from_secs(1);

/// The published frame stops standing for the rig after this long.
const FRESH_FOR: Duration = Duration::from_millis(250);

/// The player's body as the pure core measures it.
fn player_body(dimensions: &BodyDimensions) -> PlayerBody {
    PlayerBody::from_heights(dimensions.height, dimensions.eye_height)
}

/// The horizontal arm from the upright eyes back to the neck pivot for a body facing `yaw`, `(x, z)`.
fn arm(yaw: f32) -> Vec2 {
    let v = Quat::from_rotation_y(yaw) * EYES_TO_NECK;
    Vec2::new(v.x, v.z)
}

fn fresh(now: Instant) -> Option<Published> {
    STATE
        .lock()
        .latest
        .filter(|p| now.saturating_duration_since(p.at) <= FRESH_FOR)
}

/// Log each change of the step phase at debug level, with what committed it.
fn log_phase(state: &mut State, phase: StepPhase, output: &RigOutput) {
    if state.logged_phase == phase {
        return;
    }
    state.logged_phase = phase;
    tracing::debug!(
        target: "body",
        phase = phase.label(),
        demand_m = output.solve.demand_m(),
        gap_m = output.gap.length(),
        lean_reach_m = output.solve.lean_reach_m,
        bend_deg = output.solve.bend.angle().to_degrees(),
        steps = output.steps,
        "rig: the step phase changes",
    );
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Published {
    frame: GroundedFrame,
    floor: f32,
    eyes: Vec3,
    head: Quat,
    dimensions: BodyDimensions,
    lean_radius_m: f32,
    at: Instant,
}

struct State {
    rig: GroundedRig,
    latest: Option<Published>,
    grounded_at: Option<Instant>,
    /// When a snap was asked for, until it is taken or expires.
    snap: Option<Instant>,
    snaps: u64,
    height_samples: usize,
    logged_phase: StepPhase,
}

static STATE: Mutex<State> = Mutex::new(State {
    rig: GroundedRig::new(PlayerBody {
        eye_height: BodyDimensions::GENERIC_ADULT.eye_height,
        hip_height: BodyDimensions::GENERIC_ADULT.height * body_rig::body::HIP_HEIGHT_SHARE,
    }),
    latest: None,
    grounded_at: None,
    snap: None,
    snaps: 0,
    height_samples: 0,
    logged_phase: StepPhase::Idle,
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arm_points_back_from_the_eyes() {
        // Facing -Z, the neck is behind the eyes: +Z.
        let a = arm(0.0);
        assert!(a.x.abs() < 1e-6 && a.y > 0.08);
    }

    fn published(phase: StepPhase, gap: Vec2, head: Quat) -> Published {
        let mut frame = GroundedFrame::default();
        frame.output.phase = phase;
        frame.output.gap = gap;
        Published {
            frame,
            floor: 0.0,
            eyes: Vec3::ZERO,
            head,
            dimensions: BodyDimensions::GENERIC_ADULT,
            lean_radius_m: 0.2,
            at: Instant::now(),
        }
    }

    #[test]
    fn a_step_walks_along_the_gap_in_the_heads_frame() {
        // The head faces -X (a quarter turn left); a gap toward -X is straight ahead.
        let head = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let walk = walk_for(
            &published(StepPhase::Stepping, Vec2::new(-0.3, 0.0), head),
            0.5,
        )
        .unwrap();
        assert!((walk - Vec2::new(0.0, 0.5)).length() < 1e-5, "{walk}");
        // A gap to the head's right is a strafe right.
        let right = walk_for(
            &published(StepPhase::Stepping, Vec2::new(0.0, -0.3), head),
            0.5,
        )
        .unwrap();
        assert!((right - Vec2::new(0.5, 0.0)).length() < 1e-5, "{right}");
    }

    #[test]
    fn idle_settling_and_nearly_arrived_steps_do_not_walk() {
        let head = Quat::IDENTITY;
        let gap = Vec2::new(0.0, -0.3);
        assert_eq!(walk_for(&published(StepPhase::Idle, gap, head), 0.5), None);
        assert_eq!(
            walk_for(&published(StepPhase::Settling, gap, head), 0.5),
            None
        );
        let near = Vec2::new(0.0, -0.05);
        assert_eq!(
            walk_for(&published(StepPhase::Stepping, near, head), 0.5),
            None
        );
    }
}
