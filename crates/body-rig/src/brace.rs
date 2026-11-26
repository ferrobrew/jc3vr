//! The feet on the face while the hands climb: where a leg can brace its foot on the surface in
//! front of it, and each foot held on its brace until the climbing body carries the hip out of the
//! leg's reach (`docs/mod/body/ik-body.md`, "Feet on the face"). Pure.
//!
//! The caller casts from each hip toward the body's facing and from each ankle forward and down,
//! and hands the nearest hit to [`BraceFoot::update`]. A hit the leg reaches, on a face that looks
//! toward the hip, becomes the foot's brace ([`place`]): the ankle stands off the surface by its
//! height along the normal, the sole against the face with the toe up its slope, and the knee hint
//! toward the face. The brace is held in its support's frame, as a planted foot is, so the foot
//! stays put on the face while the body climbs past it and on a moving body that carries it. Once
//! the hip leaves the leg's reach the foot lets go: it moves to the latest hit the leg reaches, on
//! an arc off the face, or hangs relaxed where there is none, fading out of the brace.

use glam::Vec3;

use crate::{
    angle::forward,
    contact::{SupportFrame, SupportId},
    smooth::smoothstep,
};

/// A cast's hit on a surface a foot might brace on, in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BraceHit {
    pub point: Vec3,
    /// The surface's normal, out of the surface.
    pub normal: Vec3,
    pub support: SupportId,
}

/// One leg, in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BraceLeg {
    pub hip: Vec3,
    /// The body's facing, a heading.
    pub facing: f32,
    /// The leg's straight length from the hip to the ankle, metres.
    pub leg_m: f32,
    /// The thigh's length, metres.
    pub thigh_m: f32,
    /// From the ankle to the toe joint, metres.
    pub foot_m: f32,
    /// How far the ankle stands off the sole, metres.
    pub ankle_height_m: f32,
    /// How far the toe joint stands off the sole, metres.
    pub toe_height_m: f32,
}

/// Where a braced foot's targets go, in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BraceTarget {
    pub ankle: Vec3,
    pub toe: Vec3,
    pub knee: Vec3,
    /// The face's normal at the brace.
    pub normal: Vec3,
}

/// The braces' tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BraceTuning {
    /// The share of the leg's length past which a held brace lets go.
    pub reach_share: f32,
    /// The share of the leg's length within which a new brace is taken: under the release's, so a
    /// fresh brace is not let go at once.
    pub acquire_share: f32,
    /// The share of the leg's length within which a brace is too close to the hip to take: a knee
    /// folded to the chest.
    pub min_share: f32,
    /// How squarely the face must look toward the hip to be braced on: the least cosine between
    /// its normal and the line to the hip.
    pub min_facing: f32,
    /// How far the knee hint stands toward the face from the line between the hip and the ankle,
    /// metres.
    pub knee_m: f32,
    /// How long a foot takes to move to a new brace, and to fade in or out of one, seconds.
    pub move_s: f32,
    /// How far off the face the foot lifts on its way to a new brace, metres.
    pub arc_m: f32,
}

impl BraceTuning {
    pub const fn new() -> Self {
        Self {
            reach_share: 0.98,
            acquire_share: 0.9,
            min_share: 0.35,
            min_facing: 0.2,
            knee_m: 0.15,
            move_s: 0.3,
            arc_m: 0.08,
        }
    }
}

