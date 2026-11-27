# Swimming

In water the right stick owns the swimmer's heading as it does on foot, the left stick swims forward along it, the head's pitch dives and climbs, and the player can also swim by stroking with their arms. The game's swim locomotion is in [swim-locomotion.md](../../engine/gameplay/swim-locomotion.md); Rico's torso and arms in water are in [procedural-states.md](procedural-states.md#swimming). The detours are in `crates/jc3vr-body/src/hooks/input/swim/` and the strokes in `crates/jc3vr-body/src/controllers/locomotion/strokes/` (the pure part in `strokes::model`).

## Why the heading needs taking over

The view is composed on the body frame, so the body's yaw is the view's yaw. On foot that yaw is smooth because the mod drives the game's rate-limited orientation executor. The swim family bypasses that executor: outside a turn act the movement core turns the body toward the camera-relative stick, so looking somewhere and pushing forward turns the body toward the camera; and past [`SWIM_TURN_ACT_MIN_ANGLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::SWIM_TURN_ACT_MIN_ANGLE) the input tasks ([`NStateTask_InputSurfaceSwimTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::NStateTask_InputSurfaceSwimTask##func-Update) and its underwater counterpart) queue a turn clip whose slerp replaces the rotation, so each step swings the world under the player. The armed idle's quickturns also measure against the aim, which follows the headset.

## What swimming means

Every detour keys on either of two live signals (`SwimSignals`): [`m_CurrentMotionState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_CurrentMotionState) reading [`MOTION_STATE_SWIMMING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##const-MOTION_STATE_SWIMMING), which both movement cores write each run, or the state bits `SB_IS_SWIMMING` or `SB_IS_UW_SWIMMING`, which hold every tick in the water while the motion state follows the cores' cadence. A recency window is not used, because it would leak the detours onto foot as the player walks out. The detours engage only while the head pose has a body-yaw target; on flatscreen it has none in water, so every detour passes through to the game's own swimming.

## The detours

All are scoped to the local player and gated on swimming:

1. Liveness. The two movement cores (`UpdateSurfaceMovement`, `UpdateUnderwaterMovement`) are wrapped to mark a per-thread window and to advance a liveness counter (`ModeLiveness`) that the head pose reads alongside the swim bits, so `xr::advance_body_yaw` keeps the VR body-yaw accumulator seeded in water. A tick read as out of the water would clear it, let the core turn the body toward the camera, and reseed from there, ratcheting a straight swim into a circle.
2. Heading. The target of [`DoRotate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/physics##func-DoRotate) is replaced with `headpose::body_yaw_target()`, so the game keeps its surface roll and capsule composition while the right stick turns the body. This also covers a neutral stick, aiming, and the underwater core, which never routes its direction through detour 3. Underwater the dive pitch is folded in, since the underwater core swims along its facing and a flat target would pin the swimmer to a plane. A second wrapper marks `ProcessMotion`'s window so its nested `DoRotate` (the velocity slerp) is left alone. The surface core's one `DoRotate` site also serves grapple and explosive warps, so those face the stick's heading rather than their target while swimming.
3. Move direction. `CControllerUtility::TransformInputDirToWorldDir` returns the heading's forward inside the core window only, because the helper has many callers across locomotion, jumps, the grapple, aiming, and AI.
4. Turn acts. `CControllerUtility::GetDeltaAngleFromOrientation` reports zero, so no turn act queues and its slerp never takes the rotation from detour 2. The on-foot aim-blend sampler shares the helper, which is why swimming must mean swimming now.
5. Refusal. `MOVE_BACKWARD`, `MOVE_LEFT`, and `MOVE_RIGHT` are cleared in the `InputDeviceManager::Update` detour. The swim family has no backstroke, and reversing the direction makes the swimmer paddle in place, turn round, or drift; sideways input would reach the cores as forward motion.

`CPfxCharacterInstance::SetOrientation` is not hooked: its matrix includes the swimming capsule offset, a quarter turn about X for the surface crawl, so a yaw rewrite there sits at gimbal lock and thrashes.

## Turn rate

The game's swim turn rates trail the right stick so far that the lead passes a half turn and the shortest-arc rotation reverses. So `DoRotate` gets the input tuning's `swim.turn_rate_deg_s`, against the core's own `dt` and capped per call at the angle `DoRotate` eases out over (larger steps overshoot, since it does not clamp to the remaining error), and the lead clamp of `xr::advance_body_yaw` (`turn.max_body_lead_deg`) applies in water as on foot. Tracking the stick one for one turns faster than the on-foot executor does, which the view tuning's `turn.swim_turn_scale` trades back.

## The dive control

While swimming the head is composed onto the body's yaw only (`level_while_swimming` in the head pose), because the body pitches toward where the player looks and carrying that pitch into the view would add it to the head's; the cost is the surface bob and roll. The dive command is the head's pitch relative to the body, not the camera's world pitch, which would feed back through the body frame and roll the swimmer vertical. The input tuning's `swim.pitch_deadzone_deg`, `pitch_limit_deg`, and `pitch_rate_deg_s` shape it, and the held pitch resets to level whenever the surface core runs.

## Arm strokes

The strokes act under the controller scheme with tracked hands, run from the bindings' finish stage while the gate reads swimming (`hooks::input::swim::swimming`); their tuning is the `strokes` section of `crates/jc3vr-body/tuning/locomotion.toml`. A hand sweeping back, palm first, swims forward, a breaststroke's outward sweep adds to it, and a stroke on one side alone turns the swimmer away from that side.

The game moves a swimmer only the way it faces, at the speed of the animation's root motion ([swim-locomotion.md](../../engine/gameplay/swim-locomotion.md#swim-speed)), and the stick's magnitude only picks between the crawl and its start, so the strokes cannot drive the stick. They keep a swim speed of their own instead. Each hand is a paddle at the controller's position in the cockpit frame (`hands::controller_grips`), not the drawn hand, which the reach limit stops short exactly where a stroke catches. Its velocity relative to the head is its motion through the water, read against the estimated torso's flat forward. A hand is in the water below a fixed depth under the eyes at the surface, and always underwater; a hand holding a gun strokes nothing.

A hand moving forward, or far behind the head, is recovering and pushes nothing, so the recovery never brakes the glide; it must reach ahead of the head and travel a little before it catches. A paddling hand's drive is its backward speed plus, in a breaststroke, a share of its outward speed, scaled by the palm's alignment with the motion over a wide cone that never falls to zero, since a hand round a controller rarely holds its palm square. The drive adds speed per metre of travel, which decays between strokes (the glide) up to a cap. Leaving the water, losing the hands, or a gap in the ticks resets the strokes, so no glide carries onto foot.

After each core runs, `hooks::input::swim::propel_strokes` (pure part `strokes::model::propel`) reads the velocity the core wrote, raises its share along the facing to the strokes' speed if lower, and writes it back with `Character::SetWantedWorldVelocity`. The facing is the core's own heading, level at the surface so buoyancy keeps the vertical and pitched underwater so strokes follow the dive. The rest of the core's velocity stays, and the stick's crawl still wins when faster. The core still picks its animation from the stick, so a swimmer moved by strokes alone treads water upright while it glides.

The strokes turn the heading two ways, summed and limited: the push's torque about the head (each hand's offset crossed with its push, smoothed so a crawl's alternate arms cancel), and a steer toward the way the hands push, which carries the heading through the push's angle over about one stroke, since the game would otherwise swim a sideways stroke straight ahead. The head pose takes the turn on its next input tick (`strokes::take_turn`) and adds it to the heading the right stick owns. Each catch knocks its hand (`Event::SwimStroke`; [haptics.md](../input/haptics.md)).

## Diagnostics

Entering and leaving the water logs `swim: the swim gate opened` or `closed` with the signals and whether a heading is available. While swimming, `swim: the heading` (target `swim`, from `telemetry.rs`) logs once a second with the target, the body's yaw and turn rates, the swim signals and counters, the torso's case, the hand tiers, and the strokes' state; a steady target turn with no look input is the ratchet, and the `untargeted` counter counts core runs that found no target. The debug UI's On foot live status shows the detours' counters and, while swimming, each hand's push, the strokes caught, the strokes' speed against the speed written, and the turn.

The water line is a fixed depth below the eyes rather than the water's height, and how strokes combine with a grapple or explosive warp's velocity is unverified.
