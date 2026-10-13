# The body rig

The rig decides who moves Rico's root on each simulation tick, walks the root after the player's head in the standing posture, and leaves a lean over the feet alone. The camera stays on the player's real head throughout: the root follows the head, never the other way round. The code is in `payload/src/controllers/body/rig/`, and the settings are `controllers.body.rig`. The torso's heading still comes from the torso estimate ([alignment.md](alignment.md)), and the legs from the procedural body ([procedural-body.md](procedural-body.md)).

## Who moves the root

Several systems want the root: the game's own states, the hand climbing, the animation's authored motions, and the player's head. Exactly one owns it on each tick (`rig::root::owner`), chosen once in the root hook, a post-call detour on the on-foot locomotion task ([`NStateTask_MovementLocomotionTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_MovementLocomotionTask##func-Update)). The task writes the proxy's wanted velocity every on-foot tick, idle included, and velocity written there goes through the proxy's character context, which handles stairs, slopes, and support. The owners, in precedence order:

| Owner | When | The chase |
| --- | --- | --- |
| Game | The task did not write the velocity (the task-defined motion state in [`Character::m_CurrentMotionState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_CurrentMotionState) is not set, or Rico is attached to a vehicle), or a seat transition is under way. Every other state that moves the proxy (the grapple, jumps, stunts, ragdoll, swimming, the parachute, the wingsuit) runs a different task. | None |
| Climb | A hand grips the world. The climb replaces the proxy's motion in the pre-simulation callback. | None, and the ledger records nothing |
| Authored | The animation carries the body: an authored one-off, a hard landing, cover, or a hit reaction's hold ([procedural-body.md](procedural-body.md#the-hand-over-table)). The view rides its root motion. | None |
| Seated | The seated posture. | None |
| Player | Otherwise: on foot, standing, out of a seat. | Added |

On every tick the task wrote the velocity, the hook first replaces the clip's velocity with the stick's ([locomotion.md](locomotion.md)) and adds the palms' push off what they press into; together these are the task's movement. While the player owns the root, the chase velocity is added to it through the character's world-space accessors ([`Character::GetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-GetWantedWorldVelocity) and [`Character::SetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetWantedWorldVelocity)), because the proxy's own wanted velocity is character-local. Nothing scales or caps the sum but a sanity bound of 12 m/s, applied to both parts alike. The stick and the chase are independent: the stick moves the root and the camera together, so it never changes how far the root is from the head, and the chase does not yield to it. A player walking in the room while pushing the stick moves at the sum of the two.

The same hook steps the head-through-geometry probe and requests or releases the crouch capsule ([controllers.md, "Posture and roomscale"](../input/controllers.md#posture-and-roomscale)).

## The chase

### The target

The root walks toward the ground point under the player's upright eyes (`rig::root::chase::target`): the neck pivot, which sits below and behind the eyes in the head's frame, less the arm from the pivot to the eyes turned by the torso's heading. Nodding and turning the head swing the eyes about the pivot, so they move neither the pivot nor the target. The steady view stands the eyes of a neutral head directly over the root ([locomotion.md, "The steady view"](locomotion.md#the-steady-view)), so in the cockpit frame the root sits at the cockpit's horizontal origin and the target's cockpit position less the cockpit shift is the gap from the root.

### The gap and the velocity

The frame half runs once per rendered frame from the pose composition, after the simulation ticks. It folds the newly credited chase into the cockpit shift, measures the gap to the target, takes the lean out of it (below), and publishes the remaining gap in world metres together with the running total of credited chase the shift had folded in, and the head's velocity.

Each tick the player owns the root, the chase asks for

```
v_chase = clamp_len(feed_forward · v_head + gain · g_eff / dt, max_speed)
```

where `g_eff` is the published gap less the chase movement credited since the frame measured it. Two ticks that fall in one frame (the game below 30 fps) therefore never chase the same gap twice, and nothing accumulates between frames: every tick starts from the head as the latest frame saw it. `gain` (`chase.gain`, 0.4 by default) is the share of the gap closed per tick, a time constant of about 65 ms at the game's 30 Hz. There is no deadzone.

`v_head` is the target's own velocity in the cockpit frame, carried into the world, smoothed over 50 ms, with the first 5 cm/s taken off so tracking noise never drives a root at rest. It cancels the lag a pure proportional chase leaves behind a walking player (about 12 cm at 1.4 m/s). It is zero while the gap lies inside the lean, because a head moving within a lean moves nothing below it. `feed_forward` (`chase.feed_forward`) weights it, 1 by default, and `max_speed_mps` (`chase.max_speed_mps`) bounds the chase at 4 m/s by default. The gain and the feed-forward are debug UI tunables, because how the proxy's character context answers a wanted velocity has not been measured.

## The lean

A player leaning over a table or reaching for the floor moves the head out over feet that stay put. Without an allowance the chase would walk the capsule into the table, and the capsule's push-back would turn the lean into the world shoving the player. A lean from the hips lowers the neck pivot as it moves it out, so the pivot's drop below the player's standing height `d` bounds how far it may be out without a step (`rig::lean`):

```
A(d) = clamp(gain · (d − deadband), 0, max)
```

with `lean.gain` 4 m per metre of drop, `lean.deadband_m` 4 cm for a standing head's bob, and `lean.max_m` 0.35 m by default. The allowance is widest along the torso's forward: an ellipse `A` forward, `lean.side_share` (half) of it to either side, and `lean.back_share` (a quarter) of it behind. The part of the gap inside the ellipse is the lean, and the chase closes what lies outside it; a gap outside is scaled back along itself onto the ellipse.

The standing height is the neck pivot's height that nine in ten of the recent samples lie below, sampled while the player is on foot, standing, and looking about level, the same measure the procedural body uses ([procedural-body.md, "The player's head"](procedural-body.md#the-players-head)), and measured afresh after a floor jump. Nothing else is held between frames, so standing back up removes the allowance and the gap together, with no root motion and nothing to forget. A crouch drops the pivot straight down and leaves no gap for the allowance to absorb. Walking in the room crouched leaves at most `A` of trail, which the spine shows as a lean and which drains the moment the head rises.

The lean also places the player's feet for the rest of the mod: the target less the lean, in the cockpit frame (`rig::feet`), which the holsters and pouches hang the waist over, and the head the procedural body steps the feet under (`rig::lean_allowance`). With `lean.enabled` off the allowance is zero.

## The bookkeeping

The camera is the anchor carried by the root, plus the cockpit position less the cockpit shift, composed with the view's rotation and the world scale. When the chase walks the root toward the head, the anchor moves with the root while the cockpit position stays, so without a correction the camera would run ahead by the distance the root covered, and the gap would never close. The shift (`rig::root::shift`) takes the chase's part of the root's displacement back out of the cockpit.

The ledger (`rig::root::ledger`) keeps the split. Each tick the player owns the root, the hook submits the task's and the chase's displacements as separate vectors. The local character's pose-finalization entry completes the request against the step's solved root displacement, from [`Character::m_WorldMatrixT0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT0) to [`Character::m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT1). When the achieved displacement matches the requested sum, the whole chase is credited (exact); the match tolerates the `f32` rounding of the two absolute positions, which grows with the distance from the world origin. When something stopped or deflected the root, only the chase's forward projection on the achieved travel is credited, bounded by both the achieved and the requested distances (projected); a blocked chase perpendicular to a wall slide contributes nothing. The ledger keeps the running total of credited chase at the last two completed steps, and the frame half folds in everything completed since it last looked, so several steps between rendered frames all count and rendering never consumes a request. The shift is a step-spaced pair like the anchor it compensates: each side of the head pose pair composes with its own shift, so the engine's interpolation moves the anchor and the shift together and the camera holds still between steps.

Attribution errors cannot separate the body from the head. An under-credit leaves the camera moved by the uncredited part, and an over-credit leaves it behind by the excess; either way the next tick measures the gap from the head as it now is, so the root still converges on it. An error is a small mismatch between the camera and the real head, never a body left behind. A tick some other owner moved the root on records nothing, so a climb's motion is never credited as chase.

## Teleports and seat exits

A teleport (a camera reset, or a single-tick jump of the root further than the body's speed explains, `chase.warp_min_m` at rest; [controllers.md, "Posture and roomscale"](../input/controllers.md#posture-and-roomscale)) clears the ledger and restarts the head's velocity, and in the standing posture re-bases the shift so the head sits over the root, with no interpolation from the previous shift.

On the first frame out of a vehicle seat after an exit, a head within a metre of the root is left to the chase: the body walks to the head and the view does not move. A head further off is put back over the root, a jump the seat fade hides where the seat transition has one.

## Posture

The posture is `Auto`, `Seated`, or `Standing` (`controllers.roomscale.posture`). `Auto` is seated or standing by the head's height above the floor ([controllers.md, "Posture and roomscale"](../input/controllers.md#posture-and-roomscale)). The play area's size decides nothing: with no deadzone and the feed-forward, a player standing still produces sub-centimetre root motion, which the shift absorbs. A settings file from an earlier build that saved `Roomscale` loads as `Standing`.

- `Standing`: the true floor height, physical crouching, the crouch capsule, and the player owning the root.
- `Seated`: the seated eye reference and the crouch toggle ([controllers.md](../input/controllers.md#posture-and-roomscale)); the root moves only by the stick, and the lean still places the feet.

## Recentering

On foot in the standing posture with the chase on, the manual recenters (F7, the debug UI's and the pause menu's buttons, the stick click chord, and the recenter on resuming gameplay) re-base the yaw and the height only: the chase keeps the position, so a position re-base is never needed, and one could put the body through a wall the chase could not cross. The yaw re-base turns the shift with the cockpit frame, so the root stays where it is in the room. Seated, in a seat, with the chase off (nothing else brings the body back under the head), or without a recent frame, they re-base fully, which makes the head the cockpit origin and clears the shift. Any recenter clears the ledger and restarts the head's velocity.

## Settings

The pause menu's `VR: COMFORT` page has `POSTURE` (`AUTO`, `SEATED`, or `STANDING`), `BODY FOLLOWS YOU` (`chase.enabled`, on), and `LEAN WITHOUT STEPPING` (`lean.enabled`, on). The debug UI's settings have the rest under "Body rig": the chase's gain, feed-forward, speed limit, and warp distance, and the lean's gain, deadband, cap, and shares.

## Readouts and signals

The Controllers tab's "Body rig" section shows the root's owner and whether the on-foot tick runs; the gap to the target, the part outside the lean, and the target in the cockpit frame; the effective gap, the chase, the task, and the head's velocities; the lean, its allowance, the pivot's drop and the standing height, and the torso heading; the feet and the cockpit shift; and the ledger's exact and projected counts, with the re-bases at teleports and seat exits.

The scenario signals ([recorder.md](../testing/recorder.md)) are `rig.gap_m` (the gap before the lean), `rig.residual_m` (the part outside it), and `rig.ledger_projected` (steps credited by projection). On a flat floor, a replayed walk should keep `rig.residual_m` below 5 cm and `rig.ledger_projected` flat.

The pure parts run in a kinematic world in the unit tests (`rig::harness`): a root stepped at 30 Hz that integrates the composed velocity, optional wall planes, a frame half at 90 Hz, and the pose pair interpolated by the time since the last tick. They hold that a minimum-jerk walk of a metre peaking at 1.4 m/s keeps the gap under 3 cm and settles under 5 mm 0.3 s after stopping; that a hundred metres of random walking keeps the camera within a millimetre of the real head with nothing credited by projection; that the stick and the room add; that two or three ticks to a frame never overshoot; that a 0.3 m lean with a 0.12 m pivot drop, drop first, moves the root under 2 cm and standing up closes the gap; and that a climb clears the ledger.

## Unverified

None of this has been run in the headset.

- The proxy's character context may filter or limit a wanted velocity. A free proxy has to move by exactly its wanted velocity times the step for the exact attribution to apply at all; an acceleration limit would lag the chase, oscillate it with the feed-forward, and send every step to the projection, which leaves sideways and opposing chase-plus-stick movement partly in the view. The projected count and `rig.residual_m` show it.
- The locomotion task's update for a step has to run before the physics solve whose result the pose-finalization entry sees for the same step, so each request is completed against its own displacement, and the shift's step-spaced pair has to line up with the anchor's, so the camera holds still between steps while the root catches up.
- A lean that swings the pivot on an arc about the hips moves it out before it drops: at 20° from upright a pivot 0.7 m above the hips is 24 cm out and 4 cm down, still inside the deadband. The chase follows such a lean's first quarter metre, then holds, and walks back as the player straightens. The harness's lean drops first. Whether the allowance's gain, deadband, and cap feel right needs the headset.
- Root motion of millimetres at rest may wake the animation graph's idle and move selection or the AI's perception.
- The root turns through the orientation executor while the chase moves it; the procedural body's planted feet have to absorb both.
- A gap of more than a metre at a seat exit under the `Ride` transition jumps the view with no fade.
- The gap measure assumes the steady view: with `STEADY VIEW ON FOOT` off, the anchor is the animated head, which sits a little off the root, and the chase holds the root that far from under the eyes.
