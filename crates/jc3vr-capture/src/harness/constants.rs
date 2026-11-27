use dxbc_stereo::{OFFSET_VIEW_PROJECTION_CB0_ROWS, PER_EYE_CB0_ROWS, STEREO_REPROJ_CB_ROWS};
use jc3gi::{
    graphics_engine::{
        graphics_engine::{RenderContext, get_render_frame_counters},
        render_engine::{RenderEngine, RenderPassId},
    },
    types::math::Vector4,
};
use jc3vr_render::stereo::single_pass;
use jc3vr_view::{
    stereo::{dispatch, dispatch::RenderMode, draw_index},
    vr,
};
use parking_lot::Mutex;
use serde::Serialize;

use crate::{harness::screenshot_wants_eyes, testing::visual};

#[derive(Clone, Default, Serialize)]
pub(crate) struct CapturedConstants {
    pub records: Vec<ConstantRecord>,
    pub(crate) lighting_records: Vec<LightingConstantRecord>,
    pub(crate) unscoped_uploads: u32,
}

#[derive(Clone, Serialize)]
pub(crate) struct ConstantRecord {
    pub frame: i64,
    pub generation: u64,
    pub mode: RenderMode,
    pub(crate) physical_eye: PhysicalEye,
    pub pass: GeometryPass,
    pub render_context: ContextInputs,
    pub(crate) time_inputs: RenderTimeInputs,
    pub constants: ShaderConstants,
}

