//! The harness's side of the single-pass probe table: each probe the draw path calls, answered by the
//! stage, census, trace, water, and sun diagnostics that own it.

use std::ffi::c_void;

use jc3gi::graphics_engine::graphics_engine::RenderContext;
use jc3vr_render::stereo::single_pass::probes::{
    FsrPlane, ProbeGuard, RawDrawKind, SinglePassProbes, SunPassProbe, WaterGpuRun,
    register_single_pass_probes,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Resource;

use crate::screenshot::harness::{
    patch_draw, resolve_probe, snapshot,
    snapshot::Plane,
    stages,
    stages::{aa, dof, patch_owners, proxy, ssr, velocity},
    sun_probe, water_gpu_probe,
};

/// Register the harness's probes into the single-pass draw path.
pub fn register() {
    register_single_pass_probes(&HarnessProbes);
}

struct HarnessProbes;

impl SinglePassProbes for HarnessProbes {
    unsafe fn raw_entry(&self, kind: RawDrawKind, context: *mut c_void) {
        // SAFETY: per the trait's contract.
        unsafe { water_gpu_probe::raw_entry(kind, context) };
    }

    unsafe fn reflection_treatment(&self, context: *mut c_void) -> Option<ProbeGuard> {
        // SAFETY: per the trait's contract.
        unsafe { water_gpu_probe::reflection_treatment(context) }
            .map(|guard| Box::new(guard) as ProbeGuard)
    }

    fn submit(
        &self,
        context: *mut c_void,
        kind: &'static str,
        args: [i64; 5],
        draw: &mut dyn FnMut(),
    ) {
        velocity::submit(context, kind, args, draw);
    }

    fn submit_indirect(
        &self,
        context: *mut c_void,
        buffer: *mut c_void,
        offset: u32,
        draw: &mut dyn FnMut(),
    ) {
        velocity::submit_indirect(context, buffer, offset, draw);
    }

    unsafe fn aa_observe(&self, context: *mut c_void, vertex_count: u32, start_vertex: u32) {
        // SAFETY: per the trait's contract.
        unsafe { aa::observe(context, vertex_count, start_vertex) };
    }

    unsafe fn aa_with_color_edge(&self, context: *mut c_void, submit: &dyn Fn()) -> bool {
        // SAFETY: per the trait's contract.
        unsafe { aa::with_color_edge(context, submit) }
    }

    unsafe fn sun_depth_clip_treatment(&self, context: *mut c_void) -> Option<ProbeGuard> {
        // SAFETY: per the trait's contract.
        unsafe { sun_probe::depth_clip_treatment(context) }
            .map(|guard| Box::new(guard) as ProbeGuard)
    }

    unsafe fn sun_raw_draw(&self, context: *mut c_void, vertex_count: u32, start_vertex: u32) {
        // SAFETY: per the trait's contract.
        unsafe { sun_probe::on_raw_draw(context, vertex_count, start_vertex) };
    }

    unsafe fn sun_after_raw_draw(&self, context: *mut c_void) {
        // SAFETY: per the trait's contract.
        unsafe { sun_probe::after_raw_draw(context) };
    }

    fn sun_pass(&self, ctx: *const RenderContext) -> Option<Box<dyn SunPassProbe>> {
        sun_probe::sun_pass(ctx)
    }

    unsafe fn water_proxy_draw(&self, context: *mut c_void, submit: &dyn Fn()) -> bool {
        // SAFETY: per the trait's contract.
        unsafe { proxy::on_draw(context, &submit) }
    }

    unsafe fn dof_draw(&self, context: *mut c_void, submit: &dyn Fn()) -> bool {
        // SAFETY: per the trait's contract.
        unsafe { dof::on_draw(context, submit) }
    }

    unsafe fn fog_generation_draw(&self, context: *mut c_void, submit: &dyn Fn()) -> bool {
        // SAFETY: per the trait's contract.
        unsafe { stages::capture_fog_generation(context, submit) }
    }

    unsafe fn fog_inputs(&self, context: *mut c_void) {
        // SAFETY: per the trait's contract.
        unsafe { stages::capture_fog_inputs(context) };
    }

    unsafe fn resolve_draw(
        &self,
        context: *mut c_void,
        vertex_count: u32,
        start_vertex: u32,
    ) -> Option<ProbeGuard> {
        // SAFETY: per the trait's contract.
        unsafe { resolve_probe::on_draw(context, vertex_count, start_vertex) }
            .map(|guards| Box::new(guards) as ProbeGuard)
    }

    unsafe fn ssr_before_draw(
        &self,
        context: *mut c_void,
        vertex_count: u32,
        start_vertex: u32,
    ) -> Option<ProbeGuard> {
        // SAFETY: per the trait's contract.
        unsafe { ssr::before_draw(context, vertex_count, start_vertex) }
            .map(|guard| Box::new(guard) as ProbeGuard)
    }

    unsafe fn ssr_after_draw(&self, context: *mut c_void, vertex_count: u32, start_vertex: u32) {
        // SAFETY: per the trait's contract.
        unsafe { ssr::after_draw(context, vertex_count, start_vertex) };
    }

    fn patch_draw_select(&self) -> Option<bool> {
        patch_draw::select()
    }

    unsafe fn patch_draw_observe(
        &self,
        context: *mut c_void,
        offset: u32,
        expected: Option<usize>,
        secondary: bool,
    ) -> Option<ProbeGuard> {
        // SAFETY: per the trait's contract.
        unsafe { patch_draw::observe(context, offset, expected, secondary) }
            .map(|guard| Box::new(guard) as ProbeGuard)
    }

    fn patch_camera_experiment(&self) -> bool {
        patch_draw::camera_experiment()
    }

    fn patch_owners_before(&self, offset: u32) -> bool {
        patch_owners::before(offset)
    }

    fn patch_owners_after(&self, offset: u32) {
        patch_owners::after(offset);
    }

    fn capture_patch_submission(&self, after: bool) {
        stages::capture_patch_submission(after);
    }

    fn fsr_plane(&self, plane: FsrPlane, eye: usize, resource: &ID3D11Resource) {
        let plane = match plane {
            FsrPlane::Motion => Plane::FsrMotion,
            FsrPlane::Depth => Plane::FsrDepth,
        };
        snapshot::eye_plane_hook(plane, eye, resource);
    }

    fn water_gpu_probe(&self, pass: i32, eye: usize) -> Option<Box<dyn WaterGpuRun>> {
        water_gpu_probe::WaterGpuProbe::begin(pass, eye)
            .map(|probe| Box::new(probe) as Box<dyn WaterGpuRun>)
    }

    fn water_input_capture(&self, eye: usize) -> ProbeGuard {
        Box::new(water_gpu_probe::begin_water_input_capture(eye))
    }

    fn reflection_producer(&self, pass: i32) -> Option<ProbeGuard> {
        water_gpu_probe::reflection_producer(pass).map(|probe| Box::new(probe) as ProbeGuard)
    }
}

impl WaterGpuRun for water_gpu_probe::WaterGpuProbe {
    fn finish(self: Box<Self>) {
        water_gpu_probe::WaterGpuProbe::finish(*self);
    }
}
