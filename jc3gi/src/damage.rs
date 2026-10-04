#![cfg_attr(any(), rustfmt::skip)]
crate::__bitflags! {
    #[doc =
    " The damage-type and damage-property bits of [`DamageMsg::m_DamageFlags`](crate::damage::DamageMsg::m_DamageFlags). Only the bits whose"]
    #[doc =
    " use is established are bound; the type bits each name one damage source, and the hit reaction"]
    #[doc =
    " scales a hit's impulse by a per-type factor chosen by the lowest type bit set."]
    pub struct DamageFlags : u32 { const DMG_TYPE_UNKNOWN = 1usize as _; const
    DMG_TYPE_MELEE = 64usize as _; }
}
fn _DamageFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], DamageFlags>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A damage message (`CDamageMsg`): who dealt the damage, where and in which direction it struck,
/// how much damage and impulse it carries, and its type. The engine builds one on the stack for
/// each hit: the [`VFTABLE`](crate::damage::DamageMsg::VFTABLE) pointer, its three smart pointers cleared, then
/// [`Clear`](crate::damage::DamageMsg::Clear); it fills the fields, hands the message to the target's
/// [`PhysicsGameObject::OnDamage`](crate::physics::PhysicsGameObject::OnDamage), and destroys it with
/// [`Destruct`](crate::damage::DamageMsg::Destruct), which releases the smart pointers. Only some members are bound.
pub struct DamageMsg {
    pub __vftable: u64,
    _field_8: [u8; 8],
    /// The object that dealt the damage (a weak reference). The struck character resolves it to a
    /// character or a vehicle to tell whether the local player dealt the damage.
    pub m_Damager: crate::types::shared_ptr::WeakPtrGameObject,
    /// The Havok body id of the damaging body; [`Clear`](crate::damage::DamageMsg::Clear) sets the invalid id.
    pub m_DamagerRigidBodyID: u32,
    _field_24: [u8; 4],
    /// The weapon that dealt the damage (a weak reference), or null.
    pub m_DamagerWeapon: crate::types::shared_ptr::WeakPtrGameObject,
    /// Where the damage struck, world space. The hit reaction applies its impulse here.
    pub m_Position: crate::types::math::Vector3,
    /// The direction the damage travels, world space, normalized. The hit reaction pushes the
    /// struck character along it.
    pub m_Direction: crate::types::math::Vector3,
    /// The damager's position, world space.
    pub m_DamagerPosition: crate::types::math::Vector3,
    /// The impulse the damage carries. The struck character's hit reaction scales it by the
    /// damage type's factor and compares it against its stumble and ragdoll thresholds; zero
    /// leaves the reaction to the acts the attacker queued.
    pub m_Impulse: f32,
    /// The damage, in health points.
    pub m_Damage: i16,
    _field_62: [u8; 2],
    /// The damage type and properties.
    pub m_DamageFlags: crate::damage::DamageFlags,
    /// The damager's faction, as [`Character::GetFaction`](crate::character::character::Character::GetFaction)
    /// reports it; [`Clear`](crate::damage::DamageMsg::Clear) sets `-1`. The struck character compares it with its own.
    pub m_Faction: i32,
    /// What the damager was doing (in a stunt, under the parachute, aiming down the sights, and so
    /// on), as [`Character::GetCurrentDamageSourceStateInfo`](crate::character::character::Character::GetCurrentDamageSourceStateInfo)
    /// reports it.
    pub m_DamagerSourceStateInfo: i32,
    _field_70: [u8; 40],
    /// The damage before any armour or scaling, in health points.
    pub m_OriginalDamage: i16,
    _field_9a: [u8; 46],
}
fn _DamageMsg_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC8], DamageMsg>([0u8; 0xC8]);
    }
    unreachable!()
}
impl DamageMsg {
    pub const Clear_ADDRESS: usize = 0x1404EDDC0;
    /// `CDamageMsg::Clear`: resets the message for reuse: releases the damager and weapon
    /// references and the damage-chain data, zeroes the positions, impulse, and damage, and sets
    /// the invalid damager body, [`DMG_TYPE_UNKNOWN`](crate::damage::DamageFlags::DMG_TYPE_UNKNOWN), and faction
    /// `-1`. The smart pointers must already hold a valid value (null or a live reference).
    pub unsafe fn Clear(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Clear_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Destruct_ADDRESS: usize = 0x1404EDF50;
    /// `CDamageMsg::~CDamageMsg`: releases the damager and weapon references and the damage-chain
    /// data.
    pub unsafe fn Destruct(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Destruct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl DamageMsg {
    /// The address of the `CDamageMsg` virtual table.
    pub const VFTABLE: u64 = 5405945968;
}
impl std::convert::AsRef<DamageMsg> for DamageMsg {
    fn as_ref(&self) -> &DamageMsg {
        self
    }
}
impl std::convert::AsMut<DamageMsg> for DamageMsg {
    fn as_mut(&mut self) -> &mut DamageMsg {
        self
    }
}