impl Default for BraceTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// Where `leg` would brace on `hit`, if the leg reaches it within `reach_share` of its length and
/// the face looks toward the hip.
pub fn place(
    leg: &BraceLeg,
    hit: &BraceHit,
    reach_share: f32,
    tuning: &BraceTuning,
) -> Option<BraceTarget> {
    let normal = hit.normal.try_normalize()?;
    let to_hip = (leg.hip - hit.point).try_normalize()?;
    if normal.dot(to_hip) < tuning.min_facing {
        return None;
    }
    let ankle = hit.point + normal * leg.ankle_height_m.max(0.0);
    let reach = (ankle - leg.hip).length();
    let leg_m = leg.leg_m.max(0.0);
    if !(reach.is_finite() && reach <= leg_m * reach_share && reach >= leg_m * tuning.min_share) {
        return None;
    }
    // The toe runs up the face's slope; on a face too level to have one, along the facing laid on
    // it.
    let up_slope = Vec3::Y - normal * normal.y;
    let along = if up_slope.length() > LEVEL_SLOPE {
        up_slope.normalize()
    } else {
        let ahead = forward(leg.facing);
        let ahead = Vec3::new(ahead.x, 0.0, ahead.y);
        (ahead - normal * normal.dot(ahead)).try_normalize()?
    };
    let toe = hit.point + along * leg.foot_m.max(0.0) + normal * leg.toe_height_m.max(0.0);
    let share = if leg_m > 0.0 {
        (leg.thigh_m / leg_m).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let knee = leg.hip.lerp(ankle, share) - normal * tuning.knee_m.max(0.0);
    let target = BraceTarget {
        ankle,
        toe,
        knee,
        normal,
    };
    (ankle.is_finite() && toe.is_finite() && knee.is_finite()).then_some(target)
}

/// What [`BraceFoot::update`] did this frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BraceEvent {
    /// The foot took a brace from hanging.
    Braced,
    /// The foot let go of its brace and moved to another.
    Moved,
    /// The foot let go of its brace and hangs.
    LetGo,
}

/// One foot's brace.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct BraceFoot {
    hold: Option<Hold>,
    /// The brace the foot is moving from, and how far along, seconds.
    from: Option<(BraceTarget, f32)>,
    /// The brace's share against the hanging leg.
    weight: f32,
    last: Option<BraceTarget>,
}

/// A brace, held in its support's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Hold {
    point: Vec3,
    normal: Vec3,
    support: SupportId,
}

impl BraceFoot {
    pub const fn new() -> Self {
        Self {
            hold: None,
            from: None,
            weight: 0.0,
            last: None,
        }
    }

    /// Let go at once: a teleport, or the climb's end.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Whether the foot holds a brace.
    pub fn braced(&self) -> bool {
        self.hold.is_some()
    }

    /// Advance one frame for `leg`, with `candidate` the latest cast's hit and `frames` each
    /// support's frame now: the foot's targets and the brace's weight, or `None` where the leg
    /// hangs.
    pub fn update(
        &mut self,
        leg: &BraceLeg,
        candidate: Option<BraceHit>,
        frames: impl Fn(SupportId) -> Option<SupportFrame>,
        dt: f32,
        tuning: &BraceTuning,
    ) -> (Option<(BraceTarget, f32)>, Option<BraceEvent>) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut event = None;
        let held = self.hold.and_then(|hold| {
            let frame = frames(hold.support)?;
            let hit = BraceHit {
                point: frame.point_to_model(hold.point),
                normal: frame.rotation * hold.normal,
                support: hold.support,
            };
            place(leg, &hit, tuning.reach_share, tuning)
        });
        if held.is_none() && self.hold.take().is_some() {
            event = Some(BraceEvent::LetGo);
        }
        let mut target = held;
        if target.is_none()
            && let Some(hit) = candidate
            && let Some(frame) = frames(hit.support)
            && let Some(placed) = place(leg, &hit, tuning.acquire_share, tuning)
        {
            self.hold = Some(Hold {
                point: frame.point_to_local(hit.point),
                normal: frame.rotation.inverse() * hit.normal,
                support: hit.support,
            });
            event = Some(match (event, self.last) {
                (Some(BraceEvent::LetGo), Some(_)) => BraceEvent::Moved,
                _ => BraceEvent::Braced,
            });
            if self.weight > 0.0
                && let Some(last) = self.last
            {
                self.from = Some((last, 0.0));
            }
            target = Some(placed);
        }
        let rate = if tuning.move_s > 0.0 {
            dt / tuning.move_s
        } else {
            1.0
        };
        let goal = if target.is_some() { 1.0 } else { 0.0 };
        self.weight = if goal > self.weight {
            (self.weight + rate).min(goal)
        } else {
            (self.weight - rate).max(goal)
        };
        let Some(mut target) = target else {
            self.from = None;
            let out = self.last.filter(|_| self.weight > 0.0);
            if out.is_none() {
                self.last = None;
            }
            return (out.map(|last| (last, self.weight)), event);
        };
        if let Some((from, elapsed)) = self.from.as_mut() {
            *elapsed += dt;
            let t = if tuning.move_s > 0.0 {
                (*elapsed / tuning.move_s).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let ease = smoothstep(t);
            let lift = target.normal * tuning.arc_m.max(0.0) * 4.0 * t * (1.0 - t);
            let moved = BraceTarget {
                ankle: from.ankle.lerp(target.ankle, ease) + lift,
                toe: from.toe.lerp(target.toe, ease) + lift,
                knee: from.knee.lerp(target.knee, ease),
                normal: target.normal,
            };
            if t >= 1.0 {
                self.from = None;
            }
            target = moved;
        }
        self.last = Some(target);
        (Some((target, self.weight)), event)
    }
}

