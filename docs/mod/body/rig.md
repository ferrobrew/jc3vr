# The body rig

The rig keeps Rico's body plausibly under the player's head. The real head is ground truth and the camera stays on it: the body follows the head, never the other way round. The rig moves Rico only by driving the game's own character movement, so collision, slopes, and support stay the game's. On the walk and wherever the learned estimate is missing, the legs and pelvis are the game's animation; standing on foot with a fresh estimate, the IK body poses them with HumanIK ([ik-body.md](ik-body.md)). The upper body follows the player's posture ([procedural-body.md](procedural-body.md)). When the rig and the player disagree, the result is an extra step, never a broken skeleton.

The pure core (the pelvis solver, the step commit, the yaw follow, and the standing height) is the `body-rig` crate, run once per rendered frame by the payload and over recordings by the offline evaluator. The game-facing parts are in `crates/jc3vr-body/src/controllers/body/rig/`. The settings are `controllers.body.rig`, and the tuning is `crates/jc3vr-body/tuning/rig.toml`, whose `core` table is the pure core's.

## States

Grounded: Rico is on foot on the ground with the headset driving the view, standing or seated. The rig owns his yaw and position: the yaw follow turns him and committed steps move him. While the IK body owns the standing body, the drive follows the floor point under its FK pelvis instead of committing steps ([ik-body.md](ik-body.md#the-root)).

Game-decoupled: the wingsuit, the parachute, a seat, swimming, grapple traversal, stunts, and ragdolls. The game owns the root and yaw, and the mod poses only the head, neck, arms, and a spine bend where the clip allows ([procedural-body.md](procedural-body.md#the-hand-over-table)). A jump, a short fall, an authored motion, and hand climbing also keep the game's legs; the rig stops stepping and resumes once Rico is back on foot.

## The pelvis solver

Each frame the solver measures three things in the room frame (the cockpit frame with its floor at zero): the neck pivot, a fixed offset back and down from the eyes (`body_rig::body::EYES_TO_NECK`), which nodding and turning leave in place; the waist pivot over the floor point under Rico's pelvis at the calibrated hip height; and the spine between them. A bend is the spine's pitch and roll from vertical, within limits widest forward (`pelvis.forward_bend_deg`), narrower sideways, and narrowest back.

A bend that carries the neck out over the pelvis also lowers it along the spine's arc, so the neck's height bounds how far a legal bend explains its offset. Head-only motion moves nothing below the neck; a bend at the waist moves the neck out and down on the arc; a step moves it out without the drop, and the part past what a bend explains is step demand; a crouch drops it with no bend. Near vertical the arc is flat (two centimetres of drop explain fifteen of reach, about a standing head's bob), so the lean radius (`pelvis.lean_radius_m`) is a floor under the reach a bend explains. Behind the pelvis the radius is smaller, so a player who steps back steps rather than leaning Rico's chest into the camera, and a cap bounds the reach so a crouched player who walks still steps. Both hands held well out along the offset widen the reach a little, so a drift toward something held stays a lean.

The standing neck height is measured rather than assumed, because the runtime's floor and the calibrated eye height disagree by a few centimetres each session, and near vertical that is a large lean. It is a percentile of recent samples taken while standing with a level gaze, seeded at calibration and held near it.

## Steps

A step commits once demand passes `step.commit_m` and runs until the pelvis is back under the neck (`step.arrive_m`), a hysteresis that keeps head bob from flickering it. It drives the game's movement as a closed loop toward the floor point under the neck, at the remaining gap over `step.response_s` and capped, so it tapers on arrival. Each simulation tick subtracts the movement completed since the gap was measured, so two ticks never close the same gap twice. The drive is paced in real time, so slow motion keeps up with real steps.

The game picks its clip from the move stick, so while a step has far to go the bindings add a walk to the stick (`grounded::add_step_walk`, `grounded.walk_stick` of a deflection, which must land between the game's walk and run thresholds in [`CharacterOnFootSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::CharacterOnFootSettings)). The stick locomotion takes it back out before setting its velocity, so the walk picks the clip and the drive alone sets the speed. A step near arrival, and the settle after it, shuffle with no walk.

A weight shift inside the lean's reach commits no step, so the idle drift resolves it: an offset to the side or behind that stays while the neck is still starts a slow shuffle of the pelvis under the neck. The forward part is left alone, since leaning over a table or looking down is meant. A step that cannot arrive (a wall) gives up after `step.give_up_s`, and the push-back holds the view ([Walls](#walls)).

The camera hangs the eyes from Rico's head in the view's frame, not the body's (`hooks::camera`), because the headset's translation already carries the eyes round with a torso turn ([locomotion.md](locomotion.md#the-steady-view)). Once the body has turned under the view, Rico's eyes stand off the camera (about 12 cm sideways at a quarter turn), so the rig adds that slip to the root point it reads (`root::eye_rig_slip`) and the drift takes out each change of it: the body steps under the camera and the camera does not move.

Thumbstick locomotion adds to the physical offset rather than changing it, since the stick moves root and camera together.

## The yaw

The yaw has two layers, after Dead and Buried's body (RootMotion's solver, later Final IK's [VRIK](http://www.root-motion.com/finalikdox/html/page16.html)): the chest follows the head continuously within a clamp, and the legs, which are Rico's facing, turn only for a large or sustained turn of the head or while he moves. Readability comes before anatomy: the torso faces where the player looks, and the stance stays put until the head has clearly gone elsewhere.

The chest is a critically damped spring toward the head's heading, clamped to `yaw.chest_clamp_deg` of the legs, with the neck taking the rest. The hands pull the chest's target, not the legs', so aiming to one side turns the torso and leaves the stance. A two-layer design rather than one deadzone halves the number of body turns, because hands pulling the whole body push the target past the deadzone while the head is still inside it.

A turn of the legs starts when the head is past `yaw.turn_deg`, or past `yaw.sustain_deg` for `yaw.sustain_s`, or, while the body moves, past a moving deadzone on one side for `yaw.moving_sustain_s`; the moving sustain exists because the head sways in a walk and the legs would otherwise flip about once a second. The turn slews to the head and stops, a discrete act. A steady settle turns the legs slowly under a head held a little off them, so Rico is not left standing bladed. Looking down past `yaw.look_down_deg`, where the head's heading points at a holster rather than where the player means to face, only the large turn can start and the chest's follow fades out.

Where the learned estimator runs ([body-estimator.md](body-estimator.md)), its pelvis and chest headings take over the legs and pull the chest (`body_rig::estimate`), on the ground with the grounded rig only. The estimate moves part of the way with a glance and jitters, so the legs follow a smoothed copy through a deadband, carried at its edge so a body turn turns them continuously, and settle onto it once it is still. The estimate's say is a weight that eases in and out with arrival, falls with the estimate's confidence (by age), and fades while the stick walks the body; each frame the legs move by the rules' change and the estimate's change blended by that weight, so nothing snaps, and the rules keep running underneath so their clocks are current when it hands back. While the estimate has most of the say, a turn faster than `core.yaw.turning_deg_s` counts as a body turn for the turn-in-place animation. The band and that gate are wide so the game's turn-in-place acts do not become a constant shuffle.

The procedural body twists the chest by the chest's heading relative to the facing. Off the ground there is no grounded rig, and every shoulder estimate takes the torso's heading from one follower in the air (`procedural::wings::WingHeading`, [procedural-states.md](procedural-states.md#the-wingsuit)); elsewhere, as in a seat, the head's heading stands in.

The body turns through the game's orientation executor without turning the view ([below](#the-body-turning-under-the-view)), and the act filter lets the game's turn-in-place acts through while Rico stands, so a body turn plays the game's animation ([locomotion.md](locomotion.md#the-turning-animations)), except while the IK body plants the feet. Snap and smooth turning turn the view and the room together, so the rig's room-frame headings are unchanged by them.

## Recentering

The two-stick chord, F7, the pause menu, the debug UI's `Recenter the body`, and resuming gameplay recenter. Out of a seat the view's yaw and height re-base (`roomscale::manual_recenter_kind`), and on the next grounded frame the rig snaps (`grounded::request_snap`): the yaw faces the head, the standing height restarts, any step stops, and the cockpit shift re-bases so the eyes stand over Rico's root. A request made in a seat or the air expires, since the recenter there re-based the whole view. The camera moves to the body, so a recenter never puts the body through a wall. A follow bank turns the rig's room frame and snaps nothing.

## Who moves the root

Exactly one owner moves the root each tick (`rig::root::owner`), chosen in the root hook, a post-call detour on [`NStateTask_MovementLocomotionTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_MovementLocomotionTask##func-Update), which writes the proxy's wanted velocity every on-foot tick. In precedence order:

| Owner | When | Step drive |
| --- | --- | --- |
| Game | The task did not write the velocity (the motion state in [`Character::m_CurrentMotionState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_CurrentMotionState) is not the task's, or Rico is attached to a vehicle), or a seat transition is under way. | None |
| Climb | A hand grips the world; the climb replaces the proxy's motion. | None |
| Authored | The animation carries the body: a one-off, a hard landing, cover, or a hit reaction's hold. | None |
| Seated | Otherwise, seated. | Added |
| Player | Otherwise, standing. | Added |

On every tick the task wrote, the hook composes the wanted velocity in the on-foot order ([physics-substrate.md](physics-substrate.md#the-wanted-velocity)): the stick's velocity in place of the clip's ([locomotion.md](locomotion.md)), the palms' push, and, for the Player and Seated owners, the step drive, through [`Character::SetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetWantedWorldVelocity) since the proxy's own wanted velocity is character-local. The same hook requests or releases the crouch capsule.

## The bookkeeping

The camera is the root-carried anchor plus the cockpit position less the cockpit shift. When a step moves the root the anchor moves with it, so without a correction the camera would run ahead; the shift (`rig::root::shift`) takes the step's part of the root's displacement back out.

The ledger (`rig::root::ledger`) keeps that split. Each tick the hook submits the task's and the step's displacements separately; the pose-finalization entry completes them against the solved root displacement between [`Character::m_WorldMatrixT0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT0) and [`m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT1), crediting the whole step when the root went where asked and only its projection on the achieved travel when something deflected it. The shift is a step-spaced pair like the anchor, so interpolation moves them together and the camera holds still between steps. Attribution errors cannot separate body from head: the next frame measures the gap from the head as it is, so the pelvis still converges.

## Walls

The view stays with the real head, so walking or leaning into a wall would see through it. The push-back (`rig::root::pushback`) holds the view at the wall: when the ledger credited too little of what recent steps asked for, the step is obstructed, and the view may go only a threshold past the root (small when obstructed, an arm's length while climbing, a large runaway catch while authored, none for the game). The excess slides the cockpit shift back so the view holds rigidly; past a per-episode cap the push-back fade darkens the view instead.

The head-through-geometry probe (`rig::collision::probe`) catches a head pushed into geometry the root is nowhere near, such as a low beam. Every simulation tick under the headset, in every state, it casts from where the eyes last were in the open and from the animated head to the eyes and to either side, and the deepest hit drives the head fade. A downward ray against the terrain alone ([`PhysicsSystem::CastRayTerrain`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem##func-CastRayTerrain)) catches eyes under the heightfield, as when a wingsuit flight ends in the ground. The vehicle Rico rides is ignored, since between ticks of a fast car the head's path runs through its cabin.

## Teleports and seat exits

A teleport (a camera reset, or a root jump the body's speed cannot explain; [roomscale.md](../input/roomscale.md)) clears the ledger, stops any step, and re-bases the shift so the head sits over the root. On the first frame out of a seat, a head within `root.exit_rebase_m` of the root is left to the rig to step under; one further off is put back over the root.

## Posture

The posture is `Auto`, `Seated`, or `Standing` (`controllers.roomscale.posture`). Both follow the head the same way. Seated, the rig's room frame puts the eyes at the standing eye height plus the head's travel from the seated reference, so a seated player sitting upright reads as standing; only standing samples the standing height. Standing, the crouch capsule follows the crouch the solver measures, not the eyes' drop, so looking at the floor or leaning over a table does not lower it ([roomscale.md](../input/roomscale.md)); seated, the crouch toggle drives it. Head-tracked gamepad play works because the rig needs only the head.

## The body turning under the view

The view composes the head onto Rico's body frame, so turning Rico would turn the view ([head-and-body.md](head-and-body.md#hmd-pose-to-camera)). While the body follows (on foot, in gameplay, under the headset), the view frame is the body frame turned back by the body's heading relative to the view (`headpose::follow`). The view's heading is the turn accumulator's, moved only by the stick, the mouse, or a snap. The body is steered through the turn input's face-dir target ([`NStateTask_LocoUtil::EvaluateCharacterOrientation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_LocoUtil##func-EvaluateCharacterOrientation)) to the view's heading plus the rig's body yaw. So the body turns only because the head did, and the view only because the player pushed a turn input: the player is never reoriented without a movement action.

When the body stops following (a jump, a vehicle, the parachute, swimming), the offset freezes and the view rides the body, turned by a constant. Two cases hold the view's heading instead (`headpose::follow::BodyFollow`). Through a vehicle's entry or exit clip, the view's heading holds and steps toward the body's in hard steps at intervals (the view crate's `follow.*` tuning) until it faces within a margin, so the clip's turn never swings it. Through a ragdoll, its get-up, and a death, it holds without stepping, because the get-up turns the root up to half a turn to wherever the ragdoll fell.

When the steps end, the frozen offset is folded into the cockpit baseline (the follow bank): the baseline's yaw, the cockpit shift, and the rig's headings turn by it, the composed view is unchanged, and the cockpit's forward is the body's again, as the vehicles' controls and steering assume. A seat lock waits for the bank, so the two never turn the view at once ([roomscale.md](../input/roomscale.md)).

## The body's volumes on Rico's skeleton

The sockets and pouches ride Rico's skeleton ([weapon-handling.md](../input/weapon-handling.md#pouches)), so what the player sees on his body is where they reach. After the pose-finalization pass's solves ([`Character::UpdatePropEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePropEffects)), `rig::skeleton` reads the bones HumanIK drives for [`Effector::HIPS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-HIPS), the hips, the shoulders, and the head (through [`HumanIK::m_HIKNodeAndBonePairs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_HIKNodeAndBonePairs)) and publishes them with the character's transform; `rig::frames` carries them into the cockpit frame on the next input tick. The hip and belt volumes ride the pelvis and the rest ride the chest, except that the grenade and C4 pouches ignore the forward bend a look down puts in the chest. A skeleton older than a quarter of a second is unused.

A read must be a plausible body (`rig::frames::implausibility`). The head is held loosely, since a clip can hang the head bone forward of the shoulders; looking down does not, because the head target is the neck pivot ([head-and-body.md](head-and-body.md#body-ik-via-humanik)). An implausible or unreadable read falls back to the last plausible skeleton, kept in model space and carried by the current transform. Each change is logged on the `body` target (`body frame: ...`), and the readout's `Holsters ride` says which skeleton is in use.

## Settings and diagnostics

The pause menu's `VR: COMFORT` page has `POSTURE` and `BODY FOLLOWS YOU` (`steps.enabled`). Everything else is in the debug UI's Body fit feature: `Recenter the body`, `Lean and steps` (`pelvis.*`, `step.*`), `Yaw` (`yaw.*`), and the `Learned body estimate` section ([body-estimator.md](body-estimator.md#in-the-game)), with a live readout of the solver, steps, yaw, ledger, and push-back. Step phase changes are logged at debug level (`rig: the step phase changes`). The `Body rig` visualisation (`controllers.overlay.body_rig`, on, `rig::visual`) draws the neck and waist pivots, the spine, and the lean rings on the floor, coloured by step phase; it hangs from the head in the player's metres, so it shows the player's body as the rig reads it.

## Offline evaluation

`tracking-synth rig-eval <recording> <out.csv>` runs the same solver over a `tracking.jc3trk` and writes per-frame bend, step demand, phase, and yaw error ([recorder.md](../testing/recorder.md)); `tools/analysis/plot_rig_eval.py` plots it against the take's video, and the flags are in `tools/tracking-synth/src/main.rs`. `--weights` runs the estimator over the take first and feeds it to the rig as the game would (`export-rerun --weights` does the same, and `--ik` replays the IK body, [ik-body.md](ik-body.md#the-offline-replay)). `--pelvis` picks the inputs: `simulated` (a pelvis following the step drive; the only choice for recordings before version 8), `recorded` (the payload's own rig inputs, so a changed rig is compared with the game's on the same frames; the default when present), or `pre-ik` (the pelvis under the game's animated hips). With `rig` events each row also carries the recorded rig's values and the differences. The calibration and settings come from the recording; the tuning file is read from the repository on each run, so an edit applies without a rebuild.

The defaults were fitted with rig-eval over standing takes of walking about, looking down at the body, recentering, and fighting. The lean radius and commit threshold sit in a wide flat range; looking down moves the neck pivot 10 to 20 cm forward, which reads as a lean. With the estimate, the legs sit about half as far from the estimate's pelvis as with the rules alone, at the same lag. The estimator's tuning was fitted through the CPU reference, not the in-game GPU timing, and the neck offset is a generic figure, so a player whose neck sits further back reads looking down as a larger lean.
