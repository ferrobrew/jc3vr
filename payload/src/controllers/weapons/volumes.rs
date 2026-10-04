//! Placing the body's grab volumes: the holsters, the grenade pouch, the magazine pouch, and the C4
//! pouch. Pure.
//!
//! The volumes are placed in the cockpit frame, where the hands' grab points are, relative to the
//! character's body frame. The cockpit frame composes onto the character's world transform with the
//! character's animated head at its origin (see `headpose::xr`), so in the cockpit frame the body
//! faces the frame's own axes (+X right, +Y up, -Z forward) with the eyes at the origin. The volumes
//! therefore stay on the body when the player turns or leans their head to look at them, and move
//! with the body when a stick turn or the roomscale chase moves it.
//!
//! Vertically, the body frame follows the head down once the head drops further than a deadband
//! below its upright height, so a crouch carries the holsters down while glancing at one does not.
//!
//! Each volume sits at an offset from a body landmark estimated from the body dimensions and the
//! body's [`Stance`]:
//!
//! - the hips, at a fixed fraction of the standing height, level with the shoulders' width and the
//!   shoulders' depth; seated, where the hands reach for the hips instead: higher up the torso and
//!   forward over the thighs, because the seat is where a standing body's hip joints are;
//! - the shoulder joints ([`body::shoulder_positions`]); seated, a little forward, over the tops of
//!   the shoulders, because a backrest is behind them;
//! - the chest, at the middle of the shoulder joints, for the chest holster and the C4 pouch;
//! - the belt front, for the grenade and magazine pouches: higher up the torso and further forward
//!   than the hips, clear of the body, and biased toward one side so the pouch sits apart from both
//!   the hip holster beside it and the other pouch mirrored on the opposite side; seated, adjusted
//!   like the hips.
//!
//! The dimensions are the character's while the hands are remapped onto the character's proportions
//! (a hand the player puts on their own hip lands on the character's), and the player's otherwise.
//!
//! The stance ([`Stance`]) comes from the roomscale posture
//! (`crate::controllers::roomscale::posture::EffectivePosture`), the single source of truth for
//! seated versus standing that the head height and the chase also key off: the seated posture is
//! seated, and the standing and roomscale postures are standing. That mapping is the caller's job
//! ([`crate::controllers::weapons`]), so that this module stays pure.

use glam::{Quat, Vec3};

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume, HolsterSite},
    body::{self, BodyDimensions},
    frame::Hand,
    weapons::config::{HolstersConfig, VolumeSpec},
};

/// How the body the volumes are placed on is posed, resolved by the caller from the roomscale
/// posture.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Stance {
    /// Standing upright with the eyes at the cockpit frame's origin height.
    Standing,
    /// Seated, with the upright seated eyes at the cockpit frame's origin height: the roomscale
    /// posture already measures the cockpit height from the seated eye reference, so seated needs
    /// no offset of its own here.
    Seated,
}

impl Stance {
    pub fn label(self) -> &'static str {
        match self {
            Stance::Standing => "standing",
            Stance::Seated => "seated",
        }
    }
}

/// What the volumes are placed from: where the body frame's eyes are, and how the body is posed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    /// The body frame's origin in the cockpit frame: the eyes at their upright height, dropped with
    /// a head that has dropped further than the deadband below it.
    pub origin: Vec3,
    pub seated: bool,
}

/// The body frame for a head at `head` in `stance`: the eyes at the cockpit frame's origin, until
/// the head drops more than `crouch_deadband` below it.
pub fn body_frame(head: Vec3, stance: Stance, crouch_deadband: f32) -> BodyFrame {
    let deadband = if crouch_deadband.is_finite() {
        crouch_deadband.max(0.0)
    } else {
        0.0
    };
    let y = if head.y.is_finite() {
        (head.y + deadband).min(0.0)
    } else {
        0.0
    };
    BodyFrame {
        origin: Vec3::new(0.0, y, 0.0),
        seated: stance == Stance::Seated,
    }
}

