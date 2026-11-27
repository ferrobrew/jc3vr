//! The overlay's own producers: the grapple highlight, the sight tuning marker, and the grounded
//! body rig's diagnostics. Each builds its primitives from a published readout; the diagnostics are
//! behind their toggles in [`OverlayConfig`](crate::controllers::overlay::config::OverlayConfig).
//!
//! [`tick`] runs once per rendered frame on the render thread, before the first eye's draw. The
//! aim's readouts change at the sim tick's rate, so the grapple highlight submits only when the aim
//! has run since its last submission, and its slot expires on its own when it stops (a menu, the
//! gamepad scheme, an idle update). The sight tuning marker, which stays where it was placed, and
//! the body rig, which the grounded rig publishes once per rendered frame, submit every frame.
//!
//! The grip targets' markers are the grip model's to produce, into [`Source::GrabMarkers`]
//! ([`crate::controllers::grab::markers`]).

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use glam::Vec3;
use jc3vr_view::{
    controllers::overlay::{
        primitive::{
            Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize, with_alpha,
        },
        store,
    },
    headpose,
    vr::VR_CONFIG,
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        aim,
        aim::{
            GrappleAim, HandAim, HandSource, config::AIM_CONFIG, raycast::RayHit, sights::tuning,
        },
        body::{
            estimator,
            rig::{grounded, visual},
        },
        intent::tuning::CONTROLS,
        overlay::config::OVERLAY_CONFIG,
        scheme,
    },
    tuning::FRESHNESS,
};

/// Build and submit every producer's primitives for this frame. Every producer's slot is cleared
/// while `active` is false.
pub fn tick(active: bool) {
    let config = *OVERLAY_CONFIG.lock();
    let dominant = scheme::acting_hand();
    let sight_tuning = AIM_CONFIG.query(|c| c.sight_tuning.enabled);
    let world_scale = VR_CONFIG.lock().world_scale;
    if !active {
        for source in PRODUCED {
            store::clear(source);
        }
        return;
    }
    let mut prims = Vec::new();

    let status = aim::status();
    if LAST_AIM_UPDATE.swap(status.updates, Ordering::Relaxed) != status.updates {
        let off_hit = status.hands[dominant.other().index()].and_then(|h| h.hit);
        let grapple_at = match status.grapple.as_ref() {
            Some(g) => Some(ticked_grapple_point(g.point)),
            None => {
                // A new aim starts where it is rather than sweeping from the last one's point.
                *GRAPPLE_TICKS.lock() = (None, None);
                None
            }
        };
        submit_or_clear(
            Source::GrappleHighlight,
            true,
            FRESHNESS.get().overlay.sim_ms.duration(),
            |out| {
                if let (Some(g), Some(at)) = (&status.grapple, grapple_at) {
                    grapple_highlight(g, at, off_hit, out);
                }
            },
            &mut prims,
        );
    }

    submit_or_clear(
        Source::SightTuning,
        sight_tuning,
        FRESHNESS.get().overlay.frame_ms.duration(),
        |out| {
            if let Some(marker) = tuning::status().marker {
                sight_marker(marker, status.hands[dominant.index()], out);
            }
        },
        &mut prims,
    );
    submit_or_clear(
        Source::BodyRig,
        config.body_rig || config.body_estimate,
        FRESHNESS.get().overlay.frame_ms.duration(),
        |out| {
            let view = headpose::body_rotation();
            if config.body_rig
                && let Some(d) = grounded::diagnostics()
            {
                visual::primitives(&d, view, world_scale, out);
            }
            if config.body_estimate {
                estimator::visual::diagnostics(view, world_scale, out);
            }
        },
        &mut prims,
    );
}

/// The grapple's target highlight: a ring on the target while the game holds a valid target in
/// range, lying on the surface when the off-hand's own ray hit it there, with a centre dot and its
/// own colour for a target a tether pulls, and a wider, thin outer ring while the aim assist turned
/// the ray onto it.
/// The grapple's aim point as an anchor between its last two sim ticks' points
/// ([`Anchor::Ticked`]), called once for each aim update: the point is measured once a tick, and
/// from a moving vehicle a point drawn where the tick left it slid back by up to a tick's travel
/// and jumped on each tick. A point that holds still from one tick to the next is drawn still.
fn ticked_grapple_point(point: Vec3) -> Anchor {
    let mut ticks = GRAPPLE_TICKS.lock();
    let (_, current) = *ticks;
    let (previous, current) = match current {
        Some(current) if current.distance(point) < CONTROLS.get().overlay.ticked_jump_m => {
            (current, point)
        }
        _ => (point, point),
    };
    *ticks = (Some(previous), Some(current));
    Anchor::Ticked { previous, current }
}

