# Motion controllers

The mod makes Just Cause 3 playable with motion controllers: controller input in every mode, hands owned by the player, per-hand aiming, physical weapon handling, and roomscale posture. This page holds the principles, the OpenXR input layer, the control scheme, and the mode detector, and maps the rest:

- [bindings.md](bindings.md): the default layout per mode, the binding tables and their injection, the menus, the chords, and the stand-down rules.
- [hands-and-aiming.md](hands-and-aiming.md): the aim-state pin, the controller-driven hands and their tiers, the arm reach, the finger curls, the gun and grapple aim, the sight lines, the world-space overlay, and the scopes.
- [roomscale.md](roomscale.md): the body dimensions, the posture, the head height, crouching, the head and comfort fades, and the seat transitions.
- [weapon-handling.md](weapon-handling.md): the pouches, the foregrip, the weight of a held gun, recoil, and grenades and C4 in outline.
- [flight.md](flight.md): the wingsuit's arm steering and barrel roll, the parachute's handles, and the ripcord.
- The grip, which decides what a hand takes, is [grip.md](grip.md); vibration is [haptics.md](haptics.md); the wrist's controls panel is [controls-summary.md](controls-summary.md); vehicles are [vehicle-weapons.md](vehicle-weapons.md), [cockpit-controls.md](cockpit-controls.md), and [steering-wheel.md](steering-wheel.md).

The engine side is in [aim-state.md](../../engine/gameplay/aim-state.md), [aim-pipeline.md](../../engine/gameplay/aim-pipeline.md), [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md), [input.md](../../engine/gameplay/input.md), [state-signals.md](../../engine/character/state-signals.md), [weapon-attachment-and-arm-ik.md](../../engine/character/weapon-attachment-and-arm-ik.md), and [humanik.md](../../engine/character/humanik.md).

