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
#[repr(C, align(8))]
/// A character's user side of the interaction system (`CInteractionUserProxy`), embedded in the
/// character ([`Character::m_InteractionUserProxy`](crate::character::character::Character::m_InteractionUserProxy)).
/// It tracks the interaction graphs near the character and which of their entries the character
/// could take. Only some members are mapped.
pub struct InteractionUserProxy {
    _field_0: [u8; 536],
    /// The entries of nearby interaction graphs that the character could enter now. The player's
    /// weapon readout and a swimming character's weapon pickup
    /// ([`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling))
    /// walk it for the weapon the first entry offers.
    pub m_ValidEntries: crate::interaction::InteractionGraphEntryArray,
}
fn _InteractionUserProxy_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x228], InteractionUserProxy>([0u8; 0x228]);
    }
    unreachable!()
}
impl InteractionUserProxy {}
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