/// The holster volumes (and the grenade pouch with `grenade_pouch`) for a body in `frame` with
/// dimensions `dims`, in the order the sites are listed. Every site may be grabbed with either
/// hand: the hip and shoulder holsters by design, and the pouch because a player holding a weapon
/// in the dominant hand reaches for a grenade with the other. Whichever hand grabs the pouch throws.
pub fn holster_candidates(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    config: &HolstersConfig,
    dominant: Hand,
    grenade_pouch: bool,
) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(6);
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Hip(side)),
            hip(frame, dims, side),
            &config.hip,
            side,
            None,
        ));
    }
    let shoulders = body::shoulder_positions(frame.origin, Quat::IDENTITY, dims);
    let over_shoulder = if frame.seated {
        Vec3::NEG_Z * SEATED_SHOULDER_FORWARD_M
    } else {
        Vec3::ZERO
    };
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Shoulder(side)),
            shoulders[side.index()] + over_shoulder,
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
    if grenade_pouch {
        let off = dominant.other();
        out.push(place(
            GrabVolume::GrenadePouch,
            belt(frame, dims, off),
            &config.pouch,
            off,
            None,
        ));
    }
    out
}

/// The magazine pouch on the belt at the dominant side, mirrored from the grenade pouch so the two
/// do not share a site, but a volume only the off-hand may grab: the reload gesture's first step
/// (the dominant hand holds the weapon being reloaded).
pub fn magazine_pouch(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    spec: &VolumeSpec,
    dominant: Hand,
) -> Candidate {
    place(
        GrabVolume::MagazinePouch,
        belt(frame, dims, dominant),
        spec,
        dominant,
        Some(dominant.other()),
    )
}

/// The C4 pouch on the chest at the dominant side, a volume either hand may grab (whichever grabs it
/// throws). It is placed from the middle of the shoulder joints, like the chest holster, and offset
/// toward the dominant side, below the dominant shoulder's holster and beside the chest holster.
pub fn c4_pouch(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    spec: &VolumeSpec,
    dominant: Hand,
) -> Candidate {
    let shoulders = body::shoulder_positions(frame.origin, Quat::IDENTITY, dims);
    place(
        GrabVolume::C4Pouch,
        (shoulders[0] + shoulders[1]) * 0.5,
        spec,
        dominant,
        None,
    )
}

/// A grab point outside every holster that `hand` may grab: the nearest holster, how far the point
/// is outside its surface, and the point's offset from its centre.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct NearMiss {
    pub volume: GrabVolume,
    pub outside_m: f32,
    pub offset: Vec3,
}

/// The holster or pouch nearest `point` among `candidates` that `hand` may grab, when `point` is
/// outside every one of them and within `margin` of that one's surface.
pub fn near_miss(
    point: Vec3,
    hand: Hand,
    candidates: &[Candidate],
    margin: f32,
) -> Option<NearMiss> {
    if !point.is_finite() {
        return None;
    }
    let holsters = candidates.iter().filter(|c| {
        matches!(
            c.volume,
            GrabVolume::Holster(_)
                | GrabVolume::GrenadePouch
                | GrabVolume::MagazinePouch
                | GrabVolume::C4Pouch
        ) && c.hand.is_none_or(|h| h == hand)
            && c.radius.is_finite()
            && c.radius > 0.0
    });
    let (nearest, outside) = holsters
        .map(|c| (c, point.distance(c.centre) - c.radius))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    (outside > 0.0 && outside <= margin).then(|| NearMiss {
        volume: nearest.volume,
        outside_m: outside,
        offset: point - nearest.centre,
    })
}

/// The standing height of the hip joints as a fraction of the standing height: a generic
/// anthropometric figure, which the body file does not carry because it varies little between
/// adults.
pub const HIP_HEIGHT_FRACTION: f32 = 0.53;

/// How far up the torso, from the hip joints toward the shoulder joints and as a fraction of that
/// length, a seated player's hands reach for the hips: the belt line at the side of the waist, above
/// the seat. A best guess.
const SEATED_HIP_RISE_FRACTION: f32 = 0.35;

/// How far forward of the hip joints a seated player's hands reach for the hips, over the thighs,
/// metres. A best guess.
const SEATED_HIP_FORWARD_M: f32 = 0.15;

/// How far forward of the shoulder joints the shoulder volumes sit for a seated body, metres: over
/// the tops of the shoulders rather than behind them, where a backrest is. A best guess.
const SEATED_SHOULDER_FORWARD_M: f32 = 0.05;

/// How far up the torso, as a fraction of the standing height, the belt-front landmark sits for the
/// grenade and magazine pouches: above the hip joints, clear of the hip holster beside it. A best
/// guess.
const BELT_HEIGHT_FRACTION: f32 = 0.6;

/// How far out from the midline, as a fraction of the shoulder width, the belt-front landmark sits
/// toward its side: less than the hip joints', so the pouch on one side and its mirror on the other
/// stay apart. A best guess.
const BELT_OUT_FRACTION: f32 = 0.3;

