//! The draw-path COM detours: the vtable entry points every geometry submission passes through, and
//! the per-eye re-issue of the ones the collapse cannot handle by instance doubling alone.
//!
//! A patched vertex shader renders both eyes from one instanced draw, so the common case needs
//! nothing here. These exist for the draws that cannot take that route -- an unpatched shader, a draw
//! the engine already instanced for its own reasons, and the GPU-indirect draws whose instance count
//! lives in a buffer the CPU never sees -- each of which has to be submitted once per eye instead.

use jc3vr_view::stereo::layout::collapse_active;
use windows::Win32::Graphics::Direct3D11::{ID3D11DomainShader, ID3D11GeometryShader};

use crate::{
    probes,
    probes::{CaptureProbes, DrawArgs, DrawScope, GameDraw, RawDrawKind, Route},
    stereo::single_pass::{
        detour_timing, pinned_eye::Cb13EyePin, sun::preserve_far_depth, vegetation_once,
        water_fragment_state::packed_water_context, *,
    },
};

/// `ID3D11DeviceContext` vtable slots for `DrawIndexed`, `Draw`, and `DrawIndexedInstanced`
/// (verified against `windows`'s `ID3D11DeviceContext_Vtbl`: field 6 → slot 12, field 7 → slot 13,
/// field 14 → slot 20).
pub(super) const DRAW_INDEXED_SLOT: usize = 12;
pub(super) const DRAW_SLOT: usize = 13;
pub(super) const DRAW_INDEXED_INSTANCED_SLOT: usize = 20;

pub(super) type DrawIndexedFn = unsafe extern "system" fn(*mut c_void, u32, u32, i32);
pub(super) type DrawFn = unsafe extern "system" fn(*mut c_void, u32, u32);
pub(super) type DrawIndexedInstancedFn =
    unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32);

pub(super) static DRAW_INDEXED: DetourSlot<DrawIndexedFn> = DetourSlot::new();
pub(super) static DRAW: DetourSlot<DrawFn> = DetourSlot::new();
/// `DrawIndexedInstanced` (see [`draw_indexed_instanced_detour`]); its trampoline also serves as the
/// raw entry a promoted [`draw_indexed_detour`] draw is re-issued through.
///
/// An already-instanced draw whose patched vertex shader takes its per-instance data through a
/// vertex-buffer slot (rather than through `SV_InstanceID`, which would have deferred the shader with
/// `InstanceIdAlreadyDeclared`) has instance ids that are the game's own, so `SV_InstanceID & 1` sends
/// instance `i` to eye `i & 1` -- half the instances in each eye, and a 1-instance draw to the left eye
/// only. Promoting the instance count cannot fix it: the per-instance vertex-buffer stepping is indexed
/// by the instance id, so doubling the count reads past the instance data. Instead [`instanced_per_eye`]
/// makes the parity irrelevant, re-issuing the draw once per eye with both `cb13` eye slots and both
/// viewport slots pinned to that eye -- the bucket-(d) mechanism in
/// `docs/mod/stereo/single-pass-render-blocks.md`.
pub(super) static DRAW_INDEXED_INSTANCED: DetourSlot<DrawIndexedInstancedFn> = DetourSlot::new();

