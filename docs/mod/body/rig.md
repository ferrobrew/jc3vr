# The body rig

The rig keeps Rico's body plausibly under the player's head. The player's real head is ground truth, and the camera stays on it throughout: the body follows the head, never the other way round. The legs and the pelvis are the game's own animation, so the feet touch the ground the way the game's clips put them there, and the rig moves Rico by driving the game's own character movement. The upper body follows the player's real posture: the spine bends toward the head, the neck carries the rest of the head's turn, and the arms reach for the controllers. When the rig and the player disagree, the result is an extra step, never a broken skeleton.

The rig's pure core (the pelvis solver, the step commit, the yaw follow, and the standing height) is the `body-rig` crate, which the payload runs once per rendered frame and the offline evaluator runs over recordings ([Offline evaluation](#offline-evaluation)). The game-facing parts are in `payload/src/controllers/body/rig/`, and the settings are `controllers.body.rig`.

## States

The rig has two kinds of state.

Grounded: Rico is on foot on the ground with the headset driving the view, standing or seated. The rig owns his yaw and his position: the yaw follow turns him, and committed steps move him.

Game-decoupled: the wingsuit, the parachute, a vehicle seat, swimming, a grapple traversal or hang, a stunt position, a ragdoll, and getting up from one. The game owns the root and the yaw. The mod poses only the head, the neck, and the arms, plus a spine bend where the clip's pose allows one (a seat, the parachute's harness, the swim). The hand-over table that classifies the game's states is in [procedural-body.md](procedural-body.md#the-hand-over-table).

A jump, a short fall, an authored motion (a locomotion one-off, a hard landing, cover, a hit reaction), and climbing with the hands keep the game's legs and pelvis as well; the rig stops stepping while they play and picks up again when Rico is back on foot.

## The pelvis solver

Each frame the solver measures three points in the room frame (the cockpit frame with its floor at zero height):

- The neck pivot: a fixed offset back and down from the eyes in the head's frame, at the base of the skull. Nodding and turning the head swing the eyes about it and leave it in place.
- The waist pivot: over the floor point under Rico's pelvis, at the calibrated hip height (`body-rig`'s `HIP_HEIGHT_SHARE` of the player's height).
- The spine: one segment from the waist pivot to the neck pivot, as long as the standing neck height less the hip height. A bend is the segment's pitch and roll from vertical, within limits that are widest forward (`pelvis.forward_bend_deg`), narrower to either side (`pelvis.side_bend_deg`), and narrowest back (`pelvis.back_bend_deg`), with an ellipse between them.

A bend that carries the neck out over the pelvis also lowers it along the segment's arc. So the neck's height bounds how far out a legal bend can explain it, and the solver separates the cases the player's body produces:

- Head-only motion: a look about or a nod moves the eyes about the neck pivot, which barely moves. Nothing below the neck changes.
- A bend at the waist with the head level: the neck moves out and down on the spine's arc. The pelvis stays, and the neck takes the difference between the head's pitch and the spine's.
- A bend at the waist while looking down: the same, with the head pitched further.
- A step: the neck moves out horizontally without the drop the arc demands. The part of the offset past what a legal bend at the neck's height explains is step demand.
- A crouch: the head drops with no bend to explain it.

Near vertical the arc is flat: two centimetres of drop explain fifteen centimetres of reach, and a standing head bobs by about that much. So the lean radius (`pelvis.lean_radius_m`) sets a floor under the reach a bend explains: inside it Rico never moves, and the spine absorbs the offset. A cap (`pelvis.max_lean_m`) bounds the reach however low the neck is, so a crouched player who walks still steps.

The hands are a tie-breaker only: both held out along the offset, further than `pelvis.hand_reach_m` ahead of the neck, widen the lean's reach by `pelvis.hand_lean_m`, so a slow drift toward something held in both hands stays a lean longer.

