# The IK body

While Rico stands on foot, his lower body is posed from the learned body estimate rather than the game's idle clip: the pelvis goes where forward kinematics on the estimate puts the player's, the feet stay planted through HumanIK and move only by explicit steps, and the root follows the floor point under the pelvis. In the air the pelvis hangs under the head with relaxed legs; hand-climbing, the feet brace on the face. On the walk the clip keeps the body, with a share of the IK pelvis turning its hips. The torso, arms, and head are the procedural body's as everywhere else.

The pure parts are in `body-rig` (`fk`, `contact`, `brace`, `ik_weight`) and the game-facing parts in `crates/jc3vr-body/src/controllers/body/ik_body/`. The settings are `controllers.body.ik` (the firm ragdoll's target switch and the contacts' tuning); the rest is `crates/jc3vr-body/tuning/ik_body.toml`.

## The FK pelvis

The estimator ([body-estimator.md](body-estimator.md)) gives each SMPL joint's global rotation but no translation, so the pelvis is found by forward kinematics down from the tracked neck pivot (`body_rig::fk`), on the SMPL template scaled to the rig's spine and the player's leg length. A player who bends over gets a pelvis behind and below the head; one who crouches, one lower under it. `ik_body::fk` advances it once per frame before the grounded rig, smoothed by a spring, and eases to a fallback (the rig's waist pivot with straight legs) when the estimate goes stale. The IK takes the standing body only while the source is the estimate.

## Seeding the pose

The IK body works in the procedural body's solve gate ([procedural-body.md](procedural-body.md)), in two halves (`ik_body::gate`, `ik_body::effectors`).

Before the torso is solved, the FK body is carried into Rico's model space and scaled to his proportions, and the clip's hips are turned and moved onto the FK pelvis, carrying the whole posture (`ik_body::seed`). The hand-over uses positions only (each pelvis frame read from its hips and first spine bone), so no bone's rest frame has to be known. If a planted foot would be out of reach, the pelvis comes down up to `gate.max_pelvis_drop_m`. The torso's solve then bends the spine from the player's pelvis, and the clip's forward lean is gone with its hips.

