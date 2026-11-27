//! What the weapon handling publishes for the HUD to draw: each hand's gun for the ammunition
//! counter ([`hand_ammo`]), each hand's grab hint ([`grab_hint`]), and the weapons lying in the
//! world ([`lost_weapons`]), with the weapon icons they name ([`IconId`]).
//!
//! The input tick writes each sample on the game thread, and the HUD reads the latest one when it
//! draws. The HUD owns the drawing and everything it reads from the renderer; these modules hold
//! only the data and its slot.

pub mod grab_hint;
pub mod hand_ammo;
pub mod lost_weapons;

use jc3gi::equipment::weapon::WeaponBase;

/// A weapon icon the HUD's quads show, by the weapon's id ([`WeaponBase::m_WeaponId`]).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct IconId(pub i16);

impl IconId {
    /// The icon of `weapon`; none for a weapon without an id.
    pub(crate) fn of(weapon: &WeaponBase) -> Option<Self> {
        (weapon.m_WeaponId >= 0).then_some(Self(weapon.m_WeaponId))
    }
}
