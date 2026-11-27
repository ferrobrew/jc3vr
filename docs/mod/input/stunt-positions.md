# Vehicle grips and stunt positions

A grip on a vehicle or helicopter stays at the point the hand grips and carries the player through the moving climb ([hand-climbing.md](hand-climbing.md#moving-bodies)); it never enters the game's animated helicopter hang or snaps onto a roof, and the other hand stays free for a gun. The code is `crates/jc3vr-physical/src/controllers/physical_hands/climb/` and `physical_hands/stunt/`, always on under the controller scheme. The engine side is [stunt-positions.md](../../engine/gameplay/stunt-positions.md).

## Native automatic attachment

The climb writes the gripped body as the proxy's support so the player does not fall, and the game can read that as a vehicle below and call [`Character::AutoAttachToStuntAreaOnVehicleBelow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-AutoAttachToStuntAreaOnVehicleBelow), snapping the capsule to a stunt area. A detour refuses that entry only for the local character with a fresh physical-grip proof.

The detour runs on a worker thread, so it reads only an immutable proof published by the pre-simulation step, a generation, atomic destruction stamps, and a deadline: no world or body-storage read and no climbing lock. A change of player, proxy, world, or teleport epoch advances the generation, and the destruction hook advances a body's stamp before it goes, so a stale grip can only be conservatively dropped, never kept through destruction.

## Completed roof mantle

A deliberate mantle captures the ledge's owner and follows its frame. On completion the input tick revalidates the landing, and [`Character::IsThisVehicleBelow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsThisVehicleBelow) must confirm the same vehicle; a short-lived claim keeps automatic attachment from picking another vehicle first, and only the owner-checked deliberate entry bypasses the gate. Once attached, the game owns movement: only a fresh native not-attached observation admits climbing, hand push, or vaulting, which otherwise yield without writing a competing velocity.

## Release velocity

The release is the inherited point velocity (with the body's spin at the gripped point) plus the capped difference between the smoothed total and inherited velocities, so the fling cap limits only hand-generated motion and turning the fling off keeps the vehicle's motion. A changed grip set resets both filters, and destruction, stale input, or a teleport drops the grip with no fling.

## Normal helicopter hang

The game's helicopter hang holds Rico inverted under the airframe, which the player's body cannot match. A reel onto a helicopter hangs Rico upright on a line instead ([grapple-lock.md](grapple-lock.md#the-hang-below-a-ceiling-or-a-helicopter)), and a helicopter seat's stunt press is the seat's exit (`jc3vr_gameplay::controllers::bindings::bail::stunt_hangs_inverted`). A script's attach ([`Character::AutoAttachToClosestStuntAreaOnVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-AutoAttachToClosestStuntAreaOnVehicle)) is left to the game, since a mission may wait on the hang; in a native hang, opening every grip requests the release.

The debug UI's Climbing feature has the `Moving climbs` and `Stunt positions` rows, and on a roof the `Roof torso` row reports the stunt clip modifiers dropped there.
