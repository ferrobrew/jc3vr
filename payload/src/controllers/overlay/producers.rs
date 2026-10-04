//! The overlay's own producers: the grapple highlight and gauntlet beam, the guns' dot or laser,
//! the ghosts of yielded hands, the controller gizmos, and the sight tuning marker. Each builds its primitives from a
//! published readout, behind its toggle in [`OverlayConfig`].
//!
//! [`tick`] runs once per rendered frame on the render thread, before the first eye's draw. The
//! aim's readouts change at the sim tick's rate, so the grapple and gun producers submit only when
//! the aim has run since their last submission, and their slots expire on their own when it stops
//! (a menu, the gamepad scheme, an idle update). Their hand-attached ends are anchored to the hand,
//! so they still follow the controller at frame rate. The ghost hands and the gizmos follow the
//! render-time hand poses and submit every frame, as does the sight tuning marker, which stays
//! where it was placed.
//!
//! The grab-volume highlights are the holsters' to produce, into [`Source::GrabVolumes`].

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use glam::Vec3;

use crate::{
    config::Config,
    controllers::{
        aim::{self, GrappleAim, HandAim, HandSource, raycast::RayHit, sights::tuning},
        config::{GunReadout, OverlayConfig},
        frame::Hand,
        hands::tiers,
        overlay::{
            primitive::{
                Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, Source, SpriteSize,
            },
            resolve::HandPoses,
            store,
        },
    },
};

/// Build and submit every producer's primitives for this frame, given the render-time hand poses.
/// Every producer's slot is cleared while `active` is false.
pub fn tick(active: bool, hands: &[Option<HandPoses>; 2]) {
    let (config, dominant, grapple_origin, sight_tuning) = Config::lock_query(|c| {
        (
            c.controllers.overlay,
            c.controllers.dominant_hand,
            c.controllers.aim.grapple_origin_m,
            c.controllers.aim.sight_tuning.enabled,
        )
    });
    if !active {
        for source in PRODUCED {
            store::clear(source);
        }
        return;
    }
    let mut prims = Vec::new();

    let status = aim::status();
    if LAST_AIM_UPDATE.swap(status.updates, Ordering::Relaxed) != status.updates {
        let off = dominant.other();
        let off_hit = status.hands[off.index()].and_then(|h| h.hit);
        submit_or_clear(
            Source::GrappleHighlight,
            config.grapple_highlight,
            SIM_TTL,
            |out| {
                if let Some(g) = &status.grapple {
                    grapple_highlight(g, off_hit, out);
                }
            },
            &mut prims,
        );
        submit_or_clear(
            Source::GrappleBeam,
            config.grapple_beam,
            SIM_TTL,
            |out| {
                if let Some(g) = &status.grapple {
                    grapple_beam(g, off, grapple_origin, out);
                }
            },
            &mut prims,
        );
        let readout = config.gun_readout;
        submit_or_clear(
            Source::GunReadout,
            readout != GunReadout::None,
            SIM_TTL,
            |out| {
                gun_readout(readout, &status.hands, hands, out);
            },
            &mut prims,
        );
    }

    submit_or_clear(
        Source::GhostHands,
        config.ghost_hands,
        FRAME_TTL,
        |out| {
            ghost_hands(tiers::weights(), hands, &config, out);
        },
        &mut prims,
    );
    submit_or_clear(
        Source::DebugGizmos,
        config.debug_gizmos,
        FRAME_TTL,
        |out| {
            gizmos(hands, out);
        },
        &mut prims,
    );
    submit_or_clear(
        Source::SightTuning,
        sight_tuning,
        FRAME_TTL,
        |out| {
            if let Some(marker) = tuning::status().marker {
                sight_marker(marker, status.hands[dominant.index()], out);
            }
        },
        &mut prims,
    );
}

/// The grapple's target highlight: a ring on the target while the game holds a valid target in
/// range, lying on the surface when the off-hand's own ray hit it there, with a centre dot and its
/// own colour for a target a tether pulls.
pub fn grapple_highlight(g: &GrappleAim, off_hit: Option<RayHit>, out: &mut Vec<Primitive>) {
    if !g.valid {
        return;
    }
    let facing = off_hit
        .filter(|hit| hit.position.distance(g.point) < SURFACE_MATCH_M)
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
        at: Anchor::World(g.point),
        size: SpriteSize::Angular(GRAPPLE_RING_RAD),
        facing,
        shape: Shape::Ring { thickness: 0.3 },
        color,
        depth,
    });
    if g.tetherable {
        out.push(Primitive::Sprite {
            at: Anchor::World(g.point),
            size: SpriteSize::Angular(GRAPPLE_RING_RAD * 0.3),
            facing,
            shape: Shape::Disc,
            color,
            depth,
        });
    }
}

