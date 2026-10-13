# The wingsuit's stall and flip

The wingsuit's animation-graph blend state ([`WingsuitBlendState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState)) blends the flight samplers each frame and also decides whether the character stalls ([`UpdateSamplers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState##func-UpdateSamplers)). Nothing else in the wingsuit decides it: there is no state bit, act, or sub-state of the wingsuit task behind the stall.

## Entering and leaving

The blend state measures the character's forward airspeed, its measured velocity against its forward axis. It enters the stall when that falls below [`WingsuitBlendState_ToStallSpeed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState_ToStallSpeed) with the nose raised past [`WingsuitBlendState_StallNoseUp`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState_StallNoseUp), and leaves it once the airspeed recovers past a higher threshold. While a stall clip already has weight, the nose test is skipped. The decision lands in a local flag, set by a single instruction ([`WingsuitBlendState_StallFlagSet`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::WingsuitBlendState_StallFlagSet)) and cleared on every other path.

## The two clips

While stalling, inside the stall clip's selection segment, the blend state ramps up one of two dedicated clips: the flip, a full loop that recovers from the stall, when the nose is raised further, and the plain stall otherwise. It sets the character's in-flip flag while the flip plays, and scales every other sampler down by the stall's weight, so the stall replaces the flight blend rather than adding to it.

The stall's progression through its turn segment is read in four places: the air physics skips part of its steering while it runs, the air brake is allowed only outside it, the wingsuit task queues its close act only outside it, and the boost reads the in-flip flag (unverified: read from the symbol dump).

## What moves

Neither clip moves the physics body. The air physics ([`NAirMovement_UpdateAirPhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-NAirMovement_UpdateAirPhysics)) clamps the body's pitch and orients it from the steering, so the loop is the clip's own rotation of the pose. No velocity damping is keyed to the stall either: the speed is already lost before the stall begins, to the air physics' ordinary drag, lift, and gravity. Without the stall, a character that keeps its nose raised loses lift and drops, and the body is pulled toward its velocity (unverified: the exact gating of that pull in this build).
