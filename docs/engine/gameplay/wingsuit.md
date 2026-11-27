# The wingsuit

How the wingsuit opens, stalls, evades, and lands, and how the canopy opens from a jump or a fall; the canopy's flight is in [parachute-locomotion.md](parachute-locomotion.md). Both fly on [`NAirMovement_UpdateAirPhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-NAirMovement_UpdateAirPhysics). The transitions below are the decoded base graph's (`rico_base`, `tools/anim_graph`, [animation-graph.md](../character/animation-graph.md)).

## Opening from the air

The suit and the canopy open on `ACT_TRIGGER_WINGSUIT` and `ACT_TRIGGER_PARACHUTE` (which input raises each is unverified). The jumps (`S_JUMP_0` and its turned variants, the stunt jumps, and the train jumps), the falls (`S_FALLING`, `S_FAST_FALLING`, `S_FALL_TO_FREEFALL`), the reel-in's drops into a fall, and the falls out of the canopy and the suit take `ACT_TRIGGER_WINGSUIT` to `S_JC_WINGSUIT_OPEN_FROM_FALL`, and `S_FREE_FALLING` takes it to `S_JC_WINGSUIT_OPEN_FROM_FREEFALL`; `ACT_TRIGGER_PARACHUTE` goes to the matching canopy openings (those out of a reel are in [parachute-locomotion.md](parachute-locomotion.md#the-openings-out-of-a-reel)). Off a ledge, either opens during the jump, which runs until its clip ends before handing over to `S_FALLING`.

The jumps and `S_FALLING` gate both acts on the suit or canopy being enabled and on:

| Condition | Class | Shipped parameters |
|---|---|---|
| `C_JUMP_OR_FALL_TO_IN_AIR_SPEED` | `CVerticalSpeedCondition` | a vertical speed below -0.1 m/s: descending |
| `C_JUMP_OR_FALL_TO_IN_AIR_HEIGHT` | `CFallDistanceCondition` | a fall distance of at least 4 m |

`CFallDistanceCondition` reads zero with ground contact and otherwise a distance from the ground probe (to the ground below or fallen so far: unverified). A press while rising or too low opens nothing and is not held over. `S_FAST_FALLING` and `S_FREE_FALLING` check only the enabled flag.

The boost wingsuit also launches from the ground on `ACT_BOOST_FROM_GROUND`, from the idle, moves, and turns on foot (with the suit enabled and the boost suit owned), through the wind-up `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO`, where no movement task turns the character, to the launch `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND` along its facing. The act takes about a second's hold of the use button; which action is read is unverified.

## The stall and flip

The suit's blend state ([`WingsuitBlendState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState)) blends the flight samplers and alone decides the stall ([`UpdateSamplers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState##func-UpdateSamplers)); no state bit, act, or task sub-state is involved. The stall begins when the forward airspeed (the measured velocity along the forward axis) falls below [`WingsuitBlendState_ToStallSpeed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState_ToStallSpeed) with the nose above [`WingsuitBlendState_StallNoseUp`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState_StallNoseUp), the nose test skipped while a stall clip has weight, and ends once the airspeed passes a higher threshold. The decision is a local flag, set by one instruction ([`WingsuitBlendState_StallFlagSet`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState_StallFlagSet)) and cleared on every other path.

Inside the stall clip's selection segment, the blend state ramps up the flip, a recovering loop, with the nose raised further, or the plain stall otherwise, sets the character's in-flip flag during the flip, and scales the other samplers down by the stall's weight, replacing the flight blend. The stall's turn segment is read four times: the air physics skips part of its steering during it, the air brake and the close act are allowed only outside it ([the landing](#the-landing)), and the boost reads the in-flip flag (unverified).

Neither clip moves the body: the air physics clamps its pitch and orients it from the steering, so the loop is the pose's own. No damping is keyed to the stall; the ordinary drag, lift, and gravity have already taken the speed. Without the stall, a raised nose loses lift and drops, and the body is pulled toward its velocity (the pull's gating is unverified).

## The evade

The evade is a barrel roll, one act ([`ACT_BOOST_WINGSUIT_BARREL_ROLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::symbol_table::EventIdSymbolTable)) with no state bit or task sub-state. The air physics starts it in its boost-suit part, for a character with the boost wingsuit ([`Character::HasBoostWingsuit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-HasBoostWingsuit)) in a state that flies it (unverified gate). On [`WINGSUIT_EVADE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-WINGSUIT_EVADE) it tests a character value, believed the boost's charge (unverified), against a threshold; short, the HUD shows the boost's failure unless a missile is locked on. Otherwise, with the move stick out of its rest, it queues the act on the animated model ([`AnimatedModel::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-QueueAct)), bypassing [`Character::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-QueueAct), so watchers of the character's queue miss it while the model's queue, which the character's forwards to, sees every act. The game binds the action to the weaponized wingsuit's evade chord ([input.md](input.md)).

The directions are segments of the one clip (`SEG_IN_BARREL_ROLL_EVADE_MOVEMENT` and its `_LEFT`, `_RIGHT`, `_UP`, and `_DOWN` variants, then `SEG_IN_BARREL_ROLL_RECOVERY_MOVEMENT`), which rolls the pose a full turn. During the evade-movement segment, the air physics orients the body from a matrix the character holds and the velocity's heading rather than the steering alone; that the matrix is the animated reference bone, so the clip carries the body sideways without velocity alignment, is unverified.

## The landing

The suit lands only through its close act; still gliding at the ground, the character ends in the crash ragdoll.

The wingsuit task (`NStateTask_MovementWingSuitTask::Update`, not located) queues `ACT_CLOSE_WINGSUIT` every tick [`CANCEL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-CANCEL) is pressed, outside the stall's turn segment, unless the grapple has an active wire or [`RETRACT_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-RETRACT_GRAPPLE) is pressed, so a held air brake asks to close every tick. Every state taking the act (the glide, `S_JC_WINGSUIT_IDLE`, the openings, the grapple states, and the boosted takeoffs) gates it, outside its no-in-air-transition segment, on:

