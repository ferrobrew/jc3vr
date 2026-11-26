//! Relax the volumetric-patch terrain's two *view-dependent* hull culls for stereo (issue #40).
//!
//! The tessellated terrain (`CRenderBlockTerrainPatch` -- the terrain the game world actually
//! draws) discards whole patches inside its hull shader, from constants the type bakes once per frame
//! per constant-buffer slot in
//! [`SetupConstantBuffers`](jc3gi::graphics_engine::render_block::RenderBlockTypeTerrainPatch::SetupConstantBuffers):
//!
//! - **Back-patch cull.** The hull discards a patch when all three control-point normals are more
//!   than `m_BackPatchCullThreshold` (shipped `-0.3`, ~17.5 deg of slack) away from facing *one*
//!   direction -- the render camera's forward axis -- rather than the actual view vector to the
//!   patch. That approximation only holds for a narrow FOV: at 45-55 deg off-axis, which is most of a
//!   headset's field of view, a patch that squarely faces the eye can still be beyond the threshold
//!   from the camera axis, so it is discarded while visible.
//! - **Frustum cull.** The hull projects each control point (expanded by the patch radius) through
//!   the baked `m_OffsetViewProjection` and discards the patch when its expanded bounds fall outside
//!   a clip plane. That matrix is the *render camera's*, and the constant buffer is baked once per
//!   frame per slot -- so both eyes are culled against one frustum. Under the single-pass collapse
//!   that frustum belongs to neither eye: the render camera stays centred with the engine's own
//!   projection, built from the mod's injected 90 deg *vertical* FOV and the double-wide target's
//!   aspect (`Camera::RecalcProjection` -> `CMatrix4f::PerspectiveFov`, which takes the vertical FOV
//!   and divides by the aspect for the horizontal extent). A headset eye whose vertical FOV exceeds
//!   90 deg, or whose display cant rotates it off the centre axis, sees past that frustum's top and
//!   bottom, and every terrain patch out there is discarded for both eyes.
//!
//! A patch the hull discards is not drawn in any pass, and the coarser tile that covers the same
//! footprint is separately clipped away by the LOD clip (which keys on the streaming visibility mask,
//! not on the view), so nothing fills the hole: the pixels keep the far depth and resolve dark --
//! world-locked black patch-shaped gaps that flip with head rotation.
//!
//! The fix substitutes the two enable flags at *upload* time: the detour clears them around the
//! type's own bake, so the baked constants carry `0`, and restores the engine's values immediately
//! afterwards (the engine's own settings, and its debug UI's view of them, are untouched). The LOD
//! clip and the cull-by-detail term are left alone -- neither is view-dependent. Scoped to stereo
//! frames.
//!
//! Cost: the patches in the margin are tessellated and rasterized instead of dropped, bounded by
//! whatever the CPU-side patch cull already admitted (itself widened to the binocular union in
//! [`crate::hooks::graphics_engine::culling`]). Back-facing triangles still die at the rasterizer, and out-of-frustum ones at
//! the clipper.
//!
//! Under the single-pass collapse the frustum the hull programs cull against is the centred
//! camera's, which carries eye 0's projection and no cant, and a canted eye sees past its outer edge
//! (the left eye's: the right eye's outer side lies inside the double-wide target's clip range).
//! Clearing the flag does not reach every hull: the patch system's own hull programs test the
//! frustum unconditionally. So inside the G-buffer range under the collapse, both block types' bakes
//! ([`RenderBlockTypeTerrain::SetupConstantBuffers`](jc3gi::graphics_engine::render_block::RenderBlockTypeTerrain::SetupConstantBuffers)
//! and the patch type's) take the offset view-projection's horizontal, vertical, and `w` columns from
//! the projection bounding both eyes ([`jc3vr_view::vr::eyes_union_projection_standard`]), keeping the
//! engine's depth column. The domain programs read each eye's own rows there, so only the cull sees
//! the substitution ([`crate::stereo::single_pass::terrain_cull_both_eyes`] holds the gate). The bake
//! is cached per slot per frame, so a change of substitution between two bakes in one frame stamps
//! every slot stale and the next draw re-bakes.

