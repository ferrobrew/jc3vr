//! Each eye's camera position for the pixel shaders, selected per pixel under the collapse.
//!
//! The engine stages the camera world position into the fragment `GlobalConstants` once per pass,
//! from the centred render camera, and pixel shaders measure their view vectors (specular, Fresnel,
//! cloth sheen, environment reflection), camera distances, and the sun-shadow cascade lookup from it.
//! An instance-doubled draw covers both eye halves of the double-wide target in one submission, so
//! the staging cannot be moved per eye the way it is for a split fullscreen pass
//! ([`eye_globals`]), and every material shaded that way is
//! lit as seen from the point between the eyes.
//!
//! [`dxbc_stereo::localize_eye_camera`] rewrites every pixel shader that reads those rows to read
//! them from a mod-owned buffer instead, picking the eye by which side of a boundary its pixel lies
//! on. This module owns that buffer. It is rewritten every time the engine uploads its global
//! constants, from the rows the engine just staged:
//!
//! - inside the routed range under the collapse, each eye's slot holds the staged rows moved to that
//!   eye (the same rows `eye_globals` stages for a split pass), and the boundary is the first pixel
//!   of the right eye's half of whatever target is bound, following the target as the engine rebinds
//!   its viewport;
//! - everywhere else (the shadow, reflection, and environment passes, and any frame without the
//!   collapse), both slots hold the staged rows unchanged and the boundary is past every pixel, so a
//!   rewritten shader reads exactly what it reads from `cb0`.
//!
//! Each eye's slot also carries the eye's world offset from the centred camera, which the rewrite
//! subtracts from the light positions of the shaders that light from the clustered-lighting frame
//! constants (`single_pass.local_lights_per_eye`): the engine builds those relative to the centred
//! render camera, so once a pixel's position is measured from its eye the lights have to be too.
//! Outside an eye-split pass the offset is zero, so the lights read what the engine built.
//!
//! [`dxbc_stereo::localize_screen_door`] reads the same boundary to lay the screen-door pattern the
//! alpha-tested and fading materials discard by out from each eye's origin
//! (`single_pass.screen_door_per_eye`). The boundary is therefore published in every eye-split pass,
//! with both slots holding the staged rows when the camera rows are not moved per eye.
//!
//! [`dxbc_stereo::localize_velocity`] reads the same boundary, and two rows after it, to write the
//! velocity pass's motion vectors in each eye's screen space: in an eye-split pass the rows map a
//! pixel position to the clip space of the eye whose half it lies in, and carry each eye's map from
//! the render camera's previous clip space to its own (the `M_eye` the `cb13` reprojection reads,
//! corrected for the camera's previous projection), which the previous position is taken through;
//! everywhere else they are zero, which leaves the engine's vectors as they are.
//!
//! The split fullscreen passes and the water's per-eye draws need nothing extra: each of their pixels
//! lies in the half of the eye whose rows `eye_globals` stages for it, and that eye's slot holds the
//! same values. Nothing is re-issued; the buffer is a constant-buffer write per pass and per target
//! change.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use dxbc_stereo::{
    EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS,
    EYE_CAMERA_LIGHT_OFFSET_ROW, EYE_CAMERA_ROWS_PER_EYE, EYE_CAMERA_SOURCE_ROWS, LocalLights,
    VELOCITY_ORIGIN_ROW, VELOCITY_PIXEL_ROW, VELOCITY_REPROJECTION_ROW,
    VELOCITY_REPROJECTION_ROWS_PER_EYE, localize_eye_camera, localize_projected_screen_door,
    localize_screen_door, localize_velocity,
};
use jc3gi::{
    graphics_engine::{graphics_engine::GraphicsEngine, render_engine::RenderEngine},
    types::math::Vector4,
};
use jc3vr_view::{
    stereo::{config::SinglePassSwitch, engine_context::EngineContext, layout::collapse_active},
    vr::render_params,
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext};

use crate::stereo::single_pass::{
    CURRENT_M_EYE, collapse_full_viewport, config_flags, eye_globals, in_gbuffer_range,
    per_eye_cb::PerEyeCb, viewport::eye_half_viewport,
};

