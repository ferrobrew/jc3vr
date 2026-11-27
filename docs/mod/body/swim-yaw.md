# Smooth swim yaw

The swim locomotion turns the body by the camera and by quantized turn acts; the mod gives the right stick the swim heading instead. The engine-side mechanism is [engine/gameplay/swim-locomotion.md](../../engine/gameplay/swim-locomotion.md); the implementation is `crates/jc3vr-body/src/hooks/input/swim/`.

## The problem

In VR the view is composed on the body frame (`body × cockpit`), so the body's yaw is the view's yaw. On foot that yaw is smooth because the mod drives the game's own rate-limited orientation executor every frame. The swim locomotion family bypasses that executor and turns the body two ways, neither of which the player owns:

- The camera steers it. Outside a turn act the movement core rotates the body toward the world move direction, which is the camera-relative stick. Looking somewhere and pushing forward turns the body toward the camera.
- Turn acts quantize it. Past the [`SWIM_TURN_ACT_MIN_ANGLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::SWIM_TURN_ACT_MIN_ANGLE) threshold the input tasks ([`NStateTask_InputSurfaceSwimTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::NStateTask_InputSurfaceSwimTask##func-Update) and its underwater counterpart) dispatch a discrete turn clip (the `ACT_SURFACE_SWIM_START_120R` and `ACT_SURFACE_SWIM_START_120L` family), and while that clip runs the core replaces its rotation with a slerp across the clip window. Each act-sized step rotates the whole world under the player.

Two further effects make water worse. The head pose's on-foot detector counts orientation-evaluator calls, which stop in water, so the VR body-yaw accumulator would clear and the stick would stop steering the body. And the armed idle's turn dispatch measures against the aim direction, which follows the headset, so treading water with a weapon and looking around fires the `ACT_SURFACE_IDLE_QUICKTURN_120R` and `ACT_SURFACE_IDLE_QUICKTURN_120L` quickturns.

## The detours

Five detours and a seeding change, all scoped to the local player and gated on swimming ([What swimming means](#what-swimming-means)):

1. Window and signal. The two swim movement cores (`UpdateSurfaceMovement`, `UpdateUnderwaterMovement`) are wrapped to mark a per-thread window around their call and to mark the swim cores' liveness in the body's frame (`ModeLiveness`). The head pose's sim reads that liveness as it reads the orientation evaluator's: an advance since the last input tick means swimming, which is passed into `xr::advance_body_yaw` so the VR body-yaw accumulator stays seeded in water rather than clearing the moment the player leaves foot. The swim state bits count as well, since the cores run on the character update's cadence, which need not give every input tick a run. A tick read as out of the water would clear the accumulator; for that tick every detour passes through, the core turns the body toward the camera-relative stick, and the next tick seeds the accumulator again from the turned body, so each such tick ratchets the heading toward wherever the head points and a straight swim curves into a circle. `HeadMode` itself is untouched: the flatscreen latch stays off-foot in water, so the override is inert on flatscreen, where the free camera already hides the quantization.
2. Heading rotation. The target of [`DoRotate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/physics##func-DoRotate) is replaced with `headpose::body_yaw_target()`. This is what turns the body: the core hands `DoRotate`'s output straight to `CMatrix4f::CreateOrientation`, so the right stick's heading takes over while the game keeps its water-surface roll blend and capsule composition. It also covers what the direction override cannot reach: with the stick neutral the core aims at the character's own forward, while aiming it aims at the weapon target, and the underwater core never routes its direction through the override. Two things beyond the target are replaced:
   - The rate, the input tuning's `swim.turn_rate_deg_s` ([Turn rate and the lead clamp](#turn-rate-and-the-lead-clamp)).
   - The pitch, underwater only. The body-yaw target is flat, and the underwater core swims along its facing, so a flat target would pin the swimmer to a horizontal plane, and rotating a pitched body toward it would spend the whole rate levelling out rather than yawing. Folding the dive pitch in ([The dive control](#the-dive-control)) gives both: looking up or down climbs or dives, and the yaw stays the right stick's. The surface core overwrites the vertical component with the water plane's, so the target stays flat there.
3. Move direction. `CControllerUtility::TransformInputDirToWorldDir` returns the body-yaw target's forward instead of the camera-relative stick direction, so the camera has no influence on where the swimmer goes. It is gated on the swim-core window, because the helper is general: it has many call sites across locomotion, jumps, melee, the grapple, aiming, and AI steering, and only two of them swim. The swim input tasks call it outside that window and are left alone: their output picks a turn act, which detour 4 suppresses, and feeds the dive check, which is better served by the real camera-relative direction. The underwater core builds its own direction inline and does not route through here.
4. Turn-act suppression. `CControllerUtility::GetDeltaAngleFromOrientation` reports zero, so the input tasks never cross their `SWIM_TURN_ACT_MIN_ANGLE` threshold. This matters for more than the animation: while a turn act runs, the core replaces its `DoRotate` rotation with the animated turn's slerp, which detour 2 cannot reach. AI behaviours are unaffected (they run for non-local characters), but the local player's on-foot aim-blend sampler and facing conditions share the helper, which is why swimming has to mean swimming now, not recently.
5. Backward and sideways refusal. The `MOVE_BACKWARD`, `MOVE_LEFT`, and `MOVE_RIGHT` effectors are cleared in the same `InputDeviceManager::Update` detour that consumes the look effectors, so the game never sees them: the left stick swims forward along the heading and nothing else, and the right stick turns the body, as on foot without the strafe. Backward input cannot simply be reversed: the swim family ships no backstroke, the cores move the swimmer where it faces, and pointing the move direction against the heading makes the character paddle in place, turn round, or drift sideways depending on which core and animation segment is live. Refusing it at the source means no act is queued, no stroke animation plays, the forward axis nets to zero, and both movement cores take their idle branch. Sideways input would otherwise reach the cores as forward motion, since the move-direction override returns the heading for any stick direction.

A sixth wrapper marks the window of `ProcessMotion`, which calls `DoRotate` a second time per swim frame to slerp the velocity direction, so the heading override skips that nested call.

The arm strokes add to both: their swim speed raises the velocity each core wrote along the way the swimmer faces, the faster of it and the stick's crawl winning, and their turn is added to the heading the right stick owns ([swim-strokes.md](swim-strokes.md)).

### The dive control

The view is levelled while swimming: the head is composed onto the body's yaw only, dropping its pitch and roll (`level_while_swimming` in the head pose). The body frame normally carries both into the view, which is right for a banking wingsuit and wrong in water, where the dive control pitches the body toward where the player looks: the two would add, so the camera's pitch would be the body's plus the head's, and the world would swing out from under the player. Levelled, the camera moves exactly with the head and the body's pitch only steers the swim. The cost is the surface bob and roll, which ride the same frame.

The dive control takes its command from the head's pitch relative to the body rather than the camera's world pitch. The two differ by the body's own pitch, and the world pitch is unstable: the view is composed on the body frame, so pitching the body toward where the camera points pitches the camera further, which pitches the body further, and without the levelling the swimmer rolls to vertical within a second. The neck angle, which the body's own motion cannot change, removes the loop independently of the levelling: holding the head down descends, and levelling it levels out.

Three values in the input tuning's `swim` section shape it: `pitch_deadzone_deg` keeps a resting head from drifting the swimmer, `pitch_limit_deg` stops well short of vertical, where a heading is undefined, and `pitch_rate_deg_s` slews the held pitch so a dive eases in rather than snapping at the yaw rate. The held pitch resets to level whenever the surface core runs.

### What swimming means

Every gate keys on either of two live signals (`SwimSignals`): the character's [`m_CurrentMotionState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_CurrentMotionState) reading [`MOTION_STATE_SWIMMING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##const-MOTION_STATE_SWIMMING), which both movement cores write each frame they run, or the state bits `SB_IS_SWIMMING` or `SB_IS_UW_SWIMMING`, which the controllers' mode detector classifies swimming by. The state bits hold for every tick in the water; the motion state follows the cores' cadence and has been seen not to read swimming when the input tick and the detours look, for a reason not established. A recency stamp is not used, because its window would leak every detour onto foot after leaving the water: backward input would be refused as the player walks out, and the on-foot heading and aim-blend paths would see the swim overrides.

The detours are engaged when the player swims and the head pose has a body-yaw target, which it has while the VR accumulator is seeded or the flatscreen latch is following. Without a target every detour passes through.

### Turn rate and the lead clamp

The body must keep up with the stick. The game's own swim rates are far slower than the right stick's turn, so at the game's rate the target runs away from the body: the lead grows past a half turn, where the shortest-arc rotation reverses, which shows as overshoot, a turn continuing after release, the body fighting back mid-turn, and a measured turn angle large enough to fire a turn act despite the suppression.

Two measures close the lead. The swim turn rate (`swim.turn_rate_deg_s`) is passed to `DoRotate`, capped per call at the angle `DoRotate` eases out over (given in its doc comment): `DoRotate` scales its step by the error over that angle, capped at one, without clamping to the remaining error, so a larger step overshoots and twice that diverges. The rate replaces the game's and is computed against the `dt` the core was called with, so the body tracks the stick rather than trailing it. And the lead clamp of `xr::advance_body_yaw` (the view tuning's `turn.max_body_lead_deg`) applies in water as on foot, anchoring the accumulator to the body again each tick, so the lead can never wrap past a half turn however fast the input moves.

Tracking the stick one for one also means the turn rate is the look input's own, where on foot the game's executor rate smooths it, so the same input turns faster in water. The view tuning's `turn.swim_turn_scale` ([tuning.md](../tuning.md)) trades that back.

### What is not hooked

`CPfxCharacterInstance::SetOrientation`. Rewriting the yaw of the matrix passed to it works while treading water and breaks the instant the swimmer moves: that matrix is the orientation composed with the frame's swimming capsule offset, and the surface crawl offset is a quarter turn about X, which is exact gimbal lock for a yaw, pitch, and roll decomposition. The rewrite lands a half turn out on every moving frame and the body thrashes between the two attractors.

The surface core's single `DoRotate` site carries every branch it can aim at: the move direction, an aim target, a grapple target, and a planted explosive's target. The override takes them all, so a grapple or explosive warp begun while swimming faces the right stick's heading rather than its target. Distinguishing the branches is not possible from inside `DoRotate`, and it has not shown as a problem in play.

## Telemetry

When the player enters or leaves the water, an info line `swim: the swim gate opened` or `swim: the swim gate closed` (target `controllers`, and a diagnostic event in a recording) says which signals read swimming and whether the head pose has a heading to give the detours; without one they pass through and the game's own swim steering runs. While the local player swims, an info line `swim: the heading` (target `swim`) is logged once a second from the input tick (`crates/jc3vr-body/src/hooks/input/swim/telemetry.rs`). It carries the body-yaw target and its turn rate, the body's yaw, ground heading, and turn rate, the turn input, the two swim signals, the advance of each swim counter, the last heading rotation a core asked for and was given, the procedural torso's case and weight, the effective hand tiers, and the strokes' state. With no turn input the target moves only when the accumulator is seeded again or clamped, so a steady `target_turn_deg_s` with `look_x_peak` near zero is the ratchet. The `untargeted` counter counts the core runs that found no body-yaw target, during which the game's own heading turns the body.

## Validation

The debug UI's On foot feature shows `Swim: cores / dir-overrides / rotate-overrides / turn-suppressions / move-refusals / untargeted` in its live status. In water under VR the first four advance and `untargeted` holds still; on foot all hold still.

In the headset:

- Swimming in a straight line at the surface and underwater with no right-stick input for half a minute, with the head turned a little to one side for part of it, the swimmer holds the heading, and the telemetry's `target_turn_deg_s` and `body_turn_deg_s` stay near zero with `untargeted` at zero.
- The right stick spins the body in place with no left-stick input and stops promptly on release, with no overshoot and no fighting back mid-turn.
- The left stick moves forward along the current heading and never turns the body; pulling it back or pushing it sideways does nothing, with no stroke animation.
- The camera has no influence on the heading.
- Underwater, looking up and pushing forward climbs to the surface, and the view pitches exactly as far as the head does.
- No quickturn snaps when looking around with a weapon out.
- Diving and surfacing work, and the surface bob and roll are still present.

On flatscreen the mechanism is inert: `HeadMode` is derived from the on-foot orientation evaluator, which never runs in water, so the flatscreen latch yields no body-yaw target and every detour passes through to the game's own swim behaviour.
