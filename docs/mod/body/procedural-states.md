# The procedural body outside standing play

The procedural body ([procedural-body.md](procedural-body.md)) builds Rico's body under the player's head while he is on foot. In the other states the game's animation carries the body: the seat's pose, the swim, the harness under the parachute, and the wingsuit's flight pose are authored, and they are what the vehicle, the water, and the air move. This doc covers what the procedural body takes from them, region by region, and what it leaves. The code is in `payload/src/controllers/body/procedural/`, alongside the standing body's.

## The torso on the animation's hips

Where the body is not standing on its own legs, the hips belong to whatever carries Rico: the seat, the water, the harness. The standing body places the hips from the head, which would lift Rico out of the seat, so these cases use a second solve (`solver::anchored`, pure and unit-tested) that keeps the hips and the clip's posture and moves the spine from there by the player's own motion:

1. The chest twists toward the player's torso, by the torso's turn against Rico's facing ([alignment.md](alignment.md)), within `chest_turn_limit_deg`, about the clip's own spine and shared out up it as on foot. A spine far from upright takes less of the twist, and none lying flat, since the torso's heading is a turn about the vertical.
2. The spine swings so the neck reaches the point under the player's head, shared out along the four spine bones and repeated to converge, never past `lean_limit_deg` from the clip's own spine. A lean in the seat bends the back; the hips stay where the clip put them.
3. A look down pitches the chest forward by a share of it, as on foot, again by how upright the spine is, and the spine turns back onto the neck's line about its base.
4. The neck takes a share of the head's turn, which the head override composes onto the clip's head, so the neck's share is a share of that turn.

The clip's spine is not settled toward the reference pose first, as the standing body's is: a bike's lean into a turn and a driver's reach for the wheel stay, and the player's motion is added to them. A player whose head is where the clip has Rico's leaves the clip's pose exactly as it was. The clavicles then turn toward the hands as on foot, the hand IK queues the wrist targets at the gate against the written torso (kept out of it, with no pull, so the solve cannot bend the torso toward a hand), and the game's IK targets on the torso's bones fade by its weight ([procedural-body.md, "The arms"](procedural-body.md#the-arms)). The elbow hints point down and back from the chest, so they are queued only while the written chest is near upright.

The view's anchor in these states is the animated head ([head-and-body.md](head-and-body.md)), captured before the gate writes the pose, so the head the solve reaches for is the clip's head moved by the player's tracked motion, with no feedback from the written spine.

A switch between the two solves while the body stays procedural would move the hips at once, so the last pose written before it is held and faded into the new one over `blend_s` (`solver::pose::SolveSwitch`). Coming from the animation needs no such fade: the region blend covers it. The first gate of each case that poses the torso this way logs `procedural body: the torso follows the head on the animation's hips` at info level, with the case, both weights, and the neck's distance from its target.

## Seated in a vehicle

Seated and riding, the torso is procedural on the seat's hips and the legs keep the seat's animation (`seated_torso`, on by default; `TORSO IN VEHICLES` on the `VR: BODY` page). The hand-over case is `seated in a vehicle`; it needs the hand tiers' classification of the seat as riding ([`Character::IsInRidingInVehicleState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInRidingInVehicleState), read by the input tick, so it lags by up to a frame), which an entry, an exit, a seat switch, and a mounted gun are not, and no stunt position. Those stay the vehicle's animation, and the torso blends between the two as the seat's state changes, so an entry blends the seat in after the entry's clip and an exit hands back before it.

The legs stay the seat's: the clip puts the feet on the pedals or the footrests and moves them with the vehicle's own animation (a bike's lean, the steering's turn of the hips), and nothing the player does with their own legs is tracked to replace it. The arms are the controllers' through the hand IK, on the wheel, the bars, or a held control ([steering-wheel.md](../input/steering-wheel.md)), and the clavicles reach with them. The driving view's correction moves the head the solve reaches for as it moves the view, so Rico leans by it as a player leaning the same distance would ([head-and-body.md](head-and-body.md)).

The body IK's torso lean, which posed the torso in a seat before, stands down once the procedural torso has its full weight, as on foot. With `TORSO IN VEHICLES` off the seat's animation and the body IK's lean pose the torso as before.

The scenario signals `procedural.seated` and `procedural.neck_error_m` read the same, and `testing/scenarios/seated_torso.toml` is the template for a recorded check ([recorder.md](../testing/recorder.md)). The `VR: STATUS` page's `SEATED TORSO` row shows the torso's weight and the neck's distance from the point under the player's head; it is `OK` at full weight once a switch has faded, and `NOT SEATED` outside a seat. The Controllers tab's procedural body section says whether the last pose was the torso on the animation's hips or the body under the head, and whether a switch is still fading.

## What needs confirming in the headset

1. Seated in a car: the log says the case changes to `seated in a vehicle` after the entry's clip, then `procedural body: the torso follows the head on the animation's hips`, and `SEATED TORSO` reads `OK` with the neck within a few centimetres. Lean forward, back, and to each side: Rico's back bends with you and his hips stay on the seat; the view does not dip into the chest. Turn your torso (both hands off the wheel): the chest turns, within its limit. Look down at the lap: the chest leans a little over it. The hands stay on the wheel while you lean. With `TORSO IN VEHICLES` off, the seat's animation and the body IK's lean pose the torso as before.
2. On a motorcycle, ride through turns: Rico's lean with the bike stays, the feet stay on the footrests, and leaning your head adds to it rather than fighting it. Report a jerk at the start or end of a turn.
3. Enter and leave vehicles, switch seats, and take a mounted gun: each logs the case `vehicle` while it plays, the torso blends without a pop, and a stunt position on the roof stays the animation's. In a helicopter and a plane, the torso follows the head the same way.
4. Get shot or thrown out of a seat: the ragdoll hands over at once, as on foot.