The standing neck height is measured rather than assumed, because the runtime's floor and the calibrated eye height disagree by a few centimetres from session to session, and near vertical a few centimetres are a large lean. It is the height that nine in ten of the last half minute's samples lie below, sampled eight times a second while Rico is on foot in the standing posture and the gaze is within twenty degrees of level, seeded at the calibration and held within fifteen centimetres of it. Seated, the seated eye reference keeps its role ([controllers.md, "Posture and roomscale"](../input/controllers.md#posture-and-roomscale)).

## Steps

Step demand near the line between a lean and a step flickers with every bob of the head, so the step commit has hysteresis: a step commits once the demand passes `step.commit_m`, and a committed step runs until the pelvis is back under the neck, within `step.arrive_m`, rather than until the demand falls away.

A committed step drives the game's own character movement as a closed loop toward the floor point under the neck. The velocity is the remaining gap over `step.response_s`, at most `step.max_speed_mps`, so it tapers on arrival and the walk cycle does not overshoot. Each simulation tick takes the movement completed since the frame measured the gap out of it, so two ticks in one frame never close the same gap twice, and the drive is paced in real time, so in slow motion the body keeps up with the player's real steps. The velocity is added to the stick's through the character's wanted velocity, so the game's collision, slopes, and support move Rico.

The game's animation picks its clip from the move stick, so while a step is committed and still more than 8 cm from arriving, the bindings tick adds a walk to the move stick's half-axes (`grounded::add_step_walk`): `steps.walk_stick` of a full deflection, along the gap, in the frame of the head's heading, as the stick locomotion reads the stick. The game then plays its walk, and the stick locomotion takes the added walk back out of the stick before it sets its own velocity, so the walk picks the clip and the drive alone sets the speed. A step about to arrive, and the settle after it, shuffle under the drive with no walk.

What is left once the step arrives is slid out at `step.settle_speed_mps` while Rico idles, for at most `step.settle_s`. A step that has not arrived after `step.give_up_s` (a wall it cannot pass) stops; the push-back then holds the view at the wall ([Walls](#walls)).

Thumbstick locomotion is unchanged, and the physical offset adds to it: the stick moves the root and the camera together, so it never changes how far the root is from the head, and a player walking in the room while pushing the stick moves at the sum of the two.

## The yaw

Rico's yaw follows the head's through a deadzone (`yaw.deadzone_deg`), with a slew rate (`yaw.slew_deg_s`). A glance to the side twists the neck only. Turning the head past the deadzone starts a body turn, which runs until the body faces the target again within `yaw.settle_deg`, so a turn is a discrete act rather than a constant drag after every look. While the body moves (a step, or the stick), the deadzone narrows to `yaw.moving_deadzone_deg`, so a walking body faces where it looks sooner.

The hands bias the target: their mean horizontal offset from the neck, once further out than `yaw.hand_min_m`, pulls the target toward its own heading by `yaw.hand_bias` of the angle between them, so aiming to one side turns the torso first.

The body turns under the view without turning it ([The body turning under the view](#the-body-turning-under-the-view)), through the game's orientation executor. With `follow.turn_animation` on, the act filter lets the game's turn-in-place acts through while Rico stands still and the rig follows, so a body turn plays the game's own turn-in-place animation ([locomotion.md, "The turning animations"](locomotion.md#the-turning-animations)). Snap and smooth turning re-base the yaw as before: they turn the view and the room together, so the rig's headings, which are measured in the room, are unchanged by them.

## The upper body

The game's animation keeps the hips, the pelvis, and the legs. The procedural body poses the torso on the animation's hips ([procedural-body.md](procedural-body.md)): the spine bends toward the player's neck pivot, the bend shared out over the four spine bones within the lean limit, the chest twists toward the body's yaw, the neck takes a share of the head's turn, and the head override sets the head itself. The arms keep the two-bone IK from the shoulders the spine now places under the player's, and the clavicles reach with the hands.

## Recentering

The two-stick chord, F7, the pause menu's button, the debug UI's `Recenter the body` button in the Body fit feature, and the recenter on resuming gameplay snap the pelvis under the current neck pivot and reset the yaw and the height. Out of a seat, the recenter re-bases the view's yaw and height (`roomscale::manual_recenter_kind`), and on the next grounded frame, within a second, the rig snaps (`grounded::request_snap`; one asked for in a seat or in the air expires, since the recenter there re-based the whole view): the yaw follow faces the head's heading, the standing height starts again from the neck's height, any step under way stops, and the cockpit shift is re-based so the upright eyes over the pelvis under the neck stand over Rico's root. The camera moves to the body rather than the body to the camera, so a recenter never puts the body through a wall. A follow bank turns the rig's room frame with the cockpit frame and snaps nothing.

## Who moves the root

Several systems want the root: the game's own states, the hand climbing, the animation's authored motions, and the rig. Exactly one owns it on each tick (`rig::root::owner`), chosen in the root hook, a post-call detour on the on-foot locomotion task ([`NStateTask_MovementLocomotionTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_MovementLocomotionTask##func-Update)). The task writes the proxy's wanted velocity every on-foot tick, idle included, and velocity written there goes through the proxy's character context, which handles stairs, slopes, and support. The owners, in precedence order:

| Owner | When | The step drive |
| --- | --- | --- |
| Game | The task did not write the velocity (the task-defined motion state in [`Character::m_CurrentMotionState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_CurrentMotionState) is not set, or Rico is attached to a vehicle), or a seat transition is under way. Every other state that moves the proxy (the grapple, jumps, stunts, ragdoll, swimming, the parachute, the wingsuit) runs a different task. | None |
| Climb | A hand grips the world. The climb replaces the proxy's motion in the pre-simulation callback. | None, and the ledger records nothing |
| Authored | The animation carries the body: an authored one-off, a hard landing, cover, or a hit reaction's hold ([procedural-body.md](procedural-body.md#the-hand-over-table)). | None |
| Seated | Otherwise, in the seated posture. | Added |
| Player | Otherwise: on foot, standing. | Added |

On every tick the task wrote the velocity, the hook first replaces the clip's velocity with the stick's ([locomotion.md](locomotion.md)) and adds the palms' push off what they press into; together these are the task's movement. While the player owns the root, seated or standing, the step drive's velocity is added through the character's world-space accessors ([`Character::GetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-GetWantedWorldVelocity) and [`Character::SetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetWantedWorldVelocity)), because the proxy's own wanted velocity is character-local.

The same hook steps the head-through-geometry probe (below) and requests or releases the crouch capsule ([controllers.md, "Posture and roomscale"](../input/controllers.md#posture-and-roomscale)).

## The bookkeeping

The camera is the anchor carried by the root, plus the cockpit position less the cockpit shift, composed with the view's rotation and the world scale. The view's anchor stands the eyes of a neutral head directly over the root ([locomotion.md, "The steady view"](locomotion.md#the-steady-view)), so in the cockpit frame the root sits at the cockpit shift. When a step moves the root, the anchor moves with it while the cockpit position stays, so without a correction the camera would run ahead by the distance the root covered. The shift (`rig::root::shift`) takes the step's part of the root's displacement back out of the cockpit.

The ledger (`rig::root::ledger`) keeps the split. Each tick the player owns the root, the hook submits the task's and the step's displacements as separate vectors. The local character's pose-finalization entry completes the request against the step's solved root displacement, from [`Character::m_WorldMatrixT0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT0) to [`Character::m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT1). When the achieved displacement matches the requested sum, the whole step is credited (exact); when something stopped or deflected the root, only the step's forward projection on the achieved travel is credited (projected). The ledger keeps the running total of credited movement at the last two completed steps, and the frame half folds in everything completed since it last looked. The shift is a step-spaced pair like the anchor it compensates, so the engine's interpolation moves the anchor and the shift together and the camera holds still between steps.

Attribution errors cannot separate the body from the head. An under-credit leaves the camera moved by the uncredited part, and an over-credit leaves it behind by the excess; either way the next frame measures the gap from the head as it now is, so the pelvis still converges on the neck.

## Walls

The view stays with the real head, so a player who walks or leans into a wall would see through it while the root stands at the wall. Two things answer it: the push-back holds the view at the wall, and the fades darken a view that goes past it.

The push-back (`rig::root::pushback`) runs in the frame half after the gap is measured. When less than half of what the ledger's last two steps asked for was credited, and the step demand is over 5 cm, the step is obstructed. The view may then go only a threshold past the root:

| Owner | Threshold |
| --- | --- |
| Player or Seated, obstructed | 0.10 m |
| Player or Seated, unobstructed | none: the step drive owns the gap |
| Authored | 0.30 m, which only catches a runaway gap while the rig is suspended |
| Climb | 0.45 m, about an arm's-length lean |
| Game | none |

The part of the gap beyond the threshold is taken out of the cockpit by sliding the cockpit shift back along the gap, on both sides of the pose pair, so the view holds rigidly at the obstruction. The slide is capped per episode at 0.30 m; with the cap spent, the push-back fade rises past the threshold, black 0.15 m past it. The episode ends once the gap has stayed under half the threshold for 0.2 s. A teleport or a recenter ends the episode. With `pushback.enabled` off, nothing slides and the fade answers the gap past the threshold at once.

The head-through-geometry probe (`rig::collision::probe`) catches what the push-back cannot: a head pushed into geometry the root is nowhere near, such as a low beam or a wall the player leans into. On each tick the player's on-foot task runs, it casts from the last position the head held in the open to where it is now, and from the animated head, through the eye centre and 8 cm to either side across the view, and the deepest hit drives the head fade.

## Teleports and seat exits

A teleport (a camera reset, or a single-tick jump of the root further than the body's speed explains; [controllers.md, "Posture and roomscale"](../input/controllers.md#posture-and-roomscale)) clears the ledger, stops any step, and re-bases the shift so the head sits over the root, with no interpolation from the previous shift.

On the first frame out of a vehicle seat after an exit, a head within a metre of the root is left to the rig: the body steps to the head and the view does not move. A head further off is put back over the root.

## Posture

The posture is `Auto`, `Seated`, or `Standing` (`controllers.roomscale.posture`). Seated and standing follow the head the same way: the solver separates a lean from a step, and the step drive walks Rico after a shift in the chair. A seated player's head moves only by the upper body, so seated steps are rare. The rig's room frame puts the eyes at their height over the STAGE floor standing, and seated at the standing eye height plus the head's travel from the seated eye reference, so a seated player who sits upright reads as standing upright; only the standing posture samples the standing height.

Standing, the crouch capsule follows the crouch the solver measures, the head's drop that no bend at the waist explains, carried onto Rico's proportions, rather than the eyes' drop, so a look at the floor or a lean over a table does not lower the capsule ([controllers.md, "Posture and roomscale"](../input/controllers.md#posture-and-roomscale)). Seated, the crouch toggle drives it as before.

Head-tracked gamepad play keeps working: the rig needs only the head, the hands are a tie-breaker that is absent without controllers, and the yaw follow is the head's.

## The body turning under the view

The view composes the head onto Rico's body frame, so turning Rico would turn the view ([head-and-body.md, "HMD pose to camera"](head-and-body.md#hmd-pose-to-camera)). While the body follows (on foot, in gameplay, under the headset), the view frame is the body frame turned back by the body's heading relative to the view (`headpose::follow`), so the body can turn without the view following it. The view's heading is the turn accumulator's, which only the stick, the mouse, or a snap step moves. The body is steered, through the same face-dir target the turn input uses ([`NStateTask_LocoUtil::EvaluateCharacterOrientation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_LocoUtil##func-EvaluateCharacterOrientation)), to the view's heading plus the rig's body yaw in the cockpit frame, with no deadband of its own: the rig's deadzone and settle already decide when the body turns. Without a fresh grounded frame (off the ground, or before the first frame), the body does not follow (`rig::follow_settings`). The body therefore turns only because the player's head did, and the view turns only because the player pushed the turn input, which is the rule that the player is never reoriented without a movement action.

When the body stops following (a jump, a vehicle, the parachute, swimming, or the setting turned off), the offset freezes and the view rides the body again, turned by a constant. At the top of the next frame the frozen offset is folded into the cockpit baseline (the follow bank): the baseline's yaw turns by it, the offset goes to zero, and the cockpit shift and the rig's headings turn with the frame, so the composed view is unchanged and the cockpit frame's forward is the body's again, as the vehicles' physical controls, the driving view, and the flight steering assume.

## The body's volumes on Rico's skeleton

The sockets and pouches ride Rico's skeleton ([controllers.md, "Sockets, pouches, the foregrip, and the action-runs tier"](../input/controllers.md#sockets-pouches-the-foregrip-and-the-action-runs-tier)), so what the player sees on his body is where they reach. After the pose-finalization pass's solves ([`Character::UpdatePropEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePropEffects)), the bones HumanIK drives for [`Effector::HIPS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-HIPS), the hip joints, and the shoulder joints, found through [`HumanIK::m_HIKNodeAndBonePairs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_HIKNodeAndBonePairs), are read with the head bone and published with the character's world transform (`rig::skeleton`). The weapon handling carries them into the cockpit frame through the head pose and the view frame on its next input tick (`rig::frames`). The hip and belt volumes ride the pelvis and the shoulder, chest, grenade, and C4 volumes ride the chest, each girdle turned up along the spine (`weapons::volumes::BodyFrame`). The skeleton is used while it is younger than a quarter of a second and plausible.

## Settings and diagnostics

The pause menu's `VR: COMFORT` page has `POSTURE` and `BODY FOLLOWS YOU` (`steps.enabled`, on). Everything else is in the debug UI's Body fit feature: the `Recenter the body` button; the switches for the step drive (`steps.enabled`), the yaw follow (`follow.enabled`), the turn-in-place animation (`follow.turn_animation`), and the push-back (`pushback.enabled`), all on; and every tuning value above under `Lean and steps` (`pelvis.*`, `step.*`, and `steps.walk_stick`) and `Yaw` (`yaw.*`). Saved keys from the procedural rig (`chase`, `lean`, `torso`, `legs`) are ignored on load.

The live readout shows the owner; the step phase, the steps committed, the demand, the neck's offset, and the lean's reach; the bend's angle, pitch, and roll, the crouch, and whether the hands reach; the body yaw, its target, and whether a turn is under way; the neck's and the standing height, the standing height's samples, and the spine's length; the drive's and the task's velocities and the gap left; the cockpit shift; the push-back; and the ledger's counts with the re-bases at teleports, at seat exits, and at recenters. A change of step phase is logged at debug level on the `body` target (`rig: the step phase changes`).

The in-world diagnostics are on by default (`controllers.overlay.body_rig`, the visualisation drawer's `Body rig`, `rig::visual`): a sprite at the neck pivot and one at the waist pivot, the spine segment between them, the lean radius as a ring on the floor around the pelvis, and a fainter ring at the lean's reach, teal while idle, yellow while settling, and orange while stepping, drawn through Rico's body. They hang from the head in the player's own metres, so they show the player's body as the rig reads it, not Rico's.

The scenario signals ([recorder.md](../testing/recorder.md)) are `rig.gap_m` (the neck's offset over the pelvis), `rig.residual_m` (the step demand), `rig.stepping`, `rig.steps`, `rig.ledger_projected`, `rig.pushback_m`, `rig.obstructed`, and `rig.pushbacks`.

## Offline evaluation

`tracking-synth rig-eval <recording> <out.csv>` runs the same solver over a recorded `tracking.jc3trk` with a simulated pelvis that follows the step drive, and writes a CSV of the time, the bend, the step demand, the committed steps, the step phase, and the yaw error ([testing/recorder.md](../testing/recorder.md)); `tools/plot_rig_eval.py` plots it against the take's video.

The defaults come from the 2 November 2026 standing session's take: about ten and a half minutes on foot in which the player walked about the room, looked down at Rico's body for long stretches, recentred often, and fought. With them the rig commits 26 steps, drives the pelvis for about 32 seconds in all, starts 136 body turns, and holds the head a mean of 9 degrees from the body and the neck a mean of 8 centimetres from over the pelvis, 32 at most. The step count barely moves with the lean radius between 15 and 25 centimetres and the commit threshold between 3 and 6 (21 to 30 steps): standing a couple of centimetres below the standing height, the arc already explains more reach than the radius, and the steps that commit are real moves of a quarter to two thirds of a metre, so the radius and the threshold sit in the middle of that range. Looking down at the body carries the neck pivot ten to twenty centimetres forward and five to nine down, which the solver reads as a forward bend of ten to twenty degrees and leaves as a lean. The yaw deadzone trades turns against the head's offset: 25 degrees gives 152 turns and a mean offset of 8.6 degrees, 45 gives 117 and 11.3, and 30 sits between. The settle is short (0.6 s) because the player's own drift soon outweighs the few centimetres left after a step.

## What needs confirming in the headset

- Standing still and looking about: the readout's phase stays idle, the neck pivot marker stays over the pelvis, and the body turns only once the head has gone past the deadzone.
- Leaning over a table and reaching for the floor: no step, and the spine marker bends.
- Walking a metre or two in the room: one committed step per walk, the body walking rather than sliding, and coming to rest under the head.
- After snatches, throws, and fast arm motion, the body does not step or turn violently.
- The recenter chord puts the body under the head at once and leaves it there.
- Seated play, and head-tracked gamepad play without controllers.
- A committed step plays the game's walk rather than a slide; if it runs or does not walk, report the readout's drive speed, and tune `steps.walk_stick` against the game's walk threshold.
- A body turn while standing plays the game's turn-in-place; if it double-turns or swings the view, turn `follow.turn_animation` off and report it.
- The crouch capsule lowers for a real crouch and not for a look at the floor.

## Unverified

None of this has been run in the headset.

- The walk added to the move stick has to land between the game's walk and run thresholds ([`CharacterOnFootSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::CharacterOnFootSettings)); its default is a guess.
- The game may queue its turn-in-place acts only for a large turn, or not at all while the orientation executor is steered; the follow then turns the root alone, as before.
- The proxy's character context may filter or limit a wanted velocity, which would lag the drive and send steps to the ledger's projection.
- The neck pivot's offset from the eyes is a generic figure; a player whose neck sits further back reads looking down as a larger lean.
