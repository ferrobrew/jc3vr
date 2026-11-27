# Architecture

jc3vr is a VR mod for Just Cause 3. A DLL, the payload, is injected into the running game. Inside the process it detours the engine's frame, camera, rendering, input, character, and UI functions, drives an OpenXR session against the game's own Direct3D 11 device, renders the scene once per eye or once for both eyes at the headset's per-eye resolution, and submits the result to the OpenXR compositor. The game never sees the headset: it renders as it normally would, while the mod substitutes the camera, the targets, and the present.

Each section below links to the document that owns its detail. [CONTRIBUTING.md](../CONTRIBUTING.md) holds the conventions and the development workflow, and [README.md](README.md) indexes every document.

## Workspace

| Path | What it is |
| --- | --- |
| `injector/` | Loads the payload into a running game process (or ejects or reloads it), then stays running as the capture host that encodes the payload's video stream ([mod/capture.md](mod/capture.md)). |
| `payload/` | The injected DLL (`jc3vr_payload`), the top of the [crate layering](#crate-layering). |
| `crates/jc3vr-*` | The layer crates that hold the rest of the payload's code ([crate layering](#crate-layering)). A crate that draws keeps its HLSL and the committed DXBC in its own `shaders/`. |
| `jc3gi/` | Rust bindings to the game's types, functions, and globals, generated at build time by pyxis from the `.pyxis` files in the `jc3gi/pyxis-defs` submodule. The defs are the record of every address, offset, and reverse-engineered finding; CONTRIBUTING.md describes the rules and the submodule workflow. |
| `crates/body-estimator` | The learned full-body estimator (EgoPoser) on the CPU and through Direct3D 11 compute ([mod/body/body-estimator.md](mod/body/body-estimator.md)). |
| `crates/body-rig` | The body rig's pure core, shared by the payload and the offline evaluator ([mod/body/rig.md](mod/body/rig.md)), and the shared math in `body_rig::math`. |
| `crates/dxbc-stereo` | DXBC rewriting of the game's shaders for single-pass stereo ([mod/stereo/single-pass-stereo.md](mod/stereo/single-pass-stereo.md)). The rewrites that only the capture harness's experiments request sit behind its `capture-rewrites` feature. |
| `crates/fsr-sys` | Bindings to FSR 2 with its Direct3D 11 backend, built from `vendor/` ([mod/rendering/fsr.md](mod/rendering/fsr.md)). |
| `crates/tracking-format` | The `.jc3trk` tracking recording's data model and codec ([mod/testing/recorder.md](mod/testing/recorder.md)). |
| `crates/layering-guard` | A test crate that checks the layer crates' manifests against the [crate layering](#crate-layering). |
| `tools/shadergen` | Compiles the layer crates' shaders, the body estimator's kernels, and the FSR 2 shader permutations. |
| `tools/dxbc-tool` | Compiles HLSL and disassembles DXBC through `d3dcompiler_47` under wine. |
| `tools/scaleform-gfx-examine` | Inspects Scaleform movie files ([engine/scaleform.md](engine/scaleform.md)). |
| `tools/tracking-synth` | Offline tools over tracking recordings: printing, the body rig evaluator, the Rerun export, the view scan, and synthetic recordings ([mod/testing/recorder.md](mod/testing/recorder.md)). |
| `tools/rendering/` | The Python capture harness for image comparisons and the frame-time benchmark ([mod/testing/fast-comparison.md](mod/testing/fast-comparison.md), [mod/testing/benchmark.md](mod/testing/benchmark.md)). |
| `tools/*` (other) | Python extraction and analysis scripts for game data. Each subject's doc names its tool. |
| `scripts/` | Shell and PowerShell entry points: cross-building and testing under `cargo xwin`, launching and injecting under Proton, the shared wine prefix, the capture relay, and fetch scripts for vendored binaries. |

## Payload lifecycle

The injector calls the payload's `run` export, or `run_source_tuning` when a development launcher asks for the tuning from the source tree. Startup loads the tuning and the settings and installs the hooks through `composition::install` ([crate layering](#crate-layering)); a missing or invalid tuning file, or a missing registration, stops it and unloads the payload. The rest of the initialization, the OpenXR runtime among it, runs on the game thread at the first [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update). An eject (F5, or the injector's `--eject`) prepares every subsystem for shutdown on the game thread, tears down in a fixed order, removes the hooks, and unmaps the module. [mod/lifecycle.md](mod/lifecycle.md) owns the full startup and shutdown order.