/// The gauntlet beam from the grapple's origin on the off-hand's wrist to the grapple's point: in
/// the highlight's colour while the target is valid, and faint otherwise.
pub fn grapple_beam(g: &GrappleAim, off: Hand, origin_offset: Vec3, out: &mut Vec<Primitive>) {
    let color = match (g.valid, g.tetherable) {
        (true, true) => with_alpha(TETHER, 0.7),
        (true, false) => with_alpha(GRAPPLE, 0.7),
        (false, _) => [1.0, 1.0, 1.0, 0.2],
    };
    out.push(Primitive::Line {
        from: Anchor::Hand {
            hand: off,
            pose: HandPose::Wrist,
            offset: origin_offset,
        },
        to: Anchor::World(g.point),
        width: BEAM_WIDTH_M,
        color,
        depth: DepthMode::XRay {
            occluded_alpha: 0.25,
        },
    });
}

/// The guns' readout for each hand aiming a gun: a dot at the hit point, or a laser from the sight
/// line's origin to the hit point (to the aim point when nothing is hit). The laser's origin is
/// anchored to the hand's render-time wrist at the offset it has now, so it stays on the gun
/// between sim ticks.
pub fn gun_readout(
    readout: GunReadout,
    aims: &[Option<HandAim>; 2],
    hands: &[Option<HandPoses>; 2],
    out: &mut Vec<Primitive>,
) {
    for hand in Hand::BOTH {
        let Some(aim) = aims[hand.index()] else {
            continue;
        };
        if !matches!(aim.source, HandSource::Gun { .. }) {
            continue;
        }
        match readout {
            GunReadout::None => {}
            GunReadout::Dot => {
                if let Some(hit) = aim.hit {
                    out.push(Primitive::Sprite {
                        at: Anchor::World(hit.position),
                        size: SpriteSize::Angular(GUN_DOT_RAD),
                        facing: Facing::Billboard,
                        shape: Shape::Disc,
                        color: LASER,
                        depth: DepthMode::Tested,
                    });
                }
            }
            GunReadout::Laser => {
                let from = hands[hand.index()].map_or(Anchor::World(aim.ray.origin), |poses| {
                    let wrist = poses.wrist;
                    Anchor::Hand {
                        hand,
                        pose: HandPose::Wrist,
                        offset: wrist.orientation.inverse() * (aim.ray.origin - wrist.position),
                    }
                });
                let end = aim.hit.map_or(aim.target, |hit| hit.position);
                out.push(Primitive::Line {
                    from,
                    to: Anchor::World(end),
                    width: LASER_WIDTH_M,
                    color: with_alpha(LASER, 0.8),
                    depth: DepthMode::Tested,
                });
            }
        }
    }
}

/// A ghost at each controller whose hand the animation owns: a ring at the grip and a short pointer
/// along the aim, fading in as the hand yields.
pub fn ghost_hands(
    weights: [f32; 2],
    hands: &[Option<HandPoses>; 2],
    config: &OverlayConfig,
    out: &mut Vec<Primitive>,
) {
    for hand in Hand::BOTH {
        let yielded = 1.0 - weights[hand.index()].clamp(0.0, 1.0);
        if yielded <= 0.0 || hands[hand.index()].is_none() {
            continue;
        }
        let color = with_alpha(GHOST, GHOST[3] * yielded);
        let depth = DepthMode::XRay {
            occluded_alpha: config.xray_alpha,
        };
        out.push(Primitive::Sprite {
            at: Anchor::hand(hand, HandPose::Grip),
            size: SpriteSize::World(GHOST_RING_M),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.35 },
            color,
            depth,
        });
        out.push(Primitive::Line {
            from: Anchor::hand(hand, HandPose::Aim),
            to: Anchor::Hand {
                hand,
                pose: HandPose::Aim,
                offset: Vec3::NEG_Z * GHOST_POINTER_M,
            },
            width: GHOST_POINTER_WIDTH_M,
            color,
            depth,
        });
    }
}

