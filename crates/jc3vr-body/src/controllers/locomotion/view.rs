//! The stable view anchor's pure core.
//!
//! On foot the camera hangs from an anchor, the head bone's world position, and the headset's
//! motion is added onto it. Taken from the animated head, the anchor carries every bob, lean, and
//! start or stop lurch of the clips into the view. Fixed to Rico's root instead, the anchor is the
//! root plus a neutral standing head measured once from the idle animation, so the view moves only
//! when the root does: at the stick's velocity, smoothly, and as the player's own head moves.
//!
//! - [`NeutralCapture`] measures the neutral head, neck, and eyes in model space, averaging the
//!   animated pose over a stretch of standing still.
//! - [`HeightFilter`] smooths the root's height while grounded, so a staircase's steps read as a
//!   ramp, while a drop or a teleport passes at once.
//! - [`Handover`] eases the anchor between the animated head and the root over a short window, so
//!   leaving the ground or entering a vehicle never jumps the view.
//! - [`stable_anchors`] composes the anchors from the neutral pose and the root's two tick-spaced
//!   transforms, [`neck_model_anchors`] hangs the headset's eyes from them by the tracked neck
//!   model, and [`blend`] mixes them with the animated ones.

use body_rig::{
    body::EYES_TO_NECK,
    smooth::{critically_damped, smoothstep},
};
use glam::{Mat4, Vec3};
use jc3vr_view::headpose::Anchors;

use crate::controllers::{body::procedural::authority::Case, locomotion::tuning::LOCOMOTION};

/// The neutral standing head, neck, and eye midpoint, in Rico's model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Neutral {
    pub head: Vec3,
    pub neck: Vec3,
    pub eyes: Vec3,
}

impl Neutral {
    fn is_plausible(&self) -> bool {
        self.head.is_finite()
            && self.neck.is_finite()
            && self.eyes.is_finite()
            && (MIN_HEAD_HEIGHT_M..=MAX_HEAD_HEIGHT_M).contains(&self.head.y)
            && self.head.distance(self.neck) < MAX_BONE_SPREAD_M
            && self.eyes.distance(self.neck) < MAX_BONE_SPREAD_M
    }
}

/// Averages the animated head, neck, and eyes over the time Rico stands still.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct NeutralCapture {
    sum: [Vec3; 3],
    seconds: f32,
}

impl NeutralCapture {
    pub const fn new() -> Self {
        Self {
            sum: [Vec3::ZERO; 3],
            seconds: 0.0,
        }
    }

    /// Add a sample held for `dt` seconds. Returns the neutral pose once `capture_s` seconds have
    /// been sampled, and starts over. An implausible sample restarts the capture.
    pub fn push(&mut self, sample: Neutral, dt: f32, capture_s: f32) -> Option<Neutral> {
        if !sample.is_plausible() || !dt.is_finite() || dt <= 0.0 {
            *self = Self::new();
            return None;
        }
        self.sum[0] += sample.head * dt;
        self.sum[1] += sample.neck * dt;
        self.sum[2] += sample.eyes * dt;
        self.seconds += dt;
        if self.seconds < capture_s {
            return None;
        }
        let neutral = Neutral {
            head: self.sum[0] / self.seconds,
            neck: self.sum[1] / self.seconds,
            eyes: self.sum[2] / self.seconds,
        };
        *self = Self::new();
        neutral.is_plausible().then_some(neutral)
    }

    /// Drop the samples so far: Rico moved.
    pub fn interrupt(&mut self) {
        *self = Self::new();
    }

    /// The seconds sampled so far.
    pub fn progress(&self) -> f32 {
        self.seconds
    }
}

/// A critically damped follower of the root's height.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HeightFilter {
    state: HeightState,
    /// The height the last step returned.
    last: Option<f32>,
}

/// What the height filter is doing.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
enum HeightState {
    /// Not following: the height is the root's.
    #[default]
    Idle,
    /// Following the root on the ground: the smoothed height and its rate.
    Following { height: f32, rate: f32 },
    /// Off the ground, winding what the smoothing still hid down to nothing: the offset from the
    /// root and how fast it shrinks, metres per second.
    Releasing { offset: f32, rate: f32 },
}

/// The smoothed height at the previous and the current tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeightPair {
    pub previous: f32,
    pub current: f32,
}

impl HeightFilter {
    pub const fn new() -> Self {
        Self {
            state: HeightState::Idle,
            last: None,
        }
    }