After the pose is written, HumanIK's targets hold it through the solve: a hips position target and a zero rotation offset at full reach and resistance, which [`HumanIK::UpdateEffectorsFromTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-UpdateEffectorsFromTargets) applies onto the seeded pose, so it holds the seeded orientation; ankle and toe targets with no pull for each planted foot, so reaching a foot moves only its leg; and weak knee hints along the estimate's thighs. The effectors come from [`HumanIK::GetEffectorIdFromBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-GetEffectorIdFromBoneIndex).

The game's locomotion IK task ([`NLocoIKTask_Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/animation::arm_aim_ik##func-NLocoIKTask_Update)) queues hip and foot targets earlier in the tick, and a later queue overwrites an effector's target in place ([`HumanIK::AddEffectorTargetPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetPosition)), so at partial weight the IK target is the game's mixed toward the IK's. The targets are queued without the blend-out flag, so when the IK stops queuing one the game's task takes it back on its next target.

## Feet

Each foot is planted (a pose in its support's frame, which stays put however the pelvis moves), swinging (a planned arc and landing), or unsupported (left to the clip and the game's foot IK, which is how contacts are acquired). A planted foot steps only by a rule, in priority order: out of reach (counting the pelvis drop the gate may use, since a tall stance and the pelvis's bob otherwise leave reach on every bob), balance (the centre of mass outside the feet's support polygon), the root carried by something other than the player, twist (the pelvis turned past `contact.twist_deg` for `contact.twist_hold_s`, so a glance moves no foot), and tidy (a foot left off its stance spot at rest).

One foot swings at a time, alternating, and a twist opens with the foot on its side; after any step but a tidy, the other foot follows if left off its spot, so a body turn is two steps. A step lands at the square stance's spot under the FK pelvis ([procedural-body.md](procedural-body.md#the-stance)), led along the pelvis's travel.

The gate cannot cast, so each landing's probe is posted to a mailbox and cast from the player aim control's post-simulation update (`ik_body::probe`); the answer arrives within the swing, and a late one holds the landing briefly. No ground below leaves the foot unsupported, never planted in the air. A walk rather than a step (both feet out of reach while the pelvis moves fast, a step longer than `contact.max_step_m`, or the planted hip outrunning its reach) drops the contacts and hands the legs to the game's walk until Rico has stood still for `gate.resume_s`.

## Moving supports

A probe names the body it lands on as the foot's support (`ik_body::support`): a static body is the world, and a moving one (a deck, a platform, a crate) is named by its Havok id and the lifetime stamp hand climbing keeps ([hand-climbing.md](../input/hand-climbing.md#moving-bodies)), so a reused id is a different support. The gate does not read the physics world, so the probe's update snapshots each support's transform and velocities, and the gate carries the snapshot forward to its own moment; a stale or changed body leaves its foot unsupported. Foot probes ignore only the character, so the vehicle carrying him can be a support.

A planted foot and a swing are held in their support's frame and rebuilt each gate, so a deck carries the feet and a step taken on it lands on it. The rules read the body's travel relative to the support (`body_rig::contact::SupportFrame::carry`).

## The root

While the IK owns the standing body, the grounded rig's drive follows the floor point under the FK pelvis instead of committing steps after the neck ([rig.md](rig.md#steps)); the step phase reads `following`, and no walk is added to the stick. A bend barely moves the FK pelvis in the room, so it moves no root; a physical step does, and the feet step for themselves. The ledger, shift, push-back, and recenter are unchanged, and the rig's own step commit takes the root back whenever the IK does not own it.

## Per state

Each state asks for a weight on five channels (`body_rig::ik_weight`): the pelvis's position, its orientation, the spine, the planted feet, and the airborne legs. Weights move at a constant rate, so every hand-over takes `gate.blend_s`, and a partial weight mixes local rotations so no segment shortens.

| State | When | Weights |
|---|---|---|
| idle | on foot in a standing idle with a fresh estimate | all but the airborne legs |
| walk | the stick or the game's walk moving Rico | the pelvis's orientation and the spine at `gate.walk_weight` |
| clip | landings, turns, weapon acts, authored motions, hit reactions | none |
| airborne | a jump, or a fall while the torso is still the player's | the pelvis, spine, and airborne legs |
| climb | hand climbing | the feet (braces) and the airborne legs |

In the air the ankles are drawn toward a light air pose under the hips. The takeoff and landing are the clip's, and contacts are acquired afresh after a landing. The IK stands down, with the reason in the readout and log, when the procedural torso is off, under the gamepad scheme, while the firm ragdoll owns the legs, without a fresh estimate (except climbing), and in every case the [hand-over table](procedural-body.md#the-hand-over-table) leaves to the game. A teleport or recenter drops the contacts. While the IK owns the standing body the act filter drops the game's turn-in-place acts, so a body turn is the twist rule's steps ([locomotion.md](locomotion.md#the-turning-animations)).

## Feet on the face

While the hands climb ([hand-climbing.md](../input/hand-climbing.md#the-body)), the IK takes the legs so the idle clip's legs do not dangle off the face (`ik_body::climb`, `body_rig::brace`). Each foot casts from its hip along the facing and from its ankle forward and down, where the ground probes cast; a hit on a face that looks toward the hip, within the leg's reach, becomes a brace, with the sole along the face and the knee hinted toward it. A brace is held in its support's frame, so the foot stays put as the body climbs past; out of reach, the foot arcs to a new candidate or fades into the hanging air pose.

## The firm ragdoll's muscle target

On the grapple line and under the canopy the firm ragdoll owns the legs ([procedural-states.md](procedural-states.md#the-firm-ragdoll)), and its pre-physics step drives its bodies toward the producer's current pose, which is this tick's raw animation ([ragdoll.md](../../engine/character/ragdoll.md)): left alone, they chase the clip, not the drawn body. `firm_ragdoll::target` composes a better target: every bone above the hips where the last tick drew it (so the keyframed arms match the drawn arms), the hips and legs in the clip's local rotations hung from that spine, and each leg aimed along the IK body's air-pose leg. The lower body cannot come from the previous pose, because those bones are the blended physics and a target taken from them would chase itself.

The target is handed over as a stand-in pose producer (`firm_ragdoll::stand_in`): the game's pose header pointed at the mod's storage, flagged so nothing frees it, with both spaces marked in sync. The previous pose is read in a copy, composing any bone flagged [`HKA_POSE_F_BONE_MODEL_DIRTY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HKA_POSE_F_BONE_MODEL_DIRTY) from its local transform. A detour on [`RagdollInstance::PrePhysicsUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ragdoll::RagdollInstance##func-PrePhysicsUpdate) hands the stand-in to the local player's instance only; the game's producer, which the blend and skinning keep using, is untouched.

## Settings, readout, and logs

The debug UI's Body fit has `IK pose as the firm ragdoll's target` (`ragdoll_target`, on) and an `IK body` section with the contacts' settings; the IK body itself has no switch. The readout shows the state and stand-down reason, each channel's weight, each foot's state and support, and the step counts. The `ik body` log source reports state changes, each step with its rule, walk hand-overs, lost supports, braces, and a summary every ten seconds of standing and at the end of each spell and climb; these lines land in recordings as diagnostic events.

## The offline replay

`tracking-synth rig-eval --weights <egoposer.safetensors> --ik [--contact <json>]... <recording> <out.csv>` (`tools/tracking-synth/src/rig_eval/ik/`) replays the FK pelvis and the contact model over a take's recomputed estimate, wherever the IK would own the standing body, on flat ground with instant probes and a root that follows exactly. It reports steps per standing minute by rule, hand-overs, out-of-reach spells, the drop the feet needed, and the steps released by glances; the release thresholds are tuned with it to a few steps a minute, few per glance, and no hand-over while standing. It leaves out the root, the clip's feet, and real ground, so HumanIK's response to a held pelvis and planted feet, moving decks, and braces on vertical faces still need the headset.
