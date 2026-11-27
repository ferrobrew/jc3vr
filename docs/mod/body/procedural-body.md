# The procedural torso

The procedural torso poses Rico's spine, neck, and clavicles from the player, on the hips the game's animation keeps. The hips and the legs stay the game's, because the body rig moves Rico by driving the game's own movement ([rig.md](rig.md)); standing on foot and in a jump the IK body poses the pelvis and the legs from the learned estimate ([ik-body.md](ik-body.md)). Without the procedural torso the body is the game's animation with HumanIK corrections ([head-and-body.md](head-and-body.md)). The code is in `crates/jc3vr-body/src/controllers/body/procedural/` and its tuning in `crates/jc3vr-body/tuning/procedural.toml` ([tuning.md](../tuning.md)). The states outside standing play are in [procedural-states.md](procedural-states.md).

## Where the pose is written

The pose is written in a pre-call detour on [`HumanIK::HasTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-HasTargets) (`hooks::character::gate`), for the local player's solver and the `MAIN` pass. Its one caller sits in the pose-finalization pass after every game writer of IK targets and before the solve, the model-space sync, the hit shapes, and the AI's view of the character ([skeleton.md](../../engine/character/skeleton.md#who-reads-the-pose-before-updatepropeffects)), so the pose written there is the one bullets hit and NPCs see. The gate has no step of its own, so the pass's entry records the pass's step per thread and the blends advance by it.

The writes go through [`AnimationController::SetJoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-SetJoint), parents first. The pose producer's pose pair ([skeleton.md](../../engine/character/skeleton.md#bone-store-hkapose-model-space)) makes this tick's pose next tick's `T0`, so the skinning interpolates it like any other.

## The rig

