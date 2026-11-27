# Documentation index

`engine/` is reverse-engineered ground truth about Just Cause 3 as it is, independent of the mod. `mod/` is the mod's design and shipped implementation. `issues/` holds investigations scoped to one issue. `sessions/` records each headset playtest and is not edited after the fact. Start with [architecture.md](architecture.md); [roadmap.md](roadmap.md) says where the project stands.

[engine/rendering/rendering.md](engine/rendering/rendering.md) is the one page that covers both the engine's frame pipeline and the mod's stereo driver against it. Its section numbers (`rendering §N`) are referenced from code and docs, so it stays whole.

## engine/

- [button-hints-and-localisation.md](engine/button-hints-and-localisation.md): the localisation store, the button-mapping prompts, and the HUD's button hints.
- [pause-menu.md](engine/pause-menu.md): the pause and options menu's Scaleform screens, `pause.gfx`, its commands, and its input.
- [scaleform.md](engine/scaleform.md): the Scaleform runtime, its loader and VFS, standalone movies, fonts, and building a movie the loader accepts.
- [window-focus.md](engine/window-focus.md): how the game detects and handles losing the desktop foreground.

### rendering/

- [rendering.md](engine/rendering/rendering.md): the frame pipeline, camera and projection, present, resolution and render setups, and the stereo double-draw.
- [lighting-shadow-pipeline.md](engine/rendering/lighting-shadow-pipeline.md): the per-frame sun-shadow and global-lighting state, and which constants are parity- or counter-indexed.
- [shaders.md](engine/rendering/shaders.md): extracting and disassembling the game's shaders, and how the engine creates and reloads them.
- [render-setups-reinit.md](engine/rendering/render-setups-reinit.md): the runtime resize path and the render setups.
- [window-glass.md](engine/rendering/window-glass.md): window glass, its damage, and how a pane is drawn and hidden.
- [model-culling.md](engine/rendering/model-culling.md): the instance cull and the per-render-block frustum cull.
- [ui-rendering.md](engine/rendering/ui-rendering.md): how the Scaleform UI is updated, captured, and rendered, and the HUD movie's layout.

### performance/

- [profiling.md](engine/performance/profiling.md): the engine's CPU and GPU timing systems and the per-pass structure of the frame.

### physics/

- [havok-bodies.md](engine/physics/havok-bodies.md): the physics step, the pre-simulation callbacks, making dynamic bodies, and the collision filter.

### character/

- [skeleton.md](engine/character/skeleton.md): the pose producer, the skeleton and its bones, the Joint API, and where bones can be overridden.
- [humanik.md](engine/character/humanik.md): the HumanIK solver, its lifecycle, effectors, and external targets.
- [hands-locomotion-and-state.md](engine/character/hands-locomotion-and-state.md): the weapon-to-hand attach, aim IK, root motion, the collision proxy, seat attach, and the per-tick character state.
- [idle-stance.md](engine/character/idle-stance.md): the on-foot idle variants and why every armed idle is a combat stance.
- [animation-layers.md](engine/character/animation-layers.md): the animation layers, how the pose job stacks them, and the state tasks in pose finalization.
- [animation-graph.md](engine/character/animation-graph.md): the decoded state-machine format and what Rico's graphs do with the weapon.
- [ragdoll.md](engine/character/ragdoll.md): the ragdoll, entering and leaving it, and protected characters.

### gameplay/