/// How far in front of the eye plane the belt-front landmark sits, metres: clear of the body and of
/// the hip holster beside it, rather than at the hips' depth behind the eyes. A best guess.
const BELT_FORWARD_M: f32 = 0.18;

/// The hip landmark on `side`: as far out as the shoulder joint; standing, at the hip joints' height
/// and the shoulders' depth; seated, higher up the torso and forward over the thighs.
fn hip(frame: &BodyFrame, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = dims.shoulder_width * 0.5 * side_sign(side);
    let hip_height = dims.height * HIP_HEIGHT_FRACTION;
    let standing = Vec3::new(
        out,
        hip_height - dims.eye_height,
        body::EYES_FORWARD_OF_SHOULDERS,
    );
    frame.origin + seated_waist_adjust(frame, dims, hip_height, standing)
}

/// The belt-front landmark on `side`: higher up the torso than the hips, in front of the eye plane
/// rather than behind it, and biased toward `side` by less than the hips are, so a pouch here sits
/// clear of the hip holster beside it and of its own mirror on the opposite side. Standing and
/// seated as [`hip`].
fn belt(frame: &BodyFrame, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = dims.shoulder_width * BELT_OUT_FRACTION * side_sign(side);
    let belt_height = dims.height * BELT_HEIGHT_FRACTION;
    let standing = Vec3::new(out, belt_height - dims.eye_height, -BELT_FORWARD_M);
    frame.origin + seated_waist_adjust(frame, dims, belt_height, standing)
}

