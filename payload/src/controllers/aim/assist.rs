//! Per-hand aim assist: the plan of record's "Aim assist"
//! (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! The game's own aim assist works only around the gaze. Here each gun hand's ray is scored against
//! the game's own candidates, the [`TargetSystem`](jc3gi::aim::target::TargetSystem)'s sets that
//! the aim control's last pass included, with the game's own fitness function
//! ([`PlayerAimControl::GetFitnessScore`] for [`TARGET_WEAPONS`](TargetType::TARGET_WEAPONS)), run
//! with the camera getters returning the hand ray so that its cone is the hand's. Only hostile
//! targets count, as for the game's weapon targets. Of the candidates inside the cone, the best
//! scored one with a clear line from the hand is taken, and the hand's aim point moves toward it by
//! [`AimConfig::assist_strength`]: zero leaves the aim alone (the default), one aims at the target.
//!
//! The candidate query reads the target system rather than casting shapes: the target system
//! already holds every candidate the game's own assist can pick, and its fitness function is what
//! the game tunes per weapon.
//!
//! The grapple has an assist of its own ([`grapple_assist`]), run before the aim update so that
//! the game's own grapple targeting runs along the assisted ray. When the grapple hand's ray does
//! not already hit an object, the candidates in every set within
//! [`AimConfig::grapple_assist_cone_deg`] of the ray that the game's grapple scoring
//! ([`PlayerAimControl::SetTargetGrappleFitness`]) accepts (an object the grapple attaches to, in
//! range, not the one the wire holds) and that are in sight are ranked by their angle from the
//! ray, and the ray turns toward the nearest by [`AimConfig::grapple_assist_strength`].

use std::f32::consts::FRAC_PI_2;

use glam::{Quat, Vec3};
use jc3gi::{
    aim::{
        aim::PlayerAimControl,
        target::{FactionRelation, Target, TargetType},
    },
    types::math::Matrix4,
};

use crate::controllers::{
    aim::{
        grapple,
        ray::{self, Ray},
        raycast::{self, CastOptions},
        targets::{self, TargetSet},
    },
    config::AimConfig,
};

/// The farthest a candidate may be from the hand for aim assist, metres.
const MAX_DISTANCE_M: f32 = 150.0;

/// How far short of a candidate's body position the line of sight to it may be blocked and still
/// count as clear, metres: the line ends inside the target's own body.
const OCCLUSION_TOLERANCE_M: f32 = 0.75;

/// One hand's correction on the last aim update.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AssistCorrection {
    /// The candidate's body position.
    pub target: Vec3,
    /// How far the aim point's direction was turned, radians.
    pub angle_rad: f32,
}

/// The aim point that moves `aim_point` toward `candidate` by `strength`, both seen from `origin`:
/// the direction turns by that fraction of the angle between them and the distance moves by that
/// fraction of the difference. `None` when either point is at the origin or `strength` is not
/// positive.
pub fn assisted_point(
    origin: Vec3,
    aim_point: Vec3,
    candidate: Vec3,
    strength: f32,
) -> Option<Vec3> {
    if strength.is_nan() || strength <= 0.0 {
        return None;
    }
    let strength = strength.min(1.0);
    let to_aim = aim_point - origin;
    let to_candidate = candidate - origin;
    let (aim_dir, candidate_dir) = (to_aim.try_normalize()?, to_candidate.try_normalize()?);
    let turn = Quat::from_rotation_arc(aim_dir, candidate_dir);
    let direction = Quat::IDENTITY.slerp(turn, strength) * aim_dir;
    let distance = to_aim.length() + (to_candidate.length() - to_aim.length()) * strength;
    let point = origin + direction * distance;
    point.is_finite().then_some(point)
}

/// The assisted aim point for a gun hand whose sight line is `ray` and whose aim point is
/// `aim_point`, and the correction made, or `None` without a candidate.
///
/// # Safety
///
/// `aim` must be the local player's live aim control, called from its post-simulation update after
/// the original, on that thread, with `options` naming live objects.
pub(super) unsafe fn assist(
    aim: *mut PlayerAimControl,
    ray: &Ray,
    aim_point: Vec3,
    options: &CastOptions,
    config: &AimConfig,
) -> Option<(Vec3, AssistCorrection)> {
    if config.assist_strength.is_nan() || config.assist_strength <= 0.0 {
        return None;
    }
    let candidate = unsafe { best_candidate(aim, ray, options) }?;
    let point = assisted_point(ray.origin, aim_point, candidate, config.assist_strength)?;
    let angle_rad = (aim_point - ray.origin)
        .try_normalize()
        .zip((point - ray.origin).try_normalize())
        .map_or(0.0, |(a, b)| a.angle_between(b));
    Some((
        point,
        AssistCorrection {
            target: candidate,
            angle_rad,
        },
    ))
}

