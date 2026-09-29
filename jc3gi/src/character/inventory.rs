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
    _field_185: [u8; 603],
    /// The character that owns the inventory.
    pub m_Character: *mut crate::character::character::Character,
}
fn _Inventory_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3E8], Inventory>([0u8; 0x3E8]);
    }
    unreachable!()
}
impl Inventory {
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
    /// HUD. For the player, with no weapon input and no draw or holster playing, it acts on the
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
    pub unsafe fn UpdateWeaponHandling(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateWeaponHandling_ADDRESS,
            );
            f(self as *mut Self as _, dt)
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
