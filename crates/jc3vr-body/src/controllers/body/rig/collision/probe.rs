//! The head-through-geometry probe: whether the physical head has gone somewhere the body cannot
//! follow, and how deep.
//!
//! The view stays with the real head whatever the body does, so a player who leans or walks into a
//! wall sees through it. Each simulation tick a ray is cast from the last position the head held in
//! the open to where it is now; a hit means geometry lies between them. A second ray from the
//! animated head (which the body's capsule keeps in the open) forgives a stale last position, so
//! that a head the body has carried around the obstruction counts as clear again. The depth past the
//! hit drives the comfort fade.
//!
//! A single ray through the eye centre lets the head's sides through a corner or a door frame, so
//! the pair is cast three times, through the eye centre and the rig tuning's
//! `probe.fan_half_width_m` to either side of it across the view, and the deepest hit counts. The
//! fan stands in for a swept sphere.
//!
//! The animated head is a poor witness when the clip itself has put it in the world (a wingsuit
//! flight ending in the ground carries the head and the eyes under the terrain together), so the
//! terrain is checked on its own as well: a short ray down onto the eyes from above them meets the
//! heightfield when the eyes are below its surface, and the depth below counts as for any other hit
//! ([`below_terrain`]).
//!
//! The casts use the player aim's filter ([`crate::controllers::aim::raycast`]): the plain
//! simple-ray query can hit the player's own ragdoll and hit-proxy bodies, which a head moving down
//! or forward past the torso crosses. The casts run in the simulation phase, where the engine casts
//! its own rays.

use body_rig::math::{geometry::cast_segment, smooth::smoothstep_between};
use glam::Vec3;
use jc3vr_core::published::{Stamp, Window};

use crate::{
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions, RaycastError},
        },
        body::rig::tuning::RIG,
    },
    tuning::FRESHNESS,
};

/// The probe's result for one tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProbeResult {
    /// The head is in the open.
    Clear,
    /// Geometry separates the head from where it last was in the open; the head is `depth` metres
    /// past the obstruction.
    Obstructed { depth: f32 },
}

impl ProbeResult {
    pub fn depth(self) -> f32 {
        match self {
            ProbeResult::Clear => 0.0,
            ProbeResult::Obstructed { depth } => depth,
        }
    }
}

/// The probe's memory across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct HeadProbe {
    last_clear: Option<Vec3>,
}

impl HeadProbe {
    pub const fn new() -> Self {
        Self { last_clear: None }
    }

    /// Forget the last clear position, after a teleport or while the probe is not running.
    pub fn reset(&mut self) {
        self.last_clear = None;
    }

    /// Probe the `head` position against the geometry. `anchor` is the animated head, which the
    /// body keeps in the open, and `right` the view's horizontal right, a unit vector, which the fan
    /// spreads along. `cast` returns the first hit between two points, or `None`.
    pub fn step(
        &mut self,
        head: Vec3,
        anchor: Vec3,
        right: Vec3,
        mut cast: impl FnMut(Vec3, Vec3) -> Option<Vec3>,
    ) -> ProbeResult {
        let right = if right.is_finite() { right } else { Vec3::ZERO };
        let start = self.last_clear.unwrap_or(anchor);
        let fan_half_width = RIG.get().probe.fan_half_width_m;
        let mut deepest: Option<f32> = None;
        for side in [0.0, 1.0, -1.0] {
            let offset = right * (side * fan_half_width);
            let (from, to, anchor) = (start + offset, head + offset, anchor + offset);
            let hit = cast_segment(from, to, &mut cast)
                .filter(|_| from == anchor || cast_segment(anchor, to, &mut cast).is_some());
            if let Some(point) = hit {
                let depth = (to - point).length();
                deepest = Some(deepest.map_or(depth, |d| d.max(depth)));
            }
        }
        match deepest {
            None => {
                self.last_clear = Some(head);
                ProbeResult::Clear
            }
            Some(depth) => ProbeResult::Obstructed { depth },
        }
    }
}

