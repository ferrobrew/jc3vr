# Hand climbing

An empty hand whose body touches the static world, or a large enough moving body, grips it, and while it grips the player's body moves against the hand's motion: pulling down lifts the body, and with both hands the body follows their average. Letting go keeps the climbing speed, so a hard pull flings the player up and over, and near the top of a gripped ledge the mantle carries the body over.

The code is `crates/jc3vr-physical/src/controllers/physical_hands/climb/`, tuned in the `climb` section of `climb.toml`; it needs the hand bodies ([physical-hands.md](physical-hands.md#hand-bodies)). The engine side is in [root-motion-and-proxy.md, "The proxy's support and the fall"](../../engine/character/root-motion-and-proxy.md#the-proxys-support-and-the-fall).

## Gripping

The palms' tick casts the pickup's reach rays ([physical-hands.md](physical-hands.md#picking-things-up)) from each driven hand's body and keeps the nearest climbable hit: a static body ([`hknpBody::IS_STATIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody)), a keyframed one such as a moving platform, or a dynamic one heavy enough, such as a vehicle or a helicopter's skids; never a character. It is offered to the grip as a fallback (`GrabVolume::Climb`, [grip.md](grip.md)) that every other target beats, so a grip near a crate on a ledge takes the crate and one on a closed door opens it ([vehicle-doors.md](vehicle-doors.md)). The next physics step takes the grip and pins the hand's body where it is, so the rendered hand stays on the wall while the controller moves. The cues are `Event::ClimbGrip`, `Event::ClimbRelease`, `Event::ClimbFling`, and `Sustain::ClimbStrain` ([haptics.md](haptics.md)).

## Climbing

The palms' tick publishes each palm in the rig: its tracking-space place turned by the body's facing and scaled to world metres, but not carried by the head's world position. Each gripping hand wants the body where it was at the grip, moved back by how far the palm has moved in the rig since, and the hands' targets are averaged. The target depends only on the controllers, because a palm composed from the head, smoothed and a tick late, would chase the body it moves and overshoot. A rig jump ([physics-substrate.md, "The rig's jumps"](../body/physics-substrate.md#the-rigs-jumps)) re-takes the grips from where the body is.

The step gives the proxy the velocity that closes a share of the gap, capped, through [`hknpCharacterRigidBody::setLinearVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpCharacterRigidBody##func-setLinearVelocity). The callback runs after every proxy update, so this replaces the game's velocity, gravity and stick included; collisions still apply. On a face that leans back, a lift also carries the body toward the face by the slope (`solve::slope_follow`), so Rico rises along the rock rather than away from it. A gap past the break distance, or a gripped body that is gone or no longer climbable, lets the grips go.

## Moving bodies

A grip on a moving body is held in that body's frame from the grip on ([item-pickup.md](../../engine/gameplay/item-pickup.md#a-body-in-the-havok-world)): each step carries the gripped point and the hand's pin with the body, and the velocity given adds the gripped point's own (capped), so a player hanging from a truck travels with it and climbs it as a wall. The support written names the moving body as the ground, and a release keeps the carry, so the player leaves a vehicle at its speed.

A vehicle grip stays at the actual handhold ([stunt-positions.md](stunt-positions.md)). While a grip owns locomotion, the climb refuses the game's automatic entry into a roof stunt area (`climb::native`); only a completed mantle onto the vehicle hands over to its roof stunt position.

## Keeping the game on foot

Hanging from a wall, the proxy has no support, so the game would start a fall. It decides from the proxy's support ([`PfxCharacterInstance::m_GroundInformation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundInformation)), found by the proxy update before the step and read on the next tick. So while a hand grips, the step writes the proxy as supported, level, on the gripped body ([`m_GroundBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundBodyId)). Every reader until the next proxy update (the ground-contact condition, the fall bookkeeping, the ground raycasts) sees a standing character, so the hands keep acting, the root-fixed view stays anchored ([locomotion.md](../body/locomotion.md#the-steady-view)), and nothing lands when the climb ends; the write never outlives the grip by more than a tick. Which conditions Rico's state machine uses to start a fall is not established from the data, so the log warns if the game ever leaves on foot on the ground while a hand grips.

## Letting go and mantling

When the last grip opens, the proxy keeps the smoothed carried velocity plus a share of the relative one, capped; a grip that ends any other way does not fling. The game may take the velocity back before its air handling starts, so for a moment after release the step keeps applying the fling as a throw under gravity, until the proxy finds support or a hand grips again.

A gripped surface facing up is a ledge's top, unless the player stood when gripping and it was near the feet (a floor or a table is never a ledge). The mantle takes over once the feet come near the highest ledge gripped while the hands pull up steadily: it lifts the feet clear of the grip, carries the body over, and lets it down, releasing every grip once the proxy stands on the ledge, still on foot, so no landing plays.

## The body

Climbing is a case of its own in the procedural torso's hand-over table ([procedural-body.md](../body/procedural-body.md#the-hand-over-table)): the torso stays procedural on the animation's hips, the IK takes the legs, each foot bracing on the face or hanging ([ik-body.md](../body/ik-body.md#feet-on-the-face)), and the arms reach the pinned hands. The rig does not step while a hand grips ([rig.md, "Who moves the root"](../body/rig.md#who-moves-the-root)). A climb that ends in the air hands over to the game's own fall.

## Releases and threading

Every grip lets go on a teleport, when the palms' tick or the hand input goes stale (the hands stand down: a drawn gun, a vehicle, a lost controller), without a proxy, and when the body could not follow; one hand's grip survives a brief flicker of its body. The game's pause stops the steps and the palms' tick alike, so a grip holds through it. Grips are taken and ended, and the player moved, only in the hand bodies' pre-simulation callback after the proxy updates ([threading.md](../threading.md)), under the climb's own leaf lock, never the hand bodies'.

## Outstanding checks

- That writing the support keeps Rico out of every falling state, and nothing reacts oddly to a character standing on a wall.
- Whether the game keeps a fling's horizontal part without the carried throw.
- Whether walls and ledges are static rather than keyframed, and whether the mantle clears real ledges.
- Whether the game accepts a fast-moving support, and whether the climb's pull shoves a light vehicle.
