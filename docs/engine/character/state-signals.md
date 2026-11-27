# Per-tick character state signals

The engine exposes per-tick signals, readable from the local `CCharacter` or a cheap singleton, that separate the player's modes: on foot, in a vehicle (and its class), wingsuit, parachute, grapple traversal, swimming, and game UI up. The weapon in the hand is in [weapon-attachment-and-arm-ik.md](weapon-attachment-and-arm-ik.md), and the character's root motion, collision proxy, and teleports in [root-motion-and-proxy.md](root-motion-and-proxy.md).

The verified addresses, field offsets, and constants are in the pyxis defs: mainly [`Character`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character), [`StateBitFlag`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag), and [`Clock`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock).

## The central mechanism: the character state-bitflag words

Almost every "what is the player doing" predicate the game ships (`CCharacter::IsUsingWingsuit`, `IsReelingIn`, `IsSwimming`, and the others below) is a one-bit read of a small array of 64-bit state-bitflag words owned by the character's control parameters. The words can be read directly without calling the predicates.

- [`Character::m_ControlParameters`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_ControlParameters) is a pointer to the character's [`ControlParameters`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::ControlParameters) (confirmed in the decompile of `IsInVehicleAttachState`).
- The state-bitflag array (`m_ControlParameters->m_UserVM.m_CurrentStateBitFlags`) is the run of `u64` words [`UserVM::m_CurrentStateBitFlags0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::UserVM##field-m_CurrentStateBitFlags0), [`m_CurrentStateBitFlags1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::UserVM##field-m_CurrentStateBitFlags1), and [`m_CurrentStateBitFlags2`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::UserVM##field-m_CurrentStateBitFlags2) in [`ControlParameters::m_UserVM`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::control_parameters::ControlParameters##field-m_UserVM). The bits are named by [`StateBitFlag`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag): flag `n` lives in word `n / 64` at mask `1 << (n % 64)`. The whole read is `flags = character->m_ControlParameters->m_UserVM.m_CurrentStateBitFlags[n / 64]`.
- Some of the predicates also require [`AnimatedModel::m_InstanceData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##field-m_InstanceData) (on [`Character::m_AnimatedModel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_AnimatedModel)) to be non-null. The animation instance must exist for the bit to be meaningful, which matters only in the first frames after spawn or stream-in.

The flag each predicate tests comes from the game's accessors, verified in the IDB:

