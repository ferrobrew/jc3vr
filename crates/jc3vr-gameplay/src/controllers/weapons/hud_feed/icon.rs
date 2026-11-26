//! The weapon icons the HUD's quads show, named by weapon id.

use jc3gi::equipment::weapon::WeaponBase;

/// A weapon icon, by the weapon's id ([`WeaponBase::m_WeaponId`]).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct IconId(pub i16);

impl IconId {
    /// The icon of `weapon`; none for a weapon without an id.
    pub(crate) fn of(weapon: &WeaponBase) -> Option<Self> {
        (weapon.m_WeaponId >= 0).then_some(Self(weapon.m_WeaponId))
    }
}
