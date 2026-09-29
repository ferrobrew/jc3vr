//! Placing the body's grab volumes: the holsters and the grenade pouch. Pure.
//!
//! The volumes are placed in the cockpit frame, where the hands' grab points are, relative to the
//! character's body frame. The cockpit frame composes onto the character's world transform with the
//! character's animated head at its origin (see `headpose::xr`), so in the cockpit frame the body
//! faces the frame's own axes (+X right, +Y up, -Z forward) with the eyes at the origin. The volumes
//! therefore stay on the body when the player turns or leans their head to look at them, and move
//! with the body when a stick turn or the roomscale chase moves it.
//!
//! Vertically, the body frame follows the head down once the head drops further than a deadband
//! below its standing height, so a crouch carries the holsters down while glancing at one does not.
//!
//! Each volume sits at an offset from a body landmark estimated from the body dimensions:
//!
//! - the hips, at a fixed fraction of the standing height, level with the shoulders' width and the
//!   shoulders' depth;
//! - the shoulder joints ([`body::shoulder_positions`]);
//! - the chest, at the middle of the shoulders.
//!
//! The dimensions are the character's while the hands are remapped onto the character's proportions
//! (a hand the player puts on their own hip lands on the character's), and the player's otherwise.

use glam::{Quat, Vec3};

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume, HolsterSite},
    body::{self, BodyDimensions},
    frame::Hand,
    weapons::config::{HolstersConfig, VolumeSpec},
};

/// The body frame's origin in the cockpit frame: the eyes at standing height, dropped with a head
/// that has dropped more than `crouch_deadband` below it.
pub fn body_origin(head: Vec3, crouch_deadband: f32) -> Vec3 {
    let drop = if head.y.is_finite() {
        (head.y + crouch_deadband.max(0.0)).min(0.0)
    } else {
        0.0
    };
    Vec3::new(0.0, drop, 0.0)
}

/// The holster volumes (and the grenade pouch when enabled) for a body at `origin` with dimensions
/// `dims`, in the order the sites are listed. The hip and shoulder holsters may be grabbed with
/// either hand; the pouch only with the dominant hand, which throws.
pub fn holster_candidates(
    origin: Vec3,
    dims: &BodyDimensions,
    config: &HolstersConfig,
    dominant: Hand,
) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(6);
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Hip(side)),
            hip(origin, dims, side),
            &config.hip,
            side,
            None,
        ));
    }
    let shoulders = body::shoulder_positions(origin, Quat::IDENTITY, dims);
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Shoulder(side)),
            shoulders[side.index()],
            &config.shoulder,
            side,
            None,
        ));
    }
    out.push(place(
        GrabVolume::Holster(HolsterSite::Chest),
        (shoulders[0] + shoulders[1]) * 0.5,
        &config.chest,
        Hand::Right,
        None,
    ));
    if config.grenade_pouch {
        out.push(place(
            GrabVolume::GrenadePouch,
            hip(origin, dims, dominant),
            &config.pouch,
            dominant,
            Some(dominant),
        ));
    }
    out
}

/// The standing height of the hip joints as a fraction of the standing height: a generic
/// anthropometric figure, which the body file does not carry because it varies little between
/// adults.
pub const HIP_HEIGHT_FRACTION: f32 = 0.53;

/// The hip landmark on `side`: at the hips' height, as far out as the shoulder joint, and as deep.
fn hip(origin: Vec3, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = dims.shoulder_width * 0.5 * side_sign(side);
    origin
        + Vec3::new(
            out,
            dims.height * HIP_HEIGHT_FRACTION - dims.eye_height,
            body::EYES_FORWARD_OF_SHOULDERS,
        )
}

/// A volume at `spec`'s offset from `landmark`, mirrored for the left side.
fn place(
    volume: GrabVolume,
    landmark: Vec3,
    spec: &VolumeSpec,
    side: Hand,
    hand: Option<Hand>,
) -> Candidate {
    let offset = Vec3::new(
        spec.offset_m.x * side_sign(side),
        spec.offset_m.y,
        spec.offset_m.z,
    );
    Candidate {
        volume,
        centre: landmark + offset,
        radius: spec.radius_m.max(0.0),
        hand,
    }
}