/// A pixel shader's bytecode rewritten to read each eye's camera rows, and with
/// [`LocalLights::PerEye`] to measure its local lights from the eye, or `None` when `enabled` is off
/// or the shader reads none of the camera rows. Called for every fragment program the engine creates.
pub(crate) fn rewrite_fragment_program(
    code: &[u8],
    enabled: bool,
    local_lights: LocalLights,
) -> Option<Vec<u8>> {
    if !enabled {
        return None;
    }
    match localize_eye_camera(code, local_lights) {
        Ok(rewritten) => {
            REWRITTEN.fetch_add(1, Ordering::Relaxed);
            Some(rewritten)
        }
        Err(dxbc_stereo::DxbcError::NoCameraPosition) => None,
        Err(error) => {
            DECLINED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                target: "single_pass",
                "eye camera: a pixel shader reading the camera position was left unrewritten: {error}",
            );
            None
        }
    }
}

/// A pixel shader's bytecode rewritten to lay its screen-door pattern out from each eye's origin, or
/// `None` when `enabled` is off or the shader builds no such pattern. Called for every fragment
/// program the engine creates, after [`rewrite_fragment_program`], whose output it takes when there
/// is one.
pub(crate) fn rewrite_screen_door(code: &[u8], enabled: bool) -> Option<Vec<u8>> {
    if !enabled {
        return None;
    }
    match localize_screen_door(code) {
        Ok(rewritten) => {
            SCREEN_DOORS.fetch_add(1, Ordering::Relaxed);
            Some(rewritten)
        }
        Err(dxbc_stereo::DxbcError::NoScreenDoor) => None,
        Err(error) => {
            SCREEN_DOORS_DECLINED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                target: "single_pass",
                "screen door: a pixel shader hashing its pixel position was left unrewritten: {error}",
            );
            None
        }
    }
}

/// A pixel shader's bytecode rewritten to hash the screen door it builds from its own projected
/// position (the tree impostors') from the eye-local pixel position, or `None` when `enabled` is off
/// or the shader builds no such pattern. Called for every fragment program the engine creates, after
/// [`rewrite_screen_door`], whose output it takes when there is one.
pub(crate) fn rewrite_projected_screen_door(code: &[u8], enabled: bool) -> Option<Vec<u8>> {
    if !enabled {
        return None;
    }
    match localize_projected_screen_door(code) {
        Ok(rewritten) => {
            PROJECTED_SCREEN_DOORS.fetch_add(1, Ordering::Relaxed);
            Some(rewritten)
        }
        Err(dxbc_stereo::DxbcError::NoScreenDoor) => None,
        Err(error) => {
            SCREEN_DOORS_DECLINED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                target: "single_pass",
                "screen door: a pixel shader hashing its projected position was left unrewritten: {error}",
            );
            None
        }
    }
}

/// A pixel shader's bytecode rewritten to write its motion vector in the screen space of the eye whose
/// half the pixel lies in, or `None` when `enabled` is off or the shader writes no motion vector.
/// Called for every fragment program the engine creates, after the camera and screen-door rewrites,
/// whose output it takes when there is one.
pub(crate) fn rewrite_velocity(code: &[u8], enabled: bool) -> Option<Vec<u8>> {
    if !enabled {
        return None;
    }
    match localize_velocity(code) {
        Ok(rewritten) => {
            let count = VELOCITIES.fetch_add(1, Ordering::Relaxed) + 1;
            if count == 1 {
                tracing::info!(target: "single_pass", "velocity: velocity pixel shaders rewritten");
            }
            Some(rewritten)
        }
        Err(dxbc_stereo::DxbcError::NoVelocity) => None,
        Err(error) => {
            VELOCITIES_DECLINED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                target: "single_pass",
                "velocity: a pixel shader writing motion vectors was left unrewritten: {error}",
            );
            None
        }
    }
}

/// How many velocity pixel shaders have been rewritten since injection, and how many were declined.
pub fn velocity_counts() -> (usize, usize) {
    (
        VELOCITIES.load(Ordering::Relaxed),
        VELOCITIES_DECLINED.load(Ordering::Relaxed),
    )
}

/// Each eye's map from the render camera's previous clip space to its own, as the last eye-split
/// pass published it to the velocity rewrite: what FSR's velocity decode needs to know which
/// previous camera the motion vectors were encoded against.
pub(crate) fn velocity_maps() -> Option<[glam::Mat4; 2]> {
    *LAST_VELOCITY_MAPS.lock()
}

/// How many pixel shaders have been rewritten since injection, and how many that read the camera
/// position were declined.
pub fn counts() -> (usize, usize) {
    (
        REWRITTEN.load(Ordering::Relaxed),
        DECLINED.load(Ordering::Relaxed),
    )
}

/// How many pixel shaders have had their screen-door pattern laid out per eye since injection (from
/// the pixel position, and from their projected position), and how many that build one were
/// declined.
pub fn screen_door_counts() -> (usize, usize, usize) {
    (
        SCREEN_DOORS.load(Ordering::Relaxed),
        PROJECTED_SCREEN_DOORS.load(Ordering::Relaxed),
        SCREEN_DOORS_DECLINED.load(Ordering::Relaxed),
    )
}

