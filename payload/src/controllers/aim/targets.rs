//! The game's own aim candidates: the [`TargetSystem`]'s sets of [`Target`]s, which the player aim
//! control scores for every purpose in
//! [`PlayerAimControl::UpdateAllTargets`](jc3gi::aim::aim::PlayerAimControl::UpdateAllTargets).
//! Per-hand aim assist ([`crate::controllers::aim::assist`]) and physical melee
//! ([`crate::controllers::weapons::melee`]) choose among the same candidates the game does.
//!
//! Read them only on the thread that runs the player's aim update, where the game walks them too.

use std::ffi::c_void;

use glam::Vec3;
use jc3gi::{
    aim::{
        aim::PlayerAimControl,
        target::{Target, TargetSetNode, TargetSystem},
    },
    types::{math::Vector3, shared_ptr::WeakPtrGameObject},
};

/// One of the target system's sets.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TargetSet {
    Characters,
    Vehicles,
    OtherObjects,
}

impl TargetSet {
    pub const ALL: [TargetSet; 3] = [
        TargetSet::Characters,
        TargetSet::Vehicles,
        TargetSet::OtherObjects,
    ];
}

/// The targets in `set`, or none outside a loaded world.
///
/// # Safety
///
/// Must run on the thread that runs the player's aim update, while no target is being added or
/// removed.
pub unsafe fn targets(set: TargetSet) -> Vec<*mut Target> {
    let Some(system) = (unsafe { TargetSystem::get() }) else {
        return Vec::new();
    };
    let head = match set {
        TargetSet::Characters => system.m_CharacterTargetsHead,
        TargetSet::Vehicles => system.m_VehicleTargetsHead,
        TargetSet::OtherObjects => system.m_OtherTargetsHead,
    };
    unsafe { walk(head) }
}

/// The sets the aim control's last target pass included, as the wielded weapon's aim-assist tuning
/// chose them, indexed like [`TargetSet::ALL`].
///
/// # Safety
///
/// As [`targets`].
pub unsafe fn included_sets() -> [bool; 3] {
    match unsafe { TargetSystem::get() } {
        Some(s) => [
            s.m_IncludeCharacters,
            s.m_IncludeVehicles,
            s.m_IncludeOtherObjects,
        ],
        None => [false; 3],
    }
}

/// The target's game object (its `CGameObject` subobject), while it is alive.
///
/// # Safety
///
/// `target` must be a live target, read as [`targets`] is.
pub unsafe fn game_object(target: *mut Target) -> Option<*mut c_void> {
    let target = unsafe { target.as_ref() }?;
    unsafe { weak_target(&target.m_GameObject) }
}

/// The target's body position as the game's target selection uses it.
///
/// # Safety
///
/// As [`game_object`].
pub unsafe fn body_position(target: *mut Target) -> Option<Vec3> {
    let mut out = Vector3 { data: [0.0; 3] };
    unsafe { PlayerAimControl::GetTargetBodyPosition(&mut out, target, false, false, false) };
    let v = Vec3::from_array(out.data);
    v.is_finite().then_some(v)
}

/// The object a weak reference points at, while any strong reference to it remains.
///
/// # Safety
///
/// The reference's control block, when present, must be live (the weak reference keeps it so).
pub unsafe fn weak_target(weak: &WeakPtrGameObject) -> Option<*mut c_void> {
    /// The strong count's offset in a `boost::detail::sp_counted_base`, after its vtable.
    const USE_COUNT_OFFSET: usize = 8;
    if weak.px.is_null() || weak.pn.is_null() {
        return None;
    }
    let uses = unsafe {
        weak.pn
            .cast::<u8>()
            .add(USE_COUNT_OFFSET)
            .cast::<i32>()
            .read_volatile()
    };
    (uses > 0).then_some(weak.px)
}

/// The most elements a walk visits, a bound against a corrupt tree.
const MAX_TARGETS: usize = 4096;

/// The values of the red-black tree whose sentinel head is `head`, in order.
///
/// # Safety
///
/// `head` must be null or a live set's head node, with no concurrent modification.
unsafe fn walk(head: *mut TargetSetNode) -> Vec<*mut Target> {
    let mut out = Vec::new();
    let Some(head) = (unsafe { head.as_ref() }) else {
        return out;
    };
    let mut node = head.m_Left;
    while let Some(n) = unsafe { node.as_ref() }
        && !n.m_IsNil
        && out.len() < MAX_TARGETS
    {
        out.push(n.m_Value);
        node = unsafe { successor(node) };
    }
    out
}

/// The in-order successor of `node`: the leftmost node of its right subtree, or else the first
/// ancestor it lies left of (the head once the walk is done).
///
/// # Safety
///
/// `node` must be a live, non-sentinel node of a well-formed tree.
unsafe fn successor(node: *mut TargetSetNode) -> *mut TargetSetNode {
    unsafe {
        let right = (*node).m_Right;
        if !(*right).m_IsNil {
            let mut next = right;
            while !(*(*next).m_Left).m_IsNil {
                next = (*next).m_Left;
            }
            return next;
        }
        let mut child = node;
        let mut parent = (*node).m_Parent;
        while !(*parent).m_IsNil && child == (*parent).m_Right {
            child = parent;
            parent = (*parent).m_Parent;
        }
        parent
    }
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    fn node(value: usize, nil: bool) -> Box<TargetSetNode> {
        // SAFETY: every field is a pointer or an integer, for which zero is valid.
        let mut n: Box<TargetSetNode> = Box::new(unsafe { std::mem::zeroed() });
        n.m_Value = value as *mut Target;
        n.m_IsNil = nil;
        n
    }

    #[test]
    fn a_set_is_walked_in_order() {
        // The tree 2(1, 4(3, 5)), with the MSVC sentinel head: the root's parent, every missing
        // child, and the head's left pointing at the first element.
        let mut head = node(0, true);
        let mut n = [1, 2, 3, 4, 5].map(|v| node(v, false));
        let head_p: *mut TargetSetNode = &mut *head;
        let p: Vec<*mut TargetSetNode> = n.iter_mut().map(|b| &mut **b as *mut _).collect();
        let link = |parent: usize, left: Option<usize>, right: Option<usize>| {
            let node = p[parent];
            unsafe {
                (*node).m_Left = left.map_or(head_p, |l| p[l]);
                (*node).m_Right = right.map_or(head_p, |r| p[r]);
                for child in [left, right].into_iter().flatten() {
                    let child = p[child];
                    (*child).m_Parent = node;
                }
            }
        };
        link(1, Some(0), Some(3));
        link(0, None, None);
        link(3, Some(2), Some(4));
        link(2, None, None);
        link(4, None, None);
        let (root, first, last) = (p[1], p[0], p[4]);
        unsafe {
            (*root).m_Parent = head_p;
            (*head_p).m_Left = first;
            (*head_p).m_Parent = root;
            (*head_p).m_Right = last;
        }
        let values = unsafe { walk(head_p) }
            .into_iter()
            .map(|t| t as usize)
            .collect::<Vec<_>>();
        assert_eq!(values, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn an_empty_set_has_no_targets() {
        let mut head = node(0, true);
        let head_p: *mut TargetSetNode = &mut *head;
        unsafe {
            (*head_p).m_Left = head_p;
            (*head_p).m_Parent = head_p;
            (*head_p).m_Right = head_p;
        }
        assert!(unsafe { walk(head_p) }.is_empty());
        assert!(unsafe { walk(ptr::null_mut()) }.is_empty());
    }
}
