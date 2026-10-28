# In-process capture

The recorder streams one eye's rendered frames out of the game process at the render resolution and the headset's refresh rate, with a per-frame pose sidecar, so a trailer can be cut from footage that is neither a window capture nor limited to the desktop mirror's framing. The code is `payload/src/recorder/`, the host side is `scripts/capture_host.sh`, and the post-processing tool is `tools/capture/reframe.py`.

## Pipeline

The render thread does the minimum that needs the immediate context. In `render_engine_post_draw`, after the per-eye capture textures are copied for the frame, the recorder draws a fullscreen pass with `i420_pack_ps` that converts the chosen eye's display-referred RGBA into planar 4:2:0 YUV (BT.709, limited range) packed into a single `R8` texture of `width x height * 3 / 2`. That texture is copied into a ring of three staging textures, and the staging texture written two frames earlier is mapped with the no-wait flag. By then its copy has completed, so the map costs no GPU stall; if it has not, the map refuses and the frame is repeated. The mapped rows are copied into a pooled buffer and handed to the writer thread. The pass is skipped during the far-field phase and, in two-pass stereo, runs only on the dispatch that refreshed the chosen eye; under single-pass collapse both eyes are fresh on the one dispatch.

The writer thread owns a TCP connection to the host and the sidecar files. It connects with retries every half second, so the recording can be started before the host listener is up, and frames that arrive before the connection exists are dropped and counted. The stream is `yuv4mpegpipe`: a one-line header with the size, rate, and chroma layout, then `FRAME` followed by the planar bytes for each frame. It is self-describing and every frame is the same size, so the host side needs nothing configured and a dropped or repeated frame cannot desynchronise the parser. Frame rate is not measured; the header carries the value from the recorder config, which should be the headset's refresh rate.

Every captured display frame produces exactly one stream frame. A readback that is not ready re-sends the previous frame's bytes and is counted as a duplicate; a writer queue that is full drops the frame and is counted as a drop. The sidecar records which frames are duplicates, so the video's timing can be trusted frame by frame and corrected where it is not.

## Files

The host script writes `jc3vr_<stamp>_<n>.mkv` segments of ten minutes into its output directory, encoded with the first hardware H.264 encoder ffmpeg reports (NVENC, VAAPI, QSV, AMF) or libx264, with audio from the default sink's monitor unless overridden. Video and audio both take wall-clock timestamps at arrival, which is what lines them up; the take markers give a second reference.

The game writes the sidecar to `sessions/<stamp>/capture/<stamp>/` beside the payload DLL:

- `stream.json`: the stream size, rate, eye, pixel format, colour space and range, the host target, and the start wall clock.
- `frames.jsonl`: one line per stream frame with `stream_frame` (the frame number in the video), `capture_frame` (the render thread's ordinal), `duplicate`, `unix_ms`, `game_run_time_us`, `eye`, `predicted_display_time_ns`, and `views`: both eyes' raw located pose (position, orientation as `x, y, z, w`) in the runtime's LOCAL space and field of view as `left, right, up, down` angles in radians.

The capture ordinal, timestamps, and views describe the source pixels copied into the staging ring, rather than the later map or delivery attempt. A no-wait map that is not ready repeats both the previous pixels and their source metadata. `stream_frame` still advances for each delivered frame, and `duplicate` identifies that repeated source. Priming emits no video frame until a source is available. The existing JSON field set is unchanged; there is no separate delivery timestamp.

Take markers (`markers.jsonl` in the session root, from the director's hotkey) carry the same wall clock and game-run time, so a marker maps to a source frame through the sidecar.

## Post

`tools/capture/reframe.py` turns a segment plus its sidecar into a flat video. The eye render is off-axis and wide; the tool builds the eye's pinhole matrix from the sidecar's field of view, a symmetric output camera from the requested horizontal field of view and aspect, and warps each frame by the homography between them, so the crop is centred on the optical axis rather than the texture centre. With `--smooth` above zero the output camera follows a Gaussian-smoothed head orientation instead of the recorded one, which is a rotation-only stabilisation from ground truth: a homography, no depth, nothing estimated. The margin outside the crop is what the stabilised view pans into. `--dry-run` prints the framing and the first frame's corner mapping without decoding.

Segments after the first need `--offset`, the stream frame index the segment starts at, since the sidecar covers the whole recording.

## Operating it

1. Start `scripts/capture_host.sh [output dir]` on the host. It listens on port 47172 unless `JC3VR_CAPTURE_PORT` says otherwise.
2. In the headset, start the recording from the Director tab's Recording section or its hotkey (Numpad / by default). The tab shows the stream size, the frames sent, repeated, and dropped, and whether the host is connected.
3. Stop the recording the same way, or stop the host script; either closes the stream and finalises the last segment.

The recorder config (eye, scale, header frame rate, host, port) is in the same section and applies to the next recording. Scale below one shrinks the stream in the pack pass, which is the lever if the readback or the loopback cannot keep up at full eye resolution.
