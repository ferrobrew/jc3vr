#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a target stands toward the player (`EFactionRelation`), in
/// [`Target::m_PlayerFactionRelation`](crate::aim::target::Target::m_PlayerFactionRelation).
pub enum FactionRelation {
    E_RELATION_NONE = -1isize as _,
    E_RELATION_ALLIED = 0isize as _,
    E_RELATION_HOSTILE = 1isize as _,
    NOF_RELATIONS = 2isize as _,
}
fn _FactionRelation_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], FactionRelation>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A scored aim candidate (`CTarget`) registered with the [`TargetSystem`](crate::aim::target::TargetSystem): a weak reference to its
/// game object, its local- and world-space aim positions, per-[`TargetType`](crate::aim::target::TargetType) fitness and validity,
/// and a visibility raycast. Characters, vehicles, vehicle parts, rigid objects, and destruction
/// objects each own one. Only some members are mapped.
pub struct Target {
    /// The object the target belongs to. `px` is the object's `CGameObject` subobject; for a
    /// character it sits
    /// [`Character::GAME_OBJECT_BASE_OFFSET`](crate::character::character::Character::GAME_OBJECT_BASE_OFFSET)
    /// into the `CCharacter`.
    pub m_GameObject: crate::types::shared_ptr::WeakPtrGameObject,
    _field_10: [u8; 52],
    /// The per-[`TargetType`](crate::aim::target::TargetType) fitness scores, reset at the start of each
    /// [`PlayerAimControl::UpdateAllTargets`](crate::aim::aim::PlayerAimControl::UpdateAllTargets) pass and
    /// written by the per-purpose fitness functions.
    pub m_Fitness: [f32; 6],
    /// The per-[`TargetType`](crate::aim::target::TargetType) validity flags the per-purpose fitness passes set.
    pub m_IsValid: [bool; 6],
    _field_62: [u8; 2],
    /// A per-target scale of the aim-assist cone, which
    /// [`PlayerAimControl::GetFitnessScore`](crate::aim::aim::PlayerAimControl::GetFitnessScore) applies.
    pub m_AutoAimScaleSizeValue: f32,
    _field_68: [u8; 424],
    /// The target's relation to the player, refreshed from its faction as each
    /// [`PlayerAimControl::UpdateAllTargets`](crate::aim::aim::PlayerAimControl::UpdateAllTargets) pass visits it.
    pub m_PlayerFactionRelation: crate::aim::target::FactionRelation,
    _field_214: [u8; 4],
}
fn _Target_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x218], Target>([0u8; 0x218]);
    }
    unreachable!()
}
impl Target {}
impl std::convert::AsRef<Target> for Target {
    fn as_ref(&self) -> &Target {
        self
    }
}
impl std::convert::AsMut<Target> for Target {
    fn as_mut(&mut self) -> &mut Target {
        self
    }
}
#[repr(C, align(8))]
/// A node of one of the [`TargetSystem`](crate::aim::target::TargetSystem)'s target sets (a `std::set<CTarget *>` red-black tree
/// node). The set's head node is a sentinel with [`m_IsNil`](crate::aim::target::TargetSetNode::m_IsNil) set: its
/// [`m_Left`](crate::aim::target::TargetSetNode::m_Left) is the first element, and an element's missing child points back at it.
pub struct TargetSetNode {
    pub m_Left: *mut crate::aim::target::TargetSetNode,
    pub m_Parent: *mut crate::aim::target::TargetSetNode,
    pub m_Right: *mut crate::aim::target::TargetSetNode,
    pub m_Value: *mut crate::aim::target::Target,
    pub m_Color: u8,
    pub m_IsNil: bool,
    _field_22: [u8; 6],
}
fn _TargetSetNode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x28], TargetSetNode>([0u8; 0x28]);
    }
    unreachable!()
}
impl TargetSetNode {}
impl std::convert::AsRef<TargetSetNode> for TargetSetNode {
    fn as_ref(&self) -> &TargetSetNode {
        self
    }
}
impl std::convert::AsMut<TargetSetNode> for TargetSetNode {
    fn as_mut(&mut self) -> &mut TargetSetNode {
        self
    }
}
#[repr(C, align(8))]
/// The registry of aim candidates (`CTargetSystem`): every live [`Target`](crate::aim::target::Target), kept in one set per
/// kind. [`PlayerAimControl::UpdateAllTargets`](crate::aim::aim::PlayerAimControl::UpdateAllTargets) walks
/// the sets whose include flags the wielded weapon's aim-assist tuning sets, scoring each target
/// for every purpose. Only some members are mapped.
pub struct TargetSystem {
    _field_0: [u8; 20],
    /// Whether the current target walk includes [`m_CharacterTargetsHead`](crate::aim::target::TargetSystem::m_CharacterTargetsHead)'s
    /// set, as the aim control last set it.
    pub m_IncludeCharacters: bool,
    /// Whether the walk includes the vehicle set.
    pub m_IncludeVehicles: bool,
    /// Whether the walk includes the set of other objects.
    pub m_IncludeOtherObjects: bool,
    _field_17: [u8; 9],
    /// The head node of the set of character targets.
    pub m_CharacterTargetsHead: *mut crate::aim::target::TargetSetNode,
    _field_28: [u8; 24],
    /// The head node of the set of vehicle targets.
    pub m_VehicleTargetsHead: *mut crate::aim::target::TargetSetNode,
    _field_48: [u8; 24],
    /// The head node of the set of the other objects' targets.
    pub m_OtherTargetsHead: *mut crate::aim::target::TargetSetNode,
    _field_68: [u8; 16],
}
fn _TargetSystem_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x78], TargetSystem>([0u8; 0x78]);
    }
    unreachable!()
}
impl TargetSystem {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418188832usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl TargetSystem {}
impl std::convert::AsRef<TargetSystem> for TargetSystem {
    fn as_ref(&self) -> &TargetSystem {
        self
    }
}
impl std::convert::AsMut<TargetSystem> for TargetSystem {
    fn as_mut(&mut self) -> &mut TargetSystem {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The aim purpose a target or aim slot serves (`CTarget::ETargetType`). The target system scores
/// every candidate once per purpose, and the player's aim control keeps one slot per purpose in each
/// of its per-purpose arrays (see [`PlayerAimControl`](crate::aim::aim::PlayerAimControl)), indexed by
/// this value.
pub enum TargetType {
    TARGET_WEAPONS = 0isize as _,
    TARGET_STICKY_AIM = 1isize as _,
    TARGET_GRAPPLE = 2isize as _,
    TARGET_MELEE = 3isize as _,
    TARGET_GRENADE = 4isize as _,
    TARGET_SNAP = 5isize as _,
    NOF_TARGET_TYPES = 6isize as _,
}
fn _TargetType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], TargetType>([0u8; 0x4]);
    }
    unreachable!()
}
