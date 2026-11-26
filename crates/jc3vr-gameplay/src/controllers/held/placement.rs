//! The rigid grip's reads of the weapon handling during the inventory's placement pass: how a gun
//! moves into the hand's grip (a snap between a socket and the hand, or a slide through the hand
//! from another part of the gun), the gun body's stray from its seat on the hand's body, the
//! weapons armed in the hands that no inventory slot holds, and the capture of each placed grip.
//!
//! The types and the pure slide math live here; the reads call through the registered
//! [`PlacementSource`](crate::controllers::held::PlacementSource).

use glam::{Quat, Vec3};
use jc3gi::{
    character::{character::SafeBoneIndex, inventory::Inventory},
    equipment::weapon::WeaponBase,
};
use jc3vr_input::frame::{Hand, Pose};

use crate::controllers::held::source;

/// A snap as the rigid grip sees it: a gun moving between where it was and the hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SnapView {
    pub start: SnapStart,
    /// How far the gun is from the far end toward the hand, `0..=1`.
    pub toward_hand: f32,
}

/// The far end of a snap, as the rigid grip places it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SnapStart {
    /// The gun's pose `offset` on the dock bone `dock`, model space.
    Dock { dock: SafeBoneIndex, offset: Pose },
    /// A pose in the world.
    World(Pose),
}

/// A gun on its way onto a hand's canonical grip from where another part of it sat in the palm:
/// the bracing hand's slide onto the grip when the holding hand lets go.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripSlide {
    /// The point of the gun in the palm now, in the gun's frame: the origin once the slide is over.
    pub anchor: Vec3,
    /// The gun's rotation on the hand's wrist when the slide began.
    pub turn: Quat,
    /// How far the gun has turned from `turn` onto the canonical grip, from 0 to 1.
    pub aligned: f32,
}

/// `slide` with its turn moved from the wrist frame onto the wrist bone, for a wrist frame of
/// rotation `frame_on_bone` on the bone. A slide's turn is measured on the hand-tracking wrist
/// frame ([`crate::controllers::hands::pose`]), and the rigid grip composes it after the wrist
/// bone's own frame, which the skeleton orients differently on each side.
pub fn slide_on_bone(slide: GripSlide, frame_on_bone: Quat) -> GripSlide {
    GripSlide {
        turn: (frame_on_bone * slide.turn).normalize(),
        ..slide
    }
}

/// The pose that carries a gun held at its canonical grip to the same gun held as `slide` has it,
/// in the attach frame, for a hand whose grip puts the attach frame at `grip` on its wrist: the gun
/// turned from its slide's turn toward the canonical grip by the share it has aligned, and moved so
/// that the anchor sits where the grip's attach point sits, in the palm. The rigid grip composes it
/// after the grip's pose on the wrist; a finished slide gives the identity.
pub fn slide_offset(slide: &GripSlide, grip: Pose) -> Pose {
    let start = (grip.orientation.inverse() * slide.turn).normalize();
    let orientation = start
        .slerp(Quat::IDENTITY, slide.aligned.clamp(0.0, 1.0))
        .normalize();
    Pose {
        position: -(orientation * slide.anchor),
        orientation,
    }
}

/// The primary gun and its solved wrist immediately after native attachment placement.
pub struct HandlePlacement {
    pub generation: u64,
    pub weapon: usize,
    pub primary_gun: usize,
    pub holder: Hand,
    pub wrist_world: Pose,
    pub weight: f32,
    pub pinned: bool,
}

/// Forget the grips captured by the last placement and return the new placement's generation.
#[inline]
pub fn begin_handle_placement() -> u64 {
    (source().placement.begin)()
}

/// Capture the primary grip and wrist from the same native placement.
///
/// # Safety
///
/// The weapon and primary gun must remain live in the local character's post-placement pass.
/// `wrist_world` is the solved wrist read in that pass, not a later IK target.
#[inline]
pub unsafe fn record_handle_placement(input: HandlePlacement) {
    unsafe { (source().placement.record)(input) }
}

/// The snap of `weapon`'s gun in `hand`.
#[inline]
pub fn snap(weapon: usize, hand: Hand) -> Option<SnapView> {
    (source().placement.snap)(weapon, hand)
}

/// Where `weapon` is on its way onto `hand`'s grip, sampled now.
#[inline]
pub fn slide(weapon: usize, hand: Hand) -> Option<GripSlide> {
    (source().placement.slide)(weapon, hand)
}

/// How far the gun body of `weapon` in `hand` strays from its seat on the hand's body, in the
/// gun's own frame; `None` without a joined gun body for that weapon in that hand.
#[inline]
pub fn body_deviation(hand: Hand, weapon: usize) -> Option<Pose> {
    (source().placement.body_deviation)(hand, weapon)
}

/// The weapons armed in the hands that the carry set keeps rather than `inventory`, when
/// `inventory` is the local player's.
///
/// # Safety
///
/// The inventory must belong to the live local owner. Its update phase must retain the carry-set
/// sources without replacement or removal for the returned borrow.
#[inline]
pub unsafe fn carried_for(inventory: &Inventory) -> Vec<&WeaponBase> {
    unsafe { (source().placement.carried_for)(inventory) }
}