    /// Follow the root height `raw` for one tick of `dt` seconds. `supported` says the body stands
    /// on the ground this tick. Off it, what the smoothing still hides winds down to nothing over
    /// the locomotion tuning's `height.release_s`, so leaving the ground never jumps the view; a change beyond the snap distance
    /// (a drop, a teleport) passes at once.
    pub fn step(&mut self, raw: f32, dt: f32, supported: bool) -> HeightPair {
        let previous = self.last.unwrap_or(raw);
        let current = self.advance(raw, dt, supported);
        self.last = Some(current);
        HeightPair { previous, current }
    }

    fn advance(&mut self, raw: f32, dt: f32, supported: bool) -> f32 {
        let tuning = &LOCOMOTION.get().height;
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if !raw.is_finite() {
            self.state = HeightState::Idle;
            return raw;
        }
        let smoothed = match self.state {
            HeightState::Idle => raw,
            HeightState::Following { height, .. } => height,
            HeightState::Releasing { offset, .. } => raw + offset,
        };
        if (smoothed - raw).abs() > tuning.snap_m {
            self.state = HeightState::Idle;
            return raw;
        }
        if supported {
            let rate = match self.state {
                HeightState::Following { rate, .. } => rate,
                _ => 0.0,
            };
            let (mut y, mut rate) = critically_damped(smoothed, rate, raw, tuning.smoothing_s, dt);
            let lag = tuning.max_lag_m;
            if (y - raw).abs() > lag {
                y = raw + (y - raw).clamp(-lag, lag);
                rate = 0.0;
            }
            self.state = HeightState::Following { height: y, rate };
            return y;
        }
        let (offset, rate) = match self.state {
            HeightState::Releasing { offset, rate } => (offset, rate),
            _ => (smoothed - raw, (smoothed - raw).abs() / tuning.release_s),
        };
        let offset = offset - offset.signum() * (rate * dt).min(offset.abs());
        self.state = if offset == 0.0 {
            HeightState::Idle
        } else {
            HeightState::Releasing { offset, rate }
        };
        raw + offset
    }
}

/// The handover between the animated anchor (`0`) and the stable one (`1`).
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Handover {
    /// The linear progress, eased by [`Handover::weight`].
    progress: f32,
}

impl Handover {
    pub const fn new() -> Self {
        Self { progress: 0.0 }
    }

    /// Move toward the stable anchor while `stable`, toward the animated one otherwise, over
    /// `blend_s` seconds, and return the eased weight of the stable anchor.
    pub fn step(&mut self, stable: bool, dt: f32, blend_s: f32) -> f32 {
        let target = if stable { 1.0 } else { 0.0 };
        if blend_s <= 0.0 || !dt.is_finite() {
            self.progress = target;
        } else {
            let step = (dt.max(0.0) / blend_s).min(1.0);
            self.progress += (target - self.progress).clamp(-step, step);
        }
        self.weight()
    }

    /// The eased weight of the stable anchor.
    pub fn weight(&self) -> f32 {
        smoothstep(self.progress)
    }
}

/// What decides whether the view hangs from the root this tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Eligibility {
    /// The steady view's switch is on and the game is in play.
    pub enabled: bool,
    /// In a ragdoll or swimming, where the root is the physics body's.
    pub ragdoll: bool,
    /// The mode detection reads Rico on foot on the ground, outside a traversal.
    pub on_ground: bool,
    /// The mode detection reads a fall that began within the jump's grace.
    pub falling_in_grace: bool,
    /// What the procedural body's hand-over table reads the character doing, while the headset
    /// drives the view; `None` outside it, where the mode alone decides.
    pub case: Option<Case>,
}

impl Eligibility {
    /// Whether the root-fixed anchor applies. In the headset the hand-over table's case decides
    /// ([`Case::view_holds_root`]): the player's own head is on foot through a fall, a landing, a
    /// stumble, or a ragdoll and its get-up, so the view holds the root through all of them, even
    /// where the clip carries Rico's body away from under it. Elsewhere the mode decides: on the
    /// ground or early in a fall, out of a ragdoll.
    pub fn root_fixed(&self) -> bool {
        self.enabled
            && match self.case {
                Some(case) => case.view_holds_root(),
                None => !self.ragdoll && (self.on_ground || self.falling_in_grace),
            }
    }
}

/// How long after the movement task last ran it still counts as driving the root, seconds. It runs
/// every sim tick on foot, so this covers a few ticks' jitter without letting an unclassified
/// animation carry the body far from the view first.
pub const MOVEMENT_TASK_GRACE_S: f32 = 0.1;

