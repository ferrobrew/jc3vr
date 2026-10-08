# Stick locomotion and the steady view

On foot the game moves Rico by his clips: the movement task's speed is the animation's root speed ([engine: the locomotion task](../../engine/character/hands-locomotion-and-state.md#41-the-locomotion-task-runs-at-idle-and-writes-the-proxy-input-directly)), so a start ramps up through a wind-up clip, a stop decays through a stop clip, a turning start swings the body, and a run surges with every step. The view hung from Rico's animated head as well, so all of that, plus the walk's bob and the idle's sway, reached the headset. Neither matches what the player's own body does, and both are discomforting.

The stick locomotion takes both away from the animation. The code is in `payload/src/controllers/locomotion/`. Every part has its own switch under `controllers.locomotion` and is on by default; the pause menu's `VR: COMFORT` page has the switches and the rates.

## The velocity

`locomotion::after_task` runs from the post-call detour on [`NStateTask_MovementLocomotionTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_MovementLocomotionTask##func-Update) (`roomscale::sim`), on the ticks where the task wrote the proxy's velocity, before the palms' push and the roomscale chase add theirs. It replaces the task's horizontal velocity, through [`Character::SetWantedWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetWantedWorldVelocity), with its own, and keeps the task's vertical component.

- The direction is the move stick, read from the player action map's move half-axes, turned into the world by the view's heading on the ground plane. The bindings already rotate the stick into the off hand's frame when `controllers.locomotion.direction` asks for it, so the view is the frame either way. Rico's facing never steers the movement: the body follows the torso estimate ([alignment.md](alignment.md)).
- The target speed is the game's own (`controllers.locomotion.stick.speed = "Game"`): the float the input tasks write at [`ObjectBlackboard::SPEED_ID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/blackboard::ObjectBlackboard::SPEED_ID), the walk, run, or backpedal speed for the stick's deflection, and zero where the game would stop Rico at a drop or a wall ([`SetupTargetSpeeds`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_InputLocoSetTargetSpeedsTask##func-SetupTargetSpeeds), [engine](../../engine/character/hands-locomotion-and-state.md#41-the-locomotion-task-runs-at-idle-and-writes-the-proxy-input-directly)). Distances and chases therefore stay balanced, and the ledge stop keeps working. `Fixed` uses the stick's magnitude times `run_speed_mps` instead.
- The velocity moves toward the target at a fixed rate (`accel::step`): straight toward it, by at most `acceleration_mps2` times the step while speeding up and `deceleration_mps2` while slowing or turning away. A start, a stop, and a change of direction all take a time proportional to the change and never overshoot, so there is nothing for the view to feel but a steady push. The defaults reach the run speed in about half a second and stop from it in about a third.
- After a gap in the task (a jump, a fall, a vehicle exit), the integrator starts from the proxy's real velocity, so a landing carries its speed.

The legs play the game's clips, at the clips' own speed and cadence, so they slide a little against the ground when the two speeds differ, unless the procedural body is on: its walk ([procedural-body.md](procedural-body.md#the-feet)) keeps the feet at the root's own pace and leads them along the commanded velocity.

The task's velocity, the target, the written velocity, and the proxy's velocity are logged once a second while moving (`locomotion: stick velocity`, debug level on the `controllers` target), and the readout is the Controllers tab's "Stick locomotion and view anchor" section.

The game's speeds are measured once each: for every target speed the stick holds for two and a half seconds, the root speed of the clips is averaged after the first second and logged at info level (`locomotion: the game's on-foot speed`, with `target_mps` and `root_mps`).

## The turning animations

With the stick driving the velocity, the animations that turn the body are left with nothing to do but swing it. The act filter (`locomotion::acts`, called from the `Character::QueueAct` detour) handles the local player's acts while the headset drives the view and the player is on foot on the ground:

- the rotate-on-spot acts and their warp flavors, the standing rotation starts, and the quarter and half turns are dropped. The body still turns: the orientation executor yaws the root toward the torso's heading.
- the turning run and walk starts are replaced by the straight start, and the turning stops by the straight stop (warp and non-warp alike), so the state machine still passes through its start and stop states. Each replacement goes through the game's act pre-flight ([`AnimatedModel::TryAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##func-TryAct)) first, and the original act is queued when the state machine would not take the straight one.

The act names are bound in the pyxis defs ([`EventIdSymbolTable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::symbol_table::EventIdSymbolTable)); they are the `NCharacter` ids the game registers at startup, so resolving them never adds a name to the table.

A filter that removes acts can leave the animation state machine waiting for one it never gets. The watchdog guards against that: the filter normally acts in short bursts, at a start or a stop, so when it has acted with no gap longer than 400 ms for `acts.watchdog_s` seconds, it lets every act through for `acts.release_s` seconds and logs `locomotion: the act filter acted without a break; letting every act through` at warning level. The readout counts the dropped, straightened, and refused acts and the releases.

## The steady view

`locomotion::anchor::stabilize` runs at the anchor capture at the entry of the pose-finalization pass (`hooks::character::body_ik`), once per sim tick, and decides what the view hangs from:

- on foot, the root-fixed anchor: Rico's root ([`Character::m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT1), and `T0` for the previous tick of the pose pair) carrying a neutral standing head, neck, and eye midpoint in model space. No clip moves it, so the view moves only with the root, at the stick's velocity, and with the player's own head.
- everywhere else (a vehicle, the parachute, the wingsuit, a grapple traversal, swimming, a ragdoll, and a fall that lasts longer than `view.jump_grace_s`), the animated head as before. A jump keeps the root-fixed anchor through the grace period.

The neutral pose is measured, not assumed: while Rico stands still on the ground, not aiming, the animated head, neck, and eyes are averaged over `view.capture_s` seconds of standing, once per session (`locomotion: measured Rico's neutral standing head (model space)`, info level). Until then the view keeps the animated head. The readout's button measures it again.

The anchor hands over between the two over `view.blend_s` seconds, eased, so leaving the ground or entering a vehicle never jumps the view.

On the ground, the root's height is smoothed by a critically damped follower (`view.height_smoothing_s`), so the steps of a staircase, which the proxy climbs in small pops, read as a ramp. The smoothed height trails by at most `view.max_height_lag_m`, and a change beyond `view.height_snap_m` (a drop, a teleport) passes at once. The readout shows the current offset.

Because the camera's anchor no longer contains the animation, everything that composes onto it does too: the roomscale cockpit, the head-height remap, and the body IK's head target all measure the player's head against the root-fixed head.

## What needs confirming in the headset

- Standing still after the measurement, the view does not move when the anchor switches on (the handover log line, and a root weight of 1 in the readout).
- Walking and running with the stick: no bob, no surge, constant-rate starts and stops. In the debug log, `commanded` follows `target` at the configured rate and `proxy` follows `commanded`; a proxy velocity that lags well behind means the proxy's own smoothing is adding to the ramp.
- With the act filter on, the state machine still enters and leaves movement, starts and stops look straight, and the watchdog's warning does not appear in normal play.
- A melee lunge, a hit stumble, and a ledge stop: the first two come through the movement task as root motion and are flattened to the stick's velocity; if one stops working, turn `SMOOTH STICK MOVEMENT` off and report which.
- A jump from a run carries its speed, and landing re-seeds the velocity from the proxy.
- Stairs and slopes feel smooth, and a long drop does not lag the view.
