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
//! The body the volumes ride is one of two ([`BodyFrame`]):
//!
//! - **The player's body**, estimated from the dimensions above, turned to the player's estimated
//!   torso heading ([`crate::controllers::body::torso`]) so that the holsters stay at the player's
//!   hips and shoulders when the player turns in the room.
//! - **The character's skeleton** ([`HolsterAnchor::Rico`]): the same landmarks taken from his hip
//!   and shoulder joints carried into the cockpit frame, so the volumes sit on the body the player
//!   sees. The hip and belt volumes ride the pelvis (the line through the hip joints) and the
//!   shoulder, chest, and C4 volumes ride the chest (the line through the shoulder joints). For a
//!   skeleton built to the dimensions the two coincide. Where the skeleton is unavailable or not
//!   upright (a vehicle, a ragdoll, a skydive) the caller falls back to the player's body.
//!
//! The stance ([`Stance`]) comes from the roomscale posture
//! (`crate::controllers::roomscale::posture::EffectivePosture`), the single source of truth for
//! seated versus standing that the head height and the chase also key off: the seated posture is
//! seated, and the standing and roomscale postures are standing. That mapping is the caller's job
//! ([`crate::controllers::weapons`]), so that this module stays pure.

use glam::{Mat3, Quat, Vec3};

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume, HolsterSite},
    body::{self, BodyDimensions, alignment::measure::SkeletonLandmarks},
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

/// What the volumes are placed from: where the body frame's eyes are, how the body is posed, and
/// which body the volumes ride.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    /// The body frame's origin in the cockpit frame: the eyes at their upright height, dropped with
    /// a head that has dropped further than the deadband below it.
    pub origin: Vec3,
    pub seated: bool,
    /// The body the volumes ride.
    pub anchor: Anchor,
}

/// The body the volumes ride, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Anchor {
    /// The player's body, estimated from the dimensions, facing this heading (radians about +Y,
    /// positive turning -Z toward -X).
    Body { yaw: f32 },
    /// The character's skeleton.
    Skeleton(SkeletonLandmarks),
}

impl Anchor {
    pub fn label(&self) -> &'static str {
        match self {
            Anchor::Body { .. } => "the player's body",
            Anchor::Skeleton(_) => "the character's skeleton",
        }
    }
}

/// The body frame for a head at `head` in `stance`, riding `anchor`: the eyes at the cockpit
/// frame's origin, until the head drops more than `crouch_deadband` below it.
pub fn body_frame(head: Vec3, stance: Stance, crouch_deadband: f32, anchor: Anchor) -> BodyFrame {
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
    let anchor = match anchor {
        Anchor::Body { yaw } if !yaw.is_finite() => Anchor::Body { yaw: 0.0 },
        anchor => anchor,
    };
    BodyFrame {
        origin: Vec3::new(0.0, y, 0.0),
        seated: stance == Stance::Seated,
        anchor,
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
    let pelvis = pelvis_girdle(frame);
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Hip(side)),
            hip(frame, dims, side),
            pelvis,
            &config.hip,
            side,
            None,
        ));
    }
    let chest = chest_girdle(frame, dims);
    let over_shoulder = if frame.seated {
        chest.basis * (Vec3::NEG_Z * SEATED_SHOULDER_FORWARD_M)
    } else {
        Vec3::ZERO
    };
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Shoulder(side)),
            chest.shoulders[side.index()] + over_shoulder,
            chest.basis,
            &config.shoulder,
            side,
            None,
        ));
    }
    out.push(place(
        GrabVolume::Holster(HolsterSite::Chest),
        chest.centre(),
        chest.basis,
        &config.chest,
        Hand::Right,
        None,
    ));
    if grenade_pouch {
        let off = dominant.other();
        out.push(place(
            GrabVolume::GrenadePouch,
            belt(frame, dims, off),
            pelvis,
            &config.pouch,
            off,
            None,
        ));
    }
    out
}