/// The anchors of the neutral pose carried by the root: `world_t0` and `world_t1` are the
/// character's previous- and current-tick world matrices, and `height` the smoothed root heights
/// that replace theirs.
///
/// Only the neutral head's and neck's heights ride the root; they sit straight above it. Their
/// small horizontal offset from the root would swing around it whenever the body turns under the
/// view (the torso follow), which moves the view sideways with nothing the player did; the
/// recenter takes up the constant offset instead.
pub fn stable_anchors(
    neutral: &Neutral,
    world_t0: Mat4,
    world_t1: Mat4,
    height: HeightPair,
) -> Anchors {
    // Straight up in the world, not along the root's own up: a ragdoll's root tilts with the
    // physics body, and the view stays upright over it.
    let above = |world: Mat4, smoothed: f32, point: Vec3| {
        Vec3::new(world.w_axis.x, smoothed, world.w_axis.z) + Vec3::Y * point.y
    };
    Anchors {
        head: above(world_t1, height.current, neutral.head),
        head_prev: above(world_t0, height.previous, neutral.head),
        neck: above(world_t1, height.current, neutral.neck),
        eye_arm: neutral.eyes - neutral.neck,
    }
}

/// `stable` with the eyes hung from the head by the tracked neck model rather than by the neutral
/// pose's own neck and eyes: the neck pivot is the head bone itself, and the eyes sit
/// `-EYES_TO_NECK` from it, at the VR world scale `scale`, in the frame the camera turns the eye
/// arm with. The rig and the body read the player's neck pivot as the eyes plus `EYES_TO_NECK`
/// turned by the head, so with these anchors that pivot is the neutral head for a head held at the
/// recenter's orientation, and wherever the head turns it stays where the head bone is aimed.
///
/// The neutral pose's eyes are the idle clip's, which hold the head pitched and turned a little,
/// and its neck stands behind the head bone, so the measured arm puts the eyes a few centimetres
/// lower and further forward of the head bone than a level head carries them.
pub fn neck_model_anchors(stable: &Anchors, scale: f32) -> Anchors {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    Anchors {
        neck: stable.head,
        eye_arm: -EYES_TO_NECK * scale,
        ..*stable
    }
}

/// `animated` with its eyes carried `clearance` metres out in front of the face, along the face's
/// forward: the direction from the neck to the eyes with the skull's own axis, neck to head, taken
/// out, from the animated head, neck, and eyes in model space `model`. A clip that pitches or
/// reclines the head carries the clearance with the face, so the eyes stay ahead of it rather than
/// at the eye bones, where the chest and the shoulders of a pose that folds or leans back sit just
/// beyond the near plane. The anchors are returned unchanged when the face has no forward.
pub fn face_clear(animated: &Anchors, model: &Neutral, clearance: f32) -> Anchors {
    Anchors {
        eye_arm: animated.eye_arm + face_offset(model, clearance),
        ..*animated
    }
}

/// The eyes' offset `clearance` metres out in front of the face, model space, from the animated
/// head, neck, and eyes `model` (see [`face_clear`]); zero when the face has no forward.
pub fn face_offset(model: &Neutral, clearance: f32) -> Vec3 {
    face_forward(model)
        .map(|forward| forward * clearance)
        .filter(|offset| offset.is_finite())
        .unwrap_or(Vec3::ZERO)
}

/// The face's forward in model space: the neck-to-eyes direction with its part along the skull's
/// axis (neck to head) removed.
fn face_forward(model: &Neutral) -> Option<Vec3> {
    let up = (model.head - model.neck).try_normalize()?;
    let arm = model.eyes - model.neck;
    (arm - up * arm.dot(up)).try_normalize()
}

/// `animated` mixed toward `stable` by `weight`.
pub fn blend(animated: &Anchors, stable: &Anchors, weight: f32) -> Anchors {
    let w = weight.clamp(0.0, 1.0);
    Anchors {
        head: animated.head.lerp(stable.head, w),
        head_prev: animated.head_prev.lerp(stable.head_prev, w),
        neck: animated.neck.lerp(stable.neck, w),
        eye_arm: animated.eye_arm.lerp(stable.eye_arm, w),
    }
}

/// The range of neutral head heights above the root, metres, that describe a standing person.
const MIN_HEAD_HEIGHT_M: f32 = 1.0;
const MAX_HEAD_HEIGHT_M: f32 = 2.5;

