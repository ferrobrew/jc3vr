# Documentation index

Three kinds of document live here. `engine/` is reverse-engineered ground truth about Just Cause 3 as it is — addresses, layouts, and lifecycles, independent of what the mod does with them. `mod/` is what we build: design decisions and shipped implementation. `issues/` holds issue-scoped investigations that closed a specific question. [`roadmap.md`](roadmap.md) tracks where the project is going.

One deliberate exception: [`engine/rendering.md`](engine/rendering/rendering.md) documents both the engine's frame pipeline and the mod's stereo driver against it — its section numbers (`rendering §N`) are load-bearing anchors referenced throughout the code and docs, so it stays whole.

## engine/

- [button-hints-and-localisation.md](engine/button-hints-and-localisation.md) — the localisation store and its string lookup, the button-mapping table's prompt keys, the HUD's button-hint pipeline and its two hint stages, and how a prompt picks its gamepad glyph.
- [pause-menu.md](engine/pause-menu.md) — the pause and options menu: the Scaleform screen lifecycle and the ExternalInterface traffic, the `pause.gfx` movie's menus and folders and how the root menus' entries are ordered, settings screen, tab bar, and row widgets, the commands it sends, its input and navigation, and which parts code outside the movie can reach; reading movies with FFDec.
- [scaleform.md](engine/scaleform.md) — the Scaleform runtime: version and language, the loader and its states, the file opener and the VFS mounts, standalone movies (`CRenderToTextureUI`), fonts, widgets, and localisation, the movie files, and building a movie the loader accepts.

### rendering/

- [rendering.md](engine/rendering/rendering.md) — the frame pipeline: camera and projection (§2), buffers, the depth-reconstruction passes, the draw entry points and how shaders address the screen (§4), present and `BLOCK_FLIP` (§7), device and context (§8), resolution and render setups (§9), the stereo double-Draw machinery (§11–13).
- [lighting-shadow-pipeline.md](engine/rendering/lighting-shadow-pipeline.md) — the per-frame sun-shadow and global-lighting state: the frame counters, cascade fit and amortization, the clustered froxel light grid and its consumers (§4), `SetGlobalShaderConstants`, and the table of which GlobalConstants are parity- or counter-indexed (the flicker ping-pong surface).
- [shaders.md](engine/rendering/shaders.md) — extracting, naming, and disassembling the game's shaders, how the engine creates and reloads them, the DXBC container checksum, and the screen-pixel PCF rotation hash in the sun-shadow resolve; tooling in `tools/shaders/`.
- [render-setups-reinit.md](engine/rendering/render-setups-reinit.md) — the runtime resize path: `CreateRenderSetups`, its callers, what `ApplyResize` and `DestroyRenderSetups` touch, and how far the swapchain is separable from the scene targets.
- [model-culling.md](engine/rendering/model-culling.md) — the two model visibility gates: the instance-level BFBC cull, and the per-render-block frustum cull against the active camera that pops buildings at a widened view's edge.
- [ui-rendering.md](engine/rendering/ui-rendering.md) — how the UI renders: the `PreRender` update and capture on the game thread, the snapshot, the render worker, off-screen movies, display-info writes, blending and premultiplied alpha, and the HUD movie's layout.

### performance/

- [profiling.md](engine/performance/profiling.md) — the engine's timing systems: which are live and which are compiled out on the CPU and GPU sides, and the per-pass structure of the frame.

### character/

- [skeleton.md](engine/character/skeleton.md) — the Havok pose store, the model-space Joint API, frame ordering, and where to override bones.
- [humanik.md](engine/character/humanik.md) — the HumanIK solver: layout, per-frame lifecycle, effector ids, and the external-target injection recipe.
- [hands-locomotion-and-state.md](engine/character/hands-locomotion-and-state.md) — the weapon-to-hand attach chain, the per-arm aim IK, root motion and the collision proxy, scene queries, teleport signals, the vehicle seat attach, the proxy capsule and pause, and the per-tick character state signals.
- [idle-stance.md](engine/character/idle-stance.md) — the on-foot idle's clip variants, the animation modifiers that choose them, and why every armed idle is a combat stance.
- [animation-layers.md](engine/character/animation-layers.md) — the animation layers and their names, how the pose job stacks them, what keeps running when a layer's pose is left out (reload, holster, and draw events), and the state tasks inside the pose-finalization pass, the weapon off-hand grip among them.

### gameplay/

