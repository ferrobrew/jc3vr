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
- Hold to hold. A gun, a prop, a grenade, or a control stays in the hand while the grip is held, and letting go drops or docks it ([grip.md](grip.md)).
- The grapple is the off-hand's upper face button (Y): a tap fires, a hold on a target tethers, and with tethers attached a hold with nothing new under the aim tightens them and a double tap releases them. A sharp pull of the left hand while the button is held zips along an attached line. The grips grapple nothing ([grip.md](grip.md)): they take what is within reach.
- Two-handed weapons are held by the dominant hand alone. The off-hand can grab the foregrip, and the line between the two hands then sets the weapon's direction.
- Dual-wielded pistols split onto both hands, each fired by its own trigger and aimed by its own hand.
- Guns show no reticle by default. A dot at the hit point and a laser line are options. The grapple shows a hook-point highlight and a gauntlet beam, each configurable.
- Aim assist is re-derived per hand ray, softened, and defaults to off.
- Driver-seat vehicle weapons aim along the gaze by default, with an option for the right-hand ray. Passenger seats, gunner seats, and mounted guns aim by hand.
- Recoil is an offset between the controller pose and the rendered weapon, driven by the game's per-weapon recoil state, with a spring return, a stiffer spring under a two-hand grip, and matching haptics.
- Haptics and the head-through-geometry fade are in scope. Physical melee is not: the game's melee attack takes the body from the player.
- Vehicles use stick bindings by default. Physical controls (a wheel, handlebars, a helicopter cyclic and collective, a flight stick and throttle) are on by default alongside them, and the stick still steers any axis no hand holds a control for.
- The wingsuit and parachute use stick steering by default. Arm-pose steering for the wingsuit and grabbed handles for the parachute add to it.
- Grenades are thrown physically.
- Weapons are drawn from body holsters. A radial menu is the seated and accessibility fallback, deferred with the rest of the mod UI.
- Reloads are done by hand: eject, load from a belt pouch into the gun's well, and chamber. The game's timed reload stays as a setting of the reload button.
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
4. Yielded. The animation owns the hands, blending out from the controllers and back. With `overlay.ghost_hands` (off by default), a translucent ghost of the controller-driven hands stays visible in the world-space overlay, so that the player keeps sight of their real hands.

The starting assignments are as follows.