/// Handle a `DrawIndexed` while the dual-eye G-buffer geometry is drawing. A **patched** shader is
/// promoted to a 2-instance `DrawIndexedInstanced` -- its `SV_InstanceID & 1` selects the eye and
/// `SV_ViewportArrayIndex` routes it to that eye's viewport half (one draw, both eyes). An
/// **unpatched** shader writes no viewport index and so would rasterise to slot 0 only; under collapse
/// it is instead re-issued once per eye with both slots pinned to that eye's half, which costs a
/// second submission but is the only way it reaches both eyes. The patched/unpatched split is counted
/// for the diagnostic log.
pub(super) unsafe extern "system" fn draw_indexed_detour(
    context: *mut c_void,
    index_count: u32,
    start_index: u32,
    base_vertex: i32,
) {
    let _timing = detour_timing::enter();
    let probes = probes::armed();
    let args = [
        index_count.into(),
        start_index.into(),
        base_vertex.into(),
        0,
        0,
    ];
    let draw = GameDraw {
        context,
        entry: RawDrawKind::Indexed,
        args: DrawArgs::Direct(args),
    };
    // SAFETY (all three calls): `context` is the live immediate context this entry was called on.
    let water = unsafe { packed_water_context(&context) };
    let scope = unsafe { probes::draw_scope(probes, &draw) };
    let _lake = bind_water_variant(water, scope.as_ref());
    let _bound = unsafe { probes::draw_bound(probes, &draw) };
    let detour = DRAW_INDEXED.get().expect("set before enable");
    if vegetation_once::active()
        && vegetation_once::draw_indexed(context, index_count, start_index, base_vertex)
    {
        return;
    }
    if active() && in_gbuffer_range() && per_eye_reissue_eye().is_none() {
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        if patched {
            if collapse_active() {
                ensure_collapse_viewport(context, CollapseViewport::Split);
            }
            PATCHED_DRAWS.bump();
            if let Some(instanced) = DRAW_INDEXED_INSTANCED.get() {
                // Through the trampoline, not the detoured entry: the promotion is the mod's own
                // draw, so it must not be counted as exposure by [`draw_indexed_instanced_detour`].
                probes::submit(
                    probes,
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
                    || unsafe {
                        instanced.call(context, index_count, 2, start_index, base_vertex, 0)
                    },
                );
                return;
            }
        } else {
            UNPATCHED_DRAWS.bump();
            if collapse_active() {
                // The shader was not rewritten, so it writes no `SV_ViewportArrayIndex` and always
                // rasterises to slot 0. Left to the split binding it would appear in the left eye
                // only; left to the full binding it would stretch across both. Re-issue it once per
                // eye with both slots pinned to that eye's half. It still reads the centred `cb0`, so
                // there is no parallax on it -- but it is present, in both eyes, at the right size.
                // A pinned-eye shader in the pipeline does get the parallax, from `cb13` pinned to
                // the eye being drawn.
                let pin = Cb13EyePin::begin(context);
                for eye in 0..2 {
                    ensure_collapse_viewport(context, CollapseViewport::Eye(eye));
                    if let Some(pin) = &pin {
                        pin.pin(eye);
                    }
                    probes::submit(
                        probes,
                        context,
                        RawDrawKind::Indexed,
                        Route::PerEye(eye),
                        DrawArgs::Direct(args),
                        || unsafe { detour.call(context, index_count, start_index, base_vertex) },
                    );
                }
                ensure_collapse_viewport(context, CollapseViewport::Full);
                drop(pin);
                return;
            }
        }
    }
    probes::submit(
        probes,
        context,
        RawDrawKind::Indexed,
        Route::AsIssued,
        DrawArgs::Direct(args),
        || unsafe { detour.call(context, index_count, start_index, base_vertex) },
    );
}

/// Handle a non-indexed `Draw` (vtable slot 13). The fullscreen passes (deferred lighting,
/// screen-space effects, post) draw a fullscreen triangle this way, and under collapse they must
/// cover the **full** target, not the eye-half the previous patched geometry draw left the viewport
/// split to, so the viewport is reset to full before the draw. The geometry passes
/// ([`is_geometry_slot13_pass`]), the static light sprites, the water reflection planes, and the sky
/// bodies are promoted or re-issued per eye instead. Outside the collapse's G-buffer range the draw
/// is submitted as issued.
pub(super) unsafe extern "system" fn draw_detour(
    context: *mut c_void,
    vertex_count: u32,
    start_vertex: u32,
) {
    let _timing = detour_timing::enter();
    let probes = probes::armed();
    let args = DrawArgs::Direct([vertex_count.into(), start_vertex.into(), 0, 0, 0]);
    let draw = GameDraw {
        context,
        entry: RawDrawKind::NonIndexed,
        args,
    };
    // SAFETY (all four calls): `context` is the live immediate context this entry was called on.
    let water = unsafe { packed_water_context(&context) };
    let scope = unsafe { probes::draw_scope(probes, &draw) };
    let _lake = bind_water_variant(water, scope.as_ref());
    let _glare = unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) }
        .and_then(post_filters::bind);
    let _bound = unsafe { probes::draw_bound(probes, &draw) };
    let detour = DRAW.get().expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = |route| {
        probes::submit(
            probes,
            context,
            RawDrawKind::NonIndexed,
            route,
            args,
            || unsafe { detour.call(context, vertex_count, start_vertex) },
        )
    };

    if collapse_active() && in_gbuffer_range() && per_eye_reissue_eye().is_none() {
        let pass = current_pass_id();
        SLOT13_BY_PASS[usize::from(pass)].bump();
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        let flags = config_flags();
        if is_geometry_slot13_pass(pass) {
            // A patched shader resolves its eye from the instance parity and routes itself to that
            // eye's half, so one two-instance draw covers both eyes, as the indexed path promotes
            // its draws.
            if patched && promote_non_indexed(probes, context, vertex_count, start_vertex) {
                return;
            }
            // Any other geometry on this entry point is not the fullscreen triangle the `Full`
            // reset below exists for, so give it the same per-eye treatment the indexed path gives
            // its unpatched draws.
            if flags.has(SinglePassSwitch::Slot13PerEye)
                && instanced_per_eye(&|eye| submit(Route::PerEye(eye)))
            {
                return;
            }
        }
        if patched
            && promotes_non_indexed(pass)
            && promote_non_indexed(probes, context, vertex_count, start_vertex)
        {
            return;
        }
        // The moon is one reprojected `open` quad, and the stars one reprojected billboard draw, so
        // each takes the same two-instance promotion, with its far-plane depth kept through the
        // reprojection.
        if patched
            && (pass == RenderPassId::RP_MOON as u8 || pass == RenderPassId::RP_STARS as u8)
            && let Some(_depth) = FarDepthRows::begin()
            && promote_non_indexed(probes, context, vertex_count, start_vertex)
        {
            return;
        }
        ensure_collapse_viewport(context, CollapseViewport::Full);
    }
    // SAFETY: `context` is the live immediate context this entry was called on.
    let _sun_depth = (current_pass_id() == RenderPassId::RP_SUN as u8)
        .then(|| unsafe { preserve_far_depth(context) });
    submit(Route::AsIssued);
}

