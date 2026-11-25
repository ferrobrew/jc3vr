# Render trace

The render trace records the frame's render events over a fixed number of real frames and writes them as NDJSON, for investigating stereo rendering artifacts without a debugger. The machinery is `payload/src/debug/trace.rs`; the probes that feed it are the other modules in `payload/src/debug/`.

## Capturing

Tools > Render capture starts a trace over the next N frames, with N editable beside the button, and shows the frames left while it runs. Every probe toggle lives in the render RE window ([debug-ui.md](../debug-ui.md#render-re-window)) under "Render trace probes", so a capture records whatever those toggles are set to when it starts. The brightness-step auto-capture, also in that section, starts a trace by itself the first time the MainColor mean steps against its recent median, covering the render capture's frame count with a floor of 30 frames, and then disarms.

While a trace runs, the frame tail always runs inline on the game thread, so each eye has drained before the frame's trace events close.

## Output

A trace writes to `traces/<stamp>/` in the session folder ([sessions.md](../sessions.md)). `trace.ndjson` holds one record per line, each with `time_ns` (since the trace started), `frame` (the real-frame index within the trace), `eye` for per-dispatch events, and the `event` itself, tagged by its `ev` field. The first record is the `manifest`, a snapshot of the whole runtime configuration, so a capture says which gates, skips, and probes were on. With the per-eye screenshot probe on, the folder also holds `frameNNN_eye0.png` and `frameNNN_eye1.png`, each eye's final `BackBufferLinear` per frame with its alpha forced opaque. The game thread only copies the pixels out; the shared image writer that also writes F12 screenshots encodes and writes them on its own thread, and blocks the next submit while its queue is full, so a long screenshot trace is bounded in memory and costs no more than writing inline.

The trace is held in memory and written when its last frame ends, so a crash mid-trace loses it.

## Probes

The always-recorded events are the frame and dispatch markers, the post-effect and exposure gates, the buffer-flow events, the shadow and depth-reconstruction state, and the coordinate frame. The toggled probes are:

- Per-eye screenshots (`stereo.diagnose_rt_screenshots`): the PNGs above.
- MainColor means (`stereo.diagnose_main_color_means`, and `stereo.diagnose_pass_sweep` for the per-pass ladder): `MainColorMean` events at the pipeline stage brackets or after every late-scene pass, in `payload/src/debug/pipeline_probes.rs`.
- Global constants (`stereo.diagnose_global_constants`): `GlobalConstants` events with the fragment and vertex constant staging blocks at scene time and frame end.
- Render-target hashes (`stereo.diagnose_rt_hashes`): `rt_hash` events per eye, for finding a target written twice without a per-dispatch reset; run with stereo cameras off. See `payload/src/debug/rt_hash.rs`.
- Stereo draw diff (`stereo.diagnose_stereo_draw_diff`): `StereoDrawDiff` events, described below.

### Stereo draw diff

The single-pass stereo feasibility probe (`payload/src/debug/stereo_diff.rs`). During each traced stereo frame, `RenderPass::DoDraw` records each pass's GPU op count (draws, indexed draws, and dispatches) per eye. After both eyes drain, the two sequences are aligned by pass id and the frame's `StereoDrawDiff` event records the passes both eyes run and their op total, the ops in passes whose counts agree, the passes only eye 0 runs (the shared prepasses), whether the shared passes keep the same relative order, each divergent pass with both eyes' counts, and any pass only eye 1 runs, which should not happen. The probe arms at the stereo frame boundary, so a trace started mid-frame does not diff a partial sequence.

### Coordinate frame

The coordinate-frame probe (`payload/src/debug/coord_frame.rs`) records a `CoordFrame` event once per real frame from the render camera before the per-eye offset: the `m_TransformF` position and basis rows, the position change since the previous sample, and the travel direction's dot products with `-z_basis`, `z_basis`, right, and up. Walking forward with `dot_neg_z` near +1 confirms the world frame is right-handed and Y-up with forward along −Z (the coordinate-frame gate in [vr-runtime.md](../vr-runtime.md)).

## Log lines

The stereo draw diff and the coordinate-frame probe log nothing per frame. When a trace ends, each logs one summary line if it recorded anything: the diff's mismatched frame count, distinct divergent passes, and lowest replayable share (target `stereo_diff`), and the coordinate frame's sample count and mean forward dot while moving (target `coord_frame`). The trace dump itself logs the record count and path.

## Analysis

`tools/analyze_trace_frames.py <trace folder>` (run with `uv run`) summarises the `StereoDrawDiff` and `CoordFrame` events, then, when the folder holds screenshots, analyses them for per-frame flicker: the luminance pulse, a temporal standard-deviation heatmap, a tile ranking, the per-eye gap, the frame-parity split, and the correlation with the traced exposure.
