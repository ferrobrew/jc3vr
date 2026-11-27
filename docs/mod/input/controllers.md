# Motion controllers

This page holds the motion controllers' principles, the OpenXR input layer, the control scheme, and the mode detector. The rest is [bindings.md](bindings.md) (the layout, injection, chords, and the controls summary), [hands-and-aiming.md](hands-and-aiming.md), [grip.md](grip.md), [roomscale.md](roomscale.md), [weapon-handling.md](weapon-handling.md), [flight.md](flight.md), [haptics.md](haptics.md), and the vehicle pages ([vehicle-weapons.md](vehicle-weapons.md), [cockpit-controls.md](cockpit-controls.md), [steering-wheel.md](steering-wheel.md)).

The code shares the module path `controllers` across the layer crates ([architecture.md, "Modules by crate"](../../architecture.md#modules-by-crate)): the OpenXR side in `crates/jc3vr-view/src/vr/input/`, the body-bound subsystems (hands, aim, grab, roomscale, mode, scheme) in `crates/jc3vr-body/src/controllers/`, and the binding tables in `crates/jc3vr-gameplay/src/controllers/`. The settings are the `controllers` table (`ControllersConfig`).

## Principles

The virtual body behaves continuously and consistently with the player's. The game switches the player's body between discrete behaviours (the aim state flips locomotion to strafing and raises the weapon; the combat mental state draws and holsters on its own), and the mod dissolves these switches rather than mapping input onto them: a state change the player did not perform with their body is a defect. The view never turns unless a movement action the player took, such as a stick turn, turns it, which rules out run-and-steer locomotion.

The hands belong to the player; an animation takes them only where a scripted state cannot yet work otherwise ([hands-and-aiming.md, "Hand tiers"](hands-and-aiming.md#hand-tiers)). Without controllers the mod keeps the head-tracked gamepad game with gaze aim, and without room to walk the posture is seated.

The player maps onto Rico's proportions by remapping rather than scaling the world, because vehicle and building sizes and the sense of speed depend on true scale ([roomscale.md](roomscale.md#body-dimensions)).

## Scheme selection

The controller scheme is in effect while `controllers.enabled` is on (the pause menu's `MOTION CONTROLLERS`) and a controller is active in the published snapshot (`scheme::current`); otherwise the gamepad scheme is. Every subsystem that changes how the character behaves gates on the scheme, so a player who never picks up the controllers keeps the game's own behaviour. With `controllers.enabled` off the snapshot is not published at all, so every subsystem is inert.

## Input foundation

### Action sets

OpenXR gives every action exactly one owning set, so the actions come in two layers:

- `hands`, always active: the poses, the haptic output, and the raw inputs, named for the Index controller's physical inputs. The published snapshot reads these whatever the mode, so the grabs, the finger curls, the chords, and the scheme do not depend on the active set.
- `global`, always active, plus one mode set at a time: the semantic actions, named for what they do in the game. The mode sets are `on_foot` (also swimming, falling, and the parachute, so a grapple jump does not switch sets), `wingsuit`, `land_vehicle`, `helicopter`, `plane`, and `menu`.

Every set has priority 0, because a runtime ignores a lower-priority set's binding on an input a higher one also binds, and the raw and semantic actions bind the same inputs. Every action is read per hand, so the hand a press came from stays known after a remap.

Suggested bindings ship for the Index and Touch profiles (`vr/input/semantic/defaults.rs`; the layout is [bindings.md, "Default layout"](bindings.md#default-layout)), with a mirrored table for the left-handed. OpenXR fixes suggestions once the sets are attached, so `controllers.dominant_hand` takes effect at the next game restart, and until then the mod's hand roles follow the hand the attached bindings were suggested for (`ControllersConfig::acting_hand`), so the roles never disagree with the buttons.

Each frame the active semantic actions are published per hand as a `SemanticFrame` beside the snapshot. Their bound sources are queried every frame inside the input update ([threading.md](../threading.md)), because rebinding within one profile raises no event.

### The controller frame

Once per rendered frame `vr::input` syncs the actions, locates both hands' grip and aim spaces at the predicted display time, re-bases them by the recenter baseline as the eye views are, and publishes a `ControllerFrame` in the cockpit frame, so a controller pose composes into the world through the same transform as the head. With `controllers.input.hand_tracking` and `XR_EXT_hand_tracking`, the hand joints are published too.

The trigger, the grip, and the trackpad are thresholded into presses with hysteresis (the view tuning's `press` section). The grip press reads the squeeze force on the Index, because its capacitive squeeze value is full whenever the hand rests round the handle, and the squeeze value elsewhere; the choice is made per hand from the bound profile, since an emulated profile has no force of its own. The squeeze value still drives the finger curls and keeps a hold alive ([grip.md, "The grip's press"](grip.md#the-grips-press)).

A snapshot older than half a second counts as absent, and then the bindings inject nothing and the scheme falls back to the gamepad. Render frames outpace the sim tick, so one edge latch (`vr/input/semantic/frame.rs`) accumulates raw and semantic edges until the input tick takes them, and a press and release between two ticks is not lost.

### Reinjection and restarts

The OpenXR session persists across an uninject, and action sets attach to a session only once. So on uninject the action handles are stashed in the process environment with a fingerprint of the layout and the suggested bindings, and a reinject with the same fingerprint re-wraps them. A different fingerprint leaves the controllers unavailable until the game restarts, which a head-locked notice says (`vr/input/restart.rs`). For the same reason the actions are attached even with the controllers disabled.

### Action schema

The sets, their actions, and their types are held by the golden snapshot `vr/input/semantic/schema.golden`. The runtime keys a player's saved bindings on the names, so a name never changes meaning: a rename is a removal and an addition. A change bumps `SCHEMA_VERSION`, which is also the OpenXR application version. Adding an action is minor; removing, retyping, or moving one breaks saved bindings and needs a release note telling players to reset them.

### Input self-check

Once per runtime and set of profiles, `vr/input/self_check/` logs an `input: self-check` block checking what the layout relies on: that an input bound in two active sets reports in both, that the runtime thresholds a boolean on the Index grip force, and that the bound-source query answers.

## Mode detection

The detector (`controllers::mode`) runs once per engine input tick ([bindings.md, "Injection timing"](bindings.md#injection-timing)) over a snapshot of the engine state ([state-signals.md](../../engine/character/state-signals.md)). The first matching rule wins, in the order the engine's states overlap ([state-signals.md](../../engine/character/state-signals.md#state-overlap-and-precedence)):

1. A menu is up: `ui`, from [`Clock::m_Stop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_Stop) (the pause menu and the commlink), [`InputSystemBlocker::Ui`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputSystemBlocker##variant-Ui) (screens such as the map), and the mod's own panel.
2. A teleport is in flight ([`GameWorld::IsTeleporting`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-IsTeleporting)) or the character is not readable: hold the current mode.
3. A vehicle attach state, entry and exit included ([`Character::IsInVehicleAttachState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInVehicleAttachState)): `vehicle`, with its class and seat. The class predicates walk the interaction graph, so they run only then.
4. Grapple traversal: the gameplay mode holds, flagged for the hand tiers.
5. The wingsuit, the parachute, then falling: `airborne`.
6. Swimming, then everything else: `onfoot`.

The mode selects the mode set. A change between gameplay modes must persist for `mode.debounce_ticks`, so a flicker during an animation blend does not switch sets; entering and leaving `ui` is immediate, because a late switch would send a menu press to the gameplay bindings.
