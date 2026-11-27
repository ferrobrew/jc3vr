# Lifecycle

The payload starts on the injector's thread, finishes initializing on the game thread, and ejects in two phases: a reversible preparation that may defer, then an irreversible teardown. The order lives in `payload/src/lib.rs`; the lifecycle (`crates/jc3vr-core/src/lifecycle/`) runs it without naming any subsystem.

## Startup

`initialize_startup` sets up, in order, the session folder ([sessions.md](sessions.md)), logging, the worker table (`teardown::register`), the tuning ([tuning.md](tuning.md)), the crash handler, the settings ([settings.md](settings.md#loading)), and the composition (`composition::install`). A failure in the worker table, the tuning, or the composition stops what started and unloads the payload. The rest needs the game thread and runs at the first `Game::Update` (`initialize_from_game`): the egui state, the capture targets, the HUD redirect, and the OpenXR runtime.

## Shutdown preparation

F5, the injector's `--eject`, or a reload requests an eject, which the game thread carries out. Participants register under a `ParticipantId` their crate supplies and prepare in registration order; the first that defers or fails ends the attempt, which is retried a few frames later while gameplay goes on. A ready participant returns a guard holding its exclusion barrier, revalidated at commit and rolled back if dropped uncommitted. The participants are the human hull restore ([body/human-hull-restoration.md](body/human-hull-restoration.md)), the magazine executor (deferring while a native magazine callback runs), and the inventory spawn, which warns about engine callbacks it cannot prove retired but does not veto.

## Hook installation and removal

`composition` in the payload names every module with detours ([architecture.md](../architecture.md#crate-layering)). A module registers hook steps with `jc3vr_core::hooks`: `register_post_enable` for admission that opens once its detour is live, `register_pre_unpatch` for detours outside the hook library (the single-pass COM-vtable detours) and refusals (an armed texture census), and `register_post_unpatch` for the checks that no native callback is still in payload code (`payload/src/native_callbacks.rs`). Each kind runs in registration order and a refusing step keeps the image mapped, so a step that must run regardless registers first: stereo's COM teardown before the census, and the magazine executor's admission close before the native-callback check.

## Teardown after commit

The game thread waits for the frame tail, restores the game's vertex shaders, and runs the cleanups in reverse order (`shutdown_from_game`). A shutdown thread (`shutdown_startup`) then stops the workers, lets the engine run a few frames, removes the hooks, waits again, removes the crash handler, and unmaps the module. The settle waits let the hooks carry out what the cleanups requested and let a detour already entered return; each has a budget in the core tuning's `teardown` table, because outside VR the loop stops in the background.

Every thread the payload spawns registers its stop with `lifecycle::register_worker` from one table (`payload/src/teardown.rs`). A worker that does not stop in its budget pins the module instead of unmapping it, since a leaked DLL beats a live thread in an unmapped image.

## Fatal failures

A game-thread initialization failure or a panic in the update leaves the module loaded and pinned and stops the mod's updates; the game must be restarted. A cleanup panic or hook-removal failure after commit also keeps the module mapped.
