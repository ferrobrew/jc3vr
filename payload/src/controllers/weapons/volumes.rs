//! Placing the body's grab volumes: the holsters, the grenade pouch, the magazine pouch, and the C4
//! pouch. Pure.
//!
//! The volumes ride the character's skeleton, the body the player sees, carried into the cockpit
//! frame: what the player sees on the body is what they reach for, and the hands reach it as they
//! are drawn. The hip and belt volumes ride the pelvis (the line through the hip joints) and the
//! shoulder, chest, and chest-pouch volumes ride the chest (the line through the shoulder joints),
//! both turned up along the spine ([`BodyFrame`]), so the volumes follow the body through a crouch,
//! a lean, a fall, or the parachute as the animation and the procedural body pose it. Lengths that
//! scale with the body are fractions of the skeleton's own torso.
//!
//! Each volume sits at an offset from a landmark of the skeleton:
//!
//! - the hips, level with the shoulder joints' width, at the hip joints;
//! - the shoulder joints;
//! - the chest, at the middle of the shoulder joints, for the chest holster;
//! - the chest's front on each side, below and inside the shoulder joint, for the grenade and C4
//!   pouches ([`chest_sides`] says which pouch takes which side);
//! - the belt's front on the side of the hand without a gun, for the magazine pouch: above the hip
//!   joints, a few centimetres from the midline, in front of the belly.
//!
//! The pouches' sides are laid out around the hand that holds the guns ([`gun_side`]), not the
//! dominant hand, because the gun can ride the other hand.
//!
//! The landmarks of the chest's and the belt's front are on the body's surface, and each pouch's
//! offset carries it out in front of the surface, where a hand laid on the body closes.

use glam::{Mat3, Quat, Vec3};

use crate::controllers::{
    body::{self, rig::frames::SkeletonLandmarks},
    frame::Hand,
    grab::{Candidate, GrabVolume, HolsterSite},
    weapons::config::{HolstersConfig, PouchSide, PouchesConfig, VolumeSpec},
};

/// The body the volumes ride: the character's skeleton in the cockpit frame, with its girdles'
/// orientations.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    pub skeleton: SkeletonLandmarks,
    /// The pelvis: +X along the line from the left hip joint to the right, +Y up the spine, and -Z
    /// forward.
    pub pelvis: Quat,
    /// The chest, as the pelvis but along the line through the shoulder joints.
    pub chest: Quat,
}

impl BodyFrame {
    /// The frame of `skeleton` (in the cockpit frame), or `None` for a skeleton whose spine or
    /// girdles are degenerate.
    pub fn on(skeleton: SkeletonLandmarks) -> Option<Self> {
        if !skeleton.is_finite() {
            return None;
        }
        let spine = skeleton.chest() - skeleton.hip_centre();
        Some(Self {
            skeleton,
            pelvis: girdle_basis(skeleton.hips, spine)?,
            chest: girdle_basis(skeleton.shoulders, spine)?,
        })
    }

    /// The length of the torso, from the middle of the hip joints to the middle of the shoulder
    /// joints.
    fn torso(&self) -> f32 {
        self.skeleton.chest().distance(self.skeleton.hip_centre())
    }

    fn shoulder_width(&self) -> f32 {
        self.skeleton.shoulders[0].distance(self.skeleton.shoulders[1])
    }
}

/// The holster volumes on the body `frame`, in the order the sites are listed. Every holster may
/// be grabbed with either hand.
pub fn holster_candidates(frame: &BodyFrame, config: &HolstersConfig) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(5);
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Hip(side)),
            hip(frame, side),
            frame.pelvis,
            &config.hip,
            side,
            None,
        ));
    }
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Shoulder(side)),
            frame.skeleton.shoulders[side.index()],
            frame.chest,
            &config.shoulder,
            side,
            None,
        ));
    }
    out.push(place(
        GrabVolume::Holster(HolsterSite::Chest),
        frame.skeleton.chest(),
        frame.chest,
        &config.chest,
        Hand::Right,
        None,
    ));
    out
}

/// Keep the long guns' draws for the dominant hand while neither hand holds a weapon (`empty`):
/// the shoulder holsters and the long guns docked on them are offered to the dominant hand alone,
/// so a long gun drawn by reaching with the off-hand does not move the layout that follows the gun
/// hand. With a weapon in a hand, either hand may take one, as a second weapon or to holster it.
pub fn reserve_long_guns(candidates: &mut Vec<Candidate>, dominant: Hand, empty: bool) {
    if !empty {
        return;
    }
    candidates.retain_mut(|c| {
        if !matches!(c.volume, GrabVolume::Holster(HolsterSite::Shoulder(_))) {
            return true;
        }
        match c.hand {
            None => {
                c.hand = Some(dominant);
                true
            }
            Some(hand) => hand == dominant,
        }
    });
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
/// drawn, the hand that last held one, `last_holder`, or with none yet, the dominant hand. A gun
/// may be in either hand whichever hand is dominant ([`crate::controllers::weapons::hold`]), and a
/// layout keyed off the dominant hand would then put the magazine pouch on the gun's side, out of
/// the free hand's reach. Remembering the last holder keeps the pouches from changing sides each
/// time a gun is holstered.
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
    pouches: &PouchesConfig,
    gun_side: Hand,
    grabbers: ChestGrabbers,
) -> Option<Candidate> {
    let side = chest_sides(pouches, gun_side).grenade;
    let pouch = chest_pouch(GrabVolume::GrenadePouch, frame, &pouches.grenade, side);
    grabbers.hand().map(|hand| Candidate { hand, ..pouch })
}

