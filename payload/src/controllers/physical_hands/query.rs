//! Physical hand ray queries with pane-local glass continuation.

use glam::Vec3;
use jc3gi::physics::{GameMaterialId, PhysicsSystem};

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions, RaycastError},
    },
    glass::bullets::names_hidden_pane,
    physical_hands::{contact::SurfaceHit, engine},
};

/// # Safety
///
/// Run on the simulation thread outside the physics step. Ignored objects must be live or zero.
pub(crate) unsafe fn cast(
    from: Vec3,
    to: Vec3,
    options: &CastOptions,
) -> Result<Option<SurfaceHit>, RaycastError> {
    unsafe { cast_with_policy(from, to, options, GlassPolicy::Hidden) }
}

/// # Safety
///
/// Run on the simulation thread outside the physics step. Ignored objects must be live or zero.
pub(crate) unsafe fn cast_solid(
    from: Vec3,
    to: Vec3,
    options: &CastOptions,
) -> Result<Option<SurfaceHit>, RaycastError> {
    unsafe { cast_with_policy(from, to, options, GlassPolicy::All) }
}

const FORWARD_STEP_M: f32 = 0.001;
const MAX_CASTS: usize = 16;

unsafe fn cast_with_policy(
    from: Vec3,
    to: Vec3,
    options: &CastOptions,
    policy: GlassPolicy,
) -> Result<Option<SurfaceHit>, RaycastError> {
    continue_cast(from, to, options, |ray, distance, options| unsafe {
        raycast::cast_on_with(
            ray,
            distance,
            options,
            PhysicsSystem::QUERY_LAYER,
            |hit, info| {
                let surface = engine::surface(&hit);
                let skip = policy.skips(surface.material, || {
                    info.base.m_HitObjectId != 0 && names_hidden_pane(&mut info.base)
                });
                (surface, skip)
            },
        )
    })
}

#[derive(Clone, Copy)]
enum GlassPolicy {
    Hidden,
    All,
}

impl GlassPolicy {
    fn skips(self, material: Option<u32>, hidden: impl FnOnce() -> bool) -> bool {
        material == Some(GameMaterialId::EGameMaterialId_Glass as u32)
            && (matches!(self, Self::All) || hidden())
    }
}