| Predicate | `StateBitFlag` | Word | Notes |
|---|---|---|---|
| [`IsUsingWingsuit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsUsingWingsuit) | [`SB_IS_WINGSUIT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_WINGSUIT) | `m_CurrentStateBitFlags0` | gated on `m_InstanceData` |
| [`IsUsingParachute`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsUsingParachute) | [`SB_IS_PARACHUTING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_PARACHUTING) | `m_CurrentStateBitFlags0` | parachute deployed and controlling descent |
| [`IsFreefalling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsFreefalling) | [`SB_IS_FREE_FALLING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_FREE_FALLING) | `m_CurrentStateBitFlags0` | |
| `IsFalling` | [`SB_IS_FALLING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_FALLING) or `SB_IS_FREE_FALLING` | `m_CurrentStateBitFlags0` | freefall or fall; no accessor identified |
| [`IsFastfalling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsFastfalling) | [`SB_IS_FASTFALLING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_FASTFALLING) | `m_CurrentStateBitFlags0` | dive |
| [`IsReelingIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsReelingIn) | [`SB_IN_REELING_IN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_REELING_IN) | `m_CurrentStateBitFlags1` | grapple reel traversal in progress |
| [`IsRidingMC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsRidingMC) | [`SB_IS_RIDING_MOTORBIKE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_RIDING_MOTORBIKE) | `m_CurrentStateBitFlags1` | on a motorcycle |
| [`IsSwimming`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsSwimming) | [`SB_IS_SWIMMING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_SWIMMING) | `m_CurrentStateBitFlags1` | |
| [`IsUnderwaterSwimming`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsUnderwaterSwimming) | [`SB_IS_UW_SWIMMING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_UW_SWIMMING) | `m_CurrentStateBitFlags1` | |
| [`IsInAttachedToParachute`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInAttachedToParachute) | [`SB_IN_ATTACHED_TO_PARACHUTE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_ATTACHED_TO_PARACHUTE) | `m_CurrentStateBitFlags1` | attached-to-vehicle-parachute variant |
| [`IsPreReelingIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsPreReelingIn) | [`SB_IN_PRE_REEL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_PRE_REEL) | `m_CurrentStateBitFlags1` | reel-in wind-up |
| [`IsPreHang`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsPreHang) | [`SB_IN_PRE_HANG`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_PRE_HANG) | `m_CurrentStateBitFlags2` | |

These are game-thread data. The words are written during the character's animation-graph evaluation on the sim tick, so a read on a render worker races that write.

## The state-machine-hash predicates (vehicle riding, reeled in, hang)

A second class of predicate is not a single bit. It walks the animation rule system (`m_AnimatedModel.m_RuleSystems[0]->m_StateMachineInstance->m_CurrentState->m_HashID.m_Hash`) and compares the current animation state hash against a set of known state ids, sometimes combined by OR with a state bit or the grapple-hook state. These cost a few pointer chases. They are the authoritative signal where a raw bit is ambiguous:

| Predicate | What it establishes |
|---|---|
| [`IsInRidingInVehicleState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInRidingInVehicleState) | seated and riding (idle, driver, passenger, or reverse states, or [`SB_IS_RIDING_MOTORBIKE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_RIDING_MOTORBIKE) set) |
| [`IsInDrivingVehicleState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInDrivingVehicleState) | specifically the driver seat's driving states, or `SB_IS_RIDING_MOTORBIKE` set |
| [`IsInVehicleAttachState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInVehicleAttachState) | riding, or the enter, exit, switch-seat, or eject transitions |
| [`IsReeledIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsReeledIn) | fully reeled onto an attach point: the hook `m_State` is one of the reeled states (`GHS_REELED_ATTACHED`, `GHS_REELED_HANG`, `GHS_REELED_UPSIDEDOWN`, or `GHS_REELED_STUNT`), combined with [`SB_IN_GRAPPLE_ATTACHED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_GRAPPLE_ATTACHED), [`SB_IS_GRAPPLE_UPSIDEDOWN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_GRAPPLE_UPSIDEDOWN), or the stunt and hang state hashes |
| [`IsGrappleHanging`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsGrappleHanging) | hanging from a reeled grapple (hook `m_State == GHS_REELED_HANG`, or `S_GRAPPLE_HANG`/`S_IDLE_HANG_STUNT`, or `SB_IS_GRAPPLE_UPSIDEDOWN`) |
| [`IsStuntTraversing`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsStuntTraversing) | stunt-position traversal (`S_STUNT_FWD/RIGHT/LEFT/BWD`) |
| [`IsInMountedGunState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInMountedGunState) | manning a mounted or emplaced gun |

`IsInVehicleAttachState` is the widest test for a body bound to a vehicle. It returns true through the whole enter and exit animation, not only once the character is settled, so the mount animation is inside its envelope.

## Vehicle presence and class

The seated vehicle and its class come from the interaction graph, not from a flag:

- [`Character::GetVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-GetVehicle) fills a `boost::shared_ptr<CVehicle>`. It walks the interaction graph's top-root object and applies `rtti_cast<CVehicle>`. [`GetVehiclePtr`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-GetVehiclePtr) is the raw-pointer wrapper. Both take and release a shared-ptr refcount (an interlocked increment and decrement), so each call costs more than a field read.
- Class is decided by an RTTI `IsType` test against the vehicle's class-hierarchy `TYPE_ID`, exposed as character predicates:

| Class predicate | `TYPE_ID` tested |
|---|---|
| [`IsAttachedToVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToVehicle) | any (has a `CVehicle` and is graph-attached) |
| [`IsAttachedToLandVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToLandVehicle) | `CLandVehicle` (car and motorcycle) |
| [`IsAttachedToAirVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToAirVehicle) | `CAirVehicle` (plane and helicopter) |
| [`IsAttachedToSeaVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToSeaVehicle) | `CSeaVehicle` (boat and jetski) |
| [`IsAttachedToHelicopter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToHelicopter) | `CHelicopter` |
| [`IsAttachedToMech`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsAttachedToMech) | via [`Vehicle::IsMech`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-IsMech) |

The RTTI bases give the coarse split. The `CButtonMapping::EMapping` sections split more finely than the RTTI bases: car and motorcycle, boat and jetski, and plane and helicopter each have their own mapping. The predicates combine into the finer classes as follows:

- Helicopter: `IsAttachedToHelicopter`.
- Plane: `IsAttachedToAirVehicle && !IsAttachedToHelicopter`. Both derive from `CAirVehicle`.
- Motorcycle: `IsAttachedToLandVehicle && IsRidingMC` (`SB_IS_RIDING_MOTORBIKE`).
- Land car: `IsAttachedToLandVehicle && !IsRidingMC`.
- Boat or jetski: `IsAttachedToSeaVehicle`. The RTTI does not split boat from jetski at the `CSeaVehicle` base. The engine's vehicle-type enum is [`VehicleType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::VehicleType) (`NVehicle::EVehicleType`: `ECAR`, `EHELICOPTER`, `EBOAT`, `EAIRPLANE`, `EMOTORCYCLE`, `ESUB`, and `ETRAIN`), stored on the vehicle as `CVehicle::m_VehicleType` and returned by `CVehicle::GetVehicleType`, which is inlined everywhere. The field offset is not pinned. This enum also folds jetski into `EBOAT`; the game selects the jetski mapping band from a jetski-specific type elsewhere. TODO: pin the `m_VehicleType` offset and the jetski discriminator.
- Seat determines the input mapping, independent of class. [`Vehicle::m_LocalPlayerSeat`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##field-m_LocalPlayerSeat) (a [`Seat`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Seat), `NVehicle::ESeat`, or `ENoSeatIdentifier` when the player has no seat) is what the game's own context predicates test (`m_LocalPlayerSeat == EDriverSeat`) to decide whether driver controls apply. A passenger or gunner gets a different mapping in the same vehicle. `CVehicle::GetSeat` is bound as [`Vehicle::GetSeat`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-GetSeat).

## Grapple traversal states (`GHS_*`)

The grappling-hook state machine is the finest grapple-traversal signal, and it is independent of the character animation flags. [`Character::GetGrapplingHook`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-GetGrapplingHook) returns the hook shared-ptr. The raw hook pointer is cached in the character's inventory ([`Character::m_Inventory`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_Inventory), [`Inventory::m_GrapplingHook`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##field-m_GrapplingHook)), and its state is [`GrapplingHook::m_State`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##field-m_State), a [`GrapplingHookState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookState) (`EGrapplingHookState`): `GHS_INITIALIZING`, `GHS_INACTIVE`, `GHS_REELING_IN`, `GHS_REELED_ATTACHED`, `GHS_REELED_HANG`, `GHS_REELED_UPSIDEDOWN`, `GHS_REELED_STUNT`, and `GHS_CUSTOM_ACTIVE_WIRE`.

A reeled state (`GHS_REELED_ATTACHED`, `GHS_REELED_HANG`, `GHS_REELED_UPSIDEDOWN`, or `GHS_REELED_STUNT`) means reeled onto something (the `IsReeledIn` predicate), `GHS_REELED_HANG` is hanging, and `GHS_REELING_IN` is mid-reel. The character-level `IsReelingIn`, `IsGrappleHanging`, and `IsReeledIn` above combine this hook state with the animation state. The raw `hook->m_State` remains readable when the hook pointer is present and the character predicate is ambiguous during a transition.

## Game UI up and gameplay input suspended

The engine suspends gameplay input through three distinct mechanisms: the hard pause, the pause-until-action utility, and a switch of input focus to the UI action map.

1. Hard pause (pause menu). `CGameStateRun` propagates the update context's [`GameObjectUpdateContext::m_Paused`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObjectUpdateContext##field-m_Paused) flag into `CPhysicsSystem::m_Pause` (`Base::CSingle<CPhysicsSystem>::Instance->m_Pause = m_Paused`) and calls `CClock::Pause(true)`, which sets the game-clock game-pause bool (see "What the game's pause freezes" below). The physics system is the [`PhysicsSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem) singleton. Its `m_Pause` byte is nonzero for the whole real pause, so it is a per-tick read of whether the game's own pause is active. A real pause also freezes the camera update.
2. Pause until action. [`GamePauseUtility`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::game_pause_utility::GamePauseUtility), a `Base::CSingle` singleton, is the script facility that pauses the game until an action is pressed, used by tutorials. It is not a weapon selector; the game's weapon selection is a cross driven by the slot-select actions ([input.md, "Weapon selection, rebel drops, and gear mods"](../gameplay/input.md#weapon-selection-rebel-drops-and-gear-mods)). [`GamePauseUtility::m_state`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::game_pause_utility::GamePauseUtility##field-m_state) is its first field, and `m_state == `[`PauseState::E_STATE_PAUSED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::game_pause_utility::PauseState##variant-E_STATE_PAUSED) means the pause is up. [`GamePauseUtility::SetState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::game_pause_utility::GamePauseUtility##func-SetState) drives the state machine.
3. Input focus (map and commlink, with no physics pause). Opening the map or the commlink does not hard-pause the game. The game switches input focus to the UI action map instead. `CSteeringUI` holds a static UI action map (`CSteeringUI::GetActionMap` → `m_ActionMapUI`) that becomes the focused consumer while a UI screen is up, and the `CButtonMapping` band `FIRST_UI_MAPPING..END_UI_MAPPINGS` (bounded by the game's own `END_UI_MAPPINGS`) applies. Whether `CSteeringUI` is the active steering is the signal for these screens. No single per-tick "UI steering active" global is pinned. TODO: pin it, from `CSteeringUI::m_SteeringUI` presence plus a focus check. The pause flag does not cover the non-pausing screens.

## What the game's pause freezes, and the clock split

A real pause (the pause menu is up) switches `CGameStateRun` into its paused update, which runs [`GameStateRun::UpdateRenderPaused`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameStateRun##func-UpdateRenderPaused). That path drives the render systems: landscape LOD, UI prerender, video, `WaitForCPUDrawToFinish`, and resource streaming. It reads the active camera's already-computed `m_TransformF`, but it does not call `GameCameraManager::UpdateRender` or `CameraTree::UpdateRenderContexts`. Under a real pause the camera transform is therefore not recomputed and stays frozen at its last value. Entering pause also sets `CPhysicsSystem::m_Pause` and calls `CClock::Pause(true)`. Leaving it clears both.

[`Clock::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##func-Update) keeps two sets of accumulators, one that always advances and one that stops while the game-pause bool, [`Clock::m_Stop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_Stop), is set. The fields of the [`Clock`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock) singleton that `Update` maintains:

| Field | Type | Behaviour in `Update` |
|---|---|---|
| [`m_FPS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_FPS) | `f32` | `1 / m_SPF`, always updated |
| [`m_SPF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_SPF) | `f32` | frame delta smoothed with an exponential filter, times `m_UpdateSpeed`, always updated |
| [`m_RealFPS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_RealFPS) | `f32` | `1 / m_RealSPF`, always updated |
| [`m_RealSPF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_RealSPF) | `f32` | raw frame delta, always updated |
| [`m_UpdateSpeed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_UpdateSpeed) | `f32` | timescale multiplier on `m_SPF` |
| `m_Stop` | `bool` | game-pause bool |
| [`m_Time`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_Time) | `u32` | milliseconds, always advances |
| [`m_RealTimeAtUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_RealTimeAtUpdate) | `u32` | milliseconds, always advances |
| [`m_PauseTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_PauseTime) | `u32` | milliseconds, stops while `m_Stop` is set |
| [`m_GameTimeMicro`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_GameTimeMicro) | `u64` | microseconds of `m_SPF`, always advances |
| [`m_GameRunTimeMicro`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_GameRunTimeMicro) | `u64` | microseconds of `m_SPF`, stops while `m_Stop` is set |
| [`m_CpuFrequency`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_CpuFrequency) | `u64` | performance-counter frequency, the divisor for all deltas |

[`Clock::Pause`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##func-Pause) writes `m_Stop`. On a transition from unpaused to paused it snapshots `m_RealTimeAtUpdate` into [`m_PauseTimeAtUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_PauseTimeAtUpdate) and [`m_RealTimeAtUpdateGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_RealTimeAtUpdateGame) into [`m_PauseTimeAtUpdateGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##field-m_PauseTimeAtUpdateGame). `Update` does not zero `m_SPF` during a pause. The pause shows in the getters instead: [`Clock::GetSPF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock##func-GetSPF) returns [`Clock::PAUSED_SPF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock::PAUSED_SPF) while `m_Stop` is set unless its `ignore_pause` argument is true.

The pyxis `Clock` binds the millisecond counters as the six `u32` fields the code accesses: `m_Time`, `m_RealTimeAtUpdate`, `m_RealTimeAtUpdateGame`, `m_PauseTimeAtUpdate`, `m_PauseTimeAtUpdateGame`, and `m_PauseTime`, the millisecond counter that stops during a pause. Because of this split, game time can stop while the real clock, and the render path of the unpaused `CGameStateRun` update that calls the camera update, keeps running.

## State overlap and precedence

The states overlap. A reeled grapple is also airborne. A wingsuit dive is also falling, and wingsuit and parachute both count as falling. A vehicle-parachute is both an attach state and a parachute state. `IsInVehicleAttachState` spans the enter and exit animations. The pause and UI signals ("Game UI up and gameplay input suspended") hold regardless of the character state beneath them.

A classification of the player's state resolves each overlap by testing the more specific signal first:

1. The UI and pause signals.
2. A teleport on this tick ([root-motion-and-proxy.md, "Teleport writers and teleport signals"](root-motion-and-proxy.md#teleport-writers-and-teleport-signals)).
3. Vehicle attach (`IsInVehicleAttachState`, with class from "Vehicle presence and class" tested helicopter, plane, motorcycle, car, then sea, and the seat from `m_LocalPlayerSeat`).
4. Grapple traversal: `IsReelingIn` (`SB_IN_REELING_IN`), `IsGrappleHanging`, `IsReeledIn`, `IsPreReelingIn` (`SB_IN_PRE_REEL`), and `IsPreHang` (`SB_IN_PRE_HANG`), with the `GHS_*` hook state in `GrapplingHook::m_State` as the fine discriminator.
5. Wingsuit (`IsUsingWingsuit`, `SB_IS_WINGSUIT`, gated on `m_InstanceData`).
6. Parachute (`IsUsingParachute`, `SB_IS_PARACHUTING`, or `IsInAttachedToParachute`, `SB_IN_ATTACHED_TO_PARACHUTE`, for the vehicle-parachute case).
7. Free fall (`IsFreefalling`, `IsFalling`, and `IsFastfalling`: `SB_IS_FREE_FALLING`, `SB_IS_FALLING`, and `SB_IS_FASTFALLING`).
8. Swimming (`IsSwimming` and `IsUnderwaterSwimming`: `SB_IS_SWIMMING` and `SB_IS_UW_SWIMMING`).
9. On foot when nothing else matches.

The `EvaluateCharacterOrientation` call count advances every on-foot tick including idle ([the locomotion task](root-motion-and-proxy.md#the-locomotion-task-runs-at-idle-and-writes-the-proxy-input-directly)), so it corroborates the on-foot case but does not separate the other states.

The full read is one pointer chase to `m_ControlParameters`, three word loads, a fixed set of bit tests, and a `GetVehiclePtr` only when a vehicle bit or state indicates a vehicle.

## Edge cases

- Transitions. Vehicle enter and exit, grapple wind-up (`IsPreReelingIn`/`IsPreHang`), and reel release all pass through animation states where the coarse bit is not yet set but the body is already committed. `IsInVehicleAttachState` spans the vehicle transition, and the pre-* predicates cover the grapple wind-up. During a known transition, the state-machine-hash predicates track the state where the raw bit lags, and a blend frame can flip a predicate for a single tick.
- Overlapping states. See "State overlap and precedence".
- Swimming. `IsSwimming` is set independently of the on-foot locomotion task. The `EvaluateCharacterOrientation` call count ([the locomotion task](root-motion-and-proxy.md#the-locomotion-task-runs-at-idle-and-writes-the-proxy-input-directly)) does not identify a swimmer as on foot or as swimming; only the swim bits do.
- Spawn and stream-in. The `m_InstanceData`-gated bits (wingsuit) read false until the animation instance exists, and `m_ControlParameters` or `m_InstanceData` can be null in the first frames.
- Teleports. On the tick a fast travel or mission warp re-bases the character ([root-motion-and-proxy.md](root-motion-and-proxy.md#teleport-writers-and-teleport-signals)), the state signals can momentarily misclassify the frame.

## Ledge climbs and vaults

The player has one ledge climb: the grapple's. Reeling in to a ledge ends in the climb over it (`ACT_GRPL_CLIMB` into `S_GRAPPLE_CLIMB_START` in `rico_base.afsmb`), and the climb's motion task, [`NStateTask_ReeledInLedgeClimbMotionTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReeledInLedgeClimbMotionTask), carries the character over: its [`Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReeledInLedgeClimbMotionTask##func-Update) runs on every tick the climb's state is active, writes a code-controlled wanted velocity along the climb (from the climb segment's local time while it plays, the animation's root velocity after), and sets the task-defined motion state. The character keeps no flag of its own for the climb, so the task's running is the signal.

There is no on-foot vault or obstacle climb. The act table has `ACT_START_VAULT`, `ACT_END_VAULT`, and `ACT_CLIMB_OBSTACLE`, but no state machine the game ships under `animations/jc_statemachine` (Rico's `rico_base`, his partials and additives, and the NPCs', base and patch archives alike) names them or any vault state, so nothing queued them would be accepted. `CPfxCharacterInstance::CheckLedges` is not a ledge grab either: it pushes ledge points to the proxy so the character clamps to its support surface at a ledge's edge (`m_LedgeClampToSupportSurface`; unverified).

## Hit reactions and scripted sequences

A hit reaction keeps no state on the character that lasts as long as the reaction. [`Character::HitReaction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-HitReaction) accumulates a hit's scaled impulse and, for a player, picks by the character's thresholds: a ragdoll, a knock-back from an explosion ([`ACT_HITREACT_FLY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::symbol_table::EventIdSymbolTable), or the full ragdoll when the state machine refuses it), a stumble ([`ACT_HITREACT_STUMBLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::symbol_table::EventIdSymbolTable), or its bullet or melee variant, each tested with the state machine first), or a flinch. It queues the act straight on the character's animated model with [`AnimatedModel::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-QueueAct), not through [`Character::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-QueueAct), which only forwards there. The per-hit cooldown timers it keeps run down within the reaction and say nothing about its end. In `rico_base.afsmb` the stumbles lead to the `S_JC_HITREACT_STUMBLE_*` states and the knock-back to `S_JC_HITREACT_FLY_*`. The state bits named for reactions ([`SB_IN_HIT_REACTION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_HIT_REACTION) and [`SB_IN_REACTION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_REACTION)) are tested by no code found so far, and no state-bit task in Rico's four graphs sets either.

A scripted sequence (`CSequenceObject2`) that takes a character over calls [`Character::SetupForSequence`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetupForSequence) from its pre-simulation update, which forces the neutral state and sets [`m_InSequence`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_InSequence); [`Character::ResetAfterSequence3`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-ResetAfterSequence3) restores the default animation layers and clears it when the sequence lets go. The flag holds for the whole time a sequence drives the character, whether or not the game has left gameplay.

## Confidence

| Signal | Confidence | Basis |
|---|---|---|
| On foot (state words and orientation call count) | High | word reads verified; the call count is observed at runtime |
| Vehicle presence and coarse class (land, air, sea, and helicopter) | High | RTTI predicates located and decompiled (`IsAttachedTo*`) |
| Vehicle fine class (car or motorcycle, plane or helicopter) | High | the `IsRidingMC` bit and `IsAttachedToHelicopter` split their pairs |
| Boat or jetski | Low | not split at the `CSeaVehicle` RTTI base; needs the `m_VehicleType`/jetski discriminator (TODO) |
| Wingsuit active | High | `IsUsingWingsuit` (`SB_IS_WINGSUIT`) verified |
| Parachute open | High | `IsUsingParachute` (`SB_IS_PARACHUTING`) verified; `IsInAttachedToParachute` covers the vehicle case |
| Reel and grapple traversal (`GHS_*`) | High | `GrapplingHook::m_State` and the character predicates verified |
| Hard pause (pause menu) | High | the `CPhysicsSystem::m_Pause`/`CClock` pause path decompiled |
| Pause until action (`CGamePauseUtility`) | Medium | `m_state == E_STATE_PAUSED` established; the singleton is bound |
| Map and commlink UI focus (non-pausing) | Medium | mechanism understood (`CSteeringUI` focus and the UI mapping band); no single per-tick global pinned yet |