/// Red, green, and blue axes at each tracked hand's grip and aim poses, over the scene.
pub fn gizmos(hands: &[Option<HandPoses>; 2], out: &mut Vec<Primitive>) {
    const AXES: [(Vec3, Rgba); 3] = [
        (Vec3::X, [1.0, 0.2, 0.2, 1.0]),
        (Vec3::Y, [0.2, 1.0, 0.2, 1.0]),
        (Vec3::Z, [0.3, 0.4, 1.0, 1.0]),
    ];
    for hand in Hand::BOTH {
        if hands[hand.index()].is_none() {
            continue;
        }
        for pose in [HandPose::Grip, HandPose::Aim] {
            for (axis, color) in AXES {
                out.push(Primitive::Line {
                    from: Anchor::hand(hand, pose),
                    to: Anchor::Hand {
                        hand,
                        pose,
                        offset: axis * GIZMO_AXIS_M,
                    },
                    width: GIZMO_WIDTH_M,
                    color,
                    depth: DepthMode::Always,
                });
            }
        }
    }
}

/// The sight tuning marker: a ring and a centre dot at the marker, over the scene, and a dot where
/// the dominant hand's sight line passes closest to it, while that hand aims a gun.
pub fn sight_marker(marker: Vec3, dominant: Option<HandAim>, out: &mut Vec<Primitive>) {
    for (shape, size) in [
        (Shape::Ring { thickness: 0.25 }, MARKER_RING_RAD),
        (Shape::Disc, MARKER_DOT_RAD),
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
        size: SpriteSize::Angular(MARKER_DOT_RAD),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color: LASER,
        depth: DepthMode::Always,
    });
}

/// The sources [`tick`] produces.
const PRODUCED: [Source; 6] = [
    Source::GrappleHighlight,
    Source::GrappleBeam,
    Source::GunReadout,
    Source::GhostHands,
    Source::DebugGizmos,
    Source::SightTuning,
];

/// The time to live of the sim-rate producers' slots: a few sim ticks.
const SIM_TTL: Duration = Duration::from_millis(150);
/// The time to live of the frame-rate producers' slots.
const FRAME_TTL: Duration = Duration::from_millis(100);

/// The aim's update count at the last submission of the sim-rate producers.
static LAST_AIM_UPDATE: AtomicU64 = AtomicU64::new(u64::MAX);

const GRAPPLE: Rgba = [0.3, 0.9, 1.0, 0.9];
const TETHER: Rgba = [1.0, 0.65, 0.15, 0.95];
const LASER: Rgba = [1.0, 0.15, 0.1, 0.95];
const GHOST: Rgba = [0.85, 0.92, 1.0, 0.6];
const MARKER: Rgba = [1.0, 0.9, 0.2, 0.95];

/// How close the off-hand's hit must be to the grapple's point for the highlight to take its
/// surface normal.
const SURFACE_MATCH_M: f32 = 0.5;
const GRAPPLE_RING_RAD: f32 = 0.035;
const GUN_DOT_RAD: f32 = 0.006;
const BEAM_WIDTH_M: f32 = 0.008;
const LASER_WIDTH_M: f32 = 0.003;
const GHOST_RING_M: f32 = 0.08;
const GHOST_POINTER_M: f32 = 0.12;
const GHOST_POINTER_WIDTH_M: f32 = 0.012;
const GIZMO_AXIS_M: f32 = 0.08;
const GIZMO_WIDTH_M: f32 = 0.004;
const MARKER_RING_RAD: f32 = 0.02;
const MARKER_DOT_RAD: f32 = 0.003;

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