fn continue_cast(
    from: Vec3,
    to: Vec3,
    options: &CastOptions,
    mut cast: impl FnMut(&Ray, f32, &CastOptions) -> Result<Option<(SurfaceHit, bool)>, RaycastError>,
) -> Result<Option<SurfaceHit>, RaycastError> {
    let segment = to - from;
    let length = segment.length();
    let ray = Ray::new(from, segment).ok_or(RaycastError::InvalidDistance)?;
    if !to.is_finite() || !length.is_finite() || length <= 0.0 {
        return Err(RaycastError::InvalidDistance);
    }
    let options = CastOptions {
        material: true,
        ..*options
    };
    let mut travelled = 0.0;
    let mut origin = from;
    for _ in 0..MAX_CASTS {
        let remaining = length - travelled;
        let current = Ray { origin, ..ray };
        let Some((hit, skip)) = cast(&current, remaining, &options)? else {
            return Ok(None);
        };
        if !skip {
            return Ok(Some(hit));
        }
        let along = (hit.position - origin).dot(ray.direction);
        if !hit.position.is_finite() || !(0.0..remaining).contains(&along) {
            return Err(RaycastError::InvalidDistance);
        }
        let next = travelled + along + FORWARD_STEP_M;
        if next >= length || next <= travelled {
            return Ok(None);
        }
        let next_origin = ray.at(next);
        if next_origin == origin || !next_origin.is_finite() {
            return Ok(None);
        }
        travelled = next;
        origin = next_origin;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use test_case::test_case;

    use super::*;
    use crate::controllers::physical_hands::contact::Surface;

    const GLASS: u32 = GameMaterialId::EGameMaterialId_Glass as u32;

    fn hit(x: f32, material: Option<u32>) -> SurfaceHit {
        SurfaceHit {
            position: Vec3::new(x, 0.0, 0.0),
            normal: Vec3::NEG_X,
            surface: Surface::Solid {
                object: NonZeroUsize::new(123),
                body: 456,
            },
            material,
        }
    }

    fn scene(
        panes: &[(SurfaceHit, bool)],
        policy: GlassPolicy,
        options: &CastOptions,
    ) -> (Option<SurfaceHit>, Vec<Vec3>) {
        let mut origins = Vec::new();
        let result = continue_cast(
            Vec3::ZERO,
            Vec3::X * 4.0,
            options,
            |ray, distance, actual| {
                assert!(actual.material);
                assert_eq!(actual.ignore, options.ignore);
                assert_eq!(actual.see_through, options.see_through);
                assert!((ray.origin.x + distance - 4.0).abs() < 1e-5);
                origins.push(ray.origin);
                Ok(panes
                    .iter()
                    .find(|(hit, _)| {
                        hit.position.x >= ray.origin.x && hit.position.x < ray.origin.x + distance
                    })
                    .map(|&(hit, hidden)| (hit, policy.skips(hit.material, || hidden))))
            },
        )
        .unwrap();
        (result, origins)
    }

    #[test]
    fn hidden_near_pane_preserves_intact_far_pane_on_same_vehicle() {
        let near = hit(1.0, Some(GLASS));
        let far = hit(2.0, Some(GLASS));
        let options = CastOptions {
            ignore: [10, 20],
            ..CastOptions::default()
        };
        let (result, origins) = scene(&[(near, true), (far, false)], GlassPolicy::Hidden, &options);
        assert_eq!(result, Some(far));
        assert_eq!(origins.len(), 2);
        assert!((origins[1].x - (1.0 + FORWARD_STEP_M)).abs() < 1e-6);
        assert!(!options.material);
    }

    #[test]
    fn solid_behind_hidden_panes_on_same_vehicle_is_returned() {
        let solid = hit(3.0, None);
        let (result, origins) = scene(
            &[
                (hit(1.0, Some(GLASS)), true),
                (hit(2.0, Some(GLASS)), true),
                (solid, false),
            ],
            GlassPolicy::Hidden,
            &CastOptions::default(),
        );
        assert_eq!(result, Some(solid));
        assert_eq!(origins.len(), 3);
    }

    #[test]
    fn intact_glass_is_returned_with_material() {
        let pane = hit(1.0, Some(GLASS));
        let (result, origins) = scene(
            &[(pane, false), (hit(2.0, None), false)],
            GlassPolicy::Hidden,
            &CastOptions::default(),
        );
        assert_eq!(result, Some(pane));
        assert_eq!(origins.len(), 1);
    }

    #[test]
    fn solid_policy_skips_hidden_and_intact_glass_only() {
        let solid = hit(3.0, None);
        let (result, origins) = scene(
            &[
                (hit(1.0, Some(GLASS)), true),
                (hit(2.0, Some(GLASS)), false),
                (solid, true),
            ],
            GlassPolicy::All,
            &CastOptions::default(),
        );
        assert_eq!(result, Some(solid));
        assert_eq!(origins.len(), 3);
    }

    #[test]
    fn hidden_lookup_is_only_needed_for_glass_under_hidden_policy() {
        assert!(!GlassPolicy::Hidden.skips(None, || panic!("non-glass lookup")));
        assert!(!GlassPolicy::Hidden.skips(Some(GLASS), || false));
        assert!(GlassPolicy::Hidden.skips(Some(GLASS), || true));
        assert!(GlassPolicy::All.skips(Some(GLASS), || panic!("unneeded lookup")));
    }

    #[test]
    fn continuation_has_a_fixed_cast_budget() {
        let mut calls = 0;
        let result = continue_cast(Vec3::ZERO, Vec3::X, &CastOptions::default(), |ray, _, _| {
            calls += 1;
            Ok(Some((hit(ray.origin.x, Some(GLASS)), true)))
        });
        assert_eq!(result, Ok(None));
        assert_eq!(calls, MAX_CASTS);
    }

    #[test]
    fn hidden_pane_at_end_does_not_cast_past_target() {
        let mut calls = 0;
        let result = continue_cast(Vec3::ZERO, Vec3::X, &CastOptions::default(), |_, _, _| {
            calls += 1;
            Ok(Some((hit(1.0 - FORWARD_STEP_M / 2.0, Some(GLASS)), true)))
        });
        assert_eq!(result, Ok(None));
        assert_eq!(calls, 1);
    }

    #[test_case(Vec3::ZERO, Vec3::ZERO; "zero length")]
    #[test_case(Vec3::NAN, Vec3::X; "nan origin")]
    #[test_case(Vec3::ZERO, Vec3::NAN; "nan target")]
    #[test_case(Vec3::INFINITY, Vec3::X; "infinite origin")]
    #[test_case(Vec3::ZERO, Vec3::INFINITY; "infinite target")]
    #[test_case(Vec3::splat(-f32::MAX), Vec3::splat(f32::MAX); "overflow")]
    #[test_case(Vec3::ZERO, Vec3::splat(f32::MIN_POSITIVE); "underflow")]
    fn invalid_segments_do_not_call_the_engine(from: Vec3, to: Vec3) {
        assert_eq!(
            continue_cast(from, to, &CastOptions::default(), |_, _, _| {
                panic!("invalid segment reached the engine")
            }),
            Err(RaycastError::InvalidDistance),
        );
    }

    #[test_case(-1.0; "behind origin")]
    #[test_case(2.0; "past target")]
    #[test_case(f32::NAN; "nan hit")]
    fn invalid_continuations_stop(x: f32) {
        assert_eq!(
            continue_cast(Vec3::ZERO, Vec3::X, &CastOptions::default(), |_, _, _| {
                Ok(Some((hit(x, Some(GLASS)), true)))
            }),
            Err(RaycastError::InvalidDistance),
        );
    }

    #[test]
    fn miss_after_hidden_glass_is_clear() {
        let (result, origins) = scene(
            &[(hit(1.0, Some(GLASS)), true)],
            GlassPolicy::Hidden,
            &CastOptions::default(),
        );
        assert_eq!(result, None);
        assert_eq!(origins.len(), 2);
    }

    #[test]
    fn engine_errors_after_hidden_glass_propagate() {
        let mut calls = 0;
        let result = continue_cast(
            Vec3::ZERO,
            Vec3::X * 4.0,
            &CastOptions::default(),
            |_, _, _| {
                calls += 1;
                if calls == 1 {
                    Ok(Some((hit(1.0, Some(GLASS)), true)))
                } else {
                    Err(RaycastError::NoPhysicsSystem)
                }
            },
        );
        assert_eq!(result, Err(RaycastError::NoPhysicsSystem));
        assert_eq!(calls, 2);
    }
}
