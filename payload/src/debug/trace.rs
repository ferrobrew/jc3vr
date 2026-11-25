//! Render-call tracing subsystem: collects per-call NDJSON records over a fixed number of frames and
//! dumps them into the trace's session folder (see [`crate::session`]), with a self-describing
//! manifest as the first record.

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use parking_lot::Mutex;

use crate::{
    debug::{
        coord_frame::{self, CoordFrameSample},
        stereo_diff::{self, DrawDiff},
    },
    session::CaptureFolder,
};

/// Frames left in the active trace; 0 = inactive. Kept as a standalone atomic so the hot-path
/// [`tracing_active`] gate is lock-free -- the heavier state lives in [`TRACE_STATE`]. Driven by the
/// render capture's button.
static TRACE_FRAMES: AtomicI32 = AtomicI32::new(0);

/// The frame count a manual trace covers, edited from the render capture and reused as the floor
/// of the brightness-step auto-capture.
pub static CAPTURE_FRAMES: AtomicI32 = AtomicI32::new(60);

/// The in-progress trace's collected-but-unwritten state.
pub struct TraceState {
    /// NDJSON records collected so far.
    log: Vec<String>,
    /// The trace's [`crate::session::stamp`], naming its session folder, set at [`TraceState::start`].
    stamp: String,
    /// Absolute path of the most recent dump, shown in the UI so it's findable.
    last_path: Option<String>,
    /// Monotonic origin set at [`TraceState::start`]; each record's `time_ns` is measured from it.
    started: Option<std::time::Instant>,
    /// Real-frame index within the current trace (0-based; bumped each [`TraceState::end_frame`]).
    frame: u32,
}

/// One dumped trace record: a [`TraceEvent`] plus when/where it happened. Round-trips through serde,
/// so a dumped trace reads back into `Vec<TraceRecord>` for analysis.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TraceRecord {
    /// Nanoseconds since the trace started (monotonic), for ordering + intra-frame timing.
    pub time_ns: u64,
    /// Real-frame index within the trace (0-based).
    pub frame: u32,
    /// The eye being drawn, for per-dispatch events; omitted for frame/driver markers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye: Option<usize>,
    /// The event itself.
    pub event: TraceEvent,
}
static TRACE_STATE: Mutex<TraceState> = Mutex::new(TraceState::new());

/// GPU draw-call counts for one eye's Draw. The live global counter lives in `hooks::draw_count`;
/// this same type is the [`TraceEvent::DrawEnd`] field, with serde de/serializing the atomics as
/// their plain values.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DrawCounts {
    pub draw: AtomicUsize,
    pub draw_indexed: AtomicUsize,
    pub dispatch: AtomicUsize,
}
impl DrawCounts {
    pub const fn new() -> Self {
        Self {
            draw: AtomicUsize::new(0),
            draw_indexed: AtomicUsize::new(0),
            dispatch: AtomicUsize::new(0),
        }
    }

    /// Reset all three to zero (at each eye's `draw_begin`).
    pub fn clear(&self) {
        self.draw.store(0, Ordering::Relaxed);
        self.draw_indexed.store(0, Ordering::Relaxed);
        self.dispatch.store(0, Ordering::Relaxed);
    }

    /// A frozen copy of the current values for the `draw_end` event (the live `static` can't move).
    pub fn snapshot(&self) -> Self {
        Self {
            draw: AtomicUsize::new(self.draw.load(Ordering::Relaxed)),
            draw_indexed: AtomicUsize::new(self.draw_indexed.load(Ordering::Relaxed)),
            dispatch: AtomicUsize::new(self.dispatch.load(Ordering::Relaxed)),
        }
    }
}

