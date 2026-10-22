# The sim-rate experiment

The game simulates at 30 Hz and the headset displays at 72 to 90 Hz, so everything the sim places reaches the eye up to a tick late, and smoothed by the engine's interpolation between the last two ticks. The sim-rate experiment runs the tick faster, at 45, 60, 75, or 90 Hz, to answer two questions with a measurement rather than a guess: what a faster tick costs on the game thread, and what breaks in the game and in the mod when the step is not 1/30 of a second. It is a debug switch, not a feature. The code is `payload/src/sim_rate/`.

The frame breakdown's tick split and phase timing ([frame-timing.md](frame-timing.md#the-sim-tick)) are the cost half of the measurement, and the checklist below is the breakage half.

## Running it

The switch is "Sim rate (experiment)" in the debug UI's Game tab, under "Game". It is the setting `sim_rate.rate`, with the rates 30, 45, 60, 75, and 90, and it is not saved to the settings file. It defaults to 30, the game's own rate, at which the experiment changes nothing. Unlike a gameplay feature, which ships switched on ([settings.md](../settings.md)), it defaults to the game's own rate and stays out of the pause menu: it is a test instrument, and a player who switched it on by accident would get a different game. Because it is not saved, every session starts at the game's own rate, and a test sets the rate again each time.

The rate applies at the top of the next frame, ahead of that frame's ticks, and the readout under the switch shows what is applied: the rate, whether the physics pin is removed, the game's own update frequency field, the interpolation override, and the most ticks one frame may run.

## What it changes

Above 30 Hz, `sim_rate::frame_tick` makes three changes through the hooks' patcher, which keeps the original bytes:

