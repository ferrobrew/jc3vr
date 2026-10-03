# Parachute locomotion and steering

How the game moves and steers a parachuting character. The load-bearing finding: **the parachute steering helper mixes the *camera input matrix* into the parachute's steering** — the camera-relative yaw/pitch look-steer block turns the chute toward where the camera points, not only where the stick points.

## The state task

Parachuting runs on its own movement state task, [`NStateTask_MovementParachuteTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::NStateTask_MovementParachuteTask##func-Update), which delegates to free functions in the [`input::parachute`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute) module: [`UpdateParachutePhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-UpdateParachutePhysics), [`GetParachuteSteeringValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-GetParachuteSteeringValues), [`UpdateParachutingTransforms`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-UpdateParachutingTransforms), [`ApplyConventionalParachuteVelocities`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-ApplyConventionalParachuteVelocities), and [`ApplyAnimationVelocityWithSteering`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-ApplyAnimationVelocityWithSteering). These free functions are in the `NParachuteMovement` namespace.

`NStateTask_MovementParachuteTask::Update` reads the parachute movement effectors (the pitch and yaw inputs through the action map, [`Action::MOVE_FORWARD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_FORWARD) through [`Action::MOVE_RIGHT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-MOVE_RIGHT)), calls `UpdateParachutePhysics` with the resulting `CVector2f` input, and then handles the cancel input that closes the parachute.

## The steering helper: `GetParachuteSteeringValues`

`UpdateParachutePhysics` calls [`GetParachuteSteeringValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-GetParachuteSteeringValues) each frame. It computes three outputs:

- `steering_values_out` — the yaw/pitch/roll steering mix for the frame;
- `wanted_char_yawpitchroll_out` — the desired character orientation;
- `look_steer_out` — the camera-relative look-steer vector.

The steering is the sum of two independent mixes:

**Stick input.** `input->x`/`input->y` through `m_XInputYawPitchRollAmount`/`m_YInputYawPitchRollAmount`: the plain controller steering, present even with no camera.

**Camera look-steer.** The function reads the camera input matrix ([`GameCameraManager::GetInputMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::game_camera_manager::GameCameraManager##func-GetInputMatrix)), decomposes it to Euler angles, and computes the yaw/pitch deltas between the camera and the character's current wanted orientation (`m_SteeringProxy`). A deadzone/max-yaw/exponential shaping block (`m_LookSteerDeadZone`, `m_LookSteerMaxYaw`, `m_LookSteerExponential`) converts those deltas into `look_steer_out`, which is then:

- mixed into `steering_values_out` on all three axes via `m_XLookYawPitchRollAmount`/`m_YLookYawPitchRollAmount` (the `v63/v61/v62` sum at the tail of the helper);
- consumed by `UpdateParachutePhysics` as the yaw/pitch velocity spring inputs (`v254.x` → `m_RotateCharYawToVelocity`, `v254.y` → `m_RotateCharPitchToVelocity`) and in the slingshot/animation-input blend (`1.0 - look_steer_out.x`).

So the parachute's heading follows the camera direction once the camera-relative yaw exceeds the look-steer deadzone — with or without stick input. The look-steer block only runs for the local player (`character->m_IsPlayer`); NPC parachutes steer from the stick mix alone.

## The stick's vertical axis

[`NStateTask_MovementParachuteTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::NStateTask_MovementParachuteTask##func-Update) builds the input vector from the action map's move half-axes: across is `MOVE_RIGHT` minus `MOVE_LEFT`, along is `MOVE_BACKWARD` minus `MOVE_FORWARD`, so a stick pulled back is a positive along axis. The across axis and the along axis steer through the input mixes above. The along axis does more: [`UpdateParachutePhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-UpdateParachutePhysics) makes it the target of the character's air-control lift and drag amounts, which follow it through the settings' `m_LiftInputSpring` and `m_DragInputSpring`. [`ApplyConventionalParachuteVelocities`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-ApplyConventionalParachuteVelocities) then takes the lift from the three-entry `m_Lift` by that amount: the middle entry at rest, blending toward the last entry with the stick back and toward the first with it forward, and the drag from `m_DragX`, `m_DragY`, and `m_DragZ` the same way. Pulling the stick back is therefore the parachute's slow-down only as far as the shipped tuning makes the last entries a higher lift and drag; the code path is established in the IDB, the shipped values are not read.

## Orientation application

`UpdateParachutePhysics` applies the heading through the parachute action params' proxy matrices ([`ParachuteActionParams`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::ParachuteActionParams) in `Character`): `m_PivoProxy` (the suspension pivot), `m_CharacterProxy`, `m_ParachuteProxy`, and the blended `m_SteeringProxy` that interpolates between them. [`UpdateParachutingTransforms`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-UpdateParachutingTransforms) copies the steering matrix into `m_SteeringProxy`, converts it to a quaternion, and drives the parachute rig's swing; [`ApplyConventionalParachuteVelocities`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-ApplyConventionalParachuteVelocities) and [`ApplyAnimationVelocityWithSteering`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-ApplyAnimationVelocityWithSteering) compute the velocity response. The character's world orientation follows the `m_SteeringProxy` rotation.

## The velocity-alignment term

Alongside the stick and camera mixes, `GetParachuteSteeringValues` computes a velocity-alignment yaw: when the horizontal velocity exceeds `m_VelocityAlignMinSpeed`, the wanted yaw is driven toward the velocity heading through `m_RotateCharYawToVelocity` (a `SCharacterSpring`), blended by `m_VelocitySteeringBlendScale`. This is what keeps the canopy squared to the travel direction when not steering.

## Related

- The airborne/fall family that *does* route through the on-foot-style helper: [`UpdateFallSteering`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::locomotion##func-UpdateFallSteering), documented in the pyxis defs (`input/locomotion.pyxis`), and [`NAirMovement_UpdateStdLookSteering`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/input::parachute##func-NAirMovement_UpdateStdLookSteering), the camera look-steer. The parachute does **not** use those; it is a separate movement core.
- The shared air tuning block [`CharacterAirMovementSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::CharacterAirMovementSettings) (the `m_LookSteer*`/`m_XInputYawPitchRollAmount`/spring fields) is also embedded in the wingsuit and freefall settings.
- Pyxis defs for the parachute family: `input/parachute.pyxis`.