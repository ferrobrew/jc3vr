# Parachute head-yaw suppression

Why the parachute turned with the head in VR, and how the mod stops it. The engine-side mechanism is [engine/gameplay/parachute-locomotion.md](../../engine/gameplay/parachute-locomotion.md); the implementation is `crates/jc3vr-body/src/hooks/input/parachute.rs`.

## The problem

The parachute steering helper `NParachuteMovement::GetParachuteSteeringValues` mixes the **camera input matrix** into the chute's steering. In VR the camera follows the HMD, so turning the head past the look-steer deadzone (`m_LookSteerDeadZone`, shaped by `m_LookSteerMaxYaw`/`m_LookSteerExponential`) turns the parachute — with **no stick input at all**. The whole body frame rotates under the player, since the view is composed on the body frame (`body × cockpit`).

The mod's existing airborne suppression (`movement.suppress_air_aim_facing`) could not reach it: that hooks `UpdateFallSteering`, which governs the jump ascent and the fall, but the parachute is a separate movement core that never routes through that helper (see [engine/gameplay/parachute-locomotion.md](../../engine/gameplay/parachute-locomotion.md)).

## The fix

A single detour on `GetParachuteSteeringValues`. After the original runs, while the head is decoupled (VR) and the toggle is on, the look-steer contribution is removed:

- **`steering_values_out`** — the camera look-steer had mixed `m_XLookYawPitchRollAmount[axis]·look.x + m_YLookYawPitchRollAmount[axis]·look.y` into each axis; that exact amount is subtracted, leaving the stick input and the velocity-alignment intact.
- **`look_steer_out`** — zeroed. The caller (`UpdateParachutePhysics`) otherwise feeds this into the yaw/pitch velocity springs (`m_RotateCharYawToVelocity`/`m_RotateCharPitchToVelocity`), so leaving it would still let the head steer the springs even with the steering values cleaned.

Gated on the same `head_decoupled` predicate as the other VR body-turn suppressions: under the VR source it is always true for the local player, and on flatscreen it only applies in the decoupled-idle-not-aiming window. NPC parachutes are untouched (the game's look-steer block already skips non-players, and the hook only rewrites the local player's outputs).

## The line turn

The look steer is also what turns the canopy toward a grapple line in the game: the slingshot pulls only along the canopy's facing and its own turn toward the line is switched off ([engine: parachute-locomotion.md, "The slingshot"](../../engine/gameplay/parachute-locomotion.md#the-slingshot)), so in the game the player looks where they fire and the canopy follows the camera round. With the head's look steer removed, a line fired beside the player added a little speed and no turn (10 November take, t 1391.65).

So while the local player's hook reels in under the canopy ([`GHS_REELING_IN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookState##variant-GHS_REELING_IN)), the detour puts back the look steer a camera looking along the line would give (`line_turn` in `hooks/input/parachute.rs`): the yaw from the canopy's wanted heading to the line's ([`GrapplingHook::GetReelingInDirection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetReelingInDirection)), shaped by the game's own look-steer dead zone, maximum yaw, exponent, and maximum speed, written into `look_steer_out` and mixed into the steering values as the game mixes the camera's. The game's springs then turn the canopy, a turn rather than a snap, and the slingshot's pull grows as the facing comes round to the line. Like the game's block, it gives nothing while the stick steers across. The pitch half is left out, so the line does not pitch the canopy. A grapple pull is the movement action behind the turn.

The switch and the strength (a share of the game's turn, 1.0 by default) are `controllers.vehicles.canopy.line_turn` and `line_turn_scale`, in the debug UI's Air feature, whose live status counts the ticks it turned (`line-turn ticks`).

## The opening's turn toward the velocity

The game's steering ends by turning the wanted yaw toward the horizontal velocity's heading through a spring (the air control settings' `m_RotateCharYawToVelocity`), weighted by a blend that the closed canopy holds at one, and counting the heading error only above the settings' `m_VelocityAlignMinSpeed` ([`GetParachuteSteeringValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::GetParachuteSteeringValues)). An opening out of a fall is shielded from it by its transition state; an opening out of a reel is not, so the alignment runs at full strength from the canopy's first update. The reel's speed falls through zero and overshoots backward as the canopy opens, which reads as a heading error of half a turn: on the 14 November take the canopy turned Rico 39 degrees left in a quarter of a second at 1459.4 s, with no stick input (a strong hypothesis rather than a proof). For the head-decoupled local player the steering is called with the spring's speed at zero, which stops both its integration and its turn, for the first 0.6 s of the canopy (a gap of a quarter of a second in the steering's calls marks an opening), while the horizontal speed is below 3 m/s, and while the velocity points more than about 100 degrees from the facing (`hooks::input::parachute::yaw_alignment_held`). The look-steer removal and the line turn read the game's own settings, which differ only in that spring.

## Validation

The debug UI's Air feature shows `Parachute: look-steer suppressions` in its live status. In-headset checks: deploy the chute, hold the stick at rest, and turn the head past 90° — the chute should fly straight; the stick should still steer; and the velocity banking and the drift-toward-velocity alignment should still work. The suppression counter advances while the chute is deployed in VR and its output is rewritten; it stays put on flat ground.

## Headset checklist

1. Under the canopy, fire the grapple at a point 60° to 90° to one side and hold nothing on the stick: the canopy turns toward the line over a second or so, without a snap, and picks up speed as it comes round. The Air feature's `line-turn ticks` counts up while the line reels.
2. Fire at a point straight ahead: the canopy flies on without turning.
