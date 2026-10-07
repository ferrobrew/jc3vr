# Threading and the VR runtime lock

The mod runs code on threads it does not own: the game thread, the engine's draw task and CPU fragment workers, and the UI capture and render workers, as well as a frame-tail worker of its own. The VR runtime state (`VR_STATE` in `payload/src/vr/state.rs`) is locked for a whole frame while its holder waits on those threads, so which threads may lock it, and where in the frame, is a correctness rule. Two deadlocks came from breaking it: the pause menu's COMFORT page asked whether a session was running from the UI capture thread while the frame held the lock and waited for that thread, and the egui floating panel asked the same from the draw task. This doc describes the thread model, who holds what across a frame, the lock rules, and the guard that checks them.

## Threads

| Thread | Mod code that runs on it | Role |
| --- | --- | --- |
| Game thread | The [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update) hook (`crate::update`: the directing tools, the hand bodies' callback registration, and the debug UI), the sim tick's hooks, the hand bodies' pre-simulation callback (inside the game's update, outside the physics step), and the [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender) hook (the VR runtime pump, the frame begin, the engine's render update, the eye dispatches, and the inline frame tail) | `Game`, narrowed to `GameDebugUi` and `GameFrame` |
| Frame tail (`jc3vr-frame-tail`) | The deferred frame tail (`payload/src/vr/tail.rs`): the final drain, the mirror, the blit, and the frame submit | `FrameTail` |
| Draw task | [`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask) and everything under it: the render-pass and camera hooks, `RenderEngine::PostDraw` (the HUD quad, the egui panel, the mirror overlay), and the shader hooks. It runs on a CPU fragment worker, or inline on the game thread when [`CpuPrimaryCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/cpu_fragment##func-CpuPrimaryCount) is at most one | `Unmarked`, or `GameFrame` when inline |
| CPU fragment workers | Engine jobs and any hook they reach. The game thread also runs queued fragments itself while it waits in [`CpuFragmentWaitUntilSignalIsNonZero`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/cpu_fragment##func-CpuFragmentWaitUntilSignalIsNonZero) | `Unmarked`, or the game thread's role for a fragment it runs |
| UI capture thread | The thread [`UIManager::m_CurrentCaptureThread`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_CurrentCaptureThread) names: the pause-menu hooks (`payload/src/pause_menu/`) and the capture seam (`hooks::ui::movie_capture`). The pause screen's update runs on a thread the frame waits on while it holds the runtime lock | `Unmarked`, or `GameFrame` |
| UI render worker | The fragment [`UIManager::StartRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-StartRender) queues: the partitioned HUD render (`hooks::ui::ui_render`) | `Unmarked` |
| Mod writer threads and the shutdown thread | The screenshot, recorder, settings, diagnostics, and profiler-capture writers, and `shutdown_startup`, which flushes a pending settings save ([settings.md](settings.md)) | `Unmarked`; none locks the runtime state |

The role column is the thread's role in `payload/src/thread_role.rs`, described under [The guard](#the-guard). Engine threads keep the default `Unmarked`, so a hook that runs on an unexpected thread is caught without having been marked.

## Who holds what across a frame

A frame runs in this order on the game thread:

1. `Game::Update` calls `crate::update`, which runs the debug UI holding the egui state lock (`EguiState::get`) for the whole egui pass.
2. The sim tick runs.
3. The `Game::UpdateRender` hook calls `vr::update`, `vr::auto_recenter_tick`, and `vr::apply_native_resolution`, each of which locks the runtime state briefly. The first of them blocks until the previous frame's tail has submitted, which is how the tail keeps the next frame off its eye textures.
4. `vr::frame_begin` takes the runtime lock and keeps it in the `FrameContext` it returns.
5. The engine's render update and the eye dispatches run with the lock held. The game thread waits on the CPU fragment workers, running stolen fragments while it does, and on the draw task between dispatches ([`GraphicsEngine::WaitForCPUDrawToFinish`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-WaitForCPUDrawToFinish)).
6. With the tail inline, the game thread drains the last dispatch, submits, and releases the lock. With the tail deferred, the `FrameContext` moves to the tail worker, which holds the lock while it polls the last dispatch's completion signals (each with a timeout), records the mirror and the blit under the engine's immediate-context critical section, and submits. The game thread meanwhile returns to the next frame's `Game::Update`.

So the runtime lock is held for most of a frame, by the game thread and then by the tail, and both holders wait on the draw task and the fragment workers. Under it, the holders take the config lock, the controller frame slot, the stage information, the render-parameter slot, the input status and haptics queue, the capture-texture state, the blit and mirror pipelines, the egui panel state, and the engine's immediate-context critical section. All of these are taken briefly and nothing takes the runtime lock under them.

The egui state lock is the other long one. The game thread holds it across the debug UI, and the draw task takes it in `PostDraw`: at eye 0's to render the floating panel, holding the egui panel lock while it waits for it, and at the frame's last near dispatch's to render the mirror overlay. With the tail deferred, that last walk can overlap the next frame's debug UI, so the draw task can wait there for the game thread's egui pass; the game thread never waits on that walk while it holds the egui state lock (rule 6), and under the collapse eye 0's walk is that last walk too.

## Lock rules

1. Block on the runtime lock only on the game thread at the top of the render update, holding no other lock. The functions that do (`vr::update`, `vr::is_running`, `vr::native_eye_resolution` and `vr::engine_render_resolution`, `vr::frame_begin`, `vr::uninstall`, and `vr::wait_for_frame_tail`) are for that point, and for the shutdown path described below.
2. Everywhere else, read a published slot. `vr::render_params` says whether a VR frame is rendering and carries the per-eye parameters; `vr::stage::stage`, `controllers::frame::latest`, and `vr::recenter_count` publish the rest. The debug UI's runtime readout uses `vr::status`, which try-locks and reports a busy state instead of waiting.
3. Ask for runtime work by request. `vr::recenter`, `vr::request_recenter`, and `vr::request_seat_recenter` set atomic bits that `vr::auto_recenter_tick` services at the top of the next frame. `vr::recenter` also defers choosing the recenter kind to then, since choosing it takes the config lock.
4. Take the runtime lock outermost. A thread holding any other lock, the config lock included, must not take it: the holders take those locks under it.
5. Keep published slots as leaves: hold them briefly, take no other lock under them, and do not wait while holding them.
6. Do not wait on the draw task while holding the egui state lock. The shutdown path tears the runtime down from the lifecycle cleanups, which run under that lock, so it first calls `vr::wait_for_frame_tail`: with the tail submitted, the draw task it was waiting on is done and no longer needs the egui state lock.

## Call sites

| Call site | Lock | Threads that reach it | Why it is safe |
| --- | --- | --- | --- |
| `vr::update` (`hooks::game::game_update_render`) | Blocking, brief | Game thread, top of the render update | Rule 1 |
| `vr::auto_recenter_tick`, through `vr::is_running`, the recenter it services, and the fold of the body's frozen turn into the baseline | Blocking, brief | Game thread, top of the render update | Rule 1 |
| `vr::apply_native_resolution`, through `vr::native_eye_resolution` | Blocking, brief | Game thread, top of the render update | Rule 1 |
| `vr::frame_begin` | Blocking, held for the frame | Game thread, top of the render update | It is the holder |
| The frame tail | Owns the frame's guard | Frame tail | It is the holder, and takes no lock of its own |
| `vr::status` (`ui::vr`) | `try_lock` | Game thread in the debug UI | Cannot block |
| `vr::recenter`, through `headpose::recenter`, from F7 (`hooks::wndproc`) and the debug UI's VR and Camera tabs | None (atomic request) | The window procedure and the game thread in the debug UI | Rule 3. The Camera tab calls it holding the config lock |
| `vr::request_recenter` from the pause menu's COMFORT page, the stick-click chord, and the seat transitions | None (atomic request) | UI capture thread, the input tick, and the roomscale update | Rule 3 |
| `vr::uninstall`, from the lifecycle cleanups in `shutdown_from_game` | Blocking, brief | Game thread under the egui state lock | Rule 6 |
| The COMFORT and BODY pages' session checks, `hud::egui_panel::is_active`, `hud::mirror_overlay::is_active` | `vr::render_params` | UI capture thread, draw task, frame tail | Rule 2 |
| `vr::input::restart::current` (the restart notice, the debug UI's banner, the CONTROLS page's button layout readout) and `controllers::summary::note_shown` | None: the input status slot, then the config lock, never nested; a leaf slot | Draw task, game thread in the debug UI, UI capture thread | Rule 2 |

## The guard

`payload/src/thread_role.rs` gives each thread a role in a thread-local cell. A thread's base role is marked once with `mark_thread`, which leaves an existing role alone: the game thread marks itself `Game` at the top of `Game::Update` and of the render update, and the tail worker marks itself `FrameTail` when it starts. A `RoleScope` from `enter` narrows the role for a dynamic extent and restores the previous one when dropped: `crate::update` holds `GameDebugUi` for the egui pass, and the render update holds `GameFrame` from just after `vr::frame_begin` to its end, in flatscreen frames too, so a violation shows without a headset. The draw task and the fragments that the game thread runs itself take its role, which inside the render update is `GameFrame`, and every engine thread is `Unmarked`.

Every blocking lock of the runtime state goes through `lock_state` in `payload/src/vr/state.rs`, the only code that can reach the mutex besides the `try_lock` in `vr::status`. It reads the caller's role, and a role other than `Game` (`ThreadRole::may_block_on_vr_state`) is a violation. A debug build logs it at error level, with the caller's source location from `#[track_caller]`, and panics before it blocks, so the log names the site instead of the process wedging. A release build logs the first violation and goes on to lock, which keeps a missed case as a deadlock with a log line rather than a crash. The check costs one thread-local read per lock, and the runtime state is locked a handful of times a frame.

The guard checks the thread, not the locks the thread holds. A lock from the game thread at the top of the render update while it holds another lock (rule 4) is not caught; the functions in rule 1 are called there with nothing held, and the Camera tab's config-lock case is closed by `vr::recenter` taking no lock at all.
