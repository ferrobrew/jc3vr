use std::{num::NonZeroUsize, time::Instant};

use super::*;
use crate::controllers::body::rig::observation::{Epochs, StepStamp};

fn support(transform: Mat4) -> Support {
    let step = StepStamp {
        step: 1,
        epochs: Epochs {
            origin: 0,
            calibration: 1,
            world_scale: 1,
            authority: 1,
        },
    };
    Support {
        identity: 8,
        generation: 1,
        previous_step: step,
        current_step: step,
        previous: transform,
        current: transform,
        half_extent: glam::Vec2::splat(2.0),
    }
}

fn hit(body: u32, height: f32, object: bool) -> raycast::RayHit {
    raycast::RayHit {
        position: Vec3::new(0.1, height, 0.0),
        normal: Vec3::Y,
        distance: 0.3,
        object: object.then(|| NonZeroUsize::new(1).unwrap()),
        body,
        material: None,
    }
}

#[test]
fn actual_hit_height_is_used_instead_of_root_or_support_origin_height() {
    let surface = support(Mat4::from_translation(Vec3::new(0.0, 0.2, 0.0)));
    let local = terrain_foothold(
        surface,
        Vec3::new(0.1, 4.0, 0.0),
        0.3,
        0.03,
        &hit(7, 0.45, true),
        Some(7),
    )
    .unwrap();
    assert!((surface.world(local).position.y - 0.48).abs() < 1.0e-6);
}

#[test]
fn unrelated_or_missing_support_does_not_become_a_prop_foothold() {
    let surface = support(Mat4::IDENTITY);
    assert!(
        terrain_foothold(surface, Vec3::ZERO, 0.0, 0.03, &hit(9, 0.0, true), Some(7)).is_none()
    );
    assert!(terrain_foothold(surface, Vec3::ZERO, 0.0, 0.03, &hit(7, 0.0, true), None).is_none());
    assert!(terrain_foothold(surface, Vec3::ZERO, 0.0, 0.03, &hit(7, 0.0, false), None).is_some());
}

#[test]
fn current_transform_is_used_without_a_future_velocity_extrapolation() {
    let rotation = Quat::from_rotation_y(0.5);
    let current = Mat4::from_rotation_translation(rotation, Vec3::new(0.4, 0.1, -0.2));
    let surface = support(current);
    let point = Vec3::new(0.3, 2.0, -0.1);
    let local = terrain_foothold(surface, point, 0.7, 0.03, &hit(7, 0.15, true), Some(7)).unwrap();
    assert!(
        surface
            .world(local)
            .position
            .abs_diff_eq(Vec3::new(point.x, 0.18, point.z), 1.0e-6)
    );
    let sample = SupportSample {
        identity: 8,
        generation: 1,
        transform: current,
        at: Instant::now(),
    };
    assert_eq!(sample.transform, current);
}

#[test]
fn steep_or_invalid_normals_are_unavailable() {
    let surface = support(Mat4::IDENTITY);
    for normal in [Vec3::X, Vec3::ZERO, Vec3::splat(f32::NAN)] {
        let mut hit = hit(7, 0.0, false);
        hit.normal = normal;
        assert!(terrain_foothold(surface, Vec3::ZERO, 0.0, 0.03, &hit, None).is_none());
    }
}
