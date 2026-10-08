# Input

Just Cause 3's local-player input runs through a single action-effector map plus a separate movie-facing mouse pipeline for the Scaleform UI. The sections below cover the effector accessors, the action enumeration, the write API, an anti-tamper protection in the camera input path, the mouse and UI pipeline, and the semantic button-mapping layer above the raw actions.

## Action effectors

The engine exposes input through action effectors: [`LocalPlayerActionMap::GetActionEffector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::LocalPlayerActionMap##func-GetActionEffector) returns an `SEffectorElement` per action, and there is a `Freeze` path. The camera look axes are `LOOK_LEFT` / `LOOK_RIGHT` / `LOOK_UP` / `LOOK_DOWN` (net `yaw = right − left`, `pitch = up − down`), and the move axes are `MOVE_LEFT` / `MOVE_RIGHT` / `MOVE_FORWARD` / `MOVE_BACKWARD`. All of these resolve through the [`Action`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action) enum (see below). The gameplay buttons (fire, jump, grapple, reload, enter-vehicle, …) are likewise enumerated.

## The action enumeration

The action-name table, [`action_name_table`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::action_name_table), is a fixed array of name pointers indexed by action, built once at startup. It has [`Action::COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action::COUNT) entries. The numbering is byte-stable across builds. Every entry is captured in the [`Action`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action) enum (`input/input_action_map.pyxis`), from `Action::PAUSE` through `Action::GUI_USE_BUTTON`, and the effector accessors and the write API take it directly. (Runtime key/axis bindings load separately via `LoadActionMapFromFile`, but the action numbering itself is static.)

## The write API

The write API is on `LocalPlayerActionMap` (its instance pointer is a global; the def models it as a singleton). It sets an action by its `Action` value. `ForceSetPressed(actionId, value)` sets an analog or held value (sticks, triggers, held buttons), and `ForceSetClicked(actionId)` produces a one-frame press edge. Both guard against the null-effector sentinel that invalid actions resolve to. The low-level effector mutators (`Click`, `Press`, `Freeze`, `ForceClick`) are on `InputDeviceEffector`.

