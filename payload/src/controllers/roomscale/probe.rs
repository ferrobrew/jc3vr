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
//! The casts use the player aim's filter ([`crate::controllers::aim::raycast`]): the plain
//! simple-ray query can hit the player's own ragdoll and hit-proxy bodies, which a head moving down
//! or forward past the torso crosses. The casts run in the simulation phase, where the engine casts
//! its own rays.

use std::time::{Duration, Instant};

use glam::Vec3;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions, RaycastError},
    },
    config::HeadFadeConfig,
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
pub struct HeadProbe {
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
    /// body keeps in the open. `cast` returns the first hit between two points, or `None`.
    pub fn step(
        &mut self,
        head: Vec3,
        anchor: Vec3,
        mut cast: impl FnMut(Vec3, Vec3) -> Option<Vec3>,
    ) -> ProbeResult {
        let from = self.last_clear.unwrap_or(anchor);
        let hit = segment(from, head, &mut cast)
            .filter(|_| from == anchor || segment(anchor, head, &mut cast).is_some());
        match hit {
            None => {
                self.last_clear = Some(head);
                ProbeResult::Clear
            }
            Some(point) => ProbeResult::Obstructed {
                depth: (head - point).length(),
            },
        }
    }
}

/// The fade for a head `depth` metres into geometry: clear up to the start depth, black from the
/// full depth, smooth between.
pub fn fade_for_depth(depth: f32, config: &HeadFadeConfig) -> f32 {
    let start = config.start_depth_m.max(0.0);
    let full = config.full_depth_m.max(start + 1e-3);
    let t = ((depth - start) / (full - start)).clamp(0.0, 1.0);
    if t.is_finite() {
        t * t * (3.0 - 2.0 * t)
    } else {
        0.0
    }
}

/// How long a probe result stays current. The probe runs only while the on-foot locomotion runs, so
/// a stale result means the probe has stopped, and its fade must clear.
pub const STALE_AFTER: Duration = Duration::from_millis(200);

/// A probe result with the time it was taken.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TimedProbe {
    pub result: ProbeResult,
    pub at: Instant,
}

/// Cast from `from` to `to` against the scene, ignoring the `ignore` objects, and return the hit.
///
/// # Safety
///
/// As [`raycast::cast`]: the simulation phase, and live or zero ignored objects.
pub unsafe fn scene_cast(
    from: Vec3,
    to: Vec3,
    ignore: [usize; 2],
) -> Result<Option<Vec3>, RaycastError> {
    let Some(ray) = Ray::new(from, to - from) else {
        return Ok(None);
    };
    let options = CastOptions {
        ignore,
        see_through: false,
        material: false,
    };
    Ok(unsafe { raycast::cast(&ray, (to - from).length(), &options) }?.map(|hit| hit.position))
}

/// The shortest segment worth casting, metres.
const MIN_SEGMENT_M: f32 = 1e-3;

fn segment(
    from: Vec3,
    to: Vec3,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<Vec3>,
) -> Option<Vec3> {
    if (to - from).length() < MIN_SEGMENT_M {
        None
    } else {
        cast(from, to)
    }
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
        let r = probe.step(Vec3::new(0.5, 1.7, 0.0), Vec3::new(0.0, 1.7, 0.0), wall);
        assert_eq!(r, ProbeResult::Clear);
    }

    #[test]
    fn a_head_through_the_wall_is_obstructed_by_its_depth() {
        let mut probe = HeadProbe::default();
        let anchor = Vec3::new(0.0, 1.7, 0.0);
        probe.step(Vec3::new(0.9, 1.7, 0.0), anchor, wall);
        let r = probe.step(Vec3::new(1.2, 1.7, 0.0), anchor, wall);
        match r {
            ProbeResult::Obstructed { depth } => assert!((depth - 0.2).abs() < 1e-5),
            other => panic!("expected an obstruction, got {other:?}"),
        }
    }

    #[test]
    fn a_body_carried_around_the_wall_clears_a_stale_position() {
        let mut probe = HeadProbe::default();
        probe.step(Vec3::new(0.9, 1.7, 0.0), Vec3::new(0.0, 1.7, 0.0), wall);
        // The body has walked through a doorway: the head and its anchor are both past the wall.
        let r = probe.step(Vec3::new(1.5, 1.7, 0.0), Vec3::new(1.4, 1.7, 0.0), wall);
        assert_eq!(r, ProbeResult::Clear);
    }

    #[test]
    fn the_fade_ramps_between_the_depths() {
        let config = HeadFadeConfig {
            enabled: true,
            start_depth_m: 0.1,
            full_depth_m: 0.3,
        };
        assert_eq!(fade_for_depth(0.05, &config), 0.0);
        assert!((fade_for_depth(0.2, &config) - 0.5).abs() < 1e-5);
        assert_eq!(fade_for_depth(0.5, &config), 1.0);
    }
}