- [input.md](engine/gameplay/input.md): the action effectors, the action enumeration, button mapping, and the mouse and UI input.
- [aim-state.md](engine/gameplay/aim-state.md): the player's aim state, its writers and consumers, and the other stance switches.
- [aim-pipeline.md](engine/gameplay/aim-pipeline.md): the aim target cache, shot construction, dual-wield, and auto-aim.
- [weapon-firing.md](engine/gameplay/weapon-firing.md): what a shot needs and does, the shooting permission, and deferred weapon operations.
- [grapple-pipeline.md](engine/gameplay/grapple-pipeline.md): grapple targeting, the hook's flight and attach, and zip, tether, and retract.
- [swim-locomotion.md](engine/gameplay/swim-locomotion.md): the swim state tasks, their turn dispatch, and their heading paths.
- [parachute-locomotion.md](engine/gameplay/parachute-locomotion.md): the parachute's steering and how the camera feeds it.
- [weapon-data.md](engine/gameplay/weapon-data.md): a weapon entity's files, fire points, and model parts.
- [weapon-equip-and-drop.md](engine/gameplay/weapon-equip-and-drop.md): a weapon object's pickup, equip, unequip, and drop.
- [weapon-ownership-and-despawn.md](engine/gameplay/weapon-ownership-and-despawn.md): what keeps a weapon alive, slot moves, the respawn table, and the save.
- [weapon-gives-and-restores.md](engine/gameplay/weapon-gives-and-restores.md): every way into a slot, removals, and the three weapon caches.
- [weapon-selection.md](engine/gameplay/weapon-selection.md): slot selection and cycling, the automatic switch, and the HUD's selection cross.
- [weapon-ammo-and-reload.md](engine/gameplay/weapon-ammo-and-reload.md): magazines, reserves, the reload funnel, and the per-weapon actions.
- [melee-and-damage.md](engine/gameplay/melee-and-damage.md): damage messages, the player's melee, and hit reactions.
- [item-pickup.md](engine/gameplay/item-pickup.md): world pickups, loose bodies in the Havok world, and constraints.
- [planted-explosives.md](engine/gameplay/planted-explosives.md): C4 and its charges.
- [wingsuit-stall.md](engine/gameplay/wingsuit-stall.md): the wingsuit's stall and recovery flip.
- [wingsuit-evade.md](engine/gameplay/wingsuit-evade.md): the wingsuit's barrel-roll evade.
- [wingsuit-opening.md](engine/gameplay/wingsuit-opening.md): opening the wingsuit and the parachute from the air.
- [wingsuit-landing.md](engine/gameplay/wingsuit-landing.md): the wingsuit's landing, the air brake, and the crash.
- [vehicle-doors.md](engine/gameplay/vehicle-doors.md): door controllers, hinges and latches, and entering through a door.
- [stunt-positions.md](engine/gameplay/stunt-positions.md): stunt areas, the helicopter hang, and how a character is held there.
- [vehicle-parts.md](engine/gameplay/vehicle-parts.md): vehicle parts, deform points, and how parts are drawn and turned.

## mod/

- [vr-runtime.md](mod/vr-runtime.md): the OpenXR runtime, session lifecycle, frame loop, pose model, and mirror.
- [threading.md](mod/threading.md): the threads mod code runs on, the VR runtime's locks, and the frame overlap.
- [lifecycle.md](mod/lifecycle.md): shutdown preparation, deferred retries, and teardown.
- [tuning.md](mod/tuning.md): the tuning TOML files, their loading and hot reload, and what counts as tuning.
- [settings.md](mod/settings.md): the settings file, its load and save, and its representation.
- [hud.md](mod/hud.md): the floating-panel HUD, its compositing, cursor, and HUD depth placement.
- [pause-menu.md](mod/pause-menu.md): the VR pages inside the game's pause menu.
- [status.md](mod/status.md): the status rows shown by the debug UI and logged.
- [debug-ui.md](mod/debug-ui.md): the egui debug UI.
- [environment.md](mod/environment.md): time of day and weather from the debug UI.
- [sessions.md](mod/sessions.md): the session output folder and where each output lands.
- [capture.md](mod/capture.md): capturing both eyes to video, the capture hosts, and the pose sidecar.
- [trailer-tooling.md](mod/trailer-tooling.md): the directing tools for recording footage.
- [weapon-selector.md](mod/weapon-selector.md): the weapon wheel on the game's selection cross.
- [window-focus.md](mod/window-focus.md): keeping the game running when the desktop window loses focus.
- [scaleform-movie.md](mod/scaleform-movie.md): a mod-owned Scaleform movie (not built) and why the pause menu is used instead.

### stereo/

- [single-pass-stereo.md](mod/stereo/single-pass-stereo.md): both eyes in one geometry walk, the shader rewrites, and the per-eye corrections.
- [single-pass-render-blocks.md](mod/stereo/single-pass-render-blocks.md): what single-pass stereo needs from each render block.
- [foveation.md](mod/stereo/foveation.md): static foveated rendering.
- [swapchain-ownership.md](mod/stereo/swapchain-ownership.md): the mod-owned back buffer in place of the engine's swapchain-derived setups.
- [culling.md](mod/stereo/culling.md): widening the engine's culls to both eyes.
- [lighting-shadow-vr-interactions.md](mod/stereo/lighting-shadow-vr-interactions.md): the mod's changes to camera, frame counters, lighting, and shadows, and how they meet stereo.

