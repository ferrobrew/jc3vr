#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The player's part of a save (`SPlayerSaveData`): a reference to the player whose state it
/// stores and restores.
pub struct PlayerSaveData {
    pub m_Player: *mut crate::character::player::Player,
}
fn _PlayerSaveData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], PlayerSaveData>([0u8; 0x8]);
    }
    unreachable!()
}
impl PlayerSaveData {}
impl std::convert::AsRef<PlayerSaveData> for PlayerSaveData {
    fn as_ref(&self) -> &PlayerSaveData {
        self
    }
}
impl std::convert::AsMut<PlayerSaveData> for PlayerSaveData {
    fn as_mut(&mut self) -> &mut PlayerSaveData {
        self
    }
}
pub const StorePlayerSaveData_ADDRESS: usize = 0x140CADC80;
/// Stores the player's save data into a binary output archive
/// (`ArStore<NAr::CBinaryOutputArchive<NSaveLoad::CUserDataStream>>(const SPlayerSaveData&)`),
/// from the player's character's inventory: the spawn tag hash of each slot's weapon
/// ([`SpawnSystem::GetTagHashFromTagIndex`](crate::spawn_system::SpawnSystem::GetTagHashFromTagIndex)),
/// zero for an empty slot and always zero for the
/// [`E_SLOT_HEAVY`](crate::character::inventory::WeaponSlot::E_SLOT_HEAVY) slot; every reserve
/// ([`Inventory::GetAmmoCount`](crate::character::inventory::Inventory::GetAmmoCount)); the grappling
/// device's tag hash; the player's upgrade name; two bavarium upgrade flags; and the downloadable
/// content bits. No magazine, selected slot, or other per-weapon state is stored. The archive's
/// store of a player save data calls it.
pub unsafe fn StorePlayerSaveData(
    save_data: *const crate::save::PlayerSaveData,
    archive: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            save_data: *const crate::save::PlayerSaveData,
            archive: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(StorePlayerSaveData_ADDRESS);
        f(save_data, archive)
    }
}
pub const LoadPlayerSaveData_ADDRESS: usize = 0x140CA6FA0;
/// Loads the player's save data from a versioned binary input archive of `version`. While a flag
/// of the player is set, it reads the slots' tag hashes and the reserves, resets the inventory's
/// respawn table, writes each known slot type's tag index
/// ([`SpawnSystem::GetTagIndexFromTagHash`](crate::spawn_system::SpawnSystem::GetTagIndexFromTagHash))
/// into [`Inventory::m_WeaponToSpawn`](crate::character::inventory::Inventory::m_WeaponToSpawn) with a
/// full magazine to give, and sets each reserve
/// ([`Inventory::SetAmmoCount`](crate::character::inventory::Inventory::SetAmmoCount)); then the
/// grappling device, the upgrade name, and the flags. It spawns nothing: the inventory's handler
/// of the spawn system's post-reset event spawns the respawn table. The disassembler mislabels it
/// as the load-schema visitor's load.
pub unsafe fn LoadPlayerSaveData(
    save_data: *mut crate::save::PlayerSaveData,
    archive: *mut ::std::ffi::c_void,
    version: u8,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            save_data: *mut crate::save::PlayerSaveData,
            archive: *mut ::std::ffi::c_void,
            version: u8,
        ) = ::std::mem::transmute(LoadPlayerSaveData_ADDRESS);
        f(save_data, archive, version)
    }
}