fn side_sign(side: Hand) -> f32 {
    match side {
        Hand::Left => -1.0,
        Hand::Right => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::bindings::grab::grab_volume;

    const DIMS: BodyDimensions = BodyDimensions::GENERIC_ADULT;

    fn candidates(config: &HolstersConfig) -> Vec<Candidate> {
        holster_candidates(Vec3::ZERO, &DIMS, config, Hand::Right)
    }

    fn centre(c: &[Candidate], volume: GrabVolume) -> Vec3 {
        c.iter().find(|c| c.volume == volume).unwrap().centre
    }

    #[test]
    fn the_holsters_sit_where_the_body_has_them() {
        let config = HolstersConfig::new();
        let c = candidates(&config);
        let right_hip = centre(&c, GrabVolume::Holster(HolsterSite::Hip(Hand::Right)));
        let left_hip = centre(&c, GrabVolume::Holster(HolsterSite::Hip(Hand::Left)));
        let right_shoulder = centre(&c, GrabVolume::Holster(HolsterSite::Shoulder(Hand::Right)));
        let chest = centre(&c, GrabVolume::Holster(HolsterSite::Chest));
        assert!(right_hip.x > 0.0 && left_hip.x < 0.0);
        assert!((right_hip.x + left_hip.x).abs() < 1e-6, "the hips mirror");
        // The hips are well below the eyes, the shoulders just below, the chest between.
        assert!(right_hip.y < -0.6 && right_hip.y > -0.8);
        assert!(right_shoulder.y > -0.2 && right_shoulder.y < 0.0);
        assert!(chest.y < right_shoulder.y && chest.y > right_hip.y);
        // Over the shoulder is behind it; the chest is in front.
        assert!(right_shoulder.z > body::EYES_FORWARD_OF_SHOULDERS);
        assert!(chest.z < 0.0);
        assert_eq!(chest.x, 0.0);
    }

    #[test]
    fn the_pouch_exists_only_when_enabled_and_only_for_the_dominant_hand() {
        let mut config = HolstersConfig::new();
        assert!(
            candidates(&config)
                .iter()
                .all(|c| c.volume != GrabVolume::GrenadePouch)
        );
        config.grenade_pouch = true;
        let c = candidates(&config);
        let pouch = c
            .iter()
            .find(|c| c.volume == GrabVolume::GrenadePouch)
            .unwrap();
        assert_eq!(pouch.hand, Some(Hand::Right));
        assert_eq!(grab_volume(pouch.centre, Hand::Left, &c), None);
        assert_eq!(
            grab_volume(pouch.centre, Hand::Right, &c),
            Some(GrabVolume::GrenadePouch)
        );
    }

    #[test]
    fn a_hand_at_a_holster_grabs_it_and_a_hand_in_front_grabs_nothing() {
        let c = candidates(&HolstersConfig::new());
        for candidate in &c {
            assert_eq!(
                grab_volume(candidate.centre, Hand::Left, &c),
                Some(candidate.volume),
                "{:?}",
                candidate.volume
            );
        }
        // A hand held out in front at chest height, as when firing the grapple.
        assert_eq!(
            grab_volume(Vec3::new(0.0, -0.3, -0.45), Hand::Left, &c),
            None
        );
        // A hand hanging relaxed at the side, a little forward of the holster, still reaches it.
        let hip = centre(&c, GrabVolume::Holster(HolsterSite::Hip(Hand::Right)));
        assert!(grab_volume(hip + Vec3::new(0.0, 0.0, -0.08), Hand::Right, &c).is_some());
    }

    #[test]
    fn the_body_drops_only_past_the_deadband() {
        assert_eq!(body_origin(Vec3::new(0.3, -0.1, 0.2), 0.15), Vec3::ZERO);
        assert_eq!(body_origin(Vec3::new(0.0, 0.2, 0.0), 0.15), Vec3::ZERO);
        let crouched = body_origin(Vec3::new(0.0, -0.5, 0.0), 0.15);
        assert!((crouched.y + 0.35).abs() < 1e-6);
        assert_eq!(body_origin(Vec3::NAN, 0.15), Vec3::ZERO);
    }
}
