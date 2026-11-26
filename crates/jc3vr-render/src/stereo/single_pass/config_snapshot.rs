//! The single-pass config snapshot: the flags the render path reads, sampled off the mutex-guarded
//! configuration and held in an atomic bitfield.
//!
//! Two snapshots, for one reason. Every gate in the feature is consulted from viewport, draw, and
//! render-block detours, so reading the configuration behind its mutex would cost a lock per draw
//! submission -- the exact cost single-pass exists to remove. And with the frame tail deferred, the
//! game thread is a frame ahead of the draw thread, so a flag that moved between an arm and its
//! restore would leave that state raised for the rest of the frame. The game thread therefore
//! refreshes a process-wide snapshot per frame, and the draw thread pins its own per dispatch.
//!
//! The master switch is taken together with whether a VR session is running, latched at the same
//! frame refresh: single-pass renders both eyes into the halves of a double-wide target sized from
//! the headset's views, so without a session it stays inert and the game renders as it ships.
//!
//! Each sample also publishes the stereo layout its flags settle ([`jc3vr_view::stereo::layout`]), to the
//! frame snapshot and the dispatch pin alike, for the code that sizes and poses by the layout
//! without reading the single-pass flags.

use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use jc3vr_view::stereo::{
    config::{STEREO_CONFIG, SinglePassSwitch},
    layout,
    layout::LayoutFlags,
};

/// The single-pass config flags, sampled once per frame and pinned for the duration of a dispatch:
/// every switch at its [`SinglePassSwitch`] index (from
/// [`SinglePassConfig::switch_bits`](jc3vr_view::stereo::config::SinglePassConfig::switch_bits)),
/// and the master switch taken together with the session at [`SINGLE_PASS_BIT`].
#[derive(Clone, Copy)]
pub(super) struct ConfigFlags(u64);

impl ConfigFlags {
    pub(super) fn has(self, switch: SinglePassSwitch) -> bool {
        self.0 & (1 << switch as u64) != 0
    }

    /// Whether the master switch is on and a VR session is running.
    pub(super) fn single_pass(self) -> bool {
        self.0 & SINGLE_PASS_BIT != 0
    }

    /// The stereo layout these flags ask for, as [`layout`] publishes it.
    fn layout(self) -> LayoutFlags {
        LayoutFlags::new(self.single_pass())
    }
}

/// The flags this draw thread's dispatch was opened with, else the game thread's frame snapshot, else
/// a fresh read if neither has been taken yet.
///
/// Every gate in this module is consulted from the render thread on paths that run per draw -- the
/// viewport, indexed-draw and render-block detours -- so reading them through `Config`'s mutex made
/// the feature pay a lock acquisition per draw submission, in a feature whose purpose is to reduce
/// per-draw cost.
///
/// There are two snapshots because the two consumers want different things, and one snapshot cannot
/// give both. The **draw thread** wants a value that does not move for the whole dispatch: nearly
/// every gate here brackets something (a pass range, an eye-half viewport, an armed reprojection,
/// a per-eye re-issue loop), and a flag that changes between the arm and the restore leaks that state
/// into the rest of the frame -- so it pins its own copy in [`DISPATCH_FLAGS`] at
/// [`begin_dispatch`]. Everything else -- the game thread's resolution and camera decisions, and the
/// shader-creation hooks, which must see a toggle in the *same* frame the debug UI turns it on,
/// because that frame's shader reload is what applies it -- reads [`CONFIG_FLAGS`], resampled at frame
/// start ([`refresh_config_flags`]).
///
/// The pin ends with the dispatch ([`end_dispatch`]): the draw thread is a job-pool worker that runs
/// game-side jobs between dispatches, and those read the frame snapshot like everything else.
///
/// The frame snapshot deliberately does **not** reach the draw thread mid-dispatch. It is written on
/// the game thread, which with the frame tail deferred runs concurrently with the previous
/// frame's still-walking dispatch -- the same interleaving that made the game-thread G-buffer range
/// clear tear a live range (see [`begin_dispatch`]). A flag changing there would land between an arm
/// and its restore, which is why the draw thread reads a pinned copy instead.
pub(super) fn config_flags() -> ConfigFlags {
    if let Some(flags) = DISPATCH_FLAGS.get() {
        return flags;
    }
    let snapshot = CONFIG_FLAGS.load(Ordering::Relaxed);
    if snapshot & CONFIG_FLAGS_VALID != 0 {
        return ConfigFlags(snapshot);
    }
    ConfigFlags(store_config_flags())
}

