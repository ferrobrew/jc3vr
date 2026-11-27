# Lifecycle

The payload starts on the injector's thread, finishes its initialization on the game thread, and ejects in two phases: a reversible preparation that may defer, then an irreversible teardown. The startup and shutdown order lives in `payload/src/lib.rs`; the lifecycle (`crates/jc3vr-core/src/lifecycle/`) holds the preparation, the cleanups, and the worker stops without naming any subsystem.

## Startup

The injector calls the payload's `run` export, or `run_source_tuning` to read the tuning from the source tree ([tuning.md](tuning.md#startup)). `initialize_startup` then runs, in order:

1. The session folder ([sessions.md](sessions.md)), first, so the log and every other artifact land under it, then the panic hook and logging.
2. The worker table (`teardown::register`), while nothing has started that an eject would have to stop.
3. The tuning, before anything reads it or would need undoing.
4. The crash handler.
5. The settings ([settings.md](settings.md#loading)), before the hooks, since the first frame's VR initialization reads them.
6. The composition (`composition::install`): every crate's registration, the registration check, and the hooks ([below](#hook-installation-and-removal)).

A failure at steps 2, 3, or 6 logs why and unloads the payload from the injector's thread, after stopping whatever it already started (the tuning watcher and the settings writers, and the crash handler); nothing is installed, and the injector reports the module gone. The rest of the initialization needs the game thread and runs once, at the first `Game::Update` (`initialize_from_game`): the GPU release tracking, the debug UI's egui state, the capture targets, the HUD redirect, and the OpenXR runtime ([vr-runtime.md](vr-runtime.md)).

## Shutdown preparation

An eject is requested by F5, the injector's `--eject` (through the `eject` export), or a reload. The request only records the intent; the game thread does the rest from its pre-update entry. Repeated requests coalesce.

Preparation separates reversible work from irreversible teardown. Participants register before enabling work that needs preparing, each under a `ParticipantId` its owning crate supplies, so the lifecycle names no subsystem above it. They prepare in registration order, outside the lifecycle mutex, and the first that defers or fails ends the attempt; the next is tried a fixed number of frames later. A deferred or failed attempt leaves the shutdown flag unset, keeps every cleanup registered, and returns to normal frame work, so gameplay and the VR runtime go on. A ready participant returns a guard that holds its exclusion barrier and is validated again just before commit; dropping an uncommitted guard rolls its preparation back, and a stale attempt or a changed participant registry cannot authorize shutdown. Once every guard validates, the finalizers run with the barriers still held, and only then does the lifecycle set the shutdown flag.

The participants:

- The human hull restores the narrowed hulls in the native physics phase before it returns a ready guard, or commits with them in place after a bounded number of attempts ([body/human-hull-restoration.md](body/human-hull-restoration.md)).
- The magazine executor (`controllers::weapons::reload::shutdown`) prepares only on the game thread: it cancels a magazine extraction in progress, then defers while the reload executor has not drained or a native magazine callback is executing. Its admission opens once the hooks are enabled and closes after removal. Neither the hook library's disable nor a snapshot of suspended threads proves that a callback cannot resume in removed code, so the readiness covers the moment of preparation and not the removal itself.
- The inventory spawn (`controllers::weapons::inventory::spawn_safety`). Inventory restores and second-pistol requests hand the engine a payload callback, and nothing the engine reports proves that a queued callback has retired. The participant latches the first such exposure for the process, but it does not veto an unload: preparation and the post-unpatch check log a warning naming the risk (a callback the engine still holds would crash the game once the image unmaps) and continue. They defer only while a submission or callback is executing payload code at that moment, and preparation closes admission so no new request starts.

## Hook installation and removal

`composition` in the payload is the one place that names every module with detours. At startup it calls each crate's registration, then hands `jc3vr_core::hooks::install` the composed hook libraries ([architecture.md](../architecture.md#crate-layering) has the registrations and their constraints). A module registers its shutdown participants with the lifecycle and its hook steps with `hooks`: `register_post_enable` for admission that may open only once its detour is live, `register_pre_unpatch` for detours managed outside the hook library (the single-pass COM-vtable detours) and for refusals (an armed texture census), and `register_post_unpatch` for the checks that no native callback is still running in payload code once every patch is reverted (`payload/src/native_callbacks.rs`).

Each kind runs in registration order, and a refusing step stops removal and keeps the image mapped, so a step that must run whatever the checks after it say registers first: stereo's COM teardown before the census's refusal, and the magazine executor's admission close before the native-callback check. The libraries enable and disable under one thread suspension in the order `composition::libraries` lists them, which a unit test pins.

## Teardown after commit

After commit the game thread waits for an in-flight frame tail, restores the game's original vertex shaders, runs the cleanups in reverse registration order, and drops the egui state (`shutdown_from_game`). It then hands the rest to a shutdown thread (`shutdown_startup`), which stops the worker threads, waits for the engine to run a few frames, removes the hooks, waits a few frames again, removes the crash handler, and unmaps the module.

Every thread the payload spawns registers its stop with `lifecycle::register_worker`, and `lifecycle::stop_workers` runs the stops once, in registration order. A stop closes its worker's admission, waits for the thread within the worker's own budget, and reports whether the thread is gone. A worker that does not stop is logged by name, the remaining workers still stop, and the module is pinned instead of unmapped: leaking the DLL is better than a live thread in an unmapped image. The payload registers every worker from one table (`payload/src/teardown.rs`), whose module documentation says which positions matter: the frame tail first, since the recorder captures from the mirror it presents, and the test tools before the settings writers, since they put back the configuration they overrode. A unit test pins both.

The two settle waits (`lifecycle::settle_frames`) count the engine's own per-frame counter, which advances in the `Draw` prologue whether or not a detour is installed. The first lets the still-installed hooks carry out what the cleanups requested, such as the native-resolution restore. The second lets a detour entered before the hooks came out return. Each wait has a frame count and a budget in the core tuning's `teardown` table, because outside VR the frame loop stops while the window is in the background; a wait that runs out logs it and continues.

## Fatal failures

A game-thread initialization failure or a panic caught in the update enters the fatal state: the module stays loaded and pinned, the cleanups stay registered, and the mod's updates stop. This is containment, not a claim that gameplay or an eject is safe; the game must be restarted. A cleanup panic or a hook-removal failure after commit likewise keeps the module mapped rather than ejecting it.