/// One render event, serialized inside a [`TraceRecord`] (which stamps timing / frame / eye). The
/// `ev` tag names the event. Frame/driver markers that carry their own `eye` field set it explicitly.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "ev")]
pub enum TraceEvent {
    /// First record of every trace: a snapshot of the active runtime toggles, so a capture is
    /// self-describing (which gates / skips / the exposure pin were on) without external notes.
    #[serde(rename = "manifest")]
    Manifest {
        /// The trace's [`crate::session::stamp`], matching its folder name.
        timestamp: String,
        /// The full runtime configuration snapshot (nests cleanly via its own `Serialize`). Boxed
        /// so the one-off manifest doesn't inflate every event in the enum
        /// (`clippy::large_enum_variant`).
        config: Box<crate::config::Config>,
    },
    #[serde(rename = "frame_begin")]
    FrameBegin {
        stereo: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        present_eye: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_counters: Option<bool>,
    },
    #[serde(rename = "draw_begin")]
    DrawBegin { eye: usize },
    #[serde(rename = "draw_end")]
    DrawEnd { eye: usize, counts: DrawCounts },
    #[serde(rename = "SetupRenderCamera")]
    SetupRenderCamera,
    /// One `RenderBlockPostEffects::Draw` invocation. `repeat` marks a second-or-later invocation
    /// within one dispatch (the stale draw-list entry the between-eye parity restore cannot zero);
    /// `skip` whether the dedupe gate suppressed it. A healthy stereo frame shows exactly one
    /// non-repeat per dispatch.
    #[serde(rename = "PostEffectsBlockDraw")]
    PostEffectsBlockDraw { repeat: bool, skip: bool },
    /// The sun-shadow constants staged for this dispatch, read in `SetGlobalShaderConstants` before
    /// the anchor correction mutates them -- i.e. the raw parity-slot data the engine produced. For
    /// diagnosing the one-frame shadow-boundary jumps (issue #10's residual flicker): across a trace,
    /// `translation`/`scale_blend0` should evolve smoothly with the camera and match between the two
    /// eyes of a frame; a value that alternates between consecutive frames indicts the parity slots
    /// (even/odd frames reading diverged fits), while a smooth series clears the constants and points
    /// downstream (atlas contents or the temporal resolve).
    #[serde(rename = "ShadowState")]
    ShadowState {
        /// `render_frame_counters.m_Counter` at stage time (the sim-side shadow write selector).
        counter: u32,
        /// `render_frame_counters.m_FrameIndex` (the draw-side parity selector; parity = bit 0).
        frame_index: u32,
        /// The cascade transform's translation row, straight from the parity slot (pre-correction).
        translation: [f32; 4],
        /// All six cascades' scale + blend band (`m_ScaleBlend`). Cascade 0 refreshes every frame;
        /// the higher cascades are round-robined, so a per-parity vintage mismatch (the freeze
        /// diagnostic's artifact class) shows here as values alternating between consecutive frames.
        scale_blend: [[f32; 4]; 6],
        /// All six cascades' offset + texture-array slice (`m_OffsetRadius`).
        offset_radius: [[f32; 4]; 6],
        active_cascades: u32,
        /// The anchor-fix delta about to be applied (zero when disparity is off).
        anchor_delta: [f32; 3],
        /// The global sun-shadow fade scalar (`ShadowManager::GetShadowFade`) staged this dispatch --
        /// the whole-terrain shadow-strength knob. A value that flickers frame to frame in eye 0 while
        /// eye 1 stays put indicts a per-frame global the double-draw re-evaluates.
        shadow_fade: f32,
        /// The sub-frame interpolation fraction (`dtf`) the active camera's `UpdateRender` last
        /// received -- the render/sim-tick decoupling knob (issue #20). The cascade fit reads the
        /// interpolated camera `lerp(T0, T1, dtf)`, so if `translation` zig-zags frame to frame in
        /// lockstep with `dtf`, the flicker is the fit chasing a sub-frame-oscillating camera rather
        /// than any per-eye or projection effect.
        dtf: f32,
        /// The active camera's interpolated world translation (`m_TransformF` row 3, xyz) at stage
        /// time -- the position the sun-shadow fit anchors to. Correlate against `translation`: both
        /// zig-zagging together frame to frame confirms the fit is following the interpolated camera.
        cam_translation: [f32; 3],
        /// The active camera's forward basis vector (`m_TransformF` column 2, xyz) at stage time. The
        /// engine pushes each cascade box's centre forward along this vector, so under fast rotation
        /// (a spin) the fit centre sweeps a circle even while `cam_translation` stays put. If the
        /// cascade `translation` zig-zag tracks this vector rotating, the flicker is the swept fit
        /// centre texel-snapping, not the camera position.
        cam_forward: [f32; 3],
    },
    /// Per-call state of the off-axis depth-reconstruction override (`PerspectiveFovInverse`). The
    /// off-axis inverse is rebuilt every call from the eye's live projection and the engine's live
    /// near/far planes; if any of those wobble frame to frame, the reconstructed world positions swim
    /// and the sun shadow sampled through them crawls. A stable `proj`/`near`/`far` across frames
    /// clears the matrix and points at the depth input instead; a wobbling one indicts the live
    /// projection rebuild.
    #[serde(rename = "ReconstructionState")]
    ReconstructionState {
        /// The near/far the pass requested (the engine's per-pass symmetric params).
        req_near: f32,
        req_far: f32,
        /// The engine's live active-camera planes the override anchors to (its near/far reference).
        ref_near: f32,
        ref_far: f32,
        /// Whether the off-axis override applied (vs. yielding to the symmetric rebuild).
        applied: bool,
        /// Fingerprint of the off-axis projection being inverted: `[data0 (x-scale), data5 (y-scale),
        /// data8 (x-shear), data9 (y-shear), data10 (z-scale)]`. Any per-frame drift here is the matrix
        /// wobbling.
        proj: [f32; 5],
    },
    #[serde(rename = "SetupRenderFrameData")]
    SetupRenderFrameData { gated: bool },
    #[serde(rename = "HandBackBuffers")]
    HandBackBuffers { gated: bool },
    #[serde(rename = "SmoothedExposureUpdate")]
    SmoothedExposureUpdate { gated: bool, exposure: f32 },
    #[serde(rename = "CalcHistogramMidBright")]
    CalcHistogramMidBright { gated: bool },
    #[serde(rename = "ToneMappingUpdate")]
    ToneMappingUpdate { gated: bool },
    /// Per-frame exposure internals, read from inside `ToneMappingEffect::Update` (the canonical
    /// exposure write, with the live effect in hand). `divisor` is `m_Histogram2`'s mid-point -- the
    /// value the converged exposure actually tracks (`target = target_num / divisor`). The stereo
    /// darkening should show up here as `divisor` (hence `target`/`exposure`) differing from
    /// non-stereo, isolating whether it's the second histogram's metering or its ping-pong readback.
    #[serde(rename = "ExposureInternals")]
    ExposureInternals {
        exposure: f32,
        target_num: f32,
        divisor: f32,
        target: f32,
        hist1_bright: f32,
        hist1_mid: f32,
        hist1_buckets: Vec<u32>,
        hist2_bright: f32,
        hist2_mid: f32,
        hist2_buckets: Vec<u32>,
        num_buckets: u32,
        /// The metering ping-pong selector (`ToneMappingEffect::m_HistogramPingPong`), to spot a
        /// buffer-swap mismatch under stereo.
        pingpong: u32,
        forced: bool,
    },
    /// The render engine's CPU staging copies of the global shader constant buffers: `fp` is the
    /// 95-row fragment `GlobalConstants` block (lighting, wetness, shadow, per-view camera rows),
    /// `vp` the 49-row vertex block (view-projection, fog, and atmospheric-scattering rows), each
    /// flattened row-major. The blocks are re-staged per pass, so `at` names the sample point
    /// (`scene` at the aerial-perspective composite, `frame_end` after the draw drains). Diffing a
    /// good frame against a bad one identifies which row a latched change rides in. Gated on
    /// `stereo.diagnose_global_constants`.
    #[serde(rename = "GlobalConstants")]
    GlobalConstants {
        at: String,
        fp: Vec<f32>,
        vp: Vec<f32>,
    },
    /// The subsampled linear mean of the HDR MainColor buffer at a named pipeline stage
    /// (`post_resolve`, `pre_atmosphere`, `post_atmosphere`, `post_block_entry`, `pre_post`, or a
    /// `pass0xNN` label from the per-pass sweep). Bracketing a global change between two stages
    /// names the frame segment that injects it -- the instrument that localized the foveation
    /// blend-state latch. Gated on `stereo.diagnose_main_color_means` (stage brackets) and
    /// `stereo.diagnose_pass_sweep` (the per-pass ladder).
    #[serde(rename = "MainColorMean")]
    MainColorMean { at: String, r: f32, g: f32, b: f32 },
    #[serde(rename = "ToneMappingApply")]
    ToneMappingApply { skip: bool },
    #[serde(rename = "GenerateFinalHistogram")]
    GenerateFinalHistogram { skip: bool },
    #[serde(rename = "ApplyWorldFilters")]
    ApplyWorldFilters { gated: bool },
    #[serde(rename = "ApplyGlobalFilters")]
    ApplyGlobalFilters { gated: bool },
    #[serde(rename = "SsaoDraw")]
    SsaoDraw { forced_first_pass: bool },
    #[serde(rename = "Fsr::Dispatch")]
    FsrDispatch { input: u32, ran: bool },
    #[serde(rename = "DoF::Apply")]
    DofApply { input: u32, skip: bool },
    #[serde(rename = "MotionBlur::Apply")]
    MotionBlurApply { input: u32, skip: bool },
    #[serde(rename = "Glare::Apply")]
    GlareApply { skip: bool },
    #[serde(rename = "Fade::Apply")]
    FadeApply { skip: bool },
    #[serde(rename = "PlayerDamage::Apply")]
    PlayerDamageApply { input: u32, skip: bool },
    #[serde(rename = "SunHalo::PreApply")]
    SunHaloPreApply { skip: bool },
    #[serde(rename = "SunHalo::Apply")]
    SunHaloApply { skip: bool },
    #[serde(rename = "PostDraw")]
    PostDraw,
    #[serde(rename = "Flip")]
    Flip { blocked: bool },
    // Buffer-flow events (raw pointers as u64 so render-setup / texture instances can be compared
    // across eyes -- same pointer = same target, different pointer = a swapped instance).
    #[serde(rename = "SetRenderSetup")]
    SetRenderSetup {
        setup: u64,
        /// Draws/dispatches issued into the *previous* target since the last bind (this thread).
        draws: usize,
        indexed: usize,
        dispatch: usize,
    },
    #[serde(rename = "Clear")]
    Clear { color: [f32; 4] },
    #[serde(rename = "CopySurfaceToTexture")]
    CopySurfaceToTexture { dst: u64, src: u64 },
    #[serde(rename = "ResolveSurface")]
    ResolveSurface,
    /// A content hash of one engine render target, captured per eye after that eye's Draw completes
    /// (see [`crate::debug::rt_hash`]). With `stereo.cameras` off (both eyes share one camera) the two
    /// eyes' hashes for a given RT must match; any mismatch means that RT is written twice without a
    /// per-dispatch reset -- the "stronger in one eye" accumulation bug.
    #[serde(rename = "rt_hash")]
    RtHash { rt: RtKind, hash: u64 },
    /// One stereo frame's per-pass draw-sequence diff between the eyes, recorded after both eyes
    /// drain (see [`crate::debug::stereo_diff`]). Gated on `stereo.diagnose_stereo_draw_diff`.
    #[serde(rename = "StereoDrawDiff")]
    StereoDrawDiff(DrawDiff),
    /// The render camera's world frame and travel direction, once per real frame (see
    /// [`crate::debug::coord_frame`]).
    #[serde(rename = "CoordFrame")]
    CoordFrame(CoordFrameSample),
}