| Condition | Class | Shipped parameters |
|---|---|---|
| `C_WINGSUIT_CANCEL_FLIGHT_SPEED` | `CSpeedCondition` | the speed at or below the condition's limit |
| `C_WINGSUIT_CANCEL_LAND_HEIGHT` | `CHeightOverGroundCondition` | the height over the ground below the condition's limit |
| `C_HAS_WINGSUIT_CANCEL_UPGRADE` | `CUpgradePurchasedCondition` | the `upgrade_wingsuit_cancel` gear mod |

Slow and low, the act goes to `S_JC_WINGSUIT_TO_LAND_SLIDE`, `S_JC_WINGSUIT_TO_LAND_SLIDE_ONGROUND`, and the idle on foot; slow but higher, to `S_JC_WINGSUIT_TO_FALL_SLOW`, an ordinary fall landing as any does (`S_LAND_IDLE_*`, or `S_LAND_HARD` from high enough); faster, nothing fires.

`CSpeedCondition` compares the measured world speed, the magnitude stored after [`Character::m_MeasuredWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_MeasuredWorldVelocity) (not bound), so the sink counts with the glide; its flag picks the side of the limit. `CHeightOverGroundCondition` reads, by its flag, the body rule system's [`AnimationRuleSystem::m_HeightOverGround`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::AnimationRuleSystem##field-m_HeightOverGround) (over the nearer of the ground and the water surface) or `m_HeightOverGroundExcludingWater`, which [`Character::DoGroundInfoRaycasts`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-DoGroundInfoRaycasts) zeroes while the proxy is supported or sliding and [`Character::CalculateHeightOverGround`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-CalculateHeightOverGround) otherwise casts for.

The air brake runs while `CANCEL` is pressed in the suit, outside the turn segment, without `RETRACT_GRAPPLE`, with the `upgrade_wingsuit_air_brake` gear mod bought and enabled. It multiplies the drag and, while descending, the lift by its share of the descent; sideways steering weakens both, the lift by its own factor and the drag by a capped share (the unbound statics `s_max_airbrake_drag`, `s_max_airbrake_lift`, and their sideways factors), so a turning brake slows less. A suit that slows into a stall can neither brake nor land until it ends.

The boost (Bavarium) wingsuit, `C_PLAYER_HAS_BOOST_WINGSUIT`, also lands from its brake, `S_JC_WINGSUIT_BOOST_BREAK`, entered on `ACT_BOOST_BREAK` from the glide. With the brake held (`C_WINGSUIT_BREAK_IS_HELD`) over near-level ground (`C_SURFACE_NORMAL_HORIZONTAL_FOR_BREAK_LANDING`), it goes to `S_JC_BOOST_WINGSUIT_BREAK_TO_LAND` when near the ground (`C_BOOST_WINGSUIT_BREAK_LAND_HEIGHT`) and slow (`C_BOOST_WINGSUIT_BREAK_TO_LAND_FLIGHT_SPEED`), to `S_JC_BOOST_CRASH_RECOVERY` nearer (`C_BOOST_WINGSUIT_CRASH_RECOVERY_LAND_HEIGHT`) under a higher limit (`C_BOOST_WINGSUIT_CRASH_RECOVERY_FLIGHT_SPEED`), and to `S_JC_BOOST_HIGH_VELOCITY_CRASH_RECOVERY` at that height at any speed, each recovering onto the feet. The close act lands the boost suit too.

Every wingsuit state takes `ACT_CRASH_LAND` to `S_CRASH_LAND_RAGDOLL`; no code names the act, and what queues it on contact is unknown.

The parameters and gear mod names are the shipped graph's. `CSpeedCondition`'s evaluator and the heights `CHeightOverGroundCondition` reads, with their writes, are checked against the IDB; the task's close act, the brake's gates and multipliers, and `CHeightOverGroundCondition`'s evaluator are unverified.
