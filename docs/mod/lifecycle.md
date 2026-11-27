# Shutdown preparation

The shutdown request path in `crates/jc3vr-core/src/lifecycle/mod.rs` separates reversible preparation from irreversible teardown. Repeated requests coalesce. Each attempt has a request generation, an attempt generation, and a participant-registration revision. Deferred or failed preparation leaves the shutdown flag unset, preserves ordinary cleanup registrations, and returns to normal frame work. A retry interval bounds preparation work; it does not prove native callback quiescence.

Participants register before enabling work that requires shutdown preparation, each under a name (`ParticipantId`) that the subsystem owning it supplies, so the lifecycle names no subsystem above it. They prepare in registration order, and the first that defers or fails ends the attempt. Preparation runs outside the lifecycle mutex. A ready participant returns a guard that retains the required exclusion barrier and validates immediately before commit. Dropping an uncommitted guard rolls preparation back. Stale generations or a changed registry cannot authorize shutdown. After every guard validates, infallible finalizers run while the barriers remain held. Only then does the lifecycle set the shutdown flag and permit ordinary reverse-order cleanup, worker shutdown, hook removal, and module exit.

The human hull's participant restores the narrowed hulls in the native physics phase before it returns a ready guard, or, after a bounded number of attempts, commits with them in place ([human-hull-restoration.md](body/human-hull-restoration.md)). An unavailable or blocked restoration keeps gameplay and its required hooks active.

## Hook installation and removal

`crate::composition` is the one place that names every module with detours. At startup it runs each module's registrations, then hands `hooks::install` the composed hook library. A module registers its shutdown participants with the lifecycle, and its hook steps with `hooks`: `register_post_enable` for admission that may open only once its detour is live, `register_pre_unpatch` for detours managed outside the hook library (the single-pass COM-vtable detours) and for refusals (an armed texture census), and `register_post_unpatch` for the checks that no native callback is still running in payload code once every patch is reverted. Each kind runs in registration order, and a refusing step stops removal and keeps the image mapped. The libraries compose in a fixed enable order, which a unit test pins, because detour order and removal order both bear on startup and ejection timing.

## Teardown after commit

The game thread runs the cleanups and then hands the rest to a shutdown thread (`shutdown_startup` in `payload/src/lib.rs`). That thread stops the worker threads, waits for the engine to run a few frames, removes the hooks, waits a few frames again, removes the crash handler, and unmaps the module.

Every thread the payload spawns registers its stop with `lifecycle::register_worker`, and `lifecycle::stop_workers` runs the stops once, in registration order. A stop closes its worker's admission, waits for the thread within the worker's own budget, and reports whether the thread is gone. A worker that does not stop is logged by name, the remaining workers still stop, and the module is pinned instead of unmapped. The payload registers every worker at startup from one table (`payload/src/teardown.rs`), whose module documentation says which positions matter and why: the frame tail first, since the recorder captures from the mirror it presents, and the test tools before the settings writers, since they put back the configuration they overrode. A unit test pins both.

The two settle waits (`lifecycle::settle_frames`) count the engine's own per-frame counter, which advances in the `Draw` prologue whether or not a detour is installed. The first lets the still-installed hooks carry out what the cleanups requested, such as the native-resolution restore ([vr-runtime.md](vr-runtime.md)). The second lets a detour that was entered before the hooks came out return. Each wait has a budget in `crates/jc3vr-core/tuning/core.toml`, because outside VR the frame loop stops while the window is in the background, and a wait that runs out logs it and continues.

## Inventory spawn callbacks

Inventory restore and second-pistol requests hand the engine a payload callback, and nothing the engine reports proves that a queued callback has retired. The participant in `controllers/weapons/inventory/spawn_safety/` latches the first such exposure for the rest of the process, but the exposure does not veto an unload: an unload that can never run cannot show whether the risk is real. Preparation and the post-unpatch check log a warning naming the risk (a callback the engine still holds would crash the game once the image unmaps) and continue. They defer only while a submission or callback is executing payload code at that moment, which is transient, and preparation closes admission so no new request starts.

## Magazine executor restriction

`controllers::weapons::reload::shutdown` opens magazine admission once the hooks are enabled and closes it after removal. Shutdown preparation for the magazine executor runs only on the game thread. It cancels a magazine extraction in progress, then defers while the reload executor has not drained or a native magazine callback entry is active.

The hook library's disable operation does not establish callback completion. A snapshot of suspended threads also does not prove that a callback cannot resume in removed code. A future executor adapter must retain a proven barrier through removal or provide an equivalent lifetime guarantee. This restriction does not certify the pre-existing removal behavior of every other hook.

## Fatal failures

Initialization failures and caught update panics enter a fatal lifecycle state. The module remains loaded, ordinary cleanups remain registered, and subsequent mod updates stop. This is containment, not a claim of safe continuing gameplay or safe ejection. A game restart is required. A cleanup panic or hook-removal failure after commit also retains the module rather than ejecting it.

## Verification

Isolated lifecycle tests cover coalesced requests, bounded retries, normal-frame continuation on deferral, partial preparation rollback, stale tokens, participant-generation changes, registration exclusivity, cleanup ordering, and one-shot commit. The inventory spawn participant's tests run it in an isolated lifecycle beside the others. Magazine preparation tests cover busy drains and rejection of an installed callback without an entry-exclusion contract. The worker teardown's tests cover the registration order, a stalled stop that does not end the teardown, refused late or duplicate registrations, and the settle wait's budget; the payload's teardown table test pins its two ordered positions. These tests do not prove native callback exclusion, collider clearance, or live ejection and reinjection safety.
