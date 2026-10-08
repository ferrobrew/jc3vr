//! The palm's contacts, pure: whether each probe point of a palm has passed into a surface since it
//! was last in the open, and how deep.
//!
//! Each tick a cast runs from the last position the point held in the open to where it is now, as
//! the head probe does ([`crate::controllers::roomscale::probe`]): a hit means the point crossed a
//! surface. Against a solid surface (world geometry or an object), the point is pressed in by its
//! depth behind the surface, and its last open position stays where it was until the hand comes
//! back out. A second cast from an anchor the body keeps in the open (the shoulder) forgives a stale
//! open position, so that a hand carried around an obstacle counts as clear again. A character is
//! passed through: it is never pressed against, so the point counts as in the open at once, and the
//! next tick's cast starts from inside the character, which it does not hit.

use std::num::NonZeroUsize;

use glam::Vec3;

/// What a cast hit.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Surface {
    /// World geometry or an object that is not a character: it holds the palm out. `object` is the
    /// hit physics game object, or `None` for world geometry; `body` is the hit Havok body.
    Solid {
        object: Option<NonZeroUsize>,
        body: u32,
    },
    /// A character, which the palm passes through. `object` is the character, and `body` the hit
    /// Havok body: its proxy's capsule, or one of its ragdoll's bodies.
    Character { object: NonZeroUsize, body: u32 },
}

/// A cast's hit.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SurfaceHit {
    pub position: Vec3,
    /// The surface normal, toward the cast's origin.
    pub normal: Vec3,
    pub surface: Surface,
    /// The surface's game material, when the cast read it.
    pub material: Option<u32>,
}

/// One probe point's contact on a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PointContact {
    Clear,
    /// The point is `depth` metres behind the solid surface it crossed.
    Pressed {
        hit: SurfaceHit,
        depth: f32,
    },
}

/// One probe point's memory across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PointProbe {
    last_clear: Option<Vec3>,
}

impl PointProbe {
    pub const fn new() -> Self {
        Self { last_clear: None }
    }

    /// Forget the last open position: after a teleport, or while the palm is not acting.
    pub fn reset(&mut self) {
        self.last_clear = None;
    }

    /// Probe the point at `point`. `anchor` is a position the body keeps in the open. `cast`
    /// returns the first hit between two points, or `None`.
    pub fn step(
        &mut self,
        point: Vec3,
        anchor: Vec3,
        cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
    ) -> PointContact {
        let from = self.last_clear.unwrap_or(anchor);
        let Some(hit) = segment(from, point, cast) else {
            self.last_clear = Some(point);
            return PointContact::Clear;
        };
        if matches!(hit.surface, Surface::Character { .. }) {
            self.last_clear = Some(point);
            return PointContact::Clear;
        }
        if from != anchor && segment(anchor, point, cast).is_none() {
            self.last_clear = Some(point);
            return PointContact::Clear;
        }
        PointContact::Pressed {
            hit,
            depth: depth_behind(hit, point),
        }
    }
}

/// How far `point` lies behind the surface `hit` lies on, metres, along its normal; zero in front.
pub fn depth_behind(hit: SurfaceHit, point: Vec3) -> f32 {
    let depth = (hit.position - point).dot(hit.normal);
    if depth.is_finite() {
        depth.max(0.0)
    } else {
        0.0
    }
}

/// A palm's contacts on a tick, folded from its probe points.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandContact {
    /// The deepest press into a solid surface, and its depth.
    pub press: Option<(SurfaceHit, f32)>,
}

impl HandContact {
    pub fn fold(points: &[PointContact]) -> Self {
        let mut out = Self::default();
        for contact in points {
            match *contact {
                PointContact::Clear => {}
                PointContact::Pressed { hit, depth } => {
                    if out.press.is_none_or(|(_, d)| depth > d) {
                        out.press = Some((hit, depth));
                    }
                }
            }
        }
        out
    }

    /// The offset that carries the palm back out to the surface it presses into, world metres.
    pub fn clamp_offset(&self) -> Vec3 {
        self.press.map_or(Vec3::ZERO, |(hit, depth)| {
            hit.normal.normalize_or_zero() * depth
        })
    }
}

/// The shortest segment worth casting, metres.
const MIN_SEGMENT_M: f32 = 1e-3;

