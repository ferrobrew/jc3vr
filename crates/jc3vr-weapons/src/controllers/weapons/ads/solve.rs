//! The aim-down-sights pose. Pure.
//!
//! The gun is a rigid body in the holding hand's wrist frame. Each step below moves it as a whole,
//! and the hands on it move with it:
//!
//! 1. The stock ([`shoulder_move`]): with the butt near the shoulder pocket, the gun turns about
//!    its front hand (the hand on the foregrip, or the holding hand without one) so that the butt
//!    lies on the line to the pocket. The front hand then sets where the gun points and the
//!    shoulder holds its back, as a shouldered rifle is held.
//! 2. The sights ([`eye_move`]): with the eye just behind the gun and close to its sight line,
//!    the gun turns about a point ahead of the holding grip on the line (about where the front
//!    sight is) so that the line passes through the eye. The front sight stays where the hands put
//!    it, and the eye, the rear sight, and the front sight line up: the shot goes where the player
//!    sees the sights point.
//! 3. A scope's cheek weld ([`weld_move`]): a scope is lined up by moving the gun across its axis,
//!    keeping its direction, so the eye sits on the axis and the scope's view centres.
//!
//! Each step fades in as its distance closes from its engage distance to its lock distance, and the
//! sights' steps also fade across the bounds of where the eye counts as behind the gun and the head
//! as looking along it, so the pose is continuous in its inputs: a gun brought up to the eye settles
//! onto it rather than snapping, and a gate that opens with the eye already on the line eases the
//! sights in. The caller also eases the solved move toward each new one over time ([`ease`]), which
//! covers what the inputs themselves do in a step: the front hand coming onto or off the foregrip,
//! or the raised gun changing hands.

use body_rig::math::smooth::{follow_factor, smoothstep_between};
use glam::{Quat, Vec3};
use jc3vr_body::controllers::hands::CockpitHand;
use jc3vr_input::frame::Pose;
use serde::Deserialize;

use crate::controllers::weapons::handling_tuning::HANDLING;

/// The aim-down-sights tuning: the `ads` section of the handling tuning ([`HANDLING`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdsTuning {
    /// How far the eye may be off the sight line for the line to start turning onto it, metres.
    pub(crate) engage_m: f32,
    /// How far off the sight line the eye is when the line passes through it fully, metres.
    pub(crate) lock_m: f32,
    /// How far behind the holding grip, along the sight line, the eye may be, metres.
    pub(crate) max_eye_behind_m: f32,
    /// How far the sights fade over at each end of the eye's range behind the grip, metres: in over
    /// the first stretch behind the grip, and out over the last before
    /// [`AdsTuning::max_eye_behind_m`].
    pub(crate) eye_behind_fade_m: f32,
    /// The widest angle between the head's forward and the sight line, degrees: a gun the head does
    /// not look along is not being aimed.
    pub(crate) max_view_angle_deg: f32,
    /// The angle between the head's forward and the sight line within which the head fully looks
    /// along it, degrees; the sights fade out from here to [`AdsTuning::max_view_angle_deg`].
    pub(crate) full_view_angle_deg: f32,
    /// How far ahead of the holding grip, along the sight line, the gun turns about to bring the
    /// sights to the eye, metres: about where the front sight is.
    pub(crate) pivot_ahead_m: f32,
    /// How far behind the holding grip the butt is, along the sight line, metres.
    pub(crate) butt_behind_grip_m: f32,
    /// How far from the shoulder pocket the butt starts to seat, metres.
    pub(crate) shoulder_engage_m: f32,
    /// How far from the shoulder pocket the butt is when it is fully seated, metres.
    pub(crate) shoulder_lock_m: f32,
    /// How far off a scope's axis the eye may be for the cheek weld to start, metres.
    pub(crate) weld_engage_m: f32,
    /// How far off a scope's axis the eye is when the cheek weld is full, metres.
    pub(crate) weld_lock_m: f32,
    /// The time constant the solved move eases toward each new one with, seconds ([`ease`]).
    pub(crate) ease_s: f32,
    /// The longest step the eased moves take between two poses, seconds: after a pause in the poses
    /// (a hitch, or the hands going inert), the moves take one long step rather than replaying it.
    pub(crate) max_ease_step_s: f32,
    /// How far each eye sits from the eyes' midpoint, metres.
    pub(crate) half_ipd_m: f32,
    /// The share of a step (the eye, the shoulder, or the cheek weld) past which it counts as
    /// taken, `0..=1`.
    pub(crate) taken_share: f32,
    /// The shoulder pocket's offset from the shoulder joint in the torso's frame (+X right, +Y up,
    /// -Z forward), metres, given for the right shoulder and mirrored for the left.
    pub(crate) pocket_offset_m: Vec3,
}