- [input.md](engine/gameplay/input.md) — the action effector system, the action enumeration, the write API, the semantic button-mapping layer, and the mouse/UI pipeline.
- [aim-state.md](engine/gameplay/aim-state.md) — the player's aim state: its writers, every consumer, fire gating independent of it, the relaxed and aimed body families, the combat mental state, and the other on-foot stance switches.
- [aim-pipeline.md](engine/gameplay/aim-pipeline.md) — how the player aims and fires: the per-consumer aim target cache, shot construction, dual-wield, auto-aim, and the camera getters.
- [grapple-pipeline.md](engine/gameplay/grapple-pipeline.md) — grapple targeting, hook flight and attach, and the zip/tether/retract dispatch.
- [swim-locomotion.md](engine/gameplay/swim-locomotion.md) — the swim state-task family: act-quantized turn dispatch (fixed-angle turn clips past a threshold angle), the two heading paths (animated-turn slerp vs continuous `DoRotate`), the swimming capsule offsets that make `SetOrientation` receive a pitched frame, and why none of the on-foot orientation machinery applies in water.
- [parachute-locomotion.md](engine/gameplay/parachute-locomotion.md) — the parachute state task and steering core, and how the camera input matrix is mixed into the chute's steering through the look-steer block.
- [weapon-data.md](engine/gameplay/weapon-data.md) — where a weapon entity's files live, the weapon object and its fire points in the entity property file, the model parts, and the name hash; tooling in `tools/sights/`.
- [weapon-selection.md](engine/gameplay/weapon-selection.md) — the slot-select and cycle actions, the inventory's selection and commit logic, the automatic switch away from an empty weapon, the HUD's selection cross, and its placement.
- [melee-and-damage.md](engine/gameplay/melee-and-damage.md) — the damage message every hit is made of, the player's melee from input to hit, the struck character's hit reaction, hit objects and their run-time types, and pushing a body.
- [item-pickup.md](engine/gameplay/item-pickup.md) — what lies in the world to pick up, how the game picks up a weapon and its ammunition, and a loose body's motion, owner, and velocity setters in the Havok world.
- [planted-explosives.md](engine/gameplay/planted-explosives.md) — the C4 weapon and its charges: the cap on placed charges, the planting mode, the drop under the parachute, the collision that sticks a charge, and the detonate input.
- [vehicle-parts.md](engine/gameplay/vehicle-parts.md) — a vehicle's parts: the parts resource and its entries, the live part instances on the physics body, how the render update draws them from their root-space transforms, which vehicles have a steering wheel part, the hinged parts the engine turns, and the vehicle type hash.

## mod/

- [vr-runtime.md](mod/vr-runtime.md) — the OpenXR runtime as built: session lifecycle, the frame loop, pose model, per-eye resolution, mirror, and the playtest checklist.
- [threading.md](mod/threading.md) — the threads mod code runs on, who holds the VR runtime lock across a frame, the lock rules, and the thread-role guard that checks them.
- [hud.md](mod/hud.md) — the floating-panel HUD: the redirect, compositing, and cursor interaction.
- [pause-menu.md](mod/pause-menu.md) — the VR menu inside the game's pause menu: one `VR` entry below the root menu's options entry, whose pages are tabs on the settings screen's own tab bar, section headers in the list title's style, the six hooks, command routing, the value wrappers, the page model and row encoding, step pages, live values, the `BODY`, `CONTROLS`, and `COMFORT` pages with the body calibration and saving, and the in-game status list.
- [environment.md](mod/environment.md) — debug-UI control of time of day and weather.
- [capture.md](mod/capture.md) — the in-process recorder: the GPU pack and readback ring, the y4m stream to the host encoder, the pose sidecar, and the reframe tool.
- [trailer-tooling.md](mod/trailer-tooling.md) — directing tools and the in-process eye capture for the trailer: decisions and the working todo list.
- [weapon-selector.md](mod/weapon-selector.md) — the planned hand-steered weapon selector on the game's cross (not built): the two phases, the fallback, the bindings and the dual-wield conflict, and the in-game unknowns.
- [scaleform-movie.md](mod/scaleform-movie.md) — a mod-owned Scaleform movie (not built): why the VR settings use the pause menu instead, the hosting routes, binding and input, and the risks.

### stereo/

