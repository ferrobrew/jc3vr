#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A graph script's weapon cache (`NGSONodes::CNodeWeaponCache`): the weapons a mission takes from
/// a character and later gives back. It keeps one entry per inventory slot (the slot, the weapon's
/// spawn tag index, the reserve of its ammunition kind, and its magazine). Opaque. No function of
/// its own gives the weapons back: the cache node's lookup and the mission manager's restore of
/// cached weapons both call the inventory's weapon gift and its forced removal directly.
pub struct NodeWeaponCache {}
impl NodeWeaponCache {
    pub const CacheAllWeaponSlots_ADDRESS: usize = 0x140A4F0B0;
    /// Records every slot of `inventory` (`CNodeWeaponCache::CacheAllWeaponSlots`): for each of
    /// the eight slots an entry with the slot, the weapon's spawn tag index
    /// ([`GameObject::GetUniqueSpawnTagIndex`](crate::game::GameObject::GetUniqueSpawnTagIndex), or zero
    /// for an empty slot), the reserve of its ammunition kind
    /// ([`Inventory::GetAmmoCount`](crate::character::inventory::Inventory::GetAmmoCount)), and its
    /// magazine. With `remove` set, each slot's weapon is then forced out of the inventory.
    pub unsafe fn CacheAllWeaponSlots(
        &mut self,
        inventory: *mut crate::character::inventory::Inventory,
        remove: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                inventory: *mut crate::character::inventory::Inventory,
                remove: bool,
            ) = ::std::mem::transmute(Self::CacheAllWeaponSlots_ADDRESS);
            f(self as *mut Self as _, inventory, remove)
        }
    }
}
impl std::convert::AsRef<NodeWeaponCache> for NodeWeaponCache {
    fn as_ref(&self) -> &NodeWeaponCache {
        self
    }
}
impl std::convert::AsMut<NodeWeaponCache> for NodeWeaponCache {
    fn as_mut(&mut self) -> &mut NodeWeaponCache {
        self
    }
}