#[derive(Clone, Serialize)]
pub(crate) struct LightingConstantRecord {
    pub frame: i64,
    pub generation: u64,
    pub mode: RenderMode,
    pub(crate) physical_eye: PhysicalEye,
    pub pass: DeferredLightsPass,
    pub render_context: ContextInputs,
    pub(crate) time_inputs: RenderTimeInputs,
    pub(crate) vertex_globals: Vec<F32s<4>>,
    pub(crate) fragment_globals: Vec<F32s<4>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhysicalEye {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct GeometryPass(i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct DeferredLightsPass(i32);

#[derive(Clone, Serialize)]
pub(crate) struct ContextInputs {
    pub view: F32s<16>,
    pub projection: F32s<16>,
    pub offset_view_projection: F32s<16>,
    pub camera_position: F32s<3>,
}

#[derive(Clone, Serialize)]
pub(crate) struct RenderTimeInputs {
    pub engine: EngineTimeInputs,
    pub context: ContextTimeInputs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RenderFrameCountersInputs {
    pub counter: u32,
    pub frame_index: u32,
    pub(crate) ring_index: u32,
}

impl RenderFrameCountersInputs {
    const fn new(counter: u32, frame_index: u32, ring_index: u32) -> Self {
        Self {
            counter,
            frame_index,
            ring_index,
        }
    }

    /// The engine's live per-draw counters, read at the same capture point as the constants so the
    /// provenance describes the pass's frame.
    fn read() -> Self {
        let counters = unsafe { get_render_frame_counters() };
        Self::new(
            counters.m_Counter,
            counters.m_FrameIndex,
            counters.m_RingIndex,
        )
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct EngineTimeInputs {
    pub(crate) time_of_day: F32s<1>,
    pub dt: F32s<1>,
    pub(crate) wind_animation: F32s<1>,
    pub(crate) acc_frame_time: u32,
    pub(crate) render_frame_counters: RenderFrameCountersInputs,
}

#[derive(Clone, Serialize)]
pub(crate) struct ContextTimeInputs {
    pub(crate) time_of_day: F32s<1>,
    pub dt: F32s<1>,
    pub(crate) wind_animation: F32s<1>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(crate) enum ShaderConstants {
    MultipassCb0 {
        cb0: Cb0Rows,
    },
    SinglePassCb13 {
        centered_cb0: Cb0Rows,
        live_cb13: Option<Box<Cb13Rows>>,
        cb13_status: Cb13Status,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cb13Status {
    Uploaded,
    UploadUnavailable,
    LiveRowsUnavailable,
}

#[derive(Clone, Serialize)]
pub(crate) struct Cb0Rows {
    pub offset_view_projection: [F32s<4>; 4],
    pub camera_position: F32s<4>,
}

#[derive(Clone, Serialize)]
pub(crate) struct Cb13Rows {
    pub eyes: [Cb0Rows; 2],
    pub reprojection: [[F32s<4>; 4]; 2],
}

#[derive(Clone, Serialize)]
#[serde(bound(serialize = "[f32; N]: Serialize, [u32; N]: Serialize"))]
pub(crate) struct F32s<const N: usize> {
    pub values: [f32; N],
    pub bits: [u32; N],
}

pub(crate) fn capture(engine: &RenderEngine, ctx: &RenderContext, cb13_uploaded: bool) {
    let Some((identity, physical_eye)) = observation() else {
        return;
    };
    let frame_counters = RenderFrameCountersInputs::read();
    let mut state = STATE.lock();
    state.observe(identity);
    if let Some(pass) = DeferredLightsPass::new(ctx.m_ActiveRenderPass) {
        if !state.contains_lighting(physical_eye) {
            state.insert_lighting(LightingConstantRecord {
                frame: identity.frame,
                generation: identity.generation,
                mode: identity.mode,
                physical_eye,
                pass,
                render_context: ContextInputs::from_context(ctx),
                time_inputs: RenderTimeInputs::new(engine, ctx, frame_counters),
                vertex_globals: global_rows(&engine.m_VPGlobalConstData),
                fragment_globals: global_rows(&engine.m_FPGlobalConstData),
            });
        }
        return;
    }
    let Some(pass) = state.geometry_pass(ctx.m_ActiveRenderPass) else {
        return;
    };
    if state.contains(physical_eye, pass) {
        return;
    }
    let cb0 = Cb0Rows::from_engine(engine);
    let constants = if single_pass::active() {
        let (live_cb13, cb13_status) = cb13_snapshot(cb13_uploaded, || {
            single_pass::live_cb13_rows().map(|rows| rows.map(|row| row.data))
        });
        ShaderConstants::SinglePassCb13 {
            centered_cb0: cb0,
            live_cb13,
            cb13_status,
        }
    } else {
        ShaderConstants::MultipassCb0 { cb0 }
    };
    state.insert(ConstantRecord {
        frame: identity.frame,
        generation: identity.generation,
        mode: identity.mode,
        physical_eye,
        pass,
        render_context: ContextInputs::from_context(ctx),
        time_inputs: RenderTimeInputs::new(engine, ctx, frame_counters),
        constants,
    });
}

pub(crate) fn take(frame: i64) -> CapturedConstants {
    let metadata = visual::metadata();
    if !metadata.ready {
        reset();
        return CapturedConstants::default();
    }
    STATE.lock().take(Identity {
        frame,
        generation: metadata.generation,
        mode: dispatch::mode(),
    })
}

pub(crate) fn reset() {
    *STATE.lock() = State::new();
}

impl PhysicalEye {
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            _ => None,
        }
    }
}

impl GeometryPass {
    fn new(pass: i32) -> Option<Self> {
        (FIRST_PASS..LAST_PASS)
            .contains(&pass)
            .then_some(Self(pass))
    }
}

impl DeferredLightsPass {
    fn new(pass: i32) -> Option<Self> {
        (pass == RenderPassId::RP_DEFERRED_LIGHTS as i32).then_some(Self(pass))
    }
}

impl ContextInputs {
    fn from_context(ctx: &RenderContext) -> Self {
        Self {
            view: F32s::new(ctx.m_View.data),
            projection: F32s::new(ctx.m_ProjectionF.data),
            offset_view_projection: F32s::new(ctx.m_OffsetViewProjection.data),
            camera_position: F32s::new(ctx.m_CameraPosition.data),
        }
    }
}

impl RenderTimeInputs {
    fn new(
        engine: &RenderEngine,
        ctx: &RenderContext,
        render_frame_counters: RenderFrameCountersInputs,
    ) -> Self {
        Self {
            engine: EngineTimeInputs::from_values(
                engine.m_TimeOfDay,
                engine.m_Dt,
                engine.m_WindAnimation,
                engine.m_AccFrameTime,
                render_frame_counters,
            ),
            context: ContextTimeInputs::from_values(ctx.m_TimeOfDay, ctx.m_Dt, ctx.m_WindAnimation),
        }
    }
}

impl EngineTimeInputs {
    fn from_values(
        time_of_day: f32,
        dt: f32,
        wind_animation: f32,
        acc_frame_time: u32,
        render_frame_counters: RenderFrameCountersInputs,
    ) -> Self {
        Self {
            time_of_day: F32s::new([time_of_day]),
            dt: F32s::new([dt]),
            wind_animation: F32s::new([wind_animation]),
            acc_frame_time,
            render_frame_counters,
        }
    }
}

impl ContextTimeInputs {
    fn from_values(time_of_day: f32, dt: f32, wind_animation: f32) -> Self {
        Self {
            time_of_day: F32s::new([time_of_day]),
            dt: F32s::new([dt]),
            wind_animation: F32s::new([wind_animation]),
        }
    }
}

impl Cb0Rows {
    fn from_engine(engine: &RenderEngine) -> Self {
        Self {
            offset_view_projection: OFFSET_VIEW_PROJECTION_CB0_ROWS
                .map(|row| F32s::new(engine.m_VPGlobalConstData[row as usize].data)),
            camera_position: F32s::new(
                engine.m_VPGlobalConstData[PER_EYE_CB0_ROWS[0] as usize].data,
            ),
        }
    }
}

impl Cb13Rows {
    fn new(rows: [[f32; 4]; STEREO_REPROJ_CB_ROWS as usize]) -> Self {
        Self {
            eyes: std::array::from_fn(|eye| Cb0Rows {
                offset_view_projection: std::array::from_fn(|row| F32s::new(rows[eye * 5 + row])),
                camera_position: F32s::new(rows[eye * 5 + 4]),
            }),
            reprojection: std::array::from_fn(|eye| {
                std::array::from_fn(|row| F32s::new(rows[10 + eye * 4 + row]))
            }),
        }
    }
}

impl<const N: usize> F32s<N> {
    fn new(values: [f32; N]) -> Self {
        Self {
            bits: values.map(f32::to_bits),
            values,
        }
    }
}

const FIRST_PASS: i32 = RenderPassId::RP_Z_OCCLUDERS as i32;
const LAST_PASS: i32 = RenderPassId::RP_LAST_GBUFFER as i32;
const MAX_RECORDS: usize = 128;
const MAX_LIGHTING_RECORDS: usize = 2;
const _: () = assert!(STEREO_REPROJ_CB_ROWS == 18);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    frame: i64,
    generation: u64,
    mode: RenderMode,
}

struct State {
    identity: Option<Identity>,
    captured: CapturedConstants,
}

impl State {
    const fn new() -> Self {
        Self {
            identity: None,
            captured: CapturedConstants {
                records: Vec::new(),
                lighting_records: Vec::new(),
                unscoped_uploads: 0,
            },
        }
    }

    fn observe(&mut self, identity: Identity) {
        if self.identity != Some(identity) {
            self.identity = Some(identity);
            self.captured = CapturedConstants::default();
        }
    }

    fn geometry_pass(&mut self, pass: i32) -> Option<GeometryPass> {
        if pass == RenderPassId::RP_NONE as i32 {
            self.captured.unscoped_uploads = self.captured.unscoped_uploads.saturating_add(1);
        }
        GeometryPass::new(pass)
    }

    fn contains(&self, eye: PhysicalEye, pass: GeometryPass) -> bool {
        self.captured
            .records
            .iter()
            .any(|record| record.physical_eye == eye && record.pass == pass)
    }

    fn insert(&mut self, record: ConstantRecord) {
        if self.captured.records.len() < MAX_RECORDS
            && !self.contains(record.physical_eye, record.pass)
        {
            self.captured.records.push(record);
        }
    }

    fn contains_lighting(&self, eye: PhysicalEye) -> bool {
        self.captured
            .lighting_records
            .iter()
            .any(|record| record.physical_eye == eye)
    }

    fn insert_lighting(&mut self, record: LightingConstantRecord) {
        if self.captured.lighting_records.len() < MAX_LIGHTING_RECORDS
            && !self.contains_lighting(record.physical_eye)
        {
            self.captured.lighting_records.push(record);
        }
    }

    fn take(&mut self, identity: Identity) -> CapturedConstants {
        let previous = std::mem::replace(self, Self::new());
        if previous.identity == Some(identity) {
            previous.captured
        } else {
            CapturedConstants::default()
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

fn cb13_snapshot(
    uploaded: bool,
    live: impl FnOnce() -> Option<[[f32; 4]; STEREO_REPROJ_CB_ROWS as usize]>,
) -> (Option<Box<Cb13Rows>>, Cb13Status) {
    if !uploaded {
        return (None, Cb13Status::UploadUnavailable);
    }
    match live() {
        Some(rows) => (Some(Box::new(Cb13Rows::new(rows))), Cb13Status::Uploaded),
        None => (None, Cb13Status::LiveRowsUnavailable),
    }
}

fn global_rows(rows: &[Vector4]) -> Vec<F32s<4>> {
    rows.iter().map(|row| F32s::new(row.data)).collect()
}

fn observation() -> Option<(Identity, PhysicalEye)> {
    if !screenshot_wants_eyes() {
        return None;
    }
    let metadata = visual::metadata();
    if !metadata.ready {
        return None;
    }
    Some((
        Identity {
            frame: vr::capture_views()?.predicted_display_time_ns,
            generation: metadata.generation,
            mode: dispatch::mode(),
        },
        PhysicalEye::from_index(draw_index())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Identity {
        Identity {
            frame: 42,
            generation: 7,
            mode: RenderMode::Multipass,
        }
    }

    fn record(identity: Identity, eye: usize, pass: i32) -> ConstantRecord {
        ConstantRecord {
            frame: identity.frame,
            generation: identity.generation,
            mode: identity.mode,
            physical_eye: PhysicalEye::from_index(eye).unwrap(),
            pass: GeometryPass::new(pass).unwrap(),
            render_context: ContextInputs {
                view: F32s::new([0.0; 16]),
                projection: F32s::new([0.0; 16]),
                offset_view_projection: F32s::new([0.0; 16]),
                camera_position: F32s::new([0.0; 3]),
            },
            time_inputs: RenderTimeInputs {
                engine: EngineTimeInputs::from_values(
                    0.0,
                    0.0,
                    0.0,
                    0,
                    RenderFrameCountersInputs::new(0, 0, 0),
                ),
                context: ContextTimeInputs::from_values(0.0, 0.0, 0.0),
            },
            constants: ShaderConstants::MultipassCb0 {
                cb0: Cb0Rows {
                    offset_view_projection: std::array::from_fn(|_| F32s::new([0.0; 4])),
                    camera_position: F32s::new([0.0; 4]),
                },
            },
        }
    }

    fn lighting_record(identity: Identity, eye: usize) -> LightingConstantRecord {
        LightingConstantRecord {
            frame: identity.frame,
            generation: identity.generation,
            mode: identity.mode,
            physical_eye: PhysicalEye::from_index(eye).unwrap(),
            pass: DeferredLightsPass::new(RenderPassId::RP_DEFERRED_LIGHTS as i32).unwrap(),
            render_context: ContextInputs {
                view: F32s::new([0.0; 16]),
                projection: F32s::new([0.0; 16]),
                offset_view_projection: F32s::new([0.0; 16]),
                camera_position: F32s::new([0.0; 3]),
            },
            time_inputs: RenderTimeInputs {
                engine: EngineTimeInputs::from_values(
                    0.0,
                    0.0,
                    0.0,
                    0,
                    RenderFrameCountersInputs::new(0, 0, 0),
                ),
                context: ContextTimeInputs::from_values(0.0, 0.0, 0.0),
            },
            vertex_globals: Vec::new(),
            fragment_globals: Vec::new(),
        }
    }

    fn global_test_rows<const N: usize>(seed: u32) -> [Vector4; N] {
        std::array::from_fn(|row| Vector4 {
            data: std::array::from_fn(|col| {
                f32::from_bits(seed ^ ((row * 4 + col) as u32).wrapping_mul(0x9e37_79b9))
            }),
        })
    }

    #[test]
    fn pass_scope_comes_only_from_the_context() {
        let mut state = State::new();
        state.observe(identity());
        assert_eq!(state.geometry_pass(RenderPassId::RP_NONE as i32), None);
        assert_eq!(state.captured.unscoped_uploads, 1);
        for pass in [
            RenderPassId::PRE_RP_REFLECTION_MESH,
            RenderPassId::PRE_RP_SHADOW_0,
            RenderPassId::RP_POSTEFFECTS,
        ] {
            assert_eq!(state.geometry_pass(pass as i32), None);
        }
        assert_eq!(state.captured.unscoped_uploads, 1);
        assert!(state.geometry_pass(FIRST_PASS).is_some());
        assert!(state.geometry_pass(LAST_PASS - 1).is_some());
        assert_eq!(state.geometry_pass(LAST_PASS), None);
        assert!(state.captured.records.is_empty());
    }

    #[test]
    fn frame_generation_and_mode_rollover_clear_everything() {
        let original = identity();
        for changed in [
            Identity {
                frame: 43,
                ..original
            },
            Identity {
                generation: 8,
                ..original
            },
            Identity {
                mode: RenderMode::SinglePass,
                ..original
            },
        ] {
            let mut state = State::new();
            state.observe(original);
            state.insert(record(original, 0, FIRST_PASS));
            state.insert_lighting(lighting_record(original, 0));
            state.geometry_pass(RenderPassId::RP_NONE as i32);
            state.observe(changed);
            assert!(state.captured.records.is_empty());
            assert!(state.captured.lighting_records.is_empty());
            assert_eq!(state.captured.unscoped_uploads, 0);
            assert!(state.take(original).records.is_empty());
            assert!(state.take(original).lighting_records.is_empty());
        }
    }

    #[test]
    fn take_is_destructive_even_for_a_stale_frame() {
        let id = identity();
        let mut state = State::new();
        state.observe(id);
        state.insert(record(id, 0, FIRST_PASS));
        state.insert_lighting(lighting_record(id, 0));
        let captured = state.take(id);
        assert_eq!(captured.records.len(), 1);
        assert_eq!(captured.lighting_records.len(), 1);
        assert!(state.take(id).records.is_empty());
        assert!(state.take(id).lighting_records.is_empty());
        state.observe(id);
        state.insert(record(id, 0, FIRST_PASS));
        state.insert_lighting(lighting_record(id, 0));
        let captured = state.take(Identity { frame: 43, ..id });
        assert!(captured.records.is_empty());
        assert!(captured.lighting_records.is_empty());
        let captured = state.take(id);
        assert!(captured.records.is_empty());
        assert!(captured.lighting_records.is_empty());
    }

    #[test]
    fn only_the_deferred_lights_pass_is_accepted() {
        let deferred = DeferredLightsPass::new(RenderPassId::RP_DEFERRED_LIGHTS as i32).unwrap();
        assert_eq!(deferred.0, RenderPassId::RP_DEFERRED_LIGHTS as i32);
        for pass in [
            RenderPassId::RP_NONE,
            RenderPassId::RP_SSAO,
            RenderPassId::RP_SCREEN_SPACE_REFLECTIONS,
            RenderPassId::RP_GLOBAL_ILLUMINATION,
            RenderPassId::RP_SCREEN_SPACE_SUBSURFACE_SKIN,
            RenderPassId::RP_DEBUG_GI,
            RenderPassId::RP_LINES,
        ] {
            assert_eq!(DeferredLightsPass::new(pass as i32), None);
        }
        assert_eq!(DeferredLightsPass::new(FIRST_PASS), None);
        assert_eq!(DeferredLightsPass::new(LAST_PASS), None);
    }

    #[test]
    fn deferred_lighting_dedupes_per_physical_eye() {
        let id = identity();
        let mut state = State::new();
        state.observe(id);
        for eye in [1, 0, 1, 0, 0] {
            state.insert_lighting(lighting_record(id, eye));
        }
        assert_eq!(state.captured.lighting_records.len(), 2);
        assert_eq!(
            state.captured.lighting_records[0].physical_eye,
            PhysicalEye::Right
        );
        assert_eq!(
            state.captured.lighting_records[1].physical_eye,
            PhysicalEye::Left
        );
        assert!(state.captured.records.is_empty());
        assert_eq!(state.captured.unscoped_uploads, 0);
    }

    #[test]
    fn lighting_snapshots_preserve_every_global_row_and_bit() {
        let vertex = global_test_rows::<49>(0x7fc0_0000);
        let fragment = global_test_rows::<95>(0xff80_0000);
        let captured_vertex = global_rows(&vertex);
        let captured_fragment = global_rows(&fragment);
        assert_eq!(captured_vertex.len(), 49);
        assert_eq!(captured_fragment.len(), 95);
        for (captured, source) in captured_vertex.iter().zip(&vertex) {
            assert_eq!(captured.bits, source.data.map(f32::to_bits));
            assert_eq!(
                captured.values.map(f32::to_bits),
                source.data.map(f32::to_bits)
            );
        }
        for (captured, source) in captured_fragment.iter().zip(&fragment) {
            assert_eq!(captured.bits, source.data.map(f32::to_bits));
            assert_eq!(
                captured.values.map(f32::to_bits),
                source.data.map(f32::to_bits)
            );
        }
    }

    #[test]
    fn mono_right_preserves_the_physical_eye() {
        let id = Identity {
            mode: RenderMode::RightOnly,
            ..identity()
        };
        let mut state = State::new();
        state.observe(id);
        state.insert(record(id, 1, FIRST_PASS));
        let captured = state.take(id);
        assert_eq!(captured.records.len(), 1);
        assert_eq!(captured.records[0].physical_eye, PhysicalEye::Right);
        assert_eq!(PhysicalEye::from_index(2), None);
    }

    #[test]
    fn dedup_is_per_physical_eye_and_pass_and_bounded() {
        let id = identity();
        let mut state = State::new();
        state.observe(id);
        for _ in 0..3 {
            for pass in FIRST_PASS..LAST_PASS {
                for eye in [1, 0] {
                    state.insert(record(id, eye, pass));
                }
            }
        }
        let expected = (LAST_PASS - FIRST_PASS) as usize * 2;
        assert!(state.captured.records.len() <= MAX_RECORDS);
        assert_eq!(state.captured.records.len(), expected);
        state.observe(id);
        assert_eq!(state.captured.records.len(), expected);
    }

    #[test]
    fn failed_upload_never_reads_previous_live_rows() {
        let (rows, status) = cb13_snapshot(false, || panic!("stale rows must not be read"));
        assert!(rows.is_none());
        assert_eq!(status, Cb13Status::UploadUnavailable);
        let (rows, status) = cb13_snapshot(true, || None);
        assert!(rows.is_none());
        assert_eq!(status, Cb13Status::LiveRowsUnavailable);
        let (rows, status) = cb13_snapshot(true, || Some([[0.0; 4]; 18]));
        assert!(rows.is_some());
        assert_eq!(status, Cb13Status::Uploaded);
    }

    #[test]
    fn exact_bits_include_signed_zero_and_nan_payloads() {
        let bits = [0, 0x8000_0000, 0x7fc0_1234, 0x7f80_0000];
        let captured = F32s::new(bits.map(f32::from_bits));
        assert_eq!(captured.bits, bits);
        assert_eq!(captured.values.map(f32::to_bits), bits);
    }

    #[test]
    fn json_wire_preserves_signed_zero_and_exact_nonfinite_bits() {
        let bits = [0, 0x8000_0000, 0x7fc0_1234, 0x7f80_0000];
        let captured = F32s::new(bits.map(f32::from_bits));
        let json = serde_json::to_string(&captured).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        for (index, expected) in bits.into_iter().enumerate() {
            assert_eq!(parsed["bits"][index].as_u64(), Some(u64::from(expected)));
        }
        assert_eq!(parsed["values"][0].as_f64(), Some(0.0));
        assert_eq!(
            parsed["values"][1].as_f64().unwrap().to_bits(),
            (-0.0f64).to_bits()
        );
        assert!(parsed["values"][2].is_null());
        assert!(parsed["values"][3].is_null());
    }

    #[test]
    fn time_inputs_preserve_exact_bits() {
        let bits = [0, 0x8000_0000, 0x7fc0_1234, 0x7f80_0000];
        let counters = RenderFrameCountersInputs::new(0xdead_beef, 0x1234_5678, 2);
        let engine = EngineTimeInputs::from_values(
            f32::from_bits(bits[0]),
            f32::from_bits(bits[1]),
            f32::from_bits(bits[2]),
            0xdead_beef,
            counters,
        );
        let context = ContextTimeInputs::from_values(
            f32::from_bits(bits[3]),
            f32::from_bits(bits[2]),
            f32::from_bits(bits[1]),
        );
        assert_eq!(engine.time_of_day.bits, [bits[0]]);
        assert_eq!(engine.dt.bits, [bits[1]]);
        assert_eq!(engine.wind_animation.bits, [bits[2]]);
        assert_eq!(engine.acc_frame_time, 0xdead_beef);
        assert_eq!(engine.render_frame_counters, counters);
        assert_eq!(engine.render_frame_counters.counter, 0xdead_beef);
        assert_eq!(engine.render_frame_counters.frame_index, 0x1234_5678);
        assert_eq!(engine.render_frame_counters.ring_index, 2);
        assert_eq!(context.time_of_day.bits, [bits[3]]);
        assert_eq!(context.dt.bits, [bits[2]]);
        assert_eq!(context.wind_animation.bits, [bits[1]]);
    }

    #[test]
    fn engine_and_context_time_inputs_stay_distinct() {
        let engine = EngineTimeInputs::from_values(
            1.5,
            0.25,
            0.75,
            60,
            RenderFrameCountersInputs::new(7, 8, 9),
        );
        let context = ContextTimeInputs::from_values(2.5, 0.5, 0.125);
        assert_eq!(engine.time_of_day.values, [1.5]);
        assert_eq!(engine.dt.values, [0.25]);
        assert_eq!(engine.wind_animation.values, [0.75]);
        assert_eq!(
            engine.render_frame_counters,
            RenderFrameCountersInputs::new(7, 8, 9)
        );
        assert_eq!(context.time_of_day.values, [2.5]);
        assert_eq!(context.dt.values, [0.5]);
        assert_eq!(context.wind_animation.values, [0.125]);
    }

    #[test]
    fn live_cb13_copy_preserves_both_eye_slots_and_reprojection() {
        let rows = std::array::from_fn(|row| std::array::from_fn(|col| (row * 4 + col) as f32));
        let captured = Cb13Rows::new(rows);
        for eye in 0..2 {
            for row in 0..4 {
                assert_eq!(
                    captured.eyes[eye].offset_view_projection[row].bits,
                    rows[eye * 5 + row].map(f32::to_bits)
                );
                assert_eq!(
                    captured.reprojection[eye][row].bits,
                    rows[10 + eye * 4 + row].map(f32::to_bits)
                );
            }
            assert_eq!(
                captured.eyes[eye].camera_position.bits,
                rows[eye * 5 + 4].map(f32::to_bits)
            );
        }
    }
}