[`ForceSetPressed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::LocalPlayerActionMap##func-ForceSetPressed) goes through `InputDeviceEffector::Press`, which zeroes the effector's state time ([`InputDeviceEffector::m_StateTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputDeviceEffector##field-m_StateTime)) on every call. The readers that tell a tap from a hold report that state time as the time held, so a press repeated through `ForceSetPressed` every tick never registers as a hold. A held input has to be written as the effector's whole state instead: its value, its state, and a state time that grows while it is held. The effector's layout and the poll's state transitions are documented on [`InputDeviceEffector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputDeviceEffector).

The game's menus read a separate action map, [`ui_action_map`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::ui_action_map), not the player's map.

[`InputDeviceManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##func-Update)`(dt)` is the per-frame device poll. It overwrites the effector values, so a value written before it in the frame does not survive. `Update` runs `UpdateForceClicks` after polling, which is the `m_ForceClick` mechanism: a click latched through `ForceSetClicked` or `ForceClick` sets the effector's [`InputDeviceEffector::m_ForceClick`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputDeviceEffector##field-m_ForceClick) flag, so the press edge survives the poll instead of being cleared by it. `InputDeviceManager::m_Enabled` gates the whole poll, and clearing it freezes device input.

## Anti-tamper protection

`InputToOrbitModifier::CalculateInputDeltaAngles` carries an anti-tamper codemarker. Its caller, `InputToOrbitModifier::ProcessCameraContext`, carries none.

## The mouse and UI pipeline (issue #9)

The game's UI mouse is fed by a single choke point, and it is position-from-Windows, deltas-from-DirectInput:

- **Position**: `WndProc` on `WM_MOUSEMOVE` calls `CUIManager::SetMousePos(x, y)` (window-client pixels, the only writer of `UIManager::m_MouseX/m_MouseY`), which immediately runs `CUIManager::SendMouseEvents`. `SendMouseEvents` also runs once per frame from `CUIManager::PreUpdate`.
- **Conversion**: `SendMouseEvents` maps client pixels to movie-viewport pixels by subtracting the centering offset `(m_CachedViewport − m_MovieScale) / 2`, then feeds `GFx::MouseEvent`s to the virtual [`MovieImpl::HandleEvent`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##vfunc-HandleEvent). A move event is only emitted on frames where the DirectInput mouse reported a non-zero delta; clicks come from the steering action map's `MOUSE1`/`MOUSE2` effector edges, the wheel from the DirectInput z axis. With a gamepad in use it parks the Scaleform mouse at `(−1000, −1000)`.
- **Cursor sprite**: the cursor is doubled — an OS `HCURSOR` (`CGraphicsEngine::SetCursor`, driven by `COverlayUI`'s show/hide refcount via `CUIManager::MousePointerVisibility`) and the movie's own `MCI_cursor` clip, repositioned in stage coordinates through `COverlayUI::SetMouseCursorPosition` on every move.

Two further engine seams bear on that pipeline:

- **The engine's reset path**: `CUIManager::RestoreAfterReset` (device reset, alt-tab reacquire) resizes the movie's viewport from the device without the back buffer necessarily changing size.
- **The map's own conversion**: `CCommMapUI::OnManageInput` — the only caller of `CUIManager::GetMovieSpaceMouseCursor` — converts `GetMousePos` window-client pixels to stage space through `m_MouseDeltaX/Y`/`m_MouseScaleFac`, which only `ComputeMovieSizeOnViewSize` writes.

### The map's cursor and selection

The map ([`CommMapUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI)) does not take its input from Scaleform. [`CommMapUI::OnManageInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##func-OnManageInput) reads the actions itself, on the UI manager's steering, inside `CUIManager::PreUpdate`'s input dispatch (the commlink forwards its own dispatch to the map), and it has two branches, chosen by [`UIManager::m_IsUsingGamepad`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_IsUsingGamepad). The two select in different places:

- With a gamepad, the cursor is a fixed point at the centre of the stage and the left stick pans the map under it. Whenever the map moves or zooms, the map picks the icon or province under that point, and a clicked `GUI_OK` opens the context menu (place a waypoint, fast travel, the icon's actions) there.
- With the mouse, the cursor is wherever the mouse is, but the map selects only on a button release: a released `MOUSE1` outside a drag selects at the cursor, and a released `MOUSE2` selects there and opens the context menu. Either release writes the cursor to [`m_MouseCursorUiPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##field-m_MouseCursorUiPosition), and nothing else does, so between selections it holds the last one. A held `MOUSE1` becomes a drag once the cursor leaves the press point by a few stage pixels (see [`m_MouseDragging`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##field-m_MouseDragging)), and its release then ends the drag instead of selecting.

A clicked `GUI_OK` requests the context menu in the mouse branch too ([`m_ActivateCtxMenu`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##field-m_ActivateCtxMenu)), but it does not select. [`CommMapUI::ActivateCtxMenu`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##func-ActivateCtxMenu), which the map's update calls later in the frame, reads `m_IsUsingGamepad` again and places the menu at the gamepad cursor or at `m_MouseCursorUiPosition`. In the mouse branch, a `GUI_OK` with no earlier mouse selection therefore opens the menu wherever that field was left, for whatever was last picked. Where the field is initialised was not traced; on a freshly opened map the menu has been seen at the stage's top-left corner.

For ground-truth debugging, the [`MouseState::LastPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MouseState##field-LastPosition) field of the first entry in [`MovieImpl::MouseStates`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##field-MouseStates) (stage twips) holds the movie's post-`Advance` mouse position — the exact point the hit test used.

## The semantic button layer

`CButtonMapping::EMapping` partitions mappings by mode with `END_*` markers: generic, on-foot plus context-specific on-foot, generic vehicle, land (car/motorcycle), sea (boat/jetski), air (heli/plane), UI, weaponized wingsuit, and mech. On foot only `MAPPING_FIRE_RIGHT` exists — there is no fire-left; dual-wield alternates barrels internally off one input, and `FIRE_LEFT` is consulted only in vehicle/mounted contexts. The mapping-to-action table is **code-built** (not a data file), recovered below. `CPlayerActionObserver` keys button-hint prompts off it. The default *key/axis* bindings ship separately as `settings/keymap_gamepad.bin` and `keymap_keyboard.bin` (RTPC containers, extractable with the Gibbed tools per the RE skill; see [the default gamepad keymap](#the-default-gamepad-keymap) for the extracted gamepad layout); those bind physical inputs to the actions, while the table below binds each semantic mapping to the action or actions it consults.

## The default gamepad keymap

The default gamepad bindings, extracted from `settings/keymap_gamepad.bin`, grouped by the game's mapping sections. Buttons use Xbox names. Where one button carries several mappings, the active one depends on the context and the mode partition described in [the semantic button layer](#the-semantic-button-layer). Bindings not listed here (the pause and commlink buttons, the d-pad directions for each weapon slot, and the weaponized wingsuit's buttons) have not been extracted.

### On foot

| Mapping | Button |
|---|---|
| Move | L stick |
| Look | R stick |
| Fire (`FIRE_RIGHT`; dual-wield alternates barrels) | RT |
| Fire grapple (`FIRE_GRAPPLE`) | LB |
| Retract tethers (`RETRACT_GRAPPLE`, overloaded with `FIRE_LEFT`) | LT |
| Release and push tethers | B |
| Throw grenade | RB |
| Plant and detonate explosives | RB (context) |
| Reload | X |
| Holster | hold X |
| Jump, stunt, open parachute | A |
| Use item, enter vehicle, open wingsuit | Y |
| Melee (`MAPPING_HAMMER`) | B (context) |
| Weapon slot select | d-pad |
| Precision aim and sniper zoom | R3 |
| Reel-in context: cancel, boost, hang jump, slingshot | B, hold LT, A, A |

### Land vehicles

| Mapping | Button |
|---|---|
| Steer, and motorcycle lean and tilt | L stick |
| Accelerate and reverse | RT and LT |
| Handbrake | X |
| Nitrous and turbo jump (`USE_VEHICLE_MOD`) | hold B and tap B |
| Fire vehicle weapons, primary and secondary | RB and LB |
| Motorcycle personal weapon (`MC_FIRE`) | RB |
| Enter gunner seat, roof stunt | hold Y, A |
| Exit vehicle | Y |
| Horn | L3 |
| Look back, vehicle camera, recenter camera | R3 |

### Helicopter

| Mapping | Button |
|---|---|
| Collective up and down | RT and LT |
| Cyclic (forward, back, and roll) | L stick |
| Yaw | R stick X |
| Fire primary and secondary | RB and LB |
| Exit, stunt, nitrous | Y, A, hold B |

### Plane

| Mapping | Button |
|---|---|
| Pitch and roll | L stick |
| Rudder | X and B |
| Thrust up and down | RT and LT |
| Fire primary and secondary | RB and LB |
| Exit, stunt | Y, A |

### Boats and jetskis

| Mapping | Button |
|---|---|
| Steer | L stick |
| Accelerate and reverse | RT and LT |
| Fire, and the jetski personal weapon | RB |
| Nitrous and turbo jump | hold B and tap B |
| Exit | Y |

### Wingsuit and parachute

| Mapping | Button |
|---|---|
| Steer | L stick |
| Air brake (`CANCEL`, upgrade-gated) | hold B |
| Open or close the parachute | A |
| Grapple (slingshot boost) | LB |

## The mapping → action table

Recovered from [`ButtonMapping::PopulateMappings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::ButtonMapping##func-PopulateMappings), which hardcodes every entry; there is no data file. [`ButtonMapping::Init`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::ButtonMapping##func-Init) calls `PopulateMappings` and then `RefreshAllActionsForUi`. Each [`Mapping`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::Mapping) slot has a [`MappingInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::MappingInfo) holding a `CSteering::EAction m_Action1` (and optional `m_Action2` for chords), an `EButtonInteraction m_Interaction`, a `m_DescriptionKey` (the `buttonhint_*` localization key), optional `m_RequiredAbility` / `m_RequiredUpgrade` hashes, and the `m_Customizable` / `m_ShowInUIMapping` / `m_PCOnly` / `m_ControllerOnly` flags.

The actions are values of the [`Action`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action) enum, which is the same action space as `CSteering::EAction`. The decompiler renders some values as OR-expressions of an unrelated `CGame::EGameAction`, for example `A_SCREENSHOT|0xA0` for [`Action::FIRE_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-FIRE_GRAPPLE). These are display artifacts; the stored value is the plain `Action` integer. The table is verified end to end against `PopulateMappings`, including the actions, description keys, ability and upgrade gates, and flags.

The verified `MappingInfo` layout consists only of dwords: `m_Interaction`, `m_Action1`, `m_Action2`, `m_DescriptionKey`, `m_RequiredUpgrade` (a bare hash), `m_RequiredAbility` (a bare hash), and one packed dword for the `Customizable`, `ShowInUIMapping`, `PCOnly`, and `ControllerOnly` flags. The layout is bound on `MappingInfo`.

Interaction legend ([`ButtonInteraction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::button_mapping::ButtonInteraction)): **tap** = `BINT_SINGLE_BUTTON_TAP`, **hold** = `BINT_SINGLE_BUTTON_HOLD`, **release** = `BINT_SINGLE_BUTTON_RELEASE`, **chord** = `BINT_TWO_BUTTON_HOLD_AND_TAP` (`m_Action1` held + `m_Action2` tapped). Every `EMapping` value has a populated entry **except** the `END_*` section markers, which stay at the constructor default, with `m_Action1` set to the invalid-action sentinel `Action::COUNT`. The table is therefore complete.

### On-foot (generic and `FIRST_ON_FOOT_MAPPING`..`LAST_ON_FOOT_MAPPING`)

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_PAUSE` | tap | `GUI_PAUSE` | generic |
| `MAPPING_COMMLINK` | tap | `GUI_PDA` | generic |
| `MAPPING_FIRE_GRAPPLING_HOOK` | tap | `FIRE_GRAPPLE` | ability `grappling_hook` |
| `MAPPING_RETRACT_TETHERS` | tap | `RETRACT_GRAPPLE` | ability `retract_tether` |
| `MAPPING_RELEASE_TETHERS_NON_CHORDED` | tap | `RELEASE_GRAPPLE` | PC only |
| `MAPPING_FIRE_RIGHT` | tap | `FIRE_RIGHT` | the sole on-foot fire input |
| `MAPPING_THROW_GRENADE` | tap | `THROW_GRENADE` | ability `grenades` |
| `MAPPING_DETONATE_EXPLOSIVE_GAMEPAD` | hold | `DETONATE_EXPLOSIVE` | ability `planted_explosives`; controller only |
| `MAPPING_DETONATE_EXPLOSIVE_PC` | tap | `DETONATE_EXPLOSIVE_TAP` | ability `planted_explosives`; PC only |
| `MAPPING_RELOAD` | tap | `RELOAD` | |
| `MAPPING_JUMP` | tap | `JUMP` | |
| `MAPPING_OPEN_WINGSUIT` | tap | `OPEN_WINGSUIT` | ability `wingsuit` |
| `MAPPING_OPEN_PARACHUTE` | tap | `OPEN_PARACHUTE` | ability `parachute` |
| `MAPPING_HAMMER` | tap | `PUSH_GRAPPLE` | ability `hammer`; **melee reuses the grapple-push action**, `buttonhint_hammer` |
| `MAPPING_PRECISION_AIM` | tap | `PRECISION_AIM` | upgrade `precision_aim` |
| `MAPPING_CANCEL` | tap | `CANCEL` | |
| `MAPPING_USE_ITEM` | tap | `USE_ITEM` | |
| `MAPPING_SELECT_DUAL_WIELD` | tap | `SELECT_DUEL_WIELD` | |
| `MAPPING_SELECT_TWO_HANDED` | tap | `SELECT_TWO_HANDED` | |
| `MAPPING_SELECT_TWO_HANDED_SPECIAL` | tap | `SELECT_TWO_HANDED_SPECIAL` | |
| `MAPPING_SELECT_EXPLOSIVE` | tap | `SELECT_EXPLOSIVES` | ability `planted_explosives` |
| `MAPPING_PREV_WEAPON` | tap | `PREV_WEAPON` | PC only |
| `MAPPING_NEXT_WEAPON` | tap | `NEXT_WEAPON` | PC only |
| `MAPPING_MOVE_FORWARD` | hold | `MOVE_FORWARD` | |
| `MAPPING_MOVE_BACKWARD` | hold | `MOVE_BACKWARD` | |
| `MAPPING_MOVE_LEFT` | hold | `MOVE_LEFT` | |
| `MAPPING_MOVE_RIGHT` | hold | `MOVE_RIGHT` | |
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
| `MAPPING_SNIPER_ZOOM` | tap | `PRECISION_AIM` | **no upgrade gate** (unlike `MAPPING_PRECISION_AIM`); shares the precision-aim action |
| `MAPPING_SKIP_CUTSCENE` | hold | `SKIP_CUTSCENE` | |
| `MAPPING_LOOK_AT` | hold | `LOOK_AT` | |
| `MAPPING_AIR_BRAKE` | hold | `CANCEL` | upgrade `wingsuit_air_brake` |
| `MAPPING_SLINGSUIT_TO_REEL_IN` | tap | `OPEN_WINGSUIT` | upgrade `slingsuit_to_reel` |
| `MAPPING_HOLSTER_WEAPON` | hold | `RELOAD` | **holster = hold-reload** |
| `MAPPING_REEL_BOOST` | hold | `RETRACT_GRAPPLE` | reel-in context (hold-retract) |
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
| `MAPPING_ACCELERATE_LAND_VEHICLE` | tap | `ACCELERATE` | |
| `MAPPING_REVERSE_LAND_VEHICLE` | tap | `REVERSE` | |
| `MAPPING_NITROUS_LAND_VEHICLE` | hold | `USE_VEHICLE_MOD` | upgrade `land_vehicle_nitrous_unlock` |
| `MAPPING_TURBO_JUMP_LAND_VEHICLE` | tap | `USE_VEHICLE_MOD` | upgrade `land_vehicle_turbo_jump_unlock` |
| `MAPPING_SOUND_HORN_SIREN_LAND_VEHICLE` | tap | `SOUND_HORN_SIREN` | |
| `MAPPING_TURN_LEFT` | tap | `TURN_LEFT` | car |
| `MAPPING_TURN_RIGHT` | tap | `TURN_RIGHT` | car |
| `MAPPING_ENTER_GUNNER_SEAT` | hold | `ENTER_VEHICLE` | hidden in UI mapping |
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_CAR` | tap | `FIRE_VEHICLE_WEAPON_PRIMARY` | |
| `MAPPING_FIRE_VEHICLE_WEAPON_SECONDARY_CAR` | tap | `FIRE_VEHICLE_WEAPON_SECONDARY` | |
| `MAPPING_STUNT_JUMP_CAR` | tap | `STUNT_JUMP` | |
| `MAPPING_FIRE_WEAPON_MOTORCYCLE` | tap | `MC_FIRE` | personal weapon |
| `MAPPING_RELOAD_WEAPON_MOTORCYCLE` | tap | `MC_RELOAD` | |
| `MAPPING_HOLSTER_WEAPON_MOTORCYCLE` | hold | `MC_RELOAD` | holster = hold-reload |
| `MAPPING_TILT_FORWARD_MOTORCYCLE` | tap | `BIKE_TILT_FORWARD` | |
| `MAPPING_TILT_BACKWARD_MOTORCYCLE` | tap | `BIKE_TILT_BACKWARD` | |
| `MAPPING_LEAN_LEFT_MOTORCYCLE` | tap | `BIKE_LEAN_LEFT` | |
| `MAPPING_LEAN_RIGHT_MOTORCYCLE` | tap | `BIKE_LEAN_RIGHT` | |

### Sea vehicle: generic, boat, and jetski

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_NITROUS_SEA_VEHICLE` | hold | `USE_VEHICLE_MOD` | upgrade `sea_vehicle_nitrous_unlock` |
| `MAPPING_TURBO_JUMP_SEA_VEHICLE` | tap | `USE_VEHICLE_MOD` | upgrade `sea_vehicle_turbo_jump_unlock` |
| `MAPPING_TURN_LEFT_SEA_VEHICLE` | tap | `ACCELERATE` | **actually forward** — `buttonhint_forward_sea_vehicle` |
| `MAPPING_TURN_RIGHT_SEA_VEHICLE` | tap | `REVERSE` | **actually backward** — `buttonhint_backward_sea_vehicle` |
| `MAPPING_SOUND_HORN_SIREN_SEA_VEHICLE` | tap | `SOUND_HORN_SIREN` | |
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_BOAT` | tap | `FIRE_VEHICLE_WEAPON_PRIMARY` | |
| `MAPPING_STUNT_JUMP_BOAT` | tap | `STUNT_JUMP` | |
| `MAPPING_FIRE_WEAPON_JETSKI` | tap | `MC_FIRE` | personal weapon |
| `MAPPING_RELOAD_WEAPON_JETSKI` | tap | `MC_RELOAD` | |
| `MAPPING_HOLSTER_WEAPON_JETSKI` | hold | `MC_RELOAD` | holster = hold-reload |

### Air vehicle: generic, helicopter, and plane

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_FIRE_VEHICLE_WEAPON_PRIMARY_AIR_VEHICLE` | tap | `FIRE_VEHICLE_WEAPON_PRIMARY` | |
| `MAPPING_FIRE_VEHICLE_WEAPON_SECONDARY_AIR_VEHICLE` | tap | `FIRE_VEHICLE_WEAPON_SECONDARY` | |
| `MAPPING_HELI_INC_ALTITUDE` | tap | `HELI_INC_ALTITUDE` | |
| `MAPPING_HELI_DEC_ALTITUDE` | tap | `HELI_DEC_ALTITUDE` | |
| `MAPPING_HELI_ROLL_RIGHT` | tap | `HELI_ROLL_RIGHT` | |
| `MAPPING_HELI_ROLL_LEFT` | tap | `HELI_ROLL_LEFT` | |
| `MAPPING_HELI_TURN_LEFT` | tap | `HELI_TURN_LEFT` | |
| `MAPPING_HELI_TURN_RIGHT` | tap | `HELI_TURN_RIGHT` | |
| `MAPPING_HELI_FORWARD` | tap | `HELI_FORWARD` | |
| `MAPPING_HELI_BACKWARD` | tap | `HELI_BACKWARD` | |
| `MAPPING_STUNT_JUMP_HELICOPTER` | tap | `STUNT_JUMP` | |
| `MAPPING_NITROUS_HELICOPTER` | hold | `USE_VEHICLE_MOD` | upgrade `air_vehicle_nitrous_unlock` |
| `MAPPING_PLANE_TURN_RIGHT` | tap | `PLANE_TURN_RIGHT` | |
| `MAPPING_PLANE_TURN_LEFT` | tap | `PLANE_TURN_LEFT` | |
| `MAPPING_PLANE_ROLL_RIGHT` | tap | `PLANE_ROLL_RIGHT` | |
| `MAPPING_PLANE_ROLL_LEFT` | tap | `PLANE_ROLL_LEFT` | |
| `MAPPING_PLANE_PITCH_UP` | tap | `PLANE_PITCH_UP` | |
| `MAPPING_PLANE_PITCH_DOWN` | tap | `PLANE_PITCH_DOWN` | |
| `MAPPING_PLANE_INC_THRUST` | tap | `PLANE_INC_TRUST` | |
| `MAPPING_PLANE_DEC_THRUST` | tap | `PLANE_DEC_TRUST` | |
| `MAPPING_STUNT_JUMP_PLANE` | tap | `STUNT_JUMP` | |
| `MAPPING_NITROUS_PLANE` | hold | `PLANE_INC_TRUST` | upgrade `air_vehicle_nitrous_unlock` |

### The plane's thrust and nitrous

The plane's input reader, [`AirPlane::UpdateVehicleInputsFromActionMap`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane##func-UpdateVehicleInputsFromActionMap), runs on top of the vehicle's own each tick unless the plane is destroyed, its driver is knocked out or dead, an AI's input has gone stale, or it is under an EMP. It reads the turn from `PLANE_TURN_LEFT` and `PLANE_TURN_RIGHT`, the roll from `PLANE_ROLL_LEFT` and `PLANE_ROLL_RIGHT`, the pitch from `PLANE_PITCH_UP` and `PLANE_PITCH_DOWN`, and the throttle as the `PLANE_INC_TRUST` value less the `PLANE_DEC_TRUST` value into [`Vehicle::m_SteeringAcceleration`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringAcceleration), with the `PLANE_DEC_TRUST` value alone as the brake ([`Vehicle::m_SteeringBrake`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_SteeringBrake)). Both are values, not rates: the thrust is where the trigger is.

The nitrous is not an input of its own. With a living driver and the plane in control, a `PLANE_INC_TRUST` value above [`AirPlane::NITROUS_THRUST_THRESHOLD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::AirPlane) raises [`Vehicle::m_NitrousInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_NitrousInput) to it, and otherwise the nitrous input is left as it is. `MAPPING_NITROUS_PLANE`'s hold describes the button hint; the reader tests the value, not a hold time.

### UI

All UI mappings are tap and `m_Customizable = false`.

| Mapping | Action(s) |
|---|---|
| `MAPPING_UI_OK` | `GUI_OK` |
| `MAPPING_UI_CANCEL` | `GUI_CANCEL` |
| `MAPPING_UI_TAB_NEXT` | `GUI_TAB_NEXT` |
| `MAPPING_UI_TAB_PREV` | `GUI_TAB_PREV` |
| `MAPPING_UI_FILTER` | `GUI_FILTER` |
| `MAPPING_UI_RECENTER` | `GUI_RECENTER` |
| `MAPPING_UI_PAGE_NEXT` | `GUI_PAGE_NEXT` |
| `MAPPING_UI_PAGE_PREV` | `GUI_PAGE_PREV` |

### Weaponized wingsuit

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_WEAPONIZED_WINGSUIT_TAKEOFF` | hold | `WINGSUIT_TAKEOFF` | |
| `MAPPING_WEAPONIZED_WINGSUIT_BOOST` | tap | `WINGSUIT_BOOST` | |
| `MAPPING_WEAPONIZED_WINGSUIT_BRAKE` | tap | `WINGSUIT_AIRBRAKE` | |
| `MAPPING_WEAPONIZED_WINGSUIT_EVADE` | chord | `WINGSUIT_EVADE` held + `MOVE_ALL` tapped | |
| `MAPPING_WEAPONIZED_WINGSUIT_PRIMARY_WEAPON` | tap | `FIRE_WINGSUIT_WEAPON_MAIN` | |
| `MAPPING_WEAPONIZED_WINGSUIT_SECONDARY_WEAPON` | tap | `FIRE_WINGSUIT_WEAPON_SECONDARY` | |

### Mech and unsectioned

Mech move mappings reuse the plane and turn actions. `MAPPING_ACTIVATE_BAVARIUM_SHIELD` and `MAPPING_CHALLENGE_RESET_VEHICLE` sit past `END_MECH_MAPPINGS` with no `FIRST/LAST` section of their own.

| Mapping | Interaction | Action(s) | Condition / notes |
|---|---|---|---|
| `MAPPING_MECH_MOVE_FORWARD` | hold | `PLANE_PITCH_DOWN` | reuses the plane action |
| `MAPPING_MECH_MOVE_BACKWARD` | hold | `PLANE_PITCH_UP` | reuses the plane action |
| `MAPPING_MECH_MOVE_LEFT` | hold | `TURN_LEFT` | |
| `MAPPING_MECH_MOVE_RIGHT` | hold | `TURN_RIGHT` | |
| `MAPPING_MECH_JUMP` | tap | `MECH_JUMP` | |
| `MAPPING_MECH_PUNCH` | tap | `MECH_PUNCH` | |
| `MAPPING_MECH_GRAVITY_GUN_ATTRACT` | hold | `MECH_FIRE_GRAVITY_WEAPON_PRIMARY` | |
| `MAPPING_MECH_GRAVITY_GUN_DROP` | tap | `MECH_FIRE_GRAVITY_WEAPON_SECONDARY` | |
| `MAPPING_MECH_GRAVITY_GUN_THROW` | release | `MECH_FIRE_GRAVITY_WEAPON_PRIMARY` | fires on button release |
| `MAPPING_MECH_FIRE_RIGHT_HAND_WEAPON` | tap | `MECH_FIRE_RIGHT_HAND_WEAPON` | |
| `MAPPING_ACTIVATE_BAVARIUM_SHIELD` | tap | `SOUND_HORN_SIREN` | reuses the horn action |
| `MAPPING_CHALLENGE_RESET_VEHICLE` | hold | `CHALLENGE_RESET_VEHICLE` | |

### Gating (`IsMappingActive`) and consumption

`CButtonMapping::IsMappingActive(m, player)` is not yet located. The IDB function carrying that name is mislabelled: it takes a `CStateMachineInstance`, checks one upgrade, and is an animation state-machine condition. The gating logic below is unconfirmed. `IsMappingActive` gates a mapping on two conditions, ANDed:

1. If `m_RequiredAbility.m_Hash != 0` and a player is present, the mapping is inactive unless `CAbilitiesHandler::IsEnabled(player, ability)` is true (e.g. grenades, grappling hook, wingsuit, parachute, planted explosives, hammer, retract-tether).
2. If `m_RequiredUpgrade.m_Hash != 0`, the mapping is inactive unless [`ProfileManager::GetUpgradeInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::ProfileManager##func-GetUpgradeInfo)`(upgrade)` reports both `m_IsPurchased` and `m_IsEnabled` (e.g. precision aim, the per-class nitrous/turbo-jump unlocks, wingsuit air brake, slingsuit-to-reel).

Otherwise the mapping is active. Ability/upgrade hashes are stored per entry; the mappings without either gate are always active.

The button-hint / observer layer consumes the table through two range queries, both taking a `[from, to_inclusive]` `EMapping` sub-range (the `FIRST_*`/`LAST_*` section bounds selected by the player's current mode) and an `only_active` flag that applies `IsMappingActive`:

- `GetMappingInfosForKey(player, key, from, to, only_active, force_keyboard, force_gamepad)` — walks the range and collects every `SMappingInfo` whose `m_Action1` **or** `m_Action2` resolves through `CUIBase::GetKeyID` to the given physical key (skipping the `Action::COUNT` sentinel). This answers "what does this button do right now".
- `GetMappingInfosForAction(player, action, from, to, only_active)` — the inverse: collects every mapping in the range whose `m_Action1` or `m_Action2` equals the given `CSteering::EAction`. This is how a button-hint prompt for an action finds its glyph.

`RefreshAllActionsForUi` builds `m_AllActionsForUi` (a `forward_list<CSteering::EAction>`) for the bindings/UI-mapping screen.

Several actions do double duty, separated only by context or mode: on-foot holster is hold-`RELOAD`, and melee (`MAPPING_HAMMER`) is [`Action::PUSH_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PUSH_GRAPPLE); `MAPPING_SNIPER_ZOOM` is [`Action::PRECISION_AIM`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PRECISION_AIM) with no upgrade gate, while `MAPPING_PRECISION_AIM` is the same action gated on the `precision_aim` upgrade; nitrous/turbo-jump across every vehicle class are the same [`Action::USE_VEHICLE_MOD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-USE_VEHICLE_MOD) separated only by upgrade hash and tap-vs-hold; and the sea-vehicle "turn left/right" mappings are really forward/reverse (`ACCELERATE`/`REVERSE`).

## Analog axes read by vehicles and the wingsuit

The vehicle and wingsuit controllers read action values through `CActionMap::GetValue`, which returns the effector's float, so analog values written to the effectors pass through unchanged. The per-class readers are `UpdateVehicleInputsFromActionMap` overrides; the details below are unconfirmed (medium-high confidence). None of the vehicle readers use the camera matrices.

- **Base vehicle** (`CVehicle::UpdateVehicleInputsFromActionMap`): [`HANDBRAKE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HANDBRAKE) as a pressed state, the primary and secondary weapon actions ([`FIRE_VEHICLE_WEAPON_PRIMARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-FIRE_VEHICLE_WEAPON_PRIMARY) and [`FIRE_VEHICLE_WEAPON_SECONDARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-FIRE_VEHICLE_WEAPON_SECONDARY)) through `UpdateVehicleActionInput`, [`SOUND_HORN_SIREN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-SOUND_HORN_SIREN) for the Bavarium shield, and [`VEHICLE_RELEASE_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-VEHICLE_RELEASE_GRAPPLE) on release. Vehicles with strafing wheel steering (the mech) use [`MECH_FIRE_RIGHT_HAND_WEAPON`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MECH_FIRE_RIGHT_HAND_WEAPON), [`MECH_FIRE_GRAVITY_WEAPON_PRIMARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MECH_FIRE_GRAVITY_WEAPON_PRIMARY), and [`MECH_FIRE_GRAVITY_WEAPON_SECONDARY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MECH_FIRE_GRAVITY_WEAPON_SECONDARY) instead.
- **Land vehicles** (`CLandVehicle`): acceleration = [`ACCELERATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-ACCELERATE), reverse = [`REVERSE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-REVERSE), turn = [`TURN_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-TURN_RIGHT) − [`TURN_LEFT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-TURN_LEFT), yaw = [`PLANE_TURN_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PLANE_TURN_RIGHT) − [`PLANE_TURN_LEFT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PLANE_TURN_LEFT), roll = [`PLANE_ROLL_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PLANE_ROLL_RIGHT) − [`PLANE_ROLL_LEFT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PLANE_ROLL_LEFT), and pitch = [`PLANE_PITCH_UP`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PLANE_PITCH_UP) − [`PLANE_PITCH_DOWN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PLANE_PITCH_DOWN), all as floats. Movement is zeroed while `m_LockMovement` is set.
- **Helicopters** (`CHelicopter`): roll = [`HELI_ROLL_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HELI_ROLL_RIGHT) − [`HELI_ROLL_LEFT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HELI_ROLL_LEFT); yaw = [`HELI_TURN_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HELI_TURN_RIGHT) − [`HELI_TURN_LEFT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HELI_TURN_LEFT), plus, for the driver, the mouse device's values for the same two actions scaled by the yaw mouse sensitivity, with the sum clamped to ±1; fore/aft = −([`HELI_FORWARD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HELI_FORWARD) − [`HELI_BACKWARD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-HELI_BACKWARD)).
- **Planes, boats, and motorbikes** were not read individually; their mappings are in the mapping table above.

The wingsuit is not a vehicle. `NStateTask_MovementWingSuitTask::Update` reads the move axes as floats: x = [`MOVE_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_RIGHT) − [`MOVE_LEFT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_LEFT), and y from [`MOVE_FORWARD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_FORWARD) and [`MOVE_BACKWARD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_BACKWARD) (the decompile of the y term is garbled; medium confidence). It passes the pair to `NAirMovement::UpdateAirPhysics`, which maps x and y to yaw, pitch, and roll through the tuning's `m_XInputYawPitchRollAmount` and `m_YInputYawPitchRollAmount`, and for the local player adds `NAirMovement::UpdateStdLookSteering`, the camera-driven look-steer. While stalling, [`CANCEL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-CANCEL) closes the wingsuit unless the grapple has an active wire or [`RETRACT_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-RETRACT_GRAPPLE) is held. The same `Update` contains an anti-tamper branch (an `NAntiTamper` check that corrupts the movement settings when tampering is detected), the same kind of protection as `CalculateInputDeltaAngles` above. Its address is not yet located. The parachute's inputs are in `parachute-locomotion.md`.

## Weapon selection, rebel drops, and gear mods

The game has a weapon selection cross but no radial wheel and no slow-motion selector. The slot-select actions (`SELECT_DUEL_WIELD`, `SELECT_TWO_HANDED`, `SELECT_TWO_HANDED_SPECIAL`, `SELECT_EXPLOSIVES`, and the PC-only `PREV_WEAPON`/`NEXT_WEAPON`) are read into the character's weapon inputs and acted on by [`Inventory::UpdateWeaponHandling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-UpdateWeaponHandling), not by the animation state machine; while a slot-select action is held, the HUD shows a four-armed cross (dual-wield, two-handed, two-handed special, planted explosive) that highlights the held slot and commits on release. The reader, the decision, and the cross are in [weapon-selection.md](weapon-selection.md). `CGamePauseUtility` is a script facility that pauses the game until an action is pressed, used by tutorials; it is not a weapon selector.

Rebel drops are ordered from the PDA/comm UI (`CRebelSupport`), not through a gameplay action (medium confidence).

Gear mods have no gameplay input. They are switched in the comm/skill UI: `CCommSkillUI::HandleCommand` receives an upgrade id and a boolean from the Scaleform movie and sets `SUpgradeInfo::m_IsEnabled`. The [`UpgradeInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::UpgradeInfo) that [`ProfileManager::GetUpgradeInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::ProfileManager##func-GetUpgradeInfo)`(hash)` returns carries [`m_IsPurchased`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::UpgradeInfo##field-m_IsPurchased) and [`m_IsEnabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::UpgradeInfo##field-m_IsEnabled); gameplay code tests both (for example `CGrenade::PropelToward` for the homing upgrade).

## No quick-time events

No QTE or button-mash logic was found. `SInputMash` (a spring-driven gauge with a 10 s timeout) is embedded in `CControlParameters` and constructed, but no code that updates it is located, and the binary has no QTE or mash strings. Interactions, hijacks, and use-item prompts are driven by the animation state machine through plain button conditions (`CButtonPressedCondition`, one action id), and the use-item progress bar (`NSetHUDUseItemProgressTask`) only reflects how far the use animation has played. Melee ([`NStateTask_InputMeleeTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_InputMeleeTask##func-Update)) requires the hammer ability, targets the melee slot of [`PlayerAimControl::m_Target`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_Target) (index [`TargetType::TARGET_MELEE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::target::TargetType##variant-TARGET_MELEE)), and queues acts. Medium confidence.
