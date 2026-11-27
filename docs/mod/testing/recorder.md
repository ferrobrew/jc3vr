# Tracking recorder

The tracking recorder writes what the OpenXR runtime gives the mod each frame to a file, with the mod's own decisions beside it as events, so that a headset session can be studied afterwards without the game: printed, evaluated against the body rig offline, or scrubbed in Rerun beside the take's video. Recordings are evidence, not something to play back into the game; the game is not deterministic, so the same input would not reproduce what followed.

The code is `crates/jc3vr-view/src/recorder/` (the recorder, and the conversion between the payload's types and the file's), `crates/tracking-format/` (the file format's data model and codec, a crate of its own so that host tools build it on Linux and Windows), `crates/jc3vr-tooling/src/diagnostics/` (the samplers of the pose, the bodies, and the world state), `crates/jc3vr-body/src/controllers/telemetry/` (the notes the controllers write as they act), `tools/tracking-synth/` (the offline tools and the synthetic recordings), and the debug UI's Tracking capture feature in `crates/jc3vr-ui/src/ui/testing.rs`.

## Starting and stopping

Nothing records until it is started, and `testing.enabled` (on by default) switches every way of starting a tracking recording off. There are four ways:

- F1 starts a session recording, the video and a tracking recording together ([capture.md](../capture.md#session-recording)).
- The director's key bindings ([trailer-tooling.md](../trailer-tooling.md)): Numpad/ starts or stops a session recording.
- The debug UI's Tools > Tracking capture, on the desktop or in the headset, which also saves the lookback (below).
- The request file, for a script. Every second (every frame in a capture harness session, whose environment carries `JC3VR_HARNESS_TOKEN`) the game looks for `test_request.toml` in the configuration directory ([settings.md](../settings.md)), reads at most 8 KiB of it, deletes it, and acts on it. `testing.request_file` turns the watch off. Under Proton the configuration directory is inside the game's prefix.

```toml
command = "record"  # record or stop_recording
```

The capture harness's commands share the file ([fast-comparison.md](fast-comparison.md), [benchmark.md](benchmark.md)).

## What a recording holds

A recording needs a player character to mark its start at. It starts at the top of the frame after the request, and samples once per VR frame at the end of the frame loop's begin, after the views are located and the controllers sampled; without a running session it writes no frames until one starts. Each frame records what the mod published, not the runtime's raw answers: both eyes' poses in the cockpit frame and their fields of view, both controllers' published frames (poses with their validity, tracking flags, and velocities, every raw input, the derived presses, the profiles, the focus, the mode set, and, with `testing.record_hand_joints` on, the tracked hand joints), the semantic actions' per-hand states, the session state, whether the view pose was valid, the STAGE floor height and play area, and the recenters that took effect before the frame, among them the follow bank ([`vr/recenter.rs`](../../../crates/jc3vr-view/src/vr/recenter.rs)).

The header holds the start marker (the player character's world matrix, and the name of a director bookmark within two metres), the build stamp, the start stamp, the semantic action schema's version, the dominant hand, the world scale, and the director's time scale.

Beside the frames, the recording carries events, each stamped on the frames' clock and written before the frame captured after it. The payload's enums are recorded as their labels and nested runtime values as their `Debug` text, read against the build stamp, so the file does not change when the payload's types do. The kinds (`tracking_format::event::EventKind`, with every field in `crates/tracking-format/src/event.rs`) are:

| Kind | When | What it is for |
|---|---|---|
| `controller_state` | once per game frame, after the render update | The mod's state for the frame: scheme, mode, each hand's IK, tiers, and late correction, the aim rays, the grapple aim, the body rig's posture and root, the character's measured motion and height over the ground (what the animation graph's speed and height conditions read), and the `view`, `grapple`, `vehicle`, and `frame` groups: where the cockpit sits in the world and the camera's tick poses and render fraction, the wire, the vehicle and the hands' controls, and the render frame index with the mirror's present counter, which the video's frame stamp carries. Its fields are separately read snapshots, not one instant. |
| `grip_acquisition` | at each stage of a grip | The candidates the resolver offered and why each lost, the reach scan, and what the request took ([physical-hands.md](../input/physical-hands.md#interaction-evidence)). |
| `grip_release` | when a physical-hands hold ends | What was held, why it ended, and whether it was thrown. |
| `gun_release`, `throw` | when an item leaves a hand | The release route and its refusals, and the requested linear and angular velocity in world units per game second with the time scale. |
| `take_marker` | at each take marker | The same fields as the video sidecar's copy ([capture.md](../capture.md#session-recording)). |
| `body_state` | when the body's animation state changes, and every five seconds unchanged | The rule system's state (named from `crates/jc3vr-tooling/src/diagnostics/state_names.rs`, otherwise its hash) and the procedural body's hand-over case ([procedural-body.md](../body/procedural-body.md#the-hand-over-table)). |
| `wingsuit_gesture`, `snatch` | when the recognizer's or the snatch's phase changes | The same changes the log reports ([wingsuit-gestures.md](../input/wingsuit-gestures.md), [grapple-snatch.md](../input/grapple-snatch.md)). |
| `pose` | twice per pose-finalization pass | Rico's body bones in model space with the character's transform: `pre_ik`, the animated pose before the HumanIK solves, and `final`, after them and the mod's joint writes. |
| `rig`, `rig_config` | each grounded-rig frame; the configuration when it changes and every five seconds | The rig's inputs exactly as the payload built them and its outputs, so the rig can be replayed offline. |
| `bodies`, `body_intro` | after each simulation step; a body's identity when it joins and every five seconds | The dynamic bodies near Rico's root or hands, held ones first, written again only when they move. The step finds new bodies by walking a slice of the world's body slots, and records its own cost. |
| `held` | after each step, per hand holding something | The held body's pose and the hold's report. |
| `force` | each velocity set or impulse through the physical hands' writers | The source file and line that asked, and the body's velocities before and after, so the mod's share of a body's motion can be told from the world's. |
| `injected` | each input tick the bindings write actions | Each write's action, value, phase, and outcome. |
| `diagnostic` | a refusal or another moment a playtest needs explained | Written by `controllers::telemetry::report`, which logs the same text under `controllers`, so the log and the recording agree. |
| `haptic` | each one-off cue the mixer takes | The hand, source, primitive, amplitude, length, and frequency. |
| `settings` | at the start, at the head of a lookback save, and on each saved change | Every setting as TOML, so a take says what it ran with. |
| `frame_timing` | each second | The frame-time breakdown ([frame-timing.md](../performance/frame-timing.md)); the recorder's own sampling is the `recorder` slot. |

An ordinary take writes about 12 MB a minute before compression, most of it the poses, the controller state, and the hand joints; the reads and the encoding cost tens of microseconds a frame on the game thread.

The file goes to `recordings/<stamp>/tracking.jc3trk` in the session folder ([sessions.md](../sessions.md)). A writer thread appends each frame with its input records and events as one queue batch, so queue pressure drops whole batches: a rejected batch counts as a dropped frame and leaves an input-loss boundary on the next accepted batch. Oversized batches and encoding or I/O errors are failures, not drops. Tools > Tracking capture shows the frames, the drops, the bytes accepted, the duration, and the writer's error. Stopped writers are kept until their threads end, without waiting for the disk on the game thread; a new recording is refused while the bounded set of writers is full, and shutdown reports an unmap while one still runs.

### Lookback

While motion controllers are enabled and the OpenXR runtime is up, the recorder keeps the last capture batches, encoded once exactly as a recording writes them, whether or not a recording runs: the tuning's `recorder.lookback_window_s` (`crates/jc3vr-view/tuning/view.toml`), bounded also by bytes. "Save the last N s" in Tools > Tracking capture, or F2 while no recording runs, writes it to `recordings/<stamp>/lookback.jc3trk` on a writer thread of its own. The start marker is the player's world matrix on the oldest kept frame, and the times start where the window started, not at zero. When earlier batches were evicted, the file opens with an input discontinuity counting the input records they took. Events noted while neither a recording nor the lookback collects are not kept.

### Input observations

The sampler captures the protected raw controls and the active semantic actions in one unordered sync group. Runtime change times are kept as metadata on their own clock; neither the sampling order nor different change times establish the order in which buttons were pressed. Each record keeps the logical time, the clock and source generations, the exact value bits, the focus and context, the source query's outcome (failed, absent, empty, and limited stay distinct), the recording-relative capture time, and the controller publication's identity when there is one. Bounded history loss and invalid sampling produce explicit discontinuities rather than merged taps. A cached binding does not identify the individual contributors to a merged semantic value.

### File format

The format is a versioned wire format, version 8 (`tracking_format::VERSION`). All numbers are little-endian. A file is the magic `JC3VRTRK`, a `u16` format version, a `u32` header length and the header, and then records to the end of the file, each a tag byte, a `u32` length, and the payload. Since version 3 each batch is an independent checksummed chunk, stored with LZ4 when that is smaller; compression runs on the writer thread. Each version keeps the previous one's layouts and adds fields behind a presence byte or adds event kinds, so the reader accepts every version back to 1, and a container rejects event kinds newer than its version. Dense samples are quantised: a rotation as its smallest three components in six bytes, and a position or velocity as three fixed-point `i16`s; a pose names its bone list by a set number, so the bones it holds can grow without a version.

Decoding validates the stored and expanded sizes, the record counts, the budgets, the exact decompressed length, and the checksum before parsing the inner records. A truncated final chunk keeps the complete chunks before it, so a file cut off by the game stopping mid-write reads; a complete corrupt chunk, or any other malformed field, is an error naming the field and its byte offset. The budgets are resource limits, not a promise that a whole recording fits in memory. The per-field layouts are in `crates/tracking-format/src/codec.rs` and `event_codec.rs` with its submodules, and the per-version differences are there too. The payload converts its own types to the crate's field by field, so a field added to a recorded type does not build until the format accounts for it.

## Offline tools

`tools/tracking-synth` reads any recording, recorded or synthetic. Run it on the host:

```sh
cargo run -p tracking-synth --target x86_64-unknown-linux-gnu -- <command> ...   # on Linux
cargo run -p tracking-synth -- <command> ...                                     # on Windows
```

The `--target` is needed on Linux because the workspace builds for Windows by default. `--help` lists the commands and their options. `scripts/rerun_take.sh` runs `export-rerun` and `scan-view` on a take's folder after each take.

- `dump <file>` prints a recording: a header, then each frame and event in time order, one line each, or with `--json` one JSON object per line with every field. `--from <time>` and `--to <time>` limit it to an inclusive stretch. A time is seconds on the recording's clock (`--from 754.2`) or an OpenXR predicted display time in nanoseconds (`--from xr:123456789000`), the clock a video sidecar stamps its frames with. The file holds no wall-clock anchor except inside `take_marker` events.
- `rig-eval <file> <out.csv>` runs the grounded body rig's solver (`body-rig`) over a recording with a simulated pelvis, writes one CSV row per grounded frame (with the video's time when the take's sidecar sits beside the recording), and logs a summary of the steps, the turns, and the held leans. It runs while the recorded hand-over case is on foot, follows every recenter and follow bank, and takes the rig's tuning from `crates/jc3vr-body/tuning/rig.toml` with `--tuning <json>` overriding fields. `--pelvis recorded` or `--pelvis pre-ik` feeds it the payload's recorded rig inputs or the game's animated hips and diffs each row against the rig the game ran, and `--weights` with `--ik` replays the IK body's contacts ([rig.md](../body/rig.md#offline-evaluation), [ik-body.md](../body/ik-body.md#the-offline-replay)). `tools/analysis/plot_rig_eval.py` plots the CSV.
- `export-rerun <file> -o <out.rrd>` writes a recording, or a stretch of it, as a Rerun recording with the take's video ([rerun.md](rerun.md)).
- `estimate-body --weights <safetensors> <file> -o <out.csv>` runs the learned body estimator over a stretch from the recorded head and grip poses and writes its estimate at 60 Hz, starting `--lead` seconds early so the estimator's window is full ([body-estimator.md](../body/body-estimator.md)).
- `scan-view <file>` scans a take's video for flashes and checks each against the camera's place relative to Rico's head ([view-scan.md](view-scan.md)).

## Synthetic recordings

A synthetic recording is written from a script instead of recorded in a headset, so the body rig and the offline tools can be exercised without anyone wearing one. With no command, `tracking-synth` writes every built-in script to `target/tracking-synth/<script>.jc3trk`; naming scripts writes only those, `--out <dir>` writes elsewhere, `--rate <hz>` samples at another rate than 90 Hz, and `--list` prints the scripts (standing, walking, leaning, crouching, turning, the sticks, and looking around). The files are regenerated rather than committed; regenerate them after a change to the tool, the file format, or the semantic action schema. The tool's tests generate them in memory as fixtures for `rig-eval` and `export-rerun`.

A script (`tools/tracking-synth/src/scripts.rs`) describes the player as a body rather than as three tracked devices: tracks for where it stands, its heading, the hips' drop, the torso's lean, the head's yaw and pitch, and the sway, and per hand an offset from where the arm hangs and the controls. Tracks are eased between keys with the quintic smootherstep, so every key is a moment of rest. The rig (`tools/tracking-synth/src/rig.rs`) poses a 1.8 m player of standard proportions from the tracks and reads the devices off the skeleton, with gait bob, sway, and a few millimetres of hand tremor; evaluate the recordings with `rig-eval`'s default 1.8 m body so the measurements match. The aim pose approximates the Index's offset from the grip rather than measuring it.

Every frame is a focused, rendered session with the head located and both controllers tracked on the Index profile in the on-foot mode set, with the payload's default press thresholds. The semantic actions are at rest but for `move` and `turn` from the sticks, indexed by the payload's snapshot of the action schema, `crates/jc3vr-view/src/vr/input/semantic/schema.golden`. There are no hand joints and no input observation records, and the first frame carries a full manual recenter that turns nothing, so a reader starts afresh from the scripted head. The header names the build as `tracking-synth`, and the start marker is the identity with no bookmark. A synthetic recording is clean: no tracking loss, jitter, or dropped frames, and its body moves as the rig models it rather than as a person does.