/// Rewrite the buffer from the rows the engine just staged and bind it. Called from the
/// `SetAllGlobalShaderProgramConstants` detour after the engine's own upload, on the render thread.
pub(crate) fn sync(engine: &RenderEngine) {
    if REWRITTEN.load(Ordering::Relaxed) == 0
        && SCREEN_DOORS.load(Ordering::Relaxed) == 0
        && PROJECTED_SCREEN_DOORS.load(Ordering::Relaxed) == 0
        && VELOCITIES.load(Ordering::Relaxed) == 0
    {
        return;
    }
    let staged = &engine.m_FPGlobalConstData;
    let mirrored = block(EYE_CAMERA_SOURCE_ROWS.map(|row| staged[row as usize]), ZERO);
    let flags = config_flags();
    let pixel_camera = flags.has(SinglePassSwitch::PixelCameraPerEye);
    let local_lights = flags.has(SinglePassSwitch::LocalLightsPerEye);
    let eye_block = |eye: usize| -> Option<[Vector4; EYE_ROWS]> {
        let offset = if local_lights {
            eye_globals::eye_light_offset(eye)?
        } else {
            ZERO
        };
        Some(block(eye_globals::eye_rows(staged, eye)?, offset))
    };
    let split = collapse_active() && in_gbuffer_range();
    let per_eye = (split && pixel_camera)
        .then(|| Some([eye_block(0)?, eye_block(1)?]))
        .flatten()
        .filter(|eyes| {
            eyes.iter()
                .flatten()
                .all(|row| row.data.iter().all(|v| v.is_finite()))
        });
    with_state(|ctx, state| {
        state.mirrored = mirrored;
        state.split = split;
        state.per_eye = per_eye;
        state.velocity = (*CURRENT_M_EYE.lock()).and_then(previous_reprojection);
        if split && state.velocity.is_some() {
            *LAST_VELOCITY_MAPS.lock() = state.velocity;
        }
        if split && state.velocity.is_none() && !VELOCITY_UNPUBLISHED.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                target: "single_pass",
                "velocity: an eye-split pass staged its constants without the eyes' reprojection \
                 matrices, so its motion vectors keep the engine's double-wide encoding",
            );
        }
        let rows = state.rows(collapse_full_viewport());
        upload(ctx, state, &rows, true);
    });
}

/// Each eye's map from the render camera's previous clip space to the clip space an eye-sized render
/// of that eye reprojects through, from the eyes' `M_eye`.
///
/// An eye rendered on its own reprojects through its own previous camera position, the centred
/// camera's previous orientation (the engine's previous view carries none of the per-eye display
/// cant the camera hook applies after the fact), and the render camera's previous projection, which
/// the engine builds for an eye-sized target. Under the collapse that projection is the double-wide
/// one, whose horizontal row is the eye-sized one's halved, and the previous view is the centred
/// camera's. `M_eye = P_eye E⁻¹ P⁻¹` takes the camera's current clip space to the eye's, with `E` the
/// eye's pose in the camera's frame and `P` the camera's current projection, so
/// `E⁻¹ = P_eye⁻¹ M_eye P`; of that pose only the eye's offset is wanted ([`eye_offset_view`]), and
/// the map is `S P_prev T⁻¹ P_prev⁻¹`, with `T` the eye's offset in the camera's frame and `S`
/// doubling the horizontal clip coordinate. The eyes are rigid with respect to the camera, so the
/// previous frame's eye cameras relate to the previous render camera as this frame's do to the
/// current one. `None` while the render camera or an eye's parameters are unavailable, or a
/// projection is singular.
fn previous_reprojection(m_eye: [glam::Mat4; 2]) -> Option<[glam::Mat4; 2]> {
    // SAFETY: called on the render thread from the global-constant upload, where the engine's render
    // camera is live and not being written.
    let camera = unsafe { &GraphicsEngine::get()?.m_RenderCamera };
    let current = glam::Mat4::from(camera.m_ProjectionF).as_dmat4();
    let previous = glam::Mat4::from(camera.m_PreviousProjF).as_dmat4();
    let invertible = |m: &glam::DMat4| m.determinant().abs() > f64::EPSILON;
    if !invertible(&previous) {
        return None;
    }
    let eye_sized =
        glam::DMat4::from_scale(glam::DVec3::new(PREVIOUS_X_SCALE, 1.0, 1.0)) * previous;
    let previous_inverse = previous.inverse();
    let mut maps = [glam::Mat4::IDENTITY; 2];
    for (eye, map) in maps.iter_mut().enumerate() {
        let eye_projection = glam::Mat4::from(render_params(eye)?.projection_reverse_z).as_dmat4();
        if !invertible(&eye_projection) {
            return None;
        }
        let pose_inverse = eye_projection.inverse() * m_eye[eye].as_dmat4() * current;
        *map = (eye_sized * eye_offset_view(pose_inverse) * previous_inverse).as_mat4();
    }
    Some(maps)
}

