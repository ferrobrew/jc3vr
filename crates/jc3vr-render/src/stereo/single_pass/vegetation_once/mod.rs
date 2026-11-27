//! The bark and foliage blocks drawn once for both eyes, instead of re-issued per eye.
//!
//! Both blocks bake the centred camera's view-projection into a vertex constant inside their own
//! `Draw` and draw three ways from it: a plain indexed draw, a hardware-instanced draw that takes its
//! per-instance data from a vertex stream, and a GPU-indirect draw whose instance count lives in a
//! buffer the vegetation update fills from compute. The fallback re-issues the whole block once per
//! eye with that constant reprojected (`per_eye_reissue`). Here the block runs once instead, inside a
//! scope the draw detours consult, and each of its draws binds a reprojected variant of the vertex
//! shader the block's setup bound ([`variants`]), whose `M_eye` post-multiply moves the centred clip
//! position into the eye its instance parity picks:
//!
//! - a plain draw is promoted to two instances with the viewport split, as a patched one is;
//! - a GPU-indirect draw is issued against a copy of the argument buffer with every instance count
//!   doubled ([`indirect_args`]); its variant halves the instance id it fetches the instance by;
//! - a hardware-instanced draw takes a step-rate-doubled layout twin when one exists
//!   ([`crate::stereo::single_pass::hw_instanced`]), and otherwise that one draw is re-issued per
//!   eye with `cb13` pinned to the eye, which is what the whole-block re-issue spent on it anyway.
//!
//! Everything else the shaders read -- the wind, the instance fades and LOD, the camera position the
//! foliage's wind-noise lookup and the bark's camera-relative offset use -- stays the engine's single
//! computation, shared by both eyes as it is under the re-issue. The pixel shaders are untouched: the
//! per-eye camera, light, and screen-door rewrites already pick the eye from the pixel position.
//!
//! A block is drawn once only when the switch is on, the eye split is live, the block is not drawing
//! its billboards (which route through a geometry shader), and the vertex shader its setup bound has a
//! variant; otherwise it takes the re-issue. A draw inside a single-drawn block that cannot be
//! converted after all (no variant on the bound shader, or no argument copy) is re-issued per eye
//! without parallax rather than dropped, and logged once.

mod indirect_args;
pub(crate) mod variants;

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use jc3gi::graphics_engine::{graphics_engine::RenderContext, render_pass::RenderStatus};
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11Buffer, ID3D11DeviceContext, ID3D11VertexShader},
    core::Interface,
};

use crate::{
    probes,
    probes::{DrawArgs, RawDrawKind, Route},
    stereo::single_pass::{
        BOUND_VS, CollapseViewport, DRAW_INDEXED_INSTANCED, SinglePassSwitch,
        bind_both_viewport_slots, config_flags, current_pass_id, ensure_collapse_viewport,
        eye_split_state, hw_instanced_once, instanced_per_eye,
    },
};

/// Draw a bark or foliage block once for both eyes, reporting whether it did; `false` leaves the
/// block to the per-eye re-issue. `draw` invokes the block's original `Draw` or `DrawZ`.
///
/// # Safety
///
/// `rc` must be the live render context the block's detour received.
pub(crate) unsafe fn draw_block(rc: *const RenderContext, draw: &mut dyn FnMut()) -> bool {
    if !config_flags().has(SinglePassSwitch::VegetationOnce) {
        return false;
    }
    let Some((_, full, d3d)) = eye_split_state() else {
        return false;
    };
    // SAFETY: per this function's contract.
    let Some(rc) = (unsafe { rc.as_ref() }) else {
        return false;
    };
    if rc
        .m_RenderStatus
        .contains(RenderStatus::RENDERSTATUS_BILLBOARD_ARRAY)
        || !bound_shader_has_variant()
        || indirect_args::failed()
    {
        DECLINED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    SCOPE.store(true, Ordering::Relaxed);
    draw();
    SCOPE.store(false, Ordering::Relaxed);
    // Leave the full viewport behind, as the re-issue does.
    bind_both_viewport_slots(d3d, full);
    DRAWN_ONCE.fetch_add(1, Ordering::Relaxed);
    true
}

/// Whether a single-drawn vegetation block is drawing on this thread's dispatch.
pub(super) fn active() -> bool {
    SCOPE.load(Ordering::Relaxed)
}

/// Handle a plain indexed draw inside a single-drawn block: one two-instance draw with the variant
/// bound and the viewport split. `false` when the bound shader has no variant.
#[inline]
pub(super) fn draw_indexed(
    context: *mut c_void,
    index_count: u32,
    start_index: u32,
    base_vertex: i32,
) -> bool {
    let Some(instanced) = DRAW_INDEXED_INSTANCED.get() else {
        return false;
    };
    // SAFETY: `context` is the live immediate context the detoured entry was called on.
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return false;
    };
    let Some(_variant) = BoundVariant::bind(ctx) else {
        note_unconverted("a plain draw's vertex shader has no variant");
        return false;
    };
    ensure_collapse_viewport(context, CollapseViewport::Split);
    probes::submit(
        probes::armed(),
        context,
        RawDrawKind::Indexed,
        Route::Promoted,
        DrawArgs::Direct([
            index_count.into(),
            2,
            start_index.into(),
            base_vertex.into(),
            0,
        ]),
        // SAFETY: the caller's arguments as two instances, through the trampoline so the mod's own
        // draw is not seen again as a game draw.
        || unsafe { instanced.call(context, index_count, 2, start_index, base_vertex, 0) },
    );
    true
}