## Path of a frame

The engine runs one frame on the game thread as an update, a render update, and one or more draws, which the engine's draw task records on a CPU fragment worker. [mod/threading.md](mod/threading.md) owns the thread model and the lock rules; [mod/vr-runtime.md](mod/vr-runtime.md) owns the OpenXR loop step by step. The steps below follow one frame with a VR session running.

### Update and simulation

1. The [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update) detour (the payload's `hooks::game`) runs the payload's `update` before the original: the directing tools, the tracking recorder's frame tick, the capture harness's requests, the hand bodies' frame tick, and the debug UI.
2. The original update runs the simulation tick. The mod's [`InputDeviceManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##func-Update) detour consumes the look and move input and lands the controller bindings' writes ([mod/input/input.md](mod/input/input.md)). The input tick's game stage, in the world's pre-simulation queue, makes the bindings' decisions and every act that changes the world ([mod/input/controllers.md](mod/input/bindings.md#injection-timing)).
3. The character hooks run inside the pose-finalization pass: the procedural torso and the wrist targets before the HumanIK solve, then the head override and the finger curls after it ([mod/body/procedural-body.md](mod/body/procedural-body.md), [mod/body/head-and-body.md](mod/body/head-and-body.md)). The rig moves Rico through the game's own character movement ([mod/body/rig.md](mod/body/rig.md)).

### Render update and the OpenXR frame

4. The [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender) detour opens the frame's generation of the per-frame slots (`jc3vr_core::render_frame`), which keep the values of the frame being prepared apart from those of the frame the draw task is still recording.
5. `vr::update` pumps OpenXR events and brings the session up, retries it, or tears it down. Without a headset runtime, the mod plays in flatscreen stereo and keeps retrying. `vr::apply_native_resolution` sizes the scene targets to the headset's per-eye resolution; while a session runs, the engine renders into a back buffer the mod owns rather than the DXGI swapchain ([mod/stereo/swapchain-ownership.md](mod/stereo/swapchain-ownership.md)).
6. `vr::frame_wait` calls `xrWaitFrame`, which paces the game against the compositor in place of vsync, locates the eye views, and samples the controllers into the published `ControllerFrame` that the controller subsystems read.
7. `vr::begin_render_frame` composes the head pose into the world (`headpose::xr`) and publishes it with the per-eye off-axis projections. The original render update runs. At render time, the presentation layer re-poses the held weapon, the hands, and the upper body on the skinning palette from the latest tracking, so nothing attached to the player is drawn a simulation tick behind ([mod/rendering/presentation-layer.md](mod/rendering/presentation-layer.md), [mod/rendering/render-time-anchoring.md](mod/rendering/render-time-anchoring.md)).
8. `WaitedFrame::begin` calls `xrBeginFrame` once the previous frame's tail has submitted. When a frame renders, `vr::arm_direct` acquires the swapchain image so each eye can be blitted into its slice from the draw task.

### Draws

9. `hooks::game` builds a dispatch plan (`stereo::dispatch::DispatchPlan`) and calls [`Game::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Draw) once per dispatch. Between dispatches it restores the engine state that advances once per draw (frame counters, constant-buffer rings, temporal history indices, draw-list parity), so each dispatch sees the same frame ([engine/rendering/rendering.md](engine/rendering/rendering.md) sections 5 and 11 to 13).
10. Each draw runs on the draw task ([`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask)). The [`Camera::SetupRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetupRenderCamera) detour applies the eye's projection and offset. The render crate's `hooks::graphics_engine` corrects what does not survive per-eye rendering: culling widened to both eyes ([mod/stereo/culling.md](mod/stereo/culling.md)), lighting and shadow state ([mod/stereo/lighting-shadow-vr-interactions.md](mod/stereo/lighting-shadow-vr-interactions.md)), and FSR or the mod's TAA in place of the engine's SMAA ([mod/rendering/fsr.md](mod/rendering/fsr.md), [mod/rendering/taa.md](mod/rendering/taa.md)). Game shaders are patched in memory when the engine creates them ([mod/rendering/shader-patching.md](mod/rendering/shader-patching.md)).
11. The [`RenderEngine::PostDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-PostDraw) detour draws the world-space overlay, the scope lens, the floating HUD panel, the debug UI, the controls summary, and the desktop mirror overlay into each eye's back buffer, then blits the eye into its swapchain slice or copies it into its capture texture. The overlay and the scope lens are the render crate's own passes; the others are registered in the view crate's `hooks::graphics_engine::post_draw` by the crates that own them, as ordered tables, so the render hooks name none of the layers above them.