/// The translation part of an eye's view from the camera's (`pose_inverse`, a rigid map from the
/// camera's view space to the eye's): the camera's view moved to the eye's position, keeping the
/// camera's orientation. `pose_inverse` maps `x` to `R x + t`, which puts the eye at `-Rᵀ t` in the
/// camera's frame, so the offset view maps `x` to `x + Rᵀ t`. On parallel panels `R` is the identity
/// and this is `pose_inverse` itself.
fn eye_offset_view(pose_inverse: glam::DMat4) -> glam::DMat4 {
    let rotation = glam::DMat3::from_mat4(pose_inverse);
    glam::DMat4::from_translation(rotation.transpose() * pose_inverse.w_axis.truncate())
}

/// What an eye-sized render's previous projection scales the double-wide one's horizontal clip
/// coordinate by.
const PREVIOUS_X_SCALE: f64 = 2.0;

/// Follow the bound target: called when the collapse records a new engine viewport, so the boundary
/// stays on the seam of the target the next draws render into.
///
/// Runs on every viewport bind under the collapse, so it returns before taking any lock unless the
/// pass is eye-split and the boundary moved.
pub(super) fn viewport_changed() {
    if !SPLIT.load(Ordering::Relaxed) {
        return;
    }
    let full = collapse_full_viewport();
    if full.map(layout_key) == Some(UPLOADED_LAYOUT.load(Ordering::Relaxed)) {
        return;
    }
    with_state(|ctx, state| {
        if !state.split || state.buffer.buffer().is_none() {
            return;
        }
        let rows = state.rows(full);
        upload(ctx, state, &rows, false);
    });
}

/// Put both slots back to the staged rows when the routed range ends, so a draw between the range and
/// the engine's next upload reads what `cb0` holds.
pub(super) fn range_ended() {
    if !SPLIT.load(Ordering::Relaxed) {
        return;
    }
    with_state(|ctx, state| {
        state.per_eye = None;
        if !std::mem::take(&mut state.split) || state.buffer.buffer().is_none() {
            return;
        }
        let rows = state.rows(None);
        upload(ctx, state, &rows, false);
    });
}

/// Unbind and release the buffer, so it does not outlive an eject.
pub(super) fn release() {
    if let Some(d3d) = EngineContext::get() {
        // SAFETY: on the eject path with the engine live, under the engine's context mutex.
        d3d.with_lock(|ctx| unsafe { Rows::unbind_pixel(ctx) });
    }
    let mut state = STATE.lock();
    state.buffer.release();
    state.split = false;
    state.per_eye = None;
    SPLIT.store(false, Ordering::Relaxed);
    REWRITTEN.store(0, Ordering::Relaxed);
    DECLINED.store(0, Ordering::Relaxed);
    SCREEN_DOORS.store(0, Ordering::Relaxed);
    SCREEN_DOORS_DECLINED.store(0, Ordering::Relaxed);
}

/// The buffer's size in rows.
const ROWS: usize = EYE_CAMERA_CB_ROWS as usize;
/// Each eye's block of rows.
const EYE_ROWS: usize = EYE_CAMERA_ROWS_PER_EYE as usize;
/// The row holding the boundary.
const BOUNDARY: usize = EYE_CAMERA_BOUNDARY_ROW as usize;

/// The rows the velocity rewrite reads.
const PIXEL: usize = VELOCITY_PIXEL_ROW as usize;
const ORIGIN: usize = VELOCITY_ORIGIN_ROW as usize;
const REPROJECTION: usize = VELOCITY_REPROJECTION_ROW as usize;
const REPROJECTION_ROWS: usize = VELOCITY_REPROJECTION_ROWS_PER_EYE as usize;
/// The rows [`velocity_rows`] lays out from [`PIXEL`] on.
const VELOCITY_ROWS: usize = REPROJECTION + 2 * REPROJECTION_ROWS - PIXEL;

const _: () =
    assert!(ORIGIN == PIXEL + 1 && REPROJECTION == ORIGIN + 1 && PIXEL + VELOCITY_ROWS == ROWS);