/// The engine render targets hashed by the per-eye RT-doubling diagnostic. The colour/material buffers
/// are where a doubled darkening term (shadow / AO) surfaces; velocity is included to localise where
/// the per-eye divergence first enters (with identical cameras, the geometry-pass outputs --
/// GBuffer1/2/3 -- stay identical, so a divergent velocity points upstream of lighting at the
/// previous-frame view-projection, while a divergent GBuffer0/MainColor alone points at a screen-space
/// resolve or scratch reuse). Depth targets are excluded -- they are awkward and fault-prone to read
/// back on the CPU and track the geometry pass anyway.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtKind {
    MainColor,
    GBuffer0,
    GBuffer1,
    GBuffer2,
    GBuffer3,
    Velocity,
    BackBufferLinear,
}

/// Whether a render trace is currently collecting. Lock-free; lets hooks skip readback work when off.
pub fn tracing_active() -> bool {
    TRACE_FRAMES.load(Ordering::Relaxed) > 0
}

/// Frames left in the active trace (0 = inactive), for the debug UI's countdown.
pub fn active_frames() -> i32 {
    TRACE_FRAMES.load(Ordering::Relaxed)
}

impl Default for TraceState {
    /// Delegates to the const [`TraceState::new`] -- the `TRACE_STATE` static needs a const
    /// initializer, which a derived `Default` can't provide, so `new` stays the single source.
    fn default() -> Self {
        Self::new()
    }
}