### Submit and mirror

12. `vr::present_and_submit` blits any eye the draw did not, and ends the frame with one projection layer. `vr::present_mirror` draws one or both eyes into the game window and presents the game's swapchain, the only present of the frame. Normally these run on the frame-tail worker (`vr::tail`), so the game thread starts the next frame's update while the last draw and the GPU finish; [mod/vr-runtime.md](mod/vr-runtime.md#the-frame-loop) lists when they stay inline.

### Stereo rendering paths

Multipass stereo dispatches `Game::Draw` once per eye. Single-pass stereo, the default while a session runs, dispatches once into a double-wide target: the game's vertex shaders are rewritten so one instanced draw emits both eyes through viewport routing, and the content that reads a single camera is corrected per eye ([mod/stereo/single-pass-stereo.md](mod/stereo/single-pass-stereo.md)). Where the frame time goes in each path is in [mod/performance/performance.md](mod/performance/performance.md) and [mod/performance/frame-budget.md](mod/performance/frame-budget.md).

### The floating panel

The game's Scaleform UI is redirected into a texture the mod owns by rebinding [`UIManager::m_RenderBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_RenderBuffer). That texture is drawn as a quad in each eye's scene at `RenderEngine::PostDraw`, so it reaches the compositor inside the projection layer rather than as a separate composition layer. World-anchored markers are reprojected into the world rather than drawn on the panel ([mod/hud.md](mod/hud.md), [engine/rendering/ui-rendering.md](engine/rendering/ui-rendering.md)). The VR settings are pages in the game's own pause menu ([mod/pause-menu.md](mod/pause-menu.md)); everything else is in the debug UI ([mod/debug-ui.md](mod/debug-ui.md)).

## Modules by crate

Each layer crate's top-level modules group by the domain they serve. Detours live in each crate's `hooks/`, which mirrors the pyxis hierarchy. Where a domain spans layers, each crate holds its layer's part under the same module path, so `controllers::overlay` is the overlay's data model in the view crate, its draw pass in the render crate, and its placement in the body crate. Each crate that owns tuning reads it through its `tuning` module ([mod/tuning.md](mod/tuning.md)).

- `jc3vr-core`: `lifecycle`, `module`, `thread_role`, `game_tick`, `cpu_placement`, `hash`, `session`, `logging`, `render_frame`, `frame_timing`, `published` (published values and their clocks), `profiler`, `config` (the configuration engine), `registration`, `hooks` (the plumbing), and `clean_view`.
- `jc3vr-input`: `frame`, `motion`, `scheme`, `haptics`, `semantic`, and `capture`.
- `jc3vr-view`: `vr`, `headpose`, the camera and window-focus detours (`hooks::{camera, window_focus}`), the registries the render hooks share (`hooks::graphics_engine::{capture, post_draw, palette_edit, player_draw}`), the frame state (`stereo`, `fsr`), `recorder`, `gpu_passes`, and the data the layers above hand the render passes (`controllers::{cockpit, origin_provenance, overlay, lens_frame}`, `hud`).
- `jc3vr-render`: `stereo`, `fsr`, `debug`, `screenshot`, `image_writer`, `readback`, `draw_path`, the engine-pipeline detours (`hooks::{graphics_engine, clock, draw_count, profiler}`), the GPU profiler lane (`profiler`), the capture probes (`probes`), and the render passes of the layers above (`controllers::{overlay, scope, draw_state}`).
- `jc3vr-capture`: `harness`, the visual stage's controls (`testing::visual`), and the benchmark samplers (`testing::benchmark`).
- `jc3vr-body`: the body-bound controller subsystems (`controllers::{body, body_frame, hands, fingers, presentation, grab, aim, aim_state, locomotion, roomscale, mode, scheme, clock, haptics, overlay, gamepad_setting, telemetry}`), the modules the subsystems above meet in (`controllers::{intent, held, physics, facts}`), `contributions`, and the body detours (`hooks::{character, animation, input}`).
- `jc3vr-weapons`: `controllers::{weapons, loadout, spawn}` and `contributions`.
- `jc3vr-physical`: `controllers::{physical_hands, glass}` and `contributions`.
- `jc3vr-vehicles`: `vehicles` and the equipment detours (`hooks::equipment`).
- `jc3vr-grapple`: `grapple` and `controllers::{grapple_button, yank, snatch}`.
- `jc3vr-gameplay`: `controllers::{bindings, summary, camera_height, config}`, the registrations of the body's contributions (`controllers::register_*`), and the gameplay detours (`hooks::{input::look, sim, online}`).
- `jc3vr-hud`: `hud`, `egui_impl`, the UI detours (`hooks::ui`), and `controllers::{map, summary}`.
- `jc3vr-tooling`: `recorder`, `director`, `testing`, and `diagnostics`.
- `jc3vr-ui`: `ui`, `status`, `pause_menu`, the VR pages (`controllers::{menu, presets, body}`), and the profiler's panel (`profiler`).
- `jc3vr_payload`: `lib.rs` (the entry points, startup, shutdown, and the per-frame update), `composition`, `native_callbacks`, `teardown`, `allocator`, `eject_gpu`, `crash`, `config` (the settings aggregate, its schema, slot table, format, and load), and the game's frame entry points and the window procedure (`hooks::{game, wndproc}`).

## Crate layering

The payload is built from layer crates, each of which may depend only on the crates below it; crates side by side may not depend on each other. From the bottom:

1. `jc3vr-core`: the lifecycle, the configuration engine, the tuning loader, the hook plumbing, the frame slots, timing, and the profiler core.
2. `jc3vr-input`: the pure controller frame types, motion, the scheme, the haptic mix, and the input-capture policy. It depends on no layer crate, so it builds and tests natively.
3. `jc3vr-view`: the OpenXR runtime, the head pose, the camera and window-focus detours, the frame state the render hooks read per draw, the tracking recorder, and the registries and cells the render hooks share with the layers above.
4. `jc3vr-render` beside the gameplay stack, on the view crate, with no edge between the render crate and any crate of the stack:
   - `jc3vr-body`: the body-bound controller subsystems, the vocabulary, held-item facts, and physics substrate the crates above share, and the character, animation, and input detours.
   - `jc3vr-weapons` and `jc3vr-physical`, side by side on the body crate: the weapon handling and loadout, and the physical hands and glass. What one knows about the other (a held weapon, a reached weapon, the held guns' bodies) crosses through the body crate's held-item facts and registrations.
   - `jc3vr-vehicles` and `jc3vr-grapple`, side by side on the weapons and physical crates: the vehicles, mounted guns, wingsuit, and parachute, and the grapple with the snatch. What the grapple tells the vehicles crosses as a body fact. The vehicles crate's `register` returns the equipment detours the hand-aimed mounted guns drive, so the composition enables them only once the vehicle handling has registered. The grapple's `register` installs its comfort filter as the head pose's `jc3vr_view::headpose::BodyFrameFilter`.
   - `jc3vr-gameplay`, on all of them: the bindings, the summary model, and the look, sim, and online-manager detours. It fills the ordered tables of the body's contributions record (`jc3vr_body::contributions::BodyContributions`) and declares their order, since it is the lowest crate that sees every subsystem they hold.
5. `jc3vr-capture`, on the render crate: the screenshot harness, the visual stage's controls, and the benchmark samplers. It defines no detours. The render hooks reach it only through the probe table the render crate keeps (`probes`), which the capture crate fills at install and the hooks call at generic events that name no diagnostic, and only once a capture or benchmark request has armed them.
6. `jc3vr-hud` and `jc3vr-tooling`, side by side. Tooling may depend on the capture crate; the HUD may not.
7. `jc3vr-ui`: the debug UI, the status rows, and the pause menu.
8. `jc3vr_payload`, the only `cdylib`, which may depend on every layer crate.

The gameplay stack and the capture crate reach the render passes only through the view crate's registries and cells, so an edit to the render crate rebuilds none of them, and an edit to a crate of the stack rebuilds neither its sibling nor the crates below it.

A lower crate reaches a higher one through a registration the higher one makes rather than by naming it: the passes the render crate's post-draw runs, for example, are registered in the view crate's `hooks::graphics_engine::post_draw` by the gameplay, HUD, and tooling crates. What a crate reads from the crates above it are its contributions. A crate declares them in one `Contributions` struct with one field per contribution and takes them through one `register` call, so a registrant that leaves one out fails to compile. Names the lower crate keeps for its own bookkeeping are opaque values the registrant supplies: a shutdown participant registers under a `ParticipantId` its owning crate defines, and the lifecycle only compares and prints it. The body's ordered tables (the hand stages, the world weapon observers, the pre-weapon hooks, the auto-attach refusals, and the grip sources' order) and the post-draw's dispatch ticks and eye layers take no keys: the registrant's table declares their order.

`composition::install` in the payload calls one `register` per crate, in one place, before it composes the hook libraries. The order of the registrations is free but for three constraints, which [mod/lifecycle.md](mod/lifecycle.md#hook-installation-and-removal) and the module doc of `payload/src/composition.rs` state with their reasons: every registration happens before `jc3vr_core::hooks::install`; stereo's hook-removal step registers before the texture census's; and the magazine executor's admission closes before the check that no native callback is still running. The hook libraries enable in the order `composition::libraries` lists them, all under one thread suspension; no two detour the same function, so the game cannot observe the order, and a unit test pins it so that a change to it is deliberate. A subsystem reads its settings through the slot it declares beside their type, so no layer names the settings aggregate, which only the payload sees.

Every registration reports what it reads and finds missing to one `RegistrationCheck` (`jc3vr_core::registration`). After the last registration and before any hook is installed, the composition finishes the check, which returns every missing contribution at once. A missing contribution is a broken composition, not a state the game can reach, so the payload logs the list, stops its startup, and unloads itself. Nothing panics, and nothing is skipped silently: each registry the composition fills is reported by the crate that fills it, through a probe the registry's crate keeps beside it (`frame_participants_registered`, `frame_pass_registered`, and the like).

Cargo forbids cycles between the crates; `crates/layering-guard` checks the rest. It reads the payload's manifest and every `crates/jc3vr-*/Cargo.toml` and checks that every dependency in every section (normal, dev, build, and per-target) points down the order and never across between siblings, reading each dependency by its package name and by its path so an alias cannot hide one. It also checks that no layer crate depends on the payload, that every layer crate is a workspace member, that only the payload is a `cdylib`, and that the view and render crates' `hooks` modules never name the gameplay stack or the HUD. Each failure says how to fix it. When an edge is refused, move the code down to the crate that owns it, or invert the edge with a registration; do not move the layer.

## Reading order

1. [CONTRIBUTING.md](../CONTRIBUTING.md): the workspace rules, the pyxis workflow, and the build, test, and lint commands.
2. This page, then [mod/vr-runtime.md](mod/vr-runtime.md) and [mod/threading.md](mod/threading.md) for the frame loop and the thread model in detail.
3. [engine/rendering/rendering.md](engine/rendering/rendering.md) for the engine's frame pipeline, which every rendering change depends on, then [mod/stereo/swapchain-ownership.md](mod/stereo/swapchain-ownership.md) and [mod/stereo/single-pass-stereo.md](mod/stereo/single-pass-stereo.md).
4. [mod/hud.md](mod/hud.md), [mod/settings.md](mod/settings.md), and [mod/tuning.md](mod/tuning.md) for the panel, the settings, and the tuning.
5. [mod/input/controllers.md](mod/input/controllers.md), [mod/input/grip.md](mod/input/grip.md), and [mod/body/rig.md](mod/body/rig.md) for the controllers and the body, with [engine/gameplay/input.md](engine/gameplay/input.md) and [engine/character/skeleton.md](engine/character/skeleton.md) for the engine side.
6. The latest folder in `docs/sessions/` for the state of the most recent headset playtest.

## Where new findings go

A finding about the game goes into the pyxis defs as a typed binding with its doc comment, and its explanation goes into `docs/engine/<subject>/`. A design or implementation of the mod goes into `docs/mod/<subject>/`, under the subject directory that matches the crate module. An investigation that answers one issue's question goes into `docs/issues/`. A doc that would mix engine and mod material is split along that line. Every new doc gets an entry in [README.md](README.md). Docs and comments name pyxis symbols and carry no addresses, offsets, or other game-owned constants (CONTRIBUTING.md has the full rule).