- [single-pass-stereo.md](mod/stereo/single-pass-stereo.md) — rendering both eyes in one geometry walk into a double-wide target: the shader rewrites, the routing, the four ways content goes wrong under the collapse and what fixes each, and the configuration.
- [single-pass-render-blocks.md](mod/stereo/single-pass-render-blocks.md) — the per-render-block record of what single-pass stereo needs from each, and what it took to get there.
- [far-field.md](mod/stereo/far-field.md) — the monoscopic far field (issue #32): identifying the far-regime scene work and sharing it between eyes.
- [foveation.md](mod/stereo/foveation.md) — static foveated rendering (issue #29): the stencil radial-density-masking design, the depth-stencil-state seam, and the build plan.
- [swapchain-ownership.md](mod/stereo/swapchain-ownership.md) — the mod-owned back buffer: substituting the engine's swapchain-derived render setups so the scene renders per-eye while the DXGI buffers stay at the window size.
- [culling.md](mod/stereo/culling.md) — widening the engine's visibility culls to the binocular union: the union cull projection, the scene cull and its size and occlusion corrections, the spawn and character-occlusion frusta, the per-render-block frustum cull, and terrain.
- [lighting-shadow-vr-interactions.md](mod/stereo/lighting-shadow-vr-interactions.md) — every default-on mod modification touching camera, frame counters, lighting, or shadows, how each interacts with the stereo double-draw, and the candidate per-frame globals behind the terrain-wide sun-shadow flicker.

### rendering/

- [fsr.md](mod/rendering/fsr.md) — FSR anti-aliasing and upscaling in the stereo pipeline.
- [shader-patching.md](mod/rendering/shader-patching.md) — patching the game's shaders in memory at creation: the create hooks, the DXBC checksum refresh, the forced reload through `LoadShaderBundle`, the restore on eject, the sun-shadow PCF hash patch (issue #10), and the LOD dissolve patch.
- [upscaling-evaluation.md](mod/rendering/upscaling-evaluation.md) — Intel XeSS assessed against the shipped FSR2 as anti-aliasing and as a VR upscaler: the Arc-only D3D11 backend, the D3D11on12 route that does work and what it costs, XeSS's sub-rectangle API against the eye seam, and why the recommendation is still to finish FSR2 and validate foveation.
- [ui-decoupling.md](mod/rendering/ui-decoupling.md) — splitting HUD parts out of the one texture (proposal): why the issue #14 attempts failed, the premultiplied-alpha bug in the composite, the techniques, the atlas proposal, and the pending experiments.

### performance/

- [performance.md](mod/performance/performance.md) — where the frame actually goes, measured: geometry bound by draw submission rather than fill, the post chain bound by pixels, and what single-pass stereo buys.
- [profiler.md](mod/performance/profiler.md) — the in-game profiler (issue #34): puffin CPU scopes across the frame phases and render passes, the GPU timestamp lane, the flame graph, and the F9 trace capture.

### body/

- [head-and-body.md](mod/body/head-and-body.md) — how head and body yaw relate in VR: coupling schemes, the headpose abstraction, the head-bone override, head hiding, and body IK.
- [grapple-comfort.md](mod/body/grapple-comfort.md) — the grapple body-frame filter (issue #36): the hold from fire to landing, the yaw handoff, the landing-snap absorber, and the telemetry capture.
- [swim-yaw.md](mod/body/swim-yaw.md) — the right stick owns the swim heading: the `DoRotate` target override, the forward/back-only move direction, the accumulator seeding, and the turn-act suppression.
- [parachute-yaw.md](mod/body/parachute-yaw.md) — parachute head-yaw suppression (issue #48): removing the camera-relative look-steer from the chute's steering so the head does not turn it.
- [unarmed-body.md](mod/body/unarmed-body.md) — the weapon-agnostic body: on foot under the controllers, the weapon's modifiers kept off all but the action layer, the partial and additive layers left out of the pose while they run, and the game's off-hand weapon grip released.

### input/

- [input.md](mod/input/input.md) — how the mod taps, consumes, and injects the game's input.
- [controllers-and-roomscale.md](mod/input/controllers-and-roomscale.md) — the motion-controller and roomscale plan of record: the continuous-body principles, the aim-state pin, controller-owned hands and their tiers, per-hand aiming, weapon handling, per-mode bindings, roomscale, the milestones, the implementation status, and the headset test checklist.
- [controls-summary.md](mod/input/controls-summary.md) — the controls summary on the off-hand wrist: hiding the game's button hints, the binding tables read by both the injection and a describer, labels from the game's localisation, the panel and its glance, the glyph and font assets, and the in-game status list.
- [controllers.md](mod/input/controllers.md) — the motion controllers as built: the OpenXR input foundation, scheme selection, mode detection, the bindings and their injection, the gamepad-semantics pin, the aim-state pin, controller-owned hands, per-hand aiming, the sight lines and their tuning tool, the world-space overlay layer, recoil, haptics, dual-wield, the anchored grapple hand, vehicles and flight, grenades, melee, aim assist, the magazine gesture, and scopes.
- [physical-hands.md](mod/input/physical-hands.md) — each empty hand's palm colliding with the world: the probes, pushing off surfaces, holding the hand at them, punches and slaps through the game's melee damage, knocking objects, picking up, holding, and throwing loose objects, grabbing weapons, and the headset checklist.
- [steering-wheel.md](mod/input/steering-wheel.md) — the physical wheel and handlebars placed from the driven vehicle's parts: the box fit, the override table, the fallback held in the vehicle's space, the handlebars for the ATV, jetski, and duck, the rendered wheel turned in the render update, and the headset checklist.

## issues/

- [08-14-hud-overlays-and-depth.md](issues/08-14-hud-overlays-and-depth.md) — HUD overlays and depth (issues #8, #14).
- [15-enclosed-vehicles.md](issues/15-enclosed-vehicles.md) — enclosed vehicles (issue #15).
- [20-animation-judder.md](issues/20-animation-judder.md) — animation judder (issue #20).
- [32-monoscopic-far-field.md](issues/32-monoscopic-far-field.md) — the render-pass sort/depth-bucket machinery as the distance-split mechanism for a shared far field (issue #32).
- [40-terrain-black-tiles.md](issues/40-terrain-black-tiles.md) — terrain walls and cave ceilings rendering black at grazing angles (issue #40).
- [swim-yaw-endless-spin.md](issues/swim-yaw-endless-spin.md) — the swimming body flipping 180° the moment it moved: the crawl capsule offset's gimbal lock under the old `SetOrientation` yaw chase, and which `DoRotate` call actually turns the body.
