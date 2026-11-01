#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The game object that holds a set of interaction graphs (`CInteractionContext`). Only some
/// members are mapped.
pub struct InteractionContext {
    _field_0: [u8; 112],
    /// The game object the context belongs to (`CGameObject::m_Parent`), such as the weapon whose
    /// pickup interaction the context carries.
    pub m_Parent: crate::types::shared_ptr::WeakPtrGameObject,
}
fn _InteractionContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x80], InteractionContext>([0u8; 0x80]);
    }
    unreachable!()
}
impl InteractionContext {}
impl std::convert::AsRef<InteractionContext> for InteractionContext {
    fn as_ref(&self) -> &InteractionContext {
        self
    }
}
impl std::convert::AsMut<InteractionContext> for InteractionContext {
    fn as_mut(&mut self) -> &mut InteractionContext {
        self
    }
}
#[repr(C, align(8))]
/// A graph of interaction nodes (`CInteractionGraph`), owned by an interaction context. Only some
/// members are mapped.
pub struct InteractionGraph {
    _field_0: [u8; 96],
    /// The context that owns the graph.
    pub m_Context: *mut crate::interaction::InteractionContext,
}
fn _InteractionGraph_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x68], InteractionGraph>([0u8; 0x68]);
    }
    unreachable!()
}
impl InteractionGraph {
    pub const GetNodeByIndex_ADDRESS: usize = 0x140A37D60;
    /// The graph's node at `index`, or null when the index is past the graph's last node.
    pub unsafe fn GetNodeByIndex(
        &mut self,
        index: u64,
    ) -> *mut crate::interaction::InteractionGraphNode {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                index: u64,
            ) -> *mut crate::interaction::InteractionGraphNode = ::std::mem::transmute(
                Self::GetNodeByIndex_ADDRESS,
            );
            f(self as *mut Self as _, index)
        }
    }
}
impl std::convert::AsRef<InteractionGraph> for InteractionGraph {
    fn as_ref(&self) -> &InteractionGraph {
        self
    }
}
impl std::convert::AsMut<InteractionGraph> for InteractionGraph {
    fn as_mut(&mut self) -> &mut InteractionGraph {
        self
    }
}
#[repr(C, align(8))]
/// One way into an interaction graph (`CInteractionGraphEntry`). Only some members are mapped.
pub struct InteractionGraphEntry {
    _field_0: [u8; 8],
    /// The index of the graph node the entry leads to, for
    /// [`InteractionGraph::GetNodeByIndex`](crate::interaction::InteractionGraph::GetNodeByIndex).
    pub m_EntryNodeIndex: u64,
    _field_10: [u8; 80],
    /// The graph the entry belongs to.
    pub m_Graph: *mut crate::interaction::InteractionGraph,
}
fn _InteractionGraphEntry_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x68], InteractionGraphEntry>([0u8; 0x68]);
    }
    unreachable!()
}
impl InteractionGraphEntry {}
impl std::convert::AsRef<InteractionGraphEntry> for InteractionGraphEntry {
    fn as_ref(&self) -> &InteractionGraphEntry {
        self
    }
}
impl std::convert::AsMut<InteractionGraphEntry> for InteractionGraphEntry {
    fn as_mut(&mut self) -> &mut InteractionGraphEntry {
        self
    }
}
#[repr(C, align(8))]
/// A `TArray<CInteractionGraphEntry *>`: the start and the end of its element array.
pub struct InteractionGraphEntryArray {
    pub m_Begin: *mut *mut crate::interaction::InteractionGraphEntry,
    pub m_End: *mut *mut crate::interaction::InteractionGraphEntry,
}
fn _InteractionGraphEntryArray_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], InteractionGraphEntryArray>([0u8; 0x10]);
    }
    unreachable!()
}
impl InteractionGraphEntryArray {}
impl std::convert::AsRef<InteractionGraphEntryArray> for InteractionGraphEntryArray {
    fn as_ref(&self) -> &InteractionGraphEntryArray {
        self
    }
}
impl std::convert::AsMut<InteractionGraphEntryArray> for InteractionGraphEntryArray {
    fn as_mut(&mut self) -> &mut InteractionGraphEntryArray {
        self
    }
}
#[repr(C, align(16))]
/// One node of an interaction graph (`CInteractionGraphNode`). Only some members are mapped.
pub struct InteractionGraphNode {
    _field_0: [u8; 80],
    /// The game object the node stands for, such as the weapon placement rule of a weapon rack's
    /// pickup, or null.
    pub m_GameObject: crate::types::shared_ptr::WeakPtrGameObject,
    _field_60: [u8; 16],
}
fn _InteractionGraphNode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], InteractionGraphNode>([0u8; 0x70]);
    }
    unreachable!()
}
impl InteractionGraphNode {}
impl std::convert::AsRef<InteractionGraphNode> for InteractionGraphNode {
    fn as_ref(&self) -> &InteractionGraphNode {
        self
    }
}
impl std::convert::AsMut<InteractionGraphNode> for InteractionGraphNode {
    fn as_mut(&mut self) -> &mut InteractionGraphNode {
        self
    }
}
#[repr(C, align(1))]
/// The interaction graph entry condition `CInteractionGraphParentWeaponNotInInventory`, one of the
/// pickup conditions: a pickup graph that uses it is not offered to a character that already
/// carries a weapon of the same kind in the offered weapon's slot. Only its evaluation is mapped.
pub struct InteractionGraphParentWeaponNotInInventory {
    _field_0: [u8; 8],
}
fn _InteractionGraphParentWeaponNotInInventory_size_check() {
    unsafe {
        ::std::mem::transmute::<
            [u8; 0x8],
            InteractionGraphParentWeaponNotInInventory,
        >([0u8; 0x8]);
    }
    unreachable!()
}
impl InteractionGraphParentWeaponNotInInventory {
    pub const Evaluate_ADDRESS: usize = 0x140A5B6C0;
    /// Evaluates the condition for `user`. It reads the user's entries
    /// ([`InteractionUserProxy::m_PotentialEntries`](crate::interaction::InteractionUserProxy::m_PotentialEntries) while
    /// [`InteractionUserProxy::m_IsValidatingEntry`](crate::interaction::InteractionUserProxy::m_IsValidatingEntry) is set, otherwise
    /// [`InteractionUserProxy::m_ValidEntries`](crate::interaction::InteractionUserProxy::m_ValidEntries)) and takes the weapon the first entry offers, found
    /// as the weapon readout finds it: the entry's graph's context's parent when that is a weapon,
    /// or the weapon spawned by the weapon placement rule the entry's node stands for. It fails
    /// only when the user's character
    /// ([`InteractionUserProxy::m_Character`](crate::interaction::InteractionUserProxy::m_Character)) holds a weapon in that weapon's slot
    /// ([`WeaponBase::m_EquipSlot`](crate::equipment::weapon::WeaponBase::m_EquipSlot)) with the same
    /// [`WeaponBase::m_WeaponId`](crate::equipment::weapon::WeaponBase::m_WeaponId); with no entries, no
    /// weapon found, or an empty slot, it passes. A non-zero `evaluation_type` logs a warning that
    /// the condition is meant for entries alone.
    pub unsafe fn Evaluate(
        &mut self,
        user: *mut crate::interaction::InteractionUserProxy,
        evaluation_type: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                user: *mut crate::interaction::InteractionUserProxy,
                evaluation_type: u32,
            ) -> bool = ::std::mem::transmute(Self::Evaluate_ADDRESS);
            f(self as *mut Self as _, user, evaluation_type)
        }
    }
}
impl std::convert::AsRef<InteractionGraphParentWeaponNotInInventory>
for InteractionGraphParentWeaponNotInInventory {
    fn as_ref(&self) -> &InteractionGraphParentWeaponNotInInventory {
        self
    }
}
impl std::convert::AsMut<InteractionGraphParentWeaponNotInInventory>
for InteractionGraphParentWeaponNotInInventory {
    fn as_mut(&mut self) -> &mut InteractionGraphParentWeaponNotInInventory {
        self
    }
}
#[repr(C, align(8))]
/// A character's user side of the interaction system (`CInteractionUserProxy`), embedded in the
/// character ([`Character::m_InteractionUserProxy`](crate::character::character::Character::m_InteractionUserProxy)).
/// It tracks the interaction graphs near the character and which of their entries the character
/// could take. Only some members are mapped.
pub struct InteractionUserProxy {
    _field_0: [u8; 8],
    /// The character the proxy belongs to.
    pub m_Character: *mut crate::character::character::Character,
    _field_10: [u8; 392],
    /// The entries being validated on this update, which
    /// [`ProcessPotentialEntries`](crate::interaction::InteractionUserProxy::ProcessPotentialEntries) works through and empties. An
    /// entry whose graph's conditions pass becomes one of
    /// [`m_ValidEntries`](crate::interaction::InteractionUserProxy::m_ValidEntries).
    pub m_PotentialEntries: crate::interaction::InteractionGraphEntryArray,
    _field_1a8: [u8; 112],
    /// The entries of nearby interaction graphs that the character could enter now: those whose
    /// conditions passed, such as
    /// [`InteractionGraphParentWeaponNotInInventory`](crate::interaction::InteractionGraphParentWeaponNotInInventory)
    /// on a weapon pickup. The player's weapon readout and a swimming character's weapon pickup
    /// ([`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling))
    /// walk it for the weapon the first entry offers.
    pub m_ValidEntries: crate::interaction::InteractionGraphEntryArray,
    _field_228: [u8; 20],
    /// Set while [`ProcessPotentialEntries`](crate::interaction::InteractionUserProxy::ProcessPotentialEntries) runs, so that a
    /// condition evaluated meanwhile reads [`m_PotentialEntries`](crate::interaction::InteractionUserProxy::m_PotentialEntries) rather
    /// than [`m_ValidEntries`](crate::interaction::InteractionUserProxy::m_ValidEntries).
    pub m_IsValidatingEntry: bool,
    _field_23d: [u8; 3],
}
fn _InteractionUserProxy_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x240], InteractionUserProxy>([0u8; 0x240]);
    }
    unreachable!()
}
impl InteractionUserProxy {
    pub const ProcessPotentialEntries_ADDRESS: usize = 0x140A671F0;
    /// Works through [`m_PotentialEntries`](crate::interaction::InteractionUserProxy::m_PotentialEntries) with
    /// [`m_IsValidatingEntry`](crate::interaction::InteractionUserProxy::m_IsValidatingEntry) set, and empties it: an entry whose
    /// interaction's act the character's animated model can take moves to
    /// [`m_ValidEntries`](crate::interaction::InteractionUserProxy::m_ValidEntries), and any other has its interaction's components
    /// deactivated. Returns whether an entry was moved.
    pub unsafe fn ProcessPotentialEntries(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::ProcessPotentialEntries_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<InteractionUserProxy> for InteractionUserProxy {
    fn as_ref(&self) -> &InteractionUserProxy {
        self
    }
}
impl std::convert::AsMut<InteractionUserProxy> for InteractionUserProxy {
    fn as_mut(&mut self) -> &mut InteractionUserProxy {
        self
    }
}
