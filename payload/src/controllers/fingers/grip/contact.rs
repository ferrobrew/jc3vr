//! Contact-fitted curls: each finger of a hand holding a body closes until a short cast from its
//! segments meets the body, so the fingers sit on its surface.
//!
//! The finger curls publish the hand's proportions once per tick they write the fingers
//! ([`publish_geometry`]): each finger's root and its bones' lengths, in the palm's frame, read
//! from the animated bones. The holding source then sweeps each finger in the world
//! ([`fit_contacts`]) through the same bend the curls give it ([`pose`](crate::controllers::fingers::pose)):
//! from straight toward a fist in [`SWEEP_STEPS`] steps, casting the chord the fingertip and the
//! middle joint each move along over a step. The first chord that meets the held body ends the
//! finger's sweep at the curl where it met it; a finger that meets nothing keeps the pose's own
//! curl. A cast that meets something else (the hand's own body, the world) is passed over, so a
//! finger is never stopped short by what it is not holding.
//!
//! The casts are bounded: [`FINGERS_PER_TICK`] fingers of a hand are swept per tick, so a fresh
//! hold is fitted over three ticks, and a fitted hold is swept again only every [`REFIT`], since
//! the hand holds the body still in its own frame. A new body or a new pose starts the fit over.

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    fingers::{
        config::FingersConfig,
        curl::Finger,
        grip::catalogue::GripPose,
        pose::{
            BoneJoint, PalmFrame, bone_directions, finger_axes, joint_angles, thumb_axis,
            thumb_open,
        },
    },
    frame::{Hand, Pose},
};

/// A hand's proportions: each finger's root and bone lengths, indexed by [`Finger::index`]. The
/// roots are in the palm's frame, as `(toward the thumb, out of the palm, toward the fingers)`
/// metres from the wrist.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandGeometry {
    pub roots: [Vec3; 5],
    /// The proximal, middle, and distal bones' lengths, metres. The distal bone has no joint
    /// beyond it, so its length is a share of the middle bone's ([`DISTAL_SHARE`]).
    pub lengths: [[f32; 3]; 5],
}

impl HandGeometry {
    /// The proportions of the animated `chains` (root outward, indexed by [`Finger::index`]) in
    /// the frame `palm`, or `None` for a chain with coincident joints.
    pub fn measure(palm: &PalmFrame, chains: &[[BoneJoint; 3]; 5]) -> Option<Self> {
        let mut roots = [Vec3::ZERO; 5];
        let mut lengths = [[0.0; 3]; 5];
        for (i, chain) in chains.iter().enumerate() {
            let offset = chain[0].position - palm.wrist;
            roots[i] = Vec3::new(
                offset.dot(palm.toward_thumb),
                offset.dot(palm.palm),
                offset.dot(palm.forward),
            );
            let proximal = chain[0].position.distance(chain[1].position);
            let middle = chain[1].position.distance(chain[2].position);
            if !(proximal > 1e-4 && middle > 1e-4) {
                return None;
            }
            lengths[i] = [proximal, middle, middle * DISTAL_SHARE];
        }
        Some(Self { roots, lengths })
    }

    /// Finger `finger`'s root in the frame `palm`.
    pub fn root_in(&self, finger: Finger, palm: &PalmFrame) -> Vec3 {
        let r = self.roots[finger.index()];
        palm.wrist + palm.toward_thumb * r.x + palm.palm * r.y + palm.forward * r.z
    }
}

/// The distal bone's length as a share of the middle bone's.
pub const DISTAL_SHARE: f32 = 0.8;

/// Publish `hand`'s proportions. Called by the finger curls each tick they write the fingers.
pub fn publish_geometry(hand: Hand, geometry: HandGeometry) {
    FIT.lock().geometry[hand.index()] = Some(geometry);
}

/// What a contact cast met: the share of the chord travelled to the hit, and the hit body's Havok
/// id.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CastHit {
    pub fraction: f32,
    pub body: u32,
}

/// The body a hand holds and where the hand is, for its fit.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FitTarget {
    /// The held body's Havok id.
    pub body: u32,
    /// The hand's wrist frame in the world, as the rendered hand holds the body.
    pub wrist: Pose,
    pub pose: GripPose,
}