impl TraceState {
    const fn new() -> Self {
        Self {
            log: Vec::new(),
            stamp: String::new(),
            last_path: None,
            started: None,
            frame: 0,
        }
    }

    /// Build a [`TraceRecord`] (stamping the current time + frame) and append it (caller holds the
    /// lock).
    fn push_event(&mut self, eye: Option<usize>, event: TraceEvent) {
        let record = TraceRecord {
            time_ns: self
                .started
                .map(|s| s.elapsed().as_nanos() as u64)
                .unwrap_or(0),
            frame: self.frame,
            eye,
            event,
        };
        if let Ok(s) = serde_json::to_string(&record) {
            self.log.push(s);
        }
    }

    /// Write the collected log to `trace.ndjson` in the trace's session folder and record its path
    /// for the UI (caller holds the lock).
    fn dump(&mut self) {
        let dir = match crate::session::capture_folder(CaptureFolder::Trace, &self.stamp) {
            Ok(dir) => dir,
            Err(e) => {
                tracing::error!("render trace: {e}");
                return;
            }
        };
        let path = dir.join("trace.ndjson");
        match std::fs::write(&path, self.log.join("\n")) {
            Ok(()) => {
                let shown = path.display().to_string();
                tracing::info!(
                    "Render trace dumped: {} records -> {}",
                    self.log.len(),
                    shown
                );
                self.last_path = Some(shown);
            }
            Err(e) => tracing::error!("Failed to write render trace: {e}"),
        }
    }