/// How far below the terrain's surface the eyes are, metres, or `None` when they are above it or
/// further below than the rig tuning's `probe.terrain_reach_m`. `cast` returns the first terrain
/// hit between two points.
pub(crate) fn below_terrain(
    eyes: Vec3,
    mut cast: impl FnMut(Vec3, Vec3) -> Option<Vec3>,
) -> Option<f32> {
    if !eyes.is_finite() {
        return None;
    }
    let above = eyes + Vec3::Y * RIG.get().probe.terrain_reach_m;
    cast(above, eyes)
        .map(|surface| surface.y - eyes.y)
        .filter(|depth| depth.is_finite() && *depth > 0.0)
}

/// The deeper of a scene probe's result and a terrain check's depth.
pub(crate) fn with_terrain(result: ProbeResult, terrain: Option<f32>) -> ProbeResult {
    match terrain {
        Some(depth) if depth > result.depth() => ProbeResult::Obstructed { depth },
        _ => result,
    }
}

/// Cast from `from` to `to` against the terrain alone and return the hit.
///
/// # Safety
///
/// As [`raycast::cast_terrain`].
pub(crate) unsafe fn terrain_cast(from: Vec3, to: Vec3) -> Result<Option<Vec3>, RaycastError> {
    let Some(ray) = Ray::new(from, to - from) else {
        return Ok(None);
    };
    unsafe { raycast::cast_terrain(&ray, (to - from).length()) }
}

/// The head fade for a head `depth` metres into geometry, between the rig tuning's
/// `probe.head_fade_start_m` and `probe.head_fade_full_m`.
pub fn head_fade(depth: f32) -> f32 {
    let probe = &RIG.get().probe;
    fade_for_depth(depth, probe.head_fade_start_m, probe.head_fade_full_m)
}

/// The fade for a head `depth` metres into geometry: clear up to `start_m`, black from `full_m`,
/// smooth between.
pub(crate) fn fade_for_depth(depth: f32, start_m: f32, full_m: f32) -> f32 {
    let start = start_m.max(0.0);
    let full = full_m.max(start + 1e-3);
    smoothstep_between(start, full, depth)
}

/// How long a probe result stays current: the freshness tuning's `rig.probe_ms`.
pub fn stale_after() -> Window {
    FRESHNESS.get().rig.probe_ms.window()
}

/// A probe result with the time it was taken.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TimedProbe {
    pub result: ProbeResult,
    pub at: Stamp,
}

