//! Placing the body's grab volumes: the holsters, the grenade pouch, the magazine pouch, and the C4
//! pouch. Pure.
//!
//! The volumes are placed in the cockpit frame relative to the body frame. The cockpit frame
//! composes onto the character's world transform with the character's animated head at its origin
//! (see `headpose::xr`), so in the cockpit frame the body faces the frame's own axes (+X right, +Y
//! up, -Z forward) with the eyes at the origin. The volumes therefore stay on the body when the
//! player turns or leans their head to look at them, and move with the body when a stick turn or the
//! roomscale chase moves it.
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
//! - the chest, at the middle of the shoulder joints, for the chest holster;
//! - the chest's front on each side, below and inside the shoulder joint, for the grenade and C4
//!   pouches ([`chest_sides`] says which pouch takes which side);
//! - the belt's front on the side of the hand without a gun, for the magazine pouch: above the hip
//!   joints, a few centimetres from the midline, and a few centimetres in front of the eyes' vertical
//!   line, where the belly is; seated, adjusted like the hips.
//!
//! The pouches' sides are laid out around the hand that holds the guns ([`gun_side`]), not the
//! dominant hand, because the gun can ride the other hand.
//!
//! The landmarks of the chest's and the belt's front are on the body's surface, and each pouch's
//! offset carries it out in front of the surface, where a hand laid on the body closes.
//!
//! The body the volumes ride is one of two ([`BodyFrame`]):
//!
//! - The player's body, estimated from the player's own dimensions, turned to the player's
//!   estimated torso heading ([`crate::controllers::body::torso`]) so that the volumes stay at the
//!   player's hips, belt, chest, and shoulders when the player turns in the room. The caller tests
//!   these volumes against the player's own hands, before the body-scale remap, so a hand put on
//!   the player's own belt finds the pouch there whatever the character's proportions.
//! - The character's skeleton ([`HolsterAnchor::Rico`](crate::controllers::weapons::config::HolsterAnchor::Rico)):
//!   the same landmarks taken from his hip and shoulder joints carried into the cockpit frame, so
//!   the volumes sit on the body the player sees. The hip and belt volumes ride the pelvis (the
//!   line through the hip joints) and the shoulder, chest, and chest-pouch volumes ride the chest
//!   (the line through the shoulder joints). For a skeleton built to the dimensions the two
//!   coincide. Where the skeleton is unavailable or not upright (a vehicle, a ragdoll, a skydive)
//!   the caller falls back to the player's body.
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
    weapons::config::{HolstersConfig, PouchSide, PouchesConfig, VolumeSpec},
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

/// The holster volumes for a body in `frame` with dimensions `dims`, in the order the sites are
/// listed. Every holster may be grabbed with either hand.
pub fn holster_candidates(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    config: &HolstersConfig,
) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(5);
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
    out
}

/// The sides of the chest the two chest pouches take, by the body's own sides.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ChestSides {
    pub grenade: Hand,
    pub c4: Hand,
}

/// The chest's sides for a player whose guns ride `gun_side` (see [`gun_side`]): the grenade pouch
/// on the side `pouches.grenade_side` names, and the C4 pouch on the other, so the two never share
/// a side.
pub fn chest_sides(pouches: &PouchesConfig, gun_side: Hand) -> ChestSides {
    let grenade = match pouches.grenade_side {
        PouchSide::Dominant => gun_side,
        PouchSide::OffHand => gun_side.other(),
    };
    ChestSides {
        grenade,
        c4: grenade.other(),
    }
}

/// The hand the body's pouches are laid out around: the hand holding a gun, `holder`, or with none
/// drawn, the hand that last held one, `last_holder`, or with none yet, the dominant hand. A
/// single gun stays on the hand attach bone the game gives it whichever hand is dominant, so a
/// left-handed player can hold the gun in the right hand, and a layout keyed off the dominant hand
/// would then put the magazine pouch on the gun's side, out of the free hand's reach. Remembering
/// the last holder keeps the pouches from changing sides each time a gun is holstered.
pub fn gun_side(holder: Option<Hand>, last_holder: Option<Hand>, dominant: Hand) -> Hand {
    holder.or(last_holder).unwrap_or(dominant)
}

/// The hands that may take from the chest pouches.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ChestGrabbers {
    Either,
    Only(Hand),
    /// No hand is free: the pouches are not placed.
    Neither,
}

