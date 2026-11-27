# Tracking recorder

The tracking recorder writes what the OpenXR runtime gives the mod each frame to a file, with the mod's own decisions beside it as events, so that a headset session can be studied afterwards without the game: printed, evaluated against the body rig offline, or scrubbed in Rerun beside the take's video. Recordings are evidence, not something to play back: the game is not deterministic, so the same input would not reproduce what followed.

The recorder is `crates/jc3vr-view/src/recorder/`, the format `crates/tracking-format/` (its own crate so host tools build it), the world samplers `crates/jc3vr-tooling/src/diagnostics/`, the controllers' notes `crates/jc3vr-body/src/controllers/telemetry/`, and the offline tools `tools/tracking-synth/`.

## Starting and stopping

`testing.enabled` (on by default) gates every way of starting a recording: F1's session recording with the video ([capture.md](../capture.md#session-recording)), the director's Numpad / ([trailer-tooling.md](../trailer-tooling.md)), Tools > Tracking capture, and the request file. For the last, the game looks for `test_request.toml` in the configuration directory ([settings.md](../settings.md)) every second (every frame in a capture harness session), reads and deletes it, and acts on `command = "record"` or `"stop_recording"`; the capture harness's commands share the file ([fast-comparison.md](fast-comparison.md), [benchmark.md](benchmark.md)), and `testing.request_file` turns the watch off.

## What a recording holds

A recording needs a player character to mark its start at, and samples once per VR frame after the views are located and the controllers sampled. Each frame records what the mod published, not the runtime's raw answers: the eyes' poses and fields of view, both controllers' published frames (with the hand joints under `testing.record_hand_joints`), the semantic actions, the session state, the STAGE floor, and the recenters that took effect. The header holds the start marker, the build stamp, and the action schema's version.

Beside the frames the recording carries events (`tracking_format::event::EventKind`). The payload's enums are recorded as labels and nested values as `Debug` text, so the file does not change when the payload's types do. The ones a reader reaches for most:

- `controller_state`, once per game frame: the mode, the hands' IK, the aim rays, the rig's posture and root, where the cockpit sits in the world, the camera's tick poses, the grapple, and the vehicle. Its fields are separately read snapshots, not one instant.
- `pose`, twice per pose-finalization pass: Rico's bones before the HumanIK solves and after them and the mod's joint writes.
- `rig` and `rig_config`: the grounded rig's inputs exactly as the payload built them and its outputs, so it can be replayed offline.
- `grip_acquisition`, `grip_release`, `gun_release`, and `throw`: why a grip took what it took ([physical-hands.md](../input/physical-hands.md#interaction-evidence)) and how each item left a hand.
- `bodies`, `held`, and `force`: the dynamic bodies near Rico, each hand's held body, and each velocity write or impulse the mod made with the source line that asked, so the mod's share of a body's motion can be told from the world's.
- `diagnostic`: a refusal or another moment a playtest needs explained, written by `controllers::telemetry::report`, which logs the same text, so the log and the recording agree.
- `settings`, every setting as TOML at the start and on each change, and `frame_timing`, the [frame breakdown](../performance/performance.md#the-frame-breakdown) each second.

The file goes to `recordings/<stamp>/tracking.jc3trk` in the session folder ([sessions.md](../sessions.md)). A writer thread appends each frame with its events as one queue batch, so queue pressure drops whole batches, each counted as a dropped frame and marked by an input-loss boundary on the next batch; encoding and I/O errors are failures, not drops.

### Lookback

While motion controllers and the runtime are up, the recorder keeps the last batches, encoded once as a recording writes them, over the view tuning's `recorder.lookback_window_s` and a byte bound. "Save the last N s" in Tracking capture, or F2 while no recording runs, writes it to `recordings/<stamp>/lookback.jc3trk`. Its times start where the window started, and when earlier batches were evicted the file opens with an input discontinuity.

### Input observations

The sampler captures the raw controls and the semantic actions in one unordered sync group, so neither the sampling order nor the runtime's change times say in which order buttons were pressed. History loss produces an explicit discontinuity rather than merged taps.

### File format

The format is a versioned wire format (`tracking_format::VERSION`): the magic `JC3VRTRK`, a `u16` version, the header, and then tagged, length-prefixed records, each batch an independent checksummed chunk stored with LZ4 when smaller. Each version keeps the previous one's layouts and adds fields behind a presence byte or new event kinds, so the reader accepts every version back to 1. A truncated final chunk keeps the complete ones before it, so a file cut off by a crash reads; any other malformed field is an error naming the field and its offset. The layouts are in `crates/tracking-format/src/codec.rs` and `event_codec.rs`. The payload converts its types to the crate's field by field, so a field added to a recorded type does not build until the format accounts for it.

## Offline tools

`tools/tracking-synth` reads any recording. On Linux it needs `--target x86_64-unknown-linux-gnu`, because the workspace builds for Windows by default; `--help` lists the commands.

- `dump` prints a recording, one line per frame and event or JSON with `--json`, over an inclusive `--from`/`--to` stretch given in recording seconds or as an OpenXR display time (`xr:<ns>`), the clock a video sidecar stamps its frames with.
- `rig-eval` runs the grounded rig's solver (`body-rig`) over a recording and writes a CSV row per grounded frame, optionally fed the recorded rig inputs to diff against the rig the game ran ([rig.md](../body/rig.md#offline-evaluation), [ik-body.md](../body/ik-body.md#the-offline-replay)); `tools/analysis/plot_rig_eval.py` plots it.
- `export-rerun` and `scan-view` write a Rerun recording and scan the video for flashes ([rerun.md](rerun.md)).
- `estimate-body` runs the learned body estimator over a stretch ([body-estimator.md](../body/body-estimator.md)).

## Synthetic recordings

With no command, `tracking-synth` writes its built-in scripts to `target/tracking-synth/<script>.jc3trk` (`--list` names them), so the rig and the tools can be exercised without a headset; the files are regenerated, not committed. A script (`src/scripts.rs`) describes the player as a body with eased tracks, and the rig (`src/rig.rs`) poses a 1.8 m player and reads the devices off the skeleton, so evaluate with `rig-eval`'s default 1.8 m body. A synthetic recording is clean: no tracking loss, jitter, dropped frames, or hand joints.