    /// The collecting trace's stamp and current real-frame index, or `None` when no trace is
    /// collecting. Used by [`crate::debug::rt_hash`] to name per-frame render-target images into the
    /// same folder as the trace NDJSON.
    pub fn current_frame() -> Option<(String, u32)> {
        if !tracing_active() {
            return None;
        }
        let state = TRACE_STATE.lock();
        Some((state.stamp.clone(), state.frame))
    }

    /// Append a frame/driver marker record (no dispatch eye) while a trace is active (auto-locks).
    pub fn record(event: TraceEvent) {
        if tracing_active() {
            TRACE_STATE.lock().push_event(None, event);
        }
    }

    /// Append a per-dispatch record tagged with the current eye while a trace is active (auto-locks).
    pub fn record_eye(event: TraceEvent) {
        if tracing_active() {
            let eye = crate::stereo::draw_index();
            TRACE_STATE.lock().push_event(Some(eye), event);
        }
    }

    /// Begin a render-call trace covering the next `frames` real frames (auto-locks). The manifest is
    /// written as the first record while the counter is still 0 (lock held), then the counter is
    /// armed -- so render-thread hooks that observe it can never race ahead of the manifest.
    pub fn start(frames: i32) {
        let stamp = crate::session::stamp();
        {
            let mut state = TRACE_STATE.lock();
            let last_path = state.last_path.take();
            *state = TraceState {
                last_path,
                stamp: stamp.clone(),
                started: Some(std::time::Instant::now()),
                ..Default::default()
            };
            state.push_event(None, build_manifest(stamp));
        }
        stereo_diff::begin_run();
        coord_frame::begin_run();
        TRACE_FRAMES.store(frames, Ordering::Relaxed);
        tracing::info!("Render trace started ({frames} frames)");
    }