/// The grapple's aim assist on the last aim update.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrappleAssist {
    /// The candidate's body position.
    pub target: Vec3,
    /// How far the grapple's ray was turned, radians.
    pub angle_rad: f32,
}

/// The grapple's ray turned toward the valid grapple target nearest `ray` within the configured
/// cone, and the turn made, or `None` when the assist is off, the ray already hits an object, or
/// no candidate qualifies.
///
/// # Safety
///
/// `aim` must be the local player's live aim control, called from its post-simulation update
/// before the original, on that thread, with `options` naming live objects and no grapple view set.
pub(super) unsafe fn grapple_assist(
    aim: *mut PlayerAimControl,
    ray: &Ray,
    options: &CastOptions,
    config: &AimConfig,
) -> Option<(Ray, GrappleAssist)> {
    let strength = config.grapple_assist_strength;
    let cone = config.grapple_assist_cone_deg.to_radians();
    if strength.is_nan() || strength <= 0.0 || cone.is_nan() || cone <= 0.0 {
        return None;
    }
    // A ray on an object already has the game's own grapple target, which the assist would only
    // take away.
    let hit = unsafe { raycast::cast(ray, MAX_DISTANCE_M, options) }.ok()?;
    if hit.is_some_and(|h| h.object.is_some()) {
        return None;
    }
    let aim = unsafe { aim.as_mut() }?;
    let view = Matrix4::from(ray::view_matrix(ray));
    let mut candidates = grapple::with_view(view, || {
        let mut candidates = Vec::new();
        for set in TargetSet::ALL {
            for target in unsafe { targets::targets(set) } {
                if let Some(candidate) = unsafe { grapple_candidate(aim, target, ray, cone) } {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    });
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let target = candidates
        .into_iter()
        .find(|(_, position)| unsafe {
            raycast::line_of_sight(ray.origin, *position, OCCLUSION_TOLERANCE_M, options)
        })
        .map(|(_, position)| position)?;
    let bent = assisted_ray(ray, target, strength)?;
    Some((
        bent,
        GrappleAssist {
            target,
            angle_rad: bent.direction.angle_between(ray.direction),
        },
    ))
}

/// `ray` turned toward `target` by `strength` of the angle between them: one aims at it.
pub fn assisted_ray(ray: &Ray, target: Vec3, strength: f32) -> Option<Ray> {
    let distance = (target - ray.origin).length();
    let point = assisted_point(ray.origin, ray.at(distance), target, strength)?;
    Ray::new(ray.origin, point - ray.origin)
}

/// Whether `position` lies within `cone` radians of `ray` and within the assist's reach, and its
/// angle from the ray when it does.
fn angle_in_cone(ray: &Ray, position: Vec3, cone: f32) -> Option<f32> {
    ray.offset(position)
        .filter(|offset| offset.within(MAX_DISTANCE_M, cone))
        .map(|offset| offset.angle)
}

/// A grapple candidate's angle from `ray` and its body position, when it is within `cone` of the
/// ray and the game's grapple scoring accepts it.
///
/// # Safety
///
/// As [`grapple_assist`], inside the view override.
unsafe fn grapple_candidate(
    aim: &mut PlayerAimControl,
    target: *mut Target,
    ray: &Ray,
    cone: f32,
) -> Option<(f32, Vec3)> {
    let object = unsafe { targets::game_object(target) }?;
    let position = unsafe { targets::body_position(target) }?;
    let angle = angle_in_cone(ray, position, cone)?;
    // The game's own grapple scoring: the object kinds the grapple attaches to, in range, and not
    // the object the wire already holds. The fitness it writes on the target is reset by the
    // game's own target pass in the update that follows.
    unsafe { aim.SetTargetGrappleFitness(object, target) }.then_some((angle, position))
}

/// The body position of the best-scored hostile candidate in the ray's cone with a clear line from
/// the ray's origin.
///
/// # Safety
///
/// As [`assist`].
unsafe fn best_candidate(
    aim: *mut PlayerAimControl,
    ray: &Ray,
    options: &CastOptions,
) -> Option<Vec3> {
    let aim = unsafe { aim.as_mut() }?;
    let included = unsafe { targets::included_sets() };
    let view = Matrix4::from(ray::view_matrix(ray));
    let mut scored = grapple::with_view(view, || {
        let mut scored = Vec::new();
        for (set, _) in TargetSet::ALL
            .into_iter()
            .zip(included)
            .filter(|(_, included)| *included)
        {
            for target in unsafe { targets::targets(set) } {
                if let Some(candidate) = unsafe { score(aim, target, ray) } {
                    scored.push(candidate);
                }
            }
        }
        scored
    });
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
        .into_iter()
        .find(|(_, position)| unsafe {
            raycast::line_of_sight(ray.origin, *position, OCCLUSION_TOLERANCE_M, options)
        })
        .map(|(_, position)| position)
}

/// The game's weapon fitness of `target` around the current view, and its body position, when it
/// is a live hostile candidate ahead of the ray within reach.
///
/// # Safety
///
/// As [`assist`], inside the view override.
unsafe fn score(aim: &mut PlayerAimControl, target: *mut Target, ray: &Ray) -> Option<(f32, Vec3)> {
    unsafe { targets::game_object(target) }?;
    // Read as an integer: the relation is refreshed by the game and may hold a value the enum does
    // not name.
    let relation = unsafe {
        (&raw const (*target).m_PlayerFactionRelation)
            .cast::<i32>()
            .read()
    };
    if relation != FactionRelation::E_RELATION_HOSTILE as i32 {
        return None;
    }
    let position = unsafe { targets::body_position(target) }?;
    let ahead = ray
        .offset(position)
        .is_some_and(|offset| offset.distance <= MAX_DISTANCE_M && offset.angle < FRAC_PI_2);
    if !ahead {
        return None;
    }
    let mut fitness = 0.0f32;
    let inside = unsafe { aim.GetFitnessScore(target, TargetType::TARGET_WEAPONS, &mut fitness) };
    (inside && fitness.is_finite()).then_some((fitness, position))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn no_strength_leaves_the_aim_alone() {
        let p = assisted_point(Vec3::ZERO, Vec3::NEG_Z * 10.0, Vec3::X * 10.0, 0.0);
        assert_eq!(p, None);
    }

    #[test]
    fn full_strength_aims_at_the_candidate() {
        let p = assisted_point(
            Vec3::ZERO,
            Vec3::NEG_Z * 10.0,
            Vec3::new(1.0, 0.0, -20.0),
            1.0,
        );
        assert!(close(p.unwrap(), Vec3::new(1.0, 0.0, -20.0)));
    }

    #[test]
    fn half_strength_turns_half_the_angle() {
        let origin = Vec3::new(0.0, 1.0, 0.0);
        let aim = origin + Vec3::NEG_Z * 10.0;
        let candidate = origin + Vec3::new(-10.0, 0.0, 0.0);
        let p = assisted_point(origin, aim, candidate, 0.5).unwrap();
        let angle = (p - origin).angle_between(Vec3::NEG_Z);
        assert!((angle - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
        assert!(((p - origin).length() - 10.0).abs() < 1e-4);
    }

    #[test]
    fn the_grapple_ray_turns_onto_a_target_in_the_cone() {
        let ray = Ray::new(Vec3::new(0.0, 1.5, 0.0), Vec3::NEG_Z).unwrap();
        let target = ray.origin + Vec3::new(1.0, 0.5, -20.0);
        let angle = angle_in_cone(&ray, target, 5f32.to_radians()).unwrap();
        assert!(angle > 0.0);
        let bent = assisted_ray(&ray, target, 1.0).unwrap();
        assert!(bent.direction.angle_between(target - ray.origin) < 1e-3);
        assert_eq!(bent.origin, ray.origin);
        let half = assisted_ray(&ray, target, 0.5).unwrap();
        assert!((half.direction.angle_between(ray.direction) - angle * 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_target_outside_the_cone_or_behind_is_not_a_grapple_candidate() {
        let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
        let cone = 5f32.to_radians();
        assert!(angle_in_cone(&ray, Vec3::new(5.0, 0.0, -20.0), cone).is_none());
        assert!(angle_in_cone(&ray, Vec3::new(0.0, 0.0, 20.0), cone).is_none());
        assert!(angle_in_cone(&ray, Vec3::ZERO, cone).is_none());
        assert!(angle_in_cone(&ray, Vec3::NEG_Z * (MAX_DISTANCE_M + 1.0), cone).is_none());
        // The nearer to the ray ranks first.
        let near = angle_in_cone(&ray, Vec3::new(0.5, 0.0, -20.0), cone).unwrap();
        let far = angle_in_cone(&ray, Vec3::new(1.5, 0.0, -20.0), cone).unwrap();
        assert!(near < far);
    }

    #[test]
    fn a_point_at_the_origin_is_not_assisted() {
        assert_eq!(assisted_point(Vec3::ZERO, Vec3::ZERO, Vec3::X, 0.5), None);
        assert_eq!(assisted_point(Vec3::ZERO, Vec3::X, Vec3::ZERO, 0.5), None);
    }
}