fn with_alpha([r, g, b, _]: Rgba, a: f32) -> Rgba {
    [r, g, b, a]
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::{aim::ray::Ray, frame::Pose, overlay::resolve::anchor_position};

    fn grapple(valid: bool, tetherable: bool) -> GrappleAim {
        GrappleAim {
            ray: Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap(),
            point: Vec3::new(0.0, 0.0, -30.0),
            hit: true,
            valid,
            tetherable,
        }
    }

    fn poses(position: Vec3) -> HandPoses {
        let pose = Pose {
            position,
            orientation: Quat::from_rotation_x(0.4),
        };
        HandPoses {
            wrist: pose,
            grip: pose,
            aim: pose,
        }
    }

    fn hit(position: Vec3) -> RayHit {
        RayHit {
            position,
            normal: Vec3::Z,
            distance: position.length(),
            object: None,
            body: 0,
        }
    }

    #[test]
    fn the_highlight_shows_only_for_a_valid_target() {
        let mut out = Vec::new();
        grapple_highlight(&grapple(false, false), None, &mut out);
        assert!(out.is_empty());
        grapple_highlight(&grapple(true, false), None, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_tetherable_target_has_its_own_style() {
        let mut plain = Vec::new();
        grapple_highlight(&grapple(true, false), None, &mut plain);
        let mut tether = Vec::new();
        grapple_highlight(&grapple(true, true), None, &mut tether);
        assert_eq!(tether.len(), 2);
        let color = |p: &Primitive| match p {
            Primitive::Sprite { color, .. } => *color,
            Primitive::Line { color, .. } => *color,
        };
        assert_ne!(color(&plain[0]), color(&tether[0]));
    }

    #[test]
    fn the_highlight_lies_on_the_surface_the_hand_hit_there() {
        let mut out = Vec::new();
        let g = grapple(true, false);
        grapple_highlight(&g, Some(hit(g.point + Vec3::X * 0.1)), &mut out);
        assert!(matches!(
            out[0],
            Primitive::Sprite {
                facing: Facing::Surface { .. },
                ..
            }
        ));
        out.clear();
        grapple_highlight(&g, Some(hit(Vec3::new(0.0, 0.0, -5.0))), &mut out);
        assert!(matches!(
            out[0],
            Primitive::Sprite {
                facing: Facing::Billboard,
                ..
            }
        ));
    }

    #[test]
    fn the_laser_starts_at_the_sight_origin_and_follows_the_hand() {
        let origin = Vec3::new(0.3, 1.1, -0.4);
        let aim = HandAim {
            source: HandSource::Gun { gun: 0 },
            ray: Ray::new(origin, Vec3::NEG_Z).unwrap(),
            hit: Some(hit(Vec3::new(0.3, 1.1, -12.0))),
            target: Vec3::new(0.3, 1.1, -12.0),
        };
        let hands = [None, Some(poses(Vec3::new(0.25, 1.0, -0.3)))];
        let mut out = Vec::new();
        gun_readout(GunReadout::Laser, &[None, Some(aim)], &hands, &mut out);
        let Primitive::Line { from, .. } = out[0] else {
            panic!("the laser is a line");
        };
        assert!(anchor_position(from, &hands).unwrap().distance(origin) < 1e-5);

        let mut moved = hands;
        moved[1] = Some(poses(Vec3::new(0.35, 1.0, -0.3)));
        let shifted = anchor_position(from, &moved).unwrap();
        assert!(shifted.distance(origin + Vec3::X * 0.1) < 1e-5);
    }

    #[test]
    fn the_dot_needs_a_hit_and_a_gun() {
        let mut aim = HandAim {
            source: HandSource::Gun { gun: 0 },
            ray: Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap(),
            hit: None,
            target: Vec3::new(0.0, 0.0, -50.0),
        };
        let mut out = Vec::new();
        gun_readout(GunReadout::Dot, &[Some(aim), None], &[None; 2], &mut out);
        assert!(out.is_empty());
        aim.hit = Some(hit(Vec3::new(0.0, 0.0, -8.0)));
        aim.source = HandSource::AimPose;
        gun_readout(GunReadout::Dot, &[Some(aim), None], &[None; 2], &mut out);
        assert!(out.is_empty());
        aim.source = HandSource::Gun { gun: 0 };
        gun_readout(GunReadout::Dot, &[Some(aim), None], &[None; 2], &mut out);
        assert_eq!(out.len(), 1);
        gun_readout(GunReadout::None, &[Some(aim), None], &[None; 2], &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn ghosts_appear_only_for_yielding_hands() {
        let hands = [Some(poses(Vec3::ZERO)), Some(poses(Vec3::X))];
        let mut out = Vec::new();
        ghost_hands([1.0, 1.0], &hands, &OverlayConfig::new(), &mut out);
        assert!(out.is_empty());
        ghost_hands([1.0, 0.25], &hands, &OverlayConfig::new(), &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|p| match p {
            Primitive::Sprite { at, .. } | Primitive::Line { from: at, .. } => matches!(
                at,
                Anchor::Hand {
                    hand: Hand::Right,
                    ..
                }
            ),
        }));
    }

    #[test]
    fn gizmos_draw_three_axes_per_pose() {
        let mut out = Vec::new();
        gizmos(&[Some(poses(Vec3::ZERO)), None], &mut out);
        assert_eq!(out.len(), 6);
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
