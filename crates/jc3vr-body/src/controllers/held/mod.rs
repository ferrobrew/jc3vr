//! What each hand holds: the armed weapon per hand, the kind of item in it, the gun a hand holds
//! by the weapon handling, the state of a held gun's slide and of a rack's pistols, and how a gun
//! moves into the hand's grip. The weapon handling owns all of it; this module is the seam through
//! which the systems below the weapon handling (the hands, and the aim) read it.
//!
//! The weapon handling registers one [`HeldSource`] at install ([`register_held_source`]), a table
//! of its own query functions, and every read here calls through it. The reads are live: each one
//! reads the weapon handling's state at the moment of the call, exactly as a direct call into the
//! weapon handling would, so a reader on the render thread or inside the inventory's placement
//! pass sees the same values it always did. A per-tick copy would not do: the slide and snap
//! progress are sampled at the read, and the render-time readers run between ticks.
//!
//! Reading before the source is registered is a programming error and panics; the controllers'
//! install registers it before any hook is enabled.
//!
//! - [`armed_in`], [`armed_hand_of`]: the weapon armed in a hand, by the hand or by the weapon;
//! - [`armed_weapon`]: the weapon armed in a hand, as the native object a character holds;
//! - [`owned_holder`]: the hand holding a weapon while the mod owns its attach bones;
//! - [`holder`]: the hand a weapon is in by either of the two;
//! - [`item`]: the kind of item the loadout table has in a hand ([`HeldItem`]);
//! - [`wielded_gun_hands`]: which hands hold a gun of the wielded weapon;
//! - [`detonator_hand`]: the hand the C4 detonate act draws the detonator in;
//! - [`is_single_pistol`]: whether a weapon is one pistol split off the dual-wield pair;
//! - [`rack_stack_offset`], [`adjust_part`]: a rack pair's second pistol drawn apart, and a held
//!   gun's slide and magazine parts as the reload moves them;
//! - [`placement`]: the rigid grip's reads during the inventory's placement pass: a snap moving a
//!   gun between a socket and the hand ([`SnapView`]), a slide through the hand onto the grip
//!   ([`GripSlide`]), the gun body's stray from its seat, and the record of the placed grip;
//! - [`native`]: the native weapon object's accessors both sides read.

use std::sync::OnceLock;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::Inventory},
    equipment::weapon::{ModelAttachementWeaponComponent, WeaponBase},
};
use jc3vr_input::frame::{Hand, Pose};

pub mod native;
pub mod placement;

pub use placement::{GripSlide, HandlePlacement, SnapStart, SnapView, slide_offset, slide_on_bone};

/// The kind of item the loadout table has in a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeldItem {
    /// A weapon.
    Weapon,
    /// A loose object a hand picked up.
    Prop,
    /// A hand grenade.
    Grenade,
    /// A C4 charge.
    Charge,
}

/// The weapon handling's query functions behind every read of this module. Weapons are named by
/// the address of their native object, as the weapon handling names them.
#[derive(Clone, Copy)]
pub struct HeldSource {
    /// The weapon armed in a hand.
    pub armed_in: fn(Hand) -> Option<usize>,
    /// The hand a weapon is armed in.
    pub armed_hand_of: fn(usize) -> Option<Hand>,
    /// The weapon armed in a hand, when the character's inventory or the carry set holds it.
    ///
    /// # Safety
    ///
    /// As [`armed_weapon`].
    pub armed_weapon: for<'a> unsafe fn(&'a Character, Hand) -> Option<&'a WeaponBase>,
    /// The hand holding a weapon, while the mod owns its attach bones.
    pub owned_holder: fn(usize) -> Option<Hand>,
    /// The kind of item the loadout table has in a hand.
    pub item: fn(Hand) -> Option<HeldItem>,
    /// Which hands hold a gun of the wielded weapon, as of the weapon handling's last tick, by
    /// [`Hand::index`]; neither while the weapon handling is inactive.
    pub wielded_gun_hands: fn() -> [bool; 2],
    /// The hand the C4 detonate act draws a weapon's detonator in, while the act plays.
    pub detonator_hand: fn(usize) -> Option<Hand>,
    /// Whether a weapon is a single pistol: one pistol of the dual-wield pair split off to be held
    /// on its own, which shows and fires only its first gun.
    pub is_single_pistol: fn(usize) -> bool,
    /// The world-space offset a finite rack source's second pistol is drawn at while neither of
    /// the pair is claimed.
    pub rack_stack_offset: fn(usize) -> Option<Vec3>,
    /// The matrix to push for a part of a weapon in place of the corrected one, at a render
    /// fraction: a slide held back or worked, or a magazine part in the hand. `None` leaves it.
    pub adjust_part: fn(&ModelAttachementWeaponComponent, usize, Mat4, f32) -> Option<Mat4>,
    /// The placement pass's reads ([`placement`]).
    pub placement: PlacementSource,
}

