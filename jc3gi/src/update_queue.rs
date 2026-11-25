#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `CUpdateQueue`: the game world's queue of per-object update functions, kept sorted by
/// priority. The pre-simulation, post-simulation, post-camera, and render updates are four
/// priority bands of the one queue, each run by [`Execute`](crate::update_queue::UpdateQueue::Execute) with its own range.
///
/// Registering an update (`CGameObject::AddToUpdate`, which registers the object's
/// pre-simulation, post-simulation, and render updates as its `GetRequiredUpdates` asks) only
/// appends a pending entry and marks the queue dirty; the entry joins the queue, and starts being
/// run, at the next [`CommitAddRemove`](crate::update_queue::UpdateQueue::CommitAddRemove). Unregistering clears the entry at
/// once, so a removed object is skipped from then on. The world commits at the top of its
/// pre-simulation update
/// ([`GameWorld::UpdatePreSimImpl`](crate::game_world::GameWorld::UpdatePreSimImpl)), before that band
/// runs, and at the end of its render update. An object registered before the pre-simulation
/// commit is therefore run by the same tick's pre-simulation and post-simulation bands, and one
/// registered after it, from inside the pre-simulation band onward, is first run in the next
/// tick. Its layout is unmapped.
pub struct UpdateQueue {}
impl UpdateQueue {
    pub const Execute_ADDRESS: usize = 0x140552700;
    /// Runs the registered updates whose priority lies in `prio_min..=prio_max`, in priority
    /// order, with `dt` and `context` (a `SGameObjectUpdateContext` or `SGameObjectRenderContext`,
    /// as the band expects). An update function that returns false is unregistered. It does not
    /// commit pending registrations.
    pub unsafe fn Execute(
        &mut self,
        dt: f32,
        prio_min: i32,
        prio_max: i32,
        context: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                prio_min: i32,
                prio_max: i32,
                context: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::Execute_ADDRESS);
            f(self as *mut Self as _, dt, prio_min, prio_max, context)
        }
    }
    pub const CommitAddRemove_ADDRESS: usize = 0x140552A90;
    /// Under the queue's registration lock, when the queue is dirty, drops the cleared entries,
    /// merges the pending registrations into the queue in priority order, and rebuilds the handle
    /// lookup. The disassembler mislabels it `CUpdateQueue::ValidateEmpty`.
    pub unsafe fn CommitAddRemove(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::CommitAddRemove_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<UpdateQueue> for UpdateQueue {
    fn as_ref(&self) -> &UpdateQueue {
        self
    }
}
impl std::convert::AsMut<UpdateQueue> for UpdateQueue {
    fn as_mut(&mut self) -> &mut UpdateQueue {
        self
    }
}