| State | Starting tier | Promotion |
|---|---|---|
| On foot, empty-handed or holding weapons | Free | none needed |
| Weapon draw from a holster | Action runs | none needed |
| Grapple fire | Free: the game's fire arm IK is skipped and the hook leaves the gauntlet | none needed |
| Reel-in flight (`GHS_REELING_IN`) | Anchored: the grapple hand holds the line where the controller is | none needed |
| Grapple hang, upside-down, and stunt positions (`GHS_REELED_*`) | Anchored: the grapple hand holds the line, and its displacement pushes the hanging body, because the game's own stick swing is disabled | none needed |
| Standing on a vehicle (stunt position) | Free | none needed |
| Hanging from a helicopter (its stunt position, [`Character::IsStuntHanging`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsStuntHanging)), which wins over the grapple's reeled-in states | Yielded: the hang's animation holds the airframe, where the player's grip holds it ([stunt-positions.md](stunt-positions.md)) | none needed |
| Ledge grabs, climbs, vaults, stunt step-up and step-down | Yielded | physical climbing, which would be a rebuild of the traversal controller |
| Vehicle entry, exit, and hijack | Yielded | the seat transitions in "Roomscale design" |
| Driving and flying with stick bindings | Free: the wheel animates without the hands | none needed |
| Driving and flying with physical controls | Anchored: the hands on the wheel, handlebars, or sticks | none needed |
| Mounted guns and turrets | Anchored: the hands on the handles, which aim the gun | none needed |
| Parachute | Free, or anchored on a handle a hand has taken | the handles: a hand takes the handle above its shoulder with the grip and pulls it down to steer or brake |
| Wingsuit, stick steering | Yielded: the wing membrane is skinned between the arms and the body, and free arms may deform it | arm steering (on by default) reads the controllers; the procedural body carries the player's arms over onto the flight pose's ([body/procedural-states.md](../body/procedural-states.md#the-wingsuit)) |
| Swimming | Yielded, or free while the procedural body poses the torso in the water ([body/procedural-states.md](../body/procedural-states.md#swimming)) | arm-stroke swimming |
| Interactions (sabotage, planting, using objects) | Yielded | per interaction |
| Ragdoll, knockdown, death, and cutscenes | Yielded | none planned |

### Body scale

Rico is about 1.8 to 1.9 m tall. The mapping follows Bonelab's remapping rather than scaling the world. The world stays at true scale, because vehicle sizes, building sizes, and the sense of speed depend on it. The eyes sit at Rico's eye height. Hand offsets from the shoulders scale by the ratio of Rico's arm span to the player's. Vertical head travel (ducking and crouching) scales by the ratio of the two heights. Horizontal roomscale travel stays 1:1. Boneworks keeps motion 1:1 against a fixed body, and players report the mismatch as permanently bent knees or a misplaced floor; the remap avoids that.

The player's dimensions are the `[body]` section of the settings file ([settings.md](../settings.md)), which the calibration on the pause menu's `VR: BODY` page writes: stand straight, then T-pose (see [pause-menu.md](../pause-menu.md#body-calibration)).

### Posture

Two postures are supported.

- Seated: the seated eye reference, the head height the player actually sits at, maps to Rico's standing eye height, the body does not follow the head about the room, and a downward flick of the dominant stick toggles between standing and crouched eye height.
- Standing: STAGE space for the true floor height, physical crouching, and the body following the head about the room ([rig.md](../body/rig.md)).

OpenXR has no signal for whether the user is seated, and SteamVR's OpenXR runtime exposes only a standing basis. The head height above the STAGE floor, compared with the calibrated standing and seated eye heights, indicates a seated player. The posture setting is `auto | seated | standing`. In `auto`, the head height selects seated or standing, seated until the head is located. The play area's size decides nothing ([rig.md, "Posture"](../body/rig.md#posture)). The detection is described in [controllers.md, "Posture and roomscale"](controllers.md#posture-and-roomscale).

## Input

### Action sets

The OpenXR actions come in two layers. The always-active `hands` set holds the grip and aim poses, the haptic output, and the raw inputs, named for the physical inputs of the Valve Index controller; the gestures, grabs, chords, and pointers read them. The semantic sets hold actions named for their game meaning, and the binding tables read those: `global` is always active, and one mode set (`on_foot`, `wingsuit`, `land_vehicle`, `helicopter`, `plane`, or `menu`) is selected by the per-tick mode detector (see "Mode detection"). All sets have the same priority, so an input can drive a raw and a semantic action at once. Per-user remapping is the runtime's own binding interface (SteamVR or xrizer), not a mod UI, and that interface remaps per set and shows the semantic actions by their game meaning. The mod ships suggested bindings for the Index interaction profile and the Touch interaction profile. The layer as built is in [controllers.md, "Input foundation"](controllers.md#input-foundation).

Output is written after `InputDeviceManager::Update`, following the timing rules in [input.md](../../engine/gameplay/input.md). Held inputs are written to the effectors directly rather than through `ForceSetPressed`, because `ForceSetPressed` resets the effector's hold time on every call, so a held input would never register as a hold ([input.md, "The write API"](../../engine/gameplay/input.md#the-write-api)). The mod keeps each held action's hold time itself and writes the effector's value, state, and state time. Menu actions go to the UI action map, which the game's menus read instead of the player's map.

The controller scheme and the gamepad scheme switch on the most recent input. Tracked controllers producing input select the controller scheme. Any gamepad or keyboard input selects the gamepad scheme, which restores gaze aiming, the gamepad bindings, and the animated hands. A setting can pin either scheme.

### Mode detection

The mode detector replaces the headpose sim's binary on-foot test (the `EvaluateCharacterOrientation` call counter), which reads a swimmer as not on foot. It reads the engine's own state signals, documented in [hands-locomotion-and-state.md §5](../../engine/character/hands-locomotion-and-state.md#5-per-tick-character-state-signals): the character state-bitflag words, the state-machine-hash predicates, the vehicle class predicates, the grapple hook state, and the pause and UI signals. It runs once per engine input tick, from the same `InputDeviceManager::Update` detour as the headpose sim: one read of the local character, one snapshot of the three state words, and a `GetVehiclePtr` call only when a vehicle state is indicated, with the pointer cached for the tick.

The first matching rule selects the mode, and with it the action set. The order follows the engine's state overlaps ([§5.6](../../engine/character/hands-locomotion-and-state.md#56-state-overlap-and-precedence)).

1. A menu is up: `ui`. The signals are the game clock's stop flag ([`Clock::m_Stop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_Stop)), which covers the pause menu, the commlink, and the other pause reasons; the UI blocker bit ([`InputSystemBlocker::Ui`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputSystemBlocker##variant-Ui)) of the player action map, which covers game screens such as the map that do not pause; and the mod's own panel.
2. A deferred teleport is in flight ([`GameWorld::IsTeleporting`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-IsTeleporting)), or the character is not readable: hold the current set. The rig re-base on a teleport belongs to the chase loop (see "The chase loop").
3. Seated in a vehicle (`IsInVehicleAttachState`, which spans the entry and exit animations): `vehicle`, with the class resolved in the order helicopter, plane, motorcycle, car, sea, and the driver seat selecting the driver bindings.
4. Grapple traversal (reeling in, pre-reel, hanging, or reeled in): the gameplay mode stays (`onfoot` or `airborne`), and the reel or hang sub-state is flagged for the hand tiers.
5. Wingsuit: `airborne`, wingsuit variant.
6. Parachute, including the vehicle-parachute attachment: `airborne`, parachute variant.
7. Freefall, fall, or fastfall: `airborne`.
8. Swimming: `onfoot`, swim variant.
9. Otherwise: `onfoot`. The headpose sim keeps its own on-foot test (the orientation-evaluation counter) until it is moved onto the detector.

The modes map onto the semantic mode sets: `onfoot`, falling, and the parachute select `on_foot`; the wingsuit selects `wingsuit`; a vehicle selects `helicopter`, `plane`, or `land_vehicle` by class; and `ui` selects `menu`.

A switch between gameplay modes is debounced by a configurable number of ticks (two by default), so a single-tick flicker during an animation blend does not switch the action set. Entering and leaving `ui` is immediate, because the UI signals do not flicker and a late switch would send a menu press to the gameplay bindings. The state-machine-hash predicates take precedence over raw bits during known transitions. A null `m_ControlParameters` or `m_InstanceData` during the first frames after spawn holds the previous mode. Boats and jetskis share one class until the jetski discriminator is pinned.

### Grip semantics

The grip holds what it takes for as long as it is held, everywhere: a gun, a prop, a grenade, a vehicle control, a handle. Letting go drops it with the hand's velocity, or docks it in the socket that fits it under the hand. Of everything a hand is within reach of, the target whose grab point is nearest the hand wins, whatever placed it, and the hand keeps looking while the grip is held, so a hand closed in the air takes what it is then laid on. An empty grip in the air is a fist; a grip on a surface climbs it. Every target is marked in its kind's colour, the one a squeeze would take highlighted. The model and its parts are in [grip.md](grip.md).

### Per-mode bindings

The tables are the default bindings of the semantic actions, laid out for the Valve Index controller, which has A and B on both hands; the player can rebind each action in the runtime's binding interface. In the tables, R and L are the dominant hand and the off-hand, A and B are the dominant hand's face buttons, and X and Y are the off-hand's A and B. The face buttons mirror the gamepad's by side: the gamepad's A and Y (jump, and use or enter) are on the dominant hand, and its X and B (reload, and release tethers) on the off-hand, so a vehicle is exited with the button that entered it. A left-handed layout mirrors every gameplay binding; the menu bindings stay on the physical hands, so that the previous and next tabs and pages keep their sides. The system button of either hand is free on the Index controller and carries pause and the commlink. The Touch profile shares the topology through its own suggested bindings, with its left menu button as the system button, and has no trackpad. The off-hand Index trackpad selects weapon slots, as the gamepad's d-pad does. The tables follow the game's own mode partition and stay congruent with [the default gamepad keymap](../../engine/gameplay/input.md#the-default-gamepad-keymap) where the VR model allows, so that a player's gamepad habits carry over; the action IDs each mapping drives are in [the mapping to action table](../../engine/gameplay/input.md#the-mapping--action-table). Where a controller input stands for a gamepad button, the injection holds every gameplay action that the default gamepad keymap puts on that button, and the game's own context selects the one that applies. The action IDs that serve several mappings (holster as a short hold of `RELOAD`, nitrous and turbo jump as a hold and a tap of `USE_VEHICLE_MOD`, the gunner seat as a held `ENTER_VEHICLE`, and the tether as a held `FIRE_GRAPPLE`) are separated by the game through tap versus hold and through the mode context, so the injection reproduces the hold time of the button it stands for. The system button is the one overload the mod resolves itself. Melee (`PUSH_GRAPPLE` under the hammer ability) is left off the virtual B button, because there it would melee on every tether release. Wherever a mapping is marked as a gesture, the button remains as the fallback.

#### On foot

| Action | VR binding |
|---|---|
| Move | L stick; the magnitude selects walk or run |
| Body turn | R stick, smooth by default, snap as an option |
| Fire the dominant-hand weapon (`FIRE_RIGHT`) | R trigger |
| Fire the left pistol while dual-wielding | L trigger (see "Dual-wield") |
| Fire grapple (`FIRE_GRAPPLE`: fire; tap while attached to zip; hold to tether) | Y; with a line attached, a sharp pull of the left hand while Y is held also zips |
| Retract tethers (`RETRACT_GRAPPLE`) | L trigger; L stick click while dual-wielding, where the L trigger fires the left pistol; with tethers attached, hold Y with nothing new under the aim, and pull the grapple hand back while holding it to tighten harder |
| Release tethers and cancel (`RELEASE_GRAPPLE`, `CANCEL`) | double tap Y with tethers attached |
| Jump, stunt, open parachute | A |
| Use item, enter vehicle (hold: gunner seat), open wingsuit | B; held, it also steps off a ledge the character would stop at (`BYPASS_FALL_PREVENTION`), and while a ragdoll keeps the character one (`CANCEL` held) |
| Reload | X ejects the magazine (the game's reload as a setting); the belt pouch loads |
| Holster | grip press with the weapon hand at the held weapon's holster; a short hold of X also holsters, as on the gamepad |
| Weapon slot select | draw from a holster; press the L trackpad up, right, left, or down for the pistols, the two-handed, the special, or the explosives slot (Index); the L stick click cycles `NEXT_WEAPON` as a fallback (on by default; not while dual-wielding) |
| Throw grenade | grab from the grenade pouch and throw; `throw_grenade` is an action with no default binding |
| Throw C4 | grab from the C4 pouch on the chest and throw; it sticks where it lands; no button |
| Detonate C4 | touch the button on the inside of the left wrist with the right index fingertip; or hold the R stick click (`DETONATE_EXPLOSIVE` alone) |
| Melee (`PUSH_GRAPPLE` under the hammer ability) | none |
| Precision aim and sniper zoom | removed; scopes magnify optically (see "Scopes") |
| Reel-in context: cancel, boost, hang jump, slingshot | double tap Y, hold L trigger (hold L stick click while dual-wielding), A, A |
| Pause, commlink | System button tap, System button hold (either hand) |
| Skip a cutscene | hold the right B, as the game's prompt asks (every mode but the menus, either handedness) |
| Crouch toggle (seated posture) | R stick flicked down |

#### Land vehicles (car; motorcycle in the last rows)

| Action | VR binding |
|---|---|
| Steer | L stick; wheel or handlebars |
| Accelerate and reverse | R trigger and L trigger |
| Handbrake | X |
| Nitrous and turbo jump (`USE_VEHICLE_MOD`, upgrade-gated) | hold and tap R stick click; the cockpit's nitrous button |
| Fire vehicle weapons, primary and secondary | R grip and L grip, when the grip takes nothing |
| Grapple | Y, with its gestures as on foot; its double tap releases a tether fired from the vehicle (`VEHICLE_RELEASE_GRAPPLE`, with `RELEASE_GRAPPLE`) |
| Enter gunner seat, roof stunt | hold B, A |
| Exit vehicle | B |
| Horn | L stick click |
| Look back, vehicle camera, recenter camera | removed; the neck replaces them, and F7 recenters |
| Motorcycle personal weapon (`MC_FIRE`) | R grip, aimed by the right hand |
| Motorcycle lean and tilt | L stick; handlebar lean |
| Motorcycle throttle | R trigger; twisting the handlebars' right grip back |

#### Helicopter

| Action | VR binding |
|---|---|
| Collective up and down | R trigger and L trigger; collective lever |
| Cyclic (forward, back, and roll) | L stick; cyclic stick |
| Yaw | R stick X |
| Fire primary and secondary | R grip and L grip, aimed by the gaze, when the grip takes nothing |
| Grapple | Y |
| Exit, stunt, nitrous | B, A, hold R stick click |

#### Plane

| Action | VR binding |
|---|---|
| Pitch and roll | L stick; flight stick |
| Rudder | R stick X |
| Thrust up and down | R trigger and L trigger; throttle |
| Fire primary and secondary | R grip and L grip, aimed by the gaze, when the grip takes nothing |
| Grapple | Y |
| Exit, stunt | B, A |

#### Boats and jetskis

| Action | VR binding |
|---|---|
| Steer | L stick; wheel or handlebars |
| Accelerate and reverse | R trigger and L trigger |
| Fire, and the jetski personal weapon | R grip, aimed by the gaze (boat) or the right hand (jetski) |
| Nitrous and turbo jump | hold and tap R stick click |
| Exit | B |

#### Mech

The mech is its own vehicle class ([`Character::IsAttachedToMech`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToMech)), on the land vehicle set's actions at the gamepad's places. It has no physical controls.

| Action | VR binding |
|---|---|
| Walk and turn (the plane's pitch and the car's turn) | L stick |
| Right-hand weapon (`MECH_FIRE_RIGHT_HAND_WEAPON`) | R trigger |
| Gravity gun: attract while held, throw on release (`MECH_FIRE_GRAVITY_WEAPON_PRIMARY`) | L trigger |
| Gravity gun: drop (`MECH_FIRE_GRAVITY_WEAPON_SECONDARY`) | R grip, when it takes nothing |
| Punch (`MECH_PUNCH`) | L grip, when it takes nothing |
| Jump (`MECH_JUMP`) | A |
| Exit, grapple | B, Y |

#### Wingsuit and parachute

| Action | VR binding |
|---|---|
| Steer (analog `MOVE_*`) | L stick; arm-pose steering |
| Air brake (`CANCEL` hold, upgrade-gated; `WINGSUIT_AIRBRAKE` when weaponized) | L trigger |
| Boost (weaponized) | R stick click |
| Fire weapons (weaponized wingsuit or parachute) | R trigger and R grip, aimed by the right hand |
| Open the parachute | A |
| Close the wingsuit | B, the button that opened it |
| Release tethers | double tap Y with tethers attached |
| Grapple (slingshot boost) | Y |

#### UI

While a game menu is open, the `ui` action set replaces the gameplay bindings, matching the game's own `END_UI_MAPPINGS` boundary. Both hands navigate the menus alike, with each hand's grip as its modifier; the layout as built is in [controllers.md, "Bindings and injection"](controllers.md#bindings-and-injection). A laser pointer on the floating panel is deferred with the rest of the mod UI. `payload/src/hud/pointer.rs` already isolates the pointer source for it.

## Aiming

### Guns

The dominant hand's ray reaches each held weapon's `m_AimTargetPosition` through the per-weapon aim update: the base update (`CWeaponBase::UpdateFromUserPostSim_Parallel` through its base) is handed the hand's aim point in place of the character's shared one, and the weapon stores it before it builds the shot. The ray is the weapon's sight line, not the controller's own axis: the gun's transform carried to the controller pose, times the gun's current fire point, times a per-weapon sight correction. The target point is the ray's hit point, found with `CPhysicsSystem::CastRay` and the player aim's own ray filter from the sim phase, and it is kept well beyond the near-muzzle distance at which `GetShotMatrix` falls back to the barrel direction. The per-weapon field is the seam because it is the only one that gives two weapons two directions: the character-level aim point (`m_AimTargetPositionWeapons`) fans out unchanged to every held weapon. The character-level write still runs every frame, with the dominant hand's aim point, because a frame without it sets no `WasSetThisFrame` flag and the game then aims every weapon 25 m ahead of the body ([aim-pipeline.md, "From aim control to the character"](../../engine/gameplay/aim-pipeline.md#from-aim-control-to-the-character-cplayerupdatepostcamera)).

The per-weapon sight offsets are derived and then tuned. The fire-point matrices (`m_InternalFirePositions`) give the muzzle origin and the barrel axis: `GetShotMatrix` treats the composed fire-point matrix's -Z axis as the barrel. An offline tool, `tools/sights/`, reads the weapon models from the game archives, derives the sight line from the model geometry above the fire point, and emits `tools/sights/sights.json`, keyed by the weapon name hash, with a confidence grade per weapon ([weapon-data.md](../../engine/gameplay/weapon-data.md) has the files it reads). Scoped sights and pistols derive reliably; iron sights on rifles with rails, muzzle devices, or carry handles do not, and are graded uncertain. The payload embeds the table and uses only its trusted grades; a per-weapon config entry overrides it, and a weapon with neither aims along its fire point. A tuning tool in the debug UI writes the config entry: the player lines up the sights on a marker and captures, and the entry can be copied into the table ([controllers.md, "Sight lines"](controllers.md#sight-lines)).

Guns show no reticle by default. The options are a dot at the hit point, at the hit point's true depth so that it fuses in stereo, and a laser line from the muzzle to the hit point. Both are drawn by the world-space overlay layer. The game's own weapon reticle, which marks the gaze's aim point, is hidden wherever the guns aim by hand (on foot whether or not a gun is out, and in a vehicle while its weapons aim by hand), and its grapple reticle everywhere under the controller scheme while the grapple aims by hand (`overlay.hide_game_reticle`, on). With the grapple's hand aim off (`aim.grapple`), the gaze aims the grapple and its reticle stays.

### The grapple

The grapple slot, index [`TargetType::TARGET_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::target::TargetType##variant-TARGET_GRAPPLE), of the player aim control's slot arrays ([`m_AimPos`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_AimPos), [`m_DirectPositions`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_DirectPositions), [`m_DirectTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_DirectTargets), [`m_DirectHits`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_DirectHits), [`m_IsInRange`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_IsInRange), and [`m_Target`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_Target)) is filled from the left controller's ray by the game's own targeting. Every part of the aim control that fills the slot takes its ray from the adjusted camera matrix or the camera matrix getter. While a thread-local override is set, both getters return a camera matrix built from the grapple ray. [`PlayerAimControl::UpdateDirectAim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-UpdateDirectAim) runs twice: once under the override, from which the grapple slot is kept, and once without it, which refills every other slot from the camera. The grapple target scoring, its visibility casts, and the re-validation of the last-valid grapple cache also run under the override, so [`m_LastHitGrappleTargetGO`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##field-m_LastHitGrappleTargetGO) and the grapple slot of `m_Target` are filled the way the game would fill them, and entity attachment and tethering follow the left hand. Every grapple consumer reads the grapple slot, so the hook fire, the tether semantics, and `IsLastValidGrappleAimPosStillValid` follow the left hand without further changes. Three alternatives are rejected. A post-hook on `UpdateDirectAim` that casts its own ray and writes each slot array by hand would have to reproduce the game's filter, range, holey-geometry handling, and target resolution. Overwriting only the latched target at fire time (`CGrapplingHook::SetUpTarget`) corrects the hook but leaves every reader of the live slot on the gaze ray. Telling the grapple's `GetCameraMatrix` calls apart by return address does not work, because the one raycast inside `UpdateDirectAim` serves the weapon and grapple slots together.

The game's grapple reticle pins to the centre of the screen when the aim uses a direct position, so it cannot show a hand ray, and it is hidden while the grapple aims by hand. The grapple readout is drawn by the world-space overlay layer instead: a highlight on the surface or object the grapple would attach to, shown only when the target is valid and in range, with a distinct style for targets that support tethering, and a gauntlet beam from the wrist to the target point. Each is configurable.

The yank gesture is a sharp pull of the left hand back toward the body while a line is attached and the grapple button is held. It issues the same `FIRE_GRAPPLE` tap that zips; a pull with the button up does nothing, so a hand moved back in the course of play never zips. The physical grapple builds on the pull: it speeds the reel, boosts it on a yank during the reel, tightens attached tethers, and pumps the hang swing ([physical-grapple.md](physical-grapple.md)).

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

A two-handed weapon rides the dominant hand's attach bone and aims by that hand alone. The foregrip is a grab volume for the off-hand. While the off-hand holds the foregrip, the weapon's direction is the line from the dominant hand to the off-hand, the off-hand's wrist is IK-pinned to the foregrip, and the recoil spring stiffens. The off-hand holds the foregrip while its grip is held and releases it by opening the grip (or, with the grip a toggle, by a second press), or by moving well away from it. This is the one hold of the grip: a foregrip is held the way a real one is, and letting go of it is the natural release. The engine's own secondary-hand grip IK (`UpdateSecondaryHandIKPass`) is suppressed for the off-hand, so that the engine does not pull the hand onto the foregrip on its own.

### Reloads

The game's reloads of the hand weapons are stopped, and the player reloads by hand: the reload button or a flick of the gun ejects the magazine, the free hand takes a round from a universal belt pouch and pushes it into the gun's real well, and a gun that ran dry is chambered by working its slide, charging handle, bolt, or pump; the bolt and the pump are worked after every shot. Pushing the gun's well onto the pouch reloads it in one hand. As an accessibility fallback the reload button can be left to the game's timed reload. The design is in [reload.md](reload.md).

### Grenades

A grip press on the grenade pouch puts a grenade in the hand; releasing the grip throws it. The throw animation normally drives the grenade weapon through two requests, and the mod raises them itself, so no wind-up plays ([aim-pipeline.md, "Grenades"](../../engine/gameplay/aim-pipeline.md#grenades)): the grip press raises the prime request ([`ThrownGrenadeWeapon::m_PrimeGrenade`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::ThrownGrenadeWeapon##field-m_PrimeGrenade)), and the release raises the fire request once the grenade has its physics body. A post-hook on [`Grenade::PropelToward`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::Grenade##func-PropelToward) moves the launch to the hand and overwrites the solved launch velocity, [`Explosive::m_DeferredInitialVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::Explosive##field-m_DeferredInitialVelocity), with the hand's release velocity, estimated from a short history of the hand's motion. The fuse runs from the prime, so a grenade held too long explodes in the hand; a ring at the hand counts the fuse down. No button throws a grenade.

### C4

A grip press on the C4 pouch on the chest, or choosing the explosives on the weapon wheel, puts a charge in the hand; opening the fingers throws it, and it sticks where it lands. Pressed onto a surface, it sticks there, and a red ring marks where it would. The game's planting mode (select the explosives, aim a ghost with the camera, and plant at the aimed surface with an animation) is bypassed: the mod drives the C4 weapon's own prime and make-live steps and drops the charge from the hand the way the game drops one under the parachute ([planted-explosives.md](../../engine/gameplay/planted-explosives.md)). The game's cap on placed charges and its planting permission still apply. Touching a button on the inside of the left wrist with the right index fingertip detonates the placed charges through the game's own detonate input, as does holding the R stick click. The button shows a pip for each placed charge, and each step (a charge primed, thrown, or refused, a press, a press with nothing placed, and the detonation) has its own pulse.

### Melee

There is no melee gesture. The game's melee attack lunges the body toward its target and plays its own attack, which takes the body from the player and runs counter to hands that act on the world physically. The game's melee input is on no controller binding, and an empty hand's palm passes through characters ([physical-hands.md](physical-hands.md)). A physically simulated hand may bring hitting back later.

### Scopes

Precision aim adds a camera-tree FOV hint, shows a 2D sniper overlay, and hides the player's body, which also hides the hands. The mod suppresses all three. The suppression is a refusal at [`Player::SetAimMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::player::Player##func-SetAimMode), through which the precision-aim input, the sniper rifle's bolt-action cycle, and the guided rocket launcher's shot all enter precision aim. A scoped weapon instead shows a magnified image in a lens on the scope while the scope is near and in front of an eye. The lens resamples the other eye's rendered image at the aim point's distance, because a crop of the sighting eye's own image would magnify the scope's model. A narrow-FOV third scene render, drawn only while the scope is at the eye, in mono, and at reduced resolution, would show true parallax; it is not built, and [controllers.md, "Scopes"](controllers.md#scopes) lists what it needs.

## The world-space overlay layer

A world-space overlay layer draws sprites and lines into the scene with depth, per eye. It carries the grapple highlight and beam, the optional gun dot and laser, the grip targets' markers, the ghost hands of the yielded tier, and debug controller gizmos. The floating HUD panel cannot do this, because it draws a 2D texture on one plane, and the per-hand readouts need their own depths. The layer draws after post-processing and before the HUD, and tests against the scene depth in its pixel shader ([controllers.md, "The world-space overlay"](controllers.md#the-world-space-overlay)).

## Roomscale design

The design for physical locomotion is resolved against the engine answers in [hands-locomotion-and-state.md §4](../../engine/character/hands-locomotion-and-state.md#4-proxy-input-scene-queries-teleports-seat-pose-root-motion-capsule-and-pause). It follows the topology in [head-and-body.md](../body/head-and-body.md): the head is the ground truth and the body follows it. The camera is never derived from the body's motion, so the view stays 1:1 with the real head regardless of what the body does. The body follows the head in the standing posture.

### The body following the head

Who moves the root on each simulation tick, the chase after the ground point under the player's upright eyes, the lean it leaves alone, the cockpit shift and the ledger that keep the camera on the real head, teleports, seat exits, and recentering are in [rig.md](../body/rig.md). Physical translation never rotates the body, consistent with the rule that orientation changes only with a movement action.

### Crouch, in stages

1. The body: the procedural body's hips follow the head down and the legs fold under them ([procedural-body.md](../body/procedural-body.md#crouching-and-seated-players)).
2. Capsule height: the engine swaps the proxy capsule per state itself (`SetProxyState`, which calls `hknpWorld::setBodyShape`), and every character has a crouching proxy state with a shortened capsule ([hands-locomotion-and-state.md §4.6a](../../engine/character/hands-locomotion-and-state.md#46a-capsule-dimensions-and-runtime-resize)). The mod requests that state through the character's pending proxy state ([`Character::m_PendingProxyState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_PendingProxyState)), which the engine applies on the sim thread. The request is made only while the pending state is the default or already the crouch, so the engine's own proxy states for swimming, the parachute, vehicles, and ragdoll win, and it is released only when a ray up from the feet finds the headroom to stand.
3. AI response: whether enemy perception responds to a lowered head is measured in the headset.

In the seated posture, the crouch toggle moves the eye height between the standing and crouched presets, and the body folds under it the same way.

### Seat transitions

JC3 vehicles are entered through animations in which Rico crosses metres of world, opens doors, and hijacks drivers. Under a head-anchored camera the entry is a forced camera movement that ends at an arbitrary yaw, which is a strong vection source. Bonelab avoids the problem (its vehicles are mount-in-place with trigger-volume seats and no entry animation), and LukeRoss's R.E.A.L. GTA V mod, a seated camera-only port, advises players to accept the ride or close their eyes. The mode enum below shares one seat re-base mechanism: the seated head reference is the character's own head and eye bones, which ride the vehicle through `m_AttachBone` and `m_AttachOffset`. The game's vehicle predicates do not separate an entry, a locked seat, and an exit, so the seat phase is classified from the body rule system's current state by name.

- Instant: at seat lock the cockpit re-bases immediately and the world stays live. The player's real posture is tolerated, and the player can recenter with F7. The entry animation is hidden by a fade that clears at the lock; the exit plays live and re-bases yaw and height once the character is out.
- Fade: fade out when the entry animation takes the camera, and fade in seated once the re-based view has settled (`roomscale.seat_fade_hold_s`). The exit also plays behind a fade.
- Ride: the head stays anchored through the full animation with nothing hidden. It is the default until the other modes are proven in the headset, because it is the mod's existing behaviour, and it stays as the intense option.
- Ceremony, deferred with the rest of the mod UI because it needs a confirmation prompt: the entry animation plays; at seat lock the game clock freezes and the floating panel asks the player to take their seat and confirm; on confirmation the mod re-bases, unfreezes, and enables the `vehicle` action set. The freeze is a mod-side clock freeze through the hooked `CClock::Update` (zero game dt, skipped sim tick), never the game's own pause, because `UpdateRenderPaused` does not run `CameraTree::UpdateRenderContexts`, and a real pause freezes the camera under head motion. Exit reverses the sequence. Whether the frozen world dims is decided in the headset.

The chase is disengaged while seated in every mode.

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
- The body following the head ([rig.md](../body/rig.md)), the teleport re-base, and the head-through-geometry fade.
- The crouching body, then the capsule swap.
- The Instant, Fade, and Ride seat-transition modes.

### 3. Holding

- Holsters and pouches, with the grip targets' markers.
- The two-handed foregrip.
- Recoil and haptics.
- The dual-wield split.
- The action-runs tier for reloads and draws, and the anchored tier for grapple lines and hangs.
- The yank gesture.

### 4. Vehicles and flight

- Per-class vehicle bindings, playtested class by class.
- Vehicle weapon aiming (the gaze by default, with the hand option), and hand-aimed mounted guns and turrets.
- The physical controls: wheel, handlebars, cyclic and collective, flight stick, and throttle.
- The arm steering for the wingsuit, the handles for the parachute, and the removal of the head look-steer from the wingsuit's steering.

### 5. Breadth

- The physical grenade throw.
- Scope lenses.
- Per-hand aim assist.
- The true reload.
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
| Body dimensions and remap | the settings file's `[body]` section; `body.remap_hands` (on), `body.remap_head` (on) | Built. The head remap feeds the view through the posture (milestone 2). |
| Mode detection | `mode.debounce_ticks` | Built. The headpose sim keeps its own on-foot test. |
| Bindings and injection | `bindings.enabled`, `bindings.move_deadzone`, `bindings.trigger_deadzone`, `bindings.long_press_s`, `dominant_hand`, `locomotion.direction` (`Head` or `OffHand`) | Built for every mode. An off-hand grip press inside a grab volume (milestone 3) grabs, and elsewhere fires the grapple. Falling and under the parachute the dominant hand keeps the on-foot fire, reload, and grenade bindings. The wingsuit's evade is unbound. |
| Gamepad semantics | `gamepad_semantics` | Built. |
| Aim-state pin and mental-state cuts | `aim_state.pin`, `aim_state.pin_grapple`, `aim_state.skip_arm_ik`, `aim_state.neutral_aim_point`, `aim_state.neutral_aim_distance_m`, `aim_state.clear_wall_block`, `aim_state.mental_state_cuts`, and `aim_state.without_controllers` (off; applies the other parts under the gamepad scheme for testing) | Built, except the strafe reference correction (see "The aim-state pin"). |
| Hands | `hands.ik`, `hands.late_update`, `hands.suppress_secondary_ik`, `hands.reach_t`, `hands.reach_r`, `hands.pull`, `hands.resist`, `hands.grip`, `hands.yield_blend_s`, `hands.return_blend_s`, `hands.tiers` | The free and yielded tiers are built; milestones 3 and 4 add the action-runs and anchored tiers. A state assigned a tier that is not built for it yields. A left-handed layout mirrors the bindings but does not move a single gun to the left hand: the gun stays on the attach bone the game gives it, and the pouches and the wrist detonator are laid out around whichever hand holds it rather than the dominant hand. Swapping a single gun's attach bone to the dominant hand, as the dual-wield layout already does for the pistols, is a future item; it needs the two-handed weapons' foregrip geometry and the sight corrections checked for the mirrored hold. The grapple wire's render-time correction and elbow pole hints are not built. |
| Gun aiming | `aim.gun`, `aim.max_distance_m`, `aim.min_target_distance_m`, `aim.sights` | Built. The sight line starts from the fire point, corrected by the config entry or the embedded table (milestone 5). |
| Grapple aiming | `aim.grapple`, `aim.grapple_origin_m` | Built. |
| World-space overlay | `overlay.enabled`, `overlay.gun_readout` (`None` by default, `Dot`, or `Laser`), `overlay.grapple_highlight`, `overlay.grapple_beam`, `overlay.hide_game_reticle`, `overlay.markers` (on), `overlay.ghost_hands`, `overlay.debug_gizmos`, and `overlay.body_mirror` with `overlay.body_mirror_distance_m` (the last three debugging aids, off), `overlay.depth_tolerance_m`, `overlay.depth_tolerance_rel`, `overlay.min_line_px`, `overlay.xray_alpha` | Built. |

### Milestone 2: posture and roomscale

The settings are under `roomscale`, except the seated crouch flick (`bindings.crouch_flick`) and the head remap (`body.remap_head`). The description is in [controllers.md, "Posture and roomscale"](controllers.md#posture-and-roomscale).

| Part | Settings | State |
|---|---|---|
| Posture and STAGE space | `posture` (`Auto`, `Seated`, or `Standing`) | Built. `Auto` detects seated from the head height (`auto_posture`). |
| Head height remap | `body.remap_head` (on) | Built. |
| Seated crouch toggle | `bindings.crouch_flick`, `crouch.seated_drop_m` | Built. It works with or without the head remap. |
| The body following the head and the teleport re-base | `body.rig.chase` (on), `body.rig.lean` (on) | Built ([rig.md](../body/rig.md)). |
| The head-through-geometry fade | `head_fade.enabled` (on), `head_fade.start_depth_m`, `head_fade.full_depth_m`, `fade.out_s`, `fade.in_s` | Built. The fade darkens the image in the VR blit. |
| The crouch capsule | `crouch.capsule` (on), `crouch.enter_drop_m`, `crouch.exit_drop_m` | Built. The AI response to a lowered head is not measured. |
| Seat transitions | `seat_transition` (`Ride` by default, `Instant`, or `Fade`), `seat_fade_hold_s` | Built. Ceremony is deferred with the mod UI. |

### Milestone 3: holding

The settings are under `weapons` unless named otherwise. The description is in [controllers.md, "Holsters, pouches, the foregrip, and the action-runs tier"](controllers.md#holsters-pouches-the-foregrip-and-the-action-runs-tier), with recoil, haptics, dual-wield, the anchored tier, and the yank in ["Recoil, haptics, dual-wield, and the grapple hand"](controllers.md#recoil-haptics-dual-wield-and-the-grapple-hand).

| Part | Settings | State |
|---|---|---|
| Holsters and pouches | `holsters.enabled`, `holsters.hip` (radius 0.12 m), `holsters.shoulder` (0.17 m), `holsters.chest` (0.09 m), `holsters.stick_click_cycles` (on); the pouches under `pouches`: `grenade` and `c4` (0.075 m, on either side of the chest, for either hand with no gun drawn and the free hand otherwise; `grenade_side` picks the grenades' side by the gun hand), `magazine` (0.075 m, at the front of the free hand's hip, for the free hand), and `reach_margin_m` (0.05 m) | Built, with a seated placement keyed on the roomscale posture (`roomscale.posture`). The volume offsets and radii are untuned. |
| The two-handed foregrip | `foregrip.enabled`, `foregrip.hold`, `foregrip.radius_m`, `foregrip.release_distance_m`, `foregrip.blend_s`, `foregrip.fallback_distance_m`, `foregrip.min_span_m` | Built. |
| The action-runs tier | `block_fire_during_actions` | Built for reloads, draws, and holsters. |
| Recoil | `recoil.enabled`, `recoil.suppress_camera_kick`, and the kick and spring tuning under `recoil` | Built. |
| Haptics | `haptics.enabled`, `haptics.intensity`, `haptics.categories`, `haptics.game_rumble` ([haptics.md](haptics.md)) | Built. |
| The dual-wield split | `dual_wield.split`, `dual_wield.place_by_handedness`, `dual_wield.independent_draw` | Built. Independent draws are untested in the headset. |
| The anchored tier for the grapple line and the hang push | `hands.anchor.enabled`, `hands.anchor.hang_swing`, and the swing tuning under `hands.anchor` | Built for the reel and hang states. A grapple hand that holds a gun stays free. |
| The yank gesture | `bindings.yank.enabled` | Built. It is disarmed while the line carries the character (the reel-in and the hang). |
| F7 during the chase | none | Re-bases yaw and height only while the chase is active ([rig.md, "Recentering"](../body/rig.md#recentering)). |

### Milestone 4: vehicles and flight

The settings are under `vehicles`. The description is in [controllers.md, "Vehicles, mounted guns, and flight"](controllers.md#vehicles-mounted-guns-and-flight).

| Part | Settings | State |
|---|---|---|
| Per-class vehicle bindings | the milestone 1 bindings | Built in milestone 1; each class needs its own playtest. Boats and jetskis share one class. |
| Driver weapons by hand | `driver_weapons_by_hand` (on) | Built. The dominant hand aims the driver's weapons by default. |
| Mounted guns and gunner seats | `mounted.aim_by_hands`, `mounted.anchor_hands`, `mounted.min_lever_m` | Built. |
| Physical controls | `physical.wheel` (with its lock, response, deadzone, tracking, and return), `physical.handlebars`, `physical.cyclic`, `physical.collective`, `physical.flight_stick`, and `physical.throttle` (each on), `physical.grab_radius_m`, `physical.release_distance_m` | Built. The placement and ranges are untuned. A hand on a control cannot fire the vehicle's weapons with its grip; another fire input while a control is held is not built. |
| Arm steering | `arm_steering.wingsuit` (on), `arm_steering.min_spread`, `arm_steering.deadzone_m`, `arm_steering.full_bank_m`, `arm_steering.full_pitch_m`, `arm_steering.invert_pitch`, `arm_steering.show_guide` (on) | Built for the wingsuit. The hand tier stays yielded; Rico's arms follow the player's through the procedural body (`body.procedural.wingsuit_arms`). |
| Parachute handles | `parachute_handles.enabled` (on), `parachute_handles.offset_m`, `parachute_handles.radius_m`, `parachute_handles.full_pull_m`, `parachute_handles.deadzone_m`, `parachute_handles.brake_threshold`, `parachute_handles.let_go_distance_m` | Built. They replace the parachute's arm steering. The placement and the pull are untuned, and whether the stick-back slow-down the handles brake with is a strong brake depends on shipped tuning that was not read. |
| The wingsuit's head look-steer removal | `movement.suppress_parachute_look_steer`, outside the `controllers` section | Built. |
| Weaponized wingsuit guns aimed by the right hand | none | Not built. They aim by the gaze, because the gun aim covers only the wielded slot. |

### Milestone 5: breadth

The settings are under `weapons` unless named otherwise. The description is in [controllers.md, "Grenades, aim assist, and reloads"](controllers.md#grenades-aim-assist-and-reloads), ["Scopes"](controllers.md#scopes), and ["Sight lines"](controllers.md#sight-lines).

| Part | Settings | State |
|---|---|---|
| The physical grenade throw | `grenade_throw`, and the release, warning, and fallback tuning under `grenade` | Built. No button throws a grenade. |
| The C4 throw and the wrist detonator | `c4_throw` and `wrist_detonator` (both on), and the pouch, release, and button tuning under `c4` | Built. The R stick click detonates. |
| Precision aim suppression | `scope.suppress_precision_aim` | Built. |
| Scope lenses | `scopes`, `scope.source` (`OtherEye` by default, or `OwnEye`), `scope.path` (`Resample`; `SceneRender` falls back to it), and the lens tuning under `scope` | The resampled lens is built. The narrow-field scene render is not. |
| Per-hand aim assist | `aim.assist_strength` (0, which is off), `aim.assist_max_distance_m`, `aim.assist_occlusion_tolerance_m` | Built. |
| The true reload | `reload` (on): the button, the flick, the pouch (`pouches.magazine`), the well, the belt reload, the chamber, the manual action, and the dual split | Built, not run in the headset. See [reload.md](reload.md). |
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
- The wingsuit's L trigger mapping (the air brake).
- The generic actions (`BTN_*`, `L1`, `R2`, and the others) mirrored alongside each virtual gamepad button.
- The R stick click carries only `DETONATE_EXPLOSIVE`, which the gamepad rules act on once it is held past the hold threshold, as the gamepad's RB hold detonates.
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
5. Vehicle and air bindings. The car, the motorcycle, the helicopter, the plane, and the boat drive and fire; nitrous holds and the turbo jump taps separate; a held enter reaches the gunner seat. The wingsuit boosts and brakes, and the parachute opens and steers.
6. Gamepad semantics. The prompts show gamepad buttons under the controller scheme, and the setting returns to its previous value under the gamepad scheme.
7. The aim-state pin. Firing while walking keeps the body in the walk family with no weapon raise and no snap. The aim flags in the readout stay clear while firing. The grapple fires and tethers, and the body's behaviour on a held tether is acceptable. Entering combat draws nothing, leaving combat holsters nothing, and the reload hold still holsters. A gun fired with its barrel near a wall is not blocked. Check the strafe direction with the gun pointed sideways, and whether the trigger on an empty hand draws a gun.
8. Hands. The hands are the right way up with the fingers forward; fit the grip offset. The arms reach the controllers, and the arm-span remap feels right with calibrated body dimensions. The gun sits in the hand, fast hand motion shows no lag, the gun and the hand never separate, and the forearm stays attached at the elbow. Ledge grabs, climbs, vehicle entry, swimming, the wingsuit, and ragdoll yield the hands and return them smoothly. The off-hand is not pulled onto the weapon, and the grapple wire leaves the left forearm.
9. Aiming. Shots land where the sights point; use the `Dot` and `Laser` readouts, and check the aiming readout's sight source for each weapon. Aim at a nearby wall. The grapple follows the off-hand ray, and its surface zips, object tethers, and highlight validity match where the hook attaches. Aim a hand ray from inside a vehicle.
10. The overlay. The readouts appear in both eyes at the correct depth and fuse in stereo, with and without single-pass stereo, and are hidden where the scene is in front of them. Nothing is drawn in the far field or in cinematic mode. With the debugging aids on, the ghost hands appear during yielded states and the gizmos match the controllers; and the readouts disappear when a menu opens.
11. Posture. With `body.remap_head` on, each of the `Seated`, `Standing`, and `Auto` postures puts the eyes at Rico's eye height, and ducking lowers the view by the scaled amount. `Auto` starts seated when the player sits and standing when they stand, switches to standing within a second of standing up and to seated after 20 s of sitting down. Seated, sitting normally puts the view on Rico's eyes on foot and in a vehicle seat after a recenter (F7, the pause menu's `RECENTER VIEW`, or the stick click chord) at once, or within about a minute without one; the readout's `camera_minus_rico_m` stays near zero and `seated_reference_source` names the source. In the seated posture the dominant stick flicked down toggles the crouched eye height, and the toggle does not lower the view in a vehicle seat.
12. Sight lines. Each scoped weapon and pistol aims along its sights with the embedded table; the aiming readout names the source. With `aim.sight_tuning.enabled` on, tune an iron-sighted rifle: line up on the marker, capture with the tab's button and with the chord, adjust with the drag values, reset, and copy the entry as JSON. The chord does not get in the way of holding the foregrip.
13. Holsters. Each hip, shoulder, and chest volume draws its slot, the held weapon holsters at its own holster, and a heavy weapon holsters without dropping. The overlay highlights the volume a hand is in. A grapple fired near the hip does not draw a weapon. The hands stay on the controllers through a reload, a draw, and a holster, and the weapon does not fire during them.
14. The foregrip. The off-hand grabs a two-handed weapon's foregrip, the barrel follows the line between the hands, the off-hand stays on the grip in the rendered image and the sight line, and a second press or moving away releases it.
15. Recoil and haptics. Shots kick the gun up and back and the spring returns it; the foregrip stiffens it; the view does not kick. Fire pulses the firing hand; damage, collisions, and a vehicle's engine and road rumble pulse continuously by side (ride in a car and a boat and feel the engine on both hands); the grapple pulses on fire, attachment, and a yank. With a physical gamepad connected, it stays quiet while the controllers vibrate, unless `gamepad_rumble` is on; firing a gun still gives the firing hand its own precise pulse rather than a muddier generic one.
16. Dual-wield. Each trigger fires its own pistol along its own hand's ray, semi-automatic pistols fire once per pull on each hand, and the off-hand stick click retracts tethers.
17. The grapple hand. While reeling and hanging, the grapple hand holds the line; while hanging, moving that hand pushes the body. A sharp pull on a live line zips.
18. The body following the head (`body.rig.chase.enabled`, in the standing posture). Walking physically moves the body with the head and the view never runs ahead; walking while pushing the stick moves at the sum of the two; leaning over a table moves nothing; a wall stops the body while the view stays with the head. A fast travel, a respawn, a load, and a vehicle teleport re-base without a chase spike. F7 re-bases yaw and height only.
19. The head fade (`roomscale.head_fade.enabled`). Putting the head into a wall darkens the view with depth and clears on the way out, and moving the head down past the torso does not trigger it.
20. Crouch (`roomscale.crouch.visual`, then `roomscale.crouch.capsule`). A physical crouch folds the legs with the feet planted; with the capsule, Rico fits under low cover, cannot stand up into a ceiling, and swimming, vehicles, the parachute, and ragdoll keep their own capsules. Check whether enemies react to a lowered head.
21. Seat transitions (`roomscale.seat_transition`). With `Instant` and `Fade`, enter, shuffle seats, and exit each vehicle class; the fades cover the animations and the seated view is re-based. Note any seat state logged as unmatched.
22. Mounted guns. A mounted gun and a gunner seat anchor the hands to the handles, and moving the hands swings the muzzle about the pivot without runaway turning.
23. Vehicle weapons (`vehicles.driver_weapons_by_hand`). The driver's weapons follow the dominant hand, turrets trail it, and the motorcycle and jetski personal weapon aims with the hand.
24. Physical controls (each under `vehicles.physical`). The wheel, handlebars, cyclic, collective, flight stick, and throttle sit where the hands expect them, are held only while the grip is held and never taken by a hand that passes through them with the grip open, steer in the right direction, and leave the vehicle's weapons unfired.
25. Arm steering and the parachute handles (`vehicles.arm_steering.wingsuit`, `vehicles.parachute_handles`). In the wingsuit, seated or standing, spreading the arms lights the rings on the hands, lowering the right hand banks right and lowering both hands dives; the head no longer steers the wingsuit. Under the parachute, the hands steer nothing until they take the handles above the shoulders with the grip, and let go when the grip opens; pulling the right handle down banks right, pulling both down slows the canopy, and letting go or raising them flies level.
26. Scopes. Precision aim does not engage from the input, the sniper's bolt cycle, or the guided rocket launcher, and the launcher's missile still guides. The lens appears when the eye is behind the scope, magnifies the target the right way round, and is not blocked by the gun or the hands.
27. Grenades. A grip at the pouch primes a grenade into the hand, the fuse ring counts down, a release throws with the hand's speed and direction, and a grenade held too long explodes in the hand.
28. Aim assist (`aim.assist_strength` above 0). Each hand's aim point pulls toward a hostile target near its sight line, and never toward a target behind cover.
29. The true reload (`weapons.reload`). For each weapon class (see [reload.md](reload.md)): a tap of the reload button ejects and a hold still holsters; a flick ejects; the pouch gives a round to the free hand and nothing with an empty reserve; the round catches and seats in the gun's real well; a belt push reloads in one hand; a run-dry gun clicks until its action is worked; the sniper bolt and the shotgun pump need working after each shot; dual-wielded pistols run dry and reload separately; the HUD counts follow; unlimited ammunition and pickups still work.
30. C4 (`weapons.c4_throw`, `weapons.wrist_detonator`). A grip at the chest pouch primes a charge into either hand, a release throws it with the hand's speed and direction, and it sticks to the ground, a wall, and a vehicle where it lands. A grab is refused at the cap of placed charges. A throw pulses the throwing hand and adds a pip beside the wrist button; a refused grab buzzes. A poke of the right fingertip at the left wrist button, straight in, from above, or from the side, detonates the charges, with a click on both hands, a flash, and a strong pulse when they go off; a slow touch presses after a short dwell; a press with nothing placed buzzes and flashes grey; a finger passing beside the button, coming up through the wrist, or found resting on it when tracking returns does not press. Holding the R stick click detonates too, a tap does not, and it never throws a grenade.
31. The recenter chord. With charges placed, click the right stick a moment before the left (a quarter second apart) and hold both: the view recenters, nothing detonates, and the log has `stick chord: the second click joined late`. Hold the right click alone: the charges detonate.
32. Stick click latency. In a car, a tap of the left stick click honks at once, with no third-of-a-second delay; on foot it steps the weapon at once. Pressing the right B within a third of a second of the left click still opens the debug panel.
33. The wrist summary on mode changes. Jump, drop off a ledge, and grapple-launch on foot: the summary does not pop up. Open the wingsuit or the parachute from a fall, or enter a car: it shows for three seconds, with a `controls summary: the mode changed` line in the log.
34. Cutscenes. Hold the right B through a skippable cutscene: the game's skip prompt fills and the cutscene skips, right- and left-handed. The `VR: STATUS` page's fingerprint row asks for a restart after this update until the game is restarted.
35. The wingsuit's buttons. Open the wingsuit with the right B and close it with the right B again; nothing boosts. In the weaponized suit, click the right stick: it boosts. The left A does nothing in the suit.
36. Aiming the grapple in the wingsuit. With the arms spread and banking slightly, hold the left B and point the left hand at a target: the suit keeps flying the line it had, and lets go of the target to grapple it; once the button is up, the arms steer again. The log has `wingsuit arm steering: the grapple is aimed` and the button-up line.
37. The motorcycle's throttle grip. On a motorcycle with the handlebars held, roll the right grip back: the bike accelerates in proportion, with a click as it opens, and no nitrous or turbo jump; the trigger still accelerates. In a helicopter, raise and lower the collective with an ordinary wrist roll: no nitrous; twist it hard either way: nitrous.
38. The mech. Enter a mech: the summary's heading reads `MECH`, the left stick walks, the right trigger fires the right-hand weapon, the left trigger held attracts with the gravity gun and let go throws, the right grip drops, the left grip punches, and A jumps.
39. Tethers from a vehicle. Drive a car, tether it to something with the grapple, and double tap the left B: the tether lets go.
40. Menu pages with the pointer. In the map or a menu with the native pointer showing, aim just off the panel's edge and pull the right trigger: nothing pages. Pull the left trigger: the page goes back; hold the left grip and pull it: the page goes forward.
41. Map zoom. Open the map and push the left stick up and down: it zooms, and the wrist summary lists the zoom on that stick. Rebind `map_zoom` in the runtime's binding interface to another input: the map zooms from that input instead.
42. Hold to hold across interruptions. Hold a rifle, cover a controller's sensors until the hand is lost, and uncover it with the grip still closed: the rifle stays in the hand; open and close the grip: `hold: the grip closed on the gun in the hand` is logged, and opening it again away from the body drops it. Choose a rifle on the weapon wheel and close the hand on it, then open it in front of the body: it falls. With the pistols drawn in both hands, open one hand anywhere: that pistol goes back to its hip and the other stays drawn. Enter a car holding a rifle: it is not dropped.
43. Long guns by the dominant hand. With both hands empty, reach the left hand to the left shoulder and squeeze: nothing is drawn and the shoulder's marker does not light for it; reach across with the right hand: the special weapon comes into the right hand. With a pistol in the right hand, the left hand draws the left shoulder's gun as a second weapon.
44. The gamepad's held B. Walk slowly toward a ledge: the character stops at it; hold the right B and walk on: he steps off. Get knocked into a ragdoll and hold the right B: he stays a ragdoll; let go and he gets up. Pressing the right B to enter a car still enters it.
45. The game's hints on the wrist. Stand by a car with an armed passenger seat, or get a mission prompt: the summary pops up with `ON SCREEN` first, naming the prompt with the controller's glyph (the gunner seat on the right B, held). With tethers attached, the release hint points at the left B's double tap. Doors, characters, the holsters, and climbable surfaces have markers in their colours (`GRIP MARKERS`).

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

- The radial weapon menu, the seated and accessibility fallback for holsters.
- The Ceremony seat-transition mode and its confirmation prompt.
- The laser pointer for the floating panel and the game's menus.