/// The place of the light offset in each eye's block.
const LIGHT_OFFSET: usize = EYE_CAMERA_LIGHT_OFFSET_ROW as usize;

const _: () = assert!(LIGHT_OFFSET == eye_globals::EYE_ROW_COUNT && EYE_ROWS == LIGHT_OFFSET + 1);

const ZERO: Vector4 = Vector4 { data: [0.0; 4] };

/// One eye's block: the copies of the camera rows, then the light offset.
fn block(
    rows: [Vector4; eye_globals::EYE_ROW_COUNT],
    light_offset: Vector4,
) -> [Vector4; EYE_ROWS] {
    let mut block = [ZERO; EYE_ROWS];
    block[..LIGHT_OFFSET].copy_from_slice(&rows);
    block[LIGHT_OFFSET] = light_offset;
    block
}

/// A boundary no pixel reaches: every pixel reads the left eye's slot.
const NO_BOUNDARY: f32 = f32::MAX;

static REWRITTEN: AtomicUsize = AtomicUsize::new(0);
static DECLINED: AtomicUsize = AtomicUsize::new(0);
static SCREEN_DOORS: AtomicUsize = AtomicUsize::new(0);
static SCREEN_DOORS_DECLINED: AtomicUsize = AtomicUsize::new(0);
static PROJECTED_SCREEN_DOORS: AtomicUsize = AtomicUsize::new(0);
static VELOCITIES: AtomicUsize = AtomicUsize::new(0);
static VELOCITIES_DECLINED: AtomicUsize = AtomicUsize::new(0);
static VELOCITY_UNPUBLISHED: AtomicBool = AtomicBool::new(false);
static LAST_VELOCITY_MAPS: Mutex<Option<[glam::Mat4; 2]>> = Mutex::new(None);
static UPLOAD_WARNED: AtomicBool = AtomicBool::new(false);
/// Whether the live buffer was written for an eye-split pass, and the [`layout_key`] of the viewport
/// it was laid out for, so the per-bind and per-range paths can return without locking.
static SPLIT: AtomicBool = AtomicBool::new(false);
static UPLOADED_LAYOUT: AtomicU64 = AtomicU64::new(0);

/// The mod-owned buffer the rewritten pixel shaders read.
type Rows = PerEyeCb<ROWS, EYE_CAMERA_CB_REGISTER>;

static STATE: Mutex<State> = Mutex::new(State {
    buffer: Rows::new(),
    mirrored: [Vector4 { data: [0.0; 4] }; EYE_ROWS],
    split: false,
    per_eye: None,
    velocity: None,
    uploaded: [Vector4 { data: [0.0; 4] }; ROWS],
});

struct State {
    buffer: Rows,
    /// The rows the engine staged at its last upload, with a zero light offset.
    mirrored: [Vector4; EYE_ROWS],
    /// Whether the current pass is an eye-split one, which publishes the eye boundary.
    split: bool,
    /// Each eye's rows, while the current pass is an eye-split one and the camera rows move per eye.
    per_eye: Option<[[Vector4; EYE_ROWS]; 2]>,
    /// Each eye's map from the render camera's previous clip space to its own, while an eye-split
    /// pass publishes the velocity rows.
    velocity: Option<[glam::Mat4; 2]>,
    /// What the live buffer holds.
    uploaded: [Vector4; ROWS],
}

impl State {
    /// The buffer contents for the current pass, split at the seam of the full viewport `full` while
    /// the pass is eye-split. Without an eye-split pass or a viewport, both slots hold the staged
    /// rows, no pixel reaches the boundary, and the velocity rows are zero; in an eye-split pass
    /// without per-eye rows, both slots hold the staged rows and the boundary is published for the
    /// screen-door rewrite.
    fn rows(&self, full: Option<D3D11_VIEWPORT>) -> [Vector4; ROWS] {
        match (self.split, full) {
            (true, Some(full)) => compose(
                self.per_eye.unwrap_or([self.mirrored; 2]),
                eye_half_viewport(full, 1).TopLeftX,
                self.velocity
                    .and_then(|reprojection| velocity_rows(full, reprojection)),
            ),
            _ => compose([self.mirrored; 2], NO_BOUNDARY, None),
        }
    }
}

