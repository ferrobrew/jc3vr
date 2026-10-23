#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How [`Inventory::AddWeapon`](crate::character::inventory::Inventory::AddWeapon) equips the weapon it adds (`CInventory::EEquipMode`).
pub enum EquipMode {
    E_DONT_EQUIP = 0isize as _,
    E_EQUIP = 1isize as _,
    E_EQUIP_IF_IN_SELECTED_SLOT = 2isize as _,
}
fn _EquipMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], EquipMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The character's equipment inventory (`CInventory`), embedded in `CCharacter`
/// ([`Character::m_Inventory`](crate::character::character::Character::m_Inventory)). Only some members
/// are mapped.
pub struct Inventory {
    _field_0: [u8; 32],
    /// The weapon in each slot, indexed by [`WeaponSlot`](crate::character::inventory::WeaponSlot) (up to
    /// [`E_NUM_WEAPON_SLOTS`](crate::character::inventory::WeaponSlot::E_NUM_WEAPON_SLOTS)); a null pointer for an empty slot.
    /// [`Character::GetWeaponInInventory`](crate::character::character::Character::GetWeaponInInventory) returns a reference to an entry.
    pub m_Weapons: [crate::types::shared_ptr::SharedPtr<
        crate::equipment::weapon::WeaponBase,
    >; 8],
    _field_a0: [u8; 32],
    /// The character's grappling hook, or null before the inventory is populated.
    /// `CCharacter::GetGrapplingHook` returns a reference to this field.
    pub m_GrapplingHook: crate::types::shared_ptr::SharedPtr<
        crate::equipment::grappling_hook::GrapplingHook,
    >,
    _field_d0: [u8; 24],
    /// The respawn table's weapon type for each slot (`m_WeaponToSpawn`), indexed by
    /// [`WeaponSlot`](crate::character::inventory::WeaponSlot): a spawn tag index
    /// ([`GameObject::GetUniqueSpawnTagIndex`](crate::game::GameObject::GetUniqueSpawnTagIndex)), or zero
    /// for none. The spawn system's pre-reset event
    /// ([`m_SpawnSystemPreReset`](crate::character::inventory::Inventory::m_SpawnSystemPreReset)) records each occupied slot's
    /// weapon type here when the slot has no entry yet, the player save data's load
    /// ([`LoadPlayerSaveData`](crate::save::LoadPlayerSaveData)) writes the saved types here, and the
    /// post-reset event ([`m_SpawnSystemPostReset`](crate::character::inventory::Inventory::m_SpawnSystemPostReset)) requests a spawn
    /// of each entry; each spawned weapon replaces the slot's occupant through the character's
    /// pickup. [`GivePlayerWeapon`](crate::character::inventory::Inventory::GivePlayerWeapon) writes an entry and requests its spawn,
    /// and [`WeaponSpawned`](crate::character::inventory::Inventory::WeaponSpawned) clears it once the weapon arrives.
    pub m_WeaponToSpawn: [i32; 8],
    /// The loadout the pre-reset event copies into [`m_WeaponToSpawn`](crate::character::inventory::Inventory::m_WeaponToSpawn) in
    /// place of the slots' current weapons when
    /// [`RESPAWN_RESET_LOADOUT`](crate::character::inventory::SpawnSystemResetFlags::RESPAWN_RESET_LOADOUT) is set: a spawn tag
    /// index per slot, or zero.
    pub m_WeaponToSpawnReset: [i32; 8],
    /// The magazine each respawn-table entry arrives with.
    pub m_WeaponToSpawnAmmunition: [i32; 8],
    /// Per respawn-table entry, whether the arriving weapon's add drops the slot's occupant into
    /// the world rather than despawning it ([`AddWeapon`](crate::character::inventory::Inventory::AddWeapon)).
    pub m_WeaponToSpawnDropOld: [bool; 8],
    /// The slot whose arriving respawn-table weapon is equipped
    /// ([`E_EQUIP`](crate::character::inventory::EquipMode::E_EQUIP)), or [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE), in which case an
    /// arriving weapon is equipped only into the selected slot
    /// ([`E_EQUIP_IF_IN_SELECTED_SLOT`](crate::character::inventory::EquipMode::E_EQUIP_IF_IN_SELECTED_SLOT)). An arriving
    /// weapon of any other slot is not equipped ([`E_DONT_EQUIP`](crate::character::inventory::EquipMode::E_DONT_EQUIP)), and
    /// the arrival of the named slot's weapon sets it back to [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE)
    /// ([`WeaponSpawned`](crate::character::inventory::Inventory::WeaponSpawned)). The pre-reset event records the selected slot
    /// here.
    pub m_WeaponToSpawnEquippedWeapon: crate::character::inventory::WeaponSlot,
    /// Per respawn-table entry, whether rounds beyond the magazine go to the reserve on arrival.
    pub m_WeaponToSpawnExtraAmmo: [bool; 8],
    /// Per slot, whether a spawn of the slot's respawn-table entry is in flight.
    /// [`RequestWeaponSpawn`](crate::character::inventory::Inventory::RequestWeaponSpawn) does nothing for a slot already pending.
    pub m_WeaponPendingSpawn: [bool; 8],
    _field_164: [u8; 24],
    /// Per slot, whether the slot's occupant is marked for removal. [`ForceRemoveWeapon`](crate::character::inventory::Inventory::ForceRemoveWeapon),
    /// [`ForceRemoveAllWeapons`](crate::character::inventory::Inventory::ForceRemoveAllWeapons), and the post-reset event set the
    /// marks; [`UpdatePrePhysics`](crate::character::inventory::Inventory::UpdatePrePhysics) carries them out through
    /// [`Internal_RemoveWeapon`](crate::character::inventory::Inventory::Internal_RemoveWeapon);
    /// [`CancelRemoveWeapon`](crate::character::inventory::Inventory::CancelRemoveWeapon) and an arriving respawn-table weapon of the
    /// expected type ([`WeaponSpawned`](crate::character::inventory::Inventory::WeaponSpawned)) clear one.
    pub m_WeaponToRemove: [bool; 8],
    /// Whether the current animation permits shooting; one half of
    /// [`GetAnimationAllowsShooting`](crate::character::inventory::Inventory::GetAnimationAllowsShooting).
    pub m_DoesAnimationAllowShooting: bool,
    /// Whether the character may reload: recomputed by
    /// [`UpdateInventoryPermissions`](crate::character::inventory::Inventory::UpdateInventoryPermissions), and cleared by the
    /// [`SB_DISABLE_RELOAD`](crate::state::StateBitFlag::SB_DISABLE_RELOAD) state bit.
    pub m_DoesAnimationAllowReloading: bool,
    /// Whether the character may use grenades: recomputed by
    /// [`UpdateInventoryPermissions`](crate::character::inventory::Inventory::UpdateInventoryPermissions) (off while planting an
    /// explosive, while a camera hint of the wielded weapon is active, while the grapple is busy, and
    /// without the grenade ability), and cleared by the
    /// [`SB_DISABLE_USE_GRENADES`](crate::state::StateBitFlag::SB_DISABLE_USE_GRENADES) state bit.
    /// [`UpdateWeapons_Parallel`](crate::character::inventory::Inventory::UpdateWeapons_Parallel) zeroes the grenade input while it
    /// is clear.
    pub m_DoesAnimationAllowGrenades: bool,
    _field_187: [u8; 1],
    /// Whether the current animation permits picking up a weapon; one half of
    /// [`GetAnimationAllowsPickup`](crate::character::inventory::Inventory::GetAnimationAllowsPickup).
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) acts on the use-item input only while
    /// it is set.
    pub m_DoesAnimationAllowPickup: bool,
    /// Whether the current animation permits switching weapons: recomputed by
    /// [`UpdateInventoryPermissions`](crate::character::inventory::Inventory::UpdateInventoryPermissions). A deferred switch, a
    /// committed slot selection
    /// ([`Character::SwitchWeapon`](crate::character::character::Character::SwitchWeapon)), and
    /// [`TryToSwitchToValidWeapon`](crate::character::inventory::Inventory::TryToSwitchToValidWeapon) all wait on it.
    pub m_DoesAnimationAllowSwitchingWeapons: bool,
    /// Whether the character may plant explosives: recomputed by
    /// [`UpdateInventoryPermissions`](crate::character::inventory::Inventory::UpdateInventoryPermissions), and off for the player
    /// without the planted-explosives ability. One half of
    /// [`GetAnimationAllowsPlantingExplosives`](crate::character::inventory::Inventory::GetAnimationAllowsPlantingExplosives).
    pub m_DoesAnimationAllowPlantingExplosives: bool,
    /// Whether the character may detonate its placed explosives: recomputed by
    /// [`UpdateInventoryPermissions`](crate::character::inventory::Inventory::UpdateInventoryPermissions), and off for the player
    /// without the planted-explosives ability. One half of
    /// [`GetAnimationAllowsTriggeringExplosives`](crate::character::inventory::Inventory::GetAnimationAllowsTriggeringExplosives).
    pub m_DoesAnimationAllowTriggeringExplosives: bool,
    _field_18c: [u8; 1],
    /// Whether the current animation permits holstering the wielded weapon; one half of
    /// [`GetAnimationAllowsHolstering`](crate::character::inventory::Inventory::GetAnimationAllowsHolstering).
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) holsters on the holster input only
    /// while it is set, and defers the holster otherwise.
    pub m_DoesAnimationAllowHolstering: bool,
    /// Whether the current animation permits drawing a weapon; one half of
    /// [`GetAnimationAllowsDrawing`](crate::character::inventory::Inventory::GetAnimationAllowsDrawing).
    pub m_DoesAnimationAllowDrawing: bool,
    _field_18f: [u8; 3],
    /// Whether the current animation permits the weapon-selection input; one half of
    /// [`GetAnimationAllowsSwitchWeaponGUI`](crate::character::inventory::Inventory::GetAnimationAllowsSwitchWeaponGUI).
    pub m_DoesAnimationAllowSwitchWeaponGUI: bool,
    /// Whether the current animation permits selecting the planted explosive.
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) offers the
    /// [`E_SLOT_PLANTED_EXPLOSIVE`](crate::character::inventory::WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE) slot to the selection only
    /// while it is set, the slot holds a weapon, the player has the planted-explosives ability, and
    /// the owner's
    /// [`SB_DISABLE_SWITCH_TO_PLANTED_EXPLOSIVE`](crate::state::StateBitFlag::SB_DISABLE_SWITCH_TO_PLANTED_EXPLOSIVE)
    /// state bit is clear.
    pub m_DoesAnimationAllowSwitchToPlantedExplosive: bool,
    _field_194: [u8; 8],
    /// Set by a holster click in [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling): while set,
    /// the weapon selection is hidden and records no slot
    /// ([`m_SlotToSelectOnRelease`](crate::character::inventory::Inventory::m_SlotToSelectOnRelease) stays
    /// [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE)). It clears once every slot-select input is zero.
    pub m_WeaponSwitchInputConsumed: bool,
    _field_19d: [u8; 3],
    /// Seconds a pending selection has waited: while
    /// [`m_SlotToSelectOnRelease`](crate::character::inventory::Inventory::m_SlotToSelectOnRelease) is set with no select input held,
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) counts it up and shows the pending slot
    /// alone on the HUD until it passes two seconds. Zero otherwise.
    pub m_WeaponSwitchQueuedTimer: f32,
    /// The slot the last weapon cycle step (`PREV_WEAPON` / `NEXT_WEAPON`) chose.
    pub m_LastCycleSelectedWeaponSlot: crate::character::inventory::WeaponSlot,
    /// The slot a weapon cycle step queued, or [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE). Once
    /// [`m_CycleSelectedWeaponTimer`](crate::character::inventory::Inventory::m_CycleSelectedWeaponTimer) reaches half a second with
    /// no further step, it becomes [`m_SlotToSelectOnRelease`](crate::character::inventory::Inventory::m_SlotToSelectOnRelease)
    /// (or is dropped when it is the selected slot), and a held slot-select input clears it.
    pub m_QueuedCycleSelectedWeaponSlot: crate::character::inventory::WeaponSlot,
    /// Seconds since the last weapon cycle step.
    pub m_CycleSelectedWeaponTimer: f32,
    /// The character's reserve of each kind of ammunition, indexed by [`AmmunitionType`](crate::equipment::weapon::AmmunitionType) (up to
    /// [`NOF_AMMO_TYPES`](crate::equipment::weapon::AmmunitionType::NOF_AMMO_TYPES)). A weapon's magazine
    /// ([`WeaponBase::m_MagazineAmmo`](crate::equipment::weapon::WeaponBase::m_MagazineAmmo)) is separate;
    /// a reload moves rounds from here into it. Read it through
    /// [`GetAmmoCount`](crate::character::inventory::Inventory::GetAmmoCount) and write it through
    /// [`SetAmmoCount`](crate::character::inventory::Inventory::SetAmmoCount), which keeps the HUD current.
    pub m_Ammunition: [i32; 17],
    /// The cap on each entry of [`m_Ammunition`](crate::character::inventory::Inventory::m_Ammunition), indexed the same way; the
    /// ammunition upgrades raise it.
    pub m_MaxAmmunition: [i32; 17],
    _field_238: [u8; 424],
    /// The character that owns the inventory.
    pub m_Character: *mut crate::character::character::Character,
    /// The subscription to the spawn system's `spawn.reset_all_spawn_rules` event, sent at once
    /// when the spawn system resets ([`ResetSystem`](crate::spawn_system::SpawnSystem::ResetSystem)),
    /// before it releases its references to the spawned objects.
    /// [`HandleEvent`](crate::character::inventory::Inventory::HandleEvent) then records the slots' weapons into the respawn table
    /// ([`m_WeaponToSpawn`](crate::character::inventory::Inventory::m_WeaponToSpawn)) and the selected slot, and refreshes the HUD's
    /// weapon list.
    pub m_SpawnSystemPreReset: crate::event::ReceiveEvent,
    /// The subscription to the spawn system's `spawn.post_reset_all_spawn_rules` event, posted at
    /// the end of the reset and so handled later. [`HandleEvent`](crate::character::inventory::Inventory::HandleEvent) then marks
    /// every slot's occupant for removal, clears the character's next selected slot, and requests a
    /// spawn of every slot's respawn entry and of the grappling device.
    pub m_SpawnSystemPostReset: crate::event::ReceiveEvent,
    /// How the next spawn reset rebuilds the slots, read and cleared by the pre-reset event.
    /// [`Player::ReInit`](crate::character::player::Player::ReInit) writes it.
    pub m_SpawnSystemResetFlags: crate::character::inventory::SpawnSystemResetFlags,
    /// The slot the weapon selection commits when every slot-select input is released, or
    /// [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE). While a slot-select input is held,
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) rewrites it every frame with the winning
    /// slot, or with [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE) when the winner is the wielded weapon's slot or
    /// holds no weapon. On a frame with every slot-select input zero and this set, it switches to
    /// the slot ([`Character::SwitchWeapon`](crate::character::character::Character::SwitchWeapon)) when the animation allows switching, or selects and
    /// draws it with nothing wielded, and resets it to [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE); otherwise it
    /// stays set and the commit is retried every frame. Entering a vehicle seat other than a
    /// motorcycle's resets it.
    pub m_SlotToSelectOnRelease: crate::character::inventory::WeaponSlot,
    _field_3f8: [u8; 120],
    /// Under keyboard semantics, whether the planted explosive is offered to the weapon selection.
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) sets it on any frame the explosives
    /// select input reads zero; under gamepad semantics it is not consulted.
    pub m_CanSelectExplosiveOnRelease: bool,
    _field_471: [u8; 7],
}
fn _Inventory_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x478], Inventory>([0u8; 0x478]);
    }
    unreachable!()
}
impl Inventory {
    pub const GetAmmoCount_ADDRESS: usize = 0x1408BB460;
    /// The reserve of `ammo_type`: its entry of [`m_Ammunition`](crate::character::inventory::Inventory::m_Ammunition).
    pub unsafe fn GetAmmoCount(
        &self,
        ammo_type: crate::equipment::weapon::AmmunitionType,
    ) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ammo_type: crate::equipment::weapon::AmmunitionType,
            ) -> i32 = ::std::mem::transmute(Self::GetAmmoCount_ADDRESS);
            f(self as *const Self as _, ammo_type)
        }
    }
    pub const SetAmmoCount_ADDRESS: usize = 0x1408C6670;
    /// Sets the reserve of `ammo_type` to `count`, capped at its
    /// [`m_MaxAmmunition`](crate::character::inventory::Inventory::m_MaxAmmunition) entry. For the locally controlled owner it pushes
    /// the new reserve to the HUD (`CHUDUI::OnSetSelectedInventoryAmmo`) when it is the selected
    /// weapon's kind, or refreshes the grenade count for
    /// [`AMMO_HAND_GRENADE`](crate::equipment::weapon::AmmunitionType::AMMO_HAND_GRENADE); for
    /// [`AMMO_BEACON`](crate::equipment::weapon::AmmunitionType::AMMO_BEACON) and [`AMMO_FLARE`](crate::equipment::weapon::AmmunitionType::AMMO_FLARE)
    /// it sends the resupply-point refresh event and, when the count changed and `save_game` is
    /// set, autosaves.
    ///
    /// A refill from empty (the old reserve zero, `count` positive) reloads: when the wielded
    /// weapon uses `ammo_type` and its magazine is empty it calls
    /// [`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon),
    /// and for [`AMMO_HAND_GRENADE`](crate::equipment::weapon::AmmunitionType::AMMO_HAND_GRENADE) it calls the hand-grenade
    /// weapon's [`Reload`](crate::equipment::weapon::WeaponBase::Reload) directly when that weapon's
    /// magazine is empty. [`WeaponBase::Reload`](crate::equipment::weapon::WeaponBase::Reload) writes the
    /// reserve back through it.
    pub unsafe fn SetAmmoCount(
        &mut self,
        ammo_type: crate::equipment::weapon::AmmunitionType,
        count: i32,
        save_game: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ammo_type: crate::equipment::weapon::AmmunitionType,
                count: i32,
                save_game: bool,
            ) = ::std::mem::transmute(Self::SetAmmoCount_ADDRESS);
            f(self as *mut Self as _, ammo_type, count, save_game)
        }
    }
    pub const GiveMaxAmmo_ADDRESS: usize = 0x1408D5ED0;
    /// Fills every reserve to its [`m_MaxAmmunition`](crate::character::inventory::Inventory::m_MaxAmmunition) entry, with the same
    /// HUD updates, events, autosave, and refill-from-empty reloads as
    /// [`SetAmmoCount`](crate::character::inventory::Inventory::SetAmmoCount) for each kind.
    pub unsafe fn GiveMaxAmmo(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::GiveMaxAmmo_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const GetAnimationAllowsReloading_ADDRESS: usize = 0x1408BB660;
    /// Whether the character may reload:
    /// [`m_DoesAnimationAllowReloading`](crate::character::inventory::Inventory::m_DoesAnimationAllowReloading) is set and the
    /// owner's [`SB_DISABLE_RELOAD`](crate::state::StateBitFlag::SB_DISABLE_RELOAD) state bit is clear.
    /// [`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon)
    /// and the ammunition check require it.
    pub unsafe fn GetAnimationAllowsReloading(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsReloading_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const Update_Parallel_ADDRESS: usize = 0x140942920;
    /// The inventory's parallel per-frame update. `muzzle_target` and `grenade_target` point at
    /// by-value `CVector3f` copies of the owner's weapon and grenade aim targets.
    pub unsafe fn Update_Parallel(
        &mut self,
        dt: f32,
        muzzle_target: *const crate::types::math::Vector3,
        grenade_target: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                muzzle_target: *const crate::types::math::Vector3,
                grenade_target: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::Update_Parallel_ADDRESS);
            f(self as *mut Self as _, dt, muzzle_target, grenade_target)
        }
    }
    pub const UpdateWeapons_Parallel_ADDRESS: usize = 0x1408C69A0;
    /// The inventory's parallel weapon update, called from [`Update_Parallel`](crate::character::inventory::Inventory::Update_Parallel)
    /// with the same aim targets. It calls each active weapon's
    /// [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel):
    /// the mounted gun in [`E_SLOT_HEAVY`](crate::character::inventory::WeaponSlot::E_SLOT_HEAVY) while the owner is in a
    /// mounted-gun state, otherwise the boost wingsuit's guns while the owner has the boost wingsuit
    /// ([`Character::HasBoostWingsuit`](crate::character::character::Character::HasBoostWingsuit)) and flies
    /// it, or else the wielding weapon, all with `muzzle_target`, and then the selected grenade with `grenade_target`. The
    /// fire inputs passed down come from the owner's
    /// [`m_WeaponInputs`](crate::character::character::Character::m_WeaponInputs), zeroed while shooting
    /// (or throwing) is not allowed: the primary input unless
    /// [`GetAnimationAllowsShooting`](crate::character::inventory::Inventory::GetAnimationAllowsShooting) holds, and the grenade
    /// input unless [`m_DoesAnimationAllowGrenades`](crate::character::inventory::Inventory::m_DoesAnimationAllowGrenades) is set and
    /// [`SB_DISABLE_USE_GRENADES`](crate::state::StateBitFlag::SB_DISABLE_USE_GRENADES) is clear. The
    /// secondary input is always zero. For the player, a riding state
    /// ([`Character::IsInRidingInVehicleState`](crate::character::character::Character::IsInRidingInVehicleState))
    /// skips the wielding weapon unless the ride is a motorcycle, and skips the grenade in any
    /// riding state. Nothing inside a weapon reads the shooting permission, so a weapon updated
    /// directly bypasses it. The disassembler mislabels it `CGrapplingHook::GetFireInputEnabled`.
    pub unsafe fn UpdateWeapons_Parallel(
        &mut self,
        dt: f32,
        muzzle_target: *const crate::types::math::Vector3,
        grenade_target: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                muzzle_target: *const crate::types::math::Vector3,
                grenade_target: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::UpdateWeapons_Parallel_ADDRESS);
            f(self as *mut Self as _, dt, muzzle_target, grenade_target)
        }
    }
    pub const UpdateWeapons_Serial_ADDRESS: usize = 0x14091BBB0;
    /// The serial counterpart of [`UpdateWeapons_Parallel`](crate::character::inventory::Inventory::UpdateWeapons_Parallel): the same
    /// weapons with the same targets and inputs, through
    /// [`UpdateFromUserPostSim_Serial`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Serial),
    /// and additionally the planted-explosive slot's weapon.
    pub unsafe fn UpdateWeapons_Serial(
        &mut self,
        dt: f32,
        muzzle_target: *const crate::types::math::Vector3,
        grenade_target: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                muzzle_target: *const crate::types::math::Vector3,
                grenade_target: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::UpdateWeapons_Serial_ADDRESS);
            f(self as *mut Self as _, dt, muzzle_target, grenade_target)
        }
    }
    pub const GetAnimationAllowsShooting_ADDRESS: usize = 0x1408BB630;
    /// Whether the character may shoot: [`m_DoesAnimationAllowShooting`](crate::character::inventory::Inventory::m_DoesAnimationAllowShooting)
    /// is set and the owner's
    /// [`SB_DISABLE_SHOOT`](crate::state::StateBitFlag::SB_DISABLE_SHOOT) state
    /// bit is clear.
    pub unsafe fn GetAnimationAllowsShooting(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsShooting_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetAnimationAllowsPlantingExplosives_ADDRESS: usize = 0x1408BB7B0;
    /// Whether the character may plant explosives:
    /// [`m_DoesAnimationAllowPlantingExplosives`](crate::character::inventory::Inventory::m_DoesAnimationAllowPlantingExplosives) is
    /// set and the owner's
    /// [`SB_DISABLE_PLANT_EXPLOSIVES`](crate::state::StateBitFlag::SB_DISABLE_PLANT_EXPLOSIVES) state bit
    /// is clear.
    pub unsafe fn GetAnimationAllowsPlantingExplosives(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsPlantingExplosives_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetAnimationAllowsTriggeringExplosives_ADDRESS: usize = 0x1408BB7E0;
    /// Whether the character may detonate its placed explosives:
    /// [`m_DoesAnimationAllowTriggeringExplosives`](crate::character::inventory::Inventory::m_DoesAnimationAllowTriggeringExplosives)
    /// is set and the owner's
    /// [`SB_DISABLE_TRIGGER_EXPLOSIVES`](crate::state::StateBitFlag::SB_DISABLE_TRIGGER_EXPLOSIVES) state
    /// bit is clear.
    pub unsafe fn GetAnimationAllowsTriggeringExplosives(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsTriggeringExplosives_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdateInventoryPermissions_ADDRESS: usize = 0x14091B3E0;
    /// Recomputes the inventory's per-frame permissions from the owner's state (it consults the
    /// grapple reel-in predicates among others). Shooting is cleared while pre-reeling or reeling
    /// in without the shoot-while-reeling upgrade, every permission while the grappling hook holds
    /// an object, and the grenade permission and two others while the wielded weapon's
    /// [`m_PrecisionCameraHash`](crate::equipment::weapon::WeaponBase::m_PrecisionCameraHash) hint is
    /// active. It then ANDs each permission with the inverse of its disable state bit
    /// ([`SB_DISABLE_SHOOT`](crate::state::StateBitFlag::SB_DISABLE_SHOOT) and its siblings), writes the
    /// permission bytes ([`m_DoesAnimationAllowShooting`](crate::character::inventory::Inventory::m_DoesAnimationAllowShooting)
    /// onward), and clears the disable bits. The disable bits are therefore per-frame requests:
    /// the graph's `NSetInventoryPermissions` state tasks set them during the animation update
    /// ([`NSetInventoryPermissions_SetPermissions`](crate::character::inventory::NSetInventoryPermissions_SetPermissions)), and this latches and clears them.
    pub unsafe fn UpdateInventoryPermissions(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateInventoryPermissions_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateWeaponHandling_ADDRESS: usize = 0x140932F10;
    /// The inventory's per-frame weapon-handling update (`CInventory::UpdateWeaponHandling`), run
    /// from [`Update_Parallel`](crate::character::inventory::Inventory::Update_Parallel) after
    /// [`UpdateInventoryPermissions`](crate::character::inventory::Inventory::UpdateInventoryPermissions). It handles the holster
    /// input ([`Character::HolsterWeapon`](crate::character::character::Character::HolsterWeapon), or a
    /// deferred holster while the animation does not allow one), draws a weapon when weapon input
    /// arrives with nothing wielded, the precision-aim and grenade inputs, and the weapon-selection
    /// HUD. The input draws are two: with nothing wielded
    /// ([`Character::GetWieldingWeapon`](crate::character::character::Character::GetWieldingWeapon)) or a
    /// holster playing, a positive fire input
    /// ([`E_INPUT_MAIN_WEAPON`](crate::character::character::WeaponInput::E_INPUT_MAIN_WEAPON)) calls
    /// [`Character::DrawWeapon`](crate::character::character::Character::DrawWeapon); and for the player,
    /// with nothing wielded, a positive precision-aim input
    /// ([`E_INPUT_PRECISION_AIM`](crate::character::character::WeaponInput::E_INPUT_PRECISION_AIM)) that
    /// precision aim is possible for draws the selected weapon too, entering precision aim when
    /// that weapon supports it. Both read the inputs from
    /// [`Character::m_WeaponInputs`](crate::character::character::Character::m_WeaponInputs). For the player, with no weapon input and no draw or holster playing, it acts on the
    /// avatar's mental state: in [`E_COMBAT`](crate::character::avatar::MentalState::E_COMBAT) with
    /// nothing wielded it draws the selected weapon when the animation allows it (the latched
    /// permission and a clear
    /// [`SB_DISABLE_MENTAL_STATE_DRAW_WEAPON`](crate::state::StateBitFlag::SB_DISABLE_MENTAL_STATE_DRAW_WEAPON));
    /// in [`E_RELAXED`](crate::character::avatar::MentalState::E_RELAXED) with a weapon wielded, while
    /// holstering is allowed and the weapon has no deferred unequip pending, it leaves precision
    /// aim, clears the grenade and mortar permissions, and queues `UB_ACT_HOLSTER` through
    /// [`Character::QueueAct`](crate::character::character::Character::QueueAct). The relaxed branch
    /// fires every tick its conditions hold, and it is how the player's holster input takes
    /// effect, since the player's `HolsterWeapon` only forces the relaxed state.
    ///
    /// It is also where a weapon in the world is picked up. While
    /// [`GetAnimationAllowsPickup`](crate::character::inventory::Inventory::GetAnimationAllowsPickup) holds and the use-item input
    /// ([`E_INPUT_USE_ITEM`](crate::character::character::WeaponInput::E_INPUT_USE_ITEM)) is positive, a
    /// character on land queues `ACT_PICKUP_WEAPON`, the full-body pickup animation. The weapon's
    /// pickup interaction starts the pickup when it executes
    /// ([`WeaponBase::PickupStart`](crate::equipment::weapon::WeaponBase::PickupStart)), and the weapon's
    /// pickup track message completes it
    /// ([`WeaponBase::PickupItem`](crate::equipment::weapon::WeaponBase::PickupItem)).
    /// A swimming character instead takes the first weapon among its interaction user proxy's
    /// valid entries (or the weapon of a weapon placement rule among them), drops the weapon
    /// already in that weapon's slot through the occupant's drop virtual, picks it up at once with
    /// `PickupItem`, and queues `ACT_PICKUP_WEAPON_PARTIAL` after cancelling the reload (or, in a
    /// state that forbids drawing, unequips it again).
    pub unsafe fn UpdateWeaponHandling(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateWeaponHandling_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const GetAnimationAllowsPickup_ADDRESS: usize = 0x1408BB6F0;
    /// Whether the character may pick up a weapon now:
    /// [`m_DoesAnimationAllowPickup`](crate::character::inventory::Inventory::m_DoesAnimationAllowPickup) is set and the owner's
    /// [`SB_DISABLE_PICKUP`](crate::state::StateBitFlag::SB_DISABLE_PICKUP) state bit is clear. The weapon
    /// pickup interaction tests it before it starts a pickup.
    pub unsafe fn GetAnimationAllowsPickup(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsPickup_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetAnimationAllowsDrawing_ADDRESS: usize = 0x1408BB840;
    /// Whether the character may draw a weapon:
    /// [`m_DoesAnimationAllowDrawing`](crate::character::inventory::Inventory::m_DoesAnimationAllowDrawing) is set and the owner's
    /// [`SB_DISABLE_DRAW_WEAPON`](crate::state::StateBitFlag::SB_DISABLE_DRAW_WEAPON) state bit is clear.
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) commits a slot selection with nothing
    /// wielded only while it holds.
    pub unsafe fn GetAnimationAllowsDrawing(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsDrawing_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetAnimationAllowsHolstering_ADDRESS: usize = 0x1408BB870;
    /// Whether the character may holster the wielded weapon:
    /// [`m_DoesAnimationAllowHolstering`](crate::character::inventory::Inventory::m_DoesAnimationAllowHolstering) is set and the
    /// owner's [`SB_DISABLE_HOLSTER_WEAPON`](crate::state::StateBitFlag::SB_DISABLE_HOLSTER_WEAPON) state
    /// bit is clear.
    pub unsafe fn GetAnimationAllowsHolstering(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsHolstering_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetAnimationAllowsSwitchWeaponGUI_ADDRESS: usize = 0x1408BB750;
    /// Whether the weapon-selection input is read:
    /// [`m_DoesAnimationAllowSwitchWeaponGUI`](crate::character::inventory::Inventory::m_DoesAnimationAllowSwitchWeaponGUI) is set
    /// and the owner's
    /// [`SB_DISABLE_SWITCH_WEAPON_GUI`](crate::state::StateBitFlag::SB_DISABLE_SWITCH_WEAPON_GUI) state bit
    /// is clear. Its one caller, the player's global input update, folds the slot-select and cycle
    /// actions into [`Character::m_WeaponInputs`](crate::character::character::Character::m_WeaponInputs)
    /// only while it holds, so while it is false those inputs read zero.
    pub unsafe fn GetAnimationAllowsSwitchWeaponGUI(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetAnimationAllowsSwitchWeaponGUI_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const TryToSwitchToValidWeapon_ADDRESS: usize = 0x1408D61F0;
    /// Switches to the first weapon that still has ammunition, in a fixed order: the
    /// [`E_SLOT_TWO_HANDED`](crate::character::inventory::WeaponSlot::E_SLOT_TWO_HANDED) weapon, then the
    /// [`E_SLOT_DUAL_WIELD`](crate::character::inventory::WeaponSlot::E_SLOT_DUAL_WIELD) weapon, then the
    /// [`E_SLOT_TWO_HANDED_SPECIAL`](crate::character::inventory::WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL) weapon. A weapon
    /// qualifies when its magazine is not empty or the inventory holds ammunition of its type; the
    /// order does not depend on the wielded slot, which is skipped only because it is empty. The
    /// first that qualifies is switched to through
    /// [`Character::SelectWeapon`](crate::character::character::Character::SelectWeapon), and the function
    /// returns true. It does nothing and returns false while
    /// [`m_DoesAnimationAllowSwitchingWeapons`](crate::character::inventory::Inventory::m_DoesAnimationAllowSwitchingWeapons) is
    /// clear, while the owner's
    /// [`SB_DISABLE_SWITCH_WEAPONS`](crate::state::StateBitFlag::SB_DISABLE_SWITCH_WEAPONS) state bit is
    /// set, or when no weapon qualifies.
    ///
    /// It has two callers. [`WeaponBase::CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase)
    /// calls it when the player pulls the trigger of a weapon that is empty and cannot reload,
    /// which is the game's automatic switch away from an empty weapon. The `NWeaponEquipTask`
    /// state task ([`NWeaponEquipTask_OnEnter`](crate::equipment::weapon_animation::NWeaponEquipTask_OnEnter)) calls it on entry when its character wields nothing and the task is not
    /// configured to equip a fixed slot.
    pub unsafe fn TryToSwitchToValidWeapon(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::TryToSwitchToValidWeapon_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateAttachmentTransforms_ADDRESS: usize = 0x1408E46A0;
    /// Places the inventory's weapons on the character
    /// (`CInventory::UpdateAttachmentTransforms`): calls each slot weapon's attachment-transform
    /// virtual, which sets every gun's current world matrix
    /// ([`WeaponData::m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1)) from the
    /// bone its attach-bone table
    /// ([`WeaponData::m_lBoneAttachementPerWeaponState`](crate::equipment::weapon::WeaponData::m_lBoneAttachementPerWeaponState))
    /// names for the weapon's state, then places the hand-held hijacking props, the boost
    /// wingsuit's guns while wingsuiting, and the hijacking explosive. The character's pose-finalization pass
    /// ([`Character::UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel))
    /// calls it after its state tasks and again after the `MAIN` IK solve;
    /// [`Character::UpdateSecondaryHandIKPass`](crate::character::character::Character::UpdateSecondaryHandIKPass)
    /// and the character's prop-transform update call it too. The `p3` and `p4` floats are
    /// passed through to the weapons unchanged.
    pub unsafe fn UpdateAttachmentTransforms(
        &mut self,
        character: *const crate::character::character::Character,
        p3: f32,
        p4: f32,
        transform_type: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *const crate::character::character::Character,
                p3: f32,
                p4: f32,
                transform_type: u32,
            ) = ::std::mem::transmute(Self::UpdateAttachmentTransforms_ADDRESS);
            f(self as *mut Self as _, character, p3, p4, transform_type)
        }
    }
    pub const UpdateInventoryPostFinalizePose_ADDRESS: usize = 0x1408C6960;
    /// Runs each slot weapon's
    /// [`UpdatePostFinalizePose`](crate::equipment::weapon::WeaponBase::UpdatePostFinalizePose), which
    /// rewrites every gun's attach-bone table from the weapon's slot. The character's
    /// pose-finalization pass calls it after the attachment placement and the IK passes, so a
    /// table entry written by the character's state tasks or by any other writer lasts until this
    /// call.
    pub unsafe fn UpdateInventoryPostFinalizePose(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateInventoryPostFinalizePose_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const DropWeapon_ADDRESS: usize = 0x140932710;
    /// Drops the weapon in `slot` into the world: clears the slot at once, stops using the weapon
    /// when it is wielding or dual-wielding ([`WeaponBase::StopUsing`](crate::equipment::weapon::WeaponBase::StopUsing),
    /// which holsters any weapon but a heavy one), and defers the drop itself
    /// ([`WeaponBase::OnDrop`](crate::equipment::weapon::WeaponBase::OnDrop)) to the weapon's next
    /// pre-simulation update. With `select_valid` set and `slot` selected, it then selects another
    /// weapon. An empty slot is left as it is. The inventory's add drops the
    /// occupant of a slot this way when the slot's drop flag is set, and the character drops a
    /// heavy weapon this way when its holster or cancel input is clicked.
    pub unsafe fn DropWeapon(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
        select_valid: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
                select_valid: bool,
            ) = ::std::mem::transmute(Self::DropWeapon_ADDRESS);
            f(self as *mut Self as _, slot, select_valid)
        }
    }
    pub const UpdateAttachmentEffects_ADDRESS: usize = 0x1408C6D30;
    /// Updates the inventory's attached effect container (the wielded weapon's muzzle flash,
    /// casings, and sounds), when it has one.
    pub unsafe fn UpdateAttachmentEffects(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateAttachmentEffects_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const RemoveWeapon_ADDRESS: usize = 0x1408ECE00;
    /// Takes the weapon out of `slot` without dropping it (`CInventory::RemoveWeapon`): clears the
    /// slot's reference and releases it, and clears the character's next selected slot when it was
    /// `slot`. The weapon's state, holder
    /// ([`WeaponBase::m_Holder`](crate::equipment::weapon::WeaponBase::m_Holder)), and user are left as
    /// they were, so a weapon that has another strong reference stays as it
    /// is, holstered on the character, outside the inventory.
    pub unsafe fn RemoveWeapon(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
            ) = ::std::mem::transmute(Self::RemoveWeapon_ADDRESS);
            f(self as *mut Self as _, slot)
        }
    }
    pub const AddWeapon_ADDRESS: usize = 0x1409328B0;
    /// Puts `weapon` into its slot
    /// ([`WeaponBase::m_EquipSlot`](crate::equipment::weapon::WeaponBase::m_EquipSlot))
    /// (`CInventory::AddWeapon`). A different weapon already in the slot is dropped through
    /// [`DropWeapon`](crate::character::inventory::Inventory::DropWeapon) when the slot's drop-old flag of the respawn table is set,
    /// and despawned otherwise. The slot then takes a reference to `weapon`, and the weapon's
    /// [`OnPickup`](crate::equipment::weapon::WeaponBase::OnPickup) runs with the character's
    /// `CGameObject` subobject as its holder. With [`E_EQUIP`](crate::character::inventory::EquipMode::E_EQUIP), or
    /// [`E_EQUIP_IF_IN_SELECTED_SLOT`](crate::character::inventory::EquipMode::E_EQUIP_IF_IN_SELECTED_SLOT) on the selected
    /// slot, the wielded weapon is unequipped
    /// ([`OnUnequip`](crate::equipment::weapon::WeaponBase::OnUnequip)), `weapon` is equipped
    /// ([`OnEquip`](crate::equipment::weapon::WeaponBase::OnEquip)), and the HUD shows it as selected for
    /// a locally controlled character. A hand grenade, a planted explosive, or a beacon is started
    /// in use. The weapon's visibility is set to the character's. A weapon in the mounted state
    /// only takes the slot and becomes the selected slot. Adding the slot's own occupant again does
    /// nothing.
    pub unsafe fn AddWeapon(
        &mut self,
        weapon: *const crate::types::shared_ptr::SharedPtr<
            crate::equipment::weapon::WeaponBase,
        >,
        mode: crate::character::inventory::EquipMode,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon: *const crate::types::shared_ptr::SharedPtr<
                    crate::equipment::weapon::WeaponBase,
                >,
                mode: crate::character::inventory::EquipMode,
            ) = ::std::mem::transmute(Self::AddWeapon_ADDRESS);
            f(self as *mut Self as _, weapon, mode)
        }
    }
    pub const HandleEvent_ADDRESS: usize = 0x1409425D0;
    /// The inventory's event handler (`CInventory::HandleEvent`), reached through its event
    /// handler's vtable for the events it subscribes: the spawn system's pre-reset
    /// ([`m_SpawnSystemPreReset`](crate::character::inventory::Inventory::m_SpawnSystemPreReset)) and post-reset
    /// ([`m_SpawnSystemPostReset`](crate::character::inventory::Inventory::m_SpawnSystemPostReset)) events, told apart with
    /// [`EventMatches`](crate::event::EventMatches).
    pub unsafe fn HandleEvent(
        &mut self,
        event: *const crate::event::EventImpl,
        data: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                event: *const crate::event::EventImpl,
                data: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::HandleEvent_ADDRESS);
            f(self as *mut Self as _, event, data)
        }
    }
    pub const UpdateWeaponVisibility_ADDRESS: usize = 0x1408C6D50;
    /// Shows or hides every slot's weapon: calls each one's
    /// [`SetVisible`](crate::equipment::weapon::WeaponBase::SetVisible) with `visible` and neither fade nor
    /// force. The character calls it when its own visibility changes.
    pub unsafe fn UpdateWeaponVisibility(&mut self, visible: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, visible: bool) = ::std::mem::transmute(
                Self::UpdateWeaponVisibility_ADDRESS,
            );
            f(self as *mut Self as _, visible)
        }
    }
    pub const GivePlayerWeapon_ADDRESS: usize = 0x140947980;
    /// Gives the character a weapon of spawn tag index `tag_index` in `slot` through the respawn
    /// table: writes the slot's entry ([`m_WeaponToSpawn`](crate::character::inventory::Inventory::m_WeaponToSpawn) and its
    /// [`m_WeaponToSpawnAmmunition`](crate::character::inventory::Inventory::m_WeaponToSpawnAmmunition),
    /// [`m_WeaponToSpawnExtraAmmo`](crate::character::inventory::Inventory::m_WeaponToSpawnExtraAmmo), and
    /// [`m_WeaponToSpawnDropOld`](crate::character::inventory::Inventory::m_WeaponToSpawnDropOld) entries, and with
    /// `start_equipped` [`m_WeaponToSpawnEquippedWeapon`](crate::character::inventory::Inventory::m_WeaponToSpawnEquippedWeapon))
    /// and requests its spawn ([`RequestWeaponSpawn`](crate::character::inventory::Inventory::RequestWeaponSpawn)). The weapon
    /// arrives later through [`InventoryWeaponSpawnCallback`](crate::character::inventory::InventoryWeaponSpawnCallback) and
    /// [`WeaponSpawned`](crate::character::inventory::Inventory::WeaponSpawned). Every game-side give takes this path: missions and
    /// their checkpoint restores, challenges, the graph script's `SpawnWeapons` node, the
    /// `equip weapon` event, a rack's infinite pickups
    /// ([`WeaponBase::m_InfinitePickup`](crate::equipment::weapon::WeaponBase::m_InfinitePickup)), and the
    /// game-completion reward.
    pub unsafe fn GivePlayerWeapon(
        &mut self,
        tag_index: i32,
        ammo: i32,
        slot: crate::character::inventory::WeaponSlot,
        start_equipped: bool,
        extra_ammo_to_inventory: bool,
        drop_old: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                tag_index: i32,
                ammo: i32,
                slot: crate::character::inventory::WeaponSlot,
                start_equipped: bool,
                extra_ammo_to_inventory: bool,
                drop_old: bool,
            ) = ::std::mem::transmute(Self::GivePlayerWeapon_ADDRESS);
            f(
                self as *mut Self as _,
                tag_index,
                ammo,
                slot,
                start_equipped,
                extra_ammo_to_inventory,
                drop_old,
            )
        }
    }
    pub const RequestWeaponSpawn_ADDRESS: usize = 0x140942510;
    /// Requests the spawn of `slot`'s respawn-table entry from the spawn system, with
    /// [`InventoryWeaponSpawnCallback`](crate::character::inventory::InventoryWeaponSpawnCallback) as the callback, unless one is already pending
    /// ([`m_WeaponPendingSpawn`](crate::character::inventory::Inventory::m_WeaponPendingSpawn)). The disassembler swaps its name with
    /// [`RequestSpawnGrapplingDevice`](crate::character::inventory::Inventory::RequestSpawnGrapplingDevice): it labels this function
    /// `CInventory::RequestSpawnGrapplingDevice`.
    pub unsafe fn RequestWeaponSpawn(&mut self, slot: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, slot: i32) = ::std::mem::transmute(
                Self::RequestWeaponSpawn_ADDRESS,
            );
            f(self as *mut Self as _, slot)
        }
    }
    pub const RequestSpawnGrapplingDevice_ADDRESS: usize = 0x14091C470;
    /// Requests the spawn of the character's grappling device. The disassembler labels it
    /// `CInventory::RequestWeaponSpawn`; see [`RequestWeaponSpawn`](crate::character::inventory::Inventory::RequestWeaponSpawn).
    pub unsafe fn RequestSpawnGrapplingDevice(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RequestSpawnGrapplingDevice_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const WeaponSpawned_ADDRESS: usize = 0x140932B30;
    /// Hands a weapon spawned for the respawn table to the character. `weapon` is a
    /// `boost::shared_ptr<CWeaponBase>` passed by value, so it points at the caller's copy, which
    /// this function releases. When the weapon's spawn tag index is the slot's
    /// [`m_WeaponToSpawn`](crate::character::inventory::Inventory::m_WeaponToSpawn) entry (the slot being the weapon's
    /// [`m_EquipSlot`](crate::equipment::weapon::WeaponBase::m_EquipSlot)), it clears the slot's removal
    /// mark ([`m_WeaponToRemove`](crate::character::inventory::Inventory::m_WeaponToRemove)), picks the equip mode
    /// ([`E_EQUIP`](crate::character::inventory::EquipMode::E_EQUIP) for the
    /// [`m_WeaponToSpawnEquippedWeapon`](crate::character::inventory::Inventory::m_WeaponToSpawnEquippedWeapon) slot, after
    /// unequipping the wielded weapon; [`E_EQUIP_IF_IN_SELECTED_SLOT`](crate::character::inventory::EquipMode::E_EQUIP_IF_IN_SELECTED_SLOT)
    /// when that is [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE); [`E_DONT_EQUIP`](crate::character::inventory::EquipMode::E_DONT_EQUIP)
    /// otherwise), passes the weapon to
    /// [`Character::PickUpWeapon`](crate::character::character::Character::PickUpWeapon), sets the
    /// entry's magazine and, with the extra-ammunition flag, moves the overflow into the reserve,
    /// applies the character's visibility to every slot weapon, and clears the entry. On a tag
    /// mismatch it removes the slot's occupant ([`Internal_RemoveWeapon`](crate::character::inventory::Inventory::Internal_RemoveWeapon)),
    /// or despawns the newcomer when the slot is empty. Ordinary world weapons never pass through
    /// it: it sees only respawn-table weapons, the gives and the respawns after a spawn reset.
    pub unsafe fn WeaponSpawned(
        &mut self,
        weapon: *mut crate::types::shared_ptr::SharedPtr<
            crate::equipment::weapon::WeaponBase,
        >,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon: *mut crate::types::shared_ptr::SharedPtr<
                    crate::equipment::weapon::WeaponBase,
                >,
            ) = ::std::mem::transmute(Self::WeaponSpawned_ADDRESS);
            f(self as *mut Self as _, weapon)
        }
    }
    pub const ForceRemoveWeapon_ADDRESS: usize = 0x1408D5D40;
    /// Marks `slot`'s occupant for removal ([`m_WeaponToRemove`](crate::character::inventory::Inventory::m_WeaponToRemove)), carried
    /// out by the next [`UpdatePrePhysics`](crate::character::inventory::Inventory::UpdatePrePhysics). With `reset_ammo` it also
    /// empties the weapon's magazine and the reserve of its ammunition type, and with
    /// `clear_spawning` it clears the slot's respawn-table entry.
    pub unsafe fn ForceRemoveWeapon(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
        reset_ammo: bool,
        clear_spawning: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
                reset_ammo: bool,
                clear_spawning: bool,
            ) = ::std::mem::transmute(Self::ForceRemoveWeapon_ADDRESS);
            f(self as *mut Self as _, slot, reset_ammo, clear_spawning)
        }
    }
    pub const ForceRemoveAllWeapons_ADDRESS: usize = 0x140902D30;
    /// Marks every slot for removal, like [`ForceRemoveWeapon`](crate::character::inventory::Inventory::ForceRemoveWeapon) on each.
    pub unsafe fn ForceRemoveAllWeapons(&mut self, clear_spawning: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, clear_spawning: bool) = ::std::mem::transmute(
                Self::ForceRemoveAllWeapons_ADDRESS,
            );
            f(self as *mut Self as _, clear_spawning)
        }
    }
    pub const CancelRemoveWeapon_ADDRESS: usize = 0x1408BB450;
    /// Clears `slot`'s removal mark ([`m_WeaponToRemove`](crate::character::inventory::Inventory::m_WeaponToRemove)).
    pub unsafe fn CancelRemoveWeapon(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
            ) = ::std::mem::transmute(Self::CancelRemoveWeapon_ADDRESS);
            f(self as *mut Self as _, slot)
        }
    }
    pub const Internal_RemoveWeapon_ADDRESS: usize = 0x14091B280;
    /// Carries out `slot`'s removal: a mounted gun is stopped and released, any other occupant is
    /// despawned ([`DespawnWeapon`](crate::character::inventory::Inventory::DespawnWeapon)), and the mark is cleared.
    pub unsafe fn Internal_RemoveWeapon(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
            ) = ::std::mem::transmute(Self::Internal_RemoveWeapon_ADDRESS);
            f(self as *mut Self as _, slot)
        }
    }
    pub const DespawnWeapon_ADDRESS: usize = 0x14091AC10;
    /// Empties `slot` and despawns its weapon at once
    /// ([`WeaponBase::Despawn`](crate::equipment::weapon::WeaponBase::Despawn)). The challenges call it
    /// directly, bypassing the removal marks.
    pub unsafe fn DespawnWeapon(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
            ) = ::std::mem::transmute(Self::DespawnWeapon_ADDRESS);
            f(self as *mut Self as _, slot)
        }
    }
    pub const UpdatePrePhysics_ADDRESS: usize = 0x140932DA0;
    /// The inventory's pre-physics update: carries out every marked removal
    /// ([`Internal_RemoveWeapon`](crate::character::inventory::Inventory::Internal_RemoveWeapon)), then updates the secondary attach
    /// objects.
    pub unsafe fn UpdatePrePhysics(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdatePrePhysics_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const GetWeapon_ADDRESS: usize = 0x1408BB9C0;
    /// The weapon reference in `slot` ([`m_Weapons`](crate::character::inventory::Inventory::m_Weapons)). The disassembler mislabels
    /// it `CPOI::Create`.
    pub unsafe fn GetWeapon(
        &self,
        slot: crate::character::inventory::WeaponSlot,
    ) -> *const crate::types::shared_ptr::SharedPtr<
        crate::equipment::weapon::WeaponBase,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                slot: crate::character::inventory::WeaponSlot,
            ) -> *const crate::types::shared_ptr::SharedPtr<
                crate::equipment::weapon::WeaponBase,
            > = ::std::mem::transmute(Self::GetWeapon_ADDRESS);
            f(self as *const Self as _, slot)
        }
    }
    pub const HasWeaponWithAmmoType_ADDRESS: usize = 0x1408C6EF0;
    /// Whether a slot holds a weapon whose
    /// [`m_AmmunitionType`](crate::equipment::weapon::WeaponBase::m_AmmunitionType) is `ammo_type`. It
    /// reads only the eight slots. Its one caller is the pickup's automatic ammunition credit
    /// ([`WeaponBase::ShouldAutoPickup`](crate::equipment::weapon::WeaponBase::ShouldAutoPickup)).
    pub unsafe fn HasWeaponWithAmmoType(
        &self,
        ammo_type: crate::equipment::weapon::AmmunitionType,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ammo_type: crate::equipment::weapon::AmmunitionType,
            ) -> bool = ::std::mem::transmute(Self::HasWeaponWithAmmoType_ADDRESS);
            f(self as *const Self as _, ammo_type)
        }
    }
    pub const AddAmmo_ADDRESS: usize = 0x1408D6190;
    /// Adds `count` rounds to the reserve of `ammo_type`, up to its cap
    /// ([`m_MaxAmmunition`](crate::character::inventory::Inventory::m_MaxAmmunition)), and returns how many were added.
    pub unsafe fn AddAmmo(
        &mut self,
        ammo_type: crate::equipment::weapon::AmmunitionType,
        count: i32,
    ) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ammo_type: crate::equipment::weapon::AmmunitionType,
                count: i32,
            ) -> i32 = ::std::mem::transmute(Self::AddAmmo_ADDRESS);
            f(self as *mut Self as _, ammo_type, count)
        }
    }
    pub const RespawnPlayerGear_ADDRESS: usize = 0x1409479D0;
    /// Re-requests the spawn of every respawn-table entry not already pending, and of the
    /// grappling device. The player's post-physics update calls it once the save data has loaded.
    pub unsafe fn RespawnPlayerGear(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RespawnPlayerGear_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RespawnPlayerWeapon_ADDRESS: usize = 0x140942840;
    /// Requests the respawn table's weapons with the two-handed slot as the equipped slot
    /// ([`m_WeaponToSpawnEquippedWeapon`](crate::character::inventory::Inventory::m_WeaponToSpawnEquippedWeapon)).
    pub unsafe fn RespawnPlayerWeapon(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RespawnPlayerWeapon_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const GiveMaxAmmoToAllWeaponInInventory_ADDRESS: usize = 0x1408D6010;
    /// Fills the reserve of each slot weapon's ammunition type. The player's respawn
    /// ([`Player::ReInit`](crate::character::player::Player::ReInit)) calls it.
    pub unsafe fn GiveMaxAmmoToAllWeaponInInventory(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::GiveMaxAmmoToAllWeaponInInventory_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SelectValidWeapon_ADDRESS: usize = 0x14091AD40;
    /// Selects a weapon that has ammunition and returns whether it found one. The meanings of the
    /// two flags are unverified. [`Character::DrawWeapon`](crate::character::character::Character::DrawWeapon),
    /// [`DropWeapon`](crate::character::inventory::Inventory::DropWeapon), [`WeaponBase::StopUsing`](crate::equipment::weapon::WeaponBase::StopUsing)
    /// for a mounted gun, the planted explosive's stop, and the store's close call it.
    pub unsafe fn SelectValidWeapon(&mut self, p1: bool, p2: bool) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                p1: bool,
                p2: bool,
            ) -> bool = ::std::mem::transmute(Self::SelectValidWeapon_ADDRESS);
            f(self as *mut Self as _, p1, p2)
        }
    }
}
impl std::convert::AsRef<Inventory> for Inventory {
    fn as_ref(&self) -> &Inventory {
        self
    }
}
impl std::convert::AsMut<Inventory> for Inventory {
    fn as_mut(&mut self) -> &mut Inventory {
        self
    }
}
crate::__bitflags! {
    #[doc =
    " The flags of [`Inventory::m_SpawnSystemResetFlags`](crate::character::inventory::Inventory::m_SpawnSystemResetFlags), which shape what the spawn system's"]
    #[doc = " pre-reset event records into the respawn table."] pub struct
    SpawnSystemResetFlags : u32 { const RESPAWN_RESET_LOADOUT = 1usize as _; const
    FULL_MAGAZINES = 2usize as _; const RESET_GRAPPLING_DEVICE = 4usize as _; }
}
fn _SpawnSystemResetFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SpawnSystemResetFlags>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// An inventory weapon slot (`CInventory::EWeaponSlot`). Mounted guns a character picks up occupy
/// [`E_SLOT_HEAVY`](crate::character::inventory::WeaponSlot::E_SLOT_HEAVY): `CCharacter::IsInMountedGunState` tests the weapon in that
/// slot.
pub enum WeaponSlot {
    E_SLOT_DUAL_WIELD = 0isize as _,
    E_SLOT_TWO_HANDED = 1isize as _,
    E_SLOT_HEAVY = 2isize as _,
    E_SLOT_HAND_GRENADE = 3isize as _,
    E_SLOT_PLANTED_EXPLOSIVE = 4isize as _,
    E_SLOT_FOW = 5isize as _,
    E_SLOT_TWO_HANDED_SPECIAL = 6isize as _,
    E_SLOT_BEACON = 7isize as _,
    E_NUM_WEAPON_SLOTS = 8isize as _,
    E_NONE = 9isize as _,
}
fn _WeaponSlot_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WeaponSlot>([0u8; 0x4]);
    }
    unreachable!()
}
pub const InventoryWeaponSpawnCallback_ADDRESS: usize = 0x140942180;
/// The spawn callback ([`SpawnCallback`](crate::spawn_system::SpawnCallback)) of the respawn table's
/// requests, with the inventory as its userdata. It despawns a spawned weapon that is not a
/// profile item, and hands any other to [`Inventory::WeaponSpawned`](crate::character::inventory::Inventory::WeaponSpawned).
pub unsafe fn InventoryWeaponSpawnCallback(
    objects: *const crate::types::std_vector::Vector<
        crate::types::shared_ptr::SharedPtr<crate::game::GameObject>,
    >,
    inventory: *mut crate::character::inventory::Inventory,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            objects: *const crate::types::std_vector::Vector<
                crate::types::shared_ptr::SharedPtr<crate::game::GameObject>,
            >,
            inventory: *mut crate::character::inventory::Inventory,
        ) = ::std::mem::transmute(InventoryWeaponSpawnCallback_ADDRESS);
        f(objects, inventory)
    }
}
pub const NSetInventoryPermissions_TranslatePermissionsFlagToStateBit_ADDRESS: usize = 0x140837E80;
/// Maps a bit of an `NSetInventoryPermissions` state task's `Flags` property to the state bit it
/// sets: the shoot flag to [`SB_DISABLE_SHOOT`](crate::state::StateBitFlag::SB_DISABLE_SHOOT), the reload
/// flag to [`SB_DISABLE_RELOAD`](crate::state::StateBitFlag::SB_DISABLE_RELOAD), the grenade flag to
/// [`SB_DISABLE_USE_GRENADES`](crate::state::StateBitFlag::SB_DISABLE_USE_GRENADES), and so on through
/// the disable bits that [`Inventory::UpdateInventoryPermissions`](crate::character::inventory::Inventory::UpdateInventoryPermissions) latches.
pub unsafe fn NSetInventoryPermissions_TranslatePermissionsFlagToStateBit(
    flag_bit: i32,
) -> i32 {
    unsafe {
        let f: unsafe extern "system" fn(flag_bit: i32) -> i32 = ::std::mem::transmute(
            NSetInventoryPermissions_TranslatePermissionsFlagToStateBit_ADDRESS,
        );
        f(flag_bit)
    }
}
pub const NSetInventoryPermissions_SetPermissions_ADDRESS: usize = 0x140837F50;
/// Sets the state bits of the permission `flags` (mapped through
/// [`NSetInventoryPermissions_TranslatePermissionsFlagToStateBit`](crate::character::inventory::NSetInventoryPermissions_TranslatePermissionsFlagToStateBit)) in `model`'s current state-bit
/// words. The state task's scopes call it: the whole-state scope on entry and every update, and the
/// segment scope within a named clip segment.
pub unsafe fn NSetInventoryPermissions_SetPermissions(
    model: *mut crate::character::character::AnimatedModel,
    flags: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            model: *mut crate::character::character::AnimatedModel,
            flags: i32,
        ) = ::std::mem::transmute(NSetInventoryPermissions_SetPermissions_ADDRESS);
        f(model, flags)
    }
}
pub const NSetInventoryPermissions_NStateScope_OnEnter_ADDRESS: usize = 0x1408493E0;
/// The entry of the whole-state scope of the `NSetInventoryPermissions` state task.
pub unsafe fn NSetInventoryPermissions_NStateScope_OnEnter(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NSetInventoryPermissions_NStateScope_OnEnter_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NSetInventoryPermissions_NStateScope_Update_ADDRESS: usize = 0x140837FC0;
/// The update of the whole-state scope of the `NSetInventoryPermissions` state task, which sets
/// the permission bits on every update of its state.
pub unsafe fn NSetInventoryPermissions_NStateScope_Update(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NSetInventoryPermissions_NStateScope_Update_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NSetInventoryPermissions_NSegmentScope_OnEnter_ADDRESS: usize = 0x140838020;
/// The entry of the segment scope of the `NSetInventoryPermissions` state task.
pub unsafe fn NSetInventoryPermissions_NSegmentScope_OnEnter(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(
            NSetInventoryPermissions_NSegmentScope_OnEnter_ADDRESS,
        );
        f(context, properties, instance)
    }
}
pub const NSetInventoryPermissions_NSegmentScope_Update_ADDRESS: usize = 0x140847810;
/// The update of the segment scope of the `NSetInventoryPermissions` state task, which sets the
/// permission bits while the state's clip is inside the task's named segment.
pub unsafe fn NSetInventoryPermissions_NSegmentScope_Update(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NSetInventoryPermissions_NSegmentScope_Update_ADDRESS);
        f(context, properties, instance)
    }
}
