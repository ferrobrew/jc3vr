# The physics substrate

The hands, the weapons, the grapple, and the vehicle handling reach the physics world through one layer, `jc3vr_body::controllers::physics`. Every helper fails with one error, `PhysicsError`, whose message starts `physics:`.

- `bodies` reads a dynamic body's pose, velocities, and mass after checking that it still belongs to the object it was taken from, sets its velocities (`drive`), and applies impulses (`shove`, and `push` for a body a ray hit this tick). The writes go through [`hknpWorld::applyBodyImpulseAt`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-applyBodyImpulseAt) and [`hknpWorld::setBodyVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-setBodyVelocity) in game-space points.
- `proxy` reads and drives the local player's proxy body; `hold` writes it as supported on a moving body, which is how the vehicle rail carries Rico on a helicopter's skid.
- `vehicles` finds the vehicle behind a hit object or body, including a loosened part's.
- `scan` walks the dynamic bodies by their broad-phase boxes, all at once or a slice per step.

## The rig's jumps

A snap turn, a hard view step through a vehicle's entry or exit ([rig.md](rig.md#the-body-turning-under-the-view)), a recenter, and a follow bank move every palm in the rig at once without the hands moving. The mod makes all four, so each is counted rather than inferred: `headpose::xr::rig_turn_count` counts one-tick turns of the body frame and `vr::rebase_count` counts re-bases of the cockpit frame. `rig::RigWatch` reports, per reader, `Rebase`, `Turn`, `Unannounced` (the facing turned past `rig.unannounced_turn_rad` with no event: a respawn, cutscene, or teleport), or `Settling` for the tick after one of the mod's events, which covers a reader on another thread taking the new count with the old facing. The threshold is a quarter turn per tick, wider than any smooth turn.

The hand bodies, the climb, and the grapple's pump restart their motion on any jump. For release velocities, a re-base or unannounced turn cuts the hand history, while a turn or its settling tick only turns the body frame.

## The wanted velocity

The engine moves the proxy by the wanted velocity its movement task writes each simulation tick. The mod's writers are `wanted::Contributor`s applied through one `wanted::Composition` in a fixed order, each reading the velocity the previous one left:

1. Before the world update: the vehicle rail's entry or step-off, and the grapple lock's release and handback. The movement task carries these into its tick.
2. In the character's update, exactly one movement task runs. On foot, the locomotion post-call composes the stick's velocity ([locomotion.md](locomotion.md)), adds the palms' push ([physical-hands.md](../input/physical-hands.md#pushing-off-surfaces)), and, while the player owns the root, adds and caps the step drive ([rig.md](rig.md#who-moves-the-root)). Swimming, the strokes add speed along the facing ([swimming.md](swimming.md#arm-strokes)). Reeling, the reel's arrival hands on the grapple lock's velocity ([grapple.md, "The lock"](../grapple/grapple.md#the-lock)).

A contributor applied out of order is a programming error and stops a debug build.

## Release velocities

Everything a hand lets go of (a prop, a ragdoll, a gun, a grenade, a C4 charge) leaves through `physics::release`, so the same swing gives the same motion.

The hand's share is the peak of the controller's history since the hand took the item (`jc3vr_input::motion::release_velocity`), carried rigidly to the released point (a prop's centre of mass, a gun's world point, a grenade's centre); the spin is the hand's angular velocity. The player's own movement, sampled once per input tick by `release::sample`, is added: the character's velocity plus the rig's turn crossed with the offset to the point. Only the hand's share is capped (`release.max_hand_mps`), so a throw from a moving car keeps the car's speed; there is no cap by mass. In slow motion the hand's share per real second becomes the same motion per game second ([slow-motion.md](../input/slow-motion.md#throws)).

Without fresh rig motion a release has none: a gun drops at rest and a prop keeps its velocity. An object one hand cannot lift keeps the velocity the hold gave it. A drop that is not a throw uses a hand cap of zero. The climb's fling launches the player rather than an object, so it keeps the climb's own estimate and shares only the shaping ([hand-climbing.md](../input/hand-climbing.md)).
