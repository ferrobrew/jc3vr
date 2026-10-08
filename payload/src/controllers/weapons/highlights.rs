//! The grab-volume highlights: each placed pouch (faint, and brighter while a hand is within
//! reach), with `holsters.show_volumes` every body volume, the volume each hand is in, the gun's
//! well while a round is in hand, and a held foregrip, submitted to the overlay's
//! [`Source::GrabVolumes`] slot at the input tick's rate.

use std::time::Duration;

use glam::{Quat, Vec3};

use crate::{
    controllers::{
        bindings::grab::{Candidate, GrabVolume},
        config::ControllersConfig,
        frame::{Hand, Pose},
        hands::{ActiveHands, CockpitHand, pose as hand_pose},
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        weapons::{foregrip, is_body_volume, reload, volumes},
    },
    headpose,
};

/// The tick's volumes and where the hands are among them, for the highlights.
pub(super) struct Reach<'a> {
    pub candidates: &'a [Candidate],
    /// The volume each hand is in, by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// Each hand's point for the body's volumes, by [`Hand::index`].
    pub body_points: [Option<Vec3>; 2],
}

impl Reach<'_> {
    /// Whether a hand that may grab `c` is within reach of it: inside it, or within the pouches'
    /// reach margin of its surface.
    fn within(&self, c: &Candidate, margin: f32) -> bool {
        Hand::BOTH.into_iter().any(|hand| {
            c.hand.is_none_or(|h| h == hand)
                && self.body_points[hand.index()]
                    .is_some_and(|p| p.distance(c.centre) <= c.radius + margin.max(0.0))
        })
    }

    /// Where to draw the body volume `c` for the player to see it, given `drawn`, the hands as
    /// drawn: on the player's body the volumes are reached by the player's own hand before the
    /// body-scale remap, but the hand is drawn remapped, so the volume is drawn shifted with the
    /// hand that reaches for it, `reacher` (or, with none named, the hand that may grab it and is
    /// nearest), so that the drawn hand meets the drawn ring where the grab happens. On the
    /// character's skeleton the two points agree and nothing shifts.
    fn drawn_centre(
        &self,
        c: &Candidate,
        reacher: Option<Hand>,
        drawn: &[Option<CockpitHand>; 2],
    ) -> Vec3 {
        let shift = |hand: Hand| {
            let body = self.body_points[hand.index()]?;
            let drawn = drawn[hand.index()]?.grip.position;
            Some((drawn - body, body.distance(c.centre)))
        };
        let shift = match reacher.or(c.hand) {
            Some(hand) => shift(hand),
            None => Hand::BOTH
                .into_iter()
                .filter_map(shift)
                .min_by(|a, b| a.1.total_cmp(&b.1)),
        };
        c.centre + shift.map_or(Vec3::ZERO, |(shift, _)| shift)
    }
}

/// Submit a highlight for the volume each hand is in, every placed pouch (faint, and brighter
/// while a hand is within reach of it), and a marker on a held foregrip.
pub(super) fn submit(config: &ControllersConfig, active: &ActiveHands, reach: &Reach) {
    let Reach {
        candidates,
        volumes,
        ..
    } = *reach;
    let show_all = config.weapons.holsters.show_volumes;
    let pouches = &config.weapons.pouches;
    if !(config.overlay.enabled && (config.overlay.grab_volumes || show_all || pouches.show)) {
        store::clear(Source::GrabVolumes);
        return;
    }
    let head = headpose::query();
    let body = headpose::xr::body_rotation();
    let to_world = |point: Vec3| {
        hand_pose::compose_world(
            head.position,
            body,
            active.head.position,
            Pose {
                position: point,
                orientation: Quat::IDENTITY,
            },
            active.world_scale,
        )
        .position
    };
    let mut prims = Vec::new();
    for c in candidates
        .iter()
        .filter(|c| is_body_volume(c.volume) && !volumes.contains(&Some(c.volume)))
    {
        let pouch = volumes::is_pouch(c.volume) && pouches.show;
        let alpha = if pouch && reach.within(c, pouches.reach_margin_m) {
            REACH_ALPHA
        } else if pouch || show_all {
            SHOWN_ALPHA
        } else {
            continue;
        };
        let [r, g, b, _] = colour(c.volume);
        prims.push(ring(
            to_world(reach.drawn_centre(c, None, &active.hands)),
            c.radius * active.world_scale,
            [r, g, b, alpha],
        ));
    }
    let highlight = |volume: GrabVolume| {
        config.overlay.grab_volumes || show_all || (pouches.show && volumes::is_pouch(volume))
    };
    for (volume, hand) in volumes.iter().zip(Hand::BOTH) {
        let Some(volume) = volume.filter(|v| highlight(*v)) else {
            continue;
        };
        if let Some(c) = candidates.iter().find(|c| c.volume == volume) {
            let centre = if is_body_volume(volume) {
                reach.drawn_centre(c, Some(hand), &active.hands)
            } else {
                c.centre
            };
            prims.push(ring(
                to_world(centre),
                c.radius * active.world_scale,
                colour(volume),
            ));
        }
    }
    if let Some((centre, radius)) = reload::well_ring(&config.weapons.reload) {
        prims.push(ring(
            to_world(centre),
            radius * active.world_scale,
            MAGAZINE,
        ));
    }
    if let Some(hand) = reload::holding_hand()
        && let Some(held) = active.hands[hand.index()]
    {
        prims.push(marker(
            to_world(held.grip.position),
            active.world_scale,
            MAGAZINE,
        ));
    }
    if let Some(hold) = foregrip::status().hold.filter(|h| h.engaged) {
        let mut posed = active.hands;
        foregrip::apply(&mut posed, &config.weapons.foregrip);
        if let Some(off) = posed[hold.holder.other().index()] {
            prims.push(marker(
                to_world(off.grip.position),
                active.world_scale,
                FOREGRIP,
            ));
        }
    }
    store::submit(Source::GrabVolumes, &mut prims, HIGHLIGHT_TTL);
}

