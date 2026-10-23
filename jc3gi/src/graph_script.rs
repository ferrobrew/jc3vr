#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A graph script's weapon cache (`NGSONodes::CNodeWeaponCache`): the weapons a mission takes from
/// a character and later gives back, one [`WeaponCacheEntry`](crate::graph_script::WeaponCacheEntry) per inventory slot. The
/// `SpawnWeapons` node ([`NGSONodes_SpawnWeapons`](crate::graph_script::NGSONodes_SpawnWeapons)) fills it and gives the weapons back through
/// [`RestoreWeapons`](crate::graph_script::NodeWeaponCache::RestoreWeapons).
pub struct NodeWeaponCache {
    /// The cached slots.
    pub m_Entries: crate::types::std_vector::Vector<
        crate::graph_script::WeaponCacheEntry,
    >,
}
fn _NodeWeaponCache_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], NodeWeaponCache>([0u8; 0x20]);
    }
    unreachable!()
}
impl NodeWeaponCache {
    pub const CacheAllWeaponSlots_ADDRESS: usize = 0x140A4F0B0;
    /// Records every slot of `inventory` (`CNodeWeaponCache::CacheAllWeaponSlots`): for each of
    /// the eight slots an entry with the slot, the weapon's spawn tag index
    /// ([`GameObject::GetUniqueSpawnTagIndex`](crate::game::GameObject::GetUniqueSpawnTagIndex), or zero
    /// for an empty slot), the reserve of its ammunition kind
    /// ([`Inventory::GetAmmoCount`](crate::character::inventory::Inventory::GetAmmoCount)), and its
    /// magazine. With `remove` set, each slot's weapon is then forced out of the inventory. Its one
    /// caller is [`NGSONodes_SpawnWeapons`](crate::graph_script::NGSONodes_SpawnWeapons), which removes only for its remove-all type.
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
    pub const RestoreWeapons_ADDRESS: usize = 0x140A44880;
    /// Gives the cached slots back to `inventory` (`CNodeWeaponCache::RestoreWeapons`). For each
    /// entry but the heavy slot's: the slot's current weapon's reserve is zeroed; a weapon of the
    /// cached type keeps its object and gets the cached reserve and magazine; another type or an
    /// empty slot gets a give
    /// ([`Inventory::GivePlayerWeapon`](crate::character::inventory::Inventory::GivePlayerWeapon), with the
    /// reserve and the magazine as its rounds, not equipped, with extra ammunition to the reserve,
    /// and despawning the occupant); and a cached empty slot that now holds a weapon is marked for
    /// removal ([`Inventory::ForceRemoveWeapon`](crate::character::inventory::Inventory::ForceRemoveWeapon)),
    /// except the planted-explosive slot. The disassembler mislabels it `CNodeWeaponCache::Get`,
    /// and gives this name to a player-save-data store helper.
    pub unsafe fn RestoreWeapons(
        &mut self,
        inventory: *mut crate::character::inventory::Inventory,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                inventory: *mut crate::character::inventory::Inventory,
            ) = ::std::mem::transmute(Self::RestoreWeapons_ADDRESS);
            f(self as *mut Self as _, inventory)
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
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// One slot's record in a weapon cache: the slot, the weapon's spawn tag index
/// ([`GameObject::GetUniqueSpawnTagIndex`](crate::game::GameObject::GetUniqueSpawnTagIndex), or zero for
/// an empty slot), the reserve of its ammunition kind, and its magazine. The graph script's
/// [`NodeWeaponCache`](crate::graph_script::NodeWeaponCache) and the mission manager's checkpoint cache
/// ([`MissionManager::m_CachedWeapons`](crate::mission::MissionManager::m_CachedWeapons)) keep these.
pub struct WeaponCacheEntry {
    pub m_Slot: crate::character::inventory::WeaponSlot,
    pub m_SpawnTagIndex: i32,
    pub m_Ammunition: i32,
    pub m_MagazineAmmo: i32,
}
fn _WeaponCacheEntry_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], WeaponCacheEntry>([0u8; 0x10]);
    }
    unreachable!()
}
impl WeaponCacheEntry {}
impl std::convert::AsRef<WeaponCacheEntry> for WeaponCacheEntry {
    fn as_ref(&self) -> &WeaponCacheEntry {
        self
    }
}
impl std::convert::AsMut<WeaponCacheEntry> for WeaponCacheEntry {
    fn as_mut(&mut self) -> &mut WeaponCacheEntry {
        self
    }
}
pub const NGSONodes_SpawnWeapons_ADDRESS: usize = 0x140A31700;
/// The graph script node `SpawnWeapons` (`NGSONodes::SpawnWeapons`), which strips, gives, caches,
/// and restores a character's weapons; `processor` is the graph script processor, `node` the node,
/// and `input_pin` the pin that fired, and it returns the node result. Its fields are a removal
/// type (only replace, remove where the field is empty, or remove all and replace), a weapon tag
/// and ammunition for each of the dual-wield, two-handed, special, hand-grenade, and
/// planted-explosive slots, the equipped weapon, and the cache.
///
/// On its main input it clears the cache, records the slots
/// ([`NodeWeaponCache::CacheAllWeaponSlots`](crate::graph_script::NodeWeaponCache::CacheAllWeaponSlots), removing them for the remove-all type), and returns
/// the player to normal aim; then per field it marks the slot for removal
/// ([`Inventory::ForceRemoveWeapon`](crate::character::inventory::Inventory::ForceRemoveWeapon)) for an
/// empty field under remove-where-empty, or under remove-all; keeps a weapon of the same type,
/// setting its reserve and cancelling its removal
/// ([`Inventory::CancelRemoveWeapon`](crate::character::inventory::Inventory::CancelRemoveWeapon)); and
/// otherwise gives the field's weapon
/// ([`Inventory::GivePlayerWeapon`](crate::character::inventory::Inventory::GivePlayerWeapon), equipped for
/// the equipped field's slot). With no equipped field it unequips the wielded weapon
/// ([`WeaponBase::UnequipDeferred`](crate::equipment::weapon::WeaponBase::UnequipDeferred)) and relaxes
/// the character; with an equipped field and nothing to give it selects it
/// ([`Character::SelectWeapon`](crate::character::character::Character::SelectWeapon)). A second pin only
/// caches; a third restores ([`NodeWeaponCache::RestoreWeapons`](crate::graph_script::NodeWeaponCache::RestoreWeapons), after a return to normal aim) and
/// refreshes the HUD's weapons; the update pin waits until no slot's spawn is pending.
pub unsafe fn NGSONodes_SpawnWeapons(
    processor: *mut ::std::ffi::c_void,
    node: *const ::std::ffi::c_void,
    input_pin: u32,
) -> u32 {
    unsafe {
        let f: unsafe extern "system" fn(
            processor: *mut ::std::ffi::c_void,
            node: *const ::std::ffi::c_void,
            input_pin: u32,
        ) -> u32 = ::std::mem::transmute(NGSONodes_SpawnWeapons_ADDRESS);
        f(processor, node, input_pin)
    }
}