/// Re-read the single-pass config flags into the frame snapshot. Called at frame start, on the game
/// thread, and read by everything that is not inside a draw-thread dispatch -- see [`config_flags`]
/// for why the draw thread pins its own copy instead of reading this one.
///
/// It also latches whether a VR session is running, which the master switch is taken together
/// with. The session state is the one the previous frame's runtime pump published, so a session
/// that starts or stops takes effect from the next frame's refresh, for every reader of that frame
/// at once.
pub fn refresh_config_flags() {
    SESSION_LIVE.store(jc3vr_view::vr::is_running_published(), Ordering::Relaxed);
    store_config_flags();
}

/// Pin the config flags for the dispatch this thread is opening, so every gate it consults answers
/// the same for the whole of it. Called from [`begin_dispatch`].
pub(super) fn pin_dispatch_config_flags() {
    let flags = ConfigFlags(store_config_flags());
    DISPATCH_FLAGS.set(Some(flags));
    layout::pin_dispatch_layout(flags.layout());
}

/// The end of the dispatch this thread opened: drops the pinned flags when it goes out of scope, so
/// whatever this thread runs between dispatches reads the frame snapshot like any other thread.
///
/// The engine runs its draw dispatch on a worker of its job pool, and the pool's workers run
/// game-side jobs between dispatches too. A pin left in place outlived its dispatch on that worker:
/// a job it later ran read the flags of whatever dispatch the worker last drew, so the
/// water's reflection camera update, which runs as such a job, saw the collapse off on the frames it
/// landed on a worker last used for a multipass dispatch, and sized the reflection to the double-wide
/// target.
pub struct DispatchEnd(());

/// Open the scope that ends this thread's dispatch pin; held for the dispatch's last seam.
pub fn end_dispatch() -> DispatchEnd {
    DispatchEnd(())
}

impl Drop for DispatchEnd {
    fn drop(&mut self) {
        DISPATCH_FLAGS.set(None);
        layout::unpin_dispatch_layout();
    }
}

/// Whether a VR session was running at the last [`refresh_config_flags`]. Off until the first
/// refresh, so nothing single-pass runs before the frame driver has seen a session.
static SESSION_LIVE: AtomicBool = AtomicBool::new(false);

fn store_config_flags() -> u64 {
    let session_live = SESSION_LIVE.load(Ordering::Relaxed);
    let snapshot = STEREO_CONFIG.query(|c| {
        let s = &c.single_pass;
        s.switch_bits()
            | if s.enabled && session_live {
                SINGLE_PASS_BIT
            } else {
                0
            }
    }) | CONFIG_FLAGS_VALID;
    CONFIG_FLAGS.store(snapshot, Ordering::Relaxed);
    layout::publish_frame_layout(ConfigFlags(snapshot).layout());
    snapshot
}

/// The master switch's bit, just past the switches'.
const SINGLE_PASS_BIT: u64 = 1 << SinglePassSwitch::COUNT;
/// Marks [`CONFIG_FLAGS`] as having been sampled at least once, so a zero snapshot before the first
/// frame is not mistaken for "everything off".
const CONFIG_FLAGS_VALID: u64 = 1 << 63;
const _: () = assert!(SINGLE_PASS_BIT < CONFIG_FLAGS_VALID);
static CONFIG_FLAGS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The flags [`begin_dispatch`] pinned for the dispatch in flight on this thread, or `None` on a
    /// thread that has never opened one. Thread-local rather than global because the point is to keep
    /// the draw thread's view of the config still while the game thread is free to resample its own.
    static DISPATCH_FLAGS: Cell<Option<ConfigFlags>> = const { Cell::new(None) };
}