/// How much of +Y must stand along a face for it to have a slope to run the toe up.
const LEVEL_SLOPE: f32 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const TUNING: BraceTuning = BraceTuning::new();

    /// A leg whose hip stands at `hip`, facing -Z toward a wall.
    fn leg(hip: Vec3) -> BraceLeg {
        BraceLeg {
            hip,
            facing: 0.0,
            leg_m: 0.85,
            thigh_m: 0.42,
            foot_m: 0.14,
            ankle_height_m: 0.09,
            toe_height_m: 0.025,
        }
    }

    /// A vertical wall at `z = -0.5`, facing +Z.
    fn wall(y: f32) -> BraceHit {
        BraceHit {
            point: Vec3::new(0.1, y, -0.5),
            normal: Vec3::Z,
            support: SupportId::WORLD,
        }
    }

    fn world(_: SupportId) -> Option<SupportFrame> {
        Some(SupportFrame::IDENTITY)
    }

    #[test]
    fn a_wall_in_reach_takes_the_sole_against_it_and_the_knee_toward_it() {
        let hip = Vec3::new(0.1, 1.0, 0.0);
        let target = place(&leg(hip), &wall(0.5), TUNING.reach_share, &TUNING).unwrap();
        // The ankle stands off the wall by its height, the toe up the wall, the sole flat on it.
        assert!(target.ankle.abs_diff_eq(Vec3::new(0.1, 0.5, -0.41), 1e-5));
        assert!(target.toe.abs_diff_eq(Vec3::new(0.1, 0.64, -0.475), 1e-5));
        // The knee hint stands nearer the wall than the line from the hip to the ankle.
        let on_line = hip.lerp(target.ankle, 0.42 / 0.85);
        assert!(target.knee.z < on_line.z - 0.1);
    }

    #[test]
    fn a_face_out_of_reach_or_turned_away_is_no_brace() {
        let far = place(&leg(Vec3::new(0.1, 2.0, 0.0)), &wall(0.2), 0.98, &TUNING);
        assert_eq!(far, None);
        let mut away = wall(0.5);
        away.normal = Vec3::NEG_Z;
        assert_eq!(
            place(&leg(Vec3::new(0.1, 1.0, 0.0)), &away, 0.98, &TUNING),
            None
        );
    }

    #[test]
    fn a_level_face_runs_the_toe_along_the_facing() {
        let ledge = BraceHit {
            point: Vec3::new(0.0, 0.4, -0.3),
            normal: Vec3::Y,
            support: SupportId::WORLD,
        };
        let target = place(&leg(Vec3::new(0.0, 1.0, 0.0)), &ledge, 0.98, &TUNING).unwrap();
        assert!((target.toe - target.ankle).z < -0.1);
    }

    #[test]
    fn a_held_brace_stays_put_as_the_body_climbs_and_moves_up_once_out_of_reach() {
        let mut foot = BraceFoot::new();
        let (first, event) = foot.update(
            &leg(Vec3::new(0.1, 1.0, 0.0)),
            Some(wall(0.4)),
            world,
            DT,
            &TUNING,
        );
        assert_eq!(event, Some(BraceEvent::Braced));
        let start = first.unwrap().0.ankle;
        // The body climbs 10 cm: the brace stays where it was, the cast's later hits ignored.
        let mut last = None;
        for i in 1..=20 {
            let hip = Vec3::new(0.1, 1.0 + 0.005 * i as f32, 0.0);
            let (out, event) = foot.update(
                &leg(hip),
                Some(wall(0.4 + 0.01 * i as f32)),
                world,
                DT,
                &TUNING,
            );
            assert_eq!(event, None);
            last = out;
        }
        let (held, weight) = last.unwrap();
        assert!(held.ankle.abs_diff_eq(start, 1e-5));
        assert_eq!(weight, 1.0);
        // On up past the leg's reach: the foot moves to the cast's hit near the hip, on an arc off
        // the wall.
        let hip = Vec3::new(0.1, 1.85, 0.0);
        let (out, event) = foot.update(&leg(hip), Some(wall(1.25)), world, DT, &TUNING);
        assert_eq!(event, Some(BraceEvent::Moved));
        let (moving, _) = out.unwrap();
        assert!(
            moving.ankle.y < 0.5,
            "it eases from where it was: {moving:?}"
        );
        let mut out = None;
        let mut lifted = false;
        for _ in 0..30 {
            out = foot
                .update(&leg(hip), Some(wall(1.25)), world, DT, &TUNING)
                .0;
            lifted |= out.unwrap().0.ankle.z > -0.41 + 0.02;
        }
        assert!(lifted, "the foot lifts off the wall on its way");
        assert!(
            out.unwrap()
                .0
                .ankle
                .abs_diff_eq(Vec3::new(0.1, 1.25, -0.41), 1e-4)
        );
    }

    #[test]
    fn with_nothing_in_reach_the_foot_fades_out_and_hangs() {
        let mut foot = BraceFoot::new();
        for _ in 0..30 {
            foot.update(
                &leg(Vec3::new(0.1, 1.0, 0.0)),
                Some(wall(0.4)),
                world,
                DT,
                &TUNING,
            );
        }
        let away = leg(Vec3::new(0.1, 1.0, 2.0));
        let (out, event) = foot.update(&away, None, world, DT, &TUNING);
        assert_eq!(event, Some(BraceEvent::LetGo));
        assert!(out.unwrap().1 < 1.0);
        let mut out = None;
        for _ in 0..30 {
            out = foot.update(&away, None, world, DT, &TUNING).0;
        }
        assert_eq!(out, None);
        assert!(!foot.braced());
    }

    #[test]
    fn a_moving_support_carries_its_brace() {
        let deck = SupportId(5);
        let mut at = SupportFrame::IDENTITY;
        let mut foot = BraceFoot::new();
        let hit = BraceHit {
            support: deck,
            ..wall(0.4)
        };
        let hip = Vec3::new(0.1, 1.0, 0.0);
        foot.update(&leg(hip), Some(hit), |_| Some(at), DT, &TUNING);
        at.translation = Vec3::new(0.0, 0.05, 0.0);
        let (out, _) = foot.update(&leg(hip), None, |_| Some(at), DT, &TUNING);
        assert!((out.unwrap().0.ankle.y - 0.45).abs() < 1e-5);
        // The support goes: the foot lets go.
        let (_, event) = foot.update(&leg(hip), None, |_| None, DT, &TUNING);
        assert_eq!(event, Some(BraceEvent::LetGo));
    }
}
