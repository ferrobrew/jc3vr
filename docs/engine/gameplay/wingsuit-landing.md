# The wingsuit's landing

The wingsuit lands through its close act. There is no landing that happens on contact alone: a character still in the glide when it meets the ground ends in the crash ragdoll. The landing is the close act taken low and slow, which the animation graph turns into a slide onto the feet instead of a fall.

## The close act

The wingsuit task (`NStateTask_MovementWingSuitTask::Update`, address not yet located) queues `ACT_CLOSE_WINGSUIT` on every tick that [`CANCEL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-CANCEL) is pressed, outside the stall's turn segment ([wingsuit-stall.md](wingsuit-stall.md)), unless the grapple has an active wire or [`RETRACT_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-RETRACT_GRAPPLE) is pressed. `CANCEL` held is also the air brake (below), so a held brake asks to close the suit on every tick it is held.

The act does nothing on most ticks. Every wingsuit state that takes it (the glide, `S_JC_WINGSUIT_IDLE`, the openings, the grapple states, and the boosted takeoffs) gates it on three conditions, outside the state's no-in-air-transition segment:

| Condition | Class | Shipped parameters |
|---|---|---|
| `C_WINGSUIT_CANCEL_FLIGHT_SPEED` | `CSpeedCondition` | the speed at or below the condition's limit |
| `C_WINGSUIT_CANCEL_LAND_HEIGHT` | `CHeightOverGroundCondition` | the height over the ground below the condition's limit |
| `C_HAS_WINGSUIT_CANCEL_UPGRADE` | `CUpgradePurchasedCondition` | the `upgrade_wingsuit_cancel` gear mod |

Slow enough and low enough, the act goes to `S_JC_WINGSUIT_TO_LAND_SLIDE`, then `S_JC_WINGSUIT_TO_LAND_SLIDE_ONGROUND`, and back to the idle on foot. Slow enough but higher, it goes to `S_JC_WINGSUIT_TO_FALL_SLOW`, an ordinary fall that lands as any fall does (`S_LAND_IDLE_*`, or `S_LAND_HARD` from high enough). Faster than the speed limit, no transition fires and the suit stays open.

`CSpeedCondition` compares the character's measured world speed, the magnitude stored after [`Character::m_MeasuredWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_MeasuredWorldVelocity) (not bound), so the limit counts the sink as well as the glide: a suit gliding level at the limit while it sinks fast is over it. Its flag picks the side of the limit, and the cancel's condition passes at or below it. `CHeightOverGroundCondition` reads the first of the character's ground probes while the character has no ground contact (the probe's height with or without water, by its flag), and zero with contact.

## The air brake

The air physics ([`NAirMovement_UpdateAirPhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-NAirMovement_UpdateAirPhysics)) brakes while `CANCEL` is pressed in the wingsuit, outside the stall's turn segment, without `RETRACT_GRAPPLE`, and with the `upgrade_wingsuit_air_brake` gear mod bought and enabled. The brake multiplies the drag, and while the body descends it multiplies the lift by its share of the descent. The steering's sideways input weakens both, the lift by its own factor and the drag by half of the input at most (the statics `s_max_airbrake_drag`, `s_max_airbrake_lift`, and their sideways-input factors, unbound), so a brake held while turning slows less than a level one.

The stall closes the brake and the close act alike while its turn segment runs, so a suit that slows into a stall with its nose raised cannot land until the stall ends.

## The boost wingsuit's brake landing

The boost (Bavarium) wingsuit, `C_PLAYER_HAS_BOOST_WINGSUIT`, has a landing of its own in its brake state, `S_JC_WINGSUIT_BOOST_BREAK`, entered on `ACT_BOOST_BREAK` from the glide. While the brake is held (`C_WINGSUIT_BREAK_IS_HELD`) over ground whose normal is near vertical (`C_SURFACE_NORMAL_HORIZONTAL_FOR_BREAK_LANDING`), the state goes to:

- `S_JC_BOOST_WINGSUIT_BREAK_TO_LAND` near the ground (`C_BOOST_WINGSUIT_BREAK_LAND_HEIGHT`) and slow enough (`C_BOOST_WINGSUIT_BREAK_TO_LAND_FLIGHT_SPEED`);
- `S_JC_BOOST_CRASH_RECOVERY` nearer the ground (`C_BOOST_WINGSUIT_CRASH_RECOVERY_LAND_HEIGHT`) and under a higher speed limit (`C_BOOST_WINGSUIT_CRASH_RECOVERY_FLIGHT_SPEED`);
- `S_JC_BOOST_HIGH_VELOCITY_CRASH_RECOVERY` at the same height at any speed.

Each is a recovery onto the feet rather than a ragdoll. The plain suit's close act also lands the boost suit.

## The crash

Every wingsuit state also takes `ACT_CRASH_LAND` to `S_CRASH_LAND_RAGDOLL`. No code names the act, and what queues it on contact is not found (unverified).

The parameters above are the shipped graph's (`rico_base`), read from its condition definitions, and the gear mods' names are the graph's and the symbol dump's. `CSpeedCondition`'s evaluator was checked against the release IDB. The task's close act, the air brake's gates and multipliers, and `CHeightOverGroundCondition`'s reads are read from the symbol dump (unverified).
