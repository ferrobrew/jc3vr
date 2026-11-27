# Render trace

The render trace records the frame's render events over a fixed number of real frames and writes them as NDJSON, for investigating stereo rendering artifacts without a debugger. The machinery is `crates/jc3vr-render/src/debug/trace.rs`, and the render hooks record into it.

Tools > Render capture starts a trace over the next N frames. While it runs, the frame tail runs inline on the game thread (`payload/src/hooks/game.rs`), so each eye has drained before the frame's events close. The trace is held in memory and written to `traces/<stamp>/trace.ndjson` in the session folder ([sessions.md](../sessions.md)) when its last frame ends, so a crash mid-trace loses it.

Each record has `time_ns` (since the trace started), `frame`, `eye` for per-dispatch events, and the event, tagged by its `ev` field. The first record is a `manifest` of the whole runtime configuration, so a capture says which gates and skips were on. The events cover the frame and dispatch markers, the post-effect and exposure gates, the buffer flow, and the shadow and depth-reconstruction state (`TraceEvent` in `trace.rs`).