/// The universal reload pouch on the belt at the dominant side, mirrored from the grenade pouch so
/// the two do not share a site, a volume for `grabber`, the hand without a gun (see
/// [`crate::controllers::weapons::reload`]). A gun's well pushed onto it reloads in one hand, so
/// it is placed whether or not a hand is free.
pub fn magazine_pouch(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    spec: &VolumeSpec,
    dominant: Hand,
    grabber: Option<Hand>,
) -> Candidate {
    place(
        GrabVolume::MagazinePouch,
        belt(frame, dims, dominant),
        pelvis_girdle(frame),
        spec,
        dominant,
        grabber,
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
    let chest = chest_girdle(frame, dims);
    place(
        GrabVolume::C4Pouch,
        chest.centre(),
        chest.basis,
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
/// anthropometric figure, which the body dimensions do not carry because it varies little between
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
/// and depth; seated, higher up the torso and forward over the thighs.
fn hip(frame: &BodyFrame, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = shoulder_width(frame, dims) * 0.5 * side_sign(side);
    waist(frame, dims, Vec3::new(out, 0.0, 0.0))
}

/// The belt-front landmark on `side`: higher up the torso than the hips, in front of the eye plane
/// rather than behind it, and biased toward `side` by less than the hips are, so a pouch here sits
/// clear of the hip holster beside it and of its own mirror on the opposite side. Standing and
/// seated as [`hip`].
fn belt(frame: &BodyFrame, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = shoulder_width(frame, dims) * BELT_OUT_FRACTION * side_sign(side);
    let rise = dims.height * (BELT_HEIGHT_FRACTION - HIP_HEIGHT_FRACTION);
    let forward = BELT_FORWARD_M + body::EYES_FORWARD_OF_SHOULDERS;
    waist(frame, dims, Vec3::new(out, rise, -forward))
}

/// A waist-level landmark at `offset` from the middle of the hip joints in the pelvis's frame,
/// adjusted for a seated body: higher up the torso and forward over the thighs, because the seat is
/// where a standing body's hip joints are.
fn waist(frame: &BodyFrame, dims: &BodyDimensions, offset: Vec3) -> Vec3 {
    let pelvis = pelvis_girdle(frame);
    let (hips, shoulders_up) = match &frame.anchor {
        Anchor::Body { .. } => (
            frame.origin
                + pelvis
                    * Vec3::new(
                        0.0,
                        dims.height * HIP_HEIGHT_FRACTION - dims.eye_height,
                        body::EYES_FORWARD_OF_SHOULDERS,
                    ),
            dims.shoulder_height - dims.height * HIP_HEIGHT_FRACTION,
        ),
        Anchor::Skeleton(skeleton) => {
            let hips = skeleton.hip_centre();
            (hips, skeleton.chest().y - hips.y)
        }
    };
    let standing = hips + pelvis * offset;
    if !frame.seated {
        return standing;
    }
    let torso = (shoulders_up - offset.y).max(0.0);
    standing + pelvis * Vec3::new(0.0, torso * SEATED_HIP_RISE_FRACTION, -SEATED_HIP_FORWARD_M)
}

/// The distance between the shoulder joints of the body the volumes ride.
fn shoulder_width(frame: &BodyFrame, dims: &BodyDimensions) -> f32 {
    match &frame.anchor {
        Anchor::Body { .. } => dims.shoulder_width,
        Anchor::Skeleton(skeleton) => skeleton.shoulders[0].distance(skeleton.shoulders[1]),
    }
}

/// The pelvis's orientation: the body's heading, or the line through the skeleton's hip joints.
fn pelvis_girdle(frame: &BodyFrame) -> Quat {
    match &frame.anchor {
        Anchor::Body { yaw } => Quat::from_rotation_y(*yaw),
        Anchor::Skeleton(skeleton) => girdle_basis(skeleton.hips),
    }
}

/// The shoulder joints and the chest's orientation.
struct Chest {
    shoulders: [Vec3; 2],
    basis: Quat,
}

impl Chest {
    fn centre(&self) -> Vec3 {
        (self.shoulders[0] + self.shoulders[1]) * 0.5
    }
}

/// The chest of the body the volumes ride: the shoulder joints estimated from the dimensions at
/// the body's heading, or the skeleton's, oriented by the line through them.
fn chest_girdle(frame: &BodyFrame, dims: &BodyDimensions) -> Chest {
    match &frame.anchor {
        Anchor::Body { yaw } => {
            let basis = Quat::from_rotation_y(*yaw);
            Chest {
                shoulders: body::shoulder_positions(frame.origin, basis, dims),
                basis,
            }
        }
        Anchor::Skeleton(skeleton) => Chest {
            shoulders: skeleton.shoulders,
            basis: girdle_basis(skeleton.shoulders),
        },
    }
}

/// The orientation of a girdle through `[left, right]`: +X along the line from left to right on
/// the ground, +Y up, and -Z forward. A degenerate line keeps the cockpit frame's axes.
fn girdle_basis([left, right]: [Vec3; 2]) -> Quat {
    let line = right - left;
    let Some(right_axis) = Vec3::new(line.x, 0.0, line.z).try_normalize() else {
        return Quat::IDENTITY;
    };
    let back = right_axis.cross(Vec3::Y);
    Quat::from_mat3(&Mat3::from_cols(right_axis, Vec3::Y, back)).normalize()
}

/// A volume at `spec`'s offset from `landmark` in the frame `basis`, mirrored for the left side.
fn place(
    volume: GrabVolume,
    landmark: Vec3,
    basis: Quat,
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
        centre: landmark + basis * offset,
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
        weapons::{config::C4Config, reload::config::ReloadConfig},
    };

    const DIMS: BodyDimensions = BodyDimensions::GENERIC_ADULT;
    const STANDING: BodyFrame = BodyFrame {
        origin: Vec3::ZERO,
        seated: false,
        anchor: Anchor::Body { yaw: 0.0 },
    };
    const SEATED: BodyFrame = BodyFrame {
        origin: Vec3::ZERO,
        seated: true,
        anchor: Anchor::Body { yaw: 0.0 },
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
        let magazine = ReloadConfig::new().pouch;
        for frame in [STANDING, SEATED] {
            for dims in [
                BodyDimensions::GENERIC_ADULT,
                BodyDimensions::CHARACTER_ESTIMATE,
            ] {
                let mut c = holster_candidates(&frame, &dims, &config, Hand::Right, true);
                c.push(magazine_pouch(
                    &frame,
                    &dims,
                    &magazine,
                    Hand::Right,
                    Some(Hand::Left),
                ));
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
    fn the_reload_pouch_is_on_the_dominant_side_for_the_free_hand_only() {
        let spec = ReloadConfig::new().pouch;
        for frame in [STANDING, SEATED] {
            let pouch = magazine_pouch(&frame, &DIMS, &spec, Hand::Right, Some(Hand::Left));
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
    fn the_reload_pouch_keeps_clear_of_the_hip_holsters() {
        // A belt reload pushes a gun into the pouch with its grip held, and a hip holster acts on
        // a grip press; a gap of half the pouch's radius keeps a hand at one from brushing the
        // other.
        let spec = ReloadConfig::new().pouch;
        for dims in [
            BodyDimensions::GENERIC_ADULT,
            BodyDimensions::CHARACTER_ESTIMATE,
        ] {
            for frame in [STANDING, SEATED] {
                for dominant in Hand::BOTH {
                    let c =
                        holster_candidates(&frame, &dims, &HolstersConfig::new(), dominant, false);
                    let pouch = magazine_pouch(&frame, &dims, &spec, dominant, None);
                    for hip in Hand::BOTH {
                        let hip = centre(&c, GrabVolume::Holster(HolsterSite::Hip(hip)));
                        let radius = HolstersConfig::new().hip.radius_m;
                        assert!(
                            pouch.centre.distance(hip) >= radius + 1.5 * pouch.radius,
                            "the pouch at {:?} is too close to the hip holster at {hip:?}",
                            pouch.centre
                        );
                    }
                }
            }
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
                    &ReloadConfig::new().pouch,
                    dominant,
                    Some(dominant.other()),
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
        let upright = Anchor::Body { yaw: 0.0 };
        let origin = |head, stance| body_frame(head, stance, 0.15, upright).origin;
        for stance in [Stance::Standing, Stance::Seated] {
            assert_eq!(origin(Vec3::new(0.3, -0.1, 0.2), stance), Vec3::ZERO);
            assert_eq!(origin(Vec3::new(0.0, 0.2, 0.0), stance), Vec3::ZERO);
            let crouched = origin(Vec3::new(0.0, -0.5, 0.0), stance);
            assert!((crouched.y + 0.35).abs() < 1e-6);
            assert_eq!(origin(Vec3::NAN, stance), Vec3::ZERO);
        }
        assert!(body_frame(Vec3::ZERO, Stance::Seated, 0.15, upright).seated);
        assert!(!body_frame(Vec3::ZERO, Stance::Standing, 0.15, upright).seated);
        assert_eq!(
            body_frame(
                Vec3::ZERO,
                Stance::Standing,
                0.15,
                Anchor::Body { yaw: f32::NAN }
            )
            .anchor,
            upright
        );
    }

    /// A skeleton built to the dimensions, with its eyes at the origin, turned by `yaw`.
    fn ideal_skeleton(dims: &BodyDimensions, yaw: f32) -> SkeletonLandmarks {
        let turn = Quat::from_rotation_y(yaw);
        let shoulders = body::shoulder_positions(Vec3::ZERO, turn, dims);
        let pelvis = turn
            * Vec3::new(
                0.0,
                dims.height * HIP_HEIGHT_FRACTION - dims.eye_height,
                body::EYES_FORWARD_OF_SHOULDERS,
            );
        let half = turn * Vec3::new(dims.shoulder_width * 0.22, 0.0, 0.0);
        SkeletonLandmarks {
            head: Vec3::ZERO,
            pelvis,
            hips: [pelvis - half, pelvis + half],
            shoulders,
        }
    }

    /// Every volume, holsters and pouches, for `frame`.
    fn every_volume(frame: &BodyFrame, dims: &BodyDimensions) -> Vec<Candidate> {
        let mut c = holster_candidates(frame, dims, &HolstersConfig::new(), Hand::Right, true);
        c.push(magazine_pouch(
            frame,
            dims,
            &ReloadConfig::new().pouch,
            Hand::Right,
            Some(Hand::Left),
        ));
        c.push(c4_pouch(frame, dims, &C4Config::new().pouch, Hand::Right));
        c
    }

    #[test]
    fn a_skeleton_built_to_the_dimensions_places_the_volumes_as_the_body_does() {
        for dims in [
            BodyDimensions::GENERIC_ADULT,
            BodyDimensions::CHARACTER_ESTIMATE,
        ] {
            for seated in [false, true] {
                let body = BodyFrame { seated, ..STANDING };
                let rico = BodyFrame {
                    anchor: Anchor::Skeleton(ideal_skeleton(&dims, 0.0)),
                    ..body
                };
                for (a, b) in every_volume(&body, &dims)
                    .iter()
                    .zip(every_volume(&rico, &dims))
                {
                    assert_eq!(a.volume, b.volume);
                    assert!(
                        a.centre.distance(b.centre) < 1e-4,
                        "{:?}: {} vs {} (seated {seated})",
                        a.volume,
                        a.centre,
                        b.centre
                    );
                }
            }
        }
    }

    #[test]
    fn the_volumes_turn_with_the_torso_about_the_eyes() {
        let yaw = 1.2;
        let turn = Quat::from_rotation_y(yaw);
        for seated in [false, true] {
            let ahead = every_volume(&BodyFrame { seated, ..STANDING }, &DIMS);
            let turned_body = BodyFrame {
                seated,
                anchor: Anchor::Body { yaw },
                ..STANDING
            };
            let turned_rico = BodyFrame {
                seated,
                anchor: Anchor::Skeleton(ideal_skeleton(&DIMS, yaw)),
                ..STANDING
            };
            for frame in [turned_body, turned_rico] {
                for (a, b) in ahead.iter().zip(every_volume(&frame, &DIMS)) {
                    assert!(
                        (turn * a.centre).distance(b.centre) < 1e-4,
                        "{:?} under {}",
                        a.volume,
                        frame.anchor.label()
                    );
                }
            }
        }
    }

    #[test]
    fn the_hip_volumes_ride_the_pelvis_and_the_shoulder_volumes_ride_the_chest() {
        let mut skeleton = ideal_skeleton(&DIMS, 0.0);
        let before = every_volume(
            &BodyFrame {
                anchor: Anchor::Skeleton(skeleton),
                ..STANDING
            },
            &DIMS,
        );
        // The chest turns 30 degrees over a still pelvis.
        let chest = skeleton.chest();
        let twist = Quat::from_rotation_y(0.5);
        skeleton.shoulders = skeleton.shoulders.map(|s| chest + twist * (s - chest));
        let after = every_volume(
            &BodyFrame {
                anchor: Anchor::Skeleton(skeleton),
                ..STANDING
            },
            &DIMS,
        );
        for (a, b) in before.iter().zip(&after) {
            let moved = a.centre.distance(b.centre) > 1e-3;
            let on_chest = matches!(
                a.volume,
                GrabVolume::Holster(HolsterSite::Shoulder(_) | HolsterSite::Chest)
                    | GrabVolume::C4Pouch
            );
            assert_eq!(moved, on_chest, "{:?}", a.volume);
        }
    }

    #[test]
    fn the_volumes_on_the_skeleton_do_not_overlap() {
        for dims in [
            BodyDimensions::GENERIC_ADULT,
            BodyDimensions::CHARACTER_ESTIMATE,
        ] {
            for seated in [false, true] {
                let frame = BodyFrame {
                    seated,
                    anchor: Anchor::Skeleton(ideal_skeleton(&dims, -0.6)),
                    ..STANDING
                };
                let c = every_volume(&frame, &dims);
                for (i, a) in c.iter().enumerate() {
                    for b in &c[i + 1..] {
                        assert!(
                            a.centre.distance(b.centre) > a.radius + b.radius,
                            "{:?} and {:?} overlap",
                            a.volume,
                            b.volume
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_degenerate_girdle_keeps_the_cockpit_axes() {
        assert_eq!(girdle_basis([Vec3::ZERO, Vec3::Y]), Quat::IDENTITY);
        let basis = girdle_basis([Vec3::new(-0.2, 0.0, 0.0), Vec3::new(0.2, 0.1, 0.0)]);
        assert!((basis * Vec3::NEG_Z).distance(Vec3::NEG_Z) < 1e-5);
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
