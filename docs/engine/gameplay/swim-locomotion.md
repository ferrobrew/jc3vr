# Swim locomotion

How the game moves and turns a swimming character. The load-bearing finding: **the swim family is a separate locomotion pipeline that never touches the on-foot orientation machinery** — no `NStateTask_LocoUtil::EvaluateCharacterOrientation`, no target-face-dir blackboard, no `GetAimMoveAngle` — and its body yaw changes in act-sized steps rather than continuously.

## The state tasks

Swimming runs on its own input and movement state-task pairs, one pair per stratum: [`NStateTask_InputSurfaceSwimTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::NStateTask_InputSurfaceSwimTask##func-Update) and [`NStateTask_MovementSurfaceSwimTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::NStateTask_MovementSurfaceSwimTask##func-Update) at the surface, and [`NStateTask_InputUnderWaterSwimTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::NStateTask_InputUnderWaterSwimTask##func-Update) and [`NStateTask_MovementUnderWaterSwimTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::NStateTask_MovementUnderWaterSwimTask##func-Update) underwater. The movement tasks delegate to free-function cores: [`UpdateSurfaceMovement`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::swim##func-UpdateSurfaceMovement) (player surface movement), [`UpdateSurfaceMovementNPC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::swim##func-UpdateSurfaceMovementNPC), and [`UpdateUnderwaterMovement`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::swim##func-UpdateUnderwaterMovement). The surface task branches on player or NPC, so `UpdateSurfaceMovement` is player-only.

## Swim speed

The stick's magnitude does not set the swim speed. Both movement cores take the horizontal magnitude of the animation's raw root velocity ([`AnimationController::GetRawRootVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetRawRootVelocity)) as the speed and the stick only for its direction, normalized, so how fast a swimmer goes is whatever the clip the state machine is playing moves its root at. With the stick at rest the surface core keeps that speed along the character's measured world velocity, so the clip's own slowing is the glide.

The input task turns the stick's magnitude into acts instead. At or above [`CharacterSwimSettings::m_SurfaceSwimFullDeflection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::CharacterSwimSettings##field-m_SurfaceSwimFullDeflection), a swimmer that is not already starting or crawling gets the forward start act (`ACT_SURFACE_SWIM_START_0`), whose clip accelerates into the crawl. Below it the task queues the crawl act (`ACT_SURFACE_SWIM_CRAWL`) until the stick has stayed under a tenth of full deflection for [`m_SurfaceSwimNoInputToStopTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::CharacterSwimSettings##field-m_SurfaceSwimNoInputToStopTime), and then the idle act. A partial stick from treading water therefore queues only the crawl act, and a recording with the stick held between a third and two thirds for several seconds shows the swimmer staying at a few centimetres per second, which suggests the state machine takes the crawl only through the start act. That is unverified: the transitions live in the shipped animation state machine, not the binary. A full stick takes about a second and a half to reach about 4 m/s, the start clip's acceleration. The settings come from a `.bin` resource, so the threshold's value is not in the binary.

The surface core then scales its velocity by how far it agrees with the move direction (the dot product, floored at zero), adds buoyancy, the waves' push, and the transition speed, and writes the result to the proxy's [`CharacterInput::wantedVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CharacterInput##field-wantedVelocity) in the frame of the orientation it then passes to `SetOrientation` (see "The capsule offset" below). The proxy's [`GetTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##func-GetTransform) reads that orientation back at once, and [`Character::SetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetWantedWorldVelocity) converts a world velocity with it, so a velocity can be read and replaced in the core's own convention after the core returns.

## Turn dispatch: discrete acts by angle threshold

The input tasks compute a desired direction and measure the signed XZ angle from the body's current forward with [`ControllerUtility::GetDeltaAngleFromOrientation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::controller_utility::ControllerUtility##func-GetDeltaAngleFromOrientation). The desired direction is:

- **moving**: the camera-relative move direction (stick through `GameCameraManager::GetInputMatrix`);
- **idle while wielding a weapon** (surface): the direction to the player's aim-target position;
- **grapple/planted-explosive**: the direction to the grapple or explosive target.

The angle picks an act, not a turn rate:

- An absolute angle below [`SWIM_TURN_ACT_MIN_ANGLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::SWIM_TURN_ACT_MIN_ANGLE) selects the forward crawl-start act (`ACT_SURFACE_SWIM_START_0`).
- An absolute angle from `SWIM_TURN_ACT_MIN_ANGLE` up to [`SWIM_TURN_ACT_MAX_ANGLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::SWIM_TURN_ACT_MAX_ANGLE) selects a 120-degree turn clip: `ACT_SURFACE_SWIM_START_120R/L` while moving, `ACT_SURFACE_IDLE_QUICKTURN_120R/L` for the armed idle, `ACT_UW_SWIM_IDLE_TURN_120R/L` underwater.

When a turn act is latched, the task writes the angle to the blackboard under the float key [`ObjectBlackboard::SWIM_TURN_ANGLE_ID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/blackboard::ObjectBlackboard::SWIM_TURN_ANGLE_ID) and stores the current forward and the desired direction in the character's swim action parameters (`m_AnimatedTurnBaseDir` / `m_AnimatedTurnTargetDir`).

## Orientation application

The movement cores own the actual heading. Each update, `UpdateSurfaceMovement` produces a desired forward one of two ways, and they are mutually exclusive:

1. **Animated turn (act-quantized).** If the **angle-correction animation segment** is playing, the heading is `SlerpAroundAxis(m_AnimatedTurnBaseDir, m_AnimatedTurnTargetDir, t)` ([`SlerpAroundAxis`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/physics##func-SlerpAroundAxis)), where `t` is the fraction of the turn angle taken from the segment's local time — the latched turn plays out across the clip window, then stops. This path short-circuits past everything below.
2. **Continuous rotation.** Otherwise the core rotates the character's current forward toward a desired direction with [`DoRotate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/physics##func-DoRotate), limited to [`CharacterSwimSettings::m_SurfaceSwimFaceDirSlerpSpeed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::CharacterSwimSettings##field-m_SurfaceSwimFaceDirSlerpSpeed) times [`SWIM_TURN_RATE_SCALE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::SWIM_TURN_RATE_SCALE) times `dt` degrees per update. A capture measured this rate at about 69°/s. The settings value comes from a `.bin` resource, so the figure is measured rather than read. The aim and planting-explosive branch overrides the rate with the flat rate [`SWIM_AIM_TURN_RATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::swim::SWIM_AIM_TURN_RATE). The desired direction is the world move direction from `TransformInputDirToWorldDir` while moving, the aim-target direction while aiming (at `SWIM_AIM_TURN_RATE`), the grapple target while reeling, or — with the stick neutral — **the character's own current forward**, so a swimmer with no stick input never rotates.

The resulting forward is blended with the water-surface plane for roll (`UpdateSurfacePlane`, not yet located) and assembled with [`Matrix4::CreateOrientation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::math::Matrix4##func-CreateOrientation).

`UpdateUnderwaterMovement` has the same two paths but differs in three ways that matter to any consumer substituting the direction:

- It **does not call `TransformInputDirToWorldDir`**. It builds the move direction inline, rotating the local vector `(forward, 0, -lateral)` by the camera's full input matrix — so the camera's *pitch* is baked in, and looking up or down while pushing forward is how the swimmer ascends and descends.
- That direction is three-dimensional throughout, and `DoRotate`'s output becomes both the facing and (scaled) the velocity, so **the swimmer moves where it faces**.
- Its rotation rate is a fixed `SWIM_TURN_RATE_SCALE` times `dt` degrees per update, slower than the surface rate.

The consequence: while a turn act runs the heading is quantized to the clip window, and outside one it only tracks the camera-relative stick. A third-person camera hides both; anything that composes a view on the body frame sees every step.

## The capsule offset: `SetOrientation` does not receive the facing frame

Before the orientation reaches the physics body it is composed with a fixed **swimming capsule offset**, chosen per frame by speed:

| Pose | Offset | Value |
|---|---|---|
| Surface, wanted horizontal speed at or below the idle threshold | [`m_SurfaceIdleSwimmingCapsuleOffset`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::swim_action_params::SwimActionParams##field-m_SurfaceIdleSwimmingCapsuleOffset) | identity |
| Surface, moving | [`m_SurfaceCrawlSwimmingCapsuleOffset`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::swim_action_params::SwimActionParams##field-m_SurfaceCrawlSwimmingCapsuleOffset) | a pitch about X of exactly a quarter turn |
| Underwater | [`m_UwSwimmingCapsuleOffset`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::swim_action_params::SwimActionParams##field-m_UwSwimmingCapsuleOffset) | a steep pitch about X, less than a quarter turn, plus a small downward translation |

The idle threshold and the exact angles are recorded on the fields.

All three are built in the [`SwimActionParams`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::swim_action_params::SwimActionParams) constructor. The core composes the selected offset onto its orientation, orthonormalizes, copies the matching inverse to `m_CurrentSwimmingCapsuleOffsetInv`, and only then calls [`PfxCharacterInstance::SetOrientation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##func-SetOrientation), which sets the physics body's rotation from the matrix and zeroes its angular velocity.

So the matrix handed to `SetOrientation` is the **capsule frame**, not the facing frame. For the surface crawl and underwater poses it is pitched steeply — the crawl case sits exactly at gimbal lock for a yaw/pitch/roll decomposition — and recovering a heading from it requires `m_CurrentSwimmingCapsuleOffsetInv` first.

## Related

- The on-foot orientation pipeline this family bypasses: [`NStateTask_LocoUtil::EvaluateCharacterOrientation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_LocoUtil##func-EvaluateCharacterOrientation), documented in the pyxis defs (`input/locomotion.pyxis`).
- Pyxis defs for the swim family: `input/swim.pyxis`, `input/controller_utility.pyxis`, `physics.pyxis`, `character/swim_action_params.pyxis`, and the blackboard id in `blackboard.pyxis`.
- `DoRotate` has three call sites in the binary: the surface heading rotation above, the underwater one, and the velocity slerp inside [`ProcessMotion`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/physics##func-ProcessMotion). It scales its step by the remaining angle divided by a fixed threshold angle, capped at one, times the rate. It does not clamp the step to the remaining angle, so a rate above the threshold per call overshoots and a rate above twice the threshold diverges. The threshold is recorded on [`DoRotate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/physics##func-DoRotate).
- [`ControllerUtility::TransformInputDirToWorldDir`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::controller_utility::ControllerUtility##func-TransformInputDirToWorldDir) is a general input helper, not a swim one: fifteen call sites spanning locomotion target-dir, jump, melee, grapple hang and reel-in, fall steering, aim target updates, an animation state-machine condition, and AI steering override. Only two are swim.