/// A gun's sight line in its holding wrist's frame, in cockpit metres.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WristSight {
    pub origin: Vec3,
    /// A unit vector.
    pub direction: Vec3,
}

/// What the pose is solved from, all in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AdsInputs {
    pub holder: CockpitHand,
    pub sight: WristSight,
    /// The off-hand's grip while it holds the foregrip.
    pub support: Option<Vec3>,
    /// The aiming eye.
    pub eye: Vec3,
    /// The head's forward.
    pub forward: Vec3,
    /// The shoulder pocket on the holding hand's side, when known.
    pub(crate) pocket: Option<Vec3>,
    /// Whether the gun is scoped, which welds rather than turns.
    pub scoped: bool,
}

/// A rigid move: `p' = rotation * p + translation`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigidMove {
    pub rotation: Quat,
    pub translation: Vec3,
}

impl RigidMove {
    pub const IDENTITY: RigidMove = RigidMove {
        rotation: Quat::IDENTITY,
        translation: Vec3::ZERO,
    };

    /// A turn by `rotation` about `pivot`.
    pub fn about(pivot: Vec3, rotation: Quat) -> RigidMove {
        RigidMove {
            rotation,
            translation: pivot - rotation * pivot,
        }
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        self.rotation * p + self.translation
    }

    pub fn direction(&self, d: Vec3) -> Vec3 {
        self.rotation * d
    }

    pub fn pose(&self, p: Pose) -> Pose {
        Pose {
            position: self.point(p.position),
            orientation: (self.rotation * p.orientation).normalize(),
        }
    }

    pub fn hand(&self, h: CockpitHand) -> CockpitHand {
        CockpitHand {
            wrist: self.pose(h.wrist),
            grip: self.pose(h.grip),
            aim: self.pose(h.aim),
        }
    }

    /// This move seen from `frame`: the move of a point given in `frame`'s coordinates. A move
    /// kept in the holding wrist's frame stays with the gun as the hands move it.
    pub(crate) fn in_frame(&self, frame: Pose) -> RigidMove {
        let inverse = frame.orientation.conjugate();
        RigidMove {
            rotation: (inverse * self.rotation * frame.orientation).normalize(),
            translation: inverse
                * (self.rotation * frame.position + self.translation - frame.position),
        }
    }

    /// The move in `frame`'s coordinates brought back out of it: the inverse of
    /// [`RigidMove::in_frame`].
    pub(crate) fn out_of_frame(&self, frame: Pose) -> RigidMove {
        let rotation =
            (frame.orientation * self.rotation * frame.orientation.conjugate()).normalize();
        RigidMove {
            rotation,
            translation: frame.orientation * self.translation + frame.position
                - rotation * frame.position,
        }
    }

    /// This move, then `next`.
    pub fn then(&self, next: RigidMove) -> RigidMove {
        RigidMove {
            rotation: (next.rotation * self.rotation).normalize(),
            translation: next.rotation * self.translation + next.translation,
        }
    }
}

/// The solved pose's move and how far each step took hold.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AdsSolution {
    pub moved: RigidMove,
    /// The share of each step, `0..=1`.
    pub shoulder: f32,
    pub eye: f32,
    pub weld: f32,
    /// How far the eye was off the sight line before the sights were lined up, metres, while the
    /// eye was behind the gun and the head looked along it.
    pub eye_offset_m: Option<f32>,
}

impl AdsSolution {
    /// A pose that takes no hold: the gun where the hands hold it.
    pub const NONE: AdsSolution = AdsSolution {
        moved: RigidMove::IDENTITY,
        shoulder: 0.0,
        eye: 0.0,
        weld: 0.0,
        eye_offset_m: None,
    };
}

pub fn solve(inputs: &AdsInputs) -> AdsSolution {
    let tuning = &HANDLING.get().ads;
    let mut solution = AdsSolution::NONE;
    if let Some(pocket) = inputs.pocket
        && let Some((step, weight)) = shoulder_move(inputs, pocket)
    {
        solution.moved = step;
        solution.shoulder = weight;
    }
    let line = sight_line(inputs, solution.moved);
    let grip = solution.moved.point(inputs.holder.grip.position);
    let Some(view) = eye_view(line, grip, inputs.eye, inputs.forward) else {
        return solution;
    };
    solution.eye_offset_m = Some(view.offset.length());
    let step = if inputs.scoped {
        let weight = fade(
            view.offset.length(),
            tuning.weld_lock_m,
            tuning.weld_engage_m,
        ) * view.gate;
        solution.weld = weight;
        Some(weld_move(view, weight))
    } else {
        let weight = fade(view.offset.length(), tuning.lock_m, tuning.engage_m) * view.gate;
        solution.eye = weight;
        Some(eye_move(
            line,
            grip,
            inputs.eye,
            tuning.pivot_ahead_m,
            weight,
        ))
    };
    if let Some(step) = step {
        solution.moved = solution.moved.then(step);
    }
    solution
}