/// Lay out both eyes' blocks, the boundary, and the velocity rows in the order the rewritten shaders
/// read them.
fn compose(
    eyes: [[Vector4; EYE_ROWS]; 2],
    boundary: f32,
    velocity: Option<[Vector4; VELOCITY_ROWS]>,
) -> [Vector4; ROWS] {
    let mut rows = [Vector4 { data: [0.0; 4] }; ROWS];
    for (eye, block) in eyes.iter().enumerate() {
        rows[eye * EYE_ROWS..(eye + 1) * EYE_ROWS].copy_from_slice(block);
    }
    rows[BOUNDARY].data[0] = boundary;
    if let Some(velocity) = velocity {
        rows[PIXEL..].copy_from_slice(&velocity);
    }
    rows
}

/// The velocity rewrite's rows for the eye halves of `full` and each eye's map from the render
/// camera's previous clip space to its own, or `None` for a degenerate viewport or a map that is not
/// finite.
///
/// The pixel row maps a pixel position to clip space in its eye's half, `x` from the eye's left edge
/// over the eye's width and `y` from the target's top edge over its height, and its `.w` tells the
/// shader to take the corrected positions. The origin row holds each eye's left edge in clip space.
/// The matrices follow, one row of each per row: each eye's map from the render camera's previous
/// clip space to its own ([`previous_reprojection`]).
fn velocity_rows(
    full: D3D11_VIEWPORT,
    reprojection: [glam::Mat4; 2],
) -> Option<[Vector4; VELOCITY_ROWS]> {
    let eyes = [eye_half_viewport(full, 0), eye_half_viewport(full, 1)];
    let (width, height) = (eyes[0].Width, full.Height);
    if !(width > 0.0 && height > 0.0) || !reprojection.iter().all(|m| m.is_finite()) {
        return None;
    }
    let left = |eye: usize| -1.0 - 2.0 * eyes[eye].TopLeftX / width;
    let mut rows = [ZERO; VELOCITY_ROWS];
    rows[0] = Vector4 {
        data: [
            2.0 / width,
            -2.0 / height,
            1.0 + 2.0 * full.TopLeftY / height,
            1.0,
        ],
    };
    rows[1] = Vector4 {
        data: [left(0), left(1), 0.0, 0.0],
    };
    for (eye, matrix) in reprojection.iter().enumerate() {
        for row in 0..REPROJECTION_ROWS {
            rows[REPROJECTION - PIXEL + eye * REPROJECTION_ROWS + row] = Vector4 {
                data: matrix.row(row).to_array(),
            };
        }
    }
    Some(rows)
}

/// The viewport fields the boundary and the velocity rows are built from, packed into one word so a
/// viewport bind can tell without a lock whether they moved.
fn layout_key(full: D3D11_VIEWPORT) -> u64 {
    let boundary = eye_half_viewport(full, 1).TopLeftX.to_bits();
    let vertical = full.Height.to_bits() ^ full.TopLeftY.to_bits().rotate_left(16);
    u64::from(boundary) | (u64::from(vertical) << 32)
}

/// Run `f` on the immediate context under the engine's context mutex, then the buffer state, the lock
/// order every other constant-buffer write in the mod takes. A no-op before the engine's context is
/// live.
fn with_state(f: impl FnOnce(&ID3D11DeviceContext, &mut State)) {
    let Some(d3d) = EngineContext::get() else {
        return;
    };
    d3d.with_lock(|ctx| f(ctx, &mut STATE.lock()));
}