impl ChestGrabbers {
    /// The hands that may take from the chest pouches while `gun_hands` (by [`Hand::index`]) hold
    /// guns: either hand with no gun drawn, the free hand while one hand holds a gun, and neither
    /// while both do. A gun hand that took a grenade or a charge would prime it in the gun's hand.
    pub fn for_guns(gun_hands: [bool; 2]) -> Self {
        match gun_hands {
            [false, false] => ChestGrabbers::Either,
            [true, true] => ChestGrabbers::Neither,
            [true, false] => ChestGrabbers::Only(Hand::Right),
            [false, true] => ChestGrabbers::Only(Hand::Left),
        }
    }

    /// The candidate's hand, or `None` for a pouch that is not placed.
    fn hand(self) -> Option<Option<Hand>> {
        match self {
            ChestGrabbers::Either => Some(None),
            ChestGrabbers::Only(hand) => Some(Some(hand)),
            ChestGrabbers::Neither => None,
        }
    }
}

/// The grenade pouch on the front of the chest, a volume for `grabbers` (whichever grabs it
/// throws), or `None` while no hand may take from it.
pub fn grenade_pouch(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    pouches: &PouchesConfig,
    gun_side: Hand,
    grabbers: ChestGrabbers,
) -> Option<Candidate> {
    let side = chest_sides(pouches, gun_side).grenade;
    let pouch = chest_pouch(
        GrabVolume::GrenadePouch,
        frame,
        dims,
        &pouches.grenade,
        side,
    );
    grabbers.hand().map(|hand| Candidate { hand, ..pouch })
}

/// The C4 pouch on the front of the chest, on the side the grenade pouch leaves, a volume for
/// `grabbers` (whichever grabs it throws), or `None` while no hand may take from it.
pub fn c4_pouch(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    pouches: &PouchesConfig,
    gun_side: Hand,
    grabbers: ChestGrabbers,
) -> Option<Candidate> {
    let side = chest_sides(pouches, gun_side).c4;
    let pouch = chest_pouch(GrabVolume::C4Pouch, frame, dims, &pouches.c4, side);
    grabbers.hand().map(|hand| Candidate { hand, ..pouch })
}

/// The universal reload pouch at the front of the belt, a volume for `grabber`, the hand without a
/// gun (see [`crate::controllers::weapons::reload`]), on that hand's side; with no hand free, on the
/// side away from the guns, `gun_side`. A gun's well pushed onto it reloads in one hand, so it is
/// placed whether or not a hand is free.
pub fn magazine_pouch(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    pouches: &PouchesConfig,
    gun_side: Hand,
    grabber: Option<Hand>,
) -> Candidate {
    let side = grabber.unwrap_or(gun_side.other());
    place(
        GrabVolume::MagazinePouch,
        belt(frame, dims, side),
        pelvis_girdle(frame),
        &pouches.magazine,
        side,
        grabber,
    )
}

/// Whether `volume` is one of the pouches.
pub fn is_pouch(volume: GrabVolume) -> bool {
    matches!(
        volume,
        GrabVolume::GrenadePouch | GrabVolume::MagazinePouch | GrabVolume::C4Pouch
    )
}

/// The grab volume `hand` is in among `candidates`, each tested against its own point of the hand,
/// `point_for`, as [`crate::controllers::bindings::grab::grab_volume_by`] finds it, except that
/// every other volume wins over a holster that also contains its point. A press meant for a pouch
/// or a weapon's volume that also lands in a holster's edge must not draw or switch a weapon; a
/// hand that means the holster is deep in it, clear of the pouches. Among the rest, the hand is in
/// the one it is deepest in.
pub fn grab_volume_by(
    hand: Hand,
    candidates: &[Candidate],
    point_for: impl Fn(&Candidate) -> Vec3,
) -> Option<GrabVolume> {
    candidates
        .iter()
        .filter(|c| c.hand.is_none_or(|h| h == hand))
        .filter_map(|c| {
            let holster = matches!(c.volume, GrabVolume::Holster(_));
            c.depth(point_for(c))
                .map(|depth| (holster, depth, c.volume))
        })
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(_, _, volume)| volume)
}