/// The gun's sight line in the cockpit frame after `moved`: (origin, direction).
fn sight_line(inputs: &AdsInputs, moved: RigidMove) -> (Vec3, Vec3) {
    let wrist = inputs.holder.wrist;
    let origin = wrist.position + wrist.orientation * inputs.sight.origin;
    let direction = (wrist.orientation * inputs.sight.direction).normalize_or_zero();
    (moved.point(origin), moved.direction(direction))
}

/// The turn that seats the butt in `pocket`, faded by its distance, and its weight.
pub(crate) fn shoulder_move(inputs: &AdsInputs, pocket: Vec3) -> Option<(RigidMove, f32)> {
    let tuning = &HANDLING.get().ads;
    let (_, direction) = sight_line(inputs, RigidMove::IDENTITY);
    let grip = inputs.holder.grip.position;
    let butt = grip - direction * tuning.butt_behind_grip_m;
    let weight = fade(
        butt.distance(pocket),
        tuning.shoulder_lock_m,
        tuning.shoulder_engage_m,
    );
    if weight <= 0.0 {
        return None;
    }
    let front = inputs.support.unwrap_or(grip);
    let from = (butt - front).try_normalize()?;
    let to = (pocket - front).try_normalize()?;
    let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(from, to), weight);
    Some((RigidMove::about(front, turn), weight))
}

/// Where the eye is relative to the sight line.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EyeView {
    /// The eye's offset from its nearest point on the line, across the line.
    pub offset: Vec3,
    /// How far the eye counts as behind the gun and the head as looking along it, `0..=1`: the
    /// share of the sights' steps the gates let through.
    pub gate: f32,
}

/// The eye's view of the line `(origin, direction)` held at `grip`: `None` unless the eye is behind
/// the grip by no more than [`AdsTuning::max_eye_behind_m`] and the head looks along the line within
/// [`AdsTuning::max_view_angle_deg`]. The gate fades to zero toward each of those bounds.
pub fn eye_view(
    (origin, direction): (Vec3, Vec3),
    grip: Vec3,
    eye: Vec3,
    forward: Vec3,
) -> Option<EyeView> {
    let tuning = &HANDLING.get().ads;
    let behind = -(eye - grip).dot(direction);
    let forward = forward.try_normalize()?;
    let angle_deg = forward.dot(direction).clamp(-1.0, 1.0).acos().to_degrees();
    let gate = fade(-behind, -tuning.eye_behind_fade_m, 0.0)
        * fade(
            behind,
            tuning.max_eye_behind_m - tuning.eye_behind_fade_m,
            tuning.max_eye_behind_m,
        )
        * fade(
            angle_deg,
            tuning.full_view_angle_deg,
            tuning.max_view_angle_deg,
        );
    if gate <= 0.0 {
        return None;
    }
    let along = (eye - origin).dot(direction);
    let offset = eye - (origin + direction * along);
    offset.is_finite().then_some(EyeView { offset, gate })
}

/// The turn about a point `pivot_ahead_m` ahead of `grip` on the line that brings the line through
/// `eye`, at `weight`.
pub(crate) fn eye_move(
    (origin, direction): (Vec3, Vec3),
    grip: Vec3,
    eye: Vec3,
    pivot_ahead_m: f32,
    weight: f32,
) -> RigidMove {
    let pivot = origin + direction * ((grip - origin).dot(direction) + pivot_ahead_m);
    let Some(through_eye) = (pivot - eye).try_normalize() else {
        return RigidMove::IDENTITY;
    };
    let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(direction, through_eye), weight);
    RigidMove::about(pivot, turn)
}

/// The move across the line that puts it through the eye, at `weight`.
pub(crate) fn weld_move(view: EyeView, weight: f32) -> RigidMove {
    RigidMove {
        rotation: Quat::IDENTITY,
        translation: view.offset * weight,
    }
}

/// `current` moved toward `target` over `dt` seconds, closing the gap with the time constant
/// [`AdsTuning::ease_s`]: a step in the target becomes a short glide, and a held target is reached.
pub fn ease(current: RigidMove, target: RigidMove, dt: f32) -> RigidMove {
    let t = follow_factor(dt, HANDLING.get().ads.ease_s);
    RigidMove {
        rotation: current.rotation.slerp(target.rotation, t).normalize(),
        translation: current.translation.lerp(target.translation, t),
    }
}