/// Render passes whose non-indexed draws are **geometry**, not a viewport-covering triangle.
///
/// `Draw` (slot 13) is predominantly the fullscreen-pass entry point, which is why the default is to
/// reset the viewport to full for it. But a number of render blocks submit ordinary world geometry
/// non-indexed, and pinning `Full` for those rasterises them across the whole double-wide target,
/// stretched 2x horizontally about its centre. A 2x horizontal stretch is also a 2x horizontal
/// *motion* gain, which is what makes decals appear to slide across the world at twice the camera's
/// rate.
///
/// # How this list was derived
///
/// Statically, from the binary rather than from a capture. The engine funnels every
/// non-indexed submission through one wrapper (`Graphics::Draw`), so its call sites are the complete
/// set of blocks that can reach slot 13 -- 60-odd of them. Each was classified by reading its `Draw`:
/// a block that binds a vertex stream and submits a world transform is geometry, one that submits a
/// screen-covering primitive is not. Their passes come from the engine's pass-creation sites
/// (`CRenderEngine::CreateRenderPass`, whose pass id is a literal at every call), which name the
/// owning system for each id, plus that system's enqueue site.
///
/// The two groups below are the result. Every fullscreen slot-13 block sits on a pass of its own
/// that no geometry block is enqueued onto -- including three that fall *inside* the GBuffer
/// index range and so are live under the collapse ([`RenderPassId::RP_DOWNSAMPLE_DEPTH`],
/// [`RenderPassId::RP_UNDERWATER_FOG_GRADIENT`], and the fog-volume trio) -- so no pass carries both
/// kinds and the pass id is a sound discriminator. It stays an allowlist, not a heuristic: the cost of
/// misclassifying a fullscreen pass as geometry is a visibly wrong frame, while the cost of missing a
/// geometry pass is only the status quo.
///
/// Deliberately absent: [`RenderPassId::RP_MODELS_REFLECTION`], which is model geometry but renders
/// the reflection camera's view into the reflection target, not into the double-wide scene target --
/// splitting it per eye would be wrong. The blocks whose passes lie outside the collapsed range
/// (UI, the reflection prepasses, `POST_RP_FULLSCREEN_VIDEO`) need no entry at all.
pub(super) const GEOMETRY_SLOT13_PASSES: &[u8] = &[
    // Passes whose entire draw list is a block that *always* submits non-indexed.
    // `CRoadMeshManager` owns both road passes; a road is enqueued onto the stencil one as well when
    // its visual type has shader type 0.
    RenderPassId::RP_ROAD_STENCIL as u8,
    RenderPassId::RP_ROAD_LAYERS as u8,
    // `CCreatureManager`'s pass: `CCreatureRenderBlock::Draw` has no indexed path at all.
    RenderPassId::RP_CREATURES as u8,
    // `CDecalManager`'s two passes, drawing the four `CRenderBlockDecal*` types.
    RenderPassId::RP_DECALS as u8,
    RenderPassId::RP_WINDOW_DECALS as u8,
    // `CSkidmarkManager`'s pass, drawing `CRenderBlockSkidmarks`.
    RenderPassId::RP_SKIDMARKS as u8,
    // The model-family passes. General / GeneralJC3 / GeneralMaskedJC3 / BuildingJC3 / Prop /
    // MaterialTune / Open / FXMeshFire / MeshParticle all end their `Draw` and `DrawZ` with the same
    // branch -- `DrawIndexed` when the block has an index buffer, plain `Draw` when it does not -- so
    // any index-buffer-less model in the shipped data lands on slot 13 in whichever of these it was
    // enqueued onto. The engine owns the two Z passes; `CModelInstanceManager` owns the rest, and
    // enqueues nothing but model instances onto them.
    RenderPassId::RP_Z_PASS as u8,
    RenderPassId::RP_Z_AND_VELOCITY_PASS as u8,
    RenderPassId::RP_MODELS_DYNAMIC as u8,
    RenderPassId::RP_MODELS_DYNAMIC_MASK_DAMAGE_POST_EFFECT as u8,
    RenderPassId::RP_MODELS_STATIC as u8,
    RenderPassId::RP_MODELS_TRANSPARENT as u8,
    RenderPassId::RP_MODELS_GLINT as u8,
    RenderPassId::RP_MODEL_HALO_POST as u8,
    RenderPassId::RP_MODELS_REFRACT as u8,
    RenderPassId::RP_Z_FINAL_TRANSPARENT as u8,
    RenderPassId::RP_GHOST_EFFECT as u8,
    RenderPassId::RP_OUTLINE_MASK as u8,
    RenderPassId::RP_FINAL_TRANSPARENT as u8,
];

