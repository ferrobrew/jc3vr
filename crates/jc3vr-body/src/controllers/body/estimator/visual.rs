//! The estimate's in-world diagnostic: arrows on the floor under the player's eyes for the
//! estimate's pelvis and chest headings, and, while the grounded rig runs, its legs' and chest's
//! beside them, so the two can be compared in the headset (`docs/mod/body/body-estimator.md`).
//!
//! Like the rig's own diagnostics ([`crate::controllers::body::rig::visual`]), each point is
//! anchored to the head by its offset from the eyes in the rig's room frame, carried into the world
//! by the view's rotation and the world scale. It is drawn in every mode the estimator runs in.

use body_rig::math::angle::{direction, wrap};
use glam::{Quat, Vec3};
use jc3vr_core::published::Stamp;
use jc3vr_view::controllers::overlay::primitive::{Anchor, DepthMode, Primitive, Rgba};

use crate::controllers::body::{
    estimator::{latest, producer},
    rig::grounded,
};

/// Build the arrows for the latest estimate and the grounded rig, if any, with `view` the cockpit
/// frame's rotation into the world and `scale` the world scale. Nothing is drawn without an
/// estimate or a recent frame of the window to stand it under.
pub(crate) fn diagnostics(view: Quat, scale: f32, out: &mut Vec<Primitive>) {
    let Some(estimate) = latest() else {
        return;
    };
    let Some(eyes) = producer::eyes(Stamp::now()) else {
        return;
    };
    let rig = grounded::body_yaw().zip(grounded::torso().map(|torso| torso.yaw));
    primitives(
        &Headings {
            estimate_pelvis: estimate.pelvis_yaw,
            estimate_chest: estimate.chest_yaw,
            rig,
            stale: estimate.stale(),
        },
        eyes,
        view,
        scale,
        out,
    );
}

/// The headings to draw, cockpit-frame yaws.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Headings {
    pub(crate) estimate_pelvis: f32,
    pub(crate) estimate_chest: f32,
    /// The grounded rig's legs and chest, while it runs.
    pub rig: Option<(f32, f32)>,
    /// Whether the estimate is stale, which dims its arrows.
    pub stale: bool,
}

/// Build the arrows for `headings` under the eyes at `eyes` (room frame), with `view` the cockpit
/// frame's rotation into the world and `scale` the world scale.
pub(crate) fn primitives(
    headings: &Headings,
    eyes: Vec3,
    view: Quat,
    scale: f32,
    out: &mut Vec<Primitive>,
) {
    let floor = Vec3::new(eyes.x, 0.0, eyes.z);
    let alpha = if headings.stale { STALE_ALPHA } else { 1.0 };
    let mut arrow = |yaw: f32, length: f32, lift: f32, color: Rgba, width: f32| {
        let base = floor + Vec3::Y * lift;
        let [r, g, b, a] = color;
        arrow_primitives(
            Arrow {
                base,
                yaw,
                length,
                width,
            },
            eyes,
            view,
            scale,
            [r, g, b, a * alpha],
            out,
        );
    };
    arrow(
        headings.estimate_pelvis,
        PELVIS_M,
        LIFT_M,
        ESTIMATE_PELVIS,
        WIDTH_M,
    );
    arrow(
        headings.estimate_chest,
        CHEST_M,
        LIFT_M * 2.0,
        ESTIMATE_CHEST,
        WIDTH_M * 0.6,
    );
    if let Some((legs, chest)) = headings.rig {
        let mut rig = |yaw: f32, length: f32, lift: f32, color: Rgba, width: f32| {
            arrow_primitives(
                Arrow {
                    base: floor + Vec3::Y * lift,
                    yaw,
                    length,
                    width,
                },
                eyes,
                view,
                scale,
                color,
                out,
            );
        };
        rig(legs, PELVIS_M, LIFT_M * 3.0, RIG_LEGS, WIDTH_M);
        rig(chest, CHEST_M, LIFT_M * 4.0, RIG_CHEST, WIDTH_M * 0.6);
    }
}

/// One arrow on the floor.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Arrow {
    base: Vec3,
    yaw: f32,
    length: f32,
    width: f32,
}