The code is spread over the layer crates under the same module path, `controllers` ([architecture.md, "Modules by crate"](../../architecture.md#modules-by-crate)): the OpenXR side in `crates/jc3vr-view/src/vr/input/`, the body-bound subsystems (hands, aim, grab, roomscale, mode, scheme) in `crates/jc3vr-body/src/controllers/`, the weapons in `crates/jc3vr-weapons`, the vehicles and flight in `crates/jc3vr-vehicles`, and the binding tables and the controls summary's model in `crates/jc3vr-gameplay/src/controllers/`. The settings are the `controllers` table of the settings file (`ControllersConfig`), assembled from the groups each subsystem declares beside its own code.

## Principles

The virtual body behaves continuously and consistently with the player's body. The game switches the player's body between discrete behaviours: the aim state flips locomotion to strafing, raises the weapon, and runs the arm aim IK, and the combat mental state draws and holsters the weapon on its own. The mod dissolves these switches rather than mapping controller input onto them, so a state change the player did not perform with their own body is a defect.

The mod never changes the view's orientation unless the change is attached to a movement action the player took, such as a stick turn. Run-and-steer locomotion, where the body turns to face its direction of travel, is excluded for this reason.

The hands belong to the player. An animation takes them only where a scripted state cannot yet work otherwise, and the hand tiers record each such state ([hands-and-aiming.md, "Hand tiers"](hands-and-aiming.md#hand-tiers)).

Everything degrades in two directions. Without controllers the mod keeps the head-tracked gamepad game with gaze aiming. Without room to walk, the posture is seated and roomscale translation is off.

The player maps onto Rico's proportions by remapping, as Bonelab does, rather than by scaling the world: the world stays at true scale, because vehicle sizes, building sizes, and the sense of speed depend on it; the eyes sit at Rico's eye height; hand offsets from the shoulders scale by the ratio of the arms; vertical head travel scales by the ratio of the heights; and horizontal travel stays one to one ([roomscale.md](roomscale.md)).

## Scheme selection

The controller scheme is in effect while `controllers.enabled` is on and a controller is active in the published snapshot (`scheme::current`). With the controllers switched off (the pause menu's `MOTION CONTROLLERS`) or no controller frame, the gamepad scheme is in effect. Every controller subsystem that changes how the character behaves gates on the scheme, so a player who never picks up the controllers keeps the game's own behaviour. `controllers.enabled` also gates the per-frame input sync and the snapshot's publication, so with it off every subsystem is inert.

Every controller feature ships on, as the debug UI's `Controllers` preset (Tools > Presets); the `Gamepad baseline` preset turns the controller and roomscale features off. Presets change switches only, never numeric settings, handedness, the grip offset, or the sight entries.

## Input foundation

### Action sets

OpenXR gives every action exactly one owning set, so the actions are in two layers.

- `hands`, always active: the grip and aim poses, the haptic output, and the raw inputs, named for the Valve Index controller's physical inputs. The published snapshot is read from these whatever the mode, so the gestures, grabs, finger curls, chords, input recovery, scheme detection, and pointers do not depend on the active set. Their localized names ask the player not to rebind them.
- `global`, always active, plus one mode set at a time: the semantic actions, named for what they do in the game. The mode sets are `on_foot` (on foot, swimming, falling, and the parachute, which read the same inputs, so a grapple jump does not switch sets), `wingsuit`, `land_vehicle` (cars, motorcycles, boats, the mech, and vehicles of no resolved class), `helicopter`, `plane`, and `menu`. Actions only one seat uses share an input and are told apart by the seat.

Every set has priority 0, because a runtime ignores a lower-priority set's binding on an input a higher-priority active set also binds, and the raw and semantic actions bind the same inputs. Every action has both hands as subaction paths and is read per hand, so the hand a press came from stays known after a remap. Per-user remapping is the runtime's binding interface (SteamVR's or xrizer's), where the player sees game meanings such as "Reload (hold: holster)".

Suggested bindings ship for the Index and the Oculus Touch profiles (`vr/input/semantic/defaults.rs`), written per action as a side (dominant, off-hand, both, or a fixed hand) and a component by position; the layout is in [bindings.md, "Default layout"](bindings.md#default-layout). The left-handed table mirrors every dominant and off-hand row and keeps the fixed-hand rows. OpenXR has no per-user presets and fixes suggestions once the sets are attached, so the table is chosen from `controllers.dominant_hand` when the actions are created, and a change of hand takes effect at the next game restart. Until then the mod's hand roles (the gun hand, the grapple's ray, the wrist readouts, the mirrored vehicle controls) follow the hand the attached bindings were suggested for (`ControllersConfig::acting_hand`, read from `vr::input::bound_hand`), so the roles never disagree with the buttons. SteamVR follows the mirrored table only while the player has no custom binding for the game.

Each frame the per-hand state of the active semantic actions is published as a `SemanticFrame` beside the snapshot. Each action also carries a digital state: a boolean's value, or a float past the press thresholds of the input it reads by default, with hysteresis, so a grip action keeps the mod's force thresholds wherever it is bound. The bound sources (which physical inputs each action reads on each hand) need the session, so the game thread queries them inside the frame's input update ([threading.md](../threading.md)) before every sync sample, because rebinding within one profile raises no event. Unchanged path sets reuse the published sources and tables; path strings and localized names are cached by OpenXR path. The bindings' masks act on the sources, and the controls summary draws them ([controls-summary.md](controls-summary.md#sources)).

### The controller frame

Once per rendered frame, inside the VR frame loop, `vr::input` syncs the actions, locates both hands' grip and aim spaces at the predicted display time, re-bases them against the recenter baseline exactly as the eye views are re-based, and publishes a `ControllerFrame`. Poses are in the cockpit frame (the LOCAL space re-based by the recenter baseline, OpenXR axes), so a controller pose composes into the world through the same transform as the head.

The trigger, the grip, and the trackpad are thresholded into digital presses with hysteresis, from the view tuning's `press` section ([tuning.md](../tuning.md)). The grip press reads the squeeze force on the Index (`press.squeeze_force`), because its capacitive squeeze value is full whenever the hand rests round the handle, and the squeeze value elsewhere (`press.squeeze`). The choice is made per hand from the bound profile's suggested bindings, not from the values, since an emulated profile has no force of its own. The squeeze value still drives the finger curls and, on a force grip, keeps a hold alive while the fingers stay round the handle ([grip.md, "The grip's press"](grip.md#the-grips-press)). The trackpad press reads the trackpad force and is split into a left-half and a right-half press, decided where the thumb is when the press starts and kept until the release.

With `controllers.input.hand_tracking` set and `XR_EXT_hand_tracking` offered, each hand's joints are published too, located with the controller-conforming motion range when `XR_EXT_hand_joints_motion_range` is offered. A hand whose joints cannot be located keeps its poses and inputs for that frame.

The snapshot stands for the controllers only while the frame loop keeps sampling. It is cleared when the session stops running, when a frame cannot begin, and when the controllers are disabled, and a snapshot older than half a second counts as absent, a bound above a main-thread hitch. With no snapshot the bindings inject nothing and the scheme falls back to the gamepad.

Render frames outpace the engine's sim tick, so a press and release between two ticks would be invisible to a consumer comparing states tick to tick. One edge latch (`vr/input/semantic/frame.rs`) accumulates the raw and semantic edges across publishes until the input tick, its single taker, takes them; the snapshot and the `SemanticFrame` are published, taken, and cleared together, so both streams cover the same frames. The first snapshot after a clear latches no edges, so a button held across a clear is not pressed twice.

### Reinjection and restarts

The OpenXR instance and session persist across an uninject and reinject, and action sets can be attached to a session only once. On uninject the action handles are stashed in the process environment with a fingerprint of the action layout and of every suggested binding (the suggestions count, because a runtime takes them only before attachment), and a reinject with the same fingerprint re-wraps them. A payload with a different layout, suggestions, or dominant hand cannot attach new actions to the persisted session, so its controllers stay unavailable until the game restarts. The actions are created and attached whether or not the controllers are enabled, for the same reason.

`vr/input/restart.rs` records why a restart is needed: a stash for another layout, a stash that differs only in the dominant hand, a session that already has sets attached, or a changed `controllers.dominant_hand`. While one holds, a head-locked notice (`hud/restart_notice.rs`) says "Restart the game to use the motion controllers" or "Restart the game to apply the dominant hand", and the debug UI shows the same banner.

### Action schema

The sets, their actions, and their types are the action schema, held by the golden snapshot `vr/input/semantic/schema.golden`. The runtime keys a player's saved bindings on the names, so a name never changes meaning: a rename is a removal and an addition. A change bumps `SCHEMA_VERSION`, which is also the OpenXR application version, and adds a line below. Adding an action is a minor change; removing, retyping, or moving one breaks saved bindings and needs a release note telling players to reset them. A change of suggestions alone changes the layout fingerprint and needs a game restart.

- Version 1: the raw inputs move into `hands`, `global` and the semantic mode sets replace the old raw sets, and `hands` gains `squeeze_click`, read only by the self-check.
- Version 2: `on_foot` gains `weapon_wheel`.
- Version 3: the gameplay sets gain `grapple`, the button whose gestures the grapple reads; `fire_grapple` and `release_tethers` stay for analog or custom layouts and ship unbound.
- Version 4: `global` gains `skip_cutscene`.
- Version 5: the vehicle sets' fire actions move to the upper face buttons and become booleans, `exit_vehicle` joins `stunt` on the right lower face button, `grapple` moves to the off-hand stick click, and `horn` ships unbound.
- Version 6: `wingsuit`'s `air_brake` becomes a boolean.

### Input self-check

At the first focused frame at which a hand has an interaction profile, `vr/input/self_check/` logs one `input: self-check` block under the `vr` target, and Input/bindings > Self-check shows it. It checks what the layout relies on: that one input bound in two active sets at priority 0 reports in both (a semantic side that stays inactive while the raw side is active means the runtime gives an input to one set only), that a boolean action on the Index grip force is thresholded by the runtime, and that the bound-source enumeration answers. Pressing each paired input afterwards logs an `input: self-check finding`; the grip's findings are measurements of the runtime's thresholds, never failures.

The check runs once per runtime name and set of interaction profiles, remembered in `input_self_check.toml` beside the settings file; a remembered pair logs `input: self-check skipped`, and the debug UI's "Run the self-check now" runs it again.

## Mode detection

The detector (`controllers::mode`) runs once per engine input tick, in the tick's game stage ([bindings.md, "Injection timing"](bindings.md#injection-timing)), over a snapshot of the engine state ([state-signals.md](../../engine/character/state-signals.md)). The first matching rule wins, in the order the engine's states overlap ([state-signals.md, "State overlap and precedence"](../../engine/character/state-signals.md#state-overlap-and-precedence)):

1. A menu is up: `ui`. The signals are the game clock's stop flag ([`Clock::m_Stop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_Stop)), which covers the pause menu and the commlink; the player action map's UI blocker ([`InputSystemBlocker::Ui`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputSystemBlocker##variant-Ui)), which covers screens such as the map that do not pause; and the mod's own panel.
2. A deferred teleport is in flight ([`GameWorld::IsTeleporting`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-IsTeleporting)), or the character is not readable (the first frames after a spawn): hold the current mode.
3. In a vehicle attach state, including the entry and exit ([`Character::IsInVehicleAttachState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInVehicleAttachState)): `vehicle`, with the class resolved in the order helicopter, plane, motorcycle, car, and sea (the mech is its own class), and the seat from the vehicle's local-player seat. The class predicates walk the interaction graph, so they run only while the attach state holds.
4. Grapple traversal (the pre-reel, the reel, the wind-up to a hang, reeled in): the gameplay mode holds, and the traversal is flagged for the hand tiers.
5. The wingsuit, then the parachute, then falling: `airborne`, with a variant for each.
6. Swimming, then everything else: `onfoot`, with a swim variant.

The mode selects the mode set: `onfoot` and the falling and parachute variants select `on_foot`, the wingsuit `wingsuit`, a vehicle its class's set, and `ui` the `menu` set. A change between gameplay modes must persist for the controls tuning's `mode.debounce_ticks` before it takes effect, so a one-tick flicker during an animation blend does not switch sets. Entering and leaving `ui` is immediate, because the UI signals do not flicker and a late switch would send a menu press to the gameplay bindings.

The controller subsystems' derived state (the scheme, mode, hand tiers, aim, and body rig) is recorded once per game frame as a `controller_state` event in the tracking recording ([recorder.md](../testing/recorder.md#what-a-recording-holds)).