/// Render passes whose non-indexed draws with a patched vertex shader are the static light sprites,
/// promoted to one two-instance draw.
///
/// [`RenderBlockTypeLights::Setup`](jc3gi::graphics_engine::render_block::RenderBlockTypeLights::Setup)
/// selects its fake-light programs by [`RenderPassId::RP_DEFERRED_LIGHTS`], and the draw census
/// records the block drawing in [`RenderPassId::RP_LENSFLARE`]; its third pass, the planar
/// reflection's distant lights, lies outside the routed range. It is an allowlist rather than every
/// patched non-indexed draw because the census also records patched shaders on non-indexed draws
/// that are not scene geometry routed to the eye halves (the light-propagation-volume
/// initialisation, which writes a volume texture through its own geometry shader), and splitting
/// those would change what they write. The moon takes its own promotion.
const STATIC_LIGHT_PASSES: &[u8] = &[
    RenderPassId::RP_DEFERRED_LIGHTS as u8,
    RenderPassId::RP_LENSFLARE as u8,
];

/// Both `cb13` eye slots with their reprojection keeping the clip depth, restored to the live rows on
/// drop.
///
/// The `open` vertex shader pins a sky body's clip depth to the far plane when its material asks
/// for it. `M_eye`'s depth row mixes the other clip components back in, which moves that depth off
/// the plane and clips the body, so the promoted moon draws with `M_eye`'s depth row replaced by the
/// identity row in both slots, the correction the sun's per-eye re-drive takes.
struct FarDepthRows {
    d3d: EngineContext,
    rows: [Vector4; STEREO_CB_TOTAL_ROWS],
}

impl FarDepthRows {
    /// `None` when `cb13` holds no rows yet or the write fails; the caller then draws unpromoted.
    fn begin() -> Option<Self> {
        let d3d = EngineContext::get()?;
        let rows = CB13.lock().live_rows()?;
        let mut corrected = rows;
        preserve_reprojection_depth(&mut corrected);
        write_cb13_rows(d3d, &corrected).then_some(Self { d3d, rows })
    }
}

impl Drop for FarDepthRows {
    fn drop(&mut self) {
        if !write_cb13_rows(self.d3d, &self.rows) {
            tracing::warn!(
                target: "single_pass",
                "moon: cb13 restore failed; later passes may render with the far-depth rows"
            );
        }
    }
}

/// Whether a non-indexed draw with a patched vertex shader in `pass` is promoted to one
/// two-instance draw routed to the eye halves: the static light sprites' passes, and the water
/// reflection planes' pass, whose proxy quad the `watermask` reprojection moves into each eye. The
/// moon's promotion is separate, since it also has to keep its far-plane depth.
fn promotes_non_indexed(pass: u8) -> bool {
    STATIC_LIGHT_PASSES.contains(&pass) || pass == RenderPassId::RP_REFLECTIVE_WATER_PLANES as u8
}

