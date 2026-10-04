# Motion controllers and roomscale: plan of record

The plan for making Just Cause 3 playable with motion controllers throughout: controller input in every mode, hands owned by the player, per-hand aiming (the right hand aims the guns, the left hand aims the grapple, and dual-wielded pistols aim independently), physical weapon handling, and roomscale locomotion. The plan degrades to gaze aiming with a gamepad when controllers are not in use, and to a seated posture when the player is not standing. Issue #28 (independent motion-controller aiming) is one part of it.

The engine recon behind the plan is in [aim-state.md](../../engine/gameplay/aim-state.md) (the aim state, its consumers, and fire gating), [aim-pipeline.md](../../engine/gameplay/aim-pipeline.md), [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md), [input.md](../../engine/gameplay/input.md), [hands-locomotion-and-state.md](../../engine/character/hands-locomotion-and-state.md), and [humanik.md](../../engine/character/humanik.md). The head and body model it extends is [head-and-body.md](../body/head-and-body.md).

Milestones 1 to 5 are implemented and have not yet been tested in the headset. "Implementation status" below lists what each milestone built, the setting behind each part, and the headset test checklist; the unverified assumptions of milestone 1 are listed there, and those of the later milestones in [controllers.md, "Unverified"](controllers.md#unverified). [controllers.md](controllers.md) describes the implementation as built.

## Principles

The virtual body behaves continuously and consistently with the player's body. The game switches the player's body between discrete behaviours: the aim state flips locomotion to strafing, raises the weapon, and runs the arm aim IK, and the combat mental state draws and holsters the weapon on its own. The mod dissolves these switches rather than mapping controller input onto them. A state change that the player did not perform with their own body is a defect in this model.

The mod never changes the player's view orientation unless the change is attached to a movement action the player took, such as a stick turn. Run-and-steer locomotion, where the body turns to face its direction of travel, is excluded for this reason, and is not offered as an option.

The hands belong to the player. An animation takes the hands only where a scripted state cannot yet work otherwise, and the hand tiers below record each such state and the work that would return the hands to the player.

The plan degrades in two directions. Without controllers, the mod keeps its current behaviour: gaze aiming and gamepad input. Without room to walk, the player's posture is standing or seated, and roomscale translation is disabled.

The mod's in-VR UI is an adapted egui panel, which is not a VR-native interface. Features whose main cost is new mod-specific UI are deferred until that interface is redesigned. The deferred list is at the end of the plan. The world-space overlay layer (described below) is scene rendering, not UI, and is in scope.

## Decisions

The following decisions are settled and the rest of the plan assumes them.

- Handedness is configurable. A `dominant_hand` setting mirrors the gun and grapple hands, the bindings, and the weapon attach bones. The grapple gauntlet stays on Rico's left arm model in the mirrored layout, so the grapple fires from the gauntlet on the non-dominant arm cosmetically.
- The aim state is pinned clear for the local player (see "The aim-state pin").
- The combat mental state keeps running for the AI, and its effects on the player's body are removed: no automatic draw and no automatic holster.
- Guns are sticky. A drawn gun stays in the hand until the player holsters it. Grip inputs differ between controllers (an analog grip on Touch, a force-sensing grip on Index, a binary grip button on older wands), and holding a grip for minutes is tiring on all of them, so the grip is used only as an edge: press to grab, press to grapple.
- The left grip fires the grapple, except inside a grab volume (a foregrip, a holster, a wheel), where it grabs. The grapple keeps the game's native tap and hold semantics, and a yank gesture on the left hand is an alternate zip trigger.
- Two-handed weapons are held by the dominant hand alone. The off-hand can grab the foregrip, and the line between the two hands then sets the weapon's direction.
- Dual-wielded pistols split onto both hands, each fired by its own trigger and aimed by its own hand.
- Guns show no reticle by default. A dot at the hit point and a laser line are options. The grapple shows a hook-point highlight and a gauntlet beam, each configurable.
- Aim assist is re-derived per hand ray, softened, and defaults to off.
- Driver-seat vehicle weapons aim along the gaze by default, with an option for the right-hand ray. Passenger seats, gunner seats, and mounted guns aim by hand.
- Recoil is an offset between the controller pose and the rendered weapon, driven by the game's per-weapon recoil state, with a spring return, a stiffer spring under a two-hand grip, and matching haptics.
- Haptics, physical melee, and the head-through-geometry fade are in scope.
- Vehicles use stick bindings by default. Physical controls (a wheel, handlebars, a helicopter cyclic and collective, a flight stick) are opt-in.
- The wingsuit and parachute use stick steering by default. Arm-pose steering for the wingsuit and grabbed handles for the parachute add to it.
- Grenades are thrown physically.
- Weapons are drawn from body holsters. A radial menu is the seated and accessibility fallback, deferred with the rest of the mod UI.
- Reloads start from a button. A magazine gesture is an opt-in alternative, and the button remains.
- Scoped weapons render a magnified image into a lens on the scope.
- Locomotion is head-relative by default, with an off-hand-relative option. Seated play has a crouch toggle.
- The player maps onto Rico's proportions by remapping, in the Bonelab style (see "Body scale").
- The controller scheme and the gamepad scheme switch automatically by the most recent input, and a setting can pin either one.

## What the engine provides

The game's architecture separates most of what the plan needs to separate. Each item links to the engine doc that owns the detail.

- Firing does not depend on the aim state; the remaining fire gates are listed in [aim-state.md, "Fire gating"](../../engine/gameplay/aim-state.md#fire-gating).
- Aim is kept per consumer. `CPlayerAimControl` stores its raycast in slots by target type, and every grapple consumer reads the grapple slot ([aim-pipeline.md, "The aim target"](../../engine/gameplay/aim-pipeline.md#the-aim-target-playeraimcontrol); [grapple-pipeline.md, "Readers of the grapple slot"](../../engine/gameplay/grapple-pipeline.md#4-readers-of-the-grapple-slot)).
- Fire direction is one stored point per weapon, and the shot origin is that weapon's muzzle ([aim-pipeline.md, "Fire direction, end to end"](../../engine/gameplay/aim-pipeline.md#fire-direction-end-to-end-getshotmatrix)).
- Wielded weapons ride hand attach bones through the `SetJoint` machinery, one bone per dual-wielded gun ([hands-locomotion-and-state.md §1](../../engine/character/hands-locomotion-and-state.md#1-weapon-to-hand-attachment-the-props-system)).
- The weapon's render transform and the hand mesh are both interpolated in the render phase, by separate paths ([hands-locomotion-and-state.md §1, "Where the weapon's world transform is computed per frame"](../../engine/character/hands-locomotion-and-state.md#where-the-weapons-world-transform-is-computed-per-frame)).
- Input can be injected by action ID, and every vehicle class and the wingsuit read analog values ([input.md, "The write API"](../../engine/gameplay/input.md#the-write-api); [input.md, "Analog axes read by vehicles and the wingsuit"](../../engine/gameplay/input.md#analog-axes-read-by-vehicles-and-the-wingsuit)).
- The grenade's solved launch velocity is stored where a post-hook can replace it ([aim-pipeline.md, "Grenades"](../../engine/gameplay/aim-pipeline.md#grenades)).
- The on-foot locomotion task writes the character proxy's velocity every tick, idle included, and the proxy resolves collision ([hands-locomotion-and-state.md §4.1](../../engine/character/hands-locomotion-and-state.md#41-the-locomotion-task-runs-at-idle-and-writes-the-proxy-input-directly)).

The limits are as follows. Native aim assist runs only on the gaze ray. The dual-wield slot is one weapon object with two alternating barrels that share one target. The aim state has many consumers ([aim-state.md, "Consumers"](../../engine/gameplay/aim-state.md#consumers)), and the pin has to cut the ones that affect the body. The sim runs at about 30 Hz, so anything posed in the sim phase lags the controllers unless it is updated again at render time.

## The body model

### The aim-state pin

The aim state is the set of flags [`AimState::m_AimingWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AimState##flag-m_AimingWeapon) and [`AimState::m_AimingGrapple`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AimState##flag-m_AimingGrapple) in [`Character::m_AimFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_AimFlags), and the state bit [`StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_AIMING_WPN_OR_GRPL) that mirrors them. It selects between the no-aim and aim families of full-body animation states, switches the locomotion tasks between run-and-steer and aim-relative strafing, runs the aim IK tasks, and changes jump and hang facing.

The mod pins the aim state clear for the local player with the graph state bit [`StateBitFlag::SB_NO_WEAPON_AIM`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_NO_WEAPON_AIM), which forces weapon aim off natively. Only the two aim-input functions read the bit, and its value persists, so the mod sets it for the duration of the player's aim-input function (`HandleAimingInputPlayer`) and restores it afterwards. The bit does not reach the grapple flag, so the mod also clears `m_AimingGrapple` after `PreSimUpdateGrapple`.

The existing FPS-movement shim keeps forcing the flags for the duration of each locomotion task's `Update`, so the locomotion tasks take the strafe branch and queue the aim-family strafe acts (`ACT_MOVE_AIM_FOCUS_*`). The lower body therefore plays aim-focus locomotion, while the states keyed on the aim bit and on `m_AimingWeapon` outside the task (the additive aim layers and the upper-body aim variants) stay off ([aim-state.md, "Locomotion task selection"](../../engine/gameplay/aim-state.md#locomotion-task-selection)). This combination is what ships today whenever the player is not aiming, so it has already been exercised in play. The pin extends it to the moments when the player fires.

With a weapon wielded, the strafe branch chooses its act relative to the wielded weapon's own aim point. Once the per-weapon aim point carries the hand ray, the strafe direction follows where the gun points rather than where the head faces, so pointing the gun sideways while walking forward selects a sideways strafe. The shim therefore needs to hold the wielded weapons' aim points at a head-forward point for the duration of each task's `Update` and restore them afterwards. The shim holds the selected wielded weapon's aim point ahead of the head for the task call and restores its original point afterwards.

The pin was chosen over a pin to the set state for the following reasons.

- `CPlayer::UpdateMentalState` sets the combat mental state whenever `m_AimingWeapon` is set. A permanently set aim state would keep the player permanently in combat, which changes how AI perception and civilians react, and would prevent the relaxed walk family from playing.
- The aim family brings the arm aim IK, the head look-at switch-off, a spine blend toward the character-level aim point, and aim-specific jump facing. Each would need to be cut separately.
- With the pin clear, firing still counts as weapon input and still moves the mental state into combat, so the combat state keeps its meaning: firing starts combat, walking does not.

The costs are small. The laser-sight weapon component draws its beam only while `m_AimingWeapon` is set; the mod draws its own laser. The motorcycle has aim sub-states that the pin disables, and the vehicle weapon work replaces them. The upper body holds the relaxed carry grip. With the arms driven by IK from the shoulders, only the fingers and the grip pose come from that animation, and a carry grip still holds the weapon. This needs checking in the headset.

The weapons' `m_aim_timer_override` is left as the game sets it, because NPC weapons share it. The aim timer still runs after a shot, but with the bit set it no longer sets the aim flag, so no aim tail follows a shot.

The pin also needs:

- the wall-penetration block cleared for the held weapon, because the rendered muzzle is at the controller rather than where the animation would put it;
- the player's `NAimIKTask`, `NRightArmAimIK`, `NGrapplingFireArmIK`, and `NReelInAimIK` skipped, because the controllers own the arms;
- the body's aim point on the control-parameters blackboard ([`ControlParameters::m_AimTargetWeapons`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::ControlParameters##field-m_AimTargetWeapons)) held at a neutral point ahead of the body, so that no residual aim blend turns the body or twists the spine. The character-level aim point ([`Character::m_AimTargetPositionWeapons`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_AimTargetPositionWeapons)) is not held neutral: it follows the dominant hand's aim point while hand aiming is on (see "Guns"), and otherwise stays at the gaze point the game computes. The per-weapon targets carry the hand rays.

### The mental state

`CPlayer::UpdateMentalState` keeps running, because AI perception reads it. Its effects on the player's body are removed. [`StateBitFlag::SB_DISABLE_MENTAL_STATE_DRAW_WEAPON`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_DISABLE_MENTAL_STATE_DRAW_WEAPON) stops the automatic draw when combat starts. The inventory clears the bit every tick, so the mod sets it before each of the player's `Inventory::UpdateWeaponHandling` calls. The game has two automatic holsters ([aim-state.md, "The mental state"](../../engine/gameplay/aim-state.md#the-mental-state)), and the mod removes both: it swallows `ACT_ON_UNEQUIP_WEAPON_SLOW`, which the relaxed transition queues, and it swallows `UB_ACT_HOLSTER` when the relaxed branch of the player's weapon handling queues it on its own. The player's deliberate holster (`Character::HolsterWeapon`) takes effect through that same branch, so the mod marks a deliberate holster and lets its act through. The walk family stays the one the pin selects. Drawing and holstering happen at the holsters (see "Holsters"), and the reload button's short hold still holsters, as on the gamepad. Weapon input with nothing wielded still draws a weapon, so the fire trigger on an empty hand draws a gun.

### Locomotion and facing

Locomotion is aim-relative strafing in all on-foot cases, through the FPS-movement shim. The stick direction is relative to the head by default, with an option for the off-hand controller's pointing direction. The body's yaw is the existing scheme: stick turning (smooth or snap), plus the torso following the head through a deadzone. The torso-follow drives the visible body and the movement frame only; it never rotates the view. Physical turning is always honoured.

### Hands

The arms are driven by HumanIK from the shoulders to the controller poses at all times, except in states that the hand tiers assign otherwise. The wrist effectors ([`Effector::LEFT_WRIST`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-LEFT_WRIST) and [`Effector::RIGHT_WRIST`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-RIGHT_WRIST)) take position and rotation targets from the controllers, the weapon attach bone follows the dominant hand, and the game's own arm IK is skipped (see "The aim-state pin"). The wielded weapon rides the hand's attach bone, a child of the wrist, and the engine refreshes the weapon's cached transform from that bone after the `MAIN` solve, so the gun follows the IK-driven wrist within the tick without a joint override ([humanik.md, "Per-frame lifecycle"](../../engine/character/humanik.md#per-frame-lifecycle)). The game's secondary hand IK pass, which runs after the `MAIN` solve and could move the wrists again, is switched off per controller-driven hand by zeroing its one-frame blackboard weights. The existing body IK already queues shoulder, chest, and wrist targets on the `MAIN` pass; the hand targets extend it, and its `defer_while_aiming` rule no longer applies, because the aim state no longer changes.

Hands and weapons are updated again at render time. The sim-phase pose places them at the 30 Hz sim tick, and the engine's interpolation adds one to two ticks of lag behind the controllers. Two render-phase seams replace the stale transforms with ones built from the latest controller pose: the `CWeaponBase::UpdateRender` post-hook for the weapon model, and the existing `SetMatrixPalette` hook for the hand and forearm bones of the player's skin. Both seams are updated together from the same pose. Updating only the weapon would move the gun away from the hand mesh, which is why the sim-phase placement drives the attach bone rather than the weapon's own cached matrix. The muzzle flash, the attached effects, and the shot origin stay at the sim rate, so at high hand speed they trail the rendered muzzle slightly.

### Hand tiers

Each scripted state that wants the hands is assigned one of four tiers. A state can move up a tier later without changes to the rest of the design.

1. Free. The hand follows its controller and nothing is attached to it.
2. Action runs. The game performs the action, such as a reload timer and the weapon model's own magazine and bolt animation, while the hands stay on the controllers.
3. Anchored. The hand is attached to a world point, and its movement drives something in the game.
4. Yielded. The animation owns the hands, blending out from the controllers and back. A translucent ghost of the controller-driven hands stays visible in the world-space overlay, so that the player keeps sight of their real hands.

The starting assignments are as follows.

| State | Starting tier | Promotion |
|---|---|---|
| On foot, empty-handed or holding weapons | Free | none needed |
| Reload, pump, bolt, weapon draw from a holster | Action runs | none needed; the opt-in magazine gesture starts the reload by hand |
| Grapple fire | Free: the game's fire arm IK is skipped and the hook leaves the gauntlet | none needed |
| Reel-in flight (`GHS_REELING_IN`) | Anchored: the grapple hand holds the line where the controller is | none needed |
| Grapple hang, upside-down, and stunt positions (`GHS_REELED_*`) | Anchored: the grapple hand holds the line, and its displacement pushes the hanging body, because the game's own stick swing is disabled | none needed |
| Standing on a vehicle (stunt position) | Free | none needed |
| Ledge grabs, climbs, vaults, stunt step-up and step-down | Yielded | physical climbing, which would be a rebuild of the traversal controller |
| Vehicle entry, exit, and hijack | Yielded | the seat transitions in "Roomscale design" |
| Driving and flying with stick bindings | Free: the wheel animates without the hands | none needed |
| Driving and flying with physical controls | Anchored: the hands on the wheel, handlebars, or sticks | none needed |
| Mounted guns and turrets | Anchored: the hands on the handles, which aim the gun | none needed |
| Parachute | Free, or anchored on a handle a hand has taken | the handles: a hand takes the handle above its shoulder with the grip and pulls it down to steer or brake |
| Wingsuit, stick steering | Yielded: the wing membrane is skinned between the arms and the body, and free arms may deform it | opt-in arm steering reads the controllers while the rendered arms stay yielded; anchoring the arms to the wing pose is not built |
| Swimming | Yielded | arm-stroke swimming |
| Interactions (sabotage, planting, using objects) | Yielded | per interaction |
| Ragdoll, knockdown, death, and cutscenes | Yielded | none planned |

### Body scale

Rico is about 1.8 to 1.9 m tall. The mapping follows Bonelab's remapping rather than scaling the world. The world stays at true scale, because vehicle sizes, building sizes, and the sense of speed depend on it. The eyes sit at Rico's eye height. Hand offsets from the shoulders scale by the ratio of Rico's arm span to the player's. Vertical head travel (ducking and crouching) scales by the ratio of the two heights. Horizontal roomscale travel stays 1:1. Boneworks keeps motion 1:1 against a fixed body, and players report the mismatch as permanently bent knees or a misplaced floor; the remap avoids that.

The player's dimensions come from `body.local.json`, which the calibration on the pause menu's `VR: BODY` page writes: stand straight, then T-pose (see [pause-menu.md](../pause-menu.md#body-calibration)).

### Posture

Three postures are supported.

- Seated: LOCAL space. The seated eye height at the last recenter maps to Rico's standing eye height, roomscale translation is off, and a downward flick of the dominant stick toggles between standing and crouched eye height.
- Standing: STAGE space for the true floor height. The player stands in place, roomscale translation is off, and crouching is physical.
- Roomscale: STAGE space, with the roomscale chase loop on.

OpenXR has no signal for whether the user is seated, and SteamVR's OpenXR runtime exposes only a standing basis. The mod can read two things. `xrGetReferenceSpaceBoundsRect` on the STAGE space returns a rectangle for a room-scale boundary, and returns `XR_SPACE_BOUNDS_UNAVAILABLE` or a very small rectangle for a standing-only or stationary setup, which separates standing from roomscale. The head height above the STAGE floor, compared with the calibrated standing height, indicates a seated player. The posture setting is `auto | seated | standing | roomscale`. In `auto`, the stage bounds select between standing and roomscale without asking: a play area of at least a metre each way (`roomscale.roomscale_min_extent_m`) selects roomscale. A prompt suggesting the seated posture when the head is low is deferred with the rest of the mod UI; until then, the seated posture is set explicitly.

## Input

### Action sets

The OpenXR actions carry raw inputs, named for the physical inputs of the Valve Index controller, and the bindings map them to game actions per mode. OpenXR gives every action exactly one owning set, so the pose actions cannot be shared between mode sets. There are five sets. `hands` is always active and holds the grip and aim poses of both hands and the haptic output. `onfoot`, `vehicle`, `airborne`, and `ui` are active one at a time, selected by the per-tick mode detector (see "Mode detection"), and each holds the same raw input actions. Per-user remapping is the runtime's own binding interface (SteamVR or xrizer), not a mod UI, and that interface remaps per set, so a player can move a button in vehicles without moving it on foot. The mod ships suggested bindings for the Index interaction profile (every input) and the Touch interaction profile (its inputs, mapped by position).

Output is written after `InputDeviceManager::Update`, following the timing rules in [input.md](../../engine/gameplay/input.md). Held inputs are written to the effectors directly rather than through `ForceSetPressed`, because `ForceSetPressed` resets the effector's hold time on every call, so a held input would never register as a hold ([input.md, "The write API"](../../engine/gameplay/input.md#the-write-api)). The mod keeps each held action's hold time itself and writes the effector's value, state, and state time. Menu actions go to the UI action map, which the game's menus read instead of the player's map.

The controller scheme and the gamepad scheme switch on the most recent input. Tracked controllers producing input select the controller scheme. Any gamepad or keyboard input selects the gamepad scheme, which restores gaze aiming, the gamepad bindings, and the animated hands. A setting can pin either scheme.

### Mode detection

The mode detector replaces the headpose sim's binary on-foot test (the `EvaluateCharacterOrientation` call counter), which reads a swimmer as not on foot. It reads the engine's own state signals, documented in [hands-locomotion-and-state.md §5](../../engine/character/hands-locomotion-and-state.md#5-per-tick-character-state-signals): the character state-bitflag words, the state-machine-hash predicates, the vehicle class predicates, the grapple hook state, and the pause and UI signals. It runs once per engine input tick, from the same `InputDeviceManager::Update` detour as the headpose sim: one read of the local character, one snapshot of the three state words, and a `GetVehiclePtr` call only when a vehicle state is indicated, with the pointer cached for the tick.

The first matching rule selects the action set. The order follows the engine's state overlaps ([§5.6](../../engine/character/hands-locomotion-and-state.md#56-state-overlap-and-precedence)).

1. A menu is up: `ui`. The signals are the game clock's stop flag ([`Clock::m_Stop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_Stop)), which covers the pause menu, the commlink, and the other pause reasons; the UI blocker bit ([`InputSystemBlocker::Ui`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputSystemBlocker##variant-Ui)) of the player action map, which covers game screens such as the map that do not pause; and the mod's own panel.
2. A deferred teleport is in flight ([`GameWorld::IsTeleporting`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-IsTeleporting)), or the character is not readable: hold the current set. The rig re-base on a teleport belongs to the chase loop (see "The chase loop").
3. Seated in a vehicle (`IsInVehicleAttachState`, which spans the entry and exit animations): `vehicle`, with the class resolved in the order helicopter, plane, motorcycle, car, sea, and the driver seat selecting the driver bindings.
4. Grapple traversal (reeling in, pre-reel, hanging, or reeled in): the gameplay set stays (`onfoot` or `airborne`), and the reel or hang sub-state is flagged for the hand tiers.
5. Wingsuit: `airborne`, wingsuit variant.
6. Parachute, including the vehicle-parachute attachment: `airborne`, parachute variant.
7. Freefall, fall, or fastfall: `airborne`.
8. Swimming: `onfoot`, swim variant.
9. Otherwise: `onfoot`. The headpose sim keeps its own on-foot test (the orientation-evaluation counter) until it is moved onto the detector.

A switch between gameplay modes is debounced by a configurable number of ticks (two by default), so a single-tick flicker during an animation blend does not switch the action set. Entering and leaving `ui` is immediate, because the UI signals do not flicker and a late switch would send a menu press to the gameplay bindings. The state-machine-hash predicates take precedence over raw bits during known transitions. A null `m_ControlParameters` or `m_InstanceData` during the first frames after spawn holds the previous mode. Boats and jetskis share one class until the jetski discriminator is pinned.

### Grip semantics

Guns are sticky, so the grip is an edge, never a hold. A grab volume is a region where a grip press grabs instead of performing its default action. The grab volumes are the holsters on the body, a two-handed weapon's foregrip, the grenade pouch, the magazine pouch under the opt-in magazine gesture, and, under the opt-in physical controls, the wheel, the handlebars, the flight sticks, and the turret handles. The world-space overlay highlights the volume the hand is in. The holster volumes are small and close to the body, so that a grapple fired in a hurry near the hip does not draw a weapon instead.

Holding a grenade is the exception to sticky grips: the grenade is held while the grip is held, and thrown when the grip is released.

### Per-mode bindings

The tables are laid out for the Valve Index controller, which has A and B on both hands. In the tables, R and L are the dominant hand and the off-hand, A and B are the dominant hand's face buttons, and X and Y are the off-hand's A and B. A left-handed layout mirrors every gameplay binding; the menu bindings stay on the physical hands, so that the previous and next tabs and pages keep their sides. The system button of either hand is free on the Index controller and carries pause and the commlink. The Touch profile shares the topology through its own suggested bindings, with its left menu button as the system button. The Index trackpads and finger curl stay unbound until the gesture work. The tables follow the game's own mode partition and stay congruent with [the default gamepad keymap](../../engine/gameplay/input.md#the-default-gamepad-keymap) where the VR model allows, so that a player's gamepad habits carry over; the action IDs each mapping drives are in [the mapping to action table](../../engine/gameplay/input.md#the-mapping--action-table). Where a controller input stands for a gamepad button, the injection holds every gameplay action that the default gamepad keymap puts on that button, and the game's own context selects the one that applies. The action IDs that serve several mappings (holster as a short hold of `RELOAD`, nitrous and turbo jump as a hold and a tap of `USE_VEHICLE_MOD`, the gunner seat as a held `ENTER_VEHICLE`, and the tether as a held `FIRE_GRAPPLE`) are separated by the game through tap versus hold and through the mode context, so the injection reproduces the hold time of the button it stands for. The system button is the one overload the mod resolves itself. Melee (`PUSH_GRAPPLE` under the hammer ability) is left off the virtual B button, because there it would melee on every tether release. Wherever a mapping is marked as a gesture, the button remains as the fallback.

#### On foot

| Action | VR binding |
|---|---|
| Move | L stick; the magnitude selects walk or run |
| Body turn | R stick, smooth by default, snap as an option |
| Fire the dominant-hand weapon (`FIRE_RIGHT`) | R trigger |
| Fire the left pistol while dual-wielding | L trigger (see "Dual-wield") |
| Fire grapple (`FIRE_GRAPPLE`: fire; tap while attached to zip; hold to tether) | L grip outside grab volumes; the yank gesture also zips |
| Retract tethers (`RETRACT_GRAPPLE`) | L trigger; L stick click while dual-wielding, where the L trigger fires the left pistol |
| Release tethers and cancel (`RELEASE_GRAPPLE`, `CANCEL`) | Y |
| Jump, stunt, open parachute | A |
| Use item, enter vehicle, open wingsuit | X |
| Reload | B; the opt-in magazine gesture also reloads |
| Holster | grip press with the weapon hand at the held weapon's holster; a short hold of B also holsters, as on the gamepad |
| Weapon slot select | draw from a holster; the L stick click cycles `NEXT_WEAPON` as a fallback (on by default; not while dual-wielding) |
| Throw grenade | grab from the grenade pouch and throw; R stick click is the fallback |
| Plant and detonate explosives | R stick click (context) |
| Melee (`PUSH_GRAPPLE` under the hammer ability) | a physical strike with either hand; no button |
| Precision aim and sniper zoom | removed; scopes magnify optically (see "Scopes") |
| Reel-in context: cancel, boost, hang jump, slingshot | Y, hold L trigger (hold L stick click while dual-wielding), A, A |
| Pause, commlink | System button tap, System button hold (either hand) |
| Crouch toggle (seated posture) | R stick flicked down |

#### Land vehicles (car; motorcycle in the last rows)

| Action | VR binding |
|---|---|
| Steer | L stick; opt-in wheel or handlebars |
| Accelerate and reverse | R trigger and L trigger |
| Handbrake | Y |
| Nitrous and turbo jump (`USE_VEHICLE_MOD`, upgrade-gated) | hold B and tap B |
| Fire vehicle weapons, primary and secondary | R grip and L grip |
| Enter gunner seat, roof stunt | hold X, A |
| Exit vehicle | X |
| Horn | L stick click |
| Look back, vehicle camera, recenter camera | removed; the neck replaces them, and F7 recenters |
| Motorcycle personal weapon (`MC_FIRE`) | R grip, aimed by the right hand |
| Motorcycle lean and tilt | L stick; opt-in handlebar lean |

#### Helicopter

| Action | VR binding |
|---|---|
| Collective up and down | R trigger and L trigger; opt-in collective lever |
| Cyclic (forward, back, and roll) | L stick; opt-in cyclic stick |
| Yaw | R stick X |
| Fire primary and secondary | R grip and L grip, aimed by the gaze |
| Exit, stunt, nitrous | X, A, hold B |

#### Plane

| Action | VR binding |
|---|---|
| Pitch and roll | L stick; opt-in flight stick |
| Rudder | R stick X |
| Thrust up and down | R trigger and L trigger; opt-in throttle |
| Fire primary and secondary | R grip and L grip, aimed by the gaze |
| Exit, stunt | X, A |

#### Boats and jetskis

| Action | VR binding |
|---|---|
| Steer | L stick; opt-in wheel or handlebars |
| Accelerate and reverse | R trigger and L trigger |
| Fire, and the jetski personal weapon | R grip, aimed by the gaze (boat) or the right hand (jetski) |
| Nitrous and turbo jump | hold B and tap B |
| Exit | X |

#### Wingsuit and parachute

| Action | VR binding |
|---|---|
| Steer (analog `MOVE_*`) | L stick; opt-in arm-pose steering |
| Air brake (`CANCEL` hold, upgrade-gated; `WINGSUIT_AIRBRAKE` when weaponized) | L trigger |
| Boost and evade (weaponized) | B; A with a stick flick |
| Fire weapons (weaponized wingsuit or parachute) | R trigger and R grip, aimed by the right hand |
| Open or close the parachute | A |
| Grapple (slingshot boost) | L grip |

#### UI

While a game menu is open, the `ui` action set replaces the gameplay bindings, matching the game's own `END_UI_MAPPINGS` boundary. Both hands navigate the menus alike, with each hand's grip as its modifier; the layout as built is in [controllers.md, "Bindings and injection"](controllers.md#bindings-and-injection). A laser pointer on the floating panel is deferred with the rest of the mod UI. `payload/src/hud/pointer.rs` already isolates the pointer source for it.

## Aiming

### Guns

The dominant hand's ray reaches each held weapon's `m_AimTargetPosition` through the per-weapon aim update: the base update (`CWeaponBase::UpdateFromUserPostSim_Parallel` through its base) is handed the hand's aim point in place of the character's shared one, and the weapon stores it before it builds the shot. The ray is the weapon's sight line, not the controller's own axis: the gun's transform carried to the controller pose, times the gun's current fire point, times a per-weapon sight correction. The target point is the ray's hit point, found with `CPhysicsSystem::CastRay` and the player aim's own ray filter from the sim phase, and it is kept well beyond the near-muzzle distance at which `GetShotMatrix` falls back to the barrel direction. The per-weapon field is the seam because it is the only one that gives two weapons two directions: the character-level aim point (`m_AimTargetPositionWeapons`) fans out unchanged to every held weapon. The character-level write still runs every frame, with the dominant hand's aim point, because a frame without it sets no `WasSetThisFrame` flag and the game then aims every weapon 25 m ahead of the body ([aim-pipeline.md, "From aim control to the character"](../../engine/gameplay/aim-pipeline.md#from-aim-control-to-the-character-cplayerupdatepostcamera)).

The per-weapon sight offsets are derived and then tuned. The fire-point matrices (`m_InternalFirePositions`) give the muzzle origin and the barrel axis: `GetShotMatrix` treats the composed fire-point matrix's -Z axis as the barrel. An offline tool, `tools/sights/`, reads the weapon models from the game archives, derives the sight line from the model geometry above the fire point, and emits `tools/sights/sights.json`, keyed by the weapon name hash, with a confidence grade per weapon ([weapon-data.md](../../engine/gameplay/weapon-data.md) has the files it reads). Scoped sights and pistols derive reliably; iron sights on rifles with rails, muzzle devices, or carry handles do not, and are graded uncertain. The payload embeds the table and uses only its trusted grades; a per-weapon config entry overrides it, and a weapon with neither aims along its fire point. A tuning tool in the debug UI writes the config entry: the player lines up the sights on a marker and captures, and the entry can be copied into the table ([controllers.md, "Sight lines"](controllers.md#sight-lines)).

Guns show no reticle by default. The options are a dot at the hit point, at the hit point's true depth so that it fuses in stereo, and a laser line from the muzzle to the hit point. Both are drawn by the world-space overlay layer. The game's own weapon reticle, which marks the gaze's aim point, is hidden while the guns aim by hand, and its grapple reticle while the grapple does (`overlay.hide_game_reticle`, on).

### The grapple

The grapple slot, index [`TargetType::TARGET_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::target::TargetType##variant-TARGET_GRAPPLE), of the player aim control's slot arrays ([`m_AimPos`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_AimPos), [`m_DirectPositions`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_DirectPositions), [`m_DirectTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_DirectTargets), [`m_DirectHits`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_DirectHits), [`m_IsInRange`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_IsInRange), and [`m_Target`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_Target)) is filled from the left controller's ray by the game's own targeting. Every part of the aim control that fills the slot takes its ray from the adjusted camera matrix or the camera matrix getter. While a thread-local override is set, both getters return a camera matrix built from the grapple ray. [`PlayerAimControl::UpdateDirectAim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-UpdateDirectAim) runs twice: once under the override, from which the grapple slot is kept, and once without it, which refills every other slot from the camera. The grapple target scoring, its visibility casts, and the re-validation of the last-valid grapple cache also run under the override, so [`m_LastHitGrappleTargetGO`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_LastHitGrappleTargetGO) and the grapple slot of `m_Target` are filled the way the game would fill them, and entity attachment and tethering follow the left hand. Every grapple consumer reads the grapple slot, so the hook fire, the tether semantics, and `IsLastValidGrappleAimPosStillValid` follow the left hand without further changes. Three alternatives are rejected. A post-hook on `UpdateDirectAim` that casts its own ray and writes each slot array by hand would have to reproduce the game's filter, range, holey-geometry handling, and target resolution. Overwriting only the latched target at fire time (`CGrapplingHook::SetUpTarget`) corrects the hook but leaves every reader of the live slot on the gaze ray. Telling the grapple's `GetCameraMatrix` calls apart by return address does not work, because the one raycast inside `UpdateDirectAim` serves the weapon and grapple slots together.

The game's grapple reticle pins to the centre of the screen when the aim uses a direct position, so it cannot show a hand ray. The grapple readout is drawn by the world-space overlay layer instead: a highlight on the surface or object the grapple would attach to, shown only when the target is valid and in range, with a distinct style for targets that support tethering, and a gauntlet beam from the wrist to the target point. Each is configurable.

The yank gesture is a sharp pull of the left hand back toward the body while a line is attached. It issues the same `FIRE_GRAPPLE` tap that zips.

### Dual-wield

The dual-wield slot is one `CWeaponBase` with a `CWeaponData` component per gun. Each gun has its own attach bone, so gun 0 rides the dominant hand and gun 1 rides the other hand. On foot the game exposes one fire input and alternates the barrels internally, at the tail of `Fire_Projectile` (the component index [`WeaponBase::m_NextWeaponToFire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_NextWeaponToFire) and each component's fire-position cursor [`WeaponData::m_InternalFirePositionCurrentIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_InternalFirePositionCurrentIndex)). Each trigger fires its own component: the mod pins both the cursor and the dispatched `CWeaponData` to that component, so that the muzzle origin and the flash stay consistent, and writes that hand's ray into `m_AimTargetPosition` before the shot is built. Ammunition, recoil, heat, and effects are counted per fire call, never per barrel, so nothing is counted twice.

While the off-hand holds a pistol (on foot, swimming, falling, under the parachute, or in a passenger seat), the off-hand's trigger fires it, so retracting the tethers (and the reel boost, which is the same action) moves to the left stick click. The stick click's weapon-cycle fallback, when it is enabled, yields to it while dual-wielding; the holsters draw the other slots. The stick click is the one off-hand input that is otherwise idle on foot while the holsters draw the weapons, and it keeps retraction on the grapple hand.

Each hand draws its own pistol at a hip holster and holsters it there; an undrawn pistol waits on the hip on its hand's side, and an empty hand's trigger fires nothing ([controllers.md, "Recoil, haptics, dual-wield, and the grapple hand"](controllers.md#recoil-haptics-dual-wield-and-the-grapple-hand)).

The two guns share one weapon state, so a reload is one animation for both. Under the action-runs tier the hands stay on the controllers, and both guns stop firing for the reload's duration.

### Aim assist

Aim assist is re-derived per hand ray. The mod scores the game's own hostile target candidates against each hand's sight line with the game's own weapon fitness function ([`PlayerAimControl::GetFitnessScore`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-GetFitnessScore)), and moves the written aim point toward the best visible candidate by a strength factor. The default strength is zero ([aim-pipeline.md, "Aim assist"](../../engine/gameplay/aim-pipeline.md#aim-assist-updatealltargets)).

### Vehicle, mounted, and wingsuit weapons

[`Vehicle::SetAimTargetPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-SetAimTargetPosition) writes the vehicle aim point ([`Vehicle::m_AimingTargetPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_AimingTargetPosition)) only for the driver's seat, and every built-in vehicle weapon aims from it. The driver's vehicle weapons aim along the gaze by default, which is the current behaviour. An option substitutes the right hand's aim point in the driver's `SetAimTargetPosition` call. Turrets turn toward the aim point at a limited rate, so a hand-aimed turret visibly trails the hand. Mounted guns, including a gunner's seat, use the inventory slot [`WeaponSlot::E_SLOT_HEAVY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::WeaponSlot##variant-E_SLOT_HEAVY) and the per-weapon seam, and aim by hand: the hands anchor to the gun's handles, and moving them swings the muzzle about the gun's pivot ([controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight)). Weaponized wingsuit guns aim by the right hand through the per-weapon seam.

### Recoil and haptics

Each held weapon has a recoil offset between the controller pose and the rendered weapon. Every shot adds a kick to the offset, scaled from the game's per-weapon recoil accumulators ([`WeaponBase::m_CurrentScatterValue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_CurrentScatterValue) through [`WeaponBase::m_RecoilPitch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_RecoilPitch), advanced once per shot), and a spring returns the offset toward zero. The aim ray follows the rendered weapon, so the kick affects accuracy. A player who holds still sees the gun climb; a player who pulls against the climb keeps it on target, as with real recoil. When firing stops, the spring returns the weapon to the controller pose. A two-hand grip on the foregrip stiffens the spring and reduces the kick. The game's own scatter and spread stay in effect on top of the offset.

The game's own camera recoil kick is suppressed while the guns aim by hand, because the camera belongs to the headset.

Haptic pulses come from the game's own rumbles: every one-off rumble goes through [`RumbleManager::SpawnRumble`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/rumble_manager::RumbleManager##func-SpawnRumble). A weapon's fire rumble goes to the hand whose gun fired, scaled with the kick, and the other rumbles go to the hand on the gamepad motor's side. Grapple fire, attachment, and the yank pulse on the grapple hand.

## Weapon handling

### Holsters

The holsters are grab volumes on the body: the hips for pistols, over the shoulders for two-handed and special weapons, and the chest for explosives. The two shoulders split the long weapons: the dominant shoulder holds the two-handed slot and the other shoulder the special slot. The volumes are placed from the player's body dimensions. A grip press in a holster draws that slot's weapon with `SELECT_DUEL_WIELD`, `SELECT_TWO_HANDED`, `SELECT_TWO_HANDED_SPECIAL`, or `SELECT_EXPLOSIVES`. A grip press by the hand holding a weapon, inside that weapon's own holster, holsters it through `Character::HolsterWeapon`. The short hold of the reload button that holsters on the gamepad still works, but the game decides that hold in the animation graph, and with a heavy weapon the same hold drops it, so the holsters do not use it. The draw and the holster play under the action-runs tier. A seated player's holsters are placed for a seated body: the hips higher up the torso and forward over the thighs, and the shoulders over the tops of the shoulders, clear of a backrest. The weapon cycle on the L stick click stays on as a fallback, for a player who cannot reach the holsters. The radial menu fallback is deferred with the rest of the mod UI.

### Two-handed weapons

A two-handed weapon rides the dominant hand's attach bone and aims by that hand alone. The foregrip is a grab volume for the off-hand. While the off-hand holds the foregrip, the weapon's direction is the line from the dominant hand to the off-hand, the off-hand's wrist is IK-pinned to the foregrip, and the recoil spring stiffens. The off-hand releases the foregrip with a grip press or by moving well away from it. The engine's own secondary-hand grip IK (`UpdateSecondaryHandIKPass`) is suppressed for the off-hand, so that the engine does not pull the hand onto the foregrip on its own.

### Reloads

The reload button starts the native reload. The weapon cannot fire until the reload finishes, and the weapon model plays its own magazine and bolt animation while the hands stay on the controllers. The opt-in magazine gesture moves the off-hand to a belt pouch and then to the weapon's magazine well, which starts the same reload. The button remains.

### Grenades

A grip press on the grenade pouch puts a grenade in the hand; releasing the grip throws it. The throw animation normally drives the grenade weapon through two requests, and the mod raises them itself, so no wind-up plays ([aim-pipeline.md, "Grenades"](../../engine/gameplay/aim-pipeline.md#grenades)): the grip press raises the prime request ([`ThrownGrenadeWeapon::m_PrimeGrenade`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::ThrownGrenadeWeapon##field-m_PrimeGrenade)), and the release raises the fire request once the grenade has its physics body. A post-hook on [`Grenade::PropelToward`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::Grenade##func-PropelToward) moves the launch to the hand and overwrites the solved launch velocity, [`Explosive::m_DeferredInitialVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::Explosive##field-m_DeferredInitialVelocity), with the hand's release velocity, estimated from a short history of the hand's motion. The fuse runs from the prime, so a grenade held too long explodes in the hand; a ring at the hand counts the fuse down. The R stick click fallback throws with the game's own animation and arc, toward the hand ray's aim point.

### Melee

A strike with either hand above a speed threshold, moving away from the shoulder, triggers the melee attack. The game's own melee scoring ([`PlayerAimControl::SetTargetMeleeFitness`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-SetTargetMeleeFitness)) rates the targets along the strike direction and along the hand's pointing, and the best visible one is written into the melee target slot (the [`TargetType::TARGET_MELEE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::target::TargetType##variant-TARGET_MELEE) entry of [`PlayerAimControl::m_Target`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_Target)). A strike acts only on the ground and only when the game allows melee: [`Character::CanUseMelee`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-CanUseMelee) holds and the hammer ability is unlocked.

### Scopes

Precision aim adds a camera-tree FOV hint, shows a 2D sniper overlay, and hides the player's body, which also hides the hands. The mod suppresses all three. The suppression is a refusal at [`Player::SetAimMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::player::Player##func-SetAimMode), through which the precision-aim input, the sniper rifle's bolt-action cycle, and the guided rocket launcher's shot all enter precision aim. A scoped weapon instead shows a magnified image in a lens on the scope while the scope is near and in front of an eye. The lens resamples the other eye's rendered image at the aim point's distance, because a crop of the sighting eye's own image would magnify the scope's model. A narrow-FOV third scene render, drawn only while the scope is at the eye, in mono, and at reduced resolution, would show true parallax; it is not built, and [controllers.md, "Scopes"](controllers.md#scopes) lists what it needs.

## The world-space overlay layer

A world-space overlay layer draws sprites and lines into the scene with depth, per eye. It carries the grapple highlight and beam, the optional gun dot and laser, the grab-volume highlights, the ghost hands of the yielded tier, and debug controller gizmos. The floating HUD panel cannot do this, because it draws a 2D texture on one plane, and the per-hand readouts need their own depths. The layer draws after post-processing and before the HUD, and tests against the scene depth in its pixel shader ([controllers.md, "The world-space overlay"](controllers.md#the-world-space-overlay)).

## Roomscale design

The design for physical locomotion is resolved against the engine answers in [hands-locomotion-and-state.md §4](../../engine/character/hands-locomotion-and-state.md#4-proxy-input-scene-queries-teleports-seat-pose-root-motion-capsule-and-pause). It follows the topology in [head-and-body.md](../body/head-and-body.md): the head is the ground truth and the body follows it. The camera is never derived from the body's motion, so the view stays 1:1 with the real head regardless of what the body does. The chase loop runs only in the roomscale posture.

### The chase loop

The camera is placed from the measured HMD pose each frame as the anchor plus the cockpit offset. The anchor is the character's animated head bone, and the cockpit offset is the HMD pose relative to the recenter baseline. The anchor moves with the body, so a chase that moved the body without consuming the offset would move the camera by the body's displacement on top of the unchanged cockpit offset: the view would run ahead of the real head, the measured offset would never shrink, and the body would chase the head without end. The loop therefore re-bases the cockpit baseline by the horizontal body displacement the chase causes each tick, converted into the cockpit frame and divided by the world scale, so the camera stays with the real head while the body catches up. Per sim tick:

1. Measure the body-frame XZ offset from the character to the player's physical head (the cockpit position the pose path already computes).
2. Outside a deadzone of about 10 cm, add a chase velocity `clamp(offset / time_constant, max_speed)` to the on-foot locomotion result. The time constant is a few sim ticks, so the body closes the offset without overshooting the head, and a chase that has started runs until the offset is below half the deadzone. The seam is a post-call hook on the locomotion task, adding to the velocity it writes to the character proxy ([`CharacterInput::wantedVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CharacterInput##field-wantedVelocity), in [`PfxCharacterInstance::m_Input`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_Input)). That velocity is in the character's local frame, not world space ([hands-locomotion-and-state.md §4.1](../../engine/character/hands-locomotion-and-state.md#41-the-locomotion-task-runs-at-idle-and-writes-the-proxy-input-directly)), so the add goes through the character's world wanted-velocity path: [`Character::GetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-GetWantedWorldVelocity) reads it in world space, the chase velocity is added, and [`Character::SetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetWantedWorldVelocity) converts the sum back into the local frame. The vertical component and `gameControledVelocity` stay as the task set them. The task runs every on-foot tick including idle, so walking from a standstill works. The chase runs at the sim tick, never the render frame. The engine's own displacement evaluation uses either root motion or code-driven displacement, never both, so the add does not double-count against it.
3. The proxy resolves collision, stairs, and slopes. The displacement the body achieves, measured from the character's world position with the stick-driven part excluded, is the displacement that re-bases the baseline, so a chase that a wall blocks re-bases nothing. A wall between the player and the body leaves a persistent offset: the view stays with the real head, and a probe from the last unobstructed head position to the current one drives a fade when the head goes deep into geometry. A second ray from the animated head, which the capsule keeps in the open, clears a stale last position. The probe uses the player aim filter, because the unfiltered `CastRaySimple` can hit the player's own ragdoll and hit-proxy bodies ([hands-locomotion-and-state.md §4.2](../../engine/character/hands-locomotion-and-state.md#42-raycast-and-scene-query-api-cphysicssystemcastray-and-related-functions)). The probe runs in the sim phase, never from a render hook, because scene queries run against the live physics world.
4. Physical walking and stick walking share one channel. The chase velocity scales down with the stick magnitude, so deliberate stick locomotion takes precedence, and both are expressed as the same body-local move vector.

The chase applies only through the post-call hook on the on-foot locomotion task. Every other state that moves the character proxy (grapple, jump, stunt, and ragdoll) is driven by a different task, which the hook does not touch, and a seated character follows its vehicle through the seat attachment, so neither receives the chase; the loop also stands down while `IsInVehicleAttachState` holds. Publishing the offset as the game's code-driven displacement direction is an alternative seam, rejected because it interacts with the game's own code-driven displacement users.

Physical translation never rotates the body, consistent with the rule that orientation changes only with a movement action. The chase velocity is body-local. The aim-relative strafe blend space, which the FPS-movement shim already selects, animates sidesteps and backpedals, and the body-follows-head deadzone scheme owns the torso yaw.

A teleport re-bases the cockpit baseline for that tick instead of feeding a chase spike. The game has no reliable per-character teleport flag: the character field once believed to be one is the stunt auto-exit setting ([`AbilityFlags::m_IsStuntAutoExitEnabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AbilityFlags##flag-m_IsStuntAutoExitEnabled) in [`Character::m_AbilityFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_AbilityFlags)), and the per-character frames-since-teleport counter is never reset ([hands-locomotion-and-state.md §4.3](../../engine/character/hands-locomotion-and-state.md#43-teleport-writers-and-teleport-signals)). Detection is a post-call hook on [`GameCameraManager::ResetCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::game_camera_manager::GameCameraManager##func-ResetCamera). Every discontinuity the game applies to the player resets the camera through it: the instant teleport, the placement step of the deferred teleport, the respawn after death, and loading a save. One hook therefore covers teleports, respawns, and loads, which a hook on the instant teleport alone does not. Writers that bypass the game world, such as a vehicle teleport carrying its seated driver and scripted warps, do not reset the camera, so the chase loop also treats a single-tick body jump beyond a distance that grows with the body's speed (`roomscale.chase.warp_min_m` at rest) as a teleport. Either signal records a teleport for the chase loop and the frame loop to consume: re-base the cockpit baseline, zero the chase, and hold the mode for that tick.

Recentering on foot re-bases yaw and height only while the chase is active (the roomscale posture, the chase on, and out of a seat). The chase loop keeps the position converged, so no position needs recentering, and a position recenter could move the body through a wall that the chase could not cross. Without the chase, F7 keeps its full re-base, and the seat transitions always re-base the position.

### Crouch, in stages

1. Visual crouch, first and behind config: a positional target on the hips, lowered by a share of the head's drop, and the ankles pinned where the animation put them fold the body through the same HumanIK pass the head target already uses, solved at the full-body step. The feet stay planted and the hips drop.
2. Capsule height: the engine swaps the proxy capsule per state itself (`SetProxyState`, which calls `hknpWorld::setBodyShape`), and every character has a crouching proxy state with a shortened capsule ([hands-locomotion-and-state.md §4.6a](../../engine/character/hands-locomotion-and-state.md#46a-capsule-dimensions-and-runtime-resize)). The mod requests that state through the character's pending proxy state ([`Character::m_PendingProxyState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_PendingProxyState)), which the engine applies on the sim thread. The request is made only while the pending state is the default or already the crouch, so the engine's own proxy states for swimming, the parachute, vehicles, and ragdoll win, and it is released only when a ray up from the feet finds the headroom to stand. This stage requires the visual crouch.
3. AI response: whether enemy perception responds to a lowered head is measured in the headset.

In the seated posture, the crouch toggle moves the eye height between the standing and crouched presets, and drives the same visual crouch.

### Seat transitions

JC3 vehicles are entered through animations in which Rico crosses metres of world, opens doors, and hijacks drivers. Under a head-anchored camera the entry is a forced camera movement that ends at an arbitrary yaw, which is a strong vection source. Bonelab avoids the problem (its vehicles are mount-in-place with trigger-volume seats and no entry animation), and LukeRoss's R.E.A.L. GTA V mod, a seated camera-only port, advises players to accept the ride or close their eyes. The mode enum below shares one seat re-base mechanism: the seated head reference is the character's own head and eye bones, which ride the vehicle through `m_AttachBone` and `m_AttachOffset`. The game's vehicle predicates do not separate an entry, a locked seat, and an exit, so the seat phase is classified from the body rule system's current state by name.

- Instant: at seat lock the cockpit re-bases immediately and the world stays live. The player's real posture is tolerated, and the player can recenter with F7. The entry animation is hidden by a fade that clears at the lock; the exit plays live and re-bases yaw and height once the character is out.
- Fade: fade out when the entry animation takes the camera, and fade in seated once the re-based view has settled (`roomscale.seat_fade_hold_s`). The exit also plays behind a fade.
- Ride: the head stays anchored through the full animation with nothing hidden. It is the default until the other modes are proven in the headset, because it is the mod's existing behaviour, and it stays as the intense option.
- Ceremony, deferred with the rest of the mod UI because it needs a confirmation prompt: the entry animation plays; at seat lock the game clock freezes and the floating panel asks the player to take their seat and confirm; on confirmation the mod re-bases, unfreezes, and enables the `vehicle` action set. The freeze is a mod-side clock freeze through the hooked `CClock::Update` (zero game dt, skipped sim tick), never the game's own pause, because `UpdateRenderPaused` does not run `CameraTree::UpdateRenderContexts`, and a real pause freezes the camera under head motion. Exit reverses the sequence. Whether the frozen world dims is decided in the headset.

The roomscale chase is disengaged while seated in every mode.

## Milestones

The milestones follow a vertical slice: the riskiest part of the design (the aim-state pin, controller-owned hands, and per-hand aiming) is built and tested on foot first, and breadth follows. The only developer and user for the foreseeable future is the project author, so milestone 1 hardcodes the author's body dimensions in config rather than waiting for calibration.

### 0. Bindings

Author the pyxis definitions for the seams milestone 1 uses, and verify each offset independently against the IDB, because prose transcription has no compile check. Definitions go in `20206564`. The list:

- `CPlayerAimControl`'s slot arrays (`m_AimPos`, `m_DirectPositions`, `m_DirectTargets`, `m_DirectHits`, `m_IsInRange`, `m_Target`) and the grapple cache;
- `CWeaponBase`'s `m_AimTargetPosition`, `GetShotMatrix`, the recoil accumulators, the component vector, `GetCurrentBoneAttachement`, `ChangeBoneAttachement`, `UpdateRender`, and the wall-penetration flag;
- `CWeaponData`'s T0 and T1 matrices, the fire-position array and cursor, and the model-instance pointer, plus `CModelInstance::UpdateMatrix`;
- `CPhysicsSystem::CastRay` and `CastRaySimple`, and `CRaycastFilter`;
- the character state-bitflag words ([`Character::m_ControlParameters`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_ControlParameters) and the [`UserVM::m_CurrentStateBitFlags0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::UserVM##field-m_CurrentStateBitFlags0) words), the vehicle-class predicates, and `GetVehiclePtr`, for the mode detector;
- `HandleAimingInputPlayer`, `PreSimUpdateGrapple`, `UpdateMentalState`, `NAimIKTask::Update`, `NRightArmAimIK::Update`, `NGrapplingFireArmIK::Update`, and `NReelInAimIK::Update`;
- corrections to the existing `Character` flag bytes: [`AimState::m_InCargoBay`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AimState##flag-m_InCargoBay) in [`Character::m_AimFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_AimFlags) is the in-cargo-bay flag, and [`DamageStateFlags::WEAPONS_ENABLED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::DamageStateFlags##flag-WEAPONS_ENABLED) in [`Character::m_DamageFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_DamageFlags) is the weapons-enabled flag, not a damage flag.

### 1. The on-foot slice

- The OpenXR action sets and pose actions, the mode detector, and the scheme hot-swap. Every mode uses its bindings above, so the rest of the game stays playable.
- The aim-state pin and the mental-state cuts.
- Controller-owned hands: wrist IK to the controllers, the weapon on the dominant hand's attach bone, the game's arm IK skipped, and the render-time late update for the weapon and the hand bones. The body dimensions come from config.
- Gun aiming from the dominant hand, with sight axes derived from the fire-point matrices only (milestone 5 adds the table derived from the models).
- Grapple aiming from the left hand, with the game's own targeting and target scoring run along the hand's ray.
- The world-space overlay layer, with the grapple highlight and beam, the optional gun dot and laser, and debug controller gizmos.
- The free and yielded hand tiers; every scripted state starts as yielded unless its table entry is free, and the overlay draws ghost hands for the yielded ones.
- Weapon slots cycle from the L stick click. Milestone 3's holsters replace the cycle, which stays on as a fallback.

### 2. Posture and roomscale

- The posture setting and STAGE-space support, the stage-bounds check, and the seated crouch toggle.
- The Bonelab-style remap, still driven by config dimensions.
- The chase loop, the teleport re-base, and the head-through-geometry fade.
- The visual crouch, then the capsule swap.
- The Instant, Fade, and Ride seat-transition modes.

### 3. Holding

- Holsters and grab volumes, with the overlay highlights.
- The two-handed foregrip.
- Recoil and haptics.
- The dual-wield split.
- The action-runs tier for reloads and draws, and the anchored tier for grapple lines and hangs.
- The yank gesture.

### 4. Vehicles and flight

- Per-class vehicle bindings, playtested class by class.
- Vehicle weapon aiming (the gaze by default, with the hand option), and hand-aimed mounted guns and turrets.
- The opt-in physical controls: wheel, handlebars, cyclic and collective, flight stick, and throttle.
- The arm steering for the wingsuit, the handles for the parachute, and the removal of the head look-steer from the wingsuit's steering.

### 5. Breadth

- The physical grenade throw.
- Physical melee.
- Scope lenses.
- Per-hand aim assist.
- The magazine gesture for reloads.
- The model-extraction pass for sight offsets (`tools/sights/`), and the sight-tuning tool in the Controllers tab.
- Promotions in the hand-tier table, in order of how often the states occur in play. None beyond the anchored grapple and mounted-gun rows is built.

## Implementation status

Milestones 1 to 5 are implemented and have not been run in the headset. [controllers.md](controllers.md) describes the implementation as built; the tables below map each milestone's parts to the settings that control them and to the section of controllers.md that describes them. Every setting below is in the `controllers` section of the mod's config and in the Controllers tab of the debug UI, which also shows a live readout of each part. The master switch is `controllers.enabled`; with it off, no controller snapshot is published and every controller part is inert. The parts that change how the character behaves act only under the controller scheme, so the gamepad scheme behaves as before. The posture and roomscale parts are the exception: they act on the headset alone, controllers or not; the defaults are the full `Vehicles and advanced` preset (see [controllers.md, "Test controls and saved tuning"](controllers.md#test-controls-and-saved-tuning)), so these too are on by default.

A setting marked "off" below is off by default and has to be switched on to test its part. Every other part is on by default.

### Milestone 1: the on-foot slice

| Part | Settings | State |
|---|---|---|
| OpenXR actions, poses, haptics queue, and hand tracking | `enabled`, `input.trigger_press`, `input.squeeze_press`, `input.stick_deadzone`, `input.hand_tracking` | Built. The hand-tracking joints are published for the finger curls. |
| Scheme selection | `scheme` (`Auto`, `Controllers`, or `Gamepad`) | Built. |
| Body dimensions and remap | `body.local.json` beside the payload DLL; `body.remap_hands` (on), `body.remap_head` (on) | Built. The head remap feeds the view through the posture (milestone 2). |
| Mode detection | `mode.debounce_ticks` | Built. The headpose sim keeps its own on-foot test. |
| Bindings and injection | `bindings.enabled`, `bindings.move_deadzone`, `bindings.trigger_deadzone`, `bindings.long_press_s`, `bindings.evade_flick`, `bindings.evade_window_s`, `dominant_hand`, `locomotion.direction` (`Head` or `OffHand`) | Built for every mode. An off-hand grip press inside a grab volume (milestone 3) grabs, and elsewhere fires the grapple. Falling and under the parachute the dominant hand keeps the on-foot fire, reload, and grenade bindings. The wingsuit's A evades only on a stick flick. |
| Gamepad semantics | `gamepad_semantics` | Built. |
| Aim-state pin and mental-state cuts | `aim_state.pin`, `aim_state.pin_grapple`, `aim_state.skip_arm_ik`, `aim_state.neutral_aim_point`, `aim_state.neutral_aim_distance_m`, `aim_state.clear_wall_block`, `aim_state.mental_state_cuts`, and `aim_state.without_controllers` (off; applies the other parts under the gamepad scheme for testing) | Built, except the strafe reference correction (see "The aim-state pin"). |
| Hands | `hands.ik`, `hands.late_update`, `hands.suppress_secondary_ik`, `hands.reach_t`, `hands.reach_r`, `hands.pull`, `hands.resist`, `hands.grip`, `hands.yield_blend_s`, `hands.return_blend_s`, `hands.tiers` | The free and yielded tiers are built; milestones 3 and 4 add the action-runs and anchored tiers. A state assigned a tier that is not built for it yields. A left-handed layout mirrors the bindings but does not move a single gun to the left hand. The grapple wire's render-time correction and elbow pole hints are not built. |
| Gun aiming | `aim.gun`, `aim.max_distance_m`, `aim.min_target_distance_m`, `aim.sights` | Built. The sight line starts from the fire point, corrected by the config entry or the embedded table (milestone 5). |
| Grapple aiming | `aim.grapple`, `aim.grapple_origin_m` | Built. |
| World-space overlay | `overlay.enabled`, `overlay.gun_readout` (`None` by default, `Dot`, or `Laser`), `overlay.grapple_highlight`, `overlay.grapple_beam`, `overlay.hide_game_reticle`, `overlay.ghost_hands`, `overlay.debug_gizmos` (on), `overlay.depth_tolerance_m`, `overlay.depth_tolerance_rel`, `overlay.min_line_px`, `overlay.xray_alpha` | Built. |

### Milestone 2: posture and roomscale

The settings are under `roomscale`, except the seated crouch flick (`bindings.crouch_flick`) and the head remap (`body.remap_head`). The description is in [controllers.md, "Posture and roomscale"](controllers.md#posture-and-roomscale).

| Part | Settings | State |
|---|---|---|
| Posture, STAGE space, and the stage-bounds check | `posture` (`Auto`, `Seated`, `Standing`, or `Roomscale`), `roomscale_min_extent_m` | Built. `Auto` never selects the seated posture. |
| Head height remap | `body.remap_head` (on) | Built. |
| Seated crouch toggle | `bindings.crouch_flick`, `crouch.seated_drop_m` | Built. It works with or without the head remap. |
| The chase loop and the teleport re-base | `chase.enabled` (on), `chase.deadzone_m`, `chase.max_speed_mps`, `chase.time_constant_s`, `chase.warp_min_m` | Built. |
| The head-through-geometry fade | `head_fade.enabled` (on), `head_fade.start_depth_m`, `head_fade.full_depth_m`, `fade.out_s`, `fade.in_s` | Built. The fade darkens the image in the VR blit. |
| The visual crouch and the capsule | `crouch.visual` (on), `crouch.capsule` (on; requires the visual crouch), `crouch.hips_share`, `crouch.enter_drop_m`, `crouch.exit_drop_m` | Built. The AI response to a lowered head is not measured. |
| Seat transitions | `seat_transition` (`Ride` by default, `Instant`, or `Fade`), `seat_fade_hold_s` | Built. Ceremony is deferred with the mod UI. |

### Milestone 3: holding

The settings are under `weapons` unless named otherwise. The description is in [controllers.md, "Holsters, grab volumes, the foregrip, and the action-runs tier"](controllers.md#holsters-grab-volumes-the-foregrip-and-the-action-runs-tier), with recoil, haptics, dual-wield, the anchored tier, and the yank in ["Recoil, haptics, dual-wield, and the grapple hand"](controllers.md#recoil-haptics-dual-wield-and-the-grapple-hand).

| Part | Settings | State |
|---|---|---|
| Holsters and grab volumes | `holsters.enabled`, `holsters.hip` (radius 0.15 m), `holsters.shoulder` (0.17 m), `holsters.chest` (0.10 m), `holsters.pouch` (0.10 m, at the belt front on the off-hand side, either hand), `holsters.stick_click_cycles` (on), `holsters.show_volumes` (off), `holsters.detect_seated` (on), `holsters.seated_eye_fraction` (0.8), `holsters.seated_after_s` (4 s), `overlay.grab_volumes` | Built, with a seated placement. The volume offsets and radii are untuned. |
| The two-handed foregrip | `foregrip.enabled`, `foregrip.radius_m`, `foregrip.release_distance_m`, `foregrip.blend_s`, `foregrip.fallback_distance_m`, `foregrip.min_span_m` | Built. |
| The action-runs tier | `block_fire_during_actions` | Built for reloads, draws, and holsters. |
| Recoil | `recoil.enabled`, `recoil.suppress_camera_kick`, and the kick and spring tuning under `recoil` | Built. |
| Haptics | `haptics.enabled`, `haptics.intensity`, `haptics.game_rumble`, `haptics.grapple` | Built. |
| The dual-wield split | `dual_wield.split`, `dual_wield.place_by_handedness`, `dual_wield.independent_draw` | Built. Independent draws are untested in the headset. |
| The anchored tier for the grapple line and the hang push | `hands.anchor.enabled`, `hands.anchor.hang_swing`, and the swing tuning under `hands.anchor` | Built for the reel and hang states. A grapple hand that holds a gun stays free. |
| The yank gesture | `bindings.yank.enabled` | Built. It is disarmed while the line carries the character (the reel-in and the hang). |
| F7 during the chase | none | Re-bases yaw and height only while the chase is active ([controllers.md, "Posture and roomscale"](controllers.md#posture-and-roomscale)). |

### Milestone 4: vehicles and flight

The settings are under `vehicles`. The description is in [controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight).

| Part | Settings | State |
|---|---|---|
| Per-class vehicle bindings | the milestone 1 bindings | Built in milestone 1; each class needs its own playtest. Boats and jetskis share one class. |
| Driver weapons by hand | `driver_weapons_by_hand` (on) | Built. The dominant hand aims the driver's weapons by default. |
| Mounted guns and gunner seats | `mounted.aim_by_hands`, `mounted.anchor_hands`, `mounted.min_lever_m` | Built. |
| Physical controls | `physical.wheel`, `physical.handlebars`, `physical.cyclic`, `physical.collective`, `physical.flight_stick`, and `physical.throttle` (each on), `physical.grab_radius_m`, `physical.release_distance_m` | Built. The placement and ranges are untuned. A hand on a control cannot fire the vehicle's weapons with its grip; another fire input while a control is held is not built. |
| Arm steering | `arm_steering.wingsuit` (on), `arm_steering.min_spread`, `arm_steering.deadzone_m`, `arm_steering.full_bank_m`, `arm_steering.full_pitch_m`, `arm_steering.invert_pitch` | Built for the wingsuit. Its rendered arms stay yielded. |
| Parachute handles | `parachute_handles.enabled` (on), `parachute_handles.offset_m`, `parachute_handles.radius_m`, `parachute_handles.full_pull_m`, `parachute_handles.deadzone_m`, `parachute_handles.brake_threshold`, `parachute_handles.release_distance_m` | Built. They replace the parachute's arm steering. The placement and the pull are untuned, and whether the stick-back slow-down the handles brake with is a strong brake depends on shipped tuning that was not read. |
| The wingsuit's head look-steer removal | `movement.suppress_parachute_look_steer`, outside the `controllers` section | Built. |
| Weaponized wingsuit guns aimed by the right hand | none | Not built. They aim by the gaze, because the gun aim covers only the wielded slot. |

### Milestone 5: breadth

The settings are under `weapons` unless named otherwise. The description is in [controllers.md, "Grenades, melee, aim assist, and reloads"](controllers.md#grenades-melee-aim-assist-and-reloads), ["Scopes"](controllers.md#scopes), and ["Sight lines"](controllers.md#sight-lines).

| Part | Settings | State |
|---|---|---|
| The physical grenade throw | `grenade_throw`, and the release, warning, and fallback tuning under `grenade` | Built. |
| Physical melee | `melee`, and the strike tuning under `melee_strike` | Built. |
| Precision aim suppression | `scope.suppress_precision_aim` | Built. |
| Scope lenses | `scopes`, `scope.source` (`OtherEye` by default, or `OwnEye`), `scope.path` (`Resample`; `SceneRender` falls back to it), and the lens tuning under `scope` | The resampled lens is built. The narrow-field scene render is not. |
| Per-hand aim assist | `aim.assist_strength` (0, which is off), `aim.assist_max_distance_m`, `aim.assist_occlusion_tolerance_m` | Built. |
| The magazine gesture | `reload_gesture` (on), and the pouch (0.09 m, at the belt front on the dominant side, off-hand only), well, and timing under `magazine` | Built. The reload button remains. |
| The embedded sight table | `aim.sights` entries override it | Built. Only the `scoped`, `pistol`, and `tuned` grades are used. |
| The sight tuning tool | `aim.sight_tuning.enabled` (off), `aim.sight_tuning.marker_distance_m` | Built. A tuned entry lasts until the payload unloads unless it is copied into the table. |
| Hand-tier promotions | none | Not built beyond the anchored grapple and mounted-gun rows. |

### Tools

`tools/sights/` is a standard-library Python tool that reads the game's archives, derives each hand weapon's sight line from its model, and writes `tools/sights/sights.json`, which the payload embeds ([`tools/sights/README.md`](../../../tools/sights/README.md); the files it reads are in [weapon-data.md](../../engine/gameplay/weapon-data.md)). It regenerates the table without discarding `tuned` entries.

Finger curls on the rendered hands, behind the `fingers` settings, are outside the plan's milestones; [controllers.md, "Finger curls"](controllers.md#finger-curls) describes their state.

### Unverified in milestone 1

The implementation rests on the following assumptions, each of which needs checking in the headset.

Input and scheme:

- The Touch suggested bindings and the press thresholds are untested; the Index layout is the target.
- `GetLastInputInfo` reports keyboard and mouse input under Proton, and XInput does not expose the VR controllers as a gamepad. If XInput does expose them, the scheme flips to the gamepad on controller input.
- The behaviour while the session is not focused.
- A persisted session whose action layout differs from the payload's cannot attach the new actions, so the controllers stay unavailable until the game restarts.
- The character's proportions are an estimate from his stated height until they are measured from the skeleton.

Mode detection and bindings:

- An analog action above 0.2 also reads as pressed, standing in for the device button threshold, which is not read.
- The pause and commlink clicks go to both the player map and the UI map, which may toggle pause twice.
- The wingsuit's evade (A with the move stick flicked out of its rest within `bindings.evade_window_s`) and its L trigger mapping (the air brake). The game's evade is `WINGSUIT_EVADE` held with `MOVE_ALL` tapped, and whether a flick that starts before the A press still reads as that tap is not checked.
- The generic actions (`BTN_*`, `L1`, `R2`, and the others) mirrored alongside each virtual gamepad button.
- The explosive button is the gamepad's RB, so a hold detonates.
- A device poll that the engine skips can stretch a click over two ticks.
- The teleport rule sees only deferred teleports.

The aim-state pin:

- The wall-penetration clear assumes that the weapon's first user entry is the holding character.
- The relaxed branch resets precision aim and the grenade and mortar permissions when it runs, and they are assumed to be restored on the next tick.
- The grapple pin changes the body's behaviour on a held tether: the body no longer turns to face the hook.
- The relaxed carry grip holds the weapon acceptably under the IK arms.
- The fire trigger on an empty dominant hand draws a weapon.
- The strafe act follows the gun's aim point rather than the head (see "The aim-state pin").

Hands:

- The grip offset is a best guess for the Index controller: the wrist at (0.02, 0.075, 0) m in the right grip frame, with the hand rotated 15 degrees about the grip's X axis.
- The character's model space is right-handed with +X to the right. If it is not, the hands render upside down.
- The skinning palette entries use the standard column-major and row-major matrix layouts.
- The hand and finger bones are only in the body's model slot, and the knuckle bone names resolve.
- A deferred draw at the end of the frame may see the next frame's hand snapshot.
- The grapple wire's near end is not late-updated, so it can trail the rendered forearm by up to a tick.

Aiming:

- A gun with neither a config entry nor a trusted table entry aims along its fire point with no model offset, and the gun is aligned with the IK wrist target.
- A grapple target counts as tetherable when it is an object rather than the world.
- The aim filter accepts hits on the player's own vehicle before its ignore check, so a hand ray inside a vehicle can hit the vehicle.

The overlay:

- The producers run on the render thread. The sim-rate readouts expire 150 ms after their last submission, and the per-frame ones after 100 ms.
- The grapple highlight takes its surface normal from the off-hand ray's hit when that hit is within 0.5 m of the target, and otherwise faces the head.
- The laser starts at the sight's origin, pinned to the wrist.
- The ghost hand is a ring and a pointer.
- The colours, sizes, and depth tolerances need tuning.

### Unverified in milestones 2 to 5

The assumptions behind milestones 2 to 5 and the finger curls are in [controllers.md, "Unverified"](controllers.md#unverified).

### Headset test checklist

The Controllers tab's test presets select input-only, hands and aim, weapon handling, roomscale, and vehicle feature groups without replacing calibrated values. Save tuned settings explicitly before uninjecting, and load them after reinjection. Enable diagnostic history before reproducing a problem and capture it immediately afterwards. Test focus and tracking loss with a trigger held: recovery must require neutral controls and a new press, without firing or throwing on its own. Repeat roomscale tests with perpendicular and opposing stick movement, and at locations far from the world origin.

The order puts the parts that everything else depends on first, then the parts whose failure would change the design, then the opt-in breadth. The defaults are the full `Vehicles and advanced` preset, so items 11 onward are already on; only the sight tuning tool (item 12, `aim.sight_tuning.enabled`) and aim assist (item 29, `aim.assist_strength`) need switching on to test them.

1. Input. The Controllers tab reports the OpenXR input status and the hand-tracking status. Both hands track, the grip and aim poses follow the controllers, and every button, trigger, grip, stick, and trackpad reads, including the system button.
2. Scheme. A controller press selects the controller scheme; a gamepad, keyboard, or mouse input selects the gamepad scheme; the `Controllers` and `Gamepad` settings pin it. The gamepad scheme behaves as it did before controllers existed.
3. Mode detection. The detected mode and the active action set follow walking, swimming, falling, the wingsuit, the parachute, each vehicle class in the driver and a passenger seat, the pause menu, the map, the commlink, and the mod's panel. No mode flickers during animation blends, and a fast travel holds the mode.
4. On-foot bindings. Move with the head direction and the off-hand direction, turn smooth and snap, jump, fire, grapple fire, zip, and tether hold, retract, release, use and enter, reload tap and holster hold, and the explosive tap and hold. A short system press pauses, a long one opens the commlink, and neither toggles twice. The menus navigate, confirm, cancel, change tabs, and change pages.
5. Vehicle and air bindings. The car, the motorcycle, the helicopter, the plane, and the boat drive and fire; nitrous holds and the turbo jump taps separate; a held enter reaches the gunner seat. The wingsuit boosts, evades, and brakes, and the parachute opens and steers.
6. Gamepad semantics. The prompts show gamepad buttons under the controller scheme, and the setting returns to its previous value under the gamepad scheme.
7. The aim-state pin. Firing while walking keeps the body in the walk family with no weapon raise and no snap. The aim flags in the readout stay clear while firing. The grapple fires and tethers, and the body's behaviour on a held tether is acceptable. Entering combat draws nothing, leaving combat holsters nothing, and the reload hold still holsters. A gun fired with its barrel near a wall is not blocked. Check the strafe direction with the gun pointed sideways, and whether the trigger on an empty hand draws a gun.
8. Hands. The hands are the right way up with the fingers forward; fit the grip offset. The arms reach the controllers, and the arm-span remap feels right with a `body.local.json`. The gun sits in the hand, fast hand motion shows no lag, the gun and the hand never separate, and the forearm stays attached at the elbow. Ledge grabs, climbs, vehicle entry, swimming, the wingsuit, and ragdoll yield the hands and return them smoothly. The off-hand is not pulled onto the weapon, and the grapple wire leaves the left forearm.
9. Aiming. Shots land where the sights point; use the `Dot` and `Laser` readouts, and check the aiming readout's sight source for each weapon. Aim at a nearby wall. The grapple follows the off-hand ray, and its surface zips, object tethers, and highlight validity match where the hook attaches. Aim a hand ray from inside a vehicle.
10. The overlay. The readouts appear in both eyes at the correct depth and fuse in stereo, with and without single-pass stereo, and are hidden where the scene is in front of them. Nothing is drawn in the far field or in cinematic mode. The ghost hands appear during yielded states, the gizmos match the controllers, and the readouts disappear when a menu opens.
11. Posture. With `body.remap_head` on, each of the `Seated`, `Standing`, and `Auto` postures puts the eyes at Rico's eye height, and ducking lowers the view by the scaled amount. `Auto` selects roomscale only with a play area of at least `roomscale_min_extent_m` each way. In the seated posture the dominant stick flicked down toggles the crouched eye height.
12. Sight lines. Each scoped weapon and pistol aims along its sights with the embedded table; the aiming readout names the source. With `aim.sight_tuning.enabled` on, tune an iron-sighted rifle: line up on the marker, capture with the tab's button and with the chord, adjust with the drag values, reset, and copy the entry as JSON. The chord does not get in the way of holding the foregrip.
13. Holsters. Each hip, shoulder, and chest volume draws its slot, the held weapon holsters at its own holster, and a heavy weapon holsters without dropping. The overlay highlights the volume a hand is in. A grapple fired near the hip does not draw a weapon. The hands stay on the controllers through a reload, a draw, and a holster, and the weapon does not fire during them.
14. The foregrip. The off-hand grabs a two-handed weapon's foregrip, the barrel follows the line between the hands, the off-hand stays on the grip in the rendered image and the sight line, and a second press or moving away releases it.
15. Recoil and haptics. Shots kick the gun up and back and the spring returns it; the foregrip stiffens it; the view does not kick. Fire pulses the firing hand, damage and collisions pulse by side, and the grapple pulses on fire, attachment, and a yank.
16. Dual-wield. Each trigger fires its own pistol along its own hand's ray, semi-automatic pistols fire once per pull on each hand, and the off-hand stick click retracts tethers.
17. The grapple hand. While reeling and hanging, the grapple hand holds the line; while hanging, moving that hand pushes the body. A sharp pull on a live line zips.
18. The chase loop (`roomscale.chase.enabled`, in the roomscale posture). Walking physically moves the body with the head and the view never runs ahead; the stick takes precedence; a wall stops the body while the view stays with the head. A fast travel, a respawn, a load, and a vehicle teleport re-base without a chase spike. F7 re-bases yaw and height only.
19. The head fade (`roomscale.head_fade.enabled`). Putting the head into a wall darkens the view with depth and clears on the way out, and moving the head down past the torso does not trigger it.
20. Crouch (`roomscale.crouch.visual`, then `roomscale.crouch.capsule`). A physical crouch folds the legs with the feet planted; with the capsule, Rico fits under low cover, cannot stand up into a ceiling, and swimming, vehicles, the parachute, and ragdoll keep their own capsules. Check whether enemies react to a lowered head.
21. Seat transitions (`roomscale.seat_transition`). With `Instant` and `Fade`, enter, shuffle seats, and exit each vehicle class; the fades cover the animations and the seated view is re-based. Note any seat state logged as unmatched.
22. Mounted guns. A mounted gun and a gunner seat anchor the hands to the handles, and moving the hands swings the muzzle about the pivot without runaway turning.
23. Vehicle weapons (`vehicles.driver_weapons_by_hand`). The driver's weapons follow the dominant hand, turrets trail it, and the motorcycle and jetski personal weapon aims with the hand.
24. Physical controls (each under `vehicles.physical`). The wheel, handlebars, cyclic, collective, flight stick, and throttle sit where the hands expect them, grab and release, steer in the right direction, and leave the vehicle's weapons unfired.
25. Arm steering and the parachute handles (`vehicles.arm_steering.wingsuit`, `vehicles.parachute_handles`). In the wingsuit, lowering the right hand banks right and lowering both hands dives; the head no longer steers the wingsuit. Under the parachute, the hands steer nothing until they take the handles above the shoulders with the grip; pulling the right handle down banks right, pulling both down slows the canopy, and letting go or raising them flies level.
26. Scopes. Precision aim does not engage from the input, the sniper's bolt cycle, or the guided rocket launcher, and the launcher's missile still guides. The lens appears when the eye is behind the scope, magnifies the target the right way round, and is not blocked by the gun or the hands.
27. Grenades. A grip at the pouch primes a grenade into the hand, the fuse ring counts down, a release throws with the hand's speed and direction, and a grenade held too long explodes in the hand. The stick click still throws along the hand's aim.
28. Melee. A strike away from the shoulder hits the target struck at, a pull toward the body does not strike, and a strike in the air, while reeling, or without the hammer ability does nothing.
29. Aim assist (`aim.assist_strength` above 0). Each hand's aim point pulls toward a hostile target near its sight line, and never toward a target behind cover.
30. The magazine gesture (`weapons.reload_gesture`). Taking a magazine from the off-hand pouch and bringing it to the well starts the reload, a one-tick reload click does not holster, and the button still reloads.

## Reverse-engineering work

Each question is listed with the milestone that needed its answer.

One question is open:

- Milestone 4: the jetski discriminator (`m_VehicleType` and the jetski-specific type), if boats and jetskis need different action sets. Boats and jetskis share the sea class; the physical controls tell them apart by the jetski's steering fork part ([steering-wheel.md](steering-wheel.md#handlebars)).

The narrow-field scene render for the scope lens is not a reverse-engineering question: the resampled lens replaced it, and [controllers.md, "Scopes"](controllers.md#scopes) lists what building it would take.

The milestone 2 to 5 questions are answered:

- Milestone 2: the unfiltered head probe misses the character proxy's capsule but can hit the player's own ragdoll and hit-proxy bodies, so the probe uses the player aim filter ([hands-locomotion-and-state.md §4.2](../../engine/character/hands-locomotion-and-state.md#42-raycast-and-scene-query-api-cphysicssystemcastray-and-related-functions)).
- Milestone 3: the reeled-in input task (`NStateTask_InputReeledInTask`) is not needed. The hang push acts from a post-call hook on [`NStateTask_GrapplingHangTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_GrapplingHangTask##func-Update) and scales from the game's own disabled swing ([controllers.md, "Recoil, haptics, dual-wield, and the grapple hand"](controllers.md#recoil-haptics-dual-wield-and-the-grapple-hand)).
- Milestone 5: the grenade throw runs without the wind-up through the grenade weapon's prime and fire requests, and the launch moves to the hand in a post-call hook on [`Grenade::PropelToward`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::Grenade##func-PropelToward) ([aim-pipeline.md, "Grenades"](../../engine/gameplay/aim-pipeline.md#grenades)).
- Milestone 5: the melee trigger is a click of `PUSH_GRAPPLE` under the game's own conditions ([`Character::CanUseMelee`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-CanUseMelee) and the hammer ability), with the target written into the melee slot after the game's melee scoring ([`PlayerAimControl::SetTargetMeleeFitness`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-SetTargetMeleeFitness)).
- Milestone 5: per-hand aim assist scores the game's targets with [`PlayerAimControl::GetFitnessScore`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-GetFitnessScore) around each hand's sight line, so `CMagnetWeaponComponent::CastSphereAgainstStatic` is not needed ([aim-pipeline.md, "Aim assist"](../../engine/gameplay/aim-pipeline.md#aim-assist-updatealltargets)).
- Milestone 5: precision aim is entered only through [`Player::SetAimMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::player::Player##func-SetAimMode) ([aim-pipeline.md, "Precision aim and sniper zoom"](../../engine/gameplay/aim-pipeline.md#precision-aim-and-sniper-zoom)).
- Milestone 5: a weapon's fire points and model parts are in its entity files ([weapon-data.md](../../engine/gameplay/weapon-data.md)), from which `tools/sights/` derives the sight lines.

The milestone 1 questions are answered:


- [`StateBitFlag::SB_NO_WEAPON_AIM`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_NO_WEAPON_AIM) is read only by the two aim-input functions, its value persists, and the shim still selects the strafe branch with it set (see "The aim-state pin").
- The aim-IK weight key [`ObjectBlackboard::AIM_IK_WEIGHT_ID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/blackboard::ObjectBlackboard::AIM_IK_WEIGHT_ID) is read only by the right-arm and reel-in aim tasks, so skipping the player's aim IK tasks leaves no other reader stale.
- The late update moves the hand bones by the hand's correction and swings the forearm and its twist bones about the elbow. Whether the finger bones are drawn in the same batches as the hand is unverified.
- The fire-point matrices are oriented along the barrel, which is their -Z axis.
- The grapple wire's near end follows the left forearm twist bone, so it follows an IK-driven forearm at the sim rate ([grapple-pipeline.md, "The wire's near end"](../../engine/gameplay/grapple-pipeline.md#the-wires-near-end)).
- The `UpdateMentalState` transitions and the second automatic holster are in [aim-state.md, "The mental state"](../../engine/gameplay/aim-state.md#the-mental-state).

Two questions raised during milestone 1 are answered too. The secondary hand IK pass is suppressed per hand through its one-frame blackboard weights (see "Hands"). The wingsuit's head look-steer is computed by a separate function, [`NAirMovement_UpdateStdLookSteering`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-NAirMovement_UpdateStdLookSteering), which has no anti-tamper branch and no side effects, so a post-call hook that zeroes its output removes the look-steer. The same function serves the falls, so the hook acts only while the mode detector reads the wingsuit or the parachute.

## Risks

1. The pin may not hold cleanly. The animation graph's transition wiring is hashed and was not decoded, so a state that reads the aim bit through a path the recon did not find can still switch the body. The mitigation is to test the pin first, in milestone 1, before other work depends on it.
2. The controller-owned arms work against the authored upper-body animation. Recoil, reload, and traversal animations expect to move the arms, and the hand tiers decide per state which one wins. Tuning the blends is the largest body of iteration in the plan.
3. The late update separates the rendered weapon from the sim-rate muzzle. At high hand speeds the muzzle flash and the shot origin trail the rendered muzzle by up to a tick.
4. Unreliable sights make aiming unreliable. Hitman 3's PC VR release had a drifting offset between the controller and the in-game hand that made its aiming unreliable. The derived-then-tuned sight table is the mitigation, and the tuning tool is the fallback for each weapon. The derived table covers the scoped weapons and the pistols only; most rifles' iron sights need tuning in the headset.
5. The grapple target scoring has to match the game's. If the re-run scoring differs from the native one, entity attachment and tethering behave differently from the flatscreen game.
6. Vehicle classes each need their own playtest. The helicopter, plane, and boat each have their own axis set.

## Deferred with the mod UI

These features wait for a VR-native redesign of the mod's in-VR interface.

- The posture prompt that suggests the seated posture when the head is low.
- The radial weapon menu, the seated and accessibility fallback for holsters.
- The Ceremony seat-transition mode and its confirmation prompt.
- The laser pointer for the floating panel and the game's menus.