/// Handle a hardware-instanced draw inside a single-drawn block: once through a layout twin when
/// there is one, otherwise re-issued per eye with `cb13` pinned to each eye, with the variant bound
/// either way. Without a variant it is still re-issued per eye, without parallax. `submit_doubled`
/// submits the caller's draw with the instance count it is given; `submit_eye` submits it unchanged
/// for the eye it is given.
#[inline]
pub(super) fn draw_indexed_instanced(
    context: *mut c_void,
    instance_count: u32,
    submit_doubled: &dyn Fn(u32),
    submit_eye: &dyn Fn(usize),
) -> bool {
    // SAFETY: `context` is the live immediate context the detoured entry was called on.
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return false;
    };
    let variant = BoundVariant::bind(ctx);
    if variant.is_none() {
        note_unconverted("a hardware-instanced draw's vertex shader has no variant");
    }
    let once = variant.is_some()
        && config_flags().has(SinglePassSwitch::HwInstancedOnce)
        && hw_instanced_once(context, instance_count, submit_doubled);
    once || instanced_per_eye(submit_eye)
}

/// Handle a GPU-indirect draw inside a single-drawn block: issued once against the copy of its
/// argument buffer with the instance counts doubled, with the variant bound and the viewport split.
/// `false` when the bound shader has no variant or the copy could not be made. `submit` issues the
/// draw against the argument buffer it is given, at the caller's offset.
#[inline]
pub(super) fn draw_indexed_instanced_indirect(
    context: *mut c_void,
    buffer_for_args: *mut c_void,
    submit: &dyn Fn(*mut c_void),
) -> bool {
    // SAFETY: both are the live COM pointers the detoured entry was called with.
    let (Some(ctx), Some(source)) = (unsafe {
        (
            ID3D11DeviceContext::from_raw_borrowed(&context),
            ID3D11Buffer::from_raw_borrowed(&buffer_for_args),
        )
    }) else {
        return false;
    };
    let Some(variant) = BoundVariant::bind(ctx) else {
        note_unconverted("a GPU-indirect draw's vertex shader has no variant");
        return false;
    };
    let Some(doubled) = indirect_args::doubled(ctx, source, current_pass_id()) else {
        drop(variant);
        note_unconverted("the GPU-indirect arguments could not be doubled");
        return false;
    };
    ensure_collapse_viewport(context, CollapseViewport::Split);
    submit(doubled.as_raw());
    true
}

/// Start a dispatch: the argument copies of the last one are stale.
pub(super) fn begin_dispatch() {
    indirect_args::begin_dispatch();
}

/// Drop the argument copy's resources. Runs on eject, once no draw can reach them.
pub(super) fn release() {
    indirect_args::release();
}

/// A one-line status for the debug UI.
pub fn status() -> String {
    let (attached, failed) = variants::counts();
    format!(
        "{attached} shader variants ({failed} failed); {} blocks drawn once, {} re-issued, {} draws \
         unconverted since injection; {}",
        DRAWN_ONCE.load(Ordering::Relaxed),
        DECLINED.load(Ordering::Relaxed),
        UNCONVERTED.load(Ordering::Relaxed),
        indirect_args::status(),
    )
}

static SCOPE: AtomicBool = AtomicBool::new(false);
static DRAWN_ONCE: AtomicUsize = AtomicUsize::new(0);
static DECLINED: AtomicUsize = AtomicUsize::new(0);
static UNCONVERTED: AtomicUsize = AtomicUsize::new(0);
static UNCONVERTED_LOGGED: AtomicBool = AtomicBool::new(false);

/// Whether the vertex shader bound when a block's `Draw` begins -- the one its setup bound -- has a
/// variant.
fn bound_shader_has_variant() -> bool {
    let shader = BOUND_VS.load(Ordering::Relaxed) as *mut c_void;
    // SAFETY: the pointer is the shader the last `VSSetShader` bound, which the context keeps alive
    // while it is bound; borrowing it takes no reference.
    unsafe { ID3D11VertexShader::from_raw_borrowed(&shader) }
        .and_then(variants::variant)
        .is_some()
}

/// Count a draw inside a single-drawn block that could not be converted, logging the first.
fn note_unconverted(reason: &str) {
    UNCONVERTED.fetch_add(1, Ordering::Relaxed);
    if !UNCONVERTED_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            target: "single_pass",
            "vegetation once: {reason}; that draw is re-issued per eye without parallax"
        );
    }
}

/// The bound vertex shader's variant, bound in its place until this is dropped.
struct BoundVariant<'a> {
    ctx: &'a ID3D11DeviceContext,
    original: ID3D11VertexShader,
}

impl<'a> BoundVariant<'a> {
    fn bind(ctx: &'a ID3D11DeviceContext) -> Option<Self> {
        let mut original = None;
        // SAFETY: the live immediate context; the getter only reads bound state.
        unsafe { ctx.VSGetShader(&mut original, None, None) };
        let original = original?;
        let variant = variants::variant(&original)?;
        // SAFETY: a live vertex shader; the engine's is put back on drop.
        unsafe { ctx.VSSetShader(&variant, None) };
        Some(Self { ctx, original })
    }
}

impl Drop for BoundVariant<'_> {
    fn drop(&mut self) {
        // SAFETY: the shader the engine had bound, still alive through this reference.
        unsafe { self.ctx.VSSetShader(&self.original, None) };
    }
}
