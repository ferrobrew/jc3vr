//! The rendered panels as the controller pointers see them: the center camera captured with its
//! tracking sample, the game-UI panel's and the egui debug panel's corners published with that
//! basis, and the ray intersection against them.

use std::time::Instant;

use glam::{Mat4, Vec3, Vec4};
use parking_lot::Mutex;

use crate::{
    controllers::{
        frame::{MAX_FRAME_AGE, Pose},
        hands::pose::{body_of_head, compose_world},
    },
    headpose::xr::CockpitPose,
};

/// A ray in world space, used by the pure panel intersection helper.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PanelRay {
    pub(crate) origin: Vec3,
    pub(crate) direction: Vec3,
}

/// A ray's intersection with the panel plane. `uv` is clamped for an active drag, while `inside`
/// records whether the original intersection was inside the displayed panel. `distance` is how far
/// along the ray the plane lies, in the ray's world units.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PanelHit {
    pub(crate) uv: (f32, f32),
    pub(crate) inside: bool,
    pub(crate) distance: f32,
}

/// The center camera and tracking sample captured together before per-eye offsets.
#[derive(Clone, Copy)]
pub struct RenderedCamera {
    pub head: Pose,
    pub cockpit_head: Pose,
    pub scale: f32,
}

/// Record the un-offset center camera's world transform with the cockpit head pose and world scale
/// that placed it, for the panel and the pointer to share one basis.
pub fn publish_camera(world: Mat4, cockpit: Option<CockpitPose>, scale: f32) {
    let (_, orientation, position) = world.to_scale_rotation_translation();
    *RENDERED_CAMERA.lock() = cockpit.map(|cockpit| {
        (
            RenderedCamera {
                head: Pose {
                    position,
                    orientation,
                },
                cockpit_head: Pose {
                    position: cockpit.position,
                    orientation: cockpit.orientation,
                },
                scale,
            },
            Instant::now(),
        )
    });
}

/// The latest captured camera, unless it is older than [`MAX_FRAME_AGE`].
pub fn rendered_camera() -> Option<RenderedCamera> {
    RENDERED_CAMERA
        .lock()
        .filter(|(_, time)| time.elapsed() <= MAX_FRAME_AGE)
        .map(|(camera, _)| camera)
}

/// The rendered panel and the tracking-to-world basis that placed its center camera.
#[derive(Clone, Copy)]
pub struct RenderedPanel {
    pub head: Pose,
    pub cockpit_head: Pose,
    pub scale: f32,
    pub corners: [Vec4; 4],
}

/// Publish the panel drawn this frame, or `None` when no panel with a pointer basis was drawn, so
/// the pointer never intersects corners the player no longer sees.
pub fn publish_panel(panel: Option<RenderedPanel>) {
    *RENDERED_PANEL.lock() = panel.map(|panel| (panel, Instant::now()));
}

/// Publish the egui debug panel drawn this frame, or `None` when it was not drawn with a pointer
/// basis, as [`publish_panel`] does for the game-UI panel.
pub fn publish_egui_panel(panel: Option<RenderedPanel>) {
    *RENDERED_EGUI_PANEL.lock() = panel.map(|panel| (panel, Instant::now()));
}

/// The world-space aim ray of a cockpit-frame `aim` pose against the latest fresh panel, with the
/// panel's corners.
pub(crate) fn aim_at_panel(aim: Pose) -> Option<(PanelRay, [Vec4; 4])> {
    aim_at(&RENDERED_PANEL, aim)
}

/// The world-space aim ray of a cockpit-frame `aim` pose against the latest fresh egui debug
/// panel, with the panel's corners.
pub(crate) fn aim_at_egui_panel(aim: Pose) -> Option<(PanelRay, [Vec4; 4])> {
    aim_at(&RENDERED_EGUI_PANEL, aim)
}

/// Intersect `ray` with the flat panel represented by top-left, top-right, bottom-left, and
/// bottom-right corners. The helper intentionally returns out-of-bounds UVs as a clamped hit: the
/// pointer state uses those values to keep a captured drag at the panel edge. A new hover only owns
/// the UI when [`PanelHit::inside`] is true.
pub(crate) fn intersect_panel(ray: PanelRay, corners: &[Vec4; 4]) -> Option<PanelHit> {
    if !ray.origin.is_finite() || !ray.direction.is_finite() {
        return None;
    }
    let top_left = corners[0].truncate();
    let top_right = corners[1].truncate();
    let bottom_left = corners[2].truncate();
    if !top_left.is_finite() || !top_right.is_finite() || !bottom_left.is_finite() {
        return None;
    }
    let across = top_right - top_left;
    let down = bottom_left - top_left;
    let normal = across.cross(down);
    let normal_length_squared = normal.length_squared();
    if !normal_length_squared.is_finite() || normal_length_squared <= f32::EPSILON {
        return None;
    }
    let denominator = normal.dot(ray.direction);
    if !denominator.is_finite() || denominator.abs() <= 1e-6 {
        return None;
    }
    let distance = normal.dot(top_left - ray.origin) / denominator;
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }
    let point = ray.origin + ray.direction * distance;
    let across_length_squared = across.length_squared();
    let down_length_squared = down.length_squared();
    if across_length_squared <= f32::EPSILON || down_length_squared <= f32::EPSILON {
        return None;
    }
    let relative = point - top_left;
    let u = relative.dot(across) / across_length_squared;
    let v = relative.dot(down) / down_length_squared;
    if !u.is_finite() || !v.is_finite() {
        return None;
    }
    let inside = (-1e-5..=1.00001).contains(&u) && (-1e-5..=1.00001).contains(&v);
    Some(PanelHit {
        uv: (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)),
        inside,
        distance,
    })
}