    /// Tick one real frame (called by the Draw driver); on the final frame, dump the NDJSON
    /// (auto-locks).
    pub fn end_frame() {
        if TRACE_FRAMES.load(Ordering::Relaxed) <= 0 {
            return;
        }
        let finished = TRACE_FRAMES.fetch_sub(1, Ordering::Relaxed) - 1 <= 0;
        let mut state = TRACE_STATE.lock();
        state.frame += 1;
        if finished {
            state.dump();
            drop(state);
            stereo_diff::finish_run();
            coord_frame::finish_run();
        }
    }

    /// The most recent dumped trace file's path, for the UI (auto-locks).
    pub fn last_path() -> Option<String> {
        TRACE_STATE.lock().last_path.clone()
    }
}

/// Snapshot the active runtime config into a [`TraceEvent::Manifest`] (the first record of a trace).
fn build_manifest(timestamp: String) -> TraceEvent {
    TraceEvent::Manifest {
        timestamp,
        config: Box::new(crate::config::get()),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;
    use crate::debug::stereo_diff::NamedPass;

    /// `tools/analyze_trace_frames.py` reads the probe events by their `ev` tag with the payload's
    /// fields flattened beside it; this pins that shape.
    #[test]
    fn probe_events_flatten_beside_their_tag() {
        let record = TraceRecord {
            time_ns: 5,
            frame: 2,
            eye: None,
            event: TraceEvent::StereoDrawDiff(DrawDiff {
                shared_passes: 1,
                shared_ops: 4,
                matched_ops: 4,
                order_consistent: true,
                eye1_only: vec![NamedPass {
                    pass: 3,
                    name: "P3".into(),
                }],
                ..DrawDiff::default()
            }),
        };
        assert_eq!(
            serde_json::to_value(&record).unwrap(),
            json!({
                "time_ns": 5,
                "frame": 2,
                "event": {
                    "ev": "StereoDrawDiff",
                    "shared_passes": 1,
                    "shared_ops": 4,
                    "matched_ops": 4,
                    "eye0_only_passes": 0,
                    "eye0_only_ops": 0,
                    "order_consistent": true,
                    "divergent": [],
                    "eye1_only": [{ "pass": 3, "name": "P3" }],
                },
            })
        );
        let event = serde_json::to_value(TraceEvent::CoordFrame(CoordFrameSample {
            position: [0.0; 3],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            z_basis: [0.0, 0.0, 1.0],
            delta: [0.0; 3],
            dot_neg_z: 0.0,
            dot_pos_z: 0.0,
            dot_right: 0.0,
            dot_up: 0.0,
        }))
        .unwrap();
        assert_eq!(event["ev"], "CoordFrame");
        assert_eq!(event["z_basis"], json!([0.0, 0.0, 1.0]));
    }
}
