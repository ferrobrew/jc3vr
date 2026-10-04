//! Thread roles: which part of the frame the calling thread is running, for the locks whose safety
//! depends on it.
//!
//! The VR runtime state ([`crate::vr`]) is locked by the game thread for the whole of a frame and
//! then by the frame tail until the frame is submitted, while both wait on engine threads (the draw
//! task, CPU fragment workers, the UI capture and render workers). A blocking lock of it from any of
//! those threads, or from the game thread while it already holds it, is a deadlock. Which threads
//! may take it is a property of the thread *and* of where in the frame it is, so a thread carries a
//! role: a base role marked once ([`mark_thread`]) and narrowed for a dynamic extent by a
//! [`RoleScope`] ([`enter`]). Unmarked threads -- every engine thread the mod does not own -- have
//! [`ThreadRole::Unmarked`], so a hook that runs somewhere unexpected is caught without having been
//! marked. See `docs/mod/threading.md` for the thread model and the lock rules.
//!
//! The role is a thread-local cell, so reading it costs a TLS access.

use std::{cell::Cell, marker::PhantomData};

/// What the calling thread is doing, as far as the lock rules care.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ThreadRole {
    /// A thread the mod has not marked: the engine's draw task, its CPU fragment workers, the UI
    /// capture and render workers, physics workers, and any other engine or system thread.
    Unmarked,
    /// The game thread outside the scopes below: the top of `CGame::Update` and the top of the
    /// render update, before the frame takes the VR runtime lock.
    Game,
    /// The game thread from `vr::frame_begin` to the end of the render update: it holds the VR
    /// runtime lock (or would, in a frame that has one) and waits on the draw task and the CPU
    /// fragment workers, and runs stolen fragments itself while it waits.
    GameFrame,
    /// The game thread while it runs the debug UI, holding the egui state lock the draw task also
    /// takes.
    GameDebugUi,
    /// The frame-tail worker ([`crate::vr::tail`]): it owns the frame's VR runtime lock until it
    /// submits.
    FrameTail,
}

impl ThreadRole {
    /// Whether a thread in this role may block on the VR runtime lock. Only the game thread outside
    /// a frame and outside the debug UI may: everywhere else either already holds the lock or is
    /// something a holder waits on.
    pub(crate) const fn may_block_on_vr_state(self) -> bool {
        matches!(self, Self::Game)
    }

    /// A short description of the role, for diagnostics.
    pub(crate) const fn describe(self) -> &'static str {
        match self {
            Self::Unmarked => "an engine or worker thread (not the game thread)",
            Self::Game => "the game thread",
            Self::GameFrame => "the game thread inside the frame",
            Self::GameDebugUi => "the game thread inside the debug UI",
            Self::FrameTail => "the frame-tail worker",
        }
    }
}

/// The calling thread's current role.
pub(crate) fn current() -> ThreadRole {
    ROLE.with(Cell::get)
}

/// Mark the calling thread's base role. A thread that already has a role keeps it, so the game
/// thread can mark itself at the top of every frame without clobbering a scope it is inside.
pub(crate) fn mark_thread(role: ThreadRole) {
    ROLE.with(|cell| {
        if cell.get() == ThreadRole::Unmarked {
            cell.set(role);
        }
    });
}

/// Narrow the calling thread's role to `role` until the returned scope drops, which restores the
/// role it replaced.
#[must_use = "the role lasts only as long as the scope is held"]
pub(crate) fn enter(role: ThreadRole) -> RoleScope {
    let previous = ROLE.with(|cell| cell.replace(role));
    RoleScope {
        previous,
        _thread_bound: PhantomData,
    }
}

/// A role set by [`enter`]. It restores the previous role when dropped, and is bound to the thread
/// that created it, since the role it restores is that thread's.
pub(crate) struct RoleScope {
    previous: ThreadRole,
    _thread_bound: PhantomData<*const ()>,
}

impl Drop for RoleScope {
    fn drop(&mut self) {
        ROLE.with(|cell| cell.set(self.previous));
    }
}

thread_local! {
    static ROLE: Cell<ThreadRole> = const { Cell::new(ThreadRole::Unmarked) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_thread_is_unmarked() {
        std::thread::spawn(|| assert_eq!(current(), ThreadRole::Unmarked))
            .join()
            .unwrap();
    }

    #[test]
    fn marking_keeps_an_existing_role() {
        std::thread::spawn(|| {
            mark_thread(ThreadRole::Game);
            mark_thread(ThreadRole::FrameTail);
            assert_eq!(current(), ThreadRole::Game);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn marking_inside_a_scope_does_not_override_it() {
        std::thread::spawn(|| {
            mark_thread(ThreadRole::Game);
            let scope = enter(ThreadRole::GameFrame);
            mark_thread(ThreadRole::Game);
            assert_eq!(current(), ThreadRole::GameFrame);
            drop(scope);
            assert_eq!(current(), ThreadRole::Game);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn nested_scopes_restore_in_order() {
        std::thread::spawn(|| {
            mark_thread(ThreadRole::Game);
            let outer = enter(ThreadRole::GameFrame);
            {
                let _inner = enter(ThreadRole::GameDebugUi);
                assert_eq!(current(), ThreadRole::GameDebugUi);
            }
            assert_eq!(current(), ThreadRole::GameFrame);
            drop(outer);
            assert_eq!(current(), ThreadRole::Game);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn roles_are_per_thread() {
        std::thread::spawn(|| {
            mark_thread(ThreadRole::Game);
            let _frame = enter(ThreadRole::GameFrame);
            std::thread::spawn(|| assert_eq!(current(), ThreadRole::Unmarked))
                .join()
                .unwrap();
            assert_eq!(current(), ThreadRole::GameFrame);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn only_the_game_thread_outside_its_scopes_may_block_on_the_vr_state() {
        let allowed: Vec<_> = [
            ThreadRole::Unmarked,
            ThreadRole::Game,
            ThreadRole::GameFrame,
            ThreadRole::GameDebugUi,
            ThreadRole::FrameTail,
        ]
        .into_iter()
        .filter(|role| role.may_block_on_vr_state())
        .collect();
        assert_eq!(allowed, [ThreadRole::Game]);
    }
}
