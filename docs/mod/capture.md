# Session capture

A session recording's video is made from the eye views inside the game: the payload packs both eyes into one frame on the GPU, reads it back without stalling, and streams it to a capture host, which encodes it with the default output's audio into the take's folder. The host is the injector, which stays running after it injects, or on Linux the relay, which uses the host's own ffmpeg. With the in-process capture off, the relay records the desktop mirror window from the screen instead. Either way the payload writes a per-frame pose sidecar that lines up with the video.

The payload side is `crates/jc3vr-tooling/src/recorder/`, the injector's host is `injector/src/capture/`, the relay is `scripts/capture_host.sh` running `tools/capture/relay.py`, and the post-processing tool is `tools/capture/reframe.py`. Their module documentation covers the internals.

## Session recording

F1 (or Numpad / through the director's keymap, or the Video feature's button) starts a session recording: a video take and a tracking recording ([testing/recorder.md](testing/recorder.md)) under one stamp, in one `recordings/<stamp>/` folder ([sessions.md](sessions.md)). F1 again stops both. There is no video-only recording. When the capture host does not answer within a moment, or refuses the take, the session carries on as tracking only and the Video feature says why. Neither half has a length limit or grows in memory with the take.

The two halves line up by the OpenXR predicted display time, which the sidecar's `predicted_display_time_ns` and the tracking frames' `display_time_ns` share; `tracking-synth dump --from xr:<ns>` prints the tracking around a sidecar frame.

Every route to a take marker (F2, the headset chord, the director's Numpad 3 and Mark buttons) drops it into whatever records, and with no tracking recording running also saves the tracking lookback to `recordings/<stamp>/lookback.jc3trk`, so a moment is kept even when nothing records. The headset chord is described in [input/bindings.md](input/bindings.md#chords). A marker goes into the take's sidecar while a video take runs and into the session's `markers.jsonl` otherwise, and into the tracking timeline as a `take_marker` event while tracking collects, so the copies join by `index` and `stamp`.

## What the video shows

A streamed take (the default) shows both eyes as the headset receives them, side by side at the Video feature's eye width, or with the right eye at half size for a machine that cannot move the full pair. The eyes are captured after the HUD, the panels, and the in-world overlays are drawn into them, so the clean view ([hud.md](hud.md)) at the everything scope takes those out of the footage; the desktop scope affects only the mirror window.

A mirror take records the mirror window ([vr-runtime.md](vr-runtime.md#the-desktop-mirror)), so F10's view decides what is recorded, and the window must stay visible and unmoved for the whole take, since the recorder captures a fixed region.

## The in-process capture

On the render thread, once both eyes' captures hold the frame and a stream frame is due, one fullscreen pass (`recorder/convert/`) packs both eyes into planar 4:2:0 YUV in a single texture. It is copied into a ring of staging textures, and the one written two captures earlier is mapped without waiting; if its copy has not completed, the previous image repeats rather than stalling.

The stream holds a constant rate by the wall clock (`recorder/pacing.rs`): a frame that owes no stream frame is not captured, and a frame the writer's queue cannot take passes its debt to the next, so the video stays as long as the take and in step with the host's audio. The readback is the cost to watch; growing dropped counters in the Video feature mean the host or the socket cannot keep up, and a narrower eye width is the lever.

## The control protocol and the take

The payload connects to the capture host over TCP on loopback and speaks one line of UTF-8 JSON per message (`recorder/protocol/`; the hosts' halves are `capture/protocol.rs` and `relay.py`). Either side ignores a message type it does not know, and a host refuses a version it does not serve, so a change an old peer cannot read takes a new version number. Version 2 is a streamed take: after the host answers the start line with `started`, everything the payload sends is the `yuv4mpegpipe` stream, and closing the writing half ends it. Version 1 is a mirror take. The injector serves only version 2; the relay serves both.

A take's thread (`recorder/take/`) retries the connection only until the first one succeeds: a host that refuses or drops the take ends the video for that take while the sidecar keeps being written.

## Files and alignment

A take writes into its `recordings/<stamp>/` folder:

- `frames.jsonl`, the sidecar: a line per rendered frame with both eyes' located pose in LOCAL space, their fields of view, and the clocks, plus marker lines and, for a mirror take, layout and present lines.
- `frame_map.jsonl` for a streamed take: the sidecar frame each stream frame shows.
- `video.mkv` (Matroska, so a take cut short by a crash plays up to the cut) and `stream.json`, the host's metadata with the video's first-frame wall clock.

In a streamed take, video frame `n` is `frame_map.jsonl`'s `stream_frame` `n`, at `n / frame_rate` seconds. In a mirror take the sidecar lines up by wall clock against `video_start_unix_ms`, a frame or two early. The take markers give a second reference either way.

During a mirror take the mirror stamps its present counter into the window's corner (`vr/mirror/stamp.rs` in the view crate), matching the present lines, so a held stamp tells a repeated present from the mod repeating an eye image.

## The injector's capture host

After it injects, the injector listens on the capture port and serves streamed takes one at a time until the game exits (`--no-capture-host` exits instead, `--capture-port` moves it). When the port is taken, the relay is serving and the injector leaves the takes to it.

For each take it runs ffmpeg (`JC3VR_FFMPEG`, else `ffmpeg.exe` beside the injector, else `PATH`) with the first H.264 encoder that encodes a short probe at the stream's size (`capture/encoder.rs`), since ffmpeg lists what it was built with, not what the machine can run. `scripts/fetch_ffmpeg.sh` (or `.ps1`) stages a pinned LGPL build beside the injector. Under Wine ffmpeg reaches no GPU encoder, so on Linux the relay is the better host.

The audio is a WASAPI loopback recording of the default output, muxed as stereo AAC. Loopback records everything the output plays and delivers nothing during silence, so each packet's capture time places it on the take's timeline, gaps becoming silence (`capture/audio/timeline.rs`). `JC3VR_CAPTURE_AUDIO=none` records none.

## The relay

`scripts/capture_host.sh` runs the relay on Linux; its header lists the environment overrides. Start it before injecting, so it holds the port. A streamed take is encoded with the host's ffmpeg and the default sink's monitor as audio. A mirror take finds the game window under niri and runs gpu-screen-recorder on its region, refusing a window it cannot capture whole. The relay strips the GPU-selection variables from the recorders' environment, since with them set gpu-screen-recorder finds no card.

After each take the relay runs `scripts/rerun_take.sh` on the folder in its own session, which writes `take.rrd` and the view scan ([testing/rerun.md](testing/rerun.md)); `JC3VR_CAPTURE_RERUN=0` turns it off.

## Post

`tools/capture/reframe.py` turns a single-eye mirror take into a flat video, warping each frame to a symmetric camera centred on the optical axis, optionally following a smoothed head orientation from the recorded poses.

## Operating it

On Windows, run `scripts/fetch_ffmpeg.ps1` once after building, then run the injector and leave its window open. On Linux under Proton, start `scripts/capture_host.sh`, then inject with `scripts/proton_run.sh`. Record with F1; the Video feature and the Status page show the host's state, the encoder, the audio, the counters, and any refusal.