/// The shaft and the two barbs of `arrow`, each a line anchored to the head.
fn arrow_primitives(
    arrow: Arrow,
    eyes: Vec3,
    view: Quat,
    scale: f32,
    color: Rgba,
    out: &mut Vec<Primitive>,
) {
    if !arrow.yaw.is_finite() {
        return;
    }
    let at = |p: Vec3| Anchor::Head(view * (p - eyes) * scale);
    let tip = arrow.base + direction(arrow.yaw) * arrow.length;
    let line = |from: Vec3, to: Vec3| Primitive::Line {
        from: at(from),
        to: at(to),
        width: arrow.width * scale,
        color,
        depth: DEPTH,
    };
    out.push(line(arrow.base, tip));
    for side in [-1.0, 1.0] {
        let back = direction(wrap(arrow.yaw + side * BARB_RAD + std::f32::consts::PI));
        out.push(line(tip, tip + back * arrow.length * BARB_SHARE));
    }
}

const ESTIMATE_PELVIS: Rgba = [0.25, 0.85, 0.35, 0.95];
const ESTIMATE_CHEST: Rgba = [0.65, 0.95, 0.65, 0.85];
const RIG_LEGS: Rgba = [0.9, 0.3, 0.9, 0.9];
const RIG_CHEST: Rgba = [0.97, 0.65, 0.97, 0.8];
/// A stale estimate's arrows' alpha relative to a fresh one's.
const STALE_ALPHA: f32 = 0.3;
const PELVIS_M: f32 = 0.6;
const CHEST_M: f32 = 0.45;
const WIDTH_M: f32 = 0.012;
/// How far above the floor each arrow is lifted, metres, so overlapping arrows stay apart.
const LIFT_M: f32 = 0.005;
/// The barbs' angle from the shaft, and their length as a share of the arrow's.
const BARB_RAD: f32 = 0.45;
const BARB_SHARE: f32 = 0.2;
const DEPTH: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.35,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn head_offset(anchor: Anchor) -> Vec3 {
        match anchor {
            Anchor::Head(offset) => offset,
            other => panic!("not a head anchor: {other:?}"),
        }
    }

    #[test]
    fn the_estimate_arrows_lie_on_the_floor_and_point_along_the_heading() {
        let headings = Headings {
            estimate_pelvis: std::f32::consts::FRAC_PI_2,
            estimate_chest: 0.0,
            rig: None,
            stale: false,
        };
        let eyes = Vec3::new(0.3, 1.6, -0.2);
        let mut out = Vec::new();
        primitives(&headings, eyes, Quat::IDENTITY, 1.0, &mut out);
        assert_eq!(out.len(), 6);
        let Primitive::Line {
            from, to, color, ..
        } = out[0]
        else {
            panic!("the shaft is a line");
        };
        let (from, to) = (head_offset(from), head_offset(to));
        // Under the eyes, on the floor.
        assert!((from - Vec3::new(0.0, -1.6 + LIFT_M, 0.0)).length() < 1e-5);
        // A quarter turn left of -Z faces -X.
        let along = (to - from).normalize();
        assert!((along - Vec3::NEG_X).length() < 1e-5, "{along}");
        assert_eq!(color, ESTIMATE_PELVIS);
    }

    #[test]
    fn the_rig_arrows_join_while_it_runs_and_a_stale_estimate_dims() {
        let headings = Headings {
            estimate_pelvis: 0.0,
            estimate_chest: 0.0,
            rig: Some((0.1, 0.2)),
            stale: true,
        };
        let mut out = Vec::new();
        primitives(&headings, Vec3::Y, Quat::IDENTITY, 1.0, &mut out);
        assert_eq!(out.len(), 12);
        let Primitive::Line { color, .. } = out[0] else {
            panic!("the shaft is a line");
        };
        assert_eq!(color[3], ESTIMATE_PELVIS[3] * STALE_ALPHA);
        let Primitive::Line { color, .. } = out[6] else {
            panic!("the rig's shaft is a line");
        };
        assert_eq!(color, RIG_LEGS);
    }
}
