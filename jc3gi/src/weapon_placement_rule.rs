#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A spawn rule that places a weapon in the world (`CWeaponPlacementRule`): a weapon at a spot, a
/// weapon in a rack, or a mounted gun, optionally manned. Its `CGameObject` subobject is at its
/// start. Once its weapon has spawned, the rule hands the weapon its own infinite-pickup setting
/// ([`WeaponBase::m_InfinitePickup`](crate::equipment::weapon::WeaponBase::m_InfinitePickup)); a rack's
/// weapon is put in the [`E_WS_WEAPON_RACK`](crate::equipment::weapon::WeaponState::E_WS_WEAPON_RACK)
/// state, with its own pickup interaction disabled unless the rack uses the ordinary pickup, so
/// that it is picked up through the rule's interaction node instead. Only its type id and one
/// virtual are bound.
pub struct WeaponPlacementRule {
    vftable: *const crate::weapon_placement_rule::WeaponPlacementRuleVftable,
}
fn _WeaponPlacementRule_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], WeaponPlacementRule>([0u8; 0x8]);
    }
    unreachable!()
}
impl WeaponPlacementRule {
    pub fn vftable(
        &self,
    ) -> *const crate::weapon_placement_rule::WeaponPlacementRuleVftable {
        self.vftable as *const crate::weapon_placement_rule::WeaponPlacementRuleVftable
    }
    pub const TYPE_ID_ADDRESS: usize = 0x1407C8540;
    /// `CWeaponPlacementRule::TYPE_ID`: the run-time type id of `CWeaponPlacementRule`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    /// `CSpawnRule::GetSpawnedObject`: writes into `result` a strong reference to the
    /// `index`th live object the rule spawned that is of the class `type_id` names (see
    /// [`GameObject::IsType`](crate::game::GameObject::IsType)), or a null reference when there is
    /// none, and returns `result`.
    pub unsafe fn GetSpawnedObject(
        &self,
        result: *mut crate::types::shared_ptr::SharedPtr<crate::game::GameObject>,
        type_id: *const u32,
        index: i32,
    ) -> *mut crate::types::shared_ptr::SharedPtr<crate::game::GameObject> {
        unsafe {
            let f = (&raw const (*self.vftable()).GetSpawnedObject).read();
            f(self as *const Self as _, result, type_id, index)
        }
    }
}
impl std::convert::AsRef<WeaponPlacementRule> for WeaponPlacementRule {
    fn as_ref(&self) -> &WeaponPlacementRule {
        self
    }
}
impl std::convert::AsMut<WeaponPlacementRule> for WeaponPlacementRule {
    fn as_mut(&mut self) -> &mut WeaponPlacementRule {
        self
    }
}
#[repr(C, align(8))]
pub struct WeaponPlacementRuleVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_1: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_2: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_3: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_4: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_5: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_16: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_18: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_19: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_20: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_21: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_22: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_23: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_24: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_25: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_26: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_27: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_28: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_29: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_30: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_31: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_32: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_33: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_34: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_35: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_36: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_37: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_38: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_39: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_40: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_41: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_42: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_43: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_44: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_45: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_46: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_47: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_48: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    _vfunc_49: unsafe extern "system" fn(
        this: *mut crate::weapon_placement_rule::WeaponPlacementRule,
    ),
    /// `CSpawnRule::GetSpawnedObject`: writes into `result` a strong reference to the
    /// `index`th live object the rule spawned that is of the class `type_id` names (see
    /// [`GameObject::IsType`](crate::game::GameObject::IsType)), or a null reference when there is
    /// none, and returns `result`.
    pub GetSpawnedObject: unsafe extern "system" fn(
        this: *const crate::weapon_placement_rule::WeaponPlacementRule,
        result: *mut crate::types::shared_ptr::SharedPtr<crate::game::GameObject>,
        type_id: *const u32,
        index: i32,
    ) -> *mut crate::types::shared_ptr::SharedPtr<crate::game::GameObject>,
}
fn _WeaponPlacementRuleVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x198], WeaponPlacementRuleVftable>([0u8; 0x198]);
    }
    unreachable!()
}
impl WeaponPlacementRuleVftable {}
impl std::convert::AsRef<WeaponPlacementRuleVftable> for WeaponPlacementRuleVftable {
    fn as_ref(&self) -> &WeaponPlacementRuleVftable {
        self
    }
}
impl std::convert::AsMut<WeaponPlacementRuleVftable> for WeaponPlacementRuleVftable {
    fn as_mut(&mut self) -> &mut WeaponPlacementRuleVftable {
        self
    }
}