/// The furthest the neck may sit from the head or the eyes in a plausible pose, metres.
const MAX_BONE_SPREAD_M: f32 = 0.5;

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::body::procedural::authority::{CharacterState, classify};

    const DT: f32 = 1.0 / 30.0;

    fn neutral() -> Neutral {
        Neutral {
            head: Vec3::new(0.0, 1.7, -0.05),
            neck: Vec3::new(0.0, 1.6, -0.02),
            eyes: Vec3::new(0.0, 1.72, -0.12),
        }
    }

    /// The character on foot on the ground with the movement task running, as the hand-over table
    /// reads it in the headset.
    fn on_foot() -> CharacterState {
        CharacterState {
            in_headset: true,
            in_gameplay: true,
            movement_task: true,
            ..CharacterState::default()
        }
    }

    /// The eligibility of a character the mode detection reads on the ground, in the headset.
    fn grounded(state: CharacterState) -> Eligibility {
        Eligibility {
            enabled: true,
            on_ground: true,
            case: Some(classify(&state)),
            ..Eligibility::default()
        }
    }

    #[test]
    fn the_root_holds_the_view_while_the_player_walks() {
        assert!(grounded(on_foot()).root_fixed());
        assert!(
            !Eligibility {
                enabled: false,
                ..grounded(on_foot())
            }
            .root_fixed()
        );
    }

    #[test]
    fn whatever_happens_to_rico_on_foot_the_root_holds_the_view() {
        let cases = [
            // A get-up's clip, which lifts the head from the ground.
            CharacterState {
                getting_up: true,
                ..on_foot()
            },
            // A stumble or a knock-back, for its hold.
            CharacterState {
                hit_reaction: true,
                ..on_foot()
            },
            // A hard landing, a context action, a full-body pickup, or cover.
            CharacterState {
                authored_motion: true,
                ..on_foot()
            },
            // The task stopped with Rico still on the ground.
            CharacterState {
                movement_task: false,
                ..on_foot()
            },
            // A jump, and a fall past its grace.
            CharacterState {
                jump: true,
                movement_task: false,
                ..on_foot()
            },
            CharacterState {
                flight: true,
                movement_task: false,
                ..on_foot()
            },
        ];
        for state in cases {
            assert!(grounded(state).root_fixed(), "{:?}", classify(&state));
            // Off the ground, out of the jump's grace: the case still decides.
            assert!(
                Eligibility {
                    on_ground: false,
                    ..grounded(state)
                }
                .root_fixed(),
                "{:?}",
                classify(&state)
            );
        }
    }

    #[test]
    fn a_ragdoll_holds_the_view_over_the_root() {
        let ragdoll = CharacterState {
            ragdoll: true,
            ..on_foot()
        };
        assert!(
            Eligibility {
                ragdoll: true,
                on_ground: false,
                ..grounded(ragdoll)
            }
            .root_fixed()
        );
    }

    #[test]
    fn a_clip_the_player_rides_keeps_the_animated_head() {
        let cases = [
            // The grapple's ledge climb.
            CharacterState {
                ledge_climb: true,
                ..on_foot()
            },
            // A scripted sequence.
            CharacterState {
                sequence: true,
                ..on_foot()
            },
            // Going into the wingsuit from the ground, before the mode detection reads it.
            CharacterState {
                wingsuit: true,
                ..on_foot()
            },
        ];
        for state in cases {
            assert!(!grounded(state).root_fixed(), "{:?}", classify(&state));
        }
    }

    #[test]
    fn a_tilted_root_holds_the_view_upright_above_it() {
        let neutral = neutral();
        let world = Mat4::from_rotation_translation(
            glam::Quat::from_rotation_x(1.4),
            Vec3::new(3.0, 10.0, -2.0),
        );
        let anchors = stable_anchors(
            &neutral,
            world,
            world,
            HeightPair {
                current: 10.0,
                previous: 10.0,
            },
        );
        assert!(
            anchors
                .head
                .abs_diff_eq(Vec3::new(3.0, 10.0 + neutral.head.y, -2.0), 1.0e-5)
        );
    }

    #[test]
    fn outside_the_headset_the_mode_alone_decides() {
        let mode_only = Eligibility {
            enabled: true,
            on_ground: true,
            ..Eligibility::default()
        };
        assert!(mode_only.root_fixed());
        assert!(
            !Eligibility {
                on_ground: false,
                ..mode_only
            }
            .root_fixed()
        );
        assert!(
            !Eligibility {
                ragdoll: true,
                ..mode_only
            }
            .root_fixed()
        );
    }

    #[test]
    fn the_capture_averages_once_it_has_enough() {
        let mut capture = NeutralCapture::new();
        let mut result = None;
        for tick in 0..60 {
            let sway = if tick % 2 == 0 { 0.02 } else { -0.02 };
            let mut sample = neutral();
            sample.head.x += sway;
            if let Some(neutral) = capture.push(sample, DT, 1.5) {
                result = Some(neutral);
                break;
            }
        }
        let result = result.expect("the capture never completed");
        assert!(result.head.abs_diff_eq(neutral().head, 1e-3));
    }

    #[test]
    fn an_implausible_sample_restarts_the_capture() {
        let mut capture = NeutralCapture::new();
        capture.push(neutral(), 1.0, 1.5);
        let mut bad = neutral();
        bad.head = Vec3::ZERO;
        assert_eq!(capture.push(bad, DT, 1.5), None);
        assert_eq!(capture.progress(), 0.0);
    }

    #[test]
    fn the_height_filter_ramps_a_step_and_snaps_a_drop() {
        let mut filter = HeightFilter::new();
        filter.step(0.0, DT, true);
        let first = filter.step(0.2, DT, true);
        assert!(first.current > 0.0 && first.current < 0.2);
        assert_eq!(first.previous, 0.0);
        let mut last = first.current;
        for _ in 0..30 {
            let pair = filter.step(0.2, DT, true);
            assert!(pair.current >= last - 1e-6 && pair.current <= 0.2 + 1e-6);
            last = pair.current;
        }
        assert!((last - 0.2).abs() < 1e-3);
        let drop = filter.step(-5.0, DT, true);
        assert_eq!(drop.current, -5.0);
    }

    #[test]
    fn the_height_filter_never_lags_beyond_its_limit() {
        let mut filter = HeightFilter::new();
        let mut raw = 0.0;
        for _ in 0..120 {
            raw += 3.0 * DT;
            let pair = filter.step(raw, DT, true);
            assert!(raw - pair.current <= LOCOMOTION.get().height.max_lag_m + 1e-5);
        }
    }

    #[test]
    fn leaving_the_ground_winds_the_hidden_offset_down_without_a_jump() {
        let mut filter = HeightFilter::new();
        filter.step(0.0, DT, true);
        // A jump's first tick still on the ground: the smoothing trails the rising root.
        let mut root = 0.1;
        let trailing = filter.step(root, DT, true).current;
        assert!(trailing < root);
        let mut last_gap = f32::INFINITY;
        let mut last = trailing;
        for _ in 0..10 {
            root += 0.1;
            let pair = filter.step(root, DT, false);
            // Continuous with the last tick, rising with the root, and closing on it without
            // passing it.
            assert_eq!(pair.previous, last);
            assert!(pair.current >= last);
            let gap = root - pair.current;
            assert!(
                (0.0..=last_gap + 1e-6).contains(&gap),
                "{gap} after {last_gap}"
            );
            last_gap = gap;
            last = pair.current;
        }
        assert_eq!(last, root);
        // With nothing hidden, the air's heights pass through.
        assert_eq!(filter.step(root + 0.3, DT, false).current, root + 0.3);
    }

    #[test]
    fn the_handover_eases_both_ways() {
        let mut handover = Handover::new();
        let mut weights = vec![];
        for _ in 0..10 {
            weights.push(handover.step(true, DT, 0.25));
        }
        assert!(weights.windows(2).all(|w| w[1] >= w[0]));
        assert_eq!(*weights.last().unwrap(), 1.0);
        for _ in 0..10 {
            handover.step(false, DT, 0.25);
        }
        assert_eq!(handover.weight(), 0.0);
    }

    #[test]
    fn the_stable_anchors_ride_the_root_and_ignore_its_bob() {
        let yaw = Quat::from_rotation_y(0.7);
        let t1 = Mat4::from_rotation_translation(yaw, Vec3::new(3.0, 10.0, -2.0));
        let t0 = Mat4::from_rotation_translation(yaw, Vec3::new(2.9, 10.0, -2.0));
        let height = HeightPair {
            previous: 10.0,
            current: 10.0,
        };
        let anchors = stable_anchors(&neutral(), t0, t1, height);
        assert!(
            anchors
                .head
                .abs_diff_eq(Vec3::new(3.0, 10.0 + neutral().head.y, -2.0), 1e-5)
        );
        // Turning the body in place leaves the anchors where they were.
        let turned =
            Mat4::from_rotation_translation(Quat::from_rotation_y(1.9), Vec3::new(3.0, 10.0, -2.0));
        let after = stable_anchors(&neutral(), t0, turned, height);
        assert!(after.head.abs_diff_eq(anchors.head, 1e-5));
        assert!(after.neck.abs_diff_eq(anchors.neck, 1e-5));
        assert!((anchors.head - anchors.head_prev).abs_diff_eq(Vec3::new(0.1, 0.0, 0.0), 1e-5));
        let smoothed = HeightPair {
            previous: 9.9,
            current: 9.95,
        };
        let lowered = stable_anchors(&neutral(), t0, t1, smoothed);
        assert!((anchors.head.y - lowered.head.y - 0.05).abs() < 1e-5);
        assert!((anchors.head_prev.y - lowered.head_prev.y - 0.1).abs() < 1e-5);
    }

    #[test]
    fn the_face_clearance_follows_the_face() {
        let animated = Anchors {
            head: Vec3::ZERO,
            head_prev: Vec3::ZERO,
            neck: Vec3::ZERO,
            eye_arm: Vec3::new(0.0, 0.1, -0.1),
        };
        // An upright head facing -Z: the clearance is straight ahead.
        let upright = Neutral {
            head: Vec3::new(0.0, 1.7, 0.0),
            neck: Vec3::new(0.0, 1.55, 0.0),
            eyes: Vec3::new(0.0, 1.65, -0.1),
        };
        let cleared = face_clear(&animated, &upright, 0.05);
        assert!((cleared.eye_arm - Vec3::new(0.0, 0.1, -0.15)).length() < 1e-5);
        // A head tipped back to face up: the clearance goes up, away from the chest.
        let reclined = Neutral {
            head: Vec3::new(0.0, 1.55, 0.15),
            neck: Vec3::new(0.0, 1.55, 0.0),
            eyes: Vec3::new(0.0, 1.65, 0.1),
        };
        let up = face_clear(&animated, &reclined, 0.05);
        assert!((up.eye_arm - animated.eye_arm - Vec3::new(0.0, 0.05, 0.0)).length() < 1e-5);
        // A head with no face leaves the anchors alone.
        let flat = Neutral {
            head: Vec3::ZERO,
            neck: Vec3::ZERO,
            eyes: Vec3::ZERO,
        };
        assert_eq!(face_clear(&animated, &flat, 0.05).eye_arm, animated.eye_arm);
    }

    #[test]
    fn blending_is_exact_at_the_ends() {
        let a = Anchors {
            head: Vec3::X,
            head_prev: Vec3::Y,
            neck: Vec3::Z,
            eye_arm: Vec3::ONE,
        };
        let b = Anchors {
            head: Vec3::NEG_X,
            head_prev: Vec3::NEG_Y,
            neck: Vec3::NEG_Z,
            eye_arm: Vec3::ZERO,
        };
        assert_eq!(blend(&a, &b, 0.0).head, a.head);
        assert_eq!(blend(&a, &b, 1.0).neck, b.neck);
    }

    /// Under the neck model the eyes hang from the head bone so that the neck pivot the rig and the
    /// body read off the eyes (the eyes plus `EYES_TO_NECK` turned by the head) is the head bone,
    /// at any world scale, for a head held at the view frame's orientation.
    #[test]
    fn the_neck_model_puts_the_neck_pivot_on_the_head() {
        let world_t0 = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let world_t1 = Mat4::from_translation(Vec3::new(1.1, 2.0, 3.0));
        let height = HeightPair {
            previous: 2.0,
            current: 2.0,
        };
        let stable = stable_anchors(&neutral(), world_t0, world_t1, height);
        for scale in [1.0, 1.6] {
            let model = neck_model_anchors(&stable, scale);
            assert_eq!(model.head, stable.head);
            assert_eq!(model.head_prev, stable.head_prev);
            for view in [Quat::IDENTITY, Quat::from_rotation_y(0.7)] {
                let eyes = model.head + (model.neck - model.head) + view * model.eye_arm;
                let pivot = eyes + view * EYES_TO_NECK * scale;
                assert!(
                    pivot.abs_diff_eq(model.head, 1e-5),
                    "{pivot} {}",
                    model.head
                );
            }
        }
        assert_eq!(neck_model_anchors(&stable, f32::NAN).eye_arm, -EYES_TO_NECK);
    }
}