fn segment(
    from: Vec3,
    to: Vec3,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> Option<SurfaceHit> {
    if (to - from).length() < MIN_SEGMENT_M {
        None
    } else {
        cast(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALL: Surface = Surface::Solid {
        object: None,
        body: 1,
    };

    /// A wall at `x = 1` facing -X, and a character occupying `x` in `5..6`.
    fn scene(from: Vec3, to: Vec3) -> Option<SurfaceHit> {
        let crossing = |x: f32| {
            ((from.x < x) != (to.x < x))
                .then(|| from + (to - from) * ((x - from.x) / (to.x - from.x)))
        };
        if let Some(position) = crossing(1.0) {
            return Some(SurfaceHit {
                position,
                normal: if from.x < 1.0 { Vec3::NEG_X } else { Vec3::X },
                surface: WALL,
                material: None,
            });
        }
        if from.x < 5.0
            && let Some(position) = crossing(5.0)
        {
            return Some(SurfaceHit {
                position,
                normal: Vec3::NEG_X,
                surface: Surface::Character {
                    object: NonZeroUsize::new(64).unwrap(),
                    body: 9,
                },
                material: None,
            });
        }
        None
    }

    #[test]
    fn a_point_pressed_into_the_wall_reports_its_depth() {
        let mut probe = PointProbe::new();
        let anchor = Vec3::ZERO;
        assert_eq!(
            probe.step(Vec3::new(0.8, 0.0, 0.0), anchor, &mut scene),
            PointContact::Clear
        );
        match probe.step(Vec3::new(1.15, 0.0, 0.0), anchor, &mut scene) {
            PointContact::Pressed { hit, depth } => {
                assert!((depth - 0.15).abs() < 1e-5);
                assert_eq!(hit.surface, WALL);
            }
            other => panic!("expected a press, got {other:?}"),
        }
        // Still in: the open position stays outside, so the press holds.
        assert!(matches!(
            probe.step(Vec3::new(1.1, 0.2, 0.0), anchor, &mut scene),
            PointContact::Pressed { .. }
        ));
        // Back out.
        assert_eq!(
            probe.step(Vec3::new(0.9, 0.0, 0.0), anchor, &mut scene),
            PointContact::Clear
        );
    }

    #[test]
    fn a_stale_open_position_is_forgiven_by_the_anchor() {
        let mut probe = PointProbe::new();
        probe.step(Vec3::new(0.9, 0.0, 0.0), Vec3::ZERO, &mut scene);
        // The body has gone through a doorway: the anchor and the point are both past the wall.
        assert_eq!(
            probe.step(
                Vec3::new(1.6, 0.0, 0.0),
                Vec3::new(1.4, 0.0, 0.0),
                &mut scene
            ),
            PointContact::Clear
        );
    }

    #[test]
    fn a_character_is_passed_through_and_never_pressed() {
        let mut probe = PointProbe::new();
        let anchor = Vec3::new(4.0, 0.0, 0.0);
        probe.step(Vec3::new(4.8, 0.0, 0.0), anchor, &mut scene);
        assert_eq!(
            probe.step(Vec3::new(5.2, 0.0, 0.0), anchor, &mut scene),
            PointContact::Clear
        );
        // The next tick starts inside the character and hits nothing.
        assert_eq!(
            probe.step(Vec3::new(5.3, 0.0, 0.0), anchor, &mut scene),
            PointContact::Clear
        );
    }

    #[test]
    fn the_fold_keeps_the_deepest_press() {
        let hit = |x: f32| SurfaceHit {
            position: Vec3::new(x, 0.0, 0.0),
            normal: Vec3::NEG_X,
            surface: WALL,
            material: None,
        };
        let contacts = [
            PointContact::Clear,
            PointContact::Pressed {
                hit: hit(1.0),
                depth: 0.05,
            },
            PointContact::Pressed {
                hit: hit(1.0),
                depth: 0.12,
            },
        ];
        let fold = HandContact::fold(&contacts);
        assert_eq!(fold.press.map(|(_, d)| d), Some(0.12));
        let offset = fold.clamp_offset();
        assert!((offset - Vec3::new(-0.12, 0.0, 0.0)).length() < 1e-6);
        assert_eq!(HandContact::default().clamp_offset(), Vec3::ZERO);
    }

    #[test]
    fn a_point_in_front_of_its_surface_has_no_depth() {
        let hit = SurfaceHit {
            position: Vec3::X,
            normal: Vec3::NEG_X,
            surface: WALL,
            material: None,
        };
        assert_eq!(depth_behind(hit, Vec3::new(0.5, 0.0, 0.0)), 0.0);
        assert!((depth_behind(hit, Vec3::new(1.25, 0.0, 0.0)) - 0.25).abs() < 1e-6);
    }
}