/// `1` at or under `lock`, `0` at or over `engage`, eased between.
pub fn fade(distance: f32, lock: f32, engage: f32) -> f32 {
    if !distance.is_finite() {
        return 0.0;
    }
    1.0 - smoothstep_between(lock, engage, distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> &'static AdsTuning {
        &HANDLING.get().ads
    }

    /// A right hand holding a rifle pointing down -Z, its sight line 8 cm above the grip, at head
    /// height in front of the right eye.
    fn inputs(eye: Vec3) -> AdsInputs {
        let pose = |position| Pose {
            position,
            orientation: Quat::IDENTITY,
        };
        let grip = Vec3::new(0.03, 1.5, -0.3);
        AdsInputs {
            holder: CockpitHand {
                wrist: pose(grip),
                grip: pose(grip),
                aim: pose(grip),
            },
            sight: WristSight {
                origin: Vec3::new(0.0, 0.08, 0.1),
                direction: Vec3::NEG_Z,
            },
            support: None,
            eye,
            forward: Vec3::NEG_Z,
            pocket: None,
            scoped: false,
        }
    }

    fn line_distance(solution: &AdsSolution, inputs: &AdsInputs, point: Vec3) -> f32 {
        let (o, d) = sight_line(inputs, solution.moved);
        let along = (point - o).dot(d);
        (point - (o + d * along)).length()
    }

    #[test]
    fn an_eye_close_to_the_line_is_brought_onto_it() {
        // The line runs at y = 1.58, x = 0.03; the eye is 1 cm off it, behind the grip.
        let i = inputs(Vec3::new(0.04, 1.58, -0.05));
        let s = solve(&i);
        assert!(s.eye > 0.99, "{s:?}");
        assert!(line_distance(&s, &i, i.eye) < 1e-4);
        // The pivot ahead of the grip stays put.
        let pivot = Vec3::new(0.03, 1.58, -0.3 - tuning().pivot_ahead_m);
        assert!(s.moved.point(pivot).distance(pivot) < 1e-4);
    }

    #[test]
    fn an_eye_far_from_the_line_leaves_the_gun_alone() {
        let i = inputs(Vec3::new(0.2, 1.58, -0.05));
        let s = solve(&i);
        assert_eq!(s.eye, 0.0);
        assert_eq!(s.moved.point(Vec3::ONE), Vec3::ONE);
    }

    #[test]
    fn a_gun_the_head_does_not_look_along_is_not_aimed() {
        let mut i = inputs(Vec3::new(0.04, 1.58, -0.05));
        i.forward = Vec3::X;
        assert_eq!(solve(&i).eye_offset_m, None);
        // An eye in front of the grip is not behind the sights.
        let i = inputs(Vec3::new(0.04, 1.58, -0.6));
        assert_eq!(solve(&i).eye_offset_m, None);
    }

    #[test]
    fn a_scope_is_welded_across_its_axis_without_turning() {
        let mut i = inputs(Vec3::new(0.04, 1.585, -0.1));
        i.scoped = true;
        let s = solve(&i);
        assert!(s.weld > 0.99);
        assert_eq!(s.eye, 0.0);
        assert!(s.moved.rotation.angle_between(Quat::IDENTITY) < 1e-5);
        assert!(line_distance(&s, &i, i.eye) < 1e-4);
    }

    #[test]
    fn the_butt_is_seated_in_the_pocket() {
        let mut i = inputs(Vec3::new(0.5, 3.0, 0.0));
        let butt = i.holder.grip.position + Vec3::Z * tuning().butt_behind_grip_m;
        let pocket = butt + Vec3::new(0.02, -0.02, 0.0);
        i.pocket = Some(pocket);
        let s = solve(&i);
        assert!(s.shoulder > 0.99);
        // The grip stays, and the line from it back to the pocket is the gun's.
        let grip = i.holder.grip.position;
        assert!(s.moved.point(grip).distance(grip) < 1e-5);
        let moved_butt = s.moved.point(butt);
        let back = (moved_butt - grip).normalize();
        assert!(back.dot((pocket - grip).normalize()) > 0.9999);
    }

    #[test]
    fn a_butt_away_from_the_shoulder_is_left_alone() {
        let mut i = inputs(Vec3::new(0.5, 3.0, 0.0));
        i.pocket = Some(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(solve(&i).shoulder, 0.0);
    }

    #[test]
    fn the_fade_eases_from_engage_to_lock() {
        assert_eq!(fade(0.1, 0.02, 0.06), 0.0);
        assert_eq!(fade(0.01, 0.02, 0.06), 1.0);
        assert!((fade(0.04, 0.02, 0.06) - 0.5).abs() < 1e-6);
        assert_eq!(fade(f32::NAN, 0.02, 0.06), 0.0);
        assert_eq!(fade(0.03, 0.05, 0.05), 1.0);
    }

    /// The largest change of the solved move between neighbouring inputs along a sweep, as the
    /// distance a point a metre ahead of the pivot moves, metres.
    fn largest_step(sweep: impl Iterator<Item = AdsInputs>) -> f32 {
        let probe = Vec3::new(0.03, 1.58, -1.5);
        let points: Vec<Vec3> = sweep.map(|i| solve(&i).moved.point(probe)).collect();
        points
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .fold(0.0, f32::max)
    }

    #[test]
    fn the_sights_ease_in_as_the_head_comes_to_look_along_the_gun() {
        // The eye sits a centimetre off the line, so the sights would take full hold of it at once
        // were the head's gate a step.
        let eye = Vec3::new(0.04, 1.58, -0.05);
        let sweep = (0..=400).map(|k| {
            let angle = (40.0 - k as f32 * 0.075).to_radians();
            let mut i = inputs(eye);
            i.forward = Quat::from_rotation_y(angle) * Vec3::NEG_Z;
            i
        });
        // The full turn onto the eye moves the probe more than a centimetre; no single 0.075
        // degree step of the head may carry more than a small share of it.
        assert!(largest_step(sweep) < 0.002);
        let mut looking = inputs(eye);
        looking.forward =
            Quat::from_rotation_y(tuning().full_view_angle_deg.to_radians()) * Vec3::NEG_Z;
        assert!(solve(&looking).eye > 0.99);
    }

    #[test]
    fn the_sights_ease_in_and_out_across_the_eyes_range_behind_the_grip() {
        // The eye slides along the line from in front of the grip to past the far bound.
        let sweep = (0..=600).map(|k| inputs(Vec3::new(0.04, 1.58, -0.36 + k as f32 * 0.001)));
        assert!(largest_step(sweep) < 0.002);
    }

    #[test]
    fn a_move_survives_a_round_trip_through_a_frame() {
        let frame = Pose {
            position: Vec3::new(0.2, 1.3, -0.4),
            orientation: Quat::from_euler(glam::EulerRot::YXZ, 0.7, -0.3, 0.2),
        };
        let m = RigidMove::about(Vec3::new(0.1, 1.5, -0.7), Quat::from_rotation_x(0.2));
        let local = m.in_frame(frame);
        let back = local.out_of_frame(frame);
        let p = Vec3::new(-0.3, 1.1, 0.4);
        assert!(back.point(p).distance(m.point(p)) < 1e-5);
        // In the frame, the move takes the frame's coordinates of a point to those of its image.
        let to_frame = |q: Vec3| frame.orientation.conjugate() * (q - frame.position);
        assert!(local.point(to_frame(p)).distance(to_frame(m.point(p))) < 1e-5);
    }

    #[test]
    fn the_eased_move_glides_to_a_held_target() {
        let target = RigidMove::about(Vec3::new(0.0, 1.5, -0.6), Quat::from_rotation_y(0.6));
        let probe = Vec3::new(0.0, 1.5, 0.4);
        let mut eased = RigidMove::IDENTITY;
        let mut last = eased.point(probe);
        let full = probe.distance(target.point(probe));
        for _ in 0..90 {
            eased = ease(eased, target, 1.0 / 90.0);
            let now = eased.point(probe);
            // One 90 Hz frame closes at most a fifth of the gap.
            assert!(now.distance(last) < full * 0.2);
            last = now;
        }
        assert!(last.distance(target.point(probe)) < full * 0.01);
        // No time, no movement.
        assert_eq!(ease(RigidMove::IDENTITY, target, 0.0), RigidMove::IDENTITY);
        assert_eq!(
            ease(RigidMove::IDENTITY, target, f32::NAN),
            RigidMove::IDENTITY
        );
    }

    #[test]
    fn moves_compose_in_order() {
        let a = RigidMove::about(Vec3::ZERO, Quat::from_rotation_y(0.5));
        let b = RigidMove {
            rotation: Quat::IDENTITY,
            translation: Vec3::X,
        };
        let p = Vec3::new(0.0, 0.0, 1.0);
        let both = a.then(b);
        assert!(both.point(p).distance(b.point(a.point(p))) < 1e-6);
    }
}
