# Input

The local player's input runs through one action-effector map; the Scaleform UI has a separate mouse pipeline. This page covers the effectors, where the device poll runs, the write API, the mouse and UI pipeline, the default keymap, and the semantic button-mapping layer above the raw actions.

## Action effectors

[`LocalPlayerActionMap::GetActionEffector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::LocalPlayerActionMap##func-GetActionEffector) returns an `SEffectorElement` per action, and there is a `Freeze` path. The look axes are `LOOK_LEFT`, `LOOK_RIGHT`, `LOOK_UP`, and `LOOK_DOWN` (net `yaw = right − left`, `pitch = up − down`), and the move axes are `MOVE_LEFT`, `MOVE_RIGHT`, `MOVE_FORWARD`, and `MOVE_BACKWARD`.

Actions are the [`Action`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action) enum, from `Action::PAUSE` through `Action::GUI_USE_BUTTON`, which the effector accessors and the write API take directly. It mirrors [`action_name_table`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::action_name_table), a fixed array of name pointers indexed by action and built once at startup, with [`Action::COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action::COUNT) entries. Key and axis bindings load separately (`LoadActionMapFromFile`); the numbering is static.

## Where the device poll runs

[`InputDeviceManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##func-Update) has one caller, [`GameState::PreUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameState##func-PreUpdate), through the manager's virtual table. `PreUpdate` sets the UI manager's capture thread to its own thread, starts the sound library's pre-simulation job, updates the message boxes and the accounts, restores the steering objects' action maps when they are dirty, runs the poll and then [`UpdateForceClicks`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##func-UpdateForceClicks) (unless the out-of-focus input setting is on and the window is out of focus), runs `CUIManager::PreUpdate`, waits for the sound job, and reverts the capture thread.

[`GameStateLoad::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameStateLoad##func-Update) calls `PreUpdate` inline on the game thread, and so does [`GameStateFrontend::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameStateFrontend##func-Update) while the frontend is still loading (IO, the title, login, language, and the global, spawn, landscape, and location loads). Once the world is loaded behind the main menu, the frontend calls [`GameStateRun::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameStateRun##func-Update) instead, as [`Game::UpdateGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateGame) does in play. The run state queues `PreUpdate` as the input job ([`RunInputThread`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/game##func-RunInputThread), fragment body [`RunInputThread_Proxy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/game##func-RunInputThread_Proxy)), does its own pre-simulation work, and then spins on the job's completion signal before the world update. In play, and in the main menu with the world behind it, the poll therefore runs on a CPU fragment worker concurrently with the game thread, and only occasionally on the game thread, when it steals the fragment while waiting on one of the previous frame's signals. Neither the game state (the frontend runs the run state's tick) nor the poll's thread identifies which case applies.

During the job the game thread runs the network updates, the traffic and ambient-spawn starts (whose jobs create and remove vehicles and characters), the task pool's continuations, the road decorations, the water, the effects' pre-simulation update, the world time, the render cameras' transform history, the character manager's pre-simulation step (which frees animation controls queued for deletion), the settings, the AI's queued messages and path requests, and the effects' physics-model spawn, which adds debris bodies to the physics world. The job is safe by partition, not locks: it touches only input devices, the action maps, the UI, the message boxes, the accounts, and the sound job.

After the spin the game thread runs the dialogue, the sound post-simulation update, the vehicles' pre-update, the operations, login, rebel-support, ammunition, and mission updates, the traffic and ambient-spawn sync halves, the posted-event dispatch, the challenge, encounter, heat, and spawn-system updates, and the video, then [`GameWorld::UpdatePreSim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-UpdatePreSim), the physics phase, and the post-simulation queue, with no early return. The top of the pre-simulation queue is therefore on the game thread, after the job, and before any character, weapon, or vehicle update and the physics step.

[`GameWorld::UpdatePreSimImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##func-UpdatePreSimImpl) commits the world's update queue ([`UpdateQueue::CommitAddRemove`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/update_queue::UpdateQueue##func-CommitAddRemove)) before running it. An object registers as a pending entry and a commit makes it live; the only other commit in the tick closes the world's render update. An object registered before the pre-simulation commit is updated by the same tick's pre- and post-simulation queues; one spawned from inside the queue, as weapons spawn grenades and charges, is first updated next tick. An explosive gets its physics body from [`AmmunitionManagerUpdatePostSim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/equipment::explosive##func-AmmunitionManagerUpdatePostSim), which ends the tick after the post-simulation queue, and [`Grenade::UpdatePostSim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::Grenade##func-UpdatePostSim) reads that body unchecked, so a grenade registered before the commit faults in that tick's post-simulation queue.

The player action map is read on both sides of the spin. Inside the job, `CUIManager::PreUpdate` reads [`GUI_PAUSE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-GUI_PAUSE) and the commlink action from the player steering to open and close the pause menu and the commlink, and the screen input handlers it dispatches read the player steering too. After the spin, the challenge update reads how long [`CHALLENGE_RESET_VEHICLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-CHALLENGE_RESET_VEHICLE) has been held during a race. An effector write at the top of the world update is overwritten by the next poll before either reader sees it.

## The write API

`LocalPlayerActionMap` (a global instance pointer, modelled as a singleton) sets an action by its `Action` value. `ForceSetPressed(actionId, value)` sets an analog or held value, and `ForceSetClicked(actionId)` produces a one-frame press edge; both guard against the null-effector sentinel that invalid actions resolve to. The low-level mutators (`Click`, `Press`, `Freeze`, `ForceClick`) are on `InputDeviceEffector`.

[`ForceSetPressed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::LocalPlayerActionMap##func-ForceSetPressed) goes through `InputDeviceEffector::Press`, which zeroes [`InputDeviceEffector::m_StateTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputDeviceEffector##field-m_StateTime) on every call. Readers that tell a tap from a hold report the state time as the time held, so a press repeated through `ForceSetPressed` every tick never registers as a hold: a hold needs the effector's value, its state, and a state time that grows while held. The layout and the poll's state transitions are on [`InputDeviceEffector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputDeviceEffector).

The poll overwrites the effector values, so a value written before it does not survive. `UpdateForceClicks` touches only effectors whose [`InputDeviceEffector::m_ForceClick`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputDeviceEffector##field-m_ForceClick) flag is latched (by `ForceSetClicked` or `ForceClick`), so the press edge survives the poll, and a write made right after the poll with the flag clear is left alone. `InputDeviceManager::m_Enabled` gates the whole poll.

The menus read a separate map, [`ui_action_map`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::ui_action_map). The cutscene skip reads it too: `CSequenceObject2::UpdateSkip` and the full-screen video's `CVideoManager::UpdateSkip` ask the UI steering (`CSteeringUI`) whether [`SKIP_CUTSCENE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-SKIP_CUTSCENE) is held, only while no game screen takes input. While held the skip progress rises with the frame time and the prompt fills; released, it falls back. `CSteering::LoadGUIActionMap` binds `SKIP_CUTSCENE` to the gamepad's B. Neither function is bound in the defs.

`InputToOrbitModifier::CalculateInputDeltaAngles` carries an anti-tamper codemarker; its caller, `InputToOrbitModifier::ProcessCameraContext`, carries none.

## The mouse and UI pipeline

The UI mouse takes its position from Windows and its deltas from DirectInput:

- Position: `WndProc` on `WM_MOUSEMOVE` calls `CUIManager::SetMousePos(x, y)` (window-client pixels, the only writer of `UIManager::m_MouseX` and `m_MouseY`), which runs `CUIManager::SendMouseEvents` at once. `SendMouseEvents` also runs once per frame from `CUIManager::PreUpdate`.
- Conversion: `SendMouseEvents` maps client pixels to movie-viewport pixels by subtracting the centering offset `(m_CachedViewport − m_MovieScale) / 2` and feeds `GFx::MouseEvent`s to [`MovieImpl::HandleEvent`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##vfunc-HandleEvent). It emits a move only on frames where the DirectInput mouse reported a non-zero delta; clicks come from the steering map's `MOUSE1` and `MOUSE2` edges, the wheel from the DirectInput z axis. With a gamepad in use it parks the Scaleform mouse at [`UIManager::GAMEPAD_PARKED_MOUSE_POSITION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##const-GAMEPAD_PARKED_MOUSE_POSITION) on both axes.
- Cursor: there are two, an OS `HCURSOR` (`CGraphicsEngine::SetCursor`, driven by `COverlayUI`'s show and hide refcount through `CUIManager::MousePointerVisibility`) and the movie's `MCI_cursor` clip, moved in stage coordinates by `COverlayUI::SetMouseCursorPosition` on every move.

`CUIManager::RestoreAfterReset` (device reset, alt-tab reacquire) resizes the movie's viewport from the device whether or not the back buffer changed size. The first entry of [`MovieImpl::MouseStates`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##field-MouseStates) holds in [`MouseState::LastPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MouseState##field-LastPosition) (stage twips) the movie's post-`Advance` mouse position, the point the hit test used.

### The map's cursor and selection

The map ([`CommMapUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI)) does not take its input from Scaleform. [`CommMapUI::OnManageInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##func-OnManageInput), the only caller of `CUIManager::GetMovieSpaceMouseCursor`, reads the actions on the UI manager's steering inside `CUIManager::PreUpdate`'s dispatch (the commlink forwards its dispatch to the map), converting `GetMousePos` window-client pixels to stage space through `m_MouseDeltaX`, `m_MouseDeltaY`, and `m_MouseScaleFac`, which only `ComputeMovieSizeOnViewSize` writes. It branches on [`UIManager::m_IsUsingGamepad`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_IsUsingGamepad):

- With a gamepad, the cursor is fixed at the stage centre and the left stick pans the map under it. Whenever the map moves or zooms it picks the icon or province under that point, and a clicked `GUI_OK` opens the context menu (waypoint, fast travel, the icon's actions) there.
- With the mouse, the map selects only on a release: a released `MOUSE1` outside a drag selects at the cursor, and a released `MOUSE2` selects and opens the context menu. Only these releases write [`m_MouseCursorUiPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##field-m_MouseCursorUiPosition), so between selections it holds the last one. A held `MOUSE1` becomes a drag once the cursor leaves the press point by a few stage pixels ([`m_MouseDragging`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##field-m_MouseDragging)), and its release ends the drag instead of selecting.

In the mouse branch a clicked `GUI_OK` requests the context menu ([`m_ActivateCtxMenu`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##field-m_ActivateCtxMenu)) without selecting. [`CommMapUI::ActivateCtxMenu`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##func-ActivateCtxMenu), called later in the frame, reads `m_IsUsingGamepad` again and places the menu at the gamepad cursor or at `m_MouseCursorUiPosition`, so a `GUI_OK` with no earlier mouse selection opens the menu wherever that field was left, for whatever was last picked. Where the field is initialised is untraced; on a freshly opened map the menu appears at the stage's top-left corner.

## The semantic button layer

`CButtonMapping::EMapping` partitions mappings by mode with `END_*` markers: generic, on-foot plus context-specific on-foot, generic vehicle, land (car and motorcycle), sea (boat and jetski), air (helicopter and plane), UI, weaponized wingsuit, and mech. On foot only `MAPPING_FIRE_RIGHT` exists: dual-wield alternates barrels internally off one input, and `FIRE_LEFT` is consulted only in vehicle and mounted contexts. The mapping-to-action table is built in code (below), and `CPlayerActionObserver` keys the button-hint prompts off it ([button-hints-and-localisation.md](../button-hints-and-localisation.md)). The default key and axis bindings ship separately as `settings/keymap_gamepad.bin` and `keymap_keyboard.bin` (RTPC containers, extractable with the Gibbed tools); they bind physical inputs to actions, while the table binds each semantic mapping to the actions it consults.

## The default gamepad keymap

Extracted from `settings/keymap_gamepad.bin`, in Xbox button names. Where one button carries several mappings, the context and the mode partition pick the active one. START and BACK are the pause and commlink; the d-pad selects the weapon slots (up the dual-wield pistols, right the two-handed weapon, left the special weapon, down the explosives).

| Context | Bindings |
|---|---|
| On foot | move L stick; look R stick; fire (`FIRE_RIGHT`) RT; fire grapple LB; retract tethers (overloaded with `FIRE_LEFT`) LT; release and push tethers B; throw grenade, and plant and detonate explosives in context, RB; reload X; holster hold X; jump, stunt, open parachute A; use item, enter vehicle, open wingsuit Y; melee B in context; precision aim and sniper zoom R3; in the reel-in context, cancel B, boost hold LT, hang jump A, slingshot A |
| Land vehicles | steer, and motorcycle lean and tilt, L stick; accelerate and reverse RT and LT; handbrake X; nitrous and turbo jump hold B and tap B; vehicle weapons primary and secondary RB and LB; motorcycle personal weapon RB; gunner seat hold Y; roof stunt A; exit Y; horn L3; look back, vehicle camera, and recenter R3 |
| Helicopter | collective RT and LT; cyclic (forward, back, roll) L stick; yaw R stick X; weapons RB and LB; exit Y, stunt A, nitrous hold B |
| Plane | pitch and roll L stick; rudder X and B; thrust RT and LT; weapons RB and LB; exit Y, stunt A |
| Boats and jetskis | steer L stick; accelerate and reverse RT and LT; fire and the jetski personal weapon RB; nitrous and turbo jump hold B and tap B; exit Y |
| Wingsuit and parachute | steer L stick; air brake (`CANCEL`, upgrade-gated) hold B; open or close the parachute A; grapple (slingshot boost) LB; weaponized: boost RT, brake LT, weapons RB and X, evade (with a stick flick) and take off (held) Y |
| Mech | move L stick; punch LB; jump B; gravity gun attract on hold and throw on release LT; gravity gun drop RB; right-hand weapon RT |
| Everywhere | skip cutscene (from the UI map) hold B; look at the objective hold R3; release a tether fired from a vehicle d-pad down |

`MOVE_TO_DRIVERS_SEAT` has no binding in either keymap and no gameplay reader passes it to a steering as an immediate, so a passenger reaches the driver's seat only by leaving and entering again.

## Leaving and changing seats

A seated player leaves and changes seat through two input tasks of the character's animation graph, and both read `ENTER_VEHICLE`, not `EXIT_VEHICLE`: `EXIT_VEHICLE` only names the exit's button hint (`MAPPING_EXIT_VEHICLE`), and the gamepad's Y carries both actions.

- [`NStateTask_InputVehicleExitTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::NStateTask_InputVehicleExitTask##func-Update) leaves the seat on the frame the button is released, only if it was pressed for no longer than [`m_Time`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::InputVehicleExitTaskProperties##field-m_Time): the exit is a tap, and a longer press does nothing on release.
- [`NStateTask_InputVehicleEnterTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::NStateTask_InputVehicleEnterTask##func-Update) changes seat once the button has been held for [`m_Holdtime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::InputVehicleEnterTaskProperties##field-m_Holdtime), while still held, where a change is available: a transition from the player's node to a gunner seat ([`HasAvailableGunnerDestination`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::NStateTask_InputVehicleEnterTask##func-HasAvailableGunnerDestination)), the driver's seat from a gunner seat, or a reachable cargo bay from the driver's seat. With no change available it queues `ACT_ENTER_VEHICLE` on every pressed frame; with one available, a release short of the hold time queues it once.

Both times are task properties in `rico_base`, equal in the shipped graph. The exit task runs in every seated state (the driver's and passenger's idle, forward, and reverse states, the motorcycle's riding states, and the mounted-weapon states). The enter task runs beside it only in the car-style driver's idle and reverse states, the passenger's idle state, and the spinning mounted weapon's idle state, so the seat change is offered only there.

The driver's states, the motorcycle's riding states, and the spinning mounted weapon's idle state also run an `INPUTHOLD` task on `OPEN_PARACHUTE` (A) that queues `ACT_TRIGGER_PARACHUTE` after the task's hold time. The act leads to `S_EXIT_VEHICLE_PARA`, which leaves the seat with the parachute open, only while `C_VEHICLE_IS_FAST_ENOUGH_FOR_PARA_TRANSITION` holds: a `CGraphParentVehicleSpeedCondition` comparing the squared length of the vehicle's ground-relative velocity at the character ([`Vehicle::GetGroundRelativePointVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-GetGroundRelativePointVelocity)) against a squared minimum and maximum, both `rico_base` properties (the maximum far beyond any vehicle's speed; the unit is unverified, presumed metres per second), together with the interaction graph's exit verification. The passenger's states have no such task. A tap of Y queued before the hold completes exits the ordinary way first.

## The mapping → action table

[`ButtonMapping::PopulateMappings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::ButtonMapping##func-PopulateMappings) hard-codes every entry; [`ButtonMapping::Init`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::ButtonMapping##func-Init) calls it and then `RefreshAllActionsForUi`, which builds `m_AllActionsForUi` (a `forward_list<CSteering::EAction>`) for the controls screen. Each [`Mapping`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::Mapping) has a [`MappingInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::MappingInfo) of dwords only: `m_Interaction`, `m_Action1`, `m_Action2` (for chords), `m_DescriptionKey` (the `buttonhint_*` key), `m_RequiredUpgrade` and `m_RequiredAbility` (bare hashes), and one packed dword of the `Customizable`, `ShowInUIMapping`, `PCOnly`, and `ControllerOnly` flags. The actions are `Action` values, the same space as `CSteering::EAction`; the decompiler renders some as OR-expressions of the unrelated `CGame::EGameAction` (an `A_SCREENSHOT|…` expression for [`Action::FIRE_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-FIRE_GRAPPLE)), a display artifact. The table below is verified end to end against `PopulateMappings`, actions, keys, gates, and flags.

Interactions ([`ButtonInteraction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::ButtonInteraction)): tap is `BINT_SINGLE_BUTTON_TAP`, hold `BINT_SINGLE_BUTTON_HOLD`, release `BINT_SINGLE_BUTTON_RELEASE`, and chord `BINT_TWO_BUTTON_HOLD_AND_TAP` (`m_Action1` held and `m_Action2` tapped). Every `EMapping` value is populated except the `END_*` markers, which keep the constructor default with `m_Action1` at the invalid-action sentinel `Action::COUNT`. Where a row below lists several mappings, each maps to the action of its own name with the interaction given.

### On-foot (generic and `FIRST_ON_FOOT_MAPPING`..`LAST_ON_FOOT_MAPPING`)

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_PAUSE`, `MAPPING_COMMLINK` | tap | `GUI_PAUSE`, `GUI_PDA` | generic |
| `MAPPING_FIRE_GRAPPLING_HOOK` | tap | `FIRE_GRAPPLE` | ability `grappling_hook` |
| `MAPPING_RETRACT_TETHERS` | tap | `RETRACT_GRAPPLE` | ability `retract_tether` |
| `MAPPING_RELEASE_TETHERS_NON_CHORDED` | tap | `RELEASE_GRAPPLE` | PC only |
| `MAPPING_FIRE_RIGHT` | tap | `FIRE_RIGHT` | the sole on-foot fire input |
| `MAPPING_THROW_GRENADE` | tap | `THROW_GRENADE` | ability `grenades` |
| `MAPPING_DETONATE_EXPLOSIVE_GAMEPAD` | hold | `DETONATE_EXPLOSIVE` | ability `planted_explosives`; controller only |
| `MAPPING_DETONATE_EXPLOSIVE_PC` | tap | `DETONATE_EXPLOSIVE_TAP` | ability `planted_explosives`; PC only |
| `MAPPING_RELOAD`, `MAPPING_JUMP`, `MAPPING_CANCEL`, `MAPPING_USE_ITEM` | tap | same name | |
| `MAPPING_OPEN_WINGSUIT` | tap | `OPEN_WINGSUIT` | ability `wingsuit` |
| `MAPPING_OPEN_PARACHUTE` | tap | `OPEN_PARACHUTE` | ability `parachute` |
| `MAPPING_HAMMER` | tap | `PUSH_GRAPPLE` | ability `hammer`; melee reuses the grapple-push action, `buttonhint_hammer` |
| `MAPPING_PRECISION_AIM` | tap | `PRECISION_AIM` | upgrade `precision_aim` |
| `MAPPING_SELECT_DUAL_WIELD` | tap | `SELECT_DUEL_WIELD` | |
| `MAPPING_SELECT_TWO_HANDED`, `MAPPING_SELECT_TWO_HANDED_SPECIAL` | tap | same name | |
| `MAPPING_SELECT_EXPLOSIVE` | tap | `SELECT_EXPLOSIVES` | ability `planted_explosives` |
| `MAPPING_PREV_WEAPON`, `MAPPING_NEXT_WEAPON` | tap | same name | PC only |
| `MAPPING_MOVE_FORWARD`, `_BACKWARD`, `_LEFT`, `_RIGHT` | hold | same name | |
| `MAPPING_WALK` | hold | `WALK` | PC only |

### Context-specific on-foot

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_RELEASE_TETHERS_WITH_RETRACT` | tap | `RELEASE_GRAPPLE` | controller only |
| `MAPPING_DETACH_MOUNTED_GUN` | tap | `RELOAD` | |
| `MAPPING_CANCEL_REEL_IN` | tap | `CANCEL` | reel-in context |
| `MAPPING_SLINGSHOT_TO_REEL_IN` | tap | `OPEN_PARACHUTE` | reel-in context |
| `MAPPING_PLANT_EXPLOSIVE` | tap | `PLANT_EXPLOSIVE` | |
| `MAPPING_HANG_JUMP` | tap | `JUMP` | reel-in context |
| `MAPPING_SNIPER_ZOOM` | tap | `PRECISION_AIM` | no upgrade gate, unlike `MAPPING_PRECISION_AIM` |
| `MAPPING_SKIP_CUTSCENE`, `MAPPING_LOOK_AT` | hold | same name | |
| `MAPPING_AIR_BRAKE` | hold | `CANCEL` | upgrade `wingsuit_air_brake` |
| `MAPPING_SLINGSUIT_TO_REEL_IN` | tap | `OPEN_WINGSUIT` | upgrade `slingsuit_to_reel` |
| `MAPPING_HOLSTER_WEAPON` | hold | `RELOAD` | holster is hold-reload |
| `MAPPING_REEL_BOOST` | hold | `RETRACT_GRAPPLE` | reel-in context |
| `MAPPING_KEEP_RAGDOLLING` | hold | `CANCEL` | hidden in UI mapping |

### Generic vehicle

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_LOOK_BACK_CAM` | hold | `LOOK_BACK_CAM` | |
| `MAPPING_RECENTER_CAM` | tap | `VEHICLE_CAM` | |
| `MAPPING_EXIT_VEHICLE` | tap | `EXIT_VEHICLE` | hidden in UI mapping |
| `MAPPING_ATTACHED_TO_PARACHUTE` | hold | `OPEN_PARACHUTE` | |
| `MAPPING_TOGGLE_MAGNET` | tap | `FIRE_VEHICLE_WEAPON_SECONDARY` | hidden in UI mapping |
| `MAPPING_VEHICLE_RELEASE_GRAPPLE` | tap | `VEHICLE_RELEASE_GRAPPLE` | |

### Land vehicle: generic, car, and motorcycle

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_HANDBRAKE` | tap | `HANDBRAKE` | |
| `MAPPING_ACCELERATE_LAND_VEHICLE`, `MAPPING_REVERSE_LAND_VEHICLE` | tap | `ACCELERATE`, `REVERSE` | |
| `MAPPING_NITROUS_LAND_VEHICLE` | hold | `USE_VEHICLE_MOD` | upgrade `land_vehicle_nitrous_unlock` |
| `MAPPING_TURBO_JUMP_LAND_VEHICLE` | tap | `USE_VEHICLE_MOD` | upgrade `land_vehicle_turbo_jump_unlock` |
| `MAPPING_SOUND_HORN_SIREN_LAND_VEHICLE` | tap | `SOUND_HORN_SIREN` | |
| `MAPPING_TURN_LEFT`, `MAPPING_TURN_RIGHT` | tap | same name | car |
| `MAPPING_ENTER_GUNNER_SEAT` | hold | `ENTER_VEHICLE` | hidden in UI mapping |
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_CAR`, `_SECONDARY_CAR` | tap | `FIRE_VEHICLE_WEAPON_PRIMARY`, `_SECONDARY` | |
| `MAPPING_STUNT_JUMP_CAR` | tap | `STUNT_JUMP` | |
| `MAPPING_FIRE_WEAPON_MOTORCYCLE` | tap | `MC_FIRE` | personal weapon |
| `MAPPING_RELOAD_WEAPON_MOTORCYCLE` | tap | `MC_RELOAD` | |
| `MAPPING_HOLSTER_WEAPON_MOTORCYCLE` | hold | `MC_RELOAD` | holster is hold-reload |
| `MAPPING_TILT_FORWARD_MOTORCYCLE`, `TILT_BACKWARD`, `LEAN_LEFT`, `LEAN_RIGHT` | tap | `BIKE_TILT_FORWARD`, `BIKE_TILT_BACKWARD`, `BIKE_LEAN_LEFT`, `BIKE_LEAN_RIGHT` | |

### Sea vehicle: generic, boat, and jetski

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_NITROUS_SEA_VEHICLE` | hold | `USE_VEHICLE_MOD` | upgrade `sea_vehicle_nitrous_unlock` |
| `MAPPING_TURBO_JUMP_SEA_VEHICLE` | tap | `USE_VEHICLE_MOD` | upgrade `sea_vehicle_turbo_jump_unlock` |
| `MAPPING_TURN_LEFT_SEA_VEHICLE` | tap | `ACCELERATE` | actually forward: `buttonhint_forward_sea_vehicle` |
| `MAPPING_TURN_RIGHT_SEA_VEHICLE` | tap | `REVERSE` | actually backward: `buttonhint_backward_sea_vehicle` |
| `MAPPING_SOUND_HORN_SIREN_SEA_VEHICLE` | tap | `SOUND_HORN_SIREN` | |
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_BOAT` | tap | `FIRE_VEHICLE_WEAPON_PRIMARY` | |
| `MAPPING_STUNT_JUMP_BOAT` | tap | `STUNT_JUMP` | |
| `MAPPING_FIRE_WEAPON_JETSKI` | tap | `MC_FIRE` | personal weapon |
| `MAPPING_RELOAD_WEAPON_JETSKI` | tap | `MC_RELOAD` | |
| `MAPPING_HOLSTER_WEAPON_JETSKI` | hold | `MC_RELOAD` | holster is hold-reload |

### Air vehicle: generic, helicopter, and plane

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_AIR_VEHICLE`, `_SECONDARY_AIR_VEHICLE` | tap | `FIRE_VEHICLE_WEAPON_PRIMARY`, `_SECONDARY` | |
| `MAPPING_HELI_INC_ALTITUDE`, `_DEC_ALTITUDE`, `_ROLL_RIGHT`, `_ROLL_LEFT`, `_TURN_LEFT`, `_TURN_RIGHT`, `_FORWARD`, `_BACKWARD` | tap | same name | |
| `MAPPING_STUNT_JUMP_HELICOPTER`, `MAPPING_STUNT_JUMP_PLANE` | tap | `STUNT_JUMP` | |
| `MAPPING_NITROUS_HELICOPTER` | hold | `USE_VEHICLE_MOD` | upgrade `air_vehicle_nitrous_unlock` |
| `MAPPING_PLANE_TURN_RIGHT`, `_TURN_LEFT`, `_ROLL_RIGHT`, `_ROLL_LEFT`, `_PITCH_UP`, `_PITCH_DOWN` | tap | same name | |
| `MAPPING_PLANE_INC_THRUST`, `MAPPING_PLANE_DEC_THRUST` | tap | `PLANE_INC_TRUST`, `PLANE_DEC_TRUST` | |
| `MAPPING_NITROUS_PLANE` | hold | `PLANE_INC_TRUST` | upgrade `air_vehicle_nitrous_unlock` |

### The plane's thrust and nitrous

[`AirPlane::UpdateVehicleInputsFromActionMap`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane##func-UpdateVehicleInputsFromActionMap) runs on top of the vehicle's own reader each tick unless the plane is destroyed, its driver is knocked out or dead, an AI's input has gone stale, or it is under an EMP. It reads the turn, roll, and pitch from the `PLANE_TURN_*`, `PLANE_ROLL_*`, and `PLANE_PITCH_*` pairs, the throttle as `PLANE_INC_TRUST` less `PLANE_DEC_TRUST` into [`Vehicle::m_SteeringAcceleration`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringAcceleration), and `PLANE_DEC_TRUST` alone as the brake ([`Vehicle::m_SteeringBrake`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringBrake)). Both are values, not rates: the thrust is where the trigger is.

The nitrous has no input of its own. With a living driver and the plane in control, a `PLANE_INC_TRUST` value above [`AirPlane::NITROUS_THRUST_THRESHOLD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane) raises [`Vehicle::m_NitrousInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_NitrousInput) to it, and otherwise leaves it. `MAPPING_NITROUS_PLANE`'s hold describes only the button hint.

### UI, weaponized wingsuit, and mech

The UI mappings are all tap and not customizable: `MAPPING_UI_OK`, `_CANCEL`, `_TAB_NEXT`, `_TAB_PREV`, `_FILTER`, `_RECENTER`, `_PAGE_NEXT`, and `_PAGE_PREV` map to `GUI_OK`, `GUI_CANCEL`, and so on. `MAPPING_ACTIVATE_BAVARIUM_SHIELD` and `MAPPING_CHALLENGE_RESET_VEHICLE` sit past `END_MECH_MAPPINGS` with no section of their own.

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_WEAPONIZED_WINGSUIT_TAKEOFF` | hold | `WINGSUIT_TAKEOFF` | |
| `MAPPING_WEAPONIZED_WINGSUIT_BOOST` | tap | `WINGSUIT_BOOST` | |
| `MAPPING_WEAPONIZED_WINGSUIT_BRAKE` | tap | `WINGSUIT_AIRBRAKE` | |
| `MAPPING_WEAPONIZED_WINGSUIT_EVADE` | chord | `WINGSUIT_EVADE` held + `MOVE_ALL` tapped | |
| `MAPPING_WEAPONIZED_WINGSUIT_PRIMARY_WEAPON`, `_SECONDARY_WEAPON` | tap | `FIRE_WINGSUIT_WEAPON_MAIN`, `_SECONDARY` | |
| `MAPPING_MECH_MOVE_FORWARD`, `_BACKWARD` | hold | `PLANE_PITCH_DOWN`, `PLANE_PITCH_UP` | reuse the plane actions |
| `MAPPING_MECH_MOVE_LEFT`, `_RIGHT` | hold | `TURN_LEFT`, `TURN_RIGHT` | |
| `MAPPING_MECH_JUMP`, `MAPPING_MECH_PUNCH` | tap | same name | |
| `MAPPING_MECH_GRAVITY_GUN_ATTRACT` | hold | `MECH_FIRE_GRAVITY_WEAPON_PRIMARY` | |
| `MAPPING_MECH_GRAVITY_GUN_DROP` | tap | `MECH_FIRE_GRAVITY_WEAPON_SECONDARY` | |
| `MAPPING_MECH_GRAVITY_GUN_THROW` | release | `MECH_FIRE_GRAVITY_WEAPON_PRIMARY` | |
| `MAPPING_MECH_FIRE_RIGHT_HAND_WEAPON` | tap | same name | |
| `MAPPING_ACTIVATE_BAVARIUM_SHIELD` | tap | `SOUND_HORN_SIREN` | reuses the horn action |
| `MAPPING_CHALLENGE_RESET_VEHICLE` | hold | `CHALLENGE_RESET_VEHICLE` | |

Several actions do double duty, separated only by context, gate, or interaction: holster is hold-`RELOAD`; melee is [`Action::PUSH_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PUSH_GRAPPLE); the sniper zoom and the gated precision aim are both [`Action::PRECISION_AIM`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PRECISION_AIM); every class's nitrous and turbo jump are [`Action::USE_VEHICLE_MOD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-USE_VEHICLE_MOD), told apart by upgrade hash and tap or hold; and the sea vehicle's turns are forward and reverse.

### Gating (`IsMappingActive`) and consumption

`CButtonMapping::IsMappingActive(m, player)` is not located (the IDB function of that name is a mislabelled animation state-machine condition that takes a `CStateMachineInstance` and checks one upgrade), so this gating is unconfirmed. A mapping is active unless `m_RequiredAbility` is set, a player is present, and `CAbilitiesHandler::IsEnabled(player, ability)` is false, or `m_RequiredUpgrade` is set and [`ProfileManager::GetUpgradeInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::ProfileManager##func-GetUpgradeInfo)`(upgrade)` does not report both `m_IsPurchased` and `m_IsEnabled`.

The button-hint layer consumes the table through two range queries over a `[from, to_inclusive]` `EMapping` sub-range (the `FIRST_*` and `LAST_*` bounds of the player's mode), each with an `only_active` flag that applies `IsMappingActive`:

- `GetMappingInfosForKey(player, key, from, to, only_active, force_keyboard, force_gamepad)` collects every `SMappingInfo` whose `m_Action1` or `m_Action2` resolves through `CUIBase::GetKeyID` to the physical key, skipping the `Action::COUNT` sentinel: what a button does right now.
- `GetMappingInfosForAction(player, action, from, to, only_active)` collects every mapping whose `m_Action1` or `m_Action2` equals the action: how a prompt for an action finds its glyph.

## Analog axes read by vehicles and the wingsuit

The vehicle and wingsuit controllers read actions through `CActionMap::GetValue`, which returns the effector's float, so analog values pass through unchanged. The per-class readers are `UpdateVehicleInputsFromActionMap` overrides; none uses the camera matrices. The following is unconfirmed (medium-high confidence):

- Base vehicle (`CVehicle`): [`HANDBRAKE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HANDBRAKE) as a pressed state, [`FIRE_VEHICLE_WEAPON_PRIMARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-FIRE_VEHICLE_WEAPON_PRIMARY) and [`FIRE_VEHICLE_WEAPON_SECONDARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-FIRE_VEHICLE_WEAPON_SECONDARY) through `UpdateVehicleActionInput`, [`SOUND_HORN_SIREN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-SOUND_HORN_SIREN) for the Bavarium shield, and [`VEHICLE_RELEASE_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-VEHICLE_RELEASE_GRAPPLE) on release. Vehicles with strafing wheel steering (the mech) use [`MECH_FIRE_RIGHT_HAND_WEAPON`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MECH_FIRE_RIGHT_HAND_WEAPON), [`MECH_FIRE_GRAVITY_WEAPON_PRIMARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MECH_FIRE_GRAVITY_WEAPON_PRIMARY), and [`MECH_FIRE_GRAVITY_WEAPON_SECONDARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MECH_FIRE_GRAVITY_WEAPON_SECONDARY) instead.
- Land vehicles (`CLandVehicle`), all floats: acceleration [`ACCELERATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-ACCELERATE), reverse [`REVERSE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-REVERSE), turn `TURN_RIGHT − TURN_LEFT`, yaw `PLANE_TURN_RIGHT − PLANE_TURN_LEFT`, roll `PLANE_ROLL_RIGHT − PLANE_ROLL_LEFT`, and pitch `PLANE_PITCH_UP − PLANE_PITCH_DOWN`. Movement is zeroed while `m_LockMovement` is set.
- Helicopters (`CHelicopter`): roll `HELI_ROLL_RIGHT − HELI_ROLL_LEFT`; yaw `HELI_TURN_RIGHT − HELI_TURN_LEFT` plus, for the driver, the mouse device's values for the same two actions scaled by the yaw mouse sensitivity, the sum clamped to ±1; fore and aft `−(HELI_FORWARD − HELI_BACKWARD)`.
- Planes are above; boats and motorbikes were not read individually.

The wingsuit is not a vehicle. `NStateTask_MovementWingSuitTask::Update` reads x as [`MOVE_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_RIGHT) − [`MOVE_LEFT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_LEFT) and y from [`MOVE_FORWARD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_FORWARD) and [`MOVE_BACKWARD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_BACKWARD) (the y term's decompile is garbled; medium confidence) and passes them to `NAirMovement::UpdateAirPhysics`, which maps them to yaw, pitch, and roll through the tuning's `m_XInputYawPitchRollAmount` and `m_YInputYawPitchRollAmount`, and for the local player adds the camera-driven `NAirMovement::UpdateStdLookSteering`. While stalling, [`CANCEL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-CANCEL) closes the wingsuit unless the grapple has an active wire or [`RETRACT_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-RETRACT_GRAPPLE) is held. The same `Update` has an `NAntiTamper` branch that corrupts the movement settings on detected tampering; its address is not located. The parachute's inputs are in [parachute-locomotion.md](parachute-locomotion.md).

## Weapon selection, rebel drops, and gear mods

There is a weapon selection cross but no radial wheel and no slow-motion selector: the slot-select actions are read into the character's weapon inputs and acted on by [`Inventory::UpdateWeaponHandling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-UpdateWeaponHandling), not the animation state machine ([weapon-selection.md](weapon-selection.md)). `CGamePauseUtility` is a script facility that pauses the game until an action is pressed, used by tutorials, not a weapon selector. Rebel drops are ordered from the PDA UI (`CRebelSupport`), not through a gameplay action (medium confidence).

Gear mods have no gameplay input: `CCommSkillUI::HandleCommand` receives an upgrade id and a boolean from the movie and sets `SUpgradeInfo::m_IsEnabled`. Gameplay code tests both [`m_IsPurchased`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::UpgradeInfo##field-m_IsPurchased) and [`m_IsEnabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::UpgradeInfo##field-m_IsEnabled) on the [`UpgradeInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::UpgradeInfo) that `ProfileManager::GetUpgradeInfo(hash)` returns (for example `CGrenade::PropelToward` for the homing upgrade).

## No quick-time events

No QTE or button-mash logic is found (medium confidence). `SInputMash` (a spring-driven gauge with a timeout) is embedded in `CControlParameters` and constructed, but nothing located updates it, and the binary has no QTE or mash strings. Interactions, hijacks, and use-item prompts are driven by the animation state machine through plain button conditions (`CButtonPressedCondition`, one action id), and the use-item progress bar (`NSetHUDUseItemProgressTask`) only reflects how far the use animation has played. Melee ([`NStateTask_InputMeleeTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_InputMeleeTask##func-Update)) requires the hammer ability, targets the [`TargetType::TARGET_MELEE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::target::TargetType##variant-TARGET_MELEE) slot of [`PlayerAimControl::m_Target`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_Target), and queues acts ([melee-and-damage.md](melee-and-damage.md)).