/// Submit a non-indexed draw with a patched vertex shader as one two-instance `DrawInstanced` routed
/// to the eye halves, reporting whether it did; `false` leaves the draw to the caller.
///
/// It declines without a live context, and when a domain or geometry shader is bound. The routing
/// is the viewport index the vertex shader writes, which reaches the rasteriser only from the last
/// stage before it: behind a tessellation or geometry stage that writes none, both instances would
/// land in the left eye's half. The tessellated particles (`particleeffecttess*`) are such a
/// pipeline, with a vertex shader the `cb0` remap patches and clip built in the domain shader.
fn promote_non_indexed(
    probes: Option<&'static dyn CaptureProbes>,
    context: *mut c_void,
    vertex_count: u32,
    start_vertex: u32,
) -> bool {
    // SAFETY: `context` is the live immediate context the detoured entry was called on.
    let Some(ctx) = (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
    else {
        return false;
    };
    if vertex_shader_feeds_another_stage(ctx) {
        return false;
    }
    ensure_collapse_viewport(context, CollapseViewport::Split);
    PATCHED_DRAWS.bump();
    probes::submit(
        probes,
        context,
        RawDrawKind::NonIndexed,
        Route::Promoted,
        DrawArgs::Direct([vertex_count.into(), 2, start_vertex.into(), 0, 0]),
        // SAFETY: `DrawInstanced` is not detoured, so the promotion is not seen again as a game draw;
        // the arguments are the caller's, with the instance count doubled.
        || unsafe { ctx.DrawInstanced(vertex_count, 2, start_vertex, 0) },
    );
    true
}

/// Whether a domain or geometry shader is bound after the vertex shader.
fn vertex_shader_feeds_another_stage(ctx: &ID3D11DeviceContext) -> bool {
    let mut domain: Option<ID3D11DomainShader> = None;
    let mut geometry: Option<ID3D11GeometryShader> = None;
    // SAFETY: `ctx` is the live immediate context; the getters only read bound state.
    unsafe {
        ctx.DSGetShader(&mut domain, None, None);
        ctx.GSGetShader(&mut geometry, None, None);
    }
    domain.is_some() || geometry.is_some()
}

pub(super) fn is_geometry_slot13_pass(pass: u8) -> bool {
    GEOMETRY_SLOT13_PASSES.contains(&pass)
}

// ---- GPU-indirect draws --------------------------------------------------------------------------

/// `ID3D11DeviceContext` vtable slots for the two GPU-indirect draw entry points (field 33 → slot 39,
/// field 34 → slot 40, verified against `windows`'s `ID3D11DeviceContext_Vtbl`). The engine reaches
/// them through `Graphics::DrawInstanced` (slot 39) and `Graphics::DrawIndexedInstancedIndirectNoMutex`
/// (slot 40); the terrain-patch block's near tessellating passes and the foliage block's dominant path
/// are the volume users.
pub(super) const DRAW_INDEXED_INSTANCED_INDIRECT_SLOT: usize = 39;
pub(super) const DRAW_INSTANCED_INDIRECT_SLOT: usize = 40;

/// Both indirect entry points take `(ID3D11Buffer* pBufferForArgs, UINT AlignedByteOffsetForArgs)`.
pub(super) type DrawIndirectFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32);

pub(super) static DRAW_INDEXED_INSTANCED_INDIRECT: DetourSlot<DrawIndirectFn> = DetourSlot::new();
pub(super) static DRAW_INSTANCED_INDIRECT: DetourSlot<DrawIndirectFn> = DetourSlot::new();

pub(super) unsafe extern "system" fn draw_indexed_instanced_indirect_detour(
    context: *mut c_void,
    buffer_for_args: *mut c_void,
    aligned_byte_offset: u32,
) {
    let _timing = detour_timing::enter();
    let probes = probes::armed();
    let args = DrawArgs::Indirect {
        buffer: buffer_for_args,
        offset: aligned_byte_offset,
    };
    let draw = GameDraw {
        context,
        entry: RawDrawKind::IndexedInstancedIndirect,
        args,
    };
    // SAFETY (all three calls): `context` is the live immediate context this entry was called on.
    let water = unsafe { packed_water_context(&context) };
    let scope = unsafe { probes::draw_scope(probes, &draw) };
    let _lake = bind_water_variant(water, scope.as_ref());
    let _bound = unsafe { probes::draw_bound(probes, &draw) };
    let detour = DRAW_INDEXED_INSTANCED_INDIRECT
        .get()
        .expect("set before enable");
    if vegetation_once::active()
        && vegetation_once::draw_indexed_instanced_indirect(context, buffer_for_args, &|doubled| {
            probes::submit(
                probes,
                context,
                RawDrawKind::IndexedInstancedIndirect,
                Route::Promoted,
                DrawArgs::Indirect {
                    buffer: doubled,
                    offset: aligned_byte_offset,
                },
                // SAFETY: the caller's draw against the doubled copy of its arguments, at the same
                // offset, through the trampoline.
                || unsafe { detour.call(context, doubled, aligned_byte_offset) },
            )
        })
    {
        return;
    }
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = |eye: Option<usize>| {
        probes::submit(
            probes,
            context,
            RawDrawKind::IndexedInstancedIndirect,
            eye.map_or(Route::AsIssued, Route::PerEye),
            args,
            || unsafe { detour.call(context, buffer_for_args, aligned_byte_offset) },
        )
    };
    if !indirect_per_eye(context, &submit) {
        submit(None);
    }
}

