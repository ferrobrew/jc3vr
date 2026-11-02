# Tracking recorder, replay, and scenarios

The test tools make the mod testable without anyone in the headset. The tracking recorder writes what the OpenXR runtime gives the mod each frame to a file; a replay plays that file back into the game in place of the runtime, with or without a headset; and a scenario names a recording and what must happen while it replays, and reports PASS or FAIL for each expectation. A headset session records a feature being used once, and the scenario checks it on every build afterwards.

The code is `payload/src/recorder/tracking/` (the recorder, and the conversion between the payload's types and the file's), `tracking-format/` (the file format's data model and codec, a crate of its own so that host tools build it on Linux and Windows), `tools/tracking-synth/` (the synthetic recordings), `payload/src/testing/` (the replay, the scenario runner, the signals, the log tap, and the request file), the seams in `payload/src/vr/replay.rs`, `payload/src/vr/input/replay.rs`, and the frame loop, and the debug UI's Tools page features in `payload/src/ui/testing.rs`. The scenario templates are in `testing/scenarios/` at the repository root.

## Starting and stopping

Nothing runs until it is started, and `testing.enabled` (on by default) switches every way of starting the tools off. There are three ways:

- The debug UI's Tools page, on the desktop or in the headset through the floating panel. Tracking capture records, and Replay replays a file, lists and runs the scenario files, shows a running scenario's verdicts as they settle, and lists every signal with its live value.
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
- Both controllers' published frames: the grip and aim poses with their validity and tracking flags and velocities, every raw input (triggers, grips, sticks, trackpads, buttons, and touches), the derived presses, the interaction profiles, the focus, the mode set, and, with `testing.record_hand_joints` (on), the tracked hand joints. Each joint retains its pose, radius, and independent position/orientation validity and tracking flags.
- The semantic actions' per-hand states for `global` and the active mode set.
- The session state, whether the runtime asked for the frame to be rendered, whether the view pose was valid, and the STAGE floor height and play area.
- The recenters that took effect before the frame, with their kind, cause, and turn. From version 7 these include the follow bank, which folds a frozen turn of the body under the view into the cockpit frame ([`vr/recenter.rs`](../../../payload/src/vr/recenter.rs)); it is recorded as a yaw-and-height re-base with the cause `FollowBank` and the bank's turn.

Beside the frames, the recording carries the mod's own decisions as events, each stamped on the frames' clock and written before the frame captured after it:

- `controller_state`, once per game frame, read after the engine's render update so the late hand correction is evaluated at the sub-frame fraction that frame rendered with: the scheme, the mode and its traversal and hold refinements, the controller publication's sequence and age, the input subsystem's error, each hand's IK result, the hand tiers and their blend, the late correction's reason and wrist positions, the aim rays with their hits, the grapple aim, the weapon sight, the body rig's posture, root, step drive, push-back, seat, probe, and fade, from version 6, the root's latest on-foot tick (`root`, [rig.md](../body/rig.md#who-moves-the-root)), and, from version 7, the player character's measured motion (`character`): its measured world velocity ([`Character::m_MeasuredWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_MeasuredWorldVelocity)) and its height over the ground as its ground information last wrote it to the body rule system ([`AnimationRuleSystem::m_HeightOverGround`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::AnimationRuleSystem##field-m_HeightOverGround), the nearer of the ground and the water, and `m_HeightOverGroundExcludingWater`), zero while the proxy stands on the ground. These are what the animation graph's speed and height conditions read, among them the wingsuit landing's ([wingsuit-landing.md](../../engine/gameplay/wingsuit-landing.md)). It leaves out what the frames hold (poses, validity and tracking flags, focus, action set, and the STAGE information). The fields are independently read subsystem snapshots, not one atomic instant.
- `grip_acquisition`, at each stage of a grip acquisition: a grip press or a queued pickup request, with the chosen candidate and every candidate the resolver offered (its distance, tier, grab volume, the provider's reach, and why it lost to the winner), the reach provider's scan, and the pickup consumer's branch; the simulation consumer's result for a request; and the first drive of what a request took. Identities are the controller publication sequence, the per-hand input-read ordinal, the grip episode, and the request ordinal ([physical-hands.md](../input/physical-hands.md#interaction-evidence)).
- `take_marker`, at each take marker ([capture.md](../capture.md#session-recording)): the marker's session ordinal, local stamp, Unix milliseconds, game-run time, and label, the same fields as its copy in the video sidecar or `markers.jsonl`.
- `gun_release`, at each attempt to let an inventory gun fall: the route, the refusal, which release calls returned, the controller publication the release estimate was checked against, and the native drop's cached gun motion and the capped velocity requested of it.

Version 7 adds events noted as moments, when they happen rather than every frame:

- `body_state`, when the body's animation state changes: the current state of the body's rule system as its state-name hash, with the name when the mod knows it (`payload/src/controllers/diagnostics/state_names.rs` lists the names the mod's code and the engine documents cite; another state shows as its hash), and the procedural body's hand-over case ([procedural-body.md](../body/procedural-body.md#the-hand-over-table)). An unchanged state is noted again, marked as a repeat, once five seconds have passed since the last note, so a lookback save or a recording started mid-session opens with the state it started in.
- `throw`, when an item leaves a hand with a requested motion: a gun let go (beside its `gun_release`), a grenade, a caught grenade, a charge, or a carried object thrown. It holds the hand, the item, the requested linear and angular velocity in world units (per game second), the game's time scale, so that the same motion per real second is the world's times the scale, where the hand's part of the estimate came from, and, for a grenade, whether it left armed (its pin out, or caught live).
- `grip_release`, when a hand lets go of what its grip held through the physical hands: what it held (a prop, an articulated object, a door, a ragdoll, a latched door, or a character), the object and body, why (the grip opened, the hold was lost, or the hold ended), and whether the release threw it. The press and the take are `grip_acquisition`'s.
- `wingsuit_gesture`, when the wingsuit gesture recognizer's phase or the reason it is unavailable changes, the same change the log's `wingsuit gestures:` line reports ([wingsuit-gestures.md](../input/wingsuit-gestures.md)).
- `snatch`, when the grapple snatch's phase changes: the hand, the phase it left and entered, how it ended (taken, failed, cancelled, or stood down) and why, the offered target's kind and distance on arming and firing, and the yank's distance and speed when it fired ([grapple-snatch.md](../input/grapple-snatch.md)).

The character's motion adds 22 bytes to each controller state before compression (a presence byte, the velocity, and the two heights behind their own presence byte). A body state note is about 50 bytes and repeats at most every five seconds when nothing changes; the other moments are rarer and of a similar size.

The payload's own enums are recorded as their labels and nested runtime values as their Rust `Debug` text, read against the recording's build stamp, so the file does not change when the payload's types do. A replay ignores events; they are evidence for tools.

The header holds the start marker (the player character's world matrix, and the name of a director bookmark within two metres, if any), the build stamp, the stamp of the recording's start, the semantic action schema's version, the dominant hand, the world scale, and the director's time scale.

The file goes to `recordings/<stamp>/tracking.jc3trk` in the session folder ([sessions.md](../sessions.md)). A writer thread appends input records and their frame as one queue batch, so queue pressure cannot accept only one part of that batch. A rejected batch increments the frame-drop counter and retains an explicit input-loss boundary for the next accepted batch or the final writer drain. Oversized batches, encoding errors, and I/O errors are failures rather than queue drops. Tools > Tracking capture shows frames, drops, accepted encoded bytes, duration, and the matching writer's completion error. Accepted stored bytes are not a durability guarantee. The complete logical-byte count includes only fully accepted original batches and the preamble. Full hand joints dominate the snapshot size; input records add source and observation data. A recording stores the same full samples in independent lossless version 5 chunks without changing sample rate or hand-joint capture; the reader still accepts uncompressed records. Real-capture performance has not been benchmarked.

### Lookback

While motion controllers are enabled and the OpenXR runtime is up, the recorder keeps the last ten seconds of capture batches (each frame with its input records and events, encoded once exactly as a recording writes them) whether or not a recording runs, bounded also to 64 MiB. Tools > Tracking capture shows how much it holds, and "Save the last 10 s" (or F2 while no recording runs) writes it to `recordings/<stamp>/lookback.jc3trk` in the session folder on a writer thread of its own. The start marker is the player's world matrix on the oldest kept frame. The times in a lookback file start where the window started, not at zero; a replay measures from its first frame. When earlier batches were evicted, the file opens with an input discontinuity counting the input records they took with them. The lookback stops and drops its window while a replay runs. Events noted while neither a recording nor the lookback collects are not kept, and a bounded queue of pending events drops (and counts) any beyond its size when no frame drains it.

Stopped writers retain their owners until termination is confirmed. Completed writers are reaped without waiting for disk on the game thread. Starting another recording is refused when the bounded writer-owner capacity is full. Shutdown reports unsafe unmapping while any retained writer still runs; a stopped writer's I/O error and its thread liveness are separate outcomes.

### File format

The format is a real wire format with a version, and its decoder is covered by round-trip tests. All numbers are little-endian. A file is the magic `JC3VRTRK`, a `u16` format version, a `u32` header length and the header, and then records until the end of the file, each a tag byte, a `u32` length, and the payload. Version 1 contains snapshot frames. Version 2 retains the same header and frame layout and adds independently versioned input-observation records. Version 3 stores each atomic batch in an independent checksummed chunk, using LZ4 when smaller or raw storage otherwise. Versions 1 through 3 retain their joint layout and decode aggregate validity into both position/orientation validity flags and aggregate tracking into both tracking flags. Their upper joint bits remain ignored. Version 4 stores all four component flags in the existing byte, with their conjunctions in the two low bits; its reader rejects nonzero reserved bits and inconsistent conjunctions. It accepts direct frame/input records or the existing independently checksummed chunks. Version 5 keeps the version 4 layouts and adds event records (tag 4), direct or inside a chunk, each an event's time, a kind byte, and the kind's fields; a version 4 file with an event record is an error. Version 6 keeps the version 5 layouts and appends the root's tick to the controller state event, behind a presence byte; a version 5 controller state reads without it. Version 7 keeps the version 6 layouts, appends the character's motion to the controller state event behind a presence byte (a version 6 controller state reads without it), adds the event kinds 5 through 9 (`body_state`, `throw`, `grip_release`, `wingsuit_gesture`, and `snatch`), which a version 5 or 6 container rejects as unknown kinds, and adds the recenter cause 2, the follow bank. New recorder and synthetic output uses version 7; synthetic recordings remain snapshot-only and do not establish input order. Explicit legacy frame encoders reduce each component pair to its conjunction, which is lossy. Compression runs in the writer thread. Decoding validates stored and expanded sizes, record counts, aggregate budgets, exact decompression length, and checksum before parsing inner records. A truncated final outer chunk preserves earlier complete chunks; a complete corrupt chunk is an error. Chunk budgets are resource limits, not a guarantee that the full decoded recording and conversion fit available memory. The replay loader bounds file reads before decoding. Legacy readers retain their unknown-tag behavior; versions 3 through 5 skip unknown optional tags and rejects unknown required tags, and a file whose last record was cut short (the game stopped mid-write) reads as the records before it. Any other malformed field is an error that names the field and its byte offset. The per-field layout is in `tracking-format/src/codec.rs`, and the events' in `tracking-format/src/event_codec.rs`. The payload records and replays in its own types and converts them to the crate's field by field, so a field added to a recorded type does not build until the format accounts for it.

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

Each rendered frame stages at most one due pose frame and 128 ordered-input records. The pacing decides what comes due: `real_time` follows the recording's own clock, and `frame_locked` advances one recorded pose frame per rendered frame. Due backlog remains pending in file order, including equal timestamps. A slow game can therefore replay more slowly than the recording. Staging does not discard press/release edges. Ordered input remains in complete Group/Policy dependency units and cannot run beyond the staged pose time while poses remain pending. A full publication queue pauses input staging until gameplay drains it. Completion waits for the final staged pose and ordered input to be consumed.

Loader admission rejects a dependency unit larger than 128 records or a pose frame with more than 64 recenter events, rather than splitting the unit or dropping events. Each recorded recenter remains attached to its pose frame. Strict replay continues to use recorded policy decisions without substituting current settings.

At most two loader threads are owned across the current replay and replaced loads. Admission joins completed loaders before checking this bound and refuses a saturated start before spawning. Stop does not cancel blocked filesystem work. Shutdown closes admission and retains every unfinished loader after its bounded wait; an unfinished loader keeps the payload mapped.

Replay admission checks located eye poses, FOV angles, valid controller/joint pose components, present velocities, stage measurements, and recenter rotations for finite, nondegenerate values before publication. Quaternion squared length must be within 0.01 of one. FOV angles must lie strictly between negative and positive pi/2, with left below right and down below up. Unused invalid-flag pose components remain accepted. Active hands also require finite trigger, squeeze, squeeze-force, and trackpad-force values in `0..=1`, and finite thumbstick/trackpad components in `-1..=1`. The stored hand has no per-action validity flags, so hand activity is the raw-input admission boundary; touch flags and tracking loss do not make an active hand's scalar inputs unused. Inactive raw payloads remain accepted. A consumed start transform must be finite, affine, and nonsingular. Marker mode checks the recorded matrix during loading. Bookmark mode checks the current resolved bookmark or fallback matrix before teleport. In-place mode does not validate an unused marker. Errors name the file, frame where applicable, and field. These are replay-admission rules; the lossless wire codec still preserves arbitrary IEEE values.

Cockpit-space eye positions and valid controller/joint positions are limited to 100 m per axis. These coordinates describe physical tracking around a recenter origin; the allowance exceeds a room-scale session and does not restrict world travel. The loader checks the actual head midpoint, slerp midpoint, eye deltas, and inter-eye distance using the publication path's arithmetic. Scaled cockpit vectors must retain finite squared length with headroom for rotations, differences, and native distances. The physical bound prevents finite near-`f32::MAX` positions from overflowing the midpoint or later composition. World start-marker translation does not use this cockpit bound. The current world scale is checked at preparation; unrelated live configuration or invalid native world transforms are not replay-file guarantees.

The replay replaces the runtime at the seams the runtime publishes through, so every consumer sees it:

- The head. With a session rendering, the frame loop replaces the located views' cockpit poses with the recorded ones and keeps the live poses for the compositor, so a headset shows the recorded view wherever it is, on a head or on a desk. With no session, or a session that is not rendering (a headset off the head), the render update publishes the recorded head through the same path a VR frame takes, less the per-eye render parameters, and the camera renders the desktop view from it. The headpose source is the VR source while a replay plays, with or without a session.
- The controllers and the semantic actions. The input update publishes the recorded frames instead of sampling, after it has synced the actions, and still sends the haptics to the live controllers. With no session, the render update publishes them. The recorded display times are the recording's own clock, which the hands' motion history spaces throws and strikes by.
- The STAGE floor height and play area, which the posture reads.
- The recenters. Live recenters, including the one on resuming gameplay, are held off, and each recorded recenter's effects (the roomscale re-base, the body turned under the view, and the body rig's snap) are applied on the frame it was recorded on. Live follow banks are held off too, and each recorded one (version 7) hands its turn back to the view's composition and turns the body rig and the roomscale state with the frame on its recorded frame, as the live bank did, so the view does not drift from the recorded poses, which carry the bank already. A recording older than version 7 carries no banks, and its replay leaves a frozen turn in the composition.

When the replay ends or is stopped, the published controller frames are cleared, so the last recorded hands do not hold what they held, and the live runtime takes over on the next frame.

## Scenarios

A scenario file is TOML in the scenario folder: `testing.scenario_dir`, or the `testing` folder in the configuration directory when that is empty. Tools > Replay lists the `.toml` files there. Point `testing.scenario_dir` at the repository's `testing/scenarios/` (under Proton, its `Z:` path) to use the templates in place.

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
signal = "procedural.neck_error_m"
check = "always"
below = 0.05
```

Each `[[expect]]` names exactly one probe:

- `signal`: a published signal (below). A flag reads true or false; a counter counts since the payload loaded, and with `delta` (the default for a counter) it reads how far it has grown since the replay started; a measure reads a number, or nothing while it is not measured.
- `status`: a status row ([status.md](../status.md)) by its label, in any case. It reads true while the row shows the `health` asked for (`ok`, `off`, `waiting`, or `failed`) and its detail contains `detail`. With neither given, the row must read `OK`.
- `log`: the number of log lines at info or above, since the replay started, that contain the text. `level` raises the floor to `warn` or `error`, and with a level the text may be empty, which counts every line at that level.

And one condition: `is` for a flag or a status row (true by default), or one of `below`, `above`, `at_least`, `at_most`, and `equals` for a number. A measure needs one; a counter and a log count default to `at_least = 1`.

The `check` decides over which frames the condition must hold: `eventually` on some frame (the default for flags and rows), `always` on every frame the value is present and at least one (the default for measures), `never` on no frame, and `at_end` on the last frame (the default for counters and logs). A frame is one rendered frame while the replay plays, and the frames of the settling time after it. The load and the teleport are not observed, and their log lines are dropped.

A scenario that cannot be read, or has an unknown signal, row, level, or health, a condition of the wrong kind, or more than one probe or condition, is refused with an error that names the expectation, before anything replays.

Request files are limited to 8 KiB and scenario files to 64 KiB before TOML parsing. The reader consumes at most the limit plus one sentinel byte, including when a file grows during the read. Scenarios admit at most 128 expectations. Each supplied string is limited to 4096 UTF-8 bytes, including request paths and scenario names, descriptions, recording paths, and expectation fields. Count and string checks run after the bounded parse and before replay admission. Errors identify the file and offending field/count. The game-thread reader bounds read attempts, but an individual operating-system file read can still block; these limits do not establish an I/O latency deadline.

### Results

When the replay and the settling time end, each expectation's verdict goes to the session log at info for PASS and warn for FAIL, as `scenario: <name>: PASS: <what decided it>: <expectation>`, followed by a summary line with the count. The summary file `scenarios/<start stamp>_<scenario file>.toml` in the session folder holds the same: the scenario, its file and recording, the build, whether it passed, the frame count, and each expectation's verdict and detail. A replay that fails, is stopped, or is replaced fails every expectation as not checked, with the reason.

The log probes read a tap on the log that queues every event at info or above while a scenario runs (`payload/src/testing/log_tap.rs`), whatever the log filter says, and leaves out the test tools' own lines. A burst beyond its queue loses lines. Every log expectation whose observed window contains loss fails with an explicit inconclusive detail, including absence and upper-bound checks. Signal and status verdicts remain independent. Load/teleport losses are outside the observed window. Scenario abort requests a stop for its own replay identity only, so a replacement replay or scenario request is not cancelled. Observation samples one identity-aware replay state under one lock. Pending, loading, and teleporting runs wait without reading signals or logs. Only the owned playing run and its uninterrupted completed settlement are observed. A later replay request invalidates settlement even if the replacement has already stopped and the old terminal result remains retained.

### Signals

Scenario signals read the snapshots the mod's systems publish for the status rows and the debug UI. Replay Live status has a separate, collapsed Signals table that lists every signal. Its bounded UI snapshot supports the replay-driving flag, recenter count, and the root gap, residual, push-back distance, obstruction flag, push-back count, and projected-ledger count. Other rows report an unsupported UI snapshot source. The table distinguishes not published, busy, current missing values, and stale retained values with their age. Root measurements expire against their source timestamp as well as the publication lifetime; cumulative counters use only the publication lifetime. Scenario expectations keep their own runtime reads. The signal names are:

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
| `stunt.hang_entries`, `.roof_entries`, `.releases`, `.refusals` | counter | The legacy hang-entry count, which stays zero; the stunt areas the mantles entered; the hangs let go by opening the grips; and the roof entries the game refused. |
| `push_up.pushes` | counter | Pushes up with the hands pressed down ([physical-hands.md](../input/physical-hands.md#pushing-up)). |
| `push_up.lifting` | flag | The hands lift the body. |
| `articulated.grips` | counter | Articulated objects a hand took hold of ([physical-hands.md](../input/physical-hands.md#articulated-objects)). |
| `grab.taken` | counter | Targets the grips took ([grip.md](../input/grip.md)). |
| `grab.left_holds`, `.right_holds`, `.left_fist`, `.right_fist` | flag | A grip holds what it took, or is closed on nothing; missing while the grip model does not resolve the hands. |
| `doors.grips`, `.unlatches` | counter | Grips taken on latched vehicle doors, and the latches the hands undid ([vehicle-doors.md](../input/vehicle-doors.md)). |
| `doors.on_latch` | flag | A hand grips a latched vehicle door. |
| `characters.grips`, `.yanks`, `.knockdowns`, `.regrips`, `.hijacks` | counter | Grips taken on characters, the yanks, the yanks and shoves that knocked a character down, the fallen characters' ragdolls taken, and the seated characters pulled out through an open door ([character-grabs.md](../input/character-grabs.md)). |
| `procedural.rig_error`, `.on_foot` | flag | The procedural torso's rig failed; its case is on foot. |
| `procedural.seated` | flag | The torso is posed on the seat's hips in a vehicle ([body/procedural-states.md](../body/procedural-states.md)). |
| `procedural.wing_arms` | flag | Rico's arms follow the player's in the wingsuit, with both hands queued. |
| `procedural.parachute` | flag | The torso is posed on the harness's hips under the parachute. |
| `procedural.swimming` | flag | The torso is posed on the swim's hips in the water. |
| `procedural.roof` | flag | The torso is posed on the stunt idle's hips standing on a vehicle's roof. |
| `procedural.neck_error_m` | measure | How far the solved neck is from the point under the player's head, while the procedural torso poses the torso. |
| `weapons.wielded`, `.in_left_hand`, `.in_right_hand`, `.two_handed` | flag | The wielded weapon and the hand its first gun rides. |
| `weapons.armed_in_left_hand`, `.armed_in_right_hand` | flag | A weapon the game does not wield is armed in that hand. |
| `weapons.armed_carried` | flag | A weapon armed in a hand is one the carry set keeps beyond the slots. |
| `weapons.draws`, `.docks`, `.drops`, `.promotions` | counter | Weapons drawn off a socket into a hand, docked from a hand onto a socket, and let go of from a hand, and armed weapons the reconcile made the primary's, wielded in the same hand. |
| `weapons.grabs`, `grip.flips`, `grip.pins` | counter | Weapon grabs, guns snapping on the wrist, and attach bones pinned. |
| `collide.contacts` | counter | Times a held long gun came against a wall. |
| `collide.touching` | flag | A held long gun is against a wall. |
| `regions.carries`, `.braces`, `.grips_taken`, `.let_go` | counter | Long guns taken by a part other than the grip, braced where the second hand closed on them, grips taken from a carrying hand or slid along to, and long guns left in the bracing hand when the holding hand let go, carried there or slid onto the grip from the handguard ([physical-guns.md](../input/physical-guns.md#grabbing-a-gun-anywhere)). |
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
| `rig.gap_m` | measure | How far the player's neck pivot is out over Rico's pelvis, while Rico is grounded and a VR frame renders ([rig.md](../body/rig.md#the-pelvis-solver)). |
| `rig.residual_m` | measure | The step demand: how far the neck is out past what a lean explains. |
| `rig.stepping` | flag | The body rig drives a committed step, or slides out what one left ([rig.md](../body/rig.md#steps)). |
| `rig.steps` | counter | Steps the body rig has committed. |
| `rig.ledger_projected` | counter | Completed steps whose step drive was credited by projection, because something stopped or deflected the root. |
| `rig.pushback_m` | measure | How far the push-back has slid the view back in its current episode ([rig.md](../body/rig.md#walls)). |
| `rig.obstructed` | flag | The ledger finds the player's step drive obstructed: something stops the body. |
| `rig.pushbacks` | counter | Push-back episodes: times the view started being held at a wall. |

A signal is added in `payload/src/testing/signals.rs`, reading a published snapshot only, as the status rows do.

## The templates

`testing/scenarios/` holds a starter set covering the headset checklists in [physical-hands.md](../input/physical-hands.md), [hand-climbing.md](../input/hand-climbing.md), [controllers-and-roomscale.md](../input/controllers-and-roomscale.md), [steering-wheel.md](../input/steering-wheel.md), and the procedural torso ([procedural-body.md](../body/procedural-body.md)). Each names a recording in `testing/recordings/` that does not exist yet, and says in its description what to do while recording it. To fill one: stand where the scenario should start (save a director bookmark there to have the marker name it), start a tracking recording, do what the description says, stop, copy the file from the session's `tracking` folder to the name the scenario expects, and run the scenario once to see that it passes against the build it was recorded on.

The `synthetic_*.toml` scenarios are the exception: their recordings are generated rather than recorded (below).

## Synthetic recordings

A synthetic recording is written from a script instead of recorded in a headset, so the body rig can be tested by a replay without anyone wearing one. `tools/tracking-synth` writes them:

```sh
cargo run -p tracking-synth --target x86_64-unknown-linux-gnu   # on Linux
cargo run -p tracking-synth                                     # on Windows
```

The `--target` is needed on Linux because the workspace builds for Windows by default. With no script named, the tool writes every built-in script to `testing/recordings/synthetic/<script>.jc3trk` in the repository it was built from. Naming scripts writes only those, `--out <dir>` writes elsewhere, `--rate <hz>` samples at another frame rate than 90 Hz, and `--list` prints the scripts with a line on each. The folder is git-ignored and the files are regenerated rather than committed: each runs to between 400 KB and a megabyte. Regenerate them after a change to the tool, the file format, or the semantic action schema.

`tracking-synth dump <file>` prints any recording, recorded or synthetic, to stdout: a header, then each frame and event in time order, one line each. `--json` prints JSON lines instead, a `header` object followed by one `frame` or `event` object per line, for scripts. Input records are counted in the header, not printed. `--from <time>` and `--to <time>` limit the frames and events printed to an inclusive stretch, either end open. A `<time>` is seconds on the recording's clock (`--from 754.2`), the clock of the frames' and events' times, or an OpenXR predicted display time in nanoseconds (`--from xr:123456789000`), the clock a video recording's sidecar stamps its frames with. A display-time `--from` starts at the first frame at or after it and a display-time `--to` ends at the last frame at or before it, with the events between them. There is no wall-clock form: the file holds no wall-clock anchor except inside `take_marker` events, whose times can be used as seconds. The text form prints the version 7 moments on one line each (a body state by name, or by hash when it has none, and a throw's speed per game second and per real second), a follow bank as the turn of the cockpit frame on the frame it took effect in, and the other events as their fields; the JSON form carries every field.

`tracking-synth rig-eval [--height <m>] [--eye-height <m>] [--tuning <json>] <file> <out.csv>` runs the grounded body rig's solver (`body-rig`) over a recording with a simulated pelvis that follows the step drive, and writes one CSV row per grounded frame: the time, the video's time when the take's sidecar (`frames.jsonl` and `stream.json`) sits beside the recording, the step phase and the steps committed, the step demand, the neck's offset over the pelvis and the lean's reach, the bend, the crouch, the neck's and the standing height, the head's and the body's yaw and their difference, the simulated pelvis, and the stick. It runs only while the recorded hand-over case is `on foot`, keeps its state through a spell off the ground shorter than a second, turns the room with every recenter and follow bank, and snaps the pelvis under the neck at a manual or seat recenter, as the rig's recenter does. `--tuning` takes the rig's tuning as JSON (`{"pelvis":{"lean_radius_m":0.25}}`), with any field left out at its default. `tools/plot_rig_eval.py <out.csv> <dir> [<from>-<to>...]` plots it against the video's time ([body/rig.md](../body/rig.md#offline-evaluation)).

Each built-in script is the recording of the scenario `testing/scenarios/synthetic_<script>.toml`, which names it as `../recordings/synthetic/<script>.jc3trk` and replays it with `start = "in_place"`. The scenarios check what the existing signals can tell (the replay plays, the recorded recenter is the only one, the controllers are fresh, the player stays on foot, the skeleton rigs, and the stick drives the body or nothing), and also check the rig ([rig.md, "Settings and diagnostics"](../body/rig.md#settings-and-diagnostics)): the step demand stays small unless the player walks in the room, nothing is credited by projection or pushed back on the open floor, the body steps during walks and stands at the end, and it does not step for a lean, a crouch, a look around, or standing still. The rig's thresholds are provisional until they are checked against a run. The scripts are:

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

Every frame is a focused session that asked for the frame to be rendered, with the head located and both controllers active on the Index profile, tracked, with velocities, in the on-foot mode set. The derived presses use the payload's default thresholds, the grip's read from the squeeze force, and a hand round the handle reads a squeeze of 0.9 and no force. The semantic actions are `global` and `on_foot` at rest, but for `move` from the left stick and `turn` from the right stick's horizontal axis, as a right-handed player's default bindings read them. The tool indexes them by the payload's snapshot of the action schema, `payload/src/vr/input/semantic/schema.golden`, which the payload's tests hold to the actions it creates. The STAGE space reports a 4 m by 4 m play area and the eyes' height above the floor. There are no hand joints. The first frame carries a full manual recenter that turns nothing, so the replay starts the posture and the body rig afresh from the scripted head.

The header names the build as `tracking-synth` and its version, the stamp as `synthetic <script>`, the schema version from the snapshot, the right hand dominant, and a world scale and time scale of one. The start marker is the identity at the world's origin with no bookmark, because a synthetic recording was made nowhere in the world: replayed from the marker, it would teleport the player to the origin.

The tool's tests check that every script's recording reads back as written and that the poses and inputs are the ones the scripts describe, and unit-test the tracks' easing, the rig, and the inputs. The payload's tests generate every script, decode it with the payload's decoder, check that the replay would publish its semantic actions, and check that the left stick drives this build's `move` action.

## Limits

The game is not deterministic, so a replay reproduces the player's input, not the game's response to it. Physics, animation timing, AI, traffic, weather, time of day, and streaming differ between runs, and the game's frame rate decides when the sim ticks fall against the recorded frames. A scenario should check what the input makes happen (a grip took, a weapon is in a hand, a mantle completed, no warning fired) rather than exact positions, and a recording should start somewhere quiet. Other limits:

- The state the mod keeps across frames is live, not recorded: the posture and its seated reference, the body rig's pelvis, yaw, standing height, and step, the body calibration, and the settings. A replay starts from whatever they are, so start it from the same posture and settings the recording was made with; the replay warns about the dominant hand and the world scale.
- The semantic actions are the ones the bindings saw while recording. If the game takes the replay into another mode than the recording was in, the recorded mode set's actions are published, and the bindings of the mode the game is in see none.
- The head is held at the last recorded located views between them, and is not interpolated, so a desktop rendering faster than the recording steps.
- With a session running, the compositor shows the replayed view at the live head pose, so wearing the headset during a replay is uncomfortable.
- The per-eye render parameters, and what reads them (the world-space overlay, the scope lens, the HUD panel), are absent without a session.
- The game's own clock and pause are not part of the replay: a pause menu or a time scale during a replay desynchronises it from the game.
- A replay holds live recenters off, F7 included.
- A synthetic recording is clean: it has no tracking loss, jitter, or dropped frames, and its body moves as the rig models it rather than as a person does.
