//! References to game objects the mod holds across ticks: a weak reference that tells whether the
//! object still lives without keeping it alive, and a strong one that keeps it alive while the mod
//! acts on it.
//!
//! The game hands its objects out through `boost` smart pointers, whose control blocks it owns.
//! Both references here go through the game's own operations on them
//! ([`WeakPtrGameObject::Assign`], [`WeakPtrGameObject::Lock`], and [`ReleaseSharedPtrGameObject`]),
//! so the counts stay the engine's to manage. The counts are atomic, so either reference may be
//! taken on a simulation worker thread and released on the main thread.

use std::ptr;

use jc3gi::{
    game::GameObject,
    types::shared_ptr::{ReleaseSharedPtrGameObject, SharedPtr, WeakPtrGameObject},
};

/// A weak reference of the mod's own to a game object.
pub(crate) struct Weak(WeakPtrGameObject);

// SAFETY: the reference holds only a pointer and an atomically counted control block, and the
// object is only reached through `lock`, which takes a strong reference first.
unsafe impl Send for Weak {}

impl Weak {
    /// A weak reference to what `source` refers to.
    ///
    /// # Safety
    ///
    /// `source` must be a live weak reference.
    pub(crate) unsafe fn of(source: &WeakPtrGameObject) -> Self {
        let mut weak = Weak(null_weak());
        unsafe { weak.0.Assign(source) };
        weak
    }

    /// A strong reference to the object, while it still lives.
    pub(crate) fn lock(&self) -> Option<Strong> {
        // SAFETY: the weak reference keeps its control block alive.
        unsafe { Strong::lock(&self.0) }
    }

    /// The engine weak reference this holds, for an operation that reads or copies it.
    pub(crate) fn raw(&self) -> &WeakPtrGameObject {
        &self.0
    }
}

impl Clone for Weak {
    /// A second weak reference to the same object, with its own count.
    fn clone(&self) -> Self {
        let mut weak = Weak(null_weak());
        // SAFETY: the source is a live reference, and the engine's copy takes its own count.
        unsafe { weak.0.Assign(&self.0) };
        weak
    }
}

impl Drop for Weak {
    fn drop(&mut self) {
        if self.0.pn.is_null() {
            return;
        }
        let empty = null_weak();
        // SAFETY: the reference is the mod's own; assigning an empty one releases it.
        unsafe { self.0.Assign(&empty) };
    }
}

/// A strong reference of the mod's own to a game object, which keeps it alive until dropped.
pub(crate) struct Strong(SharedPtr<GameObject>);

// SAFETY: as for `Weak`; the object stays alive while the reference is held, and the mod acts on it
// only on the threads its own rules allow.
unsafe impl Send for Strong {}

impl Strong {
    /// A strong reference to what the engine's weak reference `weak` refers to, while it still
    /// lives.
    ///
    /// # Safety
    ///
    /// `weak` must be a live weak reference.
    pub(crate) unsafe fn lock(weak: &WeakPtrGameObject) -> Option<Self> {
        let mut strong = Strong(null_shared());
        unsafe { weak.Lock(&mut strong.0) };
        (!strong.0.px.is_null()).then_some(strong)
    }

    /// Take over a strong reference the engine writes through `fill`, which nothing else
    /// releases.
    ///
    /// # Safety
    ///
    /// `fill` must leave a strong reference, or a null one, in the pointer it is given.
    pub(crate) unsafe fn filled(fill: impl FnOnce(*mut SharedPtr<GameObject>)) -> Option<Self> {
        let mut strong = Strong(null_shared());
        fill(&mut strong.0);
        (!strong.0.px.is_null()).then_some(strong)
    }

    /// Take over the strong reference `shared`, which the caller no longer releases.
    ///
    /// # Safety
    ///
    /// `shared` must be a strong reference whose release falls to the caller.
    pub(crate) unsafe fn adopt(shared: SharedPtr<GameObject>) -> Option<Self> {
        (!shared.px.is_null()).then_some(Strong(shared))
    }

    /// Give the strong reference up to a callee that releases it.
    pub(crate) fn into_raw(self) -> SharedPtr<GameObject> {
        let shared = SharedPtr {
            px: self.0.px,
            pn: self.0.pn,
        };
        std::mem::forget(self);
        shared
    }

    /// The object's `CGameObject` subobject.
    pub(crate) fn object(&self) -> *mut GameObject {
        self.0.px
    }

    /// The reference's control block, for a non-owning copy of the reference that the engine
    /// copies from, valid while this one is held.
    pub(crate) fn control_block(&self) -> *mut std::ffi::c_void {
        self.0.pn
    }
}

impl Drop for Strong {
    fn drop(&mut self) {
        if self.0.pn.is_null() {
            return;
        }
        // SAFETY: the reference is the mod's own.
        unsafe { ReleaseSharedPtrGameObject(&mut self.0) };
    }
}

/// An empty strong reference, for the engine to write one into.
fn null_shared() -> SharedPtr<GameObject> {
    // SAFETY: a shared pointer is two pointers, for which zero is the empty value.
    unsafe { std::mem::zeroed() }
}

fn null_weak() -> WeakPtrGameObject {
    WeakPtrGameObject {
        px: ptr::null_mut(),
        pn: ptr::null_mut(),
    }
}