/// The contact fit of `hand` on `target`: each finger's curl where it meets the body, `None` for a
/// finger that met nothing or is not swept yet; `None` as a whole before the finger curls have
/// published the hand's proportions. Sweeps at most [`FINGERS_PER_TICK`] fingers with `cast`,
/// which casts the chord between two world points and reports the first hit.
pub fn fit_contacts(
    hand: Hand,
    target: FitTarget,
    config: &FingersConfig,
    cast: &mut dyn FnMut(Vec3, Vec3) -> Option<CastHit>,
) -> Option<[Option<f32>; 5]> {
    let now = Instant::now();
    let mut fit = FIT.lock();
    let geometry = fit.geometry[hand.index()]?;
    let cache = &mut fit.hands[hand.index()];
    if cache.key != Some((target.body, target.pose)) {
        *cache = FitCache {
            key: Some((target.body, target.pose)),
            ..FitCache::EMPTY
        };
    }
    if cache.swept == [true; 5] {
        if cache
            .done_at
            .is_some_and(|at| now.saturating_duration_since(at) < REFIT)
        {
            return Some(cache.curls);
        }
        cache.swept = [false; 5];
    }
    let palm = palm_of_wrist(hand, target.wrist);
    let shape = target.pose.shape();
    let max_opposition = config.grip.max_opposition_deg.clamp(0.0, 90.0).to_radians();
    let finger_shape = |finger: Finger| FingerSweep {
        root: geometry.root_in(finger, &palm),
        lengths: geometry.lengths[finger.index()],
        hook: shape.hook[finger.index()],
        splay: shape.splay[finger.index()],
        thumb: (
            shape
                .thumb_spread_deg
                .unwrap_or(config.thumb_spread_deg)
                .to_radians(),
            shape.opposition.clamp(0.0, 1.0) * max_opposition,
        ),
    };
    let mut swept = 0;
    for finger in Finger::ALL {
        let i = finger.index();
        if cache.swept[i] || swept == FINGERS_PER_TICK {
            continue;
        }
        cache.curls[i] = sweep(finger, &finger_shape(finger), &palm, config, &mut |a, b| {
            cast(a, b).filter(|hit| hit.body == target.body)
        });
        cache.swept[i] = true;
        swept += 1;
    }
    if cache.swept == [true; 5] {
        cache.done_at = Some(now);
    }
    Some(cache.curls)
}

/// The palm's frame from a hand's wrist frame in the world: the fingers along its -Z, the back of
/// the hand along its +Y, and the little finger toward +X on the right hand and -X on the left.
pub fn palm_of_wrist(hand: Hand, wrist: Pose) -> PalmFrame {
    let q = wrist.orientation;
    PalmFrame {
        wrist: wrist.position,
        palm: q * Vec3::NEG_Y,
        forward: q * Vec3::NEG_Z,
        toward_thumb: match hand {
            Hand::Right => q * Vec3::NEG_X,
            Hand::Left => q * Vec3::X,
        },
    }
}

/// One finger's place and shape for a sweep.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FingerSweep {
    /// The finger's root, in the frame the sweep runs in.
    pub root: Vec3,
    pub lengths: [f32; 3],
    pub hook: f32,
    /// The knuckle's sideways turn, radians; unused for the thumb.
    pub splay: f32,
    /// The thumb's spread and opposition, radians; unused for the fingers.
    pub thumb: (f32, f32),
}

/// Sweep `finger` from straight toward a fist and return the curl at which a chord its fingertip
/// or middle joint moves along meets something `cast` reports, or `None` when nothing is met.
pub fn sweep(
    finger: Finger,
    shape: &FingerSweep,
    palm: &PalmFrame,
    config: &FingersConfig,
    cast: &mut dyn FnMut(Vec3, Vec3) -> Option<CastHit>,
) -> Option<f32> {
    let points = |curl: f32| finger_points(finger, shape, palm, config, curl);
    let mut previous = points(0.0)?;
    for step in 1..=SWEEP_STEPS {
        let (from, to) = (
            (step - 1) as f32 / SWEEP_STEPS as f32,
            step as f32 / SWEEP_STEPS as f32,
        );
        let next = points(to)?;
        for joint in [3, 2] {
            if let Some(hit) = cast(previous[joint], next[joint]) {
                return Some(from + (to - from) * hit.fraction.clamp(0.0, 1.0));
            }
        }
        previous = next;
    }
    None
}

