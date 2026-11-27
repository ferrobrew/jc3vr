# Parachute head-yaw suppression

The game's parachute turns toward where the camera looks, and in VR the camera follows the headset, so the canopy would turn with the player's head. The mod removes the head's part of the steering and turns the canopy toward a grapple line instead. The engine-side mechanism is [engine/gameplay/parachute-locomotion.md](../../engine/gameplay/parachute-locomotion.md); the implementation is `crates/jc3vr-body/src/hooks/input/parachute.rs`, and its tuning is the `parachute` section of the input tuning (`crates/jc3vr-body/tuning/input.toml`, [tuning.md](../tuning.md)).

## The problem

The parachute steering helper [`GetParachuteSteeringValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::GetParachuteSteeringValues) mixes the camera input matrix into the canopy's steering. In VR the camera follows the headset, so turning the head past the look-steer dead zone (`m_LookSteerDeadZone`, shaped by `m_LookSteerMaxYaw` and `m_LookSteerExponential`) turns the parachute with no stick input at all, and the whole body frame rotates under the player, since the view is composed on the body frame (`body × cockpit`).

The jump and fall detours in `hooks::input::locomotion` do not reach it: they hook `UpdateFallSteering`, which governs the jump's ascent and the fall, but the parachute is a separate movement core that never routes through that helper.

## The look-steer removal

A single detour on `GetParachuteSteeringValues` acts after the original runs, while the head is decoupled:

- `steering_values_out`: the camera's look steer was mixed into each axis as the look input scaled by the settings' look yaw, pitch, and roll amounts; that exact amount is subtracted, leaving the stick input and the velocity alignment intact.
- `look_steer_out`: zeroed. The caller, `UpdateParachutePhysics`, feeds it into the yaw and pitch velocity springs, so leaving it would still let the head steer the springs.

It is gated on the same `head_decoupled` predicate as the other VR body-turn suppressions: under the VR source it is always true for the local player, and on flatscreen it applies only in the decoupled idle, not aiming. NPC parachutes are untouched: the game's look-steer block already skips non-players, and the detour only rewrites the local player's outputs.

## The line turn

The look steer is also what turns the canopy toward a grapple line in the game: the slingshot pulls only along the canopy's facing and its own turn toward the line is switched off ([engine: parachute-locomotion.md, "The slingshot"](../../engine/gameplay/parachute-locomotion.md#the-slingshot)), so in the game the player looks where they fire and the canopy follows the camera round. With the head's look steer removed, a line fired beside the player would add a little speed and no turn.

So while the local player's hook reels in under the canopy ([`GHS_REELING_IN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookState##variant-GHS_REELING_IN)), the detour puts back the look steer a camera looking along the line would give (`line_turn`): the yaw from the canopy's wanted heading to the line's ([`GrapplingHook::GetReelingInDirection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetReelingInDirection)), shaped by the game's own look-steer dead zone, maximum yaw, exponent, and maximum speed, written into `look_steer_out` and mixed into the steering values as the game mixes the camera's. The game's springs then turn the canopy, a turn rather than a snap, and the slingshot's pull grows as the facing comes round to the line. Like the game's block, it gives nothing while the stick steers across. The pitch half is left out, so the line does not pitch the canopy. The grapple pull is the movement action behind the turn, and it always acts at the game's full strength.

## The opening's turn toward the velocity

The game's steering ends by turning the wanted yaw toward the horizontal velocity's heading through a spring (the air control settings' `m_RotateCharYawToVelocity`), weighted by a blend that the closed canopy holds at one, and counting the heading error only above the settings' minimum alignment speed. An opening out of a fall is shielded from it by its transition state; an opening out of a reel is not, so the alignment runs at full strength from the canopy's first update. The reel's speed falls through zero and overshoots backward as the canopy opens, which reads as a heading error of half a turn and turns Rico sharply with no stick input (a strong hypothesis rather than a proof).

For the head-decoupled local player the steering is therefore called with the spring's speed at zero, which stops both its integration and its turn (`hooks::input::parachute::yaw_alignment_held`), while any of these holds:

- the canopy opened less than `parachute.align_hold_s` ago, where a gap of the freshness tuning's `hooks.parachute_align_gap_ms` in the steering's calls marks an opening;
- the horizontal speed is below `parachute.align_min_mps`;
- the velocity points more than `parachute.align_reversed_rad` from the facing.

The look-steer removal and the line turn read the game's own settings, which differ only in that spring.

## Validation

The debug UI's Air feature shows `Parachute: look-steer suppressions`, `line-turn ticks`, and `reel openings skipped` in its live status. The suppression counter advances while the canopy is deployed in VR and its output is rewritten, and stays put on the ground.

In the headset:

1. With the canopy deployed and the stick at rest, turning the head past a right angle leaves the canopy flying straight; the stick still steers, and the velocity banking and the drift toward the velocity still work.
2. Firing the grapple at a point 60 to 90 degrees to one side with nothing on the stick, the canopy turns toward the line over a second or so, without a snap, and picks up speed as it comes round. `line-turn ticks` counts up while the line reels.
3. Firing at a point straight ahead, the canopy flies on without turning.
4. Opening the canopy out of a reel does not turn Rico sharply in its first moments.
