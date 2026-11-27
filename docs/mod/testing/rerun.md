# Viewing a take in Rerun

`tracking-synth export-rerun` writes a tracking recording ([recorder.md](recorder.md)), or a stretch of it, as a [Rerun](https://rerun.io) recording (`.rrd`), so a session can be scrubbed with the take's video beside the 3D scene, the plots, and the event log. `tracking-synth scan-view` scans the take's video for flashes and checks each against the recording. The code is `tools/tracking-synth/src/rerun_export/` and `view_scan.rs`.

The capture host runs both when a take ends: `scripts/rerun_take.sh <take folder>` writes `take.rrd` and `view-scan.md` into the take's folder and logs to `rerun.log`, and can be run by hand on any take ([capture.md](../capture.md#the-relay)). `JC3VR_CAPTURE_SCAN=0` skips the scan.

## Exporting

```sh
cargo run --release -p tracking-synth --target x86_64-unknown-linux-gnu -- \
    export-rerun --from 1860 --to 1980 --video <take>/video.mkv <take>/tracking.jc3trk -o take.rrd
rerun take.rrd
```

`--from` and `--to` take the same times as `dump`; an hour-long take writes about half a gigabyte without its video, so a stretch of a few minutes is the usual way. `--video` places the take's video on the recording's clock through the take's `frames.jsonl` and `stream.json`. Rerun reads video only from MP4, so the tool cuts the stretch with `ffmpeg` and encodes it as H.264 without B-frames and with a keyframe every second, so the cut starts at the exact frame and scrubs back cheaply; a long stretch is cut into ten-minute segments, because the viewer shows no video for an hour-long asset. `--recording-id` and `--no-blueprint` let `rerun rrd merge` join several stretches, or another tool's overlay, on one timeline. The rig options are `rig-eval`'s ([rig.md](../body/rig.md#offline-evaluation)).

The SDK is pinned to the Rerun release nixpkgs ships (`tools/tracking-synth/Cargo.toml`), and the viewer must be that version or newer (`nix shell nixpkgs#rerun`). The export is behind the tool's default `rerun` feature.

## What it shows

Everything is on the `recording` timeline in seconds, with a `wall_clock` timeline when the take's sidecar or a take marker anchors one. The default layout (`rerun_export/blueprint.rs`) puts the 3D scene and plots on the left and the video, event log, and state lanes on the right.

`world` is the game's world, Y up, less an origin near the stretch's start so the viewer works near zero. It holds Rico's root and trail, his bones before the HumanIK solves and as rendered, the cockpit placed in the world with the head, the hands' grip and aim poses, and the offline rig beside the rig the game ran, the camera and its two tick poses, the grapple's wire, grip targets, throws, held objects, the mod's forces, the dynamic bodies near Rico, and the vehicle. A version 8 recording holds where the cockpit sits in the world each frame (the controller state's `view`), and the export places it from that; an older recording is placed by an estimate fitted from the grips and the drawn wrists (`rerun_export/registration.rs`), and `states/cockpit_placement` names which placed each frame.

`plots/` holds the scalars by subject, `events/` the events as leveled log entries by subject (the mod's refusals as warnings), and `states/` the lanes: the action set, the session, the controller mode, the posture, the rigs' step phases, the grapple, the vehicle, and more. `states/move` is the coarse lane, with colours fixed by `MOVE_STATES` in `rerun_export/character.rs` so a mode reads the same in every take.

A recording from before version 8 lacks the view, camera, poses, bodies, and recorded rig, so its export estimates the cockpit and draws no skeleton. Even version 8 leaves out a gun sight's ray origin, a body's shape as it turns, and the HumanIK targets of the solve's other passes.

## Scanning for flashes

A flash is a frame, or two, unlike the frames on both sides while those are alike. In a headset take it is nearly always the camera dipping into Rico's body, which a player sees as flicker; a cut or a fast turn is not one, because the frames after it stay changed. `scan-view` decodes the video to small thumbnails, scores each frame's colour difference against its neighbours, and counts a frame over the threshold as a flash, merging flashes close together.

For each flash it reads the camera's tick pose and Rico's pre-IK head in the world around the moment, and reports the camera's least lead over the head along the head's motion (negative is behind the head, which at speed is inside Rico's back), its greatest distance from the head, the head's speed, and the controller mode. `view-scan.md` gives the counts by mode and every flash with its recording time (Rerun's timeline, to scrub to) and its video time (for `ffmpeg -ss`). The scan does not say what filled the view: a menu, a loading screen, or the debug panel opening flashes too, so look at the frames before concluding.