### rendering/

- [fsr.md](mod/rendering/fsr.md): FSR 2 in the stereo pipeline.
- [taa.md](mod/rendering/taa.md): the mod's own TAA resolve, an alternative to FSR.
- [presentation-layer.md](mod/rendering/presentation-layer.md): what is re-posed at the render rate rather than the simulation's.
- [render-trace.md](mod/rendering/render-trace.md): the render trace and its analyzer.
- [render-time-anchoring.md](mod/rendering/render-time-anchoring.md): anchoring what is drawn relative to the player at render time.
- [shader-patching.md](mod/rendering/shader-patching.md): patching the game's shaders in memory at creation.
- [upscaling-evaluation.md](mod/rendering/upscaling-evaluation.md): XeSS assessed against FSR 2.
- [ui-decoupling.md](mod/rendering/ui-decoupling.md): splitting HUD parts out of the panel texture, and the atlas.

### performance/

- [performance.md](mod/performance/performance.md): which performance tool answers which question, what limits the frame, and how to measure.
- [profiler.md](mod/performance/profiler.md): the in-game CPU and GPU profiler.
- [frame-budget.md](mod/performance/frame-budget.md): the frame-time benchmark's results at the headset's resolution.
- [frame-timing.md](mod/performance/frame-timing.md): the always-on frame breakdown.

### body/

- [runtime-testing.md](mod/body/runtime-testing.md): active replacement paths, gameplay checks, and known limitations.
- [human-hull-restoration.md](mod/body/human-hull-restoration.md): restoring the human's native shape settings.
- [attachments.md](mod/body/attachments.md): socket interaction targets and pouch markers in the body mirror.
- [head-and-body.md](mod/body/head-and-body.md): how head and body yaw relate, the head pose, and the head bone.
- [grapple-comfort.md](mod/body/grapple-comfort.md): the grapple's body-frame comfort filter.
- [swim-yaw.md](mod/body/swim-yaw.md): the right stick owning the swim heading.
- [swim-strokes.md](mod/body/swim-strokes.md): swimming with the arms.
- [canopy-opening.md](mod/body/canopy-opening.md): skipping the canopy's opening roll out of a grapple reel.
- [parachute-yaw.md](mod/body/parachute-yaw.md): keeping the head from steering the parachute.
- [locomotion.md](mod/body/locomotion.md): stick locomotion and the steady on-foot view.
- [procedural-body.md](mod/body/procedural-body.md): the procedural torso and arms.
- [procedural-states.md](mod/body/procedural-states.md): the procedural torso in seats, the wingsuit, the parachute, and water.
- [physics-substrate.md](mod/body/physics-substrate.md): the physics layer the hands, weapons, grapple, and vehicles share.
- [rig.md](mod/body/rig.md): the body rig, its pelvis solver, step commit, and yaw follow.
- [ik-body.md](mod/body/ik-body.md): the IK-first on-foot body.
- [body-estimator.md](mod/body/body-estimator.md): the learned body estimator (EgoPoser).
- [unarmed-body.md](mod/body/unarmed-body.md): the weapon-agnostic body.

### input/