use std::sync::atomic::{AtomicU8, Ordering};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        graphics_engine::{RenderContext, get_render_frame_counters},
        render_block::{RenderBlockTypeTerrain, RenderBlockTypeTerrainPatch},
        render_pass::CalculateOffsetViewProjectionMatrix,
    },
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::stereo::single_pass::{TerrainCullBlock, terrain_cull_both_eyes};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&TERRAIN_PATCH_SETUP_CONSTANT_BUFFERS_BINDER)
        .with_static_binder(&BASE_TERRAIN_SETUP_CONSTANT_BUFFERS_BINDER)
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockTypeTerrainPatch::SetupConstantBuffers_ADDRESS
)]
fn terrain_patch_setup_constant_buffers(
    this: *mut RenderBlockTypeTerrainPatch,
    render_context: *mut RenderContext,
) {
    let original = TERRAIN_PATCH_SETUP_CONSTANT_BUFFERS.get().unwrap();
    if !jc3vr_view::stereo::active() || this.is_null() {
        original.call(this, render_context);
        return;
    }
    // SAFETY: `this` is the live terrain-patch render block type the engine passed in, on the draw
    // thread. The two flags are written through raw pointers rather than a `&mut` so nothing aliases
    // the callee's own view of the type across the trampoline call, and both are restored before
    // returning, so this changes only what the bake below writes into the hull/domain constants.
    unsafe {
        let back = &raw mut (*this).m_EnableBackPatchCulling;
        let frustum = &raw mut (*this).m_EnableFrustumPatchCulling;
        let saved = (*back, *frustum);
        *back = false;
        *frustum = false;
        let stamps = &raw mut (*this).m_WasCBApplied;
        bake(TerrainCullBlock::Patch, stamps, render_context, || {
            original.call(this, render_context)
        });
        *back = saved.0;
        *frustum = saved.1;
    }
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockTypeTerrain::SetupConstantBuffers_ADDRESS
)]
fn base_terrain_setup_constant_buffers(
    this: *mut RenderBlockTypeTerrain,
    render_context: *mut RenderContext,
) {
    let original = BASE_TERRAIN_SETUP_CONSTANT_BUFFERS.get().unwrap();
    if this.is_null() || !jc3vr_view::stereo::active() {
        original.call(this, render_context);
        return;
    }
    // SAFETY: `this` is the live base-terrain render block type on the draw thread; the stamps are
    // reached through a raw pointer so nothing aliases the callee's view of the type.
    let stamps = unsafe { &raw mut (*this).m_WasCBApplied };
    bake(TerrainCullBlock::Base, stamps, render_context, || {
        original.call(this, render_context)
    });
}

/// Run a terrain type's constant bake (`call`), with the render context's offset view-projection
/// widened to both eyes when [`terrain_cull_both_eyes`] says so, and the type's per-slot cache
/// (`stamps`) marked stale whenever the substitution differs from the one the last bake of this
/// block type made.
fn bake(
    block: TerrainCullBlock,
    stamps: *mut [u32; 22],
    render_context: *mut RenderContext,
    call: impl FnOnce(),
) {
    let union = terrain_cull_both_eyes(block)
        .then(jc3vr_view::vr::eyes_union_projection_standard)
        .flatten();
    let mode = if union.is_some() {
        BakeMode::Union
    } else {
        BakeMode::Engine
    };
    let last = LAST_MODE[block as usize].swap(mode as u8, Ordering::Relaxed);
    if last != mode as u8 {
        // SAFETY: `stamps` is the live type's cache array on the draw thread. A stamp one frame back
        // can never match the frame being drawn, so every slot re-bakes on its next draw.
        unsafe {
            let stale = get_render_frame_counters().m_FrameIndex.wrapping_sub(1);
            (*stamps).fill(stale);
        }
    }
    let (Some(union), Some(rc)) = (union, unsafe { render_context.as_mut() }) else {
        call();
        return;
    };
    let engine = rc.m_OffsetViewProjection;
    rc.m_OffsetViewProjection = widened_offset_view_projection(&rc.m_View, &union, &engine);
    call();
    rc.m_OffsetViewProjection = engine;
}

/// Which offset view-projection a terrain type's cached slots were last baked with.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum BakeMode {
    Engine,
    Union,
}

/// The [`BakeMode`] of each [`TerrainCullBlock`]'s last bake, on the draw thread.
static LAST_MODE: [AtomicU8; 2] = [
    AtomicU8::new(BakeMode::Engine as u8),
    AtomicU8::new(BakeMode::Engine as u8),
];

/// The engine's offset view-projection with its clip `x`, `y`, and `w` columns taken from `view`
/// through `union`, and its depth column kept. The hull's frustum test compares `x` and `y` against
/// `w`, and its depth test reads `z` alone, so the sides become the union's while the near and far
/// tests stay the engine's. The engine's matrices are row-major for row vectors, so clip component
/// `k` is column `k`.
fn widened_offset_view_projection(view: &Matrix4, union: &Matrix4, engine: &Matrix4) -> Matrix4 {
    let mut widened = Matrix4::default();
    // SAFETY: a pure function of its two input matrices that writes only the output; all three are
    // live locals or borrows.
    unsafe { CalculateOffsetViewProjectionMatrix(view, union, &mut widened) };
    for row in 0..4 {
        widened.data[row * 4 + 2] = engine.data[row * 4 + 2];
    }
    widened
}
