#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The character's equipment inventory (`CInventory`), embedded in `CCharacter`
/// ([`Character::m_Inventory`](crate::character::character::Character::m_Inventory)). Only some members
/// are mapped.
pub struct Inventory {
    _field_0: [u8; 32],
    /// The weapon in each slot, indexed by [`WeaponSlot`](crate::character::inventory::WeaponSlot) (up to
    /// [`E_NUM_WEAPON_SLOTS`](crate::character::inventory::WeaponSlot::E_NUM_WEAPON_SLOTS)); a null pointer for an empty slot.
    /// `CCharacter::GetWeaponInInventory` returns a reference to an entry.
    pub m_Weapons: [crate::types::shared_ptr::SharedPtr<
        crate::equipment::weapon::WeaponBase,
    >; 8],
    _field_a0: [u8; 32],
    /// The character's grappling hook, or null before the inventory is populated.
    /// `CCharacter::GetGrapplingHook` returns a reference to this field.
    pub m_GrapplingHook: crate::types::shared_ptr::SharedPtr<
        crate::equipment::grappling_hook::GrapplingHook,
    >,
    _field_d0: [u8; 180],
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
    /// committed slot selection (`CCharacter::SwitchWeapon`), and
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
    _field_18c: [u8; 6],
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
    _field_3e8: [u8; 12],
    /// The slot the weapon selection commits when every slot-select input is released, or
    /// [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE). While a slot-select input is held,
    /// [`UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling) rewrites it every frame with the winning
    /// slot, or with [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE) when the winner is the wielded weapon's slot or
    /// holds no weapon. On a frame with every slot-select input zero and this set, it switches to
    /// the slot (`CCharacter::SwitchWeapon`) when the animation allows switching, or selects and
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
    /// mounted-gun state, otherwise the boost wingsuit's guns while wingsuiting or else the wielding
    /// weapon, all with `muzzle_target`, and then the selected grenade with `grenade_target`. The
    /// fire inputs passed down come from the owner's
    /// [`m_WeaponInputs`](crate::character::character::Character::m_WeaponInputs), zeroed while shooting
    /// (or throwing) is not allowed. The disassembler mislabels it `CGrapplingHook::GetFireInputEnabled`.
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
    /// grapple reel-in predicates among others).
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
    /// valid entries (or the weapon of a weapon placement rule among them), picks it up at once
    /// with `PickupItem`, and queues `ACT_PICKUP_WEAPON_PARTIAL`.
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
    /// first that qualifies is switched to through `CCharacter::SelectWeapon`, and the function
    /// returns true. It does nothing and returns false while
    /// [`m_DoesAnimationAllowSwitchingWeapons`](crate::character::inventory::Inventory::m_DoesAnimationAllowSwitchingWeapons) is
    /// clear, while the owner's
    /// [`SB_DISABLE_SWITCH_WEAPONS`](crate::state::StateBitFlag::SB_DISABLE_SWITCH_WEAPONS) state bit is
    /// set, or when no weapon qualifies.
    ///
    /// It has two callers. [`WeaponBase::CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase)
    /// calls it when the player pulls the trigger of a weapon that is empty and cannot reload,
    /// which is the game's automatic switch away from an empty weapon. The `NWeaponEquipTask`
    /// state task calls it on entry when its character wields nothing and the task is not
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