pub(super) unsafe extern "system" fn draw_instanced_indirect_detour(
    context: *mut c_void,
    buffer_for_args: *mut c_void,
    aligned_byte_offset: u32,
) {
    let _timing = detour_timing::enter();
    let probes = probes::armed();
    let args = DrawArgs::Indirect {
        buffer: buffer_for_args,
        offset: aligned_byte_offset,
    };
    let draw = GameDraw {
        context,
        entry: RawDrawKind::InstancedIndirect,
        args,
    };
    // SAFETY (all three calls): `context` is the live immediate context this entry was called on.
    let water = unsafe { packed_water_context(&context) };
    let scope = unsafe { probes::draw_scope(probes, &draw) };
    let _lake = bind_water_variant(water, scope.as_ref());
    let _bound = unsafe { probes::draw_bound(probes, &draw) };
    let detour = DRAW_INSTANCED_INDIRECT.get().expect("set before enable");
    // SAFETY: forwards the caller's arguments unchanged to the trampoline.
    let submit = |eye: Option<usize>| {
        probes::submit(
            probes,
            context,
            RawDrawKind::InstancedIndirect,
            eye.map_or(Route::AsIssued, Route::PerEye),
            args,
            || unsafe { detour.call(context, buffer_for_args, aligned_byte_offset) },
        )
    };
    if !indirect_per_eye(context, &submit) {
        submit(None);
    }
}

