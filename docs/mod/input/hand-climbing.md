# Hand climbing

An empty hand whose body touches the static world grips it with the grip, and while it grips, the player's body moves against the hand's motion: pulling a gripping hand down lifts the body, pushing it away from a wall pulls the body toward the wall, and with both hands gripping the body follows their average. Letting go keeps the climbing speed, so a hard pull flings the player up and over. Near the top of a ledge whose top the hands grip, the mantle carries the body over it and lets go once the player stands there. The code is `payload/src/controllers/physical_hands/climb/`; the engine side is in [hands-locomotion-and-state.md, §4.7](../../engine/character/hands-locomotion-and-state.md#47-the-proxys-support-and-the-fall). It needs the hand bodies ([physical-hands.md](physical-hands.md#hand-bodies)) and is off by default (`physical_hands.climb.enabled`, `CLIMB WITH THE HANDS` on the pause menu's `VR: CONTROLS` page), until it has been seen working in the headset.

## Gripping

Each tick, the palms' tick casts the pickup's reach rays ([physical-hands.md](physical-hands.md#picking-things-up)) from the body of each hand its body drives, where it was before the last step, not from the controller, out to `climb.reach_m` past the palm, and keeps the nearest hit on a static body ([`hknpBody::IS_STATIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody)): the static world, not a prop, a vehicle, a character, or a moving platform. A loose object in reach wins over it, so a grip near a crate on a ledge takes the crate.

A hand with a static surface in reach, or whose body presses into something that holds it back, and no grip reports the climb's grab volume ([`GrabVolume::Climb`](../../../payload/src/controllers/bindings/grab.rs)) to the bindings, after the pickup's, so every placed volume and the pickup volume win over it. As with every grab volume, the grip press is latched at its edge, so it does not fire the grapple: a grip pressed against a wall that no reach ray finds grips nothing, but does not grapple either. The input tick asks the next physics step to grip, and a released grip asks it to let go.

The step takes the grip only for a hand whose body acts, on a body still static. It remembers where the player's body and the palm were and pins the hand's body where it is: from then on the body's goal is that pin, not the controller, so the rendered hand stays on the wall while the controller moves. The hand pulses `climb.grab_pulse`, and `climb.release_pulse` when it lets go; a release that flings pulses `climb.fling_pulse` in proportion to the fling's speed against `climb.max_fling_speed_mps`. While the hands move the body, each gripping hand pulses `climb.tension_pulse` every tick in proportion to the climbing speed, full at `climb.tension_full_speed_mps`, so a hard pull is felt in the hands.

## Climbing

While a hand grips, the hand bodies' pre-simulation step moves the player. The palms' tick publishes each palm in the rig: the controller's tracking-space place, turned by the body's facing and scaled by the VR world scale into world metres, but not carried by the head's world position. Each gripping hand would have the body where it was at the grip, moved back by how far the palm has since moved in the rig, and the hands' targets are averaged. This is the classic climbing locomotion: the body moves opposite to the controller relative to the grip. The target depends only on the controllers, not on where the body or the view is now, so the climb is a closed loop on the body's position: a palm composed into the world from the head, whose place the root-fixed view smooths and publishes a tick late, would chase the body it moves and could overshoot without end. A recenter or a snap turn (a turn of the body's facing of more than a fifth of a radian between two ticks) moves every palm in the rig at once without the hands moving, so it re-takes the grips from where the body is then. The step gives the player's proxy the velocity that closes `climb.gain` of the gap to that place within the step, capped at `climb.max_speed_mps`, through the proxy body's own velocity setter ([`hknpCharacterRigidBody::setLinearVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpCharacterRigidBody##func-setLinearVelocity)), as the proxy's update does. The callback runs after every character's proxy update, so that velocity replaces the one the game computed, gravity and the stick's included, for the step. The step's collisions still apply: a body pulled into a wall stops at it, and a body pulled up a wall slides along it.

A gap past `climb.break_distance_m`, a hand pulled much further than the body can follow, lets every grip go. A gripped body that is gone or no longer static lets its grip go, since the step writes it as the proxy's ground every step.

## Keeping the game on foot

Hanging from a wall, the proxy has no support, so the game would start a fall: the falling states, their animations, the fall damage, and the parachute and wingsuit. The game decides from the proxy's support ([`PfxCharacterInstance::m_GroundInformation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundInformation)), which the proxy's update finds before the physics step and which the state machines read on the next tick before the proxy's next update. So while a hand grips, the step also writes the proxy as supported on the gripped surface: supported, level, at rest, with the gripped static body as the ground body ([`m_GroundBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundBodyId)). Every reader until the next proxy update sees a character standing: the ground-contact condition, the fall bookkeeping, and the ground raycasts, which then clear the fall distance instead of measuring it. The game's own idea of the player stays on foot on the ground, so the hands keep acting, the stick locomotion's root-fixed view stays anchored ([locomotion.md](../body/locomotion.md#the-steady-view)), and nothing lands when the climb ends. The proxy's update finds the real support again each tick before the write, so the write never outlives the grip by more than a tick.

That this keeps the player out of every falling state rests on the readers the disassembly shows; which conditions Rico's state machine uses to start a fall is not established from the shipped data. The readout and the log say what the game decided (see below).

## Letting go

The climbing velocity is smoothed over `climb.fling_smoothing_s`. When the last grip lets go with the grip, the proxy keeps `climb.fling_scale` of it, capped at `climb.max_fling_speed_mps` (`climb.fling`), and the game takes the player from there: a fling up a wall rises and falls, and a fling over a ledge lands on it. A grip that ends for any other reason (below) does not fling. The game may take the velocity back on the next steps, before its own air handling has the player in the air, so for `climb.fling_carry_s` after the release the step keeps giving the proxy the fling's velocity as a throw under gravity (`pickup.gravity_mps2`), until the proxy finds support or a hand grips again; whether the game keeps the fling without this is unverified.

## Mantling

A gripped surface whose normal is at least `climb.min_ledge_normal_y` upward is a ledge's top, unless the player stood on something when the hand took hold and the surface was no more than `climb.mantle_start_m` above the feet then: the floor, or a table gripped while standing, is never a ledge. With `climb.mantle`, the mantle takes over from the hands once the feet come within `climb.mantle_start_m` below the highest ledge gripped, while the hands pull the body up steadily (the smoothed climbing speed at least `climb.mantle_min_speed_mps` upward) and the feet are off what they stood on, or have climbed `climb.mantle_min_climb_m` since the climb started. It lifts the feet straight up to `climb.mantle_clearance_m` above the grip, then carries the body over it to `climb.mantle_forward_m` past the grip, at up to `climb.mantle_speed_mps`, keeping the support written all the way; once the feet are `climb.mantle_land_past_m` past the top, about the capsule's radius, it lets them down onto it as they go on. As soon as the proxy's own update finds it standing (supported on a surface at least that upward) with the feet near the grip's height, or the feet reach where the mantle carries them, every grip lets go without a fling and the player stands on the ledge, still in the on-foot states the climb kept, so no landing plays. A mantle that has not landed within three seconds gives up and lets go.

## The body

With the procedural body ([procedural-body.md](../body/procedural-body.md#the-hand-over-table)), climbing is a case of its own in the hand-over table: the torso stays procedural, and the legs hang in the air under the hips as in a jump (`air_legs`), tucking while the body rises. The arms reach the gripping hands' bodies, which stay where they took hold, so Rico's hands stay on the wall. A hit reaction, a vehicle, and the other cases above it in the table still win. When the climb ends on the ground or on a ledge, the feet are planted afresh where the stance wants them, with no landing dip; when it ends in the air, the game's own fall takes the legs as after a jump.

## When the grips let go

Besides the grip itself, every grip lets go when climbing is switched off, the player is teleported, the palms' tick stops publishing for a few steps (the hands stand down: a drawn gun, a vehicle, a lost controller), the player has no proxy, or the body could not follow the hands. One hand's grip lets go when its body stops acting for more than a few steps; a flicker of the mode or a dropped tracking sample keeps it. The hands act while the player falls on foot, so a hand flung at a ledge or jumped at a wall can grip it, and the written support then keeps the game on foot. The game's pause stops the steps and the palms' tick alike, so a grip holds through it.

## Threading

The grips are taken and ended, and the player moved, only in the hand bodies' pre-simulation callback, on the game thread outside the step and after the characters' proxy updates ([threading.md](../threading.md)). The palms' tick (the player aim's post-simulation update, on the game thread) publishes the palms, the reach, and the player's place, and pulses the hands for the grips the step took or ended; the input tick asks for grips and releases. They share the climb's own leaf lock, never the VR runtime's, and the step reads the hand bodies' poses before it is called, so the climb never takes the hand bodies' lock.

## Settings and readouts

The settings are `physical_hands.climb`, under "Hand climbing" in the debug UI's Controllers tab, with the readout beside the hand bodies': whether the hands climb or mantle, the velocity given, the gap to where the hands would have the body, whether the game judges the player in the air ([`Character::IsInAir`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInAir)), the counts of grips, releases, mantles, and dropped grips with the last reason, the last fling's speed, and for each hand what it reaches and grips.

The log has, at info level, `hand climbing: a hand gripped the world` with the body, point, and normal, `climbing starts` with the feet, `mantling over a ledge`, `mantled onto the ledge`, `let go of the world` with the fling, `a grip was let go` with the reason, and `climbing ends`. Once a second while climbing, `hand climbing: climb` at debug level gives the velocity, the gap, the gripping hands, whether the mantle runs, and whether the proxy stood on its own. If the game ever leaves on foot on the ground while a hand grips, it warns `hand climbing: the game left on foot on the ground while a hand gripped the world` with the mode and whether it judged the player in the air.

## Headset checklist

With `CLIMB WITH THE HANDS` on:

1. Put a hand flat on a wall: the readout says it reaches the wall's body. Grip: `a hand gripped the world`, a pulse, and the rendered hand stays on the wall while the controller moves.
2. Pull the gripping hand down: the body rises, the view with it, and the readout's game verdict stays `on the ground`. No `the game left on foot` warning appears, no falling or parachute animation plays, and there is no fall damage. Report the mode and the verdict if the warning does appear.
3. Hand over hand up a wall: each new grip takes over smoothly, and the body follows the average of two gripping hands.
4. Let go while pulling hard: `let go of the world` with a fling speed, and the player flies on and falls normally.
5. Climb to a ledge, grip its top with both hands, and push down: `mantling over a ledge`, the body rises over the edge and onto the top, then `mantled onto the ledge`, and the player stands there with no landing animation.
6. With `BODY FROM YOUR HEAD` on `FULL`: hanging logs the case `climbing with the hands`, the legs hang and tuck, and a mantle ends with the feet planted on the ledge.
7. Grip, then draw a gun with the other hand or enter a vehicle: the grip lets go with its reason.
8. Pause while hanging, resume: the grip still holds.

## Not yet done

- Pushing the body up with the hands: pressing down on a ledge's top, or a vault over a low wall, moves nothing, since the push off a held-back hand ([physical-hands.md](physical-hands.md#pushing-the-player)) is horizontal only and a press down is not a grip. A vertical push, or a vault that carries the body over a low top the hands press on, would build on the mantle.
- Gripping a moving body: only static bodies are gripped, so a hand cannot hang from a vehicle, a moving platform, or a swinging prop. Following one would need the grip held in the body's own frame and the body's motion added to the climb.

## Unverified in the headset

- That writing the support keeps Rico out of every falling state; that no animation, sound, or AI reacts oddly to a character standing on a wall.
- That the velocity set after the proxy's update is the one the step integrates with no sag, and how the gap-closing gain feels against the 30 Hz step.
- Whether the root-fixed view's height smoothing lags a fast climb.
- That the mantle clears real ledges with its defaults, and that the proxy's own support check finds the top so the mantle ends.
- How the fling feels, and whether the game's in-air state keeps its horizontal part.
- That the reach rays find the static world reliably, and that walls and ledges are static bodies rather than keyframed ones.