fn ring(centre: Vec3, radius: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(centre),
        size: SpriteSize::World(radius * 2.0),
        facing: Facing::Billboard,
        shape: Shape::Ring { thickness: 0.15 },
        color,
        depth: DepthMode::XRay {
            occluded_alpha: 0.4,
        },
    }
}

fn marker(at: Vec3, world_scale: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(at),
        size: SpriteSize::World(0.03 * world_scale),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color,
        depth: DepthMode::XRay {
            occluded_alpha: 0.4,
        },
    }
}

fn colour(volume: GrabVolume) -> Rgba {
    match volume {
        GrabVolume::Holster(_) | GrabVolume::Socket(_) => HOLSTER,
        GrabVolume::Foregrip | GrabVolume::Handle | GrabVolume::WorldWeapon => FOREGRIP,
        GrabVolume::GrenadePouch
        | GrabVolume::VehicleControl(_)
        | GrabVolume::ParachuteHandle(_)
        | GrabVolume::MountedHandle(_)
        | GrabVolume::Pickup
        | GrabVolume::Climb
        | GrabVolume::StuntHold => POUCH,
        GrabVolume::MagazinePouch | GrabVolume::WeaponAction | GrabVolume::WorldMagazine => {
            MAGAZINE
        }
        GrabVolume::C4Pouch | GrabVolume::GrenadePin | GrabVolume::LiveGrenade => C4,
    }
}

const HOLSTER: Rgba = [1.0, 0.75, 0.25, 0.8];
const FOREGRIP: Rgba = [0.35, 0.85, 1.0, 0.8];
const POUCH: Rgba = [0.5, 1.0, 0.45, 0.8];
const MAGAZINE: Rgba = [0.85, 0.55, 1.0, 0.8];
const C4: Rgba = [1.0, 0.4, 0.3, 0.8];
/// The opacity of a body volume drawn with `show_volumes`, or of a pouch, that no hand is in or
/// near: faint, so that it shows where the pouch is without drawing the eye.
const SHOWN_ALPHA: f32 = 0.2;
/// The opacity of a pouch a hand is within reach of but not in.
const REACH_ALPHA: f32 = 0.5;

/// The highlights are submitted at the input tick's rate, so they live a little past one tick.
const HIGHLIGHT_TTL: Duration = Duration::from_millis(150);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_volume_is_drawn_shifted_with_the_remapped_hand_that_reaches_for_it() {
        let at = |position: Vec3| Pose {
            position,
            orientation: Quat::IDENTITY,
        };
        let hand = |position: Vec3| CockpitHand {
            wrist: at(position),
            grip: at(position),
            aim: at(position),
        };
        // The player's own hands, and the hands drawn after the remap: the left moved out by 5 cm,
        // the right forward by 3 cm.
        let body_points = [
            Some(Vec3::new(-0.2, -0.6, 0.0)),
            Some(Vec3::new(0.2, -0.6, 0.0)),
        ];
        let drawn = [
            Some(hand(Vec3::new(-0.25, -0.6, 0.0))),
            Some(hand(Vec3::new(0.2, -0.6, -0.03))),
        ];
        let pouch = Candidate {
            volume: GrabVolume::MagazinePouch,
            centre: Vec3::new(-0.1, -0.6, -0.1),
            radius: 0.075,
            hand: Some(Hand::Left),
        };
        let reach = Reach {
            candidates: &[],
            volumes: [None; 2],
            body_points,
        };
        let close = |a: Vec3, b: Vec3| a.distance(b) < 1e-6;
        let left_shift = Vec3::new(-0.05, 0.0, 0.0);
        assert!(close(
            reach.drawn_centre(&pouch, None, &drawn),
            pouch.centre + left_shift
        ));
        // A volume either hand may grab follows the nearer hand, unless a hand is named.
        let either = Candidate {
            hand: None,
            centre: Vec3::new(0.15, -0.6, 0.0),
            ..pouch
        };
        let right_shift = Vec3::new(0.0, 0.0, -0.03);
        assert!(close(
            reach.drawn_centre(&either, None, &drawn),
            either.centre + right_shift
        ));
        assert!(close(
            reach.drawn_centre(&either, Some(Hand::Left), &drawn),
            either.centre + left_shift
        ));
        // Without a remap (the skeleton's volumes), the two points agree and nothing moves.
        let unmapped = Reach {
            body_points: drawn.map(|h| h.map(|h| h.grip.position)),
            ..reach
        };
        assert!(close(
            unmapped.drawn_centre(&pouch, None, &drawn),
            pouch.centre
        ));
    }
}