- It writes the rate into [`Game::m_UpdateFrequency`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##field-m_UpdateFrequency). The engine derives each tick's step from it every frame, puts the step in the update contexts, and forces the clock to the rate during the sim, so the code that reads its time from the update context or the clock follows.
- It removes the physics step's pin. [`PhysicsSystem::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem##func-Update) replaces any time it is given with its 30 Hz constant before it steps the world, so a faster tick with the pin in place runs the physics two or three times too fast. The patch replaces the one instruction that copies the constant ([`PhysicsSystem::UPDATE_STEP_PIN_OFFSET`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem)) with `nop`s; the constant itself is shared with the vehicles' post-solve update and stays.
- It forces [`Game::m_InterpolationOverride`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##field-m_InterpolationOverride) to the accumulated method. The gameplay method runs at most one tick a frame and drops the rest, so a frame rate below the tick rate would slow the game down. The accumulated method runs the frame's whole ticks, up to the engine's cap of [`Game_MaxGameUpdatesPerRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/game) a frame; a frame that would need more still loses the time. This replaces any override the Game tab's own interpolation override chose.

Back at 30 Hz all three are unpatched, which restores the game's values and the instruction, and the patcher restores them when the payload unloads. A change logs at target `sim_rate`: a warning when the experiment comes on, with the rate, and an info line when it goes back to 30.

## The mod's own 30 Hz assumptions

The mod's code that assumed the 30 Hz tick reads the step from `crate::sim_rate` instead, so the experiment measures the game rather than the mod's mistakes. At 30 Hz each reads exactly what the code used before.

- The physical hands' pre-simulation step takes the time its callback is given, and `sim_rate::world_step_seconds` turns it into the step the world takes: the 30 Hz constant while the pin is in place, the given time once it is removed. The step publishes it before driving anything (`physics::step_seconds`), and the drives, the push from a lagging hand, the climb's velocities and mantle, and the proxy's velocity timestep all read it. The callbacks fire before the physics update pins its time, which is why the given time is the tick's ([`PhysicsSystem::FirePreSimulationCallbacks`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem##func-FirePreSimulationCallbacks)).
- The stunt position's vehicle speed divides the distance between the vehicle's last two tick transforms by the world's step.
- Counts of ticks that stand for a duration are scaled by `sim_rate::scale_ticks`, rounding up: the controller mode's debounce (`controllers.mode.debounce_ticks`), the climb's tolerance for a hand that stops publishing or acting and its mantle timeout, and the contact mutes after a hand body is placed or carried.

These were checked and needed nothing: the grapple comfort filter's landing-snap absorber measures the anchor's step interval in wall time between changes ([body/grapple-comfort.md](../body/grapple-comfort.md)), so it follows any tick rate; the headpose's pose pair is rotated at each input tick and composed from the engine's own previous and current tick transforms, so it spans one tick at any rate; the headpose's on-foot latch fires on the same tick cadence as the counter it compares; the grapple's physical pull, the bindings' holds, and the haptics take the time from their caller or the clock; and the remaining `1.0 / 30.0` constants in the payload are in tests. The weapon wheel's cancel clears the game's pending selection for a count of input ticks, which is a count of the game's updates rather than a duration, and is left as it is; the checklist covers it.

## Cost

A tick cost about 3 ms of game-thread time in the last profile, and most frames at headset rates fit only when they carry no tick ([frame-timing.md](frame-timing.md#the-sim-tick) splits the frames to check this). At 60 Hz roughly four frames in five carry a tick, and at the display rate every frame does, so expect the frame rate to fall to half (reprojected) unless the tick or the rest of the frame's critical path shrinks. The streaming block's budget is per tick, so its time per second grows with the rate while there is streaming work. Measure before reading anything into the breakage list: a frame rate below the tick rate makes the accumulated method run two ticks in some frames, and one that falls below half the tick rate hits the cap and slows the game.

## Test procedure

Run each item at 60 Hz and at the display rate (75 or 90), against 30 on the same save, in flatscreen first: the desktop covers most of it, and the VR paths add confounders. Then repeat the items marked VR in the headset. Keep the `perf` and `sim_rate` log targets on, and note the window's `sim:` part of the frame breakdown with each result. Load the save fresh for each rate, since the change applies mid-session but state from the previous rate (a body asleep, a timer started) carries over.

Record per item: the rate, the frame breakdown line over the test, what was measured, and the 30 Hz baseline. A stopwatch means the real clock; a game-time measurement means the clock or the mission timer the game shows.

### Cost

- The `perf` line's `frame_ms`, `frame_max_ms`, the `sim:` split (frames with no tick, one, and two or more, with their mean and worst), `sim_tick` and its phases, `ai_thread_1` and `ai_thread_2`, and `engine_update_render` at each rate, standing still in a town, driving fast, and in a firefight.
- In the headset, the runtime's reprojection rate and the frame-loop health line beside the breakdown.
- Whether frames with two ticks appear, and whether any frame needs more than the cap (the game slows: a stopwatch against the in-game clock drifts).

### Game speed

- A stopwatch against the in-game clock over two minutes.
- A timed mission objective or a challenge timer against a stopwatch.
- Walking 100 m on flat ground: the same time at every rate.

### Physics

- A crate dropped from 10 m lands in the same time (about 1.4 s) by slow-motion video or the stopwatch.
- A barrel rolled on a slope stops in the same distance.
- Stacked props settle and go to sleep, and do not sleep while still moving (the deactivation counts frames, so it comes sooner at higher rates).
- An explosion throws props the same distance.
- Ragdolls do not jitter or explode, and get up as at 30.

### Character

- Run and sprint speed over a measured distance; jump height and air time; the fall damage threshold from a known height.
- Ledge grab and mantle, swimming, and landings, with no landing animation playing twice.
- Hit reactions and the ragdoll settle condition.

### Animation

- Idle and locomotion blend times look the same; no foot sliding.
- Transitions are not skipped (conditions are evaluated per tick).
- Cutscenes: lip sync and scripted event timing (cutscenes already run the accumulated method).

### Grapple and tether

- Reel-in speed and stop distance, tether tension and retract timing, the hang's swing period.
- The wire does not jitter or break early, and the reel's trajectory prediction still aims where it lands.
- VR: the grapple comfort filter engages and releases as at 30 (its CSV telemetry, "Log reel telemetry" in the Camera tab, shows the anchor steps and any absorbed snaps).

### Wingsuit and parachute

- Glide speed and sink rate over a measured descent, the parachute's steering response, and the near-miss slow motion entering and leaving.

### Weapons

- Rate of fire (count shots over a timed burst), reload duration, overheat timing.
- Recoil kick size and recovery: the recoil is a per-tick impulse, so expect the camera kick to change; note by how much.
- Grenade fuse time and arc, projectile speeds against a target at a known range.

### Vehicles

- Top speed and acceleration time on a measured straight, steering, suspension (no oscillation), leaning on bikes.
- Boat wake, aircraft handling, and anything speed-dependent: the vehicles keep a speed history counted in ticks, whose window shrinks with the rate.

### AI

- Reaction time from being seen to the first shot, aim tracking, patrol speed, pathfinding (soldiers reach the player), and heat decay and escalation timers against a stopwatch.

### Effects and sound

- Smoke and particles trailing a moving car: the attached effects measure velocity per tick with a 30 Hz constant, so they inherit less velocity at higher rates.
- Doppler on a passing vehicle, rain and water sound probes.

### Slow motion and pause

- The death camera and scripted slow motion enter and leave at the right speed, and pausing and unpausing does not reset the game speed.

### Streaming

- Driving fast across the map: pop-in compared with 30, `sim_streaming`, and frame spikes in `frame_max_ms`.

### The mod

- Physical hands (VR): the hands' strength against a wall and a prop, pushes, pickups, and climbing a ledge with the mantle; the hand bodies' readout in the Controllers tab counts steps at the tick rate.
- The physical grapple pull (VR): a pull reads the same speed and depth.
- The controller mode (VR): no flicker between modes during a blend, and a real mode change takes as long as at 30 (the debounce is scaled).
- Headpose (VR and flatscreen): no camera stepping in a vehicle or under the parachute, and the on-foot latch holds while standing still.
- The weapon wheel (VR): a cancelled selection does not reselect the slot.
- Stunt positions (VR): standing and hanging on a moving vehicle's roof take and leave the stunt position at the same vehicle speeds as at 30 (the stunt logic reads the vehicle's speed from its last two tick transforms and the world's step).
- Haptics (VR): contact and tension cues feel the same.
- The render-time layers (VR): the presentation layer and the late update still smooth the body and the hands; at a tick rate equal to the display rate their correction should shrink to almost nothing.

## Risks

- Cost, above, is the likeliest reason to stop.
- Contacts, ragdoll motors, the vehicles' suspension, and the grapple wire's constraints are tuned at the 30 Hz step and may behave differently at a smaller one, usually stiffer and more stable, sometimes not.
- Per-tick quantities and tick-count buffers in the game (the vehicles' speed history, the attached effects' velocity, the recoil impulse, the physics engine's deactivation frame counts) change with the rate; the checklist names where each shows.
- Code that reads the clock's smoothed frame time directly during the sim, rather than the update context or the forced clock, is unaffected by the rate but was already frame-rate dependent; how much of it there is is unverified.
- Scripted content keyed to tick counts (graph scripts, sequences) is unverified.
- Saves record times in milliseconds or seconds, so the rate should not leak into them; mission timers are unverified.