/// The grapple's aim point on the tick before last and the last tick.
static GRAPPLE_TICKS: Mutex<(Option<Vec3>, Option<Vec3>)> = Mutex::new((None, None));

pub(crate) fn grapple_highlight(
    g: &GrappleAim,
    at: Anchor,
    off_hit: Option<RayHit>,
    out: &mut Vec<Primitive>,
) {
    if !g.valid {
        return;
    }
    let overlay = &CONTROLS.get().overlay;
    let facing = off_hit
        .filter(|hit| hit.position.distance(g.point) < overlay.surface_match_m)
        .map_or(Facing::Billboard, |hit| Facing::Surface {
            normal: hit.normal,
        });
    let color = if g.tetherable { TETHER } else { GRAPPLE };
    // A tetherable target's point can be its body position, inside the object, so the highlight
    // shows faintly through the surface in front of it rather than vanishing.
    let depth = DepthMode::XRay {
        occluded_alpha: 0.35,
    };
    out.push(Primitive::Sprite {
        at,
        size: SpriteSize::Angular(overlay.grapple_ring_rad),
        facing,
        shape: Shape::Ring { thickness: 0.3 },
        color,
        depth,
    });
    if g.assist.is_some() {
        out.push(Primitive::Sprite {
            at,
            size: SpriteSize::Angular(overlay.grapple_ring_rad * overlay.assist_ring_scale),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.12 },
            color: with_alpha(color, 0.6),
            depth,
        });
    }
    if g.tetherable {
        out.push(Primitive::Sprite {
            at,
            size: SpriteSize::Angular(overlay.grapple_ring_rad * 0.3),
            facing,
            shape: Shape::Disc,
            color,
            depth,
        });
    }
}

/// The sight tuning marker: a ring and a centre dot at the marker, over the scene, and a dot where
/// the dominant hand's sight line passes closest to it, while that hand aims a gun.
pub(crate) fn sight_marker(marker: Vec3, dominant: Option<HandAim>, out: &mut Vec<Primitive>) {
    let overlay = &CONTROLS.get().overlay;
    for (shape, size) in [
        (Shape::Ring { thickness: 0.25 }, overlay.marker_ring_rad),
        (Shape::Disc, overlay.marker_dot_rad),
    ] {
        out.push(Primitive::Sprite {
            at: Anchor::World(marker),
            size: SpriteSize::Angular(size),
            facing: Facing::Billboard,
            shape,
            color: MARKER,
            depth: DepthMode::Always,
        });
    }
    let Some(aim) = dominant.filter(|a| matches!(a.source, HandSource::Gun { .. })) else {
        return;
    };
    let along = (marker - aim.ray.origin).dot(aim.ray.direction);
    if along <= 0.0 {
        return;
    }
    out.push(Primitive::Sprite {
        at: Anchor::World(aim.ray.at(along)),
        size: SpriteSize::Angular(overlay.marker_dot_rad),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color: LASER,
        depth: DepthMode::Always,
    });
}

/// The sources [`tick`] produces.
const PRODUCED: [Source; 3] = [
    Source::GrappleHighlight,
    Source::SightTuning,
    Source::BodyRig,
];

/// The aim's update count at the last submission of the sim-rate producers.
static LAST_AIM_UPDATE: AtomicU64 = AtomicU64::new(u64::MAX);

const GRAPPLE: Rgba = [0.3, 0.9, 1.0, 0.9];
const TETHER: Rgba = [1.0, 0.65, 0.15, 0.95];
const LASER: Rgba = [1.0, 0.15, 0.1, 0.95];
const MARKER: Rgba = [1.0, 0.9, 0.2, 0.95];