/// The C4 pouch on the front of the chest, on the side the grenade pouch leaves, a volume for
/// `grabbers` (whichever grabs it throws), or `None` while no hand may take from it.
pub fn c4_pouch(
    frame: &BodyFrame,
    pouches: &PouchesConfig,
    gun_side: Hand,
    grabbers: ChestGrabbers,
) -> Option<Candidate> {
    let side = chest_sides(pouches, gun_side).c4;
    let pouch = chest_pouch(GrabVolume::C4Pouch, frame, &pouches.c4, side);
    grabbers.hand().map(|hand| Candidate { hand, ..pouch })
}

/// The universal reload pouch at the front of the belt, a volume for `grabber`, the hand without a
/// gun (see [`crate::controllers::weapons::reload`]), on that hand's side; with no hand free, on the
/// side away from the guns, `gun_side`. A gun's well pushed onto it reloads in one hand, so it is
/// placed whether or not a hand is free.
pub fn magazine_pouch(
    frame: &BodyFrame,
    pouches: &PouchesConfig,
    gun_side: Hand,
    grabber: Option<Hand>,
) -> Candidate {
    let side = grabber.unwrap_or(gun_side.other());
    place(
        GrabVolume::MagazinePouch,
        belt(frame, side),
        frame.pelvis,
        &pouches.magazine,
        side,
        grabber,
    )
}

/// The standing height of the hip joints as a fraction of the standing height: a generic
/// anthropometric figure, which the body dimensions do not carry because it varies little between
/// adults.
pub const HIP_HEIGHT_FRACTION: f32 = 0.53;

/// How far above the hip joints the belt's front is, where the magazine pouch hangs, as a fraction
/// of the torso: the waist.
const BELT_RISE_FRACTION: f32 = 0.17;

/// How far out from the midline the belt's front landmark sits, as a fraction of the shoulder
/// width: toward the middle of the belt, well inside the hip holster on that side, so that a hand
/// feeling for the pouch does not land in the holster.
const BELT_OUT_FRACTION: f32 = 0.12;

/// How far in front of the hip joints' line the front of the belly is, metres.
const BELLY_FRONT_M: f32 = 0.03 + body::EYES_FORWARD_OF_SHOULDERS;

/// How far below the shoulder joints the chest pouches' landmark sits, as a fraction of the torso:
/// the upper chest.
const CHEST_POUCH_DROP_FRACTION: f32 = 0.22;

/// How far out from the midline the chest pouches' landmark sits, as a fraction of the shoulder
/// width: about under the shoulder joint, clear of the chest holster in the middle.
const CHEST_POUCH_OUT_FRACTION: f32 = 0.52;

/// How far in front of the shoulder joints' line the front of the chest is, metres: a little deeper
/// than the belly.
const CHEST_FRONT_M: f32 = 0.02 + body::EYES_FORWARD_OF_SHOULDERS;

/// The hip landmark on `side`: as far out as the shoulder joint, at the hip joints.
fn hip(frame: &BodyFrame, side: Hand) -> Vec3 {
    let out = frame.shoulder_width() * 0.5 * side_sign(side);
    frame.skeleton.hip_centre() + frame.pelvis * Vec3::new(out, 0.0, 0.0)
}

/// The belt's front landmark on `side`: on the belly's surface at the belt's height, at the front
/// of the hip on that side.
fn belt(frame: &BodyFrame, side: Hand) -> Vec3 {
    let out = frame.shoulder_width() * BELT_OUT_FRACTION * side_sign(side);
    let rise = frame.torso() * BELT_RISE_FRACTION;
    frame.skeleton.hip_centre() + frame.pelvis * Vec3::new(out, rise, -BELLY_FRONT_M)
}

/// The chest pouch `volume` on `side`: on the chest's surface below and inside the shoulder joint,
/// riding the chest.
fn chest_pouch(volume: GrabVolume, frame: &BodyFrame, spec: &VolumeSpec, side: Hand) -> Candidate {
    let out = frame.shoulder_width() * CHEST_POUCH_OUT_FRACTION * side_sign(side);
    let landmark = frame.skeleton.chest()
        + frame.chest
            * Vec3::new(
                out,
                -frame.torso() * CHEST_POUCH_DROP_FRACTION,
                -CHEST_FRONT_M,
            );
    place(volume, landmark, frame.chest, spec, side, None)
}

/// The orientation of a girdle through `[left, right]` on a body whose spine runs along `spine`:
/// +X along the line from left to right, +Y up the spine (square to the line), and -Z forward.
/// `None` for a degenerate line or spine, or one along the other.
fn girdle_basis([left, right]: [Vec3; 2], spine: Vec3) -> Option<Quat> {
    let right_axis = (right - left).try_normalize()?;
    let up = spine.reject_from_normalized(right_axis).try_normalize()?;
    let back = right_axis.cross(up);
    let basis = Quat::from_mat3(&Mat3::from_cols(right_axis, up, back)).normalize();
    basis.is_finite().then_some(basis)
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