/// The pouch `hand` reaches for at `point` though the point is outside it: the pouch nearest the
/// point among those `hand` may grab, when the point is in no volume at all and within `margin` of
/// that pouch's surface. A grip press there takes from the pouch rather than doing the grip's own
/// action (the off-hand's grapple), because a hand feeling for a pouch on the body lands a little
/// off it more often than not.
pub fn pouch_in_reach(
    point: Vec3,
    hand: Hand,
    candidates: &[Candidate],
    margin: f32,
) -> Option<GrabVolume> {
    if !point.is_finite() || !margin.is_finite() || margin <= 0.0 {
        return None;
    }
    let grabbable =
        |c: &&Candidate| c.hand.is_none_or(|h| h == hand) && c.radius.is_finite() && c.radius > 0.0;
    if candidates
        .iter()
        .filter(grabbable)
        .any(|c| c.depth(point).is_some())
    {
        return None;
    }
    candidates
        .iter()
        .filter(grabbable)
        .filter(|c| is_pouch(c.volume))
        .map(|c| (c.volume, point.distance(c.centre) - c.radius))
        .filter(|(_, outside)| *outside <= margin)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(volume, _)| volume)
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
        (matches!(c.volume, GrabVolume::Holster(_)) || is_pouch(c.volume))
            && c.hand.is_none_or(|h| h == hand)
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
const SEATED_HIP_RISE_FRACTION: f32 = 0.2;

/// How far forward of the hip joints a seated player's hands reach for the hips, over the thighs,
/// metres. A best guess.
const SEATED_HIP_FORWARD_M: f32 = 0.15;

/// How far forward of the shoulder joints the shoulder volumes sit for a seated body, metres: over
/// the tops of the shoulders rather than behind them, where a backrest is. A best guess.
const SEATED_SHOULDER_FORWARD_M: f32 = 0.05;

/// The height of the belt's front, where the magazine pouch hangs, as a fraction of the standing
/// height: the waist, a little above the hip joints. A generic anthropometric figure.
const BELT_HEIGHT_FRACTION: f32 = 0.58;

/// How far out from the midline the belt's front landmark sits, as a fraction of the shoulder
/// width: toward the middle of the belt, well inside the hip holster on that side, so that a hand
/// feeling for the pouch does not land in the holster.
const BELT_OUT_FRACTION: f32 = 0.12;

/// How far in front of the eyes' vertical line the front of the belly is, metres. A generic
/// figure: standing upright, the belly's front is about under the eyes.
const BELLY_FRONT_M: f32 = 0.03;

/// How far below the shoulder joints the chest pouches' landmark sits, as a fraction of the
/// standing height: the upper chest.
const CHEST_POUCH_DROP_FRACTION: f32 = 0.065;

/// How far out from the midline the chest pouches' landmark sits, as a fraction of the shoulder
/// width: about under the shoulder joint, clear of the chest holster in the middle.
const CHEST_POUCH_OUT_FRACTION: f32 = 0.52;

/// How far in front of the eyes' vertical line the front of the chest is, metres. A generic
/// figure: the chest is a little deeper than the belly.
const CHEST_FRONT_M: f32 = 0.02;

/// The hip landmark on `side`: as far out as the shoulder joint; standing, at the hip joints' height
/// and depth; seated, higher up the torso and forward over the thighs.
fn hip(frame: &BodyFrame, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = shoulder_width(frame, dims) * 0.5 * side_sign(side);
    waist(frame, dims, Vec3::new(out, 0.0, 0.0))
}

/// The belt's front landmark on `side`: on the belly's surface at the belt's height, at the front
/// of the hip on that side. Standing and seated as [`hip`].
fn belt(frame: &BodyFrame, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = shoulder_width(frame, dims) * BELT_OUT_FRACTION * side_sign(side);
    let rise = dims.height * (BELT_HEIGHT_FRACTION - HIP_HEIGHT_FRACTION);
    let forward = BELLY_FRONT_M + body::EYES_FORWARD_OF_SHOULDERS;
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

/// The chest pouch `volume` on `side`: on the chest's surface below and inside the shoulder joint,
/// riding the chest.
fn chest_pouch(
    volume: GrabVolume,
    frame: &BodyFrame,
    dims: &BodyDimensions,
    spec: &VolumeSpec,
    side: Hand,
) -> Candidate {
    let chest = chest_girdle(frame, dims);
    let out = shoulder_width(frame, dims) * CHEST_POUCH_OUT_FRACTION * side_sign(side);
    let landmark = chest.centre()
        + chest.basis
            * Vec3::new(
                out,
                -dims.height * CHEST_POUCH_DROP_FRACTION,
                -(CHEST_FRONT_M + body::EYES_FORWARD_OF_SHOULDERS),
            );
    place(volume, landmark, chest.basis, spec, side, None)
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
mod tests;