/// Re-issue a GPU-indirect draw once per eye with both viewport slots pinned to that eye's half,
/// reporting whether it did; `false` means the caller must submit the draw itself, once, unchanged.
///
/// An indirect draw carries its vertex/instance counts in a GPU buffer, so it can be neither
/// instance-doubled (the promotion [`draw_indexed_detour`] applies to a patched shader) nor
/// rewritten -- the arguments are submitted verbatim, twice. What the mod controls is the viewport,
/// and that is the whole defect: left alone, the draw inherits whatever the previous draw left
/// bound, which under the collapse is dominated by [`CollapseViewport::Full`]. Rasterising `cb0`'s
/// per-eye off-axis projection into the full double-wide viewport stretches the geometry 2x
/// horizontally about the target centre, and a 2x horizontal stretch is a 2x horizontal *motion*
/// gain -- the near terrain patches and foliage sliding across the screen as the camera turns.
/// Pinning each submission to its eye's half is the same deal the unpatched `DrawIndexed` path
/// takes: present and correctly sized in both eyes, with no parallax, since `cb0` stays eye 0's.
/// The exception is a pipeline carrying a pinned-eye shader (the terrain patch system's): for it
/// `cb13` is pinned to each eye in turn ([`Cb13EyePin`]), which gives each submission that eye's
/// view.
///
/// Declines outside the collapse and the G-buffer range (elsewhere there is no eye split to inherit),
/// inside a block-level per-eye re-issue (which has already pinned the viewport for this eye
/// deliberately -- the [`PER_EYE_REISSUE`] marker), and when the bound vertex shader is patched (it
/// writes `SV_ViewportArrayIndex` itself and would have to be routed, not pinned; that includes the
/// tessellated terrain families when `single_pass.terrain` transforms them).
///
/// The re-issue does not raise [`PER_EYE_REISSUE`] itself -- it drives no engine code, only the
/// trampolines, so nothing it calls can re-enter this module's detours.
pub(super) fn indirect_per_eye(context: *mut c_void, submit: &dyn Fn(Option<usize>)) -> bool {
    if !collapse_active() || !in_gbuffer_range() || per_eye_reissue_eye().is_some() {
        return false;
    }
    if !config_flags().has(SinglePassSwitch::IndirectPerEye)
        || BOUND_VS_PATCHED.load(Ordering::Relaxed)
    {
        INDIRECT_FORWARDED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    // Without a recorded full viewport `ensure_collapse_viewport` is a no-op, so the loop would submit
    // the draw twice into one region -- doubled geometry rather than a split.
    if viewport_record::scene_viewport().is_none() {
        INDIRECT_FORWARDED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    let pin = Cb13EyePin::begin(context);
    for eye in 0..2 {
        ensure_collapse_viewport(context, CollapseViewport::Eye(eye));
        if let Some(pin) = &pin {
            pin.pin(eye);
        }
        submit(Some(eye));
    }
    drop(pin);
    // Unconditional, as on every other path that pins a viewport: a leaked eye-half would clip the
    // rest of the frame -- including the fullscreen passes -- to half the target.
    ensure_collapse_viewport(context, CollapseViewport::Full);
    INDIRECT_REISSUED.fetch_add(1, Ordering::Relaxed);
    true
}

/// GPU-indirect draws [`indirect_per_eye`] re-issued, and those it saw in a collapsed G-buffer range
/// but forwarded once (the flag off, or a patched shader bound). Reset per pass range by
/// [`log_draw_split`], which reports them.
pub(super) static INDIRECT_REISSUED: AtomicUsize = AtomicUsize::new(0);
pub(super) static INDIRECT_FORWARDED: AtomicUsize = AtomicUsize::new(0);

// ---- Already-instanced draws ---------------------------------------------------------------------

/// Detour on `DrawIndexedInstanced`, handling the already-instanced case (see
/// [`DRAW_INDEXED_INSTANCED`]) with a per-eye re-issue and measuring how much of the frame it covers.
///
/// A call is in that case when a patched vertex shader is bound (so the shader reads `SV_InstanceID` as
/// an eye parity it did not ask for), the render thread is inside the G-buffer range, and the collapse
/// is on -- minus the mod's own draws. Promoted `DrawIndexed`es come through the trampoline and never
/// reach here; draws a per-eye re-issue re-drives are excluded by [`per_eye_reissue_eye`], since those
/// already land in one eye deliberately. Everything else is forwarded verbatim, once.
pub(super) unsafe extern "system" fn draw_indexed_instanced_detour(
    context: *mut c_void,
    index_count_per_instance: u32,
    instance_count: u32,
    start_index: u32,
    base_vertex: i32,
    start_instance: u32,
) {
    let _timing = detour_timing::enter();
    let probes = probes::armed();
    let args = [
        index_count_per_instance.into(),
        instance_count.into(),
        start_index.into(),
        base_vertex.into(),
        start_instance.into(),
    ];
    let draw = GameDraw {
        context,
        entry: RawDrawKind::IndexedInstanced,
        args: DrawArgs::Direct(args),
    };
    // SAFETY (all three calls): `context` is the live immediate context this entry was called on.
    let water = unsafe { packed_water_context(&context) };
    let scope = unsafe { probes::draw_scope(probes, &draw) };
    let _lake = bind_water_variant(water, scope.as_ref());
    let _bound = unsafe { probes::draw_bound(probes, &draw) };
    let detour = DRAW_INDEXED_INSTANCED.get().expect("set before enable");
    let submit = |route| {
        probes::submit(
            probes,
            context,
            RawDrawKind::IndexedInstanced,
            route,
            DrawArgs::Direct(args),
            || {
                // SAFETY: forwards the caller's arguments unchanged to the trampoline.
                unsafe {
                    detour.call(
                        context,
                        index_count_per_instance,
                        instance_count,
                        start_index,
                        base_vertex,
                        start_instance,
                    );
                }
            },
        )
    };
    let submit_promoted = |doubled: u32| {
        probes::submit(
            probes,
            context,
            RawDrawKind::IndexedInstanced,
            Route::Promoted,
            DrawArgs::Direct([
                index_count_per_instance.into(),
                doubled.into(),
                start_index.into(),
                base_vertex.into(),
                start_instance.into(),
            ]),
            // SAFETY: the caller's arguments with the instance count doubled, through the
            // trampoline so the mod's own draw is not seen again as a game draw.
            || unsafe {
                detour.call(
                    context,
                    index_count_per_instance,
                    doubled,
                    start_index,
                    base_vertex,
                    start_instance,
                )
            },
        )
    };
    if vegetation_once::active()
        && vegetation_once::draw_indexed_instanced(
            context,
            instance_count,
            &submit_promoted,
            &|eye| submit(Route::PerEye(eye)),
        )
    {
        return;
    }
    if active() && per_eye_reissue_eye().is_none() {
        INSTANCED_TOTAL.bump();
        let patched = BOUND_VS_PATCHED.load(Ordering::Relaxed);
        let in_range = in_gbuffer_range();
        if patched {
            let bucket = if in_range {
                &INSTANCED_RANGE_PATCHED
            } else {
                &INSTANCED_RANGE_OUT_PATCHED
            };
            bucket.bump();
        }
        if patched && in_range && collapse_active() {
            let flags = config_flags();
            let once = flags.has(SinglePassSwitch::HwInstancedOnce)
                && hw_instanced_once(context, instance_count, &submit_promoted);
            if once {
                record_instanced_case(instance_count, true);
                return;
            }
            let handled = flags.has(SinglePassSwitch::InstancedPerEye)
                && instanced_per_eye(&|eye| submit(Route::PerEye(eye)));
            record_instanced_case(instance_count, handled);
            if handled {
                return;
            }
        } else {
            record_instanced_bystander(patched, in_range, instance_count);
            if patched && !in_range && collapse_active() {
                // The shader writes `SV_ViewportArrayIndex` from instance parity whatever pass it is
                // in, but this one binds no eye-half pair -- so the odd instances need slot 1 to hold
                // the same region as slot 0 to appear at all.
                unify_viewport_slots();
            }
        }
    }
    submit(Route::AsIssued);
}

/// Draw a hardware-instanced draw once for both eyes through its layout's step-rate-doubled twin
/// ([`hw_instanced`]), reporting whether it did; `false` leaves the draw to the per-eye re-issue.
///
/// With every per-instance step rate doubled and the instance count doubled, instances `2k` and
/// `2k + 1` read the batch's element `k`, and the patched shader routes them to the eye their parity
/// picks, so the split viewport is all the draw needs. It declines without a live context or a
/// recorded full viewport (the split would be a no-op and both eyes would land in one region), when
/// a domain or geometry shader is bound (only the last stage before the rasteriser routes a draw),
/// when the doubled count overflows, and when the bound layout has no twin, which it logs once per
/// layout. The engine's layout is bound again afterwards, so its cached input-assembler state stays
/// true.
pub(super) fn hw_instanced_once(
    context: *mut c_void,
    instance_count: u32,
    submit: &dyn Fn(u32),
) -> bool {
    // SAFETY: `context` is the live immediate context the detoured entry was called on.
    let Some(ctx) = (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
    else {
        return false;
    };
    let Some(doubled) = instance_count.checked_mul(2) else {
        return false;
    };
    if viewport_record::scene_viewport().is_none() || vertex_shader_feeds_another_stage(ctx) {
        return false;
    }
    // SAFETY: reads the bound input layout of the live immediate context.
    let Ok(bound) = (unsafe { ctx.IAGetInputLayout() }) else {
        return false;
    };
    let Some(twin) = hw_instanced::twin_for(&bound) else {
        hw_instanced::note_untwinned(&bound);
        return false;
    };
    ensure_collapse_viewport(context, CollapseViewport::Split);
    // SAFETY: both layouts are live, and the engine's own layout is put back after the one draw.
    unsafe { ctx.IASetInputLayout(Some(&twin)) };
    submit(doubled);
    unsafe { ctx.IASetInputLayout(Some(&bound)) };
    hw_instanced::note_drawn_once();
    true
}

/// Re-issue an already-instanced draw once per eye, so the game's own instance ids stop being read as
/// an eye parity. Returns `false` when the re-issue cannot run (no full viewport recorded yet, no live
/// context, or no `cb13` contents to pin and restore), in which case the caller submits once and the
/// draw is counted as exposure.
///
/// For each eye, **both** `cb13` eye slots are filled with that eye's transforms and **both** viewport
/// slots with that eye's half of the double-wide target, so whichever parity `SV_InstanceID & 1`
/// produces -- and therefore whichever `SV_ViewportArrayIndex` the shader writes -- resolves to the
/// same eye. The instance count is left alone: per-instance vertex-buffer stepping is indexed by the
/// instance id, so doubling it would read past the batch's per-instance data.
///
/// `cb13` is shared by every patched shader in the pass, so its restore is unconditional: leaving it
/// pinned would collapse the rest of the frame's geometry to one eye. The viewport is put back exactly
/// as it was found rather than to the recorded full width, so a following draw that inherits the
/// viewport (an indirect draw that [`indirect_per_eye`] forwards) sees no trace of the pin.
pub(super) fn instanced_per_eye(submit: &dyn Fn(usize)) -> bool {
    let Some(full) = viewport_record::scene_viewport() else {
        return false;
    };
    let Some(d3d) = EngineContext::get() else {
        return false;
    };
    let Some(rows) = CB13.lock().live_rows() else {
        return false;
    };
    let saved = capture_viewport_slots(d3d);

    let mut submitted = 0;
    for eye in 0..2 {
        // The same marker the baked-cb re-issues raise: it keeps this module's own detours -- and any
        // block-level re-issue reached from here -- from compounding the split this loop sets up.
        let _reissue = PerEyeReissue::enter(eye, full);
        if !write_cb13_rows(d3d, &pin_rows_to_eye(&rows, eye)) {
            break;
        }
        bind_both_viewport_slots(d3d, eye_half_viewport(full, eye));
        submit(eye);
        submitted += 1;
    }

    if !write_cb13_rows(d3d, &rows) {
        // The buffer is still mapped, so there is no way to retry within this call. The warning is the
        // diagnostic signal: cb13 stays pinned to the last eye's rows for the rest of the pass.
        tracing::warn!(
            "cb13 restore failed after per-eye pin; geometry may render from one eye for the rest of the pass"
        );
    }
    restore_viewport_slots(d3d, saved);
    // A map failure before the first submission leaves the draw undrawn, so the caller must still
    // submit it; one after leaves it partially drawn, where a further submission would only duplicate
    // geometry in an eye that already has it.
    submitted > 0
}

/// Bind the packed water's pixel-shader variant for a draw on `water`, unless a probe's `scope` binds
/// the draw's pixel shader itself.
fn bind_water_variant<'a>(
    water: Option<&'a ID3D11DeviceContext>,
    scope: Option<&DrawScope>,
) -> Option<water_pixel_variants::Restore<'a>> {
    water
        .filter(|_| !scope.is_some_and(|scope| scope.binds_pixel_shader))
        .and_then(water_pixel_variants::bind)
}
