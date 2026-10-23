#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The mission manager (`NMissionSystem::CMissionManager`): the active mission, its restart
/// points, and the checkpoint cache of the player's weapons. Only some members are bound.
pub struct MissionManager {
    _field_0: [u8; 832],
    /// The player's slots as of the last mission checkpoint, one [`WeaponCacheEntry`](crate::graph_script::WeaponCacheEntry) per slot,
    /// written by [`CacheWeapons`](crate::mission::MissionManager::CacheWeapons) and replayed by
    /// [`RestoreCachedWeapons`](crate::mission::MissionManager::RestoreCachedWeapons).
    pub m_CachedWeapons: crate::types::std_vector::Vector<
        crate::graph_script::WeaponCacheEntry,
    >,
}
fn _MissionManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x360], MissionManager>([0u8; 0x360]);
    }
    unreachable!()
}
impl MissionManager {
    pub const CacheWeapons_ADDRESS: usize = 0x140AC7DD0;
    /// Snapshots the player's eight slots into [`m_CachedWeapons`](crate::mission::MissionManager::m_CachedWeapons): each
    /// slot's spawn tag index, the reserve of its ammunition type, and its magazine, a mounted
    /// weapon counting as empty. It removes nothing. Its one caller is
    /// [`SaveRestartPointToMap`](crate::mission::MissionManager::SaveRestartPointToMap), while a mission is active.
    pub unsafe fn CacheWeapons(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::CacheWeapons_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RestoreCachedWeapons_ADDRESS: usize = 0x140AC7FE0;
    /// Replays [`m_CachedWeapons`](crate::mission::MissionManager::m_CachedWeapons) onto the player's inventory, with the
    /// same per-slot rules as the graph script's cache
    /// ([`NodeWeaponCache::RestoreWeapons`](crate::graph_script::NodeWeaponCache::RestoreWeapons)), but for
    /// every slot: the reserve of each occupied slot's type is zeroed, a weapon of the cached type
    /// gets the cached reserve and magazine, another type or an empty slot gets a give
    /// ([`Inventory::GivePlayerWeapon`](crate::character::inventory::Inventory::GivePlayerWeapon)), and a
    /// cached empty slot with a weapon is marked for removal
    /// ([`Inventory::ForceRemoveWeapon`](crate::character::inventory::Inventory::ForceRemoveWeapon)). Its
    /// one caller is [`GoToRestartPoint`](crate::mission::MissionManager::GoToRestartPoint), after the player's respawn,
    /// while a mission is active.
    pub unsafe fn RestoreCachedWeapons(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RestoreCachedWeapons_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SaveRestartPointToMap_ADDRESS: usize = 0x140ACE700;
    /// Saves a restart point `id` into the variable map `map`; while a mission is active it also
    /// snapshots the player's weapons ([`CacheWeapons`](crate::mission::MissionManager::CacheWeapons)). Every mission
    /// checkpoint passes through it.
    pub unsafe fn SaveRestartPointToMap(
        &mut self,
        id: u64,
        map: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u64,
                map: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::SaveRestartPointToMap_ADDRESS);
            f(self as *mut Self as _, id, map)
        }
    }
    pub const GoToRestartPoint_ADDRESS: usize = 0x140ACEFE0;
    /// Restarts from the restart point `restart_point`: respawns the player there
    /// ([`Player::ReInit`](crate::character::player::Player::ReInit), which teleports with the load state)
    /// and, while a mission is active, replays the checkpoint's weapons
    /// ([`RestoreCachedWeapons`](crate::mission::MissionManager::RestoreCachedWeapons)). The player's death, a mission's
    /// reset and failure, the teleport to a mission's start, and the return to a challenge's
    /// start call it.
    pub unsafe fn GoToRestartPoint(&mut self, restart_point: *const ::std::ffi::c_void) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                restart_point: *const ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::GoToRestartPoint_ADDRESS);
            f(self as *mut Self as _, restart_point)
        }
    }
}
impl std::convert::AsRef<MissionManager> for MissionManager {
    fn as_ref(&self) -> &MissionManager {
        self
    }
}
impl std::convert::AsMut<MissionManager> for MissionManager {
    fn as_mut(&mut self) -> &mut MissionManager {
        self
    }
}
#[repr(C, align(8))]
/// The challenge manager (`CROMManager`), which starts, retries, restarts, and aborts the
/// challenges (ROMs). Its layout is unmapped.
pub struct ROMManager {}
impl ROMManager {
    pub const StartROM_ADDRESS: usize = 0x140AC9DE0;
    /// Starts the challenge `id`. When the challenge's spawn point overrides the loadout it records
    /// each slot's type and total ammunition, marks every slot for removal
    /// ([`Inventory::ForceRemoveAllWeapons`](crate::character::inventory::Inventory::ForceRemoveAllWeapons)),
    /// and gives the spawn point's weapons
    /// ([`Inventory::GivePlayerWeapon`](crate::character::inventory::Inventory::GivePlayerWeapon), equipped
    /// for the dual-wield and two-handed slots, dropping the occupants). It returns whether the
    /// challenge started. Starting teleports with the load state, so it passes through the spawn
    /// reset.
    pub unsafe fn StartROM(&mut self, id: u64) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, id: u64) -> bool = ::std::mem::transmute(
                Self::StartROM_ADDRESS,
            );
            f(self as *mut Self as _, id)
        }
    }
    pub const AbortROM_ADDRESS: usize = 0x140A8DF40;
    /// Aborts the running challenge: with a recorded loadout it marks every slot for removal and
    /// gives the recorded weapons back the way [`StartROM`](crate::mission::ROMManager::StartROM) gave the challenge's.
    pub unsafe fn AbortROM(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::AbortROM_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<ROMManager> for ROMManager {
    fn as_ref(&self) -> &ROMManager {
        self
    }
}
impl std::convert::AsMut<ROMManager> for ROMManager {
    fn as_mut(&mut self) -> &mut ROMManager {
        self
    }
}
#[repr(C, align(8))]
/// A challenge object (`CROMObject`). Its layout is unmapped.
pub struct ROMObject {}
impl ROMObject {
    pub const PostInit_ADDRESS: usize = 0x140ACAE10;
    /// The challenge object's post-initialisation (a virtual override). Unless restarting or
    /// retrying, it records every slot's type and magazine and every reserve; then it despawns at
    /// once ([`Inventory::DespawnWeapon`](crate::character::inventory::Inventory::DespawnWeapon)) each slot
    /// the challenge's loadout permission does not allow (the grenade and planted-explosive slots,
    /// and each of the dual-wield, two-handed, and special slots whose profile item fails the
    /// mask), and gives the weapons the player picked in the challenge's loadout menu.
    pub unsafe fn PostInit(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::PostInit_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RestoreWeaponsAndAmmo_ADDRESS: usize = 0x140AC8840;
    /// Restores the loadout recorded by [`PostInit`](crate::mission::ROMObject::PostInit), called at the challenge's end
    /// and on the object's exit: per slot, a recorded empty slot is despawned
    /// ([`Inventory::DespawnWeapon`](crate::character::inventory::Inventory::DespawnWeapon)) and any other
    /// gets a give of the recorded type and magazine; then every reserve but the beacons' and
    /// flares' is restored, and an autosave is requested.
    pub unsafe fn RestoreWeaponsAndAmmo() {
        unsafe {
            let f: unsafe extern "system" fn() = ::std::mem::transmute(
                Self::RestoreWeaponsAndAmmo_ADDRESS,
            );
            f()
        }
    }
}
impl std::convert::AsRef<ROMObject> for ROMObject {
    fn as_ref(&self) -> &ROMObject {
        self
    }
}
impl std::convert::AsMut<ROMObject> for ROMObject {
    fn as_mut(&mut self) -> &mut ROMObject {
        self
    }
}