/// Adjust a standing waist-level landmark for a seated body: higher up the torso and forward over
/// the thighs, because the seat is where a standing body's hip joints are. `base_height` is the
/// landmark's standing height above the floor, used to find how much torso remains above it.
fn seated_waist_adjust(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    base_height: f32,
    standing: Vec3,
) -> Vec3 {
    if !frame.seated {
        return standing;
    }
    let torso = (dims.shoulder_height - base_height).max(0.0);
    standing + Vec3::new(0.0, torso * SEATED_HIP_RISE_FRACTION, -SEATED_HIP_FORWARD_M)
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
    use crate::controllers::{
        bindings::grab::grab_volume,
        weapons::config::{C4Config, MagazineConfig},
    };

    const DIMS: BodyDimensions = BodyDimensions::GENERIC_ADULT;
    const STANDING: BodyFrame = BodyFrame {
        origin: Vec3::ZERO,
        seated: false,
    };
    const SEATED: BodyFrame = BodyFrame {
        origin: Vec3::ZERO,
        seated: true,
    };

    fn candidates(config: &HolstersConfig) -> Vec<Candidate> {
        holster_candidates(&STANDING, &DIMS, config, Hand::Right, false)
    }

    fn centre(c: &[Candidate], volume: GrabVolume) -> Vec3 {
        c.iter().find(|c| c.volume == volume).unwrap().centre
    }

    const RIGHT_HIP: GrabVolume = GrabVolume::Holster(HolsterSite::Hip(Hand::Right));
    const RIGHT_SHOULDER: GrabVolume = GrabVolume::Holster(HolsterSite::Shoulder(Hand::Right));

    #[test]
    fn the_holsters_sit_where_the_body_has_them() {
        let config = HolstersConfig::new();
        let c = candidates(&config);
        let right_hip = centre(&c, RIGHT_HIP);
        let left_hip = centre(&c, GrabVolume::Holster(HolsterSite::Hip(Hand::Left)));
        let right_shoulder = centre(&c, RIGHT_SHOULDER);
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
    fn seated_hips_are_higher_and_forward_and_the_rest_follow_the_torso() {
        let config = HolstersConfig::new();
        for dims in [
            BodyDimensions::GENERIC_ADULT,
            BodyDimensions::CHARACTER_ESTIMATE,
        ] {
            let standing = holster_candidates(&STANDING, &dims, &config, Hand::Right, false);
            let seated = holster_candidates(&SEATED, &dims, &config, Hand::Right, false);
            let (up, sh) = (centre(&standing, RIGHT_HIP), centre(&seated, RIGHT_HIP));
            // At the belt line beside the waist: about half a metre below the eyes, close to the
            // body, and in front of the eyes over the thighs.
            assert!(sh.y > up.y + 0.1, "{sh} vs {up}");
            assert!(sh.y < -0.4 && sh.y > -0.65, "{sh}");
            assert!(sh.z < 0.0 && sh.z > -0.15, "{sh}");
            assert_eq!(sh.x, up.x);
            // The shoulders come forward a little, clear of a backrest; the chest stays.
            let (us, ss) = (
                centre(&standing, RIGHT_SHOULDER),
                centre(&seated, RIGHT_SHOULDER),
            );
            assert!(ss.z < us.z && ss.z > 0.0 && ss.y == us.y);
            let chest = GrabVolume::Holster(HolsterSite::Chest);
            assert_eq!(centre(&standing, chest), centre(&seated, chest));
        }
    }

    #[test]
    fn the_default_holsters_do_not_overlap() {
        let config = HolstersConfig::new();
        let magazine = MagazineConfig::new().pouch;
        for frame in [STANDING, SEATED] {
            for dims in [
                BodyDimensions::GENERIC_ADULT,
                BodyDimensions::CHARACTER_ESTIMATE,
            ] {
                let mut c = holster_candidates(&frame, &dims, &config, Hand::Right, true);
                c.push(magazine_pouch(&frame, &dims, &magazine, Hand::Right));
                c.push(c4_pouch(&frame, &dims, &C4Config::new().pouch, Hand::Right));
                for (i, a) in c.iter().enumerate() {
                    for b in &c[i + 1..] {
                        assert!(
                            a.centre.distance(b.centre) > a.radius + b.radius,
                            "{:?} and {:?} overlap (seated {})",
                            a.volume,
                            b.volume,
                            frame.seated
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_seated_hand_at_the_waist_grabs_the_hip_holster() {
        let config = HolstersConfig::new();
        let dims = BodyDimensions::CHARACTER_ESTIMATE;
        // A hand resting at the side of the waist, over the front of the seat: half a metre below
        // the eyes, a hand's width out from the body, a little in front of the eyes.
        let hand = Vec3::new(0.22, -0.5, -0.08);
        let seated = holster_candidates(&SEATED, &dims, &config, Hand::Right, false);
        assert_eq!(grab_volume(hand, Hand::Right, &seated), Some(RIGHT_HIP));
        let standing = holster_candidates(&STANDING, &dims, &config, Hand::Right, false);
        assert_eq!(grab_volume(hand, Hand::Right, &standing), None);
    }

    #[test]
    fn the_pouch_exists_only_when_enabled_and_either_hand_may_grab_it() {
        let config = HolstersConfig::new();
        assert!(
            candidates(&config)
                .iter()
                .all(|c| c.volume != GrabVolume::GrenadePouch)
        );
        let c = holster_candidates(&STANDING, &DIMS, &config, Hand::Right, true);
        let pouch = c
            .iter()
            .find(|c| c.volume == GrabVolume::GrenadePouch)
            .unwrap();
        assert_eq!(pouch.hand, None);
        // On the off-hand side, away from the dominant hand's hip holster and gun.
        assert!(pouch.centre.x < 0.0);
        assert_eq!(
            grab_volume(pouch.centre, Hand::Left, &c),
            Some(GrabVolume::GrenadePouch)
        );
        assert_eq!(
            grab_volume(pouch.centre, Hand::Right, &c),
            Some(GrabVolume::GrenadePouch)
        );
    }

    #[test]
    fn the_magazine_pouch_is_on_the_dominant_side_for_the_off_hand_only() {
        let spec = MagazineConfig::new().pouch;
        for frame in [STANDING, SEATED] {
            let pouch = magazine_pouch(&frame, &DIMS, &spec, Hand::Right);
            // Mirrored from the grenade pouch, on the dominant side, so neither pouch shares a site.
            assert!(pouch.centre.x > 0.0);
            assert_eq!(pouch.hand, Some(Hand::Left));
            let mut c =
                holster_candidates(&frame, &DIMS, &HolstersConfig::new(), Hand::Right, true);
            c.push(pouch);
            assert_eq!(
                grab_volume(pouch.centre, Hand::Left, &c),
                Some(GrabVolume::MagazinePouch)
            );
            assert_ne!(
                grab_volume(pouch.centre, Hand::Right, &c),
                Some(GrabVolume::MagazinePouch)
            );
        }
    }

    #[test]
    fn the_c4_pouch_is_on_the_dominant_side_of_the_chest_for_either_hand() {
        let spec = C4Config::new().pouch;
        for dominant in Hand::BOTH {
            for frame in [STANDING, SEATED] {
                let mut c =
                    holster_candidates(&frame, &DIMS, &HolstersConfig::new(), dominant, true);
                let pouch = c4_pouch(&frame, &DIMS, &spec, dominant);
                c.push(magazine_pouch(
                    &frame,
                    &DIMS,
                    &MagazineConfig::new().pouch,
                    dominant,
                ));
                c.push(pouch);
                // On the dominant side, between the shoulders' height and the belt's, in front of
                // the eyes.
                assert_eq!(pouch.centre.x > 0.0, dominant == Hand::Right);
                let chest = centre(&c, GrabVolume::Holster(HolsterSite::Chest));
                let belt = centre(&c, GrabVolume::MagazinePouch);
                assert!(pouch.centre.y < 0.0 && pouch.centre.y > belt.y, "{pouch:?}");
                assert!((pouch.centre.y - chest.y).abs() < 0.1, "{pouch:?}");
                assert!(pouch.centre.z < 0.0);
                assert_eq!(pouch.hand, None);
                for hand in Hand::BOTH {
                    assert_eq!(
                        grab_volume(pouch.centre, hand, &c),
                        Some(GrabVolume::C4Pouch)
                    );
                }
            }
        }
        // A left-handed player's pouch mirrors a right-handed player's.
        let right = c4_pouch(&STANDING, &DIMS, &spec, Hand::Right).centre;
        let left = c4_pouch(&STANDING, &DIMS, &spec, Hand::Left).centre;
        assert!((right.x + left.x).abs() < 1e-6 && right.y == left.y && right.z == left.z);
    }

    #[test]
    fn a_hand_at_a_holster_grabs_it_and_a_hand_in_front_grabs_nothing() {
        for frame in [STANDING, SEATED] {
            let c = holster_candidates(&frame, &DIMS, &HolstersConfig::new(), Hand::Right, false);
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
        }
        // A hand hanging relaxed at the side, a little forward of the holster, still reaches it.
        let c = candidates(&HolstersConfig::new());
        let hip = centre(&c, RIGHT_HIP);
        assert!(grab_volume(hip + Vec3::new(0.0, 0.0, -0.08), Hand::Right, &c).is_some());
    }

    #[test]
    fn the_body_drops_only_past_the_deadband() {
        let origin = |head, stance| body_frame(head, stance, 0.15).origin;
        for stance in [Stance::Standing, Stance::Seated] {
            assert_eq!(origin(Vec3::new(0.3, -0.1, 0.2), stance), Vec3::ZERO);
            assert_eq!(origin(Vec3::new(0.0, 0.2, 0.0), stance), Vec3::ZERO);
            let crouched = origin(Vec3::new(0.0, -0.5, 0.0), stance);
            assert!((crouched.y + 0.35).abs() < 1e-6);
            assert_eq!(origin(Vec3::NAN, stance), Vec3::ZERO);
        }
        assert!(body_frame(Vec3::ZERO, Stance::Seated, 0.15).seated);
        assert!(!body_frame(Vec3::ZERO, Stance::Standing, 0.15).seated);
    }

    #[test]
    fn a_near_miss_names_the_nearest_holster_and_its_distance() {
        let c = candidates(&HolstersConfig::new());
        let hip = c.iter().find(|c| c.volume == RIGHT_HIP).unwrap();
        let point = hip.centre + Vec3::new(0.0, hip.radius + 0.05, 0.0);
        let miss = near_miss(point, Hand::Right, &c, 0.3).unwrap();
        assert_eq!(miss.volume, RIGHT_HIP);
        assert!((miss.outside_m - 0.05).abs() < 1e-5);
        assert!((miss.offset.y - (hip.radius + 0.05)).abs() < 1e-5);
        // Inside a holster, or far from all of them, is not a near miss.
        assert_eq!(near_miss(hip.centre, Hand::Right, &c, 0.3), None);
        assert_eq!(
            near_miss(Vec3::new(0.0, 0.5, -1.0), Hand::Right, &c, 0.3),
            None
        );
        assert_eq!(near_miss(Vec3::NAN, Hand::Right, &c, 0.3), None);
    }

    #[test]
    fn a_near_miss_also_names_a_pouch() {
        let c = holster_candidates(&STANDING, &DIMS, &HolstersConfig::new(), Hand::Right, true);
        let pouch = c
            .iter()
            .find(|c| c.volume == GrabVolume::GrenadePouch)
            .unwrap();
        let point = pouch.centre + Vec3::new(0.0, pouch.radius + 0.05, 0.0);
        let miss = near_miss(point, Hand::Left, &c, 0.3).unwrap();
        assert_eq!(miss.volume, GrabVolume::GrenadePouch);
    }
}