- [input.md](mod/input/input.md): how the mod taps, consumes, and injects the game's input.
- [controllers.md](mod/input/controllers.md): the motion controllers as built.
- [controllers-and-roomscale.md](mod/input/controllers-and-roomscale.md): the motion-controller and roomscale design.
- [grip.md](mod/input/grip.md): the grip and how each hand picks a grab target.
- [grip-poses.md](mod/input/grip-poses.md): the fingers wrapping what the hand holds.
- [two-handed-inventory.md](mod/input/two-handed-inventory.md): the mod's loadout table and independent hands.
- [weapon-acquisition.md](mod/input/weapon-acquisition.md): taking guns from characters' hands, racks, and lockers.
- [controls-summary.md](mod/input/controls-summary.md): the controls summary on the off-hand wrist.
- [haptics.md](mod/input/haptics.md): the controllers' vibration.
- [slow-motion.md](mod/input/slow-motion.md): the game's slow motion against real time.
- [physical-hands.md](mod/input/physical-hands.md): each empty palm colliding with the world.
- [hand-climbing.md](mod/input/hand-climbing.md): climbing with gripping hands.
- [character-grabs.md](mod/input/character-grabs.md): grabbing characters with an empty hand.
- [held-prop-impacts.md](mod/input/held-prop-impacts.md): held-body contact feedback and damage.
- [impact-sounds.md](mod/input/impact-sounds.md): impact sounds from the hands.
- [breaking-glass.md](mod/input/breaking-glass.md): breaking window glass by hand.
- [grab-view.md](mod/input/grab-view.md): the grab view diagnostic.
- [collider-view.md](mod/input/collider-view.md): the collider view diagnostic.
- [physical-weapons.md](mod/input/physical-weapons.md): weapons as physical objects in either hand.
- [physical-guns.md](mod/input/physical-guns.md): a held gun as a physics body.
- [pouch-models.md](mod/input/pouch-models.md): grenade, C4, and magazine models on Rico's pouches.
- [grenades.md](mod/input/grenades.md): grenades as independent loadout items.
- [attachments.md](mod/input/attachments.md): the belt magazine and other body attachments.
- [paired-rack-weapons.md](mod/input/paired-rack-weapons.md): rack weapons and their ammunition.
- [reload.md](mod/input/reload.md): the hand-worked reload, and what each weapon loads.
- [physical-grapple.md](mod/input/physical-grapple.md): working the grapple line with the hand.
- [grapple-lock.md](mod/input/grapple-lock.md): the grapple lock.
- [seat-grapple.md](mod/input/seat-grapple.md): the grapple from a vehicle seat.
- [grapple-line-grab.md](mod/input/grapple-line-grab.md): the free hand on the grapple line.
- [grapple-snatch.md](mod/input/grapple-snatch.md): pulling things into the hand with the grapple.
- [stunt-positions.md](mod/input/stunt-positions.md): stunt positions held by the grip.
- [vehicle-doors.md](mod/input/vehicle-doors.md): vehicle doors, hoods, and trunks by hand.
- [cockpit-controls.md](mod/input/cockpit-controls.md): the flight stick, throttle, cyclic, and collective.
- [wingsuit-gestures.md](mod/input/wingsuit-gestures.md): opening, braking, and closing the wingsuit by pose.
- [canopy-target-contract.md](mod/input/canopy-target-contract.md): the parachute canopy's targets from the native rig.
- [vehicle-weapons.md](mod/input/vehicle-weapons.md): passenger aim and mounted guns.
- [steering-wheel.md](mod/input/steering-wheel.md): the physical wheel and handlebars.

### testing/

- [recorder.md](mod/testing/recorder.md): the tracking recorder and the offline `tracking-synth` commands.
- [rerun.md](mod/testing/rerun.md): exporting a recording to Rerun.
- [view-scan.md](mod/testing/view-scan.md): scanning a take's video for camera flashes.
- [fast-comparison.md](mod/testing/fast-comparison.md): the capture harness's fast GPU comparison loop.
- [benchmark.md](mod/testing/benchmark.md): the frame-time benchmark.

## issues/

- [08-14-hud-overlays-and-depth.md](issues/08-14-hud-overlays-and-depth.md): HUD overlays and depth (#8, #14).
- [15-enclosed-vehicles.md](issues/15-enclosed-vehicles.md): enclosed vehicles (#15).
- [20-animation-judder.md](issues/20-animation-judder.md): animation judder (#20).
- [32-monoscopic-far-field.md](issues/32-monoscopic-far-field.md): the render-pass sort as a distance split for a shared far field (#32).
- [40-terrain-black-tiles.md](issues/40-terrain-black-tiles.md): terrain rendering black at grazing angles (#40).
- [far-field.md](issues/far-field.md): the removed monoscopic far field (#32).
- [periodic-sim-tick-hitches.md](issues/periodic-sim-tick-hitches.md): periodic stalls inside the sim tick.
- [session-stalls-and-low-camera.md](issues/session-stalls-and-low-camera.md): long game-thread stalls and a low camera.
- [swim-yaw-endless-spin.md](issues/swim-yaw-endless-spin.md): the swimming body flipping as it moved.
