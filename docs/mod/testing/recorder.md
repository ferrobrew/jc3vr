# Tracking recorder, replay, and scenarios

The test tools make the mod testable without anyone in the headset. The tracking recorder writes what the OpenXR runtime gives the mod each frame to a file; a replay plays that file back into the game in place of the runtime, with or without a headset; and a scenario names a recording and what must happen while it replays, and reports PASS or FAIL for each expectation. A headset session records a feature being used once, and the scenario checks it on every build afterwards.

The code is `payload/src/recorder/tracking/` (the recorder, and the conversion between the payload's types and the file's), `tracking-format/` (the file format's data model and codec, a crate of its own so that host tools build it on Linux and Windows), `tools/tracking-synth/` (the synthetic recordings), `payload/src/testing/` (the replay, the scenario runner, the signals, the log tap, and the request file), the seams in `payload/src/vr/replay.rs`, `payload/src/vr/input/replay.rs`, and the frame loop, and the Testing tab in `payload/src/ui/testing.rs`. The scenario templates are in `testing/scenarios/` at the repository root.

## Starting and stopping

Nothing runs until it is started, and `testing.enabled` (on by default) switches every way of starting the tools off. There are three ways:

- The debug UI's Testing tab, on the desktop or in the headset through the floating panel. It records, replays a file, lists and runs the scenario files, shows a running scenario's verdicts as they settle, and lists every signal with its live value.
- The director's key bindings ([trailer-tooling.md](../trailer-tooling.md)): Ctrl+Numpad/ starts or stops a tracking recording, Ctrl+Numpad* replays this session's last recording from its start marker or stops the replay, and Ctrl+Numpad- runs the last scenario again or stops it. A keymap saved by an earlier build keeps its own list; an action it does not bind takes its default chord unless the saved list binds that chord to something else.
- The request file, for a script. Every second the game looks for `test_request.toml` in the configuration directory ([settings.md](../settings.md)), reads it, deletes it, and acts on it. `testing.request_file` turns the watch off.

```toml
command = "scenario"  # record, stop_recording, replay, stop_replay, scenario, or stop_scenario
path = "climb.toml"   # replay and scenario: relative to the scenario folder, or absolute
start = "marker"      # replay only: marker, bookmark, or in_place
pacing = "real_time"  # replay only: real_time or frame_locked
```

Under Proton the configuration directory is inside the game's prefix, so a script writes the file there.

## Recording

A recording needs a player character to mark its start at, and is refused while a replay runs. It starts at the top of the frame after the request, which is where the player's transform can be read, and samples once per VR frame at the end of the frame loop's begin, after the views are located and the controllers sampled; without a running session it writes no frames until one starts. Each frame records what the mod published, not the runtime's raw answers:

- Both eyes' poses in the cockpit frame (re-based against the recenter baseline) and their fields of view, when the runtime located the views.
- Both controllers' published frames: the grip and aim poses with their validity and tracking flags and velocities, every raw input (triggers, grips, sticks, trackpads, buttons, and touches), the derived presses, the interaction profiles, the focus, the mode set, and, with `testing.record_hand_joints` (on), the tracked hand joints.
- The semantic actions' per-hand states for `global` and the active mode set.
- The session state, whether the runtime asked for the frame to be rendered, whether the view pose was valid, and the STAGE floor height and play area.
- The recenters that took effect before the frame, with their kind, cause, and turn.

The header holds the start marker (the player character's world matrix, and the name of a director bookmark within two metres, if any), the build stamp, the stamp of the recording's start, the semantic action schema's version, the dominant hand, the world scale, and the director's time scale.

The file goes to `sessions/<stamp>/tracking/<stamp>.jc3trk` beside the payload DLL. A writer thread appends input records and their frame as one queue batch, so queue pressure cannot accept only one part of that batch. A rejected batch increments the frame-drop counter and retains an explicit input-loss boundary for the next accepted batch or the final writer drain. Oversized batches, encoding errors, and I/O errors are failures rather than queue drops. The Testing tab shows frames, drops, accepted encoded bytes, duration, and the matching writer's completion error. Accepted stored bytes are not a durability guarantee. The complete logical-byte count includes only fully accepted original batches and the preamble. Full hand joints dominate the snapshot size; input records add source and observation data. The default recording remains uncompressed version 2. The debug UI's experimental compression option selects independent lossless version 3 chunks for the next recording without changing sample rate or hand-joint capture. Representative real-capture performance acceptance is still pending.

Stopped writers retain their owners until termination is confirmed. Completed writers are reaped without waiting for disk on the game thread. Starting another recording is refused when the bounded writer-owner capacity is full. Shutdown reports unsafe unmapping while any retained writer still runs; a stopped writer's I/O error and its thread liveness are separate outcomes.

### File format

The format is a real wire format with a version, and its decoder is covered by round-trip tests. All numbers are little-endian. A file is the magic `JC3VRTRK`, a `u16` format version, a `u32` header length and the header, and then records until the end of the file, each a tag byte, a `u32` length, and the payload. Version 1 contains snapshot frames. Version 2 retains the same header and frame layout and adds independently versioned input-observation records. Snapshot-only synthetic recordings still use version 1; decoding them does not invent input order. Version 3 stores each atomic batch in an independent checksummed chunk, using LZ4 when smaller or raw storage otherwise. Compression runs in the writer thread. Decoding validates stored and expanded sizes, record counts, aggregate budgets, exact decompression length, and checksum before parsing inner records. A truncated final outer chunk preserves earlier complete chunks; a complete corrupt chunk is an error. Chunk budgets are resource limits, not a guarantee that the full decoded recording and conversion fit available memory. The replay loader bounds file reads before decoding. A reader skips a record whose tag it does not know, so a later version can add records an older build passes over, and a file whose last record was cut short (the game stopped mid-write) reads as the records before it. Any other malformed field is an error that names the field and its byte offset. The per-field layout is in `tracking-format/src/codec.rs`. The payload records and replays in its own types and converts them to the crate's field by field, so a field added to a recorded type does not build until the format accounts for it.

### Input observations

The sampler captures protected raw controls and the active semantic actions in one unordered sync group. Runtime change times are retained as metadata in their own clock; neither sampling-loop order nor different runtime change times establishes an observed button order. Each record retains logical time, clock and source generations, exact value bits, focus/context, source-query outcome, recording-relative capture time, and an actual controller-publication identity when one exists. Failed, absent, empty, and limited source queries remain distinct. Cached possible bindings do not identify individual contributors to a merged semantic value.

Bounded history loss and invalid sampling produce explicit discontinuities rather than merged taps. Recorded boundaries retain their original position and loss count when replayed in different batches. The replay scheduler uses recording-relative capture time, preserves the original logical stamps, and keeps replay history separate from live sampling. Legacy or incompatible input history is unavailable, not reconstructed from frame snapshots. These records do not yet include policy settings, accepted chord decisions, or output requests, and the new chord arbiter does not consume them in gameplay yet.

## Replay

A replay loads its recording on a thread of its own, checks it against this build, teleports the player to the start, and plays. The check warns about a different dominant hand or world scale, a cut-short tail, and skipped records. A different semantic action schema, or a set recorded with another number of actions, means the action indices do not line up: the semantic actions are left out and the bindings see none, while the raw inputs, which the gestures, grabs, pointers, holsters, and climbing read, still replay.

The start is one of:

- `marker` (the default): the recording's start marker, through the director's teleport, which streams the destination in when it is far.
- `bookmark`: the director bookmark the marker names, as it stands now, or the marker when it names none or the bookmark is gone.
- `in_place`: wherever the player is.

The replay waits until no teleport is in flight, the game is in gameplay, and the player has stood within three metres of the start for 60 frames, and gives up after `testing.teleport_timeout_s` (60 s).

While it plays, each rendered frame publishes every recorded frame that has come due, in order, so a press and release between two rendered frames still reaches the input latch. The pacing decides what comes due: `real_time` follows the recording's own clock, and `frame_locked` publishes one recorded frame per rendered frame whatever the frame rate, which repeats against the game's frame count rather than the wall clock.

The replay replaces the runtime at the seams the runtime publishes through, so every consumer sees it:

- The head. With a session rendering, the frame loop replaces the located views' cockpit poses with the recorded ones and keeps the live poses for the compositor, so a headset shows the recorded view wherever it is, on a head or on a desk. With no session, or a session that is not rendering (a headset off the head), the render update publishes the recorded head through the same path a VR frame takes, less the per-eye render parameters, and the camera renders the desktop view from it. The headpose source is the VR source while a replay plays, with or without a session.
- The controllers and the semantic actions. The input update publishes the recorded frames instead of sampling, after it has synced the actions, and still sends the haptics to the live controllers. With no session, the render update publishes them. The recorded display times are the recording's own clock, which the hands' motion history spaces throws and strikes by.
- The STAGE floor height and play area, which the posture reads.
- The recenters. Live recenters, including the one on resuming gameplay, are held off, and each recorded recenter's effects (the roomscale re-base, the body turned under the view, the torso estimate) are applied on the frame it was recorded on.

When the replay ends or is stopped, the published controller frames are cleared, so the last recorded hands do not hold what they held, and the live runtime takes over on the next frame.

## Scenarios

A scenario file is TOML in the scenario folder: `testing.scenario_dir`, or the `testing` folder in the configuration directory when that is empty. The Testing tab lists the `.toml` files there. Point `testing.scenario_dir` at the repository's `testing/scenarios/` (under Proton, its `Z:` path) to use the templates in place.

```toml
name = "climb a wall"                  # optional; the file's name otherwise
description = "Grip a wall, pull up, mantle onto the ledge."
recording = "../recordings/climb_wall.jc3trk"  # relative to this file, or absolute
start = "marker"                       # marker, bookmark, or in_place
pacing = "real_time"                   # real_time or frame_locked
settle_s = 2.0                         # how long to keep checking after the replay ends

[[expect]]
signal = "climb.climbing"              # a flag; by default it must hold on some frame

[[expect]]
signal = "climb.mantles"               # a counter; by default it must grow by at least one

[[expect]]
name = "no on-foot warning"
log = "the game left on foot"          # log lines containing this, at info or above
at_most = 0

[[expect]]
status = "CALLBACK"                    # a status row
detail = "GAME THREAD"

[[expect]]
signal = "procedural.pass_through_error_m"
check = "always"
below = 0.001
```

Each `[[expect]]` names exactly one probe:

- `signal`: a published signal (below). A flag reads true or false; a counter counts since the payload loaded, and with `delta` (the default for a counter) it reads how far it has grown since the replay started; a measure reads a number, or nothing while it is not measured.
- `status`: a status row ([status.md](../status.md)) by its label, in any case. It reads true while the row shows the `health` asked for (`ok`, `off`, `waiting`, or `failed`) and its detail contains `detail`. With neither given, the row must read `OK`.
- `log`: the number of log lines at info or above, since the replay started, that contain the text. `level` raises the floor to `warn` or `error`, and with a level the text may be empty, which counts every line at that level.

And one condition: `is` for a flag or a status row (true by default), or one of `below`, `above`, `at_least`, `at_most`, and `equals` for a number. A measure needs one; a counter and a log count default to `at_least = 1`.

The `check` decides over which frames the condition must hold: `eventually` on some frame (the default for flags and rows), `always` on every frame the value is present and at least one (the default for measures), `never` on no frame, and `at_end` on the last frame (the default for counters and logs). A frame is one rendered frame while the replay plays, and the frames of the settling time after it. The load and the teleport are not observed, and their log lines are dropped.

A scenario that cannot be read, or has an unknown signal, row, level, or health, a condition of the wrong kind, or more than one probe or condition, is refused with an error that names the expectation, before anything replays.

### Results

When the replay and the settling time end, each expectation's verdict goes to the session log at info for PASS and warn for FAIL, as `scenario: <name>: PASS: <what decided it>: <expectation>`, followed by a summary line with the count. The summary file `sessions/<stamp>/testing/<start stamp>_<scenario file>.toml` holds the same: the scenario, its file and recording, the build, whether it passed, the frame count, and each expectation's verdict and detail. A replay that fails, is stopped, or is replaced fails every expectation as not checked, with the reason.

The log probes read a tap on the log that queues every event at info or above while a scenario runs (`payload/src/testing/log_tap.rs`), whatever the log filter says, and leaves out the test tools' own lines. A burst beyond its queue loses lines, which the results report.

### Signals

The signals read the snapshots the mod's systems publish for the status rows and the debug UI. The Testing tab's Signals section lists them with their live values; the names are:

| Signal | Kind | Reads |
| --- | --- | --- |
| `replay.driving` | flag | A replay is publishing its frames. |
| `vr.rendering` | flag | A VR frame is rendering to the headset. |
| `vr.recenters` | counter | Recenters of the cockpit baseline. |
| `frame.p95_ms` | measure | The 95th percentile of the recent frame intervals. |
| `controllers.frame_fresh` | flag | A controller frame is published and fresh. |
| `controllers.scheme` | flag | The controller scheme is selected. |
| `controllers.on_foot`, `.swimming`, `.in_vehicle`, `.airborne` | flag | The mode detector's mode. |
| `hand_bodies.registered`, `.on_game_thread` | flag | The hand bodies' callback is registered, and has fired on the thread that registered it. |
| `hand_bodies.fires`, `.created`, `.contacts`, `.dropped_contacts`, `.blows` | counter | The hand bodies' counts. |
| `hand_bodies.left_in_world`, `.right_in_world`, `.error` | flag | A hand's body is acting in the world; the bodies hold an error. |
| `climb.climbing`, `.mantling`, `.error` | flag | A hand grips the world; the mantle runs; the climbing holds an error. |
| `climb.grips`, `.moving_grips`, `.releases`, `.mantles`, `.drops` | counter | The climbing's counts. |
| `stunt.hanging`, `.standing` | flag | The game holds the player hanging from a helicopter, or standing in a vehicle's stunt area ([stunt-positions.md](../input/stunt-positions.md)). |
| `stunt.hang_entries`, `.roof_entries`, `.releases`, `.refusals` | counter | The hangs the grips entered, the stunt areas the mantles entered, the hangs let go by opening the grips, and the entries the game refused. |
| `push_up.pushes` | counter | Pushes up with the hands pressed down ([physical-hands.md](../input/physical-hands.md#pushing-up)). |
| `push_up.lifting` | flag | The hands lift the body. |
| `articulated.grips` | counter | Articulated objects a hand took hold of ([physical-hands.md](../input/physical-hands.md#articulated-objects)). |
| `grab.taken` | counter | Targets the grips took ([grip.md](../input/grip.md)). |
| `grab.left_holds`, `.right_holds`, `.left_fist`, `.right_fist` | flag | A grip holds what it took, or is closed on nothing; missing while the grip model does not resolve the hands. |
| `doors.grips`, `.unlatches` | counter | Grips taken on latched vehicle doors, and the latches the hands undid ([vehicle-doors.md](../input/vehicle-doors.md)). |
| `doors.on_latch` | flag | A hand grips a latched vehicle door. |
| `characters.grips`, `.yanks`, `.knockdowns`, `.regrips`, `.hijacks` | counter | Grips taken on characters, the yanks, the yanks and shoves that knocked a character down, the fallen characters' ragdolls taken, and the seated characters pulled out through an open door ([character-grabs.md](../input/character-grabs.md)). |
| `procedural.rig_error`, `.on_foot`, `.stance_measured` | flag | The procedural body's rig failed; its case is on foot; the stance is measured. |
| `procedural.pass_through_error_m`, `.pass_through_error_deg` | measure | The pass-through's largest read-back error. |
| `procedural.seated` | flag | The torso is posed on the seat's hips in a vehicle ([body/procedural-states.md](../body/procedural-states.md)). |
| `procedural.wing_arms` | flag | Rico's arms follow the player's in the wingsuit, with both hands queued. |
| `procedural.parachute` | flag | The torso is posed on the harness's hips under the parachute, the legs hanging. |
| `procedural.parachute_flare` | measure | How far the hanging legs swing forward for the brake, as a share of the full flare, under the parachute. |
| `procedural.swimming` | flag | The torso is posed on the swim's hips in the water. |
| `procedural.roof` | flag | The torso is posed on the stunt idle's hips standing on a vehicle's roof. |
| `procedural.neck_error_m` | measure | How far the solved neck is from the point under the player's head, while the procedural body poses the torso. |
| `weapons.wielded`, `.in_left_hand`, `.in_right_hand`, `.two_handed` | flag | The wielded weapon and the hand its first gun rides. |
| `weapons.armed_in_left_hand`, `.armed_in_right_hand` | flag | A weapon the game does not wield is armed in that hand. |
| `weapons.armed_carried` | flag | A weapon armed in a hand is one the carry set keeps beyond the slots. |
| `weapons.draws`, `.docks`, `.drops`, `.promotions` | counter | Weapons drawn off a socket into a hand, docked from a hand onto a socket, and let go of from a hand, and armed weapons the reconcile made the primary's, wielded in the same hand. |
| `weapons.grabs`, `grip.flips`, `grip.pins` | counter | Weapon grabs, guns snapping on the wrist, and attach bones pinned. |
| `collide.contacts` | counter | Times a held long gun came against a wall. |
| `collide.touching` | flag | A held long gun is against a wall. |
| `regions.carries`, `.braces`, `.grips_taken`, `.let_go` | counter | Long guns taken by a part other than the grip, braced where the second hand closed on them, grips taken from a carrying hand or slid along to, and long guns left carried in the bracing hand when the holding hand let go ([physical-guns.md](../input/physical-guns.md#grabbing-a-gun-anywhere)). |
| `regions.carrying` | flag | A hand carries the wielded gun by a part other than its grip. |
| `inventory.carried` | measure | Weapons the VR inventory carries beyond the slots. |
| `inventory.restored`, `.sidecar_writes` | counter | Carried weapons respawned onto their sockets after a reset or a load, and save sidecars written. |
| `inventory.stashed` | flag | The carried weapons are stashed while a mission holds the player's weapons. |
| `inventory.with_others` | measure | Weapons the player let go of that another character holds. |
| `inventory.kept` | measure | Weapons lying in the world the VR inventory keeps from the game's despawn. |
| `inventory.pins`, `.resets` | counter | Weapons in the world the inventory started keeping, and the spawn system resets it saw. |
| `ads.sighted`, `.shouldered`, `.welded` | counter | Times a long gun's sights came onto the eye, its stock into the shoulder, and a scope onto the eye. |
| `magazines.dropped`, `.picked_up`, `.seated` | counter | Magazine props dropped and noted, magazines picked up from the world, and those seated. |
| `actions.worked`, `.locked_back` | counter | Actions worked by hand (a slide, charging handle, bolt, or pump cycled), and slides caught at the rear on an empty magazine. |
| `actions.held` | flag | A hand holds a gun's action, with the physical action on. |
| `actions.travel` | measure | How far back the action of the gun in hand furthest back is, as a share of its travel, with the physical action on. |
| `c4.thrown`, `.stuck` | counter | Charges thrown or pressed on from the pouch, and those pressed onto a surface by hand. |
| `grenade.pins_pulled`, `.thrown`, `.caught`, `.thrown_on` | counter | Grenade pins pulled, grenades thrown from the pouch, live grenades caught, and caught grenades thrown on. |
| `locomotion.stick_speed_mps` | measure | The speed the stick drives the body at. |
| `rig.gap_m` | measure | How far the ground point under the player's upright eyes is from the root, while a VR frame renders ([rig.md](../body/rig.md#readouts-and-signals)). |
| `rig.residual_m` | measure | How far that point is outside the lean the chase leaves alone: what the chase still has to close. |
| `rig.ledger_projected` | counter | Completed steps whose chase was credited by projection, because something stopped or deflected the root. |
| `rig.pushback_m` | measure | How far the push-back has slid the view back in its current episode ([rig.md](../body/rig.md#walls)). |
| `rig.obstructed` | flag | The ledger finds the player's chase obstructed: something stops the body. |
| `rig.pushbacks` | counter | Push-back episodes: times the view started being held at a wall. |
| `legs.moving` | flag | The procedural body's feet walk rather than step standing ([rig.md](../body/rig.md#the-legs)). |
| `legs.touchdown_lead_m` | measure | How far ahead of its hip the last walking foot landed, while the procedural body poses the legs. |

A signal is added in `payload/src/testing/signals.rs`, reading a published snapshot only, as the status rows do.

## The templates

`testing/scenarios/` holds a starter set covering the headset checklists in [physical-hands.md](../input/physical-hands.md), [hand-climbing.md](../input/hand-climbing.md), [controllers-and-roomscale.md](../input/controllers-and-roomscale.md), [steering-wheel.md](../input/steering-wheel.md), and the procedural body ([procedural-body.md](../body/procedural-body.md)). Each names a recording in `testing/recordings/` that does not exist yet, and says in its description what to do while recording it. To fill one: stand where the scenario should start (save a director bookmark there to have the marker name it), start a tracking recording, do what the description says, stop, copy the file from the session's `tracking` folder to the name the scenario expects, and run the scenario once to see that it passes against the build it was recorded on.

The `synthetic_*.toml` scenarios are the exception: their recordings are generated rather than recorded (below).

## Synthetic recordings

A synthetic recording is written from a script instead of recorded in a headset, so the body rig can be tested by a replay without anyone wearing one. `tools/tracking-synth` writes them:

```sh
cargo run -p tracking-synth --target x86_64-unknown-linux-gnu   # on Linux
cargo run -p tracking-synth                                     # on Windows
```

The `--target` is needed on Linux because the workspace builds for Windows by default. With no script named, the tool writes every built-in script to `testing/recordings/synthetic/<script>.jc3trk` in the repository it was built from. Naming scripts writes only those, `--out <dir>` writes elsewhere, `--rate <hz>` samples at another frame rate than 90 Hz, and `--list` prints the scripts with a line on each. The folder is git-ignored and the files are regenerated rather than committed: each runs to between 400 KB and a megabyte. Regenerate them after a change to the tool, the file format, or the semantic action schema.

Each built-in script is the recording of the scenario `testing/scenarios/synthetic_<script>.toml`, which names it as `../recordings/synthetic/<script>.jc3trk` and replays it with `start = "in_place"`. The scenarios check what the existing signals can tell (the replay plays, the recorded recenter is the only one, the controllers are fresh, the player stays on foot, the skeleton rigs, and the stick drives the body or nothing), and also check the rig ([rig.md, "Readouts and signals"](../body/rig.md#readouts-and-signals)): the chase's residual stays small outside a lean, nothing is credited by projection or pushed back on the open floor, the legs walk during stick walks and room walks and stand otherwise, and the touchdown lead stays under the reach bound. The rig's thresholds are provisional until they are checked against a run. The scripts are:

- `stand_still`: standing for twelve seconds with a relaxed stance's sway.
- `walk_room_forward`: walking 1.5 m forward, pausing, and stepping back to the start.
- `walk_room_square`: walking a 1.5 m square clockwise, turning the body right at each corner.
- `lean_forward`: leaning from the hips over a table, the head 35 cm forward and 20 cm down, and back.
- `crouch`: squatting 45 cm at the hips and standing up.
- `turn_in_place`: turning the whole body 90° left on the spot, the head leading, and back.
- `stick_walk`: standing still with the left stick pushed forward for 3 s at full, 3 s at half, then released.
- `stick_strafe_and_turn`: strafing right and left with the left stick, flicking the right stick right twice, and holding it left.
- `look_around`: turning the head 70° left and right and looking down at the floor, the torso still.

A script (`tools/tracking-synth/src/scripts.rs`) describes the player as a body rather than as three tracked devices. Its tracks are keyed over time and eased between keys with the quintic smootherstep, so every key is a moment of rest, as a body's moves start and stop. The body's tracks are where it stands on the floor, its heading, how far the hips drop, how far the torso leans forward about the hips, the head's yaw and pitch on the neck, and the amount of standing sway; each hand's tracks are an offset of the grip from where the arm hangs, the thumbstick, the trigger, the squeeze value and force, and the buttons held.

The rig (`tools/tracking-synth/src/rig.rs`) poses a 1.8 m player of standard proportions from the tracks and reads the devices off the skeleton: eyes at 1.68 m, 12 cm above and 9 cm ahead of the neck pivot and 63 mm apart, hip joints at 0.93 m, shoulder joints at 1.48 m and 0.40 m apart, and 0.69 m from a shoulder to the palm's centre. The arms hang from the shoulders under gravity, swing with the gait, and come forward in a squat, so the hands follow the body. A walk bobs the head and shifts the hips by the body's speed across the floor, and a standing body sways and its hands tremor by a few millimetres. The poses are in the cockpit frame with the standing head at the start as the origin, as after a recenter. The aim pose is the grip's moved 6 cm along the handle and pitched 40° down, an approximation of the Index's rather than a measured offset. Replay the scenarios with the body dimensions of a 1.8 m player, so the body rig's measurements match the recording's.

Every frame is a focused session that asked for the frame to be rendered, with the head located and both controllers active on the Index profile, tracked, with velocities, in the on-foot mode set. The derived presses use the payload's default thresholds, the grip's read from the squeeze force, and a hand round the handle reads a squeeze of 0.9 and no force. The semantic actions are `global` and `on_foot` at rest, but for `move` from the left stick and `turn` from the right stick's horizontal axis, as a right-handed player's default bindings read them. The tool indexes them by the payload's snapshot of the action schema, `payload/src/vr/input/semantic/schema.golden`, which the payload's tests hold to the actions it creates. The STAGE space reports a 4 m by 4 m play area and the eyes' height above the floor. There are no hand joints. The first frame carries a full manual recenter that turns nothing, so the replay starts the posture, the rig's chase, and the torso estimate afresh from the scripted head.

The header names the build as `tracking-synth` and its version, the stamp as `synthetic <script>`, the schema version from the snapshot, the right hand dominant, and a world scale and time scale of one. The start marker is the identity at the world's origin with no bookmark, because a synthetic recording was made nowhere in the world: replayed from the marker, it would teleport the player to the origin.

The tool's tests check that every script's recording reads back as written and that the poses and inputs are the ones the scripts describe, and unit-test the tracks' easing, the rig, and the inputs. The payload's tests generate every script, decode it with the payload's decoder, check that the replay would publish its semantic actions, and check that the left stick drives this build's `move` action.

## Limits

The game is not deterministic, so a replay reproduces the player's input, not the game's response to it. Physics, animation timing, AI, traffic, weather, time of day, and streaming differ between runs, and the game's frame rate decides when the sim ticks fall against the recorded frames. A scenario should check what the input makes happen (a grip took, a weapon is in a hand, a mantle completed, no warning fired) rather than exact positions, and a recording should start somewhere quiet. Other limits:

- The state the mod keeps across frames is live, not recorded: the posture and its seated reference, the chase, the torso estimate, the body calibration, and the settings. A replay starts from whatever they are, so start it from the same posture and settings the recording was made with; the replay warns about the dominant hand and the world scale.
- The semantic actions are the ones the bindings saw while recording. If the game takes the replay into another mode than the recording was in, the recorded mode set's actions are published, and the bindings of the mode the game is in see none.
- The head is held at the last recorded located views between them, and is not interpolated, so a desktop rendering faster than the recording steps.
- With a session running, the compositor shows the replayed view at the live head pose, so wearing the headset during a replay is uncomfortable.
- The per-eye render parameters, and what reads them (the world-space overlay, the scope lens, the HUD panel), are absent without a session.
- The game's own clock and pause are not part of the replay: a pause menu or a time scale during a replay desynchronises it from the game.
- A replay holds live recenters off, F7 included.
- A synthetic recording is clean: it has no tracking loss, jitter, or dropped frames, and its body moves as the rig models it rather than as a person does.
