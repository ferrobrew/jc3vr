# Session capture

A recording's video is a screen recording of the game's desktop mirror window, made on the host by gpu-screen-recorder, with the desktop's audio. The payload does not touch the pixels: it tells the capture host when a take starts and stops and where its files go, and writes a per-frame pose sidecar that lines up with the video by wall clock. The payload side is `payload/src/recorder/`, the host side is `scripts/capture_host.sh` and the relay it runs, `tools/capture/relay.py`, and the post-processing tool is `tools/capture/reframe.py`.

## What the video shows

The video is the mirror window at the window's size, recorded from the screen ([vr-runtime.md](vr-runtime.md#the-desktop-mirror)). The mirror view therefore decides what is recorded: a single eye filled into the window, or the stereo pair, toggled with F10. The mirror must be on (`vr.mirror`) and the window must be visible on screen for the whole take, on the active workspace and wholly inside its output, since the recorder captures a fixed region and records whatever is drawn there; a window moved, resized, or covered during a take spoils the footage. The clean view ([hud.md](hud.md)) applies to the mirror like everything else: at the desktop scope it keeps the flat debug overlay off the window, so it stays out of the footage, and at the everything scope it also takes the HUD and the in-world overlays out of the eye images the mirror shows.

## The control protocol

The payload connects to the capture host over TCP (port 47172 by default) and speaks a line protocol, version 1 (`payload/src/recorder/protocol.rs`, and the relay's module documentation). Each message is one line of UTF-8 JSON, at most 64 KiB:

- The payload opens with `{"protocol": "jc3vr-capture", "version": 1, "type": "start", "take": <stamp>, "folder": <path or null>, "window": {"title": <string or null>, "client_size": [w, h] or null}}`. `folder` is the take's folder as the host names it ([sessions.md](sessions.md#the-sessions-root)), null on native Windows; `window` is the game window's title and client size as the payload reads them.
- The host answers `{"type": "started", "file": <path>, "video_start_unix_ms": <number or null>, "region": {"x", "y", "width", "height"}, "output": <name or null>}` once it records, or `{"type": "error", "message": <string>}` and closes. It may also send an error and close during the take, when the screen recorder exits on its own.
- The payload ends the take with `{"type": "stop"}`, or by closing the connection. After a stop line the host answers `{"type": "stopped", "exit_code": <integer or null>}` once the video file is finalized.

Either side ignores a message of a type it does not know, and the host refuses a start that names another protocol or version. A change that an old peer cannot read takes a new version number.

## The payload side

F1 (or the Video feature's button) starts a take. The payload creates the take's folder, opens `frames.jsonl` in it, and starts the take's thread, which connects with retries every half second until the host listens, sends the start line, and reads the host's answers without blocking. The connection is retried only until the first one succeeds: a host that refuses the take or drops it ends the video for that take, and the error says why, while the sidecar keeps being written until the take stops. At the stop the thread flushes the sidecar, sends the stop line, waits up to five seconds for the host's confirmation, and exits. A take that never reached a host finishes with an error saying it has no video.

The sidecar is one JSON line per record, in the order the records were made:

- A frame line per rendered frame, written from `render_engine_post_draw` on the frame's last near dispatch, when every eye image of the frame is final: `frame` (the take's frame ordinal, from zero), `unix_ms`, `game_run_time_us`, `predicted_display_time_ns`, and `views`, both eyes' raw located pose (position, orientation as `x, y, z, w`) in the runtime's LOCAL space and field of view as `left, right, up, down` angles in radians. Outside a session `views` and the display time are null.
- A mirror line whenever the mirror's layout changes, and at the first mirrored frame of the take: `frame` (the frame it applies from), `unix_ms`, and `mirror`, holding `view` (`Left`, `Right`, or `StereoPair`), the window's client size `window`, and `draws`, each eye's image as `eye` and `rect`, the whole eye image's rectangle `x, y, width, height` in window pixels, in on-screen order. A single eye's rectangle extends past the window where the mirror crops it.
- A take marker line (F2, the director's hotkey, Mark in the Video feature, or left stick click then right A in the headset): `frame`, the frame it falls before, and `marker`, holding its session ordinal `index`, the local `stamp`, `unix_ms`, `game_run_time_us`, and `label`.

- A present line for each present of the mirror: `frame` (the take's frame ordinal at the present, so the last rendered frame is one before it), `unix_ms`, `present`, the mirror's present counter since the payload loaded, and `stamped`, whether the frame stamp drew it into the window.

A reader tells the kinds apart by the `marker`, `mirror`, and `present` keys, which frame lines never have.

## The frame stamp

While a take runs, the mirror draws its present counter into the window's bottom-left corner as a strip of eight-pixel blocks (`payload/src/vr/mirror/stamp.rs`): a white start block, then the counter's low sixteen bits, least significant first, white for a one and black for a zero, then a black end block. The blocks are cleared straight into the back buffer with `ID3D11DeviceContext1::ClearView` rectangles, sized so they are square on the window whatever the back buffer's size, after the eye images and the overlay and just before the present. The sidecar's present lines carry the same counter, so a run of identical video frames can be read: one stamp held across several video frames is the recorder or the compositor repeating a present, while consecutive stamps on identical images are the mod presenting a repeated eye image. It is on by default (`vr.mirror_frame_stamp`, the "Frame stamp while capturing" switch under the desktop mirror in the debug UI's VR feature) and draws only while a take runs, so the window is clean otherwise. Reading the stamp out of a video is not automated yet; the blocks survive the default bitrate, but that is unverified on a real take. With no recording running, markers go to `markers.jsonl` in the session root instead, one JSON object per line with the same marker fields. Either way, while a tracking recording or its lookback collects, the same marker is also written to the tracking timeline as a `take_marker` event with the same fields ([testing/recorder.md](testing/recorder.md)), so the copies join by `index` and `stamp`.

## The capture host

`scripts/capture_host.sh [fallback output dir] [-- extra gpu-screen-recorder args]` runs the relay. For each connection the relay reads the start line, creates the folder (a stamped folder under the fallback directory, `captures/` by default, when the start names none), finds the game window, and runs gpu-screen-recorder on the window's region with the default output's audio, writing `video.mkv` into the folder. It waits for the recorder's first frame before confirming the start. At the stop it sends the recorder SIGINT, waits for it to finalize the file, and answers. The relay serves one take at a time and listens again after each, so one host serves every take of a session. Ctrl+C stops the relay, finalizing the take in progress; the recorder runs in its own session so the interrupt does not cut it off first.

The relay finds the window with `niri msg --json windows`: the window whose title equals the title the payload sent (`Just Cause 3` when it sent none), falling back to the Proton app id `steam_app_225540`. Several matches narrow by that app id, then by a window size equal to the payload's client size, then by focus. The region is the window's tile position in its workspace view plus the window's offset in the tile, offset by the output's logical position from `niri msg --json outputs`. niri reports the tile position for floating and tiled windows alike, so both work; the relay refuses a window whose workspace is not the active one on its output, and one that is not wholly inside the output (a tiled column scrolled partly off screen). The region is in logical pixels, which match the screen's pixels only on an output at scale 1; the relay warns on a scaled output. It refuses with a message naming the reason when the window is not found, several windows match, the window is off screen, niri or gpu-screen-recorder is missing, or the recorder exits at start, and the message reaches the in-game status.

The video is HEVC at a constant 10 Mbps by default (`-k hevc -bm cbr -q 10000`), about 1.1 GB for fifteen minutes of 1080p60. gpu-screen-recorder's own default is constant quality, which with H.264 at the `high` preset ran to about 36 Mbps on the game, 4.8 GB for fifteen minutes. AV1 at the same bitrate looks at least as good where the GPU can encode it; `gpu-screen-recorder --info` lists the codecs the GPU offers (the capture host's NVIDIA card lists H.264, HEVC, and AV1). A static desktop comes out well under the bitrate, so a short test clip of the desktop says little about the size of a game take.

The relay removes `__EGL_VENDOR_LIBRARY_FILENAMES`, `DRI_PRIME`, `VK_DRIVER_FILES`, and `VK_ICD_FILENAMES` from the recorder's environment. A terminal wrapped to render on another GPU passes them to everything it starts, and with them set gpu-screen-recorder finds no card to capture from.

The environment overrides are `JC3VR_CAPTURE_PORT` (default 47172, the payload's default), `JC3VR_CAPTURE_FPS` (default 60), `JC3VR_CAPTURE_CODEC` (`hevc` by default, `av1`, or `h264`), `JC3VR_CAPTURE_BITRATE` (the constant video bitrate in kbps, default 10000, or 0 for constant quality), `JC3VR_CAPTURE_QUALITY` (gpu-screen-recorder's quality preset for constant quality, default `high`), `JC3VR_CAPTURE_AUDIO` (a gpu-screen-recorder audio input, default `default_output`, or `none`), `JC3VR_CAPTURE_WINDOW` (a niri window id to record instead of looking the game up), and `JC3VR_CAPTURE_REGION` (a `WxH+X+Y` region to record without asking niri, for other compositors).

## Files

A recording's files share its folder, `recordings/<stamp>/` in the session folder ([sessions.md](sessions.md)):

- `frames.jsonl`: the payload's sidecar (above).
- `video.mkv`: the screen recording, H.264 video at the window's size (rounded up to even dimensions) and the default output's audio. It has a variable frame rate.
- `video.mkv.ts`: gpu-screen-recorder's record of the first frame's monotonic and wall-clock time.
- `stream.json`: the host's metadata, format `jc3vr-capture-stream` version 1: the video file's name, `video_start_unix_ms` (the first frame's wall clock), the recorder's spawn and stop times, its exit code, the recorded `region`, the `output` and its logical layout, the niri `window` (id, title, app id, and whether it floats), and the recorder's command line. The host writes it when the take starts and completes it at the stop.

A second connection for the same folder (the payload reconnecting to a restarted host) writes `video_1.mkv` and `stream_1.json`, and so on, rather than overwriting.

The video and the sidecar line up by wall clock: a sidecar line at `unix_ms` is at `unix_ms - video_start_unix_ms` milliseconds into the video. The payload and the host read the same clock, since wine reads the host's. The video frame that shows a rendered frame comes a little after its line, by the mirror's present, the compositor, and the recorder's own frame interval; the offset has not been measured, and is expected to be a frame or two. The take markers give a second reference.

## Post

`tools/capture/reframe.py VIDEO.mkv OUT.mp4` turns a take into a flat video. It reads `frames.jsonl` and the `stream.json` beside the video, gives each video frame the pose of the last rendered frame at or before its wall-clock time and the mirror layout in effect then, builds the eye's pinhole matrix in video pixels from the eye's field of view and its rectangle in the window, and warps the frame by the homography to a symmetric output camera of the requested horizontal field of view and aspect, so the crop is centred on the optical axis rather than the window centre. With `--smooth` above zero the output camera follows a Gaussian-smoothed head orientation instead of the recorded one, a rotation-only stabilisation from ground truth: a homography, no depth, nothing estimated. The part of the eye image visible in the window outside the crop is what the stabilised view pans into. The mirror must have shown a single eye; a take of the stereo pair is refused. `--dry-run` prints the framing and the first frame's corner mapping without decoding, and both modes list the take markers with their time in the video.

## Session recording

F1 starts a session recording: the video recorder and a tracking recording ([testing/recorder.md](testing/recorder.md)) started together under one stamp, so both halves land in one `recordings/<stamp>/` folder, the tracking recording as `tracking.jc3trk` beside the sidecar and the video. F1 again stops both, as it does when either one runs on its own. The Video feature has the same button and a "Session recording" line saying which halves run and under which stamp, with the tracking recording's error if it did not start (it needs `testing.enabled` and a player character). Neither recording has a length limit: the sidecar is appended by the take's thread and tracking batches by the tracking writer, so an hour-long session holds no more in memory than a short one.

The two halves line up by the OpenXR predicted display time, which the sidecar's `predicted_display_time_ns` and the tracking frames' `display_time_ns` share; `tracking-synth dump --from xr:<ns>` prints the tracking around a sidecar frame, and the sidecar's `unix_ms` places that frame in the video.

F2 is the take-marker action: an unlabelled marker into whatever records (above); in the headset the left stick click and then right A does the same, except in a vehicle while `controllers.bindings.suppress_developer_chords_in_vehicles` is on, as it is by default ([input/controllers.md](input/controllers.md#bindings-and-injection)). With no tracking recording running it also saves the tracking lookback, the last ten seconds, to `recordings/<stamp>/lookback.jc3trk`, so a moment is kept even when no session recording was started.

## Operating it

1. Start `scripts/capture_host.sh` on the host and leave it running for the session. It needs niri, gpu-screen-recorder, and Python 3 on the host.
2. Put the game window where it can stay for the take: on the active workspace, wholly on screen, uncovered. Pick the mirror view with F10.
3. Start the recording with F1 (a session recording), or the video alone from Tools > Video in the debug UI or its hotkey (Numpad / by default). The Video feature and the Status page show whether the host is connected, the recorded region and output once it records, and the host's error if it refused.
4. Stop the recording the same way: the host finalizes `video.mkv` into the take's folder and waits for the next take.

The recorder config (the capture host's address and port) is in the Video feature and applies to the next recording.

On an output where gpu-screen-recorder's region capture comes out squashed or offset, the display's plane may still have a stale size from boot; a modeset (switching the output's mode away and back) clears it.
