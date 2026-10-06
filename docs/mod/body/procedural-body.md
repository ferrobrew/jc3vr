# The procedural body

Rico's body has been the game's animation with corrections on top: HumanIK targets for the head, the torso, and the hands, a crouch that lowers the hips, and a stance that squares the feet ([head-and-body.md](head-and-body.md), [alignment.md](alignment.md)). The pelvis, the legs, and the shape of the spine stayed the clip's, so the player's waist and Rico's disagreed by whatever the clip did, and a hand on the player's hip landed beside Rico's. The procedural body builds the pose from the player instead. The code is in `payload/src/controllers/body/procedural/`, its settings are `controllers.body.procedural`, and the pause menu's `VR: BODY` page has the switch (`BODY FROM YOUR HEAD`).

It is off by default. The write seam has not been seen working in the headset yet; see the last section.

## Where the pose is written

The pose is written at the HumanIK solve gate: a pre-call detour on [`HumanIK::HasTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-HasTargets) (`hooks::character::gate`), acting for the local player's solver and the `MAIN` pass. That call has one caller, inside the pose-finalization pass, after every game writer of IK targets and the first weapon attachment refresh, and before the solve, the second attachment refresh, the model-space sync, the hit shapes, the AI's view of the character, and the attached effects ([skeleton.md, "Who reads the pose before `UpdatePropEffects`"](../../engine/character/skeleton.md#who-reads-the-pose-before-updatepropeffects)). A pose written there is the one bullets hit and NPCs see, which a write at `UpdatePropEffects`, where the head override is, would not be. The solver is found from the character, which embeds it, and the gate runs inside the HumanIK gate the game checks first (the global switch and the reduced level of detail).

The writes go through [`AnimationController::SetJoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-SetJoint) into the current pose, parents first, so each write carries the bones under it. The pose producer's pose pair ([skeleton.md](../../engine/character/skeleton.md#bone-store-hkapose-model-space)) makes the pose written this tick next tick's `T0`, so the skinning interpolates it like any other.

## The rig

The body drives the hips, the pelvis, both legs down to the foot, the four spine bones, the neck, the head, and the two clavicles (`LeftShoulder` and `RightShoulder`, measured for the chest's turn). `rig` finds them by name in the skeleton the animation controller holds ([`AnimationController::m_Skeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##field-m_Skeleton)), and checks that each bone's parent is the one the solver assumes: the legs hang from `Pelvis`, a sibling of the spine under `Hips`. A skeleton that differs is refused with a warning (`procedural rig: ...`), and the body stays the animation's. The skeleton is logged once per session (`body: skeleton bone`, one line per bone), which is how a live Rico's hierarchy can be checked against the shipped one.

## Modes

- `Off`: nothing happens at the gate.
- `PassThrough` (`TEST` in the menu): the animated pose of the driven bones is read and written back unchanged, then read again. Every five seconds it logs `procedural body: pass-through wrote the animated pose back` with the largest read-back error in metres and degrees, which should be zero. Nothing on screen should change; if anything does (a twitch, a weapon off the hand, the head override fighting), the write path is wrong.
- `Body` (`STANDING`): the standing body, below.

## The standing body

The body takes over while Rico stands on foot on the ground with the headset driving the view, the movement task running, and the stick and the body still. Walking, the air, vehicles, and every authored state keep the animation, and the hand-over between the two blends over `blend_s` seconds, eased. While the procedural pose has any weight, the pre-solve IK that would pose the same bones stands down: the body IK's shoulder, chest, and hand-pin targets, the roomscale crouch's hips and ankle targets, and the squared stance. The game's own MAIN-pass targets on the owned effectors (the hips, the waist, the chest end, the shoulders, the hip joints, the knees, the ankles, and the feet) have their reach and pull faded out by the same weight (`neutralize_targets`), so the solve after the gate leaves the pose alone. The head's own target stays: the body IK's head target closes what is left between the neck and the player's head.

The solver (`solver`, pure and unit-tested) works in Rico's model space from the animated pose:

1. The hips follow the head down: their height is the head's less the reference pose's head-over-hips height, at most the reference standing height (so legs are straight when the head is at standing height, which is also what a seated player gets, since their head is remapped to Rico's standing eye height), and at least `min_hips_height` of it. Horizontally they sit over the middle of the feet, moved back by `counterbalance` of the head's lean ahead of its neutral place over the feet, so leaning forward hinges at the hips. They turn to the player's torso heading from the torso estimate ([alignment.md](alignment.md)), at most `pelvis_turn_limit_deg`, measured on the line between the hip joints.
2. The chest twists to the torso heading, at most `chest_turn_limit_deg`, shared out up the spine (more of it higher), measured on the line between the clavicles. Then the spine swings so the neck points at the point under the player's head (the head position plus the animated head-to-neck offset turned by the head's rotation), shared out along the four spine bones and repeated three times to converge, never past `lean_limit_deg` from upright.
3. Each leg is solved with analytic two-bone IK so the ankle returns to where the animation put it, bending the knee in the plane it already bent in, and the foot keeps its animated orientation.

The rotations are composed onto the animated bones, so no bone's rest frame is assumed.

The Controllers tab's "Procedural body" section shows the mode, the weight, whether the body owns the pose, a rigging error, the pass-through's read-back error, and how far the hips sit below the animation's and the neck from its target.

## Not yet

- The feet are where the animation puts them, which while standing still is planted. Feet planted in the world that step when the body turns or moves away from them, and the gait while walking, are the next stage. They need ground heights: the engine casts its own ground rays inside the finalize pass, but the mod's scene casts have only been run from the locomotion post-hook, so the planted feet will start from the root's height and take casts from there.
- The arms are the controllers' wrist targets, solved by HumanIK on the procedural torso after the gate; the elbow hints, the clavicles' reach, and the torso's keep-out for the hands are later work.

## What needs confirming in the headset

1. With `TEST`, the read-back error in the log is zero, and nothing moves.
2. With `STANDING`, standing still: the readout's weight reaches one, the hips sit where the player's are (a hand on the hip lands on Rico's), the knees bend when the player crouches, and the feet stay on the ground. Turning the torso turns the hips and the chest within their limits.
3. The weapon stays in the hand, and shooting Rico's procedural body (from an NPC) hits where it is drawn.
4. The weight blends out as soon as the stick moves and back in on stopping, with no pop.
5. The engine's own reach for the owned effectors reads zero the tick after the gate (the body IK readout's engine values for the head stay non-zero).
