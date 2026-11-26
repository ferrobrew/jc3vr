//! The game tick token: proof that the calling code runs on the game thread at one of the frame's
//! serial points, where the world's game objects hold still for the mod's code.
//!
//! A [`GameTick`] exists only inside [`GameTick::enter`], which hands the closure a borrow of it
//! with a fresh lifetime, so neither the tick nor anything borrowed from it can outlive the call.
//! It is neither [`Send`] nor [`Sync`], so it cannot reach another thread, and it is neither
//! [`Clone`] nor [`Copy`]. Code that needs the invariant takes `&GameTick<'_>`, and the handles to
//! game objects built on it (the body crate's `LocalPlayer`) borrow the tick, so they are bounded
//! by the same call.
//!
//! What a tick asserts, for the extent of the [`GameTick::enter`] call:
//!
//! - the calling thread is the game thread, outside the engine's input job, the frame tail, and the
//!   render update's frame scope ([`ThreadRole::GameFrame`]);
//! - the engine runs no other thread that mutates the world's game objects (characters, weapons,
//!   vehicles, inventories, and their physics proxies) at the same time: the world update has not
//!   started, is between its own serial steps, or has finished;
//! - a game object that is live when the tick reaches it stays live until the closure returns,
//!   unless a call the mod itself makes frees it.
//!
//! What it does not assert: that an engine call made inside the tick leaves an object alive. An
//! engine method can free, reallocate, or reparent the object it is called on or others (dropping a
//! weapon unequips and may destroy it, an act can retire a proxy), and it can reenter the mod's own
//! detours, which may reach the same object again. A borrow's lifetime cannot see that, so code that
//! keeps an object across such a call re-resolves it afterwards (a weak reference, or a fresh fetch
//! of the local player) rather than trusting the earlier borrow. Nor does the tick make a game object
//! exclusive to one handle: two handles reached in the same tick name the same engine object, as two
//! engine pointers to it do.
//!
//! The entry points that open a tick are the serial points listed in `docs/mod/threading.md`
//! ("The game tick"). The role check in [`GameTick::enter`] is a debug-build backstop for an entry
//! point placed on the wrong thread; it cannot see whether the world is between serial steps, which
//! is why opening a tick is `unsafe`.

use std::marker::PhantomData;

use crate::thread_role::{self, ThreadRole};

/// Proof that the calling code runs on the game thread at one of the frame's serial points, for the
/// extent of the [`GameTick::enter`] call that made it. Zero-sized; passed by reference.
///
/// The tick cannot leave the call that opened it:
///
/// ```compile_fail
/// # use jc3vr_core::game_tick::GameTick;
/// let escaped = unsafe { GameTick::enter(|game| game) };
/// ```
///
/// nor reach another thread:
///
/// ```compile_fail
/// # use jc3vr_core::game_tick::GameTick;
/// fn send<T: Send>(_: T) {}
/// unsafe { GameTick::enter(|game| send(game)) };
/// ```
pub struct GameTick<'t> {
    /// Invariant in `'t`, so a tick's lifetime can neither shrink nor grow to meet another's.
    _scope: PhantomData<fn(&'t ()) -> &'t ()>,
    /// Neither `Send` nor `Sync`: the invariant is the game thread's.
    _thread_bound: PhantomData<*const ()>,
}

impl GameTick<'_> {
    /// Run `f` with a tick, which lives only for the call.
    ///
    /// # Safety
    ///
    /// The caller is on the game thread at one of the frame's serial points (`docs/mod/threading.md`,
    /// "The game tick"): no engine thread mutates the world's game objects for the extent of `f`,
    /// and none of them is freed in it except by engine calls `f` makes.
    #[inline]
    pub unsafe fn enter<R>(f: impl for<'t> FnOnce(&GameTick<'t>) -> R) -> R {
        debug_assert!(
            admits(thread_role::current()),
            "a game tick was opened on {}",
            thread_role::current().describe()
        );
        f(&GameTick {
            _scope: PhantomData,
            _thread_bound: PhantomData,
        })
    }
}

impl std::fmt::Debug for GameTick<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GameTick")
    }
}

/// Whether a thread in `role` may open a tick: the game thread outside its frame scope, including
/// the debug UI's pass, which runs at the top of the game's update.
pub const fn admits(role: ThreadRole) -> bool {
    matches!(role, ThreadRole::Game | ThreadRole::GameDebugUi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_game_thread_outside_its_frame_scope_admits_a_tick() {
        let admitted: Vec<_> = [
            ThreadRole::Unmarked,
            ThreadRole::Game,
            ThreadRole::GameFrame,
            ThreadRole::GameDebugUi,
            ThreadRole::FrameTail,
            ThreadRole::InputJob,
        ]
        .into_iter()
        .filter(|role| admits(*role))
        .collect();
        assert_eq!(admitted, [ThreadRole::Game, ThreadRole::GameDebugUi]);
    }

    #[test]
    fn a_tick_hands_back_the_closure_result() {
        std::thread::spawn(|| {
            thread_role::mark_thread(ThreadRole::Game);
            // SAFETY: a test thread marked as the game thread, with no game objects to protect.
            let value = unsafe { GameTick::enter(|_tick| 7) };
            assert_eq!(value, 7);
        })
        .join()
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "a game tick was opened on the engine's input job")]
    #[cfg(debug_assertions)]
    fn the_input_job_may_not_open_a_tick() {
        thread_role::mark_thread(ThreadRole::Game);
        let _job = thread_role::enter(ThreadRole::InputJob);
        // SAFETY: the debug check refuses before the closure runs.
        unsafe { GameTick::enter(|_tick| ()) };
    }
}