/// The weapon handling's side of the rigid grip's placement pass ([`placement`]).
#[derive(Clone, Copy)]
pub struct PlacementSource {
    /// Forget the grips captured by the last placement and return the new placement's generation.
    pub begin: fn() -> u64,
    /// Capture a placed gun's primary grip with the wrist solved in the same placement.
    ///
    /// # Safety
    ///
    /// As [`placement::record_handle_placement`].
    pub record: unsafe fn(HandlePlacement),
    /// The snap of a weapon's gun in a hand.
    pub snap: fn(usize, Hand) -> Option<SnapView>,
    /// Where a weapon is on its way onto a hand's grip, sampled now.
    pub slide: fn(usize, Hand) -> Option<GripSlide>,
    /// How far the gun body of a weapon in a hand strays from its seat on the hand's body, in the
    /// gun's own frame; `None` without a joined gun body.
    pub body_deviation: fn(Hand, usize) -> Option<Pose>,
    /// The weapons armed in the hands that the carry set keeps rather than an inventory.
    ///
    /// # Safety
    ///
    /// As [`placement::carried_for`].
    pub carried_for: for<'a> unsafe fn(&'a Inventory) -> Vec<&'a WeaponBase>,
}

/// Register the weapon handling's source; the first registration stands.
pub fn register_held_source(source: HeldSource) {
    if SOURCE.set(source).is_err() {
        tracing::warn!("held: a held-item source is already registered; the new one is ignored");
    }
}

/// Whether the weapon handling has registered its source.
pub fn is_registered() -> bool {
    SOURCE.get().is_some()
}

/// The weapon armed in `hand`.
#[inline]
pub fn armed_in(hand: Hand) -> Option<usize> {
    (source().armed_in)(hand)
}

/// The hand the weapon at `address` is armed in.
#[inline]
pub fn armed_hand_of(address: usize) -> Option<Hand> {
    (source().armed_hand_of)(address)
}

/// The weapon armed in `hand`, when `owner`'s inventory or the carry set holds it.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update or
/// on the thread updating it. The phase must retain the slot or carry-set source without native
/// mutation for the returned shared borrow.
#[inline]
pub unsafe fn armed_weapon(owner: &Character, hand: Hand) -> Option<&WeaponBase> {
    unsafe { (source().armed_weapon)(owner, hand) }
}

/// The hand holding the weapon at `address`, while the mod owns its attach bones.
#[inline]
pub fn owned_holder(address: usize) -> Option<Hand> {
    (source().owned_holder)(address)
}

/// The hand the weapon at `address` is in: the hand holding it while the mod owns its attach
/// bones, else the hand it is armed in.
pub fn holder(address: usize) -> Option<Hand> {
    owned_holder(address).or_else(|| armed_hand_of(address))
}

/// The kind of item the loadout table has in `hand`.
#[inline]
pub fn item(hand: Hand) -> Option<HeldItem> {
    (source().item)(hand)
}

/// Which hands hold a gun of the wielded weapon, as of the weapon handling's last tick, by
/// [`Hand::index`].
#[inline]
pub fn wielded_gun_hands() -> [bool; 2] {
    (source().wielded_gun_hands)()
}

/// The hand the C4 detonate act draws the detonator of the weapon at `address` in, while the act
/// plays.
#[inline]
pub fn detonator_hand(address: usize) -> Option<Hand> {
    (source().detonator_hand)(address)
}

/// Whether the weapon at `address` is a single pistol, split off the dual-wield pair.
#[inline]
pub fn is_single_pistol(address: usize) -> bool {
    (source().is_single_pistol)(address)
}

/// The world-space offset the second pistol of the finite rack source at `address` is drawn at
/// while neither of the pair is claimed.
#[inline]
pub fn rack_stack_offset(address: usize) -> Option<Vec3> {
    (source().rack_stack_offset)(address)
}

/// The matrix to push for `part` of the weapon at `weapon` in place of `matrix`, at the render
/// fraction `dtf`: a slide held back or worked, or a magazine part in the hand. `None` leaves the
/// part as it is.
#[inline]
pub fn adjust_part(
    part: &ModelAttachementWeaponComponent,
    weapon: usize,
    matrix: Mat4,
    dtf: f32,
) -> Option<Mat4> {
    (source().adjust_part)(part, weapon, matrix, dtf)
}

fn source() -> &'static HeldSource {
    SOURCE
        .get()
        .expect("held: no held-item source is registered; the controllers' install registers it")
}

static SOURCE: OnceLock<HeldSource> = OnceLock::new();
