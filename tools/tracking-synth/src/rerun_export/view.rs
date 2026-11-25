//! The recorded view (from version 8): the cockpit's placement in the world as the mod composed it,
//! and the camera the frame rendered from, with its two tick poses and the fraction it blended them
//! by, so that a stutter or a clip shows in the scene and the plots.

use rerun::{
    Color, LineStrips3D, Points3D, RecordingStreamResult, Rgba32, Scalars, Transform3D,
    TransformAxes3D,
};
use tracking_format::{
    event::{CameraState, ViewState},
    model::Pose,
};

use crate::rerun_export::{
    Context,
    registration::{Anchor, Placement},
    scene::rotation,
};

/// The cockpit's placement a recorded view gives: a cockpit point `p` is the world point
/// `head_world + view * ((p - head_cockpit) * world_scale)`, so the translation is the world head
/// less the turned and scaled cockpit head. `None` for a view that does not place anything.
pub(crate) fn placement(view: &ViewState) -> Option<Placement> {
    let rotation = view.view.normalize();
    let scale = view.world_scale;
    let translation = view.head_world.position - rotation * (view.head_cockpit * scale);
    (translation.is_finite() && rotation.is_finite() && scale.is_finite() && scale > 0.0).then_some(
        Placement {
            translation,
            rotation,
            scale,
            anchor: Anchor::View,
        },
    )
}

/// Log the stretch's cameras.
pub(crate) fn log(context: &Context<'_>) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let mut shown = false;
    for &(time_ns, state) in &context.states.all {
        let Some(view) = &state.view else { continue };
        context.set_time(time_ns);
        let distance = view
            .camera
            .as_ref()
            .map(|camera| camera.rendered.position.distance(view.head_world.position));
        if let Some(camera) = &view.camera {
            if !shown {
                rec.log_static("world/camera", &TransformAxes3D::new(0.3))?;
                shown = true;
            }
            log_camera(context, camera)?;
        }
        if let Some(distance) = distance {
            rec.log(
                "plots/camera/head_distance_m",
                &Scalars::single(f64::from(distance)),
            )?;
        }
    }
    Ok(())
}

/// How far the rendered camera is from the blend of its tick poses by its fraction, metres: zero
/// when the render interpolated as it says it did.
pub(crate) fn blend_error_m(camera: &CameraState) -> f32 {
    let blended = camera
        .previous
        .position
        .lerp(camera.current.position, camera.fraction);
    camera.rendered.position.distance(blended)
}

const TICK: Color = Color(Rgba32::from_rgb(160, 160, 220));

fn log_camera(context: &Context<'_>, camera: &CameraState) -> RecordingStreamResult<()> {
    let rec = context.rec;
    rec.log("world/camera", &transform(context, &camera.rendered))?;
    let ticks = [
        context.world(camera.previous.position),
        context.world(camera.current.position),
    ];
    rec.log(
        "world/camera_ticks",
        &Points3D::new(ticks)
            .with_radii([0.015])
            .with_colors([TICK])
            .with_labels(["previous tick", "current tick"]),
    )?;
    rec.log(
        "world/camera_ticks/step",
        &LineStrips3D::new([ticks])
            .with_colors([TICK])
            .with_radii([0.004]),
    )?;
    let scalars = [
        ("fraction", camera.fraction),
        (
            "tick_step_m",
            camera.previous.position.distance(camera.current.position),
        ),
        ("blend_error_m", blend_error_m(camera)),
    ];
    for (name, value) in scalars {
        if value.is_finite() {
            rec.log(
                format!("plots/camera/{name}"),
                &Scalars::single(f64::from(value)),
            )?;
        }
    }
    Ok(())
}

fn transform(context: &Context<'_>, pose: &Pose) -> Transform3D {
    Transform3D::from_translation_rotation(context.world(pose.position), rotation(pose.orientation))
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

    fn pose(position: Vec3) -> Pose {
        Pose {
            position,
            orientation: Quat::IDENTITY,
        }
    }

    #[test]
    fn the_view_placement_maps_the_cockpit_head_onto_the_world_head() {
        let view = ViewState {
            head_world: Pose {
                position: Vec3::new(100.0, 20.0, -50.0),
                orientation: Quat::IDENTITY,
            },
            head_cockpit: Vec3::new(0.2, 1.7, -0.1),
            view: Quat::from_rotation_y(1.1),
            world_scale: 1.25,
            camera: None,
        };
        let placement = placement(&view).unwrap();
        assert_eq!(placement.anchor, Anchor::View);
        assert!(
            placement
                .apply(view.head_cockpit)
                .distance(view.head_world.position)
                < 1e-4
        );
        // A point a cockpit metre ahead of the head is a scaled and turned metre ahead in the world.
        let ahead = view.head_cockpit + Vec3::NEG_Z;
        let expected = view.head_world.position + view.view * (Vec3::NEG_Z * view.world_scale);
        assert!(placement.apply(ahead).distance(expected) < 1e-4);
    }

    #[test]
    fn a_view_without_a_scale_places_nothing() {
        let view = ViewState {
            head_world: pose(Vec3::ZERO),
            head_cockpit: Vec3::ZERO,
            view: Quat::IDENTITY,
            world_scale: 0.0,
            camera: None,
        };
        assert_eq!(placement(&view), None);
    }

    #[test]
    fn the_blend_error_is_the_distance_from_the_fractional_blend() {
        let camera = CameraState {
            previous: pose(Vec3::ZERO),
            current: pose(Vec3::new(1.0, 0.0, 0.0)),
            fraction: 0.25,
            rendered: pose(Vec3::new(0.25, 0.0, 0.0)),
        };
        assert!(blend_error_m(&camera) < 1e-6);
        let late = CameraState {
            rendered: pose(Vec3::new(1.0, 0.0, 0.0)),
            ..camera
        };
        assert!((blend_error_m(&late) - 0.75).abs() < 1e-6);
    }
}