type PanelSlot = Mutex<Option<(RenderedPanel, Instant)>>;

static RENDERED_CAMERA: Mutex<Option<(RenderedCamera, Instant)>> = Mutex::new(None);

static RENDERED_PANEL: PanelSlot = Mutex::new(None);

static RENDERED_EGUI_PANEL: PanelSlot = Mutex::new(None);

fn aim_at(slot: &PanelSlot, aim: Pose) -> Option<(PanelRay, [Vec4; 4])> {
    let (panel, published) = (*slot.lock())?;
    (published.elapsed() <= MAX_FRAME_AGE).then(|| (panel.ray(aim), panel.corners))
}

impl RenderedPanel {
    /// The world-space ray of a cockpit-frame aim pose, composed through the same basis that
    /// placed the panel: the rendered head, with the body rotation recovered from it.
    fn ray(self, aim: Pose) -> PanelRay {
        let world = compose_world(
            self.head.position,
            body_of_head(self.head.orientation, self.cockpit_head.orientation),
            self.cockpit_head.position,
            aim,
            self.scale,
        );
        PanelRay {
            origin: world.position,
            direction: world.orientation * Vec3::NEG_Z,
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn corners() -> [Vec4; 4] {
        [
            Vec4::new(-1.0, 1.0, 0.0, 1.0),
            Vec4::new(1.0, 1.0, 0.0, 1.0),
            Vec4::new(-1.0, -1.0, 0.0, 1.0),
            Vec4::new(1.0, -1.0, 0.0, 1.0),
        ]
    }

    #[test]
    fn rendered_basis_handles_loading_base_scale_and_head_motion() {
        let base = Quat::from_rotation_y(1.2);
        let translation = Vec3::new(80.0, -15.0, 30.0);
        let scale = 2.4;
        let panel_corners =
            corners().map(|p| (translation + base * (p.truncate() * scale)).extend(1.0));
        let aim = Pose {
            position: Vec3::new(0.0, 0.0, 2.0),
            orientation: Quat::IDENTITY,
        };
        for position in [Vec3::ZERO, Vec3::new(0.3, 0.2, -0.4)] {
            let cockpit_head = Pose {
                position,
                orientation: Quat::from_rotation_x(0.4),
            };
            let panel = RenderedPanel {
                head: Pose {
                    position: translation + base * (position * scale),
                    orientation: base * cockpit_head.orientation,
                },
                cockpit_head,
                scale,
                corners: panel_corners,
            };
            let hit = intersect_panel(panel.ray(aim), &panel.corners).unwrap();
            assert!(hit.inside);
            assert!((hit.uv.0 - 0.5).abs() < 1e-5);
            assert!((hit.uv.1 - 0.5).abs() < 1e-5);
        }
    }

    #[test]
    fn ray_maps_panel_corners_to_texture_uv() {
        let panel = corners();
        let hit = intersect_panel(
            PanelRay {
                origin: Vec3::new(0.0, 0.0, 2.0),
                direction: Vec3::NEG_Z,
            },
            &panel,
        )
        .expect("the ray should hit");
        assert_eq!(hit.uv, (0.5, 0.5));
        assert!(hit.inside);

        let hit = intersect_panel(
            PanelRay {
                origin: Vec3::new(-1.0, 1.0, 2.0),
                direction: Vec3::NEG_Z,
            },
            &panel,
        )
        .expect("the ray should hit");
        assert_eq!(hit.uv, (0.0, 0.0));
    }

    #[test]
    fn parallel_and_behind_rays_do_not_hit() {
        let panel = corners();
        assert!(
            intersect_panel(
                PanelRay {
                    origin: Vec3::ZERO,
                    direction: Vec3::X,
                },
                &panel,
            )
            .is_none()
        );
        assert!(
            intersect_panel(
                PanelRay {
                    origin: Vec3::new(0.0, 0.0, -1.0),
                    direction: Vec3::NEG_Z,
                },
                &panel,
            )
            .is_none()
        );
    }

    #[test]
    fn out_of_bounds_intersection_is_clamped_but_not_a_new_hover() {
        let panel = corners();
        let hit = intersect_panel(
            PanelRay {
                origin: Vec3::new(2.0, 0.0, 2.0),
                direction: Vec3::NEG_Z,
            },
            &panel,
        )
        .expect("the ray still meets the panel plane");
        assert_eq!(hit.uv, (1.0, 0.5));
        assert!(!hit.inside);
    }
}