/// Write `rows` into the buffer, creating it on first use, and bind it on the pixel stage when `bind`
/// is set. A failure leaves the previous contents and warns once.
fn upload(ctx: &ID3D11DeviceContext, state: &mut State, rows: &[Vector4; ROWS], bind: bool) {
    // SAFETY: on the render thread under the engine's context mutex, as every other write to the
    // immediate context.
    let written = unsafe {
        state.buffer.upload(ctx, rows).map(|buffer| {
            if bind {
                Rows::bind_pixel(ctx, buffer);
            }
        })
    };
    match written {
        Ok(()) => {
            state.uploaded = *rows;
            SPLIT.store(state.split, Ordering::Relaxed);
            UPLOADED_LAYOUT.store(
                collapse_full_viewport().map_or(0, layout_key),
                Ordering::Relaxed,
            );
        }
        Err(error) => {
            if !UPLOAD_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "single_pass",
                    "eye camera: could not write the per-eye camera rows ({error}); pixel shaders \
                     keep the previous rows",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(value: f32) -> Vector4 {
        Vector4 {
            data: [value, value + 1.0, value + 2.0, 1.0],
        }
    }

    fn state(per_eye: bool) -> State {
        State {
            buffer: Rows::new(),
            mirrored: [row(10.0), row(10.0), row(40.0), ZERO],
            split: per_eye,
            per_eye: per_eye.then(|| {
                [
                    [row(9.0), row(9.0), row(39.0), row(-1.0)],
                    [row(11.0), row(11.0), row(41.0), row(1.0)],
                ]
            }),
            velocity: None,
            uploaded: [Vector4 { data: [0.0; 4] }; ROWS],
        }
    }

    /// A full viewport `width` pixels wide over a target 360 high.
    fn viewport(width: f32) -> D3D11_VIEWPORT {
        D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: width,
            Height: 360.0,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        }
    }

    /// What a rewritten shader reads for a source row at pixel `x`: the eye is the side of the
    /// boundary, and the row is that eye's copy.
    fn read(rows: &[Vector4; ROWS], x: f32, source: usize) -> Vector4 {
        let eye = usize::from(x >= rows[BOUNDARY].data[0]);
        rows[eye * EYE_ROWS + source]
    }

    /// Outside an eye-split pass every pixel, at any position, reads the rows the engine staged.
    #[test]
    fn a_pass_that_is_not_eye_split_reads_the_staged_rows() {
        let state = state(false);
        for full in [None, Some(viewport(640.0))] {
            let rows = state.rows(full);
            for x in [0.5, 319.5, 320.5, 639.5, 1e9] {
                for (source, expected) in state.mirrored.iter().enumerate() {
                    assert_eq!(read(&rows, x, source).data, expected.data);
                }
            }
        }
    }

    /// Inside an eye-split pass each half reads its own eye's rows, whatever the target's width.
    #[test]
    fn each_half_of_an_eye_split_pass_reads_its_eye() {
        let state = state(true);
        let eyes = state.per_eye.expect("per-eye rows");
        for width in [640.0f32, 320.0, 1234.0] {
            let rows = state.rows(Some(viewport(width)));
            for (x, eye) in [(0.5, 0), (width / 2.0 - 0.5, 0), (width / 2.0 + 0.5, 1)] {
                for (source, expected) in eyes[eye].iter().enumerate() {
                    assert_eq!(read(&rows, x, source).data, expected.data);
                }
            }
        }
    }

    /// An eye-split pass whose camera rows stay on the centre still publishes the boundary, for the
    /// screen-door rewrite, and every pixel on either side of it reads the staged rows.
    #[test]
    fn an_eye_split_pass_without_per_eye_rows_publishes_the_boundary() {
        let mut state = state(false);
        state.split = true;
        let rows = state.rows(Some(viewport(640.0)));
        assert_eq!(rows[BOUNDARY].data[0], 320.0);
        for x in [0.5, 319.5, 320.5, 639.5] {
            for (source, expected) in state.mirrored.iter().enumerate() {
                assert_eq!(read(&rows, x, source).data, expected.data);
            }
        }
    }

    /// The eye-local position the screen-door rewrite builds from the published boundary: the same
    /// pixel of each eye's half gets the same position, and outside an eye-split pass every
    /// position is the pixel's own.
    #[test]
    fn the_published_boundary_makes_each_half_eye_local() {
        let local = |rows: &[Vector4; ROWS], x: f32| {
            let boundary = rows[BOUNDARY].data[0];
            x - if x >= boundary { boundary } else { 0.0 }
        };
        let split = state(true).rows(Some(viewport(640.0)));
        for x in [0.5, 100.5, 319.5] {
            assert_eq!(local(&split, x), x);
            assert_eq!(local(&split, x + 320.0), x);
        }
        let unsplit = state(false).rows(Some(viewport(640.0)));
        for x in [0.5, 319.5, 320.5, 639.5] {
            assert_eq!(local(&unsplit, x), x);
        }
    }

    /// Without a recorded viewport there is no seam to split at, so the pass reads the staged rows.
    #[test]
    fn an_eye_split_pass_without_a_boundary_reads_the_staged_rows() {
        let state = state(true);
        let rows = state.rows(None);
        assert_eq!(rows[BOUNDARY].data[0], NO_BOUNDARY);
        for (source, expected) in state.mirrored.iter().enumerate() {
            assert_eq!(read(&rows, 639.5, source).data, expected.data);
        }
    }

    /// The velocity rewrite's correction, as the rewritten shader computes it from the buffer: the
    /// previous and current NDC it subtracts, given the pixel position, the previous clip position
    /// interpolated for it, and the NDC the vertex shader handed down.
    fn corrected(
        rows: &[Vector4; ROWS],
        position: [f32; 2],
        previous: glam::Vec4,
        current: [f32; 2],
    ) -> ([f32; 2], [f32; 2]) {
        let (pixel, origin) = (rows[PIXEL].data, rows[ORIGIN].data);
        let eye = usize::from(position[0] >= rows[BOUNDARY].data[0]);
        let own = [
            position[0] * pixel[0] + origin[eye],
            position[1] * pixel[1] + pixel[2],
        ];
        let row =
            |r: usize| glam::Vec4::from(rows[REPROJECTION + eye * REPROJECTION_ROWS + r].data);
        let w = row(3).dot(previous);
        let taken = [row(0).dot(previous) / w, row(1).dot(previous) / w];
        if pixel[3] != 0.0 {
            (taken, own)
        } else {
            ([previous.x / previous.w, previous.y / previous.w], current)
        }
    }

    /// Each eye's map from the centred camera's clip space: a horizontal shift by the eye's offset.
    fn reprojection() -> [glam::Mat4; 2] {
        [-0.03f32, 0.03].map(|shift| {
            glam::Mat4::from_cols(
                glam::Vec4::X,
                glam::Vec4::Y,
                glam::Vec4::Z,
                glam::Vec4::new(shift, 0.0, 0.0, 1.0),
            )
        })
    }

    fn velocity_state(per_eye: bool) -> State {
        State {
            velocity: Some(reprojection()),
            ..state(per_eye)
        }
    }

    /// In an eye-split pass each half's current position is the pixel's NDC in an eye-sized target,
    /// whatever the vertex shader handed down, and the previous position is taken through the eye's
    /// map.
    #[test]
    fn velocity_is_measured_in_each_eyes_screen_space() {
        let rows = velocity_state(true).rows(Some(viewport(640.0)));
        let previous = glam::Vec4::new(0.5, -1.0, 0.1, 2.0);
        for (x, y) in [(0.5, 0.5), (100.5, 200.5), (319.5, 359.5)] {
            let eye_sized = [x / 320.0 * 2.0 - 1.0, 1.0 - y / 360.0 * 2.0];
            for (eye, shift) in [(0, -0.03), (1, 0.03)] {
                let position = [x + 320.0 * eye as f32, y];
                let (taken, current) = corrected(&rows, position, previous, [0.9, 0.9]);
                assert!((current[0] - eye_sized[0]).abs() < 1e-5, "{x} {eye}");
                assert!((current[1] - eye_sized[1]).abs() < 1e-5, "{y} {eye}");
                assert!((taken[0] - (0.25 + shift)).abs() < 1e-6, "{eye}");
                assert!((taken[1] + 0.5).abs() < 1e-6, "{eye}");
            }
        }
    }

    /// Outside an eye-split pass, with the switch off, without a map, and with the buffer unbound (all
    /// zeros), the rewritten shader subtracts exactly the positions the vertex shader handed down.
    /// A canted eye's previous view keeps the camera's orientation and moves only to the eye: for a
    /// point given in the camera's view space, the offset view puts it where the camera would see it
    /// from the eye's position.
    #[test]
    fn the_previous_eye_view_carries_the_offset_and_not_the_cant() {
        let cant = glam::DQuat::from_rotation_y(5f64.to_radians());
        let eye = glam::DVec3::new(-0.032, 0.0015, 0.004);
        let eye_pose = glam::DMat4::from_rotation_translation(cant, eye);
        let offset = eye_offset_view(eye_pose.inverse());
        let point = glam::DVec3::new(3.0, -1.0, -20.0);
        assert!(
            offset
                .transform_point3(point)
                .abs_diff_eq(point - eye, 1e-12)
        );
        let parallel = glam::DMat4::from_translation(eye);
        assert!(eye_offset_view(parallel.inverse()).abs_diff_eq(parallel.inverse(), 1e-12));
    }

    #[test]
    fn velocity_rows_outside_an_eye_split_pass_change_nothing() {
        let unbound = [ZERO; ROWS];
        let previous = glam::Vec4::new(0.5, -1.0, 0.1, 2.0);
        for rows in [
            velocity_state(false).rows(Some(viewport(640.0))),
            velocity_state(true).rows(None),
            state(true).rows(Some(viewport(640.0))),
            unbound,
        ] {
            assert_eq!(rows[PIXEL].data[3], 0.0);
            for position in [[0.5, 0.5], [400.5, 100.5]] {
                let (taken, current) = corrected(&rows, position, previous, [0.9, 0.7]);
                assert_eq!(taken, [0.25, -0.5]);
                assert_eq!(current, [0.9, 0.7]);
            }
        }
    }

    /// A viewport with no area publishes no velocity rows.
    #[test]
    fn a_degenerate_viewport_has_no_velocity_rows() {
        assert!(velocity_rows(viewport(0.0), reprojection()).is_none());
    }
}
