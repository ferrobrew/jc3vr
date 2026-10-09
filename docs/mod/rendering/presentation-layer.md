# The render-rate presentation layer

The game simulates at 30 Hz and the headset displays at 75 to 90 Hz. Rico's pose is written once per tick, by the procedural body and the hand IK at the solve gate and by the head override after the solve ([procedural-body.md](../body/procedural-body.md)), and the skinning draws each frame between the last two ticks' poses ([skeleton.md](../../engine/character/skeleton.md#bone-store-hkapose-model-space)), so what the tick wrote is drawn up to a tick behind the player. The view and the hands were already placed from the latest tracking at render time (the late update in `payload/src/controllers/hands/late.rs`, [render-time-anchoring.md](render-time-anchoring.md)), which left the torso, the shoulders, and the arms between the shoulders and the hands a tick behind, and the hand bodies' offset stepping at the physics rate. The presentation layer re-poses everything attached to the player every rendered frame from the latest tracking. The code is `payload/src/controllers/presentation/`.

## What is drawn at which rate

| Part | Rate | Why |
| --- | --- | --- |
| The view | Render, from the latest headset pose | The camera's own interpolation with the head pose pair |
| The hands, the fingers, and the held gun and its parts | Render, by the late update's correction | The hand's correction since the ticks, on the palette and on the models |
| A hand body's offset (a hand held at a wall) | Render, blended between the physics steps | See [the hand bodies' offset](#the-hand-bodies-offset) |
| The arms | Render, solved again from the shoulders to the hands | See [the arms](#the-arms) |
| The spine, the neck, and the head | Render, bent after the headset's motion since the ticks | See [the torso and the head](#the-torso-and-the-head) |
| Held grenades and C4, a magazine in the hand, the scope's lens, and overlays on the hands | Render, on the hand as drawn | See [what rides the drawn hand](#what-rides-the-drawn-hand) |
| The hips, the pelvis, and the legs | Simulation, interpolated | They stand on the world: the feet are planted and the root's interpolation carries the body. A render-time edit would slide the feet |
| The hit shapes, the AI's view of Rico, the weapons' shot origins, and the attached effects | Simulation | They read the skeleton the tick wrote; the layer edits only the drawn copy |
| Objects the physical hands carry | Simulation, interpolated | The engine draws them from their own bodies' cached transforms; see [limits](#limits) |

## Where the pose is edited

The skinning job blends the two poses into the palette by the frame's interpolation fraction ([`PoseProducer::MakeSkinningPalette`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##func-MakeSkinningPalette)), and each render block uploads the palette per skin batch ([`RenderBlockCharacter::SetMatrixPalette`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockCharacter##func-SetMatrixPalette)). The detour on the upload already fed the player's body a copy of the palette for the head's collapse and the late update's hands (`hooks::graphics_engine::scene::render_block`), so the layer edits the same copy: after the engine's interpolation, for the local player's body model only (matched by the model instance's own render info, as the collapse is), in every pass so the shadow matches, and without writing the pose that the simulation, the hit shapes, and the next tick read. An entry is a bone's model-space transform times its inverse bind pose, so pre-multiplying it by a model-space transform moves the bone's vertices rigidly.

The upper body is re-posed as rigid groups, each rooted at one bone (`presentation::rig`): the spine joints `Spine1`, `Spine2`, `Sternum`, and `Neck`, the head, and per arm the upper arm, the forearm, and the hand. Every bone takes the edit of the nearest group root at or above it, so the clavicles ride the sternum, the twist bones their segment, the fingers and the attach bones their hand, and the facial bones the head. The groups are found by name in the skeleton once per skeleton, and a skeleton that lacks one of them, or hangs one from another place than the solve assumes (an arm not under the sternum), is refused with a warning (`presentation rig: ...`), and the draws fall back to the late update's hands.

Each tick, at the end of the pose pass (after [`Character::UpdatePropEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePropEffects) and the mod's post-solve fix-ups), the layer records the group roots' model-space positions as the tick left them ([`AnimationController::GetJoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetJoint)), the head pose the tick posed the body from, and the procedural torso's weight. Once per rendered frame, after the head pose pair is published, it snapshots the last two records, the render-time head pose pair, and the settings. A draw then interpolates the two records' joints by the frame's fraction, as the skinning does, and solves the upper body again (`presentation::solve`, pure and unit-tested). The solve is cached for the frame's other draws with the same instance matrix and fraction.

### The torso and the head

The head's motion since the ticks is measured at the neck pivot, below and behind the eyes ([procedural-body.md, "The player's head"](../body/procedural-body.md#the-players-head)): the render-time head pose interpolated as the camera interpolates it, against the two ticks' head poses interpolated by the same fraction, as a turn about the pivot and the pivot's travel. Measuring at the eyes would swing the neck whenever the player only turns the head. A motion past half a metre or a quarter turn (a teleport, a recenter) is not applied.

The spine bends after it: the four spine joints turn about their own origins, each by its share (more of it higher), so that the head's joint swings toward where the head has gone, by the procedural torso's weight, so a torso the animation drives is left as drawn. The bend keeps the bones' lengths and the spine's base, so it covers the head's travel in direction, not in distance: a crouch's drop over a frame is left to the next tick. The head turns by the head's turn about its own joint, and moves by whatever of its travel the bend did not cover, at most five centimetres. In first person the head is collapsed into the collar; the collapse point rides the neck's edit, so it stays inside the re-posed collar.

### The arms

Each arm is solved again with two-bone IK from its shoulder joint, carried by the sternum's edit, to the hand as the late update draws it, with the upper arm's and the forearm's lengths as drawn. The elbow bends toward where the drawn elbow is, which is the HumanIK solve's elbow and so already follows the elbow hint ([procedural-body.md, "The arms"](../body/procedural-body.md#the-arms)); when the drawn arm is straight, toward the elbow hint itself. With nothing moved since the ticks this reproduces the drawn arm exactly, so the layer does not move an arm the simulation agrees with. The forearm meets the hand as the late update's did: swung about the new elbow, and stretched along its length when the detach has moved the hand off the end of the arm. A hand the late update does not correct is held where it is drawn while the torso under its arm moves.

With `arms` off, the arms ride the torso's edit and the forearm swings to the hand about the carried elbow, which is the late update's own edit on a moving torso.

### The hand bodies' offset

A hand body's hand is drawn where its body is: the offset from the controller's palm to the body, measured at each physics step ([physical-hands.md](../input/physical-hands.md)). Added to the latest controller pose as one step's constant, it made a hand pressed into a wall sink in by the controller's travel since the step and spring back at the next, a visible stutter at the physics rate. The render-time hands (`hands::render_active_hands`, which the late update composes from) take the last two steps' offsets blended by the time since the latest step over the interval between them (`OffsetTimeline` in `physical_hands::physics::render`): the blend arrives at the latest step's offset as the next step lands, so the drawn hand moves continuously across the steps, one step behind them. The simulation's IK targets keep the latest step's offset. A hand pushed steadily into a wall is drawn as far into it as the controller travels in a step, without the stutter, and comes to rest on the wall when the controller stops.

## What rides the drawn hand

The late update's palette edit draws a hand at the solved wrist moved by the hand's correction, which falls short of the controller where the arm cannot reach (past the detach's cap) or while the hand blends to or from the animation. Anything placed in a hand or anchored to it at render time takes that drawn hand (`late::drawn_hands`), with the grip and the aim carried on it as the controller holds them, rather than the controller's own render-time pose: the primed and the caught grenade and the C4 charge (`hands::held`), a magazine carried in the hand during a reload, the scope's lens, the controls summary on the off-hand wrist, the hand-anchored HUD readouts, and the overlay's hand anchors. A hand the correction does not apply to is drawn where the tick solved it, and those follow it there. The gun and its parts already took the hand's correction itself.

## Cost

The layer's work is timed in the `presentation` slot of the frame breakdown (`perf: frame breakdown`, and the debug UI's Performance tab), and the layer keeps its own per-frame total across threads: the snapshot on the game thread and the draws' solves on the draw workers. A solve measured about 0.6 microseconds in a release build (16 in a debug build) on the unit-test machine; a frame solves once per distinct instance matrix and fraction, so the frame's cost is a few microseconds, bounded by the number of the player's body draws in the frame. The chain's bend stops after six passes at most.

## Settings and readouts

`controllers.hands.presentation`: `enabled` (on; `BODY AT THE FRAME RATE` on the `VR: BODY` page), `torso` (on), and `arms` (on); the debug UI's Controllers tab has all three next to the late update's switch, with a readout of the layer's state, the draws it re-posed, and its cost. Off, the late update re-poses the hands and the forearms alone, as before. The hand bodies' blended offset follows `bodies.render_at_body`.

The `VR: STATUS` page's `RENDER-RATE BODY` row (under `RENDERING`) reads `OK` with the number of the player's body draws the layer re-posed in its last active frame and its smoothed cost per frame in milliseconds; `WAITING` with why it is idle before it has run (not in the headset, no recent tick); `FAILED, NO RIG` for a skeleton without the bones; and `OFF` when switched off. The skeleton's rig is logged once (`presentation: rigged the player's skeleton for the render-rate body`).

## Limits

- Bullets, NPCs, the shot origins, and the attached effects see the pose the tick wrote, a tick behind what is drawn. Writing the render-time pose back into the skeleton would make the next tick start from it.
- Objects the physical hands carry are drawn by the engine from their own bodies, interpolated a tick behind the hand that carries them. Re-pushing them at the drawn hand would need a render update detour per object class.
- The head's vertical travel within a tick (a fast crouch) is covered by the head's own move, at most five centimetres, not by the spine, until the next tick's solve.
- The twist bones take their segment's edit; the twist spread over them is the tick's ([procedural-body.md, "The limbs' twist"](../body/procedural-body.md#the-limbs-twist)).

## What to check in the headset

1. The `RENDER-RATE BODY` row reads `OK` with a few draws and well under a tenth of a millisecond, and the log has the rig line once.
2. Look down at your chest and move your head quickly forward, back, and sideways: the chest and shoulders follow without trailing, and the body's shadow matches. Switch `BODY AT THE FRAME RATE` off and compare: the torso should then lag a tick.
3. Swing the arms quickly and punch: the elbows and upper arms move with the hands rather than catching up with them, the forearms stay attached at both ends, and the elbows point the way they did before.
4. Press a hand flat on a wall and push into it, then slide it along the wall: the hand stays on the wall without stuttering in and out. Move it off the wall: it follows the controller again with no lag.
5. Hold a primed grenade, a C4 charge, and a magazine during a reload, and reach as far as the arm goes: each stays in the hand. The wrist readouts and the controls summary stay on the wrist.
6. In a vehicle, with the torso on the animation: the arms still meet the hands, and the head turns without the spine bending.
