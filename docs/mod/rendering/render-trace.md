# Render trace

The render trace records the frame's render events over a fixed number of real frames and writes them as NDJSON, for investigating stereo rendering artifacts without a debugger. The machinery is `crates/jc3vr-render/src/debug/trace.rs`; the render hooks record into it.

## Capturing

Tools > Render capture starts a trace over the next N frames, with N editable beside the button, and shows the frames left while it runs.

While a trace runs, the frame tail always runs inline on the game thread, so each eye has drained before the frame's trace events close.

## Output

A trace writes to `traces/<stamp>/` in the session folder ([sessions.md](../sessions.md)). `trace.ndjson` holds one record per line, each with `time_ns` (since the trace started), `frame` (the real-frame index within the trace), `eye` for per-dispatch events, and the `event` itself, tagged by its `ev` field. The first record is the `manifest`, a snapshot of the whole runtime configuration, so a capture says which gates and skips were on.

The trace is held in memory and written when its last frame ends, so a crash mid-trace loses it.

## Events

Every trace records the frame and dispatch markers, the post-effect and exposure gates, the buffer-flow events, and the shadow and depth-reconstruction state.

## Log lines

The trace dump logs the record count and path.
