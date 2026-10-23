#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The animation-graph condition on the selected weapon's slot (`CWeaponSelectedCondition`),
/// configured with a slot. Rico's graphs use it as `C_DUAL_WIELD_WEAPON`, `C_TWO_HANDED_WEAPON`,
/// `C_HEAVY_WEAPON`, `C_PLANTED_EXPLOSIVE`, and `C_TWO_HANDED_SPECIAL_WEAPON`. Its layout is
/// unmapped.
pub struct WeaponSelectedCondition {}
impl WeaponSelectedCondition {
    pub const Evaluate_ADDRESS: usize = 0x1407CE1B0;
    /// Evaluates the condition for the character owning `instance`: whether the selected slot
    /// holds a weapon whose [`m_EquipSlot`](crate::equipment::weapon::WeaponBase::m_EquipSlot) is the
    /// condition's slot, in any state, so a holstered weapon counts.
    pub unsafe fn Evaluate(
        &self,
        instance: *const crate::animation::rule_system::StateMachineInstance,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                instance: *const crate::animation::rule_system::StateMachineInstance,
            ) -> bool = ::std::mem::transmute(Self::Evaluate_ADDRESS);
            f(self as *const Self as _, instance)
        }
    }
}
impl std::convert::AsRef<WeaponSelectedCondition> for WeaponSelectedCondition {
    fn as_ref(&self) -> &WeaponSelectedCondition {
        self
    }
}
impl std::convert::AsMut<WeaponSelectedCondition> for WeaponSelectedCondition {
    fn as_mut(&mut self) -> &mut WeaponSelectedCondition {
        self
    }
}
#[repr(C, align(8))]
/// The animation-graph condition on the selected weapon's type (`CWeaponTypeCondition`),
/// configured with a `WeaponId`. Rico's upper-body graph uses one per weapon (`C_<WEAPON>`), and
/// the base graph two, for the mortar states. Its layout is unmapped.
pub struct WeaponTypeCondition {}
impl WeaponTypeCondition {
    pub const Evaluate_ADDRESS: usize = 0x1407CE090;
    /// Evaluates the condition for the character owning `instance`: whether the selected slot's
    /// weapon, in any state, has the condition's weapon id.
    pub unsafe fn Evaluate(
        &self,
        instance: *const crate::animation::rule_system::StateMachineInstance,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                instance: *const crate::animation::rule_system::StateMachineInstance,
            ) -> bool = ::std::mem::transmute(Self::Evaluate_ADDRESS);
            f(self as *const Self as _, instance)
        }
    }
}
impl std::convert::AsRef<WeaponTypeCondition> for WeaponTypeCondition {
    fn as_ref(&self) -> &WeaponTypeCondition {
        self
    }
}
impl std::convert::AsMut<WeaponTypeCondition> for WeaponTypeCondition {
    fn as_mut(&mut self) -> &mut WeaponTypeCondition {
        self
    }
}
#[repr(C, align(8))]
/// The animation-graph condition on the wielded weapon (`CWeaponWieldedCondition`, registered by
/// the character manager's condition registration), configured with a `WeaponType` slot. Rico's
/// graphs use it as `C_WEAPON_WIELDED` (any slot) and `C_PLANTED_EXPLOSIVE_WIELDED`.
pub struct WeaponWieldedCondition {
    _field_0: [u8; 16],
    /// The condition's slot (a `CInventory::EWeaponSlot`), or negative for any slot.
    pub m_WeaponType: i32,
    _field_14: [u8; 4],
}
fn _WeaponWieldedCondition_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], WeaponWieldedCondition>([0u8; 0x18]);
    }
    unreachable!()
}
impl WeaponWieldedCondition {
    pub const Evaluate_ADDRESS: usize = 0x1407DF310;
    /// Evaluates the condition for the character owning `instance`. For a negative slot it is
    /// whether the character wields anything
    /// ([`Character::GetWieldingWeapon`](crate::character::character::Character::GetWieldingWeapon)).
    /// Otherwise it takes the selected slot's weapon
    /// ([`Character::m_SelectedWeaponSlot`](crate::character::character::Character::m_SelectedWeaponSlot),
    /// or the planted-explosive slot while
    /// [`Character::IsPlantingExplosive`](crate::character::character::Character::IsPlantingExplosive)
    /// holds) and is true when that weapon is
    /// [`E_WS_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_WIELDING),
    /// [`E_WS_DUAL_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_DUAL_WIELDING), or
    /// [`E_WS_MOUNTED`](crate::equipment::weapon::WeaponState::E_WS_MOUNTED) and its
    /// [`m_EquipSlot`](crate::equipment::weapon::WeaponBase::m_EquipSlot) is the condition's slot.
    pub unsafe fn Evaluate(
        &self,
        instance: *const crate::animation::rule_system::StateMachineInstance,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                instance: *const crate::animation::rule_system::StateMachineInstance,
            ) -> bool = ::std::mem::transmute(Self::Evaluate_ADDRESS);
            f(self as *const Self as _, instance)
        }
    }
}
impl std::convert::AsRef<WeaponWieldedCondition> for WeaponWieldedCondition {
    fn as_ref(&self) -> &WeaponWieldedCondition {
        self
    }
}
impl std::convert::AsMut<WeaponWieldedCondition> for WeaponWieldedCondition {
    fn as_mut(&mut self) -> &mut WeaponWieldedCondition {
        self
    }
}
#[repr(C, align(8))]
/// The animation-graph condition that the wielded weapon sits in the left hand
/// (`CWeaponWieldedInLeftHand`), used by Rico's upper-body graph as `C_LEFT_HAND_WIELD`. Its
/// layout is unmapped.
pub struct WeaponWieldedInLeftHand {}
impl WeaponWieldedInLeftHand {
    pub const Evaluate_ADDRESS: usize = 0x1407DF440;
    /// Evaluates the condition for the character owning `instance`: the weapon
    /// [`WeaponWieldedCondition::Evaluate`](crate::equipment::weapon_animation::WeaponWieldedCondition::Evaluate) would take, wielded, and
    /// [`WeaponBase::IsWieldedInLeftHand`](crate::equipment::weapon::WeaponBase::IsWieldedInLeftHand).
    pub unsafe fn Evaluate(
        &self,
        instance: *const crate::animation::rule_system::StateMachineInstance,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                instance: *const crate::animation::rule_system::StateMachineInstance,
            ) -> bool = ::std::mem::transmute(Self::Evaluate_ADDRESS);
            f(self as *const Self as _, instance)
        }
    }
}
impl std::convert::AsRef<WeaponWieldedInLeftHand> for WeaponWieldedInLeftHand {
    fn as_ref(&self) -> &WeaponWieldedInLeftHand {
        self
    }
}
impl std::convert::AsMut<WeaponWieldedInLeftHand> for WeaponWieldedInLeftHand {
    fn as_mut(&mut self) -> &mut WeaponWieldedInLeftHand {
        self
    }
}
pub const NWeaponEquipTask_OnEnter_ADDRESS: usize = 0x140850FE0;
/// The entry of the `NWeaponEquipTask` state task, which equips a weapon when its state is
/// entered: when the character wields nothing, a non-instant task calls
/// [`Inventory::TryToSwitchToValidWeapon`](crate::character::inventory::Inventory::TryToSwitchToValidWeapon),
/// and an instant one selects the first valid of the two-handed, dual-wield, and special slots and
/// equips it at once ([`WeaponBase::OnEquip`](crate::equipment::weapon::WeaponBase::OnEquip)). No state
/// of Rico's graphs carries it.
pub unsafe fn NWeaponEquipTask_OnEnter(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NWeaponEquipTask_OnEnter_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NWeaponUnequipTask_OnEnter_ADDRESS: usize = 0x1408560A0;
/// The entry of the `NWeaponUnequipTask` state task, which puts the wielded weapon away when its
/// state is entered. It acts on
/// [`Character::GetWieldingWeapon`](crate::character::character::Character::GetWieldingWeapon) alone. With
/// its `Instant` property set, a player's weapon is unequipped at once
/// ([`WeaponBase::OnUnequip`](crate::equipment::weapon::WeaponBase::OnUnequip)) with its render
/// interpolation skipped, and another character's through
/// [`WeaponBase::UnequipDeferred`](crate::equipment::weapon::WeaponBase::UnequipDeferred); without it,
/// `UB_ACT_HOLSTER` is queued. Either way the player returns to normal aim
/// ([`Player::SetAimMode`](crate::character::player::Player::SetAimMode)) and the avatar's
/// mental-state timer is reset. In Rico's base graph the instant variant runs on entry to every
/// vehicle entry, seat, and seat change, the rides, the mounted-gun pickups and seats, the
/// wingsuit openings and tricks, free fall and fast fall, the underwater dives, the hit-reaction
/// flights, the ragdoll, the get-up, and dying.
pub unsafe fn NWeaponUnequipTask_OnEnter(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NWeaponUnequipTask_OnEnter_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NWeaponHideInVehicleTask_OnEnter_ADDRESS: usize = 0x140851120;
/// The entry of the `NWeaponHideInVehicleTask` state task: when the character's vehicle
/// ([`Character::GetVehiclePtr`](crate::character::character::Character::GetVehiclePtr)) has
/// [`m_HideCharacterWeapons`](crate::vehicle::vehicle::Vehicle::m_HideCharacterWeapons) set, the
/// configured slot's weapon, if it is
/// [`E_WS_HOLSTERED`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED), becomes
/// [`E_WS_HOLSTERED_INVISIBLE`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED_INVISIBLE). It
/// touches slot weapons only, and holstered ones only. Rico's vehicle seat states carry two
/// instances each.
pub unsafe fn NWeaponHideInVehicleTask_OnEnter(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NWeaponHideInVehicleTask_OnEnter_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NWeaponHideInVehicleTask_OnExit_ADDRESS: usize = 0x140851210;
/// The exit of the `NWeaponHideInVehicleTask` state task: the configured slot's weapon goes back
/// from [`E_WS_HOLSTERED_INVISIBLE`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED_INVISIBLE) to
/// [`E_WS_HOLSTERED`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED), and the inventory's
/// visibility is reapplied
/// ([`Inventory::UpdateWeaponVisibility`](crate::character::inventory::Inventory::UpdateWeaponVisibility)).
pub unsafe fn NWeaponHideInVehicleTask_OnExit(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NWeaponHideInVehicleTask_OnExit_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NDropWeaponStateTask_OnEnter_ADDRESS: usize = 0x140814970;
/// The entry of the `NDropWeaponStateTask` state task: drops the configured slot's weapon through
/// [`Inventory::DropWeapon`](crate::character::inventory::Inventory::DropWeapon) whenever the slot holds a
/// weapon whose holder is a character, whatever the weapon's state, so a holstered weapon is
/// dropped too. Rico's base graph configures it for the heavy slot on every grapple and tether
/// fire, grenade throw, fall, free fall and fast fall, parachute and wingsuit opening, reel-in to a
/// fall, vehicle entry, surface swim, crawl, and hit-reaction flight and stumble.
pub unsafe fn NDropWeaponStateTask_OnEnter(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NDropWeaponStateTask_OnEnter_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NStopUsingWeaponStateTask_OnEnter_ADDRESS: usize = 0x14084F5B0;
/// The entry of the `NStopUsingWeaponStateTask` state task: stops the character using its
/// selected slot's weapon, whatever its state, through the
/// [`StopUsing`](crate::equipment::weapon::WeaponBase::StopUsing) virtual. Rico's ragdoll, crash, dying,
/// drowning, and get-up states carry it.
pub unsafe fn NStopUsingWeaponStateTask_OnEnter(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NStopUsingWeaponStateTask_OnEnter_ADDRESS);
        f(context, properties, instance)
    }
}
pub const NSetWeaponStateTask_OnExit_ADDRESS: usize = 0x14084EEB0;
/// The exit of the `NSetWeaponStateTask` state task, which sets a slot's weapon to a state from
/// its data on entry: it restores the configured slot's weapon to the state recorded on entry,
/// through the equip virtual ([`WeaponBase::OnEquip`](crate::equipment::weapon::WeaponBase::OnEquip)) for
/// the wielding and dual-wielding states and through
/// [`WeaponBase::SetState`](crate::equipment::weapon::WeaponBase::SetState) for any other.
pub unsafe fn NSetWeaponStateTask_OnExit(
    context: *mut crate::state::StateContext,
    properties: *mut ::std::ffi::c_void,
    instance: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            context: *mut crate::state::StateContext,
            properties: *mut ::std::ffi::c_void,
            instance: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NSetWeaponStateTask_OnExit_ADDRESS);
        f(context, properties, instance)
    }
}