/// Cast from `from` to `to` against the scene with `options`, and return the hit.
///
/// # Safety
///
/// As [`raycast::cast`]: the simulation phase, and live or zero ignored objects.
pub(crate) unsafe fn scene_cast(
    from: Vec3,
    to: Vec3,
    options: &CastOptions,
) -> Result<Option<Vec3>, RaycastError> {
    let Some(ray) = Ray::new(from, to - from) else {
        return Ok(None);
    };
    Ok(unsafe { raycast::cast(&ray, (to - from).length(), options) }?.map(|hit| hit.position))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall at `x = 1`.
    fn wall(from: Vec3, to: Vec3) -> Option<Vec3> {
        if (from.x < 1.0) != (to.x < 1.0) {
            let t = (1.0 - from.x) / (to.x - from.x);
            Some(from + (to - from) * t)
        } else {
            None
        }
    }

    #[test]
    fn a_head_in_the_open_is_clear() {
        let mut probe = HeadProbe::default();
        let r = probe.step(
            Vec3::new(0.5, 1.7, 0.0),
            Vec3::new(0.0, 1.7, 0.0),
            Vec3::Z,
            wall,
        );
        assert_eq!(r, ProbeResult::Clear);
    }

    #[test]
    fn a_head_through_the_wall_is_obstructed_by_its_depth() {
        let mut probe = HeadProbe::default();
        let anchor = Vec3::new(0.0, 1.7, 0.0);
        probe.step(Vec3::new(0.9, 1.7, 0.0), anchor, Vec3::Z, wall);
        let r = probe.step(Vec3::new(1.2, 1.7, 0.0), anchor, Vec3::Z, wall);
        match r {
            ProbeResult::Obstructed { depth } => assert!((depth - 0.2).abs() < 1e-5),
            other => panic!("expected an obstruction, got {other:?}"),
        }
    }

    #[test]
    fn a_body_carried_around_the_wall_clears_a_stale_position() {
        let mut probe = HeadProbe::default();
        probe.step(
            Vec3::new(0.9, 1.7, 0.0),
            Vec3::new(0.0, 1.7, 0.0),
            Vec3::Z,
            wall,
        );
        // The body has walked through a doorway: the head and its anchor are both past the wall.
        let r = probe.step(
            Vec3::new(1.5, 1.7, 0.0),
            Vec3::new(1.4, 1.7, 0.0),
            Vec3::Z,
            wall,
        );
        assert_eq!(r, ProbeResult::Clear);
    }

    /// A door frame's post: a wall at `x = 1` only for `z > 0.05`.
    fn post(from: Vec3, to: Vec3) -> Option<Vec3> {
        wall(from, to).filter(|hit| hit.z > 0.05)
    }

    #[test]
    fn the_fan_catches_a_post_beside_the_eye_centre() {
        let mut probe = HeadProbe::default();
        let anchor = Vec3::new(0.0, 1.7, 0.0);
        probe.step(Vec3::new(0.9, 1.7, 0.0), anchor, Vec3::Z, post);
        // The eye centre passes beside the post; the side of the head does not.
        let r = probe.step(Vec3::new(1.1, 1.7, 0.0), anchor, Vec3::Z, post);
        assert!(matches!(r, ProbeResult::Obstructed { .. }), "{r:?}");
        let mut single = HeadProbe::default();
        single.step(Vec3::new(0.9, 1.7, 0.0), anchor, Vec3::ZERO, post);
        assert_eq!(
            single.step(Vec3::new(1.1, 1.7, 0.0), anchor, Vec3::ZERO, post),
            ProbeResult::Clear
        );
    }

    /// Flat ground at `y = 0`.
    fn ground(from: Vec3, to: Vec3) -> Option<Vec3> {
        ((from.y < 0.0) != (to.y < 0.0)).then(|| from + (to - from) * (from.y / (from.y - to.y)))
    }

    #[test]
    fn eyes_under_the_terrain_are_as_deep_as_the_surface_above() {
        assert_eq!(below_terrain(Vec3::new(0.0, 1.6, 0.0), ground), None);
        let depth = below_terrain(Vec3::new(3.0, -0.3, 1.0), ground).unwrap();
        assert!((depth - 0.3).abs() < 1e-5, "{depth}");
        assert_eq!(
            below_terrain(
                Vec3::new(0.0, -RIG.get().probe.terrain_reach_m - 0.5, 0.0),
                ground,
            ),
            None
        );
        assert_eq!(
            with_terrain(ProbeResult::Clear, Some(0.3)),
            ProbeResult::Obstructed { depth: 0.3 }
        );
        assert_eq!(
            with_terrain(ProbeResult::Obstructed { depth: 0.5 }, Some(0.3)),
            ProbeResult::Obstructed { depth: 0.5 }
        );
        assert_eq!(with_terrain(ProbeResult::Clear, None), ProbeResult::Clear);
    }

    #[test]
    fn the_fade_ramps_between_the_depths() {
        assert_eq!(fade_for_depth(0.05, 0.1, 0.3), 0.0);
        assert!((fade_for_depth(0.2, 0.1, 0.3) - 0.5).abs() < 1e-5);
        assert_eq!(fade_for_depth(0.5, 0.1, 0.3), 1.0);
    }
}