/// The finger's joints at `curl`, root outward, with the fingertip last; `None` for a degenerate
/// hand.
pub fn finger_points(
    finger: Finger,
    shape: &FingerSweep,
    palm: &PalmFrame,
    config: &FingersConfig,
    curl: f32,
) -> Option<[Vec3; 4]> {
    let (open, axis, splay_axis, splay) = match finger {
        Finger::Thumb => {
            let open = thumb_open(palm, shape.thumb.0, shape.thumb.1);
            let axis = thumb_axis(palm, open)?;
            (open, axis, axis, 0.0)
        }
        _ => {
            let (open, axis, splay_axis) = finger_axes(palm, shape.root)?;
            (open, axis, splay_axis, shape.splay)
        }
    };
    let angles = joint_angles(finger, curl, shape.hook, config);
    let directions = bone_directions(open, axis, splay_axis, splay, angles);
    let mut points = [shape.root; 4];
    for k in 0..3 {
        points[k + 1] = points[k] + directions[k] * shape.lengths[k];
    }
    Some(points)
}

/// The steps a sweep takes from straight to a fist.
pub const SWEEP_STEPS: u32 = 5;
/// The fingers of a hand swept per tick.
pub const FINGERS_PER_TICK: usize = 2;
/// How long a fitted hold keeps its fit before it is swept again.
pub const REFIT: Duration = Duration::from_millis(500);

/// One hand's fit across ticks.
#[derive(Debug, Copy, Clone, PartialEq)]
struct FitCache {
    /// The body and the pose the fit is for.
    key: Option<(u32, GripPose)>,
    curls: [Option<f32>; 5],
    swept: [bool; 5],
    done_at: Option<Instant>,
}

impl FitCache {
    const EMPTY: FitCache = FitCache {
        key: None,
        curls: [None; 5],
        swept: [false; 5],
        done_at: None,
    };
}

struct Fit {
    geometry: [Option<HandGeometry>; 2],
    hands: [FitCache; 2],
}