/// Build `source`'s primitives into `buffer` and submit them while `enabled`, or clear the slot.
fn submit_or_clear(
    source: Source,
    enabled: bool,
    ttl: Duration,
    build: impl FnOnce(&mut Vec<Primitive>),
    buffer: &mut Vec<Primitive>,
) {
    if !enabled {
        store::clear(source);
        return;
    }
    buffer.clear();
    build(buffer);
    store::submit(source, buffer, ttl);
}

#[cfg(test)]
mod tests {
    fn highlight(g: &GrappleAim, off_hit: Option<RayHit>, out: &mut Vec<Primitive>) {
        grapple_highlight(g, Anchor::World(g.point), off_hit, out);
    }

    use super::*;
    use crate::controllers::aim::{assist::GrappleAssist, ray::Ray};

    fn grapple(valid: bool, tetherable: bool) -> GrappleAim {
        GrappleAim {
            ray: Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap(),
            point: Vec3::new(0.0, 0.0, -30.0),
            hit: true,
            valid,
            tetherable,
            current: true,
            seat: false,
            assist: None,
        }
    }

    fn hit(position: Vec3) -> RayHit {
        RayHit {
            position,
            normal: Vec3::Z,
            distance: position.length(),
            object: None,
            body: 0,
            material: None,
        }
    }

    #[test]
    fn the_highlight_shows_only_for_a_valid_target() {
        let mut out = Vec::new();
        highlight(&grapple(false, false), None, &mut out);
        assert!(out.is_empty());
        highlight(&grapple(true, false), None, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_tetherable_target_has_its_own_style() {
        let mut plain = Vec::new();
        highlight(&grapple(true, false), None, &mut plain);
        let mut tether = Vec::new();
        highlight(&grapple(true, true), None, &mut tether);
        assert_eq!(tether.len(), 2);
        let color = |p: &Primitive| match p {
            Primitive::Sprite { color, .. } => *color,
            Primitive::Line { color, .. } => *color,
        };
        assert_ne!(color(&plain[0]), color(&tether[0]));
    }

    #[test]
    fn an_assisted_target_shows_the_lock_ring() {
        let mut plain = Vec::new();
        highlight(&grapple(true, false), None, &mut plain);
        let assisted = GrappleAim {
            assist: Some(GrappleAssist {
                target: Vec3::new(0.0, 0.0, -30.0),
                angle_rad: 0.05,
            }),
            ..grapple(true, false)
        };
        let mut locked = Vec::new();
        highlight(&assisted, None, &mut locked);
        assert_eq!(locked.len(), plain.len() + 1);
        // No lock ring without a valid target.
        let mut none = Vec::new();
        highlight(
            &GrappleAim {
                valid: false,
                ..assisted
            },
            None,
            &mut none,
        );
        assert!(none.is_empty());
    }

    #[test]
    fn the_highlight_lies_on_the_surface_the_hand_hit_there() {
        let mut out = Vec::new();
        let g = grapple(true, false);
        highlight(&g, Some(hit(g.point + Vec3::X * 0.1)), &mut out);
        assert!(matches!(
            out[0],
            Primitive::Sprite {
                facing: Facing::Surface { .. },
                ..
            }
        ));
        out.clear();
        highlight(&g, Some(hit(Vec3::new(0.0, 0.0, -5.0))), &mut out);
        assert!(matches!(
            out[0],
            Primitive::Sprite {
                facing: Facing::Billboard,
                ..
            }
        ));
    }

    #[test]
    fn the_sight_marker_shows_where_the_gun_passes_it() {
        let marker = Vec3::new(0.0, 0.0, -10.0);
        let mut out = Vec::new();
        sight_marker(marker, None, &mut out);
        assert_eq!(out.len(), 2);

        let aim = HandAim {
            source: HandSource::Gun { gun: 0 },
            ray: Ray::new(Vec3::new(0.0, 0.1, 0.0), Vec3::NEG_Z).unwrap(),
            hit: None,
            target: Vec3::new(0.0, 0.1, -500.0),
        };
        out.clear();
        sight_marker(marker, Some(aim), &mut out);
        assert_eq!(out.len(), 3);
        let Primitive::Sprite {
            at: Anchor::World(dot),
            ..
        } = out[2]
        else {
            panic!("no gun dot");
        };
        assert!(dot.distance(Vec3::new(0.0, 0.1, -10.0)) < 1e-5);
    }
}