`rig` finds the four spine bones, the neck, the head, the clavicles, the hips, the pelvis, and the legs by name in [`AnimationController::m_Skeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##field-m_Skeleton), checks each bone's parent, and measures each arm (shoulder joint on its clavicle, length to the hand) from the reference pose. A skeleton that differs is refused with a `procedural rig: ...` warning and the body stays the animation's. Every bone index is checked against the skeleton's bone count before a read or write, since `SetJoint` does not check it.

## The hand-over table

The torso has one weight against the animation; the arms are always the controllers' (hand IK) and the head the headset's (head override). `authority::classify` reads the character's state at the gate and picks the first matching case:

| Case | Torso |
| --- | --- |
| Not in the headset, not in gameplay, or in a scripted sequence ([`Character::m_InSequence`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_InSequence)) | animation |
| Dead, or ragdoll ([`SB_IS_IN_DEAD_STATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_IN_DEAD_STATE), [`SB_IN_RAGDOLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IN_RAGDOLL)) | animation, at once |
| Getting up from a ragdoll | animation |
| Seated and riding in a vehicle; standing in a stunt area; hanging in a helicopter's stable stunt idle ([procedural-states.md](procedural-states.md)) | procedural |
| Otherwise in or on a vehicle | animation |
| Attached to a wall after a grapple's reel ([procedural-states.md](procedural-states.md#the-grapples-wall-attach)) | procedural |
| A grapple or stunt traversal, or the grapple's ledge climb | animation |
| Swimming, or the parachute | procedural |
| The wingsuit or the skydive (the arms still follow the player's; [procedural-states.md](procedural-states.md#the-wingsuit)) | animation |
| A fall past the jump's grace (the locomotion tuning's `anchor.jump_grace_s`) | animation |
| A hit reaction queued within `hit_reaction.hold_ms` | animation |
| Climbing with the hands ([hand-climbing.md](../input/hand-climbing.md)) | procedural |
| A jump within the grace | procedural |
| An authored motion: a one-off, a hard landing, a looped context action, a full-body pickup, or cover | animation |
| On foot, but the movement task has not run since the last gate | animation |
| On foot on the ground | procedural |

While the grapple lock holds the line, a traversal, a wall attach, a fall past the grace, or on foot without the movement task reads instead as `grapple line locked`, unless a clip owns the body; the torso is the animation's and the legs hang from the line ([procedural-states.md](procedural-states.md#the-firm-ragdoll)).

A jump or a spell under the parachute starts a flight hold (`blend.flight_hold_s` of simulation time) that keeps the torso procedural through the cases a flight passes through (a fall past the grace, a traversal, on foot without the movement task), so a long jump or a canopy reopened around a grapple reel does not hand the torso back and forth. Any other case ends the hold. The hold does not change the reported case.

The weight follows its case with a critically damped spring (`blend.in_s` in, the quicker `blend.out_s` out), except that a death or a ragdoll hands over at once, since a ragdoll writes the pose itself. Handing back is continuous because the animation plays all along. Every change of case is logged at info level (`procedural body: the hand-over case changes`). The steady view reads the same table with its own tracker ([locomotion.md](locomotion.md#the-steady-view)).

The vehicle seat, the traversal, and the airborne modes come from the mode detection the input tick publishes, so they lag by up to a frame; state bits, motion state, health, and support are read at the gate. Two cases keep no state on the character, so hooks stamp them (`hooks::character::signals`, `procedural::signals`): a post-call on [`NStateTask_ReeledInLedgeClimbMotionTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReeledInLedgeClimbMotionTask##func-Update) marks the ledge climb, and a pre-call on [`AnimatedModel::QueueAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-QueueAct) catches the stumble and knock-back acts [`Character::HitReaction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-HitReaction) queues on Rico's model. The jump case waits out a stair's bump (`in_air.debounce_s`, longer while the movement task runs) so a stop's settle is not a jump.

## The torso on the animation's hips

`solver::anchored` poses the torso from the clip's own pose, so a walk's stride or a driver's reach stays the base the player's head moves from. In order:

1. On the ground, the clip's forward lean is taken out first (the share `torso.delean`, eased over `torso.delean_blend_s`). Every gait clip leans forward, and swinging the neck under the head from there bends the upper spine back and tips the chest into the view.
2. The chest twists to face the player's torso: the rig's chest heading against Rico's facing, less the heading of the clip's own shoulder line, so a bladed idle is squared rather than turned further. It is limited and shared up the spine, and fades as the spine lies down.
3. The spine swings so the neck reaches the neck pivot under the player's head, shared along the four spine bones and limited to `torso.lean_limit_deg` from the clip's spine. A bend the rig reads bends Rico's back; the hips stay the clip's.
4. A look down pitches the chest forward by a share of it.
5. The chest is kept out of the camera's near field: a front point of the chest (upper spine, sternum, or a shoulder joint) close below the eyes swings the whole spine back about its base until it is behind the eyes along the view's level forward, never past `chest_clearance.limit_rad`, so a head passing the vertical cannot fold the spine backwards.
6. The neck takes a share of the head's turn; the head override sets the head after the solve.

The player's head is read at the neck pivot, below and behind the eyes (`procedural::head`), because looking down moves the eye point although nothing below the neck moved. On foot it is the camera's own pivot (`camera_neck_pivot`), the point the rig reads as the neck; where the view hangs from a clip's head, the steady view's weight blends it with the published head.

The rotations are composed onto the animated bones, so no rest frame is assumed. The game's IK targets on the torso's effectors have reach and pull faded by the torso's weight; the hips' stay the game's. The head's target keeps its reach but loses its pull, so it closes the last gap to the player's head without dragging the spine. The body IK's own torso stands down at full weight.

## The hips

The idle clip stands bladed ([idle-stance.md](../../engine/character/idle-stance.md#the-idles-clip-variants)), and the yaw follow steers the root to the legs' heading ([rig.md](rig.md#the-yaw)), so the hips would face off the player. In the case `on foot` the gate measures the heading of the line across the clip's upper legs against the root and follows it over `hips.smoothing_s` (`procedural::hips`); the yaw follow subtracts it, so the clip's hips rather than its root face the legs' heading. Brief ground cases (a hit reaction, a one-off, a jump within the grace, a tick without the movement task) hold the followed heading; only leaving the ground resets it.

## The stance

While the IK body owns the standing body, its contacts place the feet and the stance yields by their weight ([ik-body.md](ik-body.md#feet)). Otherwise (no fresh estimate, gamepad-only play, or the IK body off) no idle in Rico's set stands square, so `procedural::stance` moves the game's locomotion IK ankle and toe targets across the ground to a square stance while Rico stands still in one of the base graph's standing idles (`STANCE_IDLES`). Each target keeps the game's height. The ankles stand `stance.width_m` apart square across the followed hip heading, each toe ahead by the clip's foot length and turned out, blending in and out over `stance.blend_s`. The idle-state check matters because a landing reads as on foot once grounded, and a hard landing's get-up places the feet itself.

## The arms

The arms are the controllers' wrist targets ([hands-and-aiming.md](../input/hands-and-aiming.md#the-wrist-targets)), solved by HumanIK on the written torso. While the torso is procedural the hand IK defers its plan from the pass's entry to the gate (`hands::ik::queue_or_defer`, `queue_deferred`), since the rotation target is a delta from the wrist as it stands before the solve. At the gate:

- The wrist target is pushed out of two capsules along the written spine, the chest and the abdomen. The physical hands' held-object keep-out uses the same capsules (`body::procedural::torso_keep_out`; [physical-hands.md](../input/physical-hands.md#the-players-own-body)).
- The pull is zero, so the solve never bends the player's torso toward a hand.
- On foot, standing on a vehicle, in a jump, under the parachute, and in flight, each clavicle is moved so its shoulder joint sits on the arm remap's character shoulder ([hands-and-aiming.md](../input/hands-and-aiming.md#arm-reach)), held to the chest clearance (`solver::anchored::clear_clavicle_offset`).
- The clavicles reach with the hands: up for a hand overhead, forward for a far reach or across the body, and a little down for a reach far below.
- Each arm gets a weak elbow position hint along a pole pointing down, out, and a little back, lifted further out for a reach up or across. Where the reach runs along that pole (toward a hip holster), a second pole straight back takes over smoothly so the elbow does not jump.

The hint shapes the elbow the simulation reads (hit shapes, shot origins). The drawn elbow is placed again every frame by the presentation layer ([presentation-layer.md](../rendering/presentation-layer.md#the-arms)).

## The limbs' twist

The arm's twist bones (`df_LeftArmRollA01` under `LeftArm`, `LeftForeArmRoll` under `LeftForeArm`, and the right side alike; [skeleton.md](../../engine/character/skeleton.md)) carry the skin along the arm. `body::procedural::twist` runs in the post-call `UpdatePropEffects` detour after the head override and the finger curls, shares the hand's twist against the forearm over the forearm's twist bones, and holds the upper arm's roll back near the shoulder, by the torso's weight. A forearm can only pronate so far, so a hand turned past `twist.forearm_limit_rad` passes a share of the excess to the upper arm's twist bones. The arm bones themselves are not turned, so the elbow stays where the solve put it. The legs' twist bones stay the animation's.

## Diagnostics

The debug UI's Body/tracking > Body fit live status shows the procedural body's status rows (rigging, the hand-over case, `Twist bones`, and one row per state in [procedural-states.md](procedural-states.md)), the weight, the neck's distance from its target, each clavicle's turn, and the stance's weight and moved targets.