static FIT: Mutex<Fit> = Mutex::new(Fit {
    geometry: [None; 2],
    hands: [FitCache::EMPTY; 2],
});

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    /// A right hand at the origin in the wrist frame's convention.
    fn right_palm() -> PalmFrame {
        palm_of_wrist(Hand::Right, Pose::IDENTITY)
    }

    fn middle_finger() -> FingerSweep {
        FingerSweep {
            root: Vec3::new(0.0, 0.0, -0.09),
            lengths: [0.045, 0.028, 0.022],
            hook: 0.0,
            splay: 0.0,
            thumb: (0.7, 0.0),
        }
    }

    /// A cast against the plane `y = -depth` below the palm, as a box face under the hand would
    /// be, reporting `body`.
    fn plane(depth: f32, body: u32) -> impl FnMut(Vec3, Vec3) -> Option<CastHit> {
        move |a: Vec3, b: Vec3| {
            let (ya, yb) = (a.y + depth, b.y + depth);
            (ya > 0.0 && yb <= 0.0).then(|| CastHit {
                fraction: ya / (ya - yb),
                body,
            })
        }
    }

    #[test]
    fn the_palm_frame_of_a_wrist_matches_the_bones() {
        let right = right_palm();
        assert!(right.palm.abs_diff_eq(Vec3::NEG_Y, 1e-6));
        assert!(right.forward.abs_diff_eq(Vec3::NEG_Z, 1e-6));
        assert!(right.toward_thumb.abs_diff_eq(Vec3::NEG_X, 1e-6));
        let left = palm_of_wrist(Hand::Left, Pose::IDENTITY);
        assert!(left.toward_thumb.abs_diff_eq(Vec3::X, 1e-6));
    }

    #[test]
    fn a_straight_finger_lies_along_its_open_direction() {
        let config = FingersConfig::new();
        let points = finger_points(
            Finger::Middle,
            &middle_finger(),
            &right_palm(),
            &config,
            0.0,
        )
        .unwrap();
        let tip = points[3];
        assert!((tip.z + 0.09 + 0.045 + 0.028 + 0.022).abs() < 1e-5, "{tip}");
        assert!(tip.y.abs() < 1e-5);
        let closed = finger_points(
            Finger::Middle,
            &middle_finger(),
            &right_palm(),
            &config,
            1.0,
        )
        .unwrap();
        assert!(
            closed[3].y < -0.02,
            "a fist closes on the palm side: {}",
            closed[3]
        );
    }

    #[test]
    fn a_finger_stops_where_it_meets_the_surface() {
        let config = FingersConfig::new();
        let near = sweep(
            Finger::Middle,
            &middle_finger(),
            &right_palm(),
            &config,
            &mut plane(0.02, 7),
        )
        .unwrap();
        let far = sweep(
            Finger::Middle,
            &middle_finger(),
            &right_palm(),
            &config,
            &mut plane(0.05, 7),
        )
        .unwrap();
        assert!(near > 0.0 && near < far, "{near} {far}");
        // The fit puts the fingertip or the middle joint on the plane.
        let points = finger_points(
            Finger::Middle,
            &middle_finger(),
            &right_palm(),
            &config,
            near,
        )
        .unwrap();
        let lowest = points[2].y.min(points[3].y);
        assert!((lowest + 0.02).abs() < 0.004, "{lowest}");
    }

    #[test]
    fn a_finger_that_meets_nothing_has_no_fit() {
        let config = FingersConfig::new();
        let mut calls = 0;
        let mut miss = |_: Vec3, _: Vec3| {
            calls += 1;
            None
        };
        assert_eq!(
            sweep(
                Finger::Ring,
                &middle_finger(),
                &right_palm(),
                &config,
                &mut miss
            ),
            None
        );
        assert_eq!(calls, 2 * SWEEP_STEPS, "two chords a step, bounded");
    }

    #[test]
    fn the_thumb_sweeps_across_the_palm() {
        let config = FingersConfig::new();
        let thumb = FingerSweep {
            root: Vec3::new(-0.025, -0.01, -0.02),
            lengths: [0.035, 0.03, 0.024],
            hook: 0.0,
            splay: 0.0,
            thumb: (0.7, 0.6),
        };
        let open = finger_points(Finger::Thumb, &thumb, &right_palm(), &config, 0.0).unwrap();
        let closed = finger_points(Finger::Thumb, &thumb, &right_palm(), &config, 1.0).unwrap();
        assert!(closed[3].x > open[3].x, "toward the little finger");
        assert!(
            open[3].y < thumb.root.y,
            "the opposed thumb leaves the palm's plane"
        );
    }

    #[test]
    fn the_geometry_reads_back_where_it_was_measured() {
        let palm = right_palm();
        let chain = |root: Vec3| {
            [0.0, 0.04, 0.065].map(|d| BoneJoint {
                position: root + Vec3::NEG_Z * d,
                orientation: Quat::IDENTITY,
            })
        };
        let roots = [
            Vec3::new(-0.025, -0.01, -0.02),
            Vec3::new(-0.03, 0.0, -0.09),
            Vec3::new(0.0, 0.0, -0.09),
            Vec3::new(0.025, 0.0, -0.085),
            Vec3::new(0.05, 0.0, -0.08),
        ];
        let geometry = HandGeometry::measure(&palm, &roots.map(chain)).unwrap();
        for (i, finger) in Finger::ALL.into_iter().enumerate() {
            assert!(geometry.root_in(finger, &palm).abs_diff_eq(roots[i], 1e-6));
            assert!((geometry.lengths[i][0] - 0.04).abs() < 1e-6);
            assert!((geometry.lengths[i][2] - 0.025 * DISTAL_SHARE).abs() < 1e-6);
        }
        let flat = [BoneJoint {
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        }; 3];
        assert!(HandGeometry::measure(&palm, &[flat; 5]).is_none());
    }
}
