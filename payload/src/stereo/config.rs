//! Stereo rendering configuration. The live per-eye runtime state is [`crate::stereo::StereoState`].

use serde::{Deserialize, Serialize};

/// The single-pass stereo configuration: rendering both eyes in one geometry walk into a
/// double-wide target.
///
/// Its own struct because the feature is separable in both directions -- nothing here does
/// anything with `enabled` off, and the double-draw path works with all of it inert. Grouped, it
/// can be read or skipped as a unit, which is what a paused feature wants.
#[derive(Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct SinglePassConfig {
    /// Master switch for single-pass stereo (experimental; see `docs/mod/stereo/single-pass-stereo.md`).
    /// Instead of the double-draw (two full `game.Draw` walks, one per eye), render the G-buffer
    /// geometry once with the vertex shaders patched to emit both eyes via instancing +
    /// `SV_ViewportArrayIndex` routing into a double-wide render target. Off by default: the whole
    /// pipeline is under construction and validated against the double-draw oracle. Requires the DXVK
    /// viewport-routing capability (see [`crate::stereo::single_pass::capability`]); forced off if
    /// absent.
    pub enabled: bool,
    /// Requires [`single_pass`](Self::enabled): make the two eyes actually diverge. Fills `cb13` with **distinct** per-eye view-projections (slot 0 = eye 0, slot 1 =
    /// eye 1) instead of both = the current view, splits the bound viewport into left/right **halves**
    /// for `SV_ViewportArrayIndex` routing instead of two identical copies, and **doubles** the
    /// instance count of the G-buffer geometry draws (so `SV_InstanceID & 1` selects the eye). On its
    /// own -- without [`single_pass_double_wide`](Self::double_wide) and the collapse --
    /// this renders each eye into half of a per-eye-sized target (squished), so it is a bring-up /
    /// bisection step, not a finished look.
    pub dual_eye: bool,
    /// Requires [`single_pass_collapse`](Self::collapse) and
    /// [`vr.native_resolution`](crate::vr::VrConfig::native_resolution)): re-create the scene
    /// render targets at **2× per-eye width** so each eye's half is full resolution instead of a
    /// squished half of a per-eye target. Drives the engine render resolution
    /// ([`crate::vr::engine_render_resolution`]) via the same deferred `ApplyResize` the per-eye
    /// native-resolution path uses; the XR swapchain and per-eye capture textures stay per-eye width,
    /// so the collapse's capture split copies each full-width half straight into its eye texture.
    pub double_wide: bool,
    /// Requires [`single_pass_dual_eye`](Self::dual_eye): **collapse**
    /// the per-eye double-draw to a single `game.Draw` walk -- the actual draw-submission win. One
    /// walk produces both eyes (via the dual-eye `cb13` + viewport routing + instance doubling); the
    /// render camera stays centered, the between-eye snapshot/restore is dropped, and the capture
    /// splits the one back buffer into both eye textures. Works without
    /// [`single_pass_double_wide`](Self::double_wide) (each eye-half is then squished);
    /// with it, each half is full resolution. The riskiest step; last to enable during bring-up.
    pub collapse: bool,
    /// Census-only mode for [`single_pass`](Self::enabled): run the vertex-shader stereo rewrite
    /// on every shader at creation and tally the outcomes (patched / no per-eye references / errored)
    /// **without** substituting the patched bytecode, so rendering is unchanged. Safe to inject: it
    /// validates the DXBC rewriter against the game's real shader set and reports the true census in
    /// the debug UI, before the rest of the single-pass pipeline is wired up.
    pub patch_dryrun: bool,
    /// Record every vertex shader's name against its rewrite class and dump the result to the session
    /// directory on a shader reload.
    ///
    /// The reprojection allowlist is baked in and the census it was built from is committed to
    /// `docs/mod/stereo/single-pass-stereo.md`, so this is off by default. It earns a runtime flag rather
    /// than a rebuild because a census only sees the shaders created while it runs: an area that
    /// never loaded a family leaves that family out, and re-censusing is the way to catch one --
    /// which is a thing to do in a session, not a thing to recompile for. For the same reason it is
    /// not saved.
    #[serde(skip)]
    pub dump_vs_name_census: bool,
    /// Reproject the no-`cb0` scene-geometry families (skinned characters/NPCs, props, buildings,
    /// roads, ...) for single-pass, instead of leaving them double-drawn. When on, a vertex shader
    /// with no per-eye `cb0` operand whose name is on the reprojection allowlist is rewritten to
    /// post-multiply its own clip position by the per-eye `M_eye` (see
    /// [`crate::stereo::single_pass`]); NDC writers (sky, UI, post) are excluded by the allowlist.
    /// Requires [`single_pass`](Self::enabled); independent of the others so it can be A/B'd.
    pub reproject: bool,
    /// Extend [`single_pass_reproject`](Self::reproject) to the allowlisted families the
    /// `cb0` remap claims on a **camera-position** reference alone. The remap's candidacy test is
    /// "references one of `cb0[{4, 29..32}]`", but `cb0[4]` is a camera position a shader may read for
    /// a view vector or a distance fade while taking its clip from a baked matrix -- `generaljc3` reads
    /// it for a LOD fade and builds clip from `cb1[0..3]`. Claimed by the remap, such a family gets
    /// viewport routing but no per-eye clip, so under the collapse's centred render camera *both* eye
    /// halves are drawn from the centre viewpoint and the family sits at a rigid half-IPD offset from
    /// its surroundings -- visible in one eye, not only in stereo. This routes it to the reprojection
    /// it should have had, at no extra draw cost. Only ever moves a shader between two per-eye
    /// transforms: an unallowlisted family keeps the remap. Requires
    /// [`single_pass_reproject`](Self::reproject). On by default.
    ///
    /// The families it reaches in the shipped bundle are `generaljc3`, `landmark`, `layered` and
    /// `layeredblend` -- one shared body, confirmed from the bytecode rather than the name.
    ///
    /// Caveat worth an A/B: all four apply a depth bias *after* their projection
    /// (`o0.z += cb2[0].x · o0.w`), and that bias is folded into the clip position the reprojection
    /// then transforms by `M_eye`, rather than left as a post-projection offset. `M_eye` is near
    /// identity, so the bias survives approximately; if decal or shadow z-fighting appears on those
    /// families, this is the first thing to switch off.
    pub reproject_camera_only: bool,
    /// Single-pass the tessellated base terrain (VS → HS → DS): the vertex shader originates the eye
    /// index on the free `TEXCOORD3.z` lane, the hull shader forwards it, and the domain shader reads
    /// it to reproject its clip by the per-eye `M_eye` and route to the eye's viewport. Covers the
    /// `DrawIndexed` terrain passes (far/color/shadow); also gates the render-block re-issue that
    /// reprojects the GPU-indirect terrain-detail pass (see
    /// `payload/src/hooks/graphics_engine/terrain.rs`). Requires [`single_pass`](Self::enabled);
    /// independent so it can be A/B'd against the models.
    pub terrain: bool,
    /// Single-pass the far-distance tree impostors (`CTreeImpostorRB`): the impostor vertex shader
    /// writes its clip position from the global billboard view-projection and draws non-instanced, with
    /// no GPU-indirect path sharing it, so the reprojection rewrite plus instance-doubling covers it
    /// completely (unlike the other vegetation families, whose dominant draw is GPU-indirect). When on,
    /// the `treeimpostor*` vertex shaders take the same `M_eye` post-multiply as the reprojected scene
    /// families. Requires [`single_pass`](Self::enabled); independent so it can be A/B'd.
    pub tree_impostors: bool,
    /// Single-pass the tree-trunk/branch render block (`CRenderBlockBark`, "VegetationBark"). Its vertex
    /// shader reads a CPU-baked world-view-projection from `cb1` (not `cb0`) and draws via one of three
    /// kinds (plain, CPU-instanced, or GPU-indirect), so it can't ride the reprojection rewrite; instead
    /// the block's `Draw`/`DrawZ` is re-issued once per eye with the baked `cb1` reprojected by that eye's
    /// `M_eye`. While this is on, the `vegetationbark*` vertex shaders are also declined by the `cb0`
    /// remap, which would otherwise claim them on a `cb0[4]` camera-position reference that is not their
    /// position path. Requires the collapse; independent so it can be A/B'd. On by default: without it
    /// the trunks render at the centre view in both eyes, i.e. at zero disparity, which reads as them
    /// drifting in world space as the camera moves.
    pub bark: bool,
    /// Single-pass the grass/foliage render block (`CRenderBlockFoliage`, "VegetationFoliage"). Its vertex
    /// shader reads a baked view-projection from `cb2` (registers 4..7); the block's `Draw` and `DrawZ`
    /// are re-issued once per eye with that `cb2` copy reprojected by `M_eye`. The dominant grass path is
    /// GPU-indirect, so re-issue (not instance-doubling) is the only option. While this is on, the
    /// `vegetationfoliage*` vertex shaders are also declined by the `cb0` remap, which would otherwise
    /// claim them on the `cb0[4]` reference their wind-noise lookup makes. Does not address the separate
    /// forward-clustered-lighting black-in-VR issue. Requires the collapse. On by default, for the same
    /// reason as [`single_pass_bark`](Self::bark).
    pub foliage: bool,
    /// Single-pass the occluder depth-prime render block (`CRenderBlockOccluder`). Its non-instanced path
    /// bakes a world-view-projection into `cb1`; the block's `DrawZ` is re-issued once per eye with `cb1`
    /// reprojected by `M_eye`, so each eye's depth is primed with its own projection. Requires the
    /// collapse. **Blind-implemented, unvalidated.**
    pub occluder: bool,
    /// Re-issue an **already-instanced** draw once per eye under the collapse, instead of letting the
    /// patched vertex shader read the game's own instance ids as an eye parity. A `DrawIndexedInstanced`
    /// with a patched shader bound in the G-buffer range sends instance `i` to eye `i & 1` -- half the
    /// batch per eye, or the left eye alone at one instance. The instance count cannot simply be
    /// promoted (per-instance vertex-buffer stepping is indexed by the instance id, so doubling it reads
    /// past the instance data), so each eye gets its own submission with both `cb13` eye slots and both
    /// viewport slots pinned to that eye. Requires the collapse. On by default: it fixes a visible
    /// artifact (the buildings flickering), and costs ~130 extra draw submissions of ~20k per frame.
    pub instanced_per_eye: bool,
    /// Re-issue a **GPU-indirect** draw once per eye under the collapse, so it stops inheriting
    /// whatever viewport the previous draw left bound. `DrawIndexedInstancedIndirect` and
    /// `DrawInstancedIndirect` are how the near tessellating terrain patches
    /// (`CRenderBlockTerrainPatch` passes 56-57) and the foliage block submit, and neither entry point
    /// was detoured -- so those draws rasterised eye 0's per-eye projection into the full double-wide
    /// viewport, a 2x horizontal stretch that reads as the geometry sliding across the screen at twice
    /// the camera's rate. The counts live in a GPU buffer and cannot be doubled, so each eye gets its
    /// own submission with both viewport slots pinned to that eye's half. Like the unpatched
    /// `DrawIndexed` re-issue, the geometry is then present and correctly sized in both eyes but has no
    /// parallax. Requires the collapse. On by default: it fixes a confirmed visible artifact.
    pub indirect_per_eye: bool,
    /// Run the deferred clustered-lighting resolve once per eye under the collapse, each run masked to
    /// that eye's half of the double-wide target and reconstructing with that eye's own basis.
    ///
    /// The resolve is a fullscreen quad that rebuilds world positions from depth through the
    /// `ViewProjInv` the off-axis reconstruction (`hooks::graphics_engine::reconstruction`) substitutes,
    /// and samples the sun-shadow cascade over them. Collapsed, that single quad covers both eye halves
    /// while the basis describes one eye's frustum: the left half gets that eye's frustum compressed 2x
    /// horizontally and the right half a basis unrelated to it. The error is a function of the view
    /// matrix, so it moves as the camera turns -- the shadows slide across the screen instead of
    /// staying on the world. Splitting the pass is the only fix; one draw cannot carry two bases.
    ///
    /// Each run is masked with a **scissor**, not a half viewport, so the quad keeps its
    /// one-to-one NDC-to-pixel mapping and its G-buffer sampling stays correct; the half's basis comes
    /// from folding the full-target-NDC to eye-NDC remap into the substituted inverse. The whole render
    /// block is re-issued, so its light-assignment phase runs twice (identically) as well. Requires the
    /// collapse. On by default: sliding shadows are a worse defect
    /// than the extra light-assignment pass is a cost, and the flag remains for an A/B in the headset.
    pub reconstruct_per_eye: bool,
    /// The same per-eye split for the **atmospheric-scattering / aerial-perspective** pass, the other
    /// fullscreen consumer of the reconstruction basis.
    ///
    /// That pass reconstructs the whole screen from depth -- sky included -- and ray-marches the sun
    /// shadow cascade and aerial perspective over the reconstructed positions, so under the collapse it
    /// carries exactly the defect
    /// [`single_pass_reconstruct_per_eye`](Self::reconstruct_per_eye) describes, and
    /// splitting only the deferred resolve leaves this pass painting the same sliding error back over
    /// it. On by default, for the same reason.
    pub atmospheric_per_eye: bool,
    /// Bias the legacy (non-WaveWorks) water blocks' screen-space reflection/refraction lookup into
    /// each eye's half of the double-wide target, by re-issuing their `Draw` once per eye.
    ///
    /// The `Water*` family -- selected at the lower water-quality settings, in place of the
    /// `NvWater*` WaveWorks path -- does not sample its reflection, refraction, and depth buffers by
    /// pixel coordinate. Its block type stages a world→screen-UV matrix on vertex `cb1` once per
    /// pass (the view-projection with the NDC→UV `x·0.5 + w·0.5` already folded in), the vertex
    /// shader hands the result on as a projective `TEXCOORD1`, and the pixel shader divides by `w`.
    /// That UV is normalized over the **viewport**, i.e. over one eye's half, while the buffers it
    /// indexes are the whole double-wide target -- so each eye reads the entire two-eye image
    /// stretched across its water, and since the error is a fixed 2x scale it is a 2x motion gain
    /// too: the reflections slide over the water as the camera moves.
    ///
    /// The correction is one more bias per eye, `u' = (u + eye) · 0.5`, composed onto the rows the
    /// type staged -- no shader change, because the water vertex shaders take their clip position
    /// from the global `cb0` that the collapse already handles. It needs the per-eye re-issue only to
    /// know which eye it is for. Deliberately *not* reprojected by `M_eye`: the geometry still
    /// rasterizes from the collapsed centre view, so the UV must describe where it actually landed.
    ///
    /// Requires the collapse. On by default, but the hardest of the re-issues to A/B in practice: the
    /// affected family may not even be on screen at the water-quality setting in use, which makes a
    /// headset comparison the only way to tell the fix from a no-op, and unlike the other re-issues
    /// this one recomputes a constant the engine staged rather than transforming it in flight.
    pub water_uv_per_eye: bool,
    /// Give the WaveWorks water blocks (`NvWater*`) a per-eye view under the collapse, so the water
    /// surface has parallax instead of being one eye's view shown to both.
    ///
    /// These blocks are not affected by the projective-UV defect
    /// ([`single_pass_water_uv_per_eye`](Self::water_uv_per_eye)): their shaders derive
    /// the screen UV from `SV_Position` and the inverse screen size, which is already consistent with
    /// a double-wide target. Their defect is the other one — the vertex shader writes clip position
    /// from a baked model-view-projection in its own constant buffer rather than from the render
    /// context, so the collapse's per-eye machinery never reaches it and both eyes see the collapsed
    /// centre view. Flat water at the wrong depth reads as correct in a screenshot and wrong in a
    /// headset, which makes the defect easy to miss. On by default -- the flag stays so a suspected regression can be
    /// isolated without rebuilding.
    pub nvwater_per_eye: bool,
    /// Reproject the screen-space decal *box geometry* per eye, on top of
    /// [`single_pass_ssdecal_per_eye`](Self::ssdecal_per_eye), which fixes only where the
    /// decal reconstructs from.
    ///
    /// The block bakes a world-view-projection into its vertex constants, so like the water blocks its
    /// geometry never sees the collapse's per-eye transform: the decal lands on the right surface in
    /// both eyes but its screen coverage has no parallax. Separate flag because the reconstruction fix
    /// is the one that stops the sliding, and this one only adds depth to it. On by default.
    pub ssdecal_geometry_per_eye: bool,
    /// Give **geometry** drawn through the non-indexed `Draw` entry point (D3D11 context vtable slot
    /// 13) the same per-eye re-issue the indexed path gives its draws.
    ///
    /// Slot 13 is overwhelmingly how the fullscreen passes submit their triangle, so the collapse
    /// resets the viewport to the whole double-wide target for it. But the four decal blocks, the road
    /// layers, and the skidmarks submit ordinary world geometry non-indexed, and pinning the full
    /// viewport rasterises them across both eye halves, stretched 2x horizontally about the target
    /// centre. A 2x horizontal stretch is also a 2x horizontal *motion* gain, so they sweep across the
    /// screen at twice the camera's rate -- decals sliding over the world.
    ///
    /// Unlike the indirect and indexed paths this cannot be decided from the draw alone, since a
    /// fullscreen triangle and a decal box arrive identically; it is decided by an allowlist of passes
    /// known to carry geometry, derived by enumerating every caller of the two engine wrappers that
    /// reach the slot and taking pass membership from the pass-creation sites. On by default. The
    /// list stays an allowlist rather than a heuristic because misclassifying a fullscreen pass as
    /// geometry is a visibly wrong frame, while missing a geometry pass is only the status quo -- so
    /// a pass is added when it is evidenced, never on suspicion.
    pub slot13_per_eye: bool,
    /// Derive the collapse's eye-half viewports from the render target the engine currently has
    /// bound, rather than always from the scene's double-wide viewport.
    ///
    /// Several passes redirect their draws to a **reduced-resolution off-screen target**: the shared
    /// quarter-resolution buffer (half per axis) that the low-resolution clouds, the low-resolution
    /// particles, and the volumetric spot-light cones all render into, and the downsampled depth
    /// buffer. The collapse's viewport split is otherwise target-blind — it re-derives the halves from
    /// the recorded scene viewport before every draw in the range — so a draw into a half-sized target
    /// receives a viewport twice that target's dimensions. Its content is magnified 2x about the
    /// target's origin and cropped, and since the error is a fixed scale it is a 2x motion gain too:
    /// clouds and smoke sweep past at twice the camera's rate.
    ///
    /// The composes need no matching change. They stretch the whole low-resolution texture over the
    /// whole double-wide target with a baked UV, so a texture holding a left-half/right-half image
    /// lands as a left-half/right-half screen image on its own — and they must **not** be re-issued
    /// per eye, since both blend rather than overwrite.
    ///
    /// On by default. Everywhere except those passes the two records hold the same viewport and this is
    /// a no-op, so turning it off isolates it cleanly.
    pub collapse_viewport_follows_target: bool,
    /// Run the **SSAO** pass once per eye under the collapse, like the deferred resolve and the
    /// atmospheric scattering.
    ///
    /// SSAO is one of the seven depth-reconstruction passes (cross-reference
    /// `CMatrix4f::PerspectiveFovInverse`; the set is closed), so collapsed it reconstructs the whole
    /// double-wide target from one eye's basis. **Hazard:** unlike the two already split, it carries a
    /// temporal history it advances per invocation, so re-issuing the whole block double-advances
    /// state that is not idempotent, so the split saves and restores the history indices around the second
    /// run. On by default; turn it off if ambient occlusion looks wrong.
    pub ssao_per_eye: bool,
    /// Run the **screen-space reflection** pass once per eye under the collapse.
    ///
    /// Same reconstruction defect as [`single_pass_ssao_per_eye`](Self::ssao_per_eye).
    /// **Hazard:** SSR ray-marches a scene-colour capture taken earlier in the frame, so a second run
    /// would consume state the first already consumed -- which it does not: the block copies scene colour
    /// into its own target and writes nothing back, so a second run reproduces the capture. On by
    /// default.
    pub ssr_per_eye: bool,
    /// Run the **screen-space subsurface-scattering** (skin) pass once per eye under the collapse.
    ///
    /// Same reconstruction defect. This block calls the inverse **twice**, once per blur axis, so a
    /// per-eye split would have to mask both -- but they are in mutually exclusive branches, so exactly one
    /// fires per draw. On by default.
    pub subsurface_per_eye: bool,
    /// Apply the per-eye reconstruction basis to the **depth-of-field** post pass under the collapse.
    ///
    /// `DOFUtil::GetViewProjInverse` is the seventh and last consumer of the reconstruction basis. DoF
    /// is a post pass rather than a render block, so it may want the basis substituted rather than the
    /// pass re-issued: its compute prologue is basis-independent, so it runs once whole-target while the
    /// closing draw is split. On by default.
    pub dof_per_eye: bool,
    /// Give **screen-space decals** a per-eye block intercept under the collapse.
    ///
    /// `CRenderBlockSSDecal` is not a `PerspectiveFovInverse` consumer: its type-level `Setup` builds
    /// its own reconstruction basis inline and uploads it to fragment `cb1[0..3]`, and its pixel shader
    /// derives the depth-fetch UV *projectively* from its own clip position — normalized over one eye
    /// while the depth buffer is double-wide. So the decal reconstructs from the wrong surface, and the
    /// error moves with the camera.
    ///
    /// Both halves need fixing together: re-upload that eye's basis per eye, **and** bias the shader's
    /// projective UV into that eye's half of the buffer. Note the same `uv` feeds the reconstruction
    /// matrix (which wants the per-eye value) and the depth fetch (which wants the double-wide one), so
    /// they must stay separate. On by default.
    pub ssdecal_per_eye: bool,
    /// Requires [`single_pass_reconstruct_per_eye`](Self::reconstruct_per_eye), and rides on the
    /// off-axis froxel tile-bounds override: build the clustered
    /// (froxel) light grid **per eye** as well, instead of building it once with eye 0's projection
    /// against the double-wide tile count.
    ///
    /// The grid is a 64-pixel tile lattice over the framebuffer; collapsed, it is twice as wide as
    /// the frustum whose projection fills it, so every local light lands in the wrong tiles and the
    /// forward-lit families that sample it (foliage, glass, particles, blended materials -- ~20
    /// render-block types) are lit wrongly in both eyes. The resolve re-issue already runs the whole
    /// block once per eye; this makes each run's light assignment cover only that eye's half of the
    /// tile grid, with that eye's projection and tile bounds.
    ///
    /// The two halves compose because the assignment blend is commutative, the halves are disjoint,
    /// and the compaction phase is per-tile-local -- so long as the second run's whole-target clear
    /// is suppressed, which is what makes the grid end the frame valid for **both** eyes. That
    /// matters well outside the block: foliage samples the grid a frame early, in the G-buffer range.
    ///
    /// Declines itself (leaving the un-split behaviour) when the double-wide render width is not a
    /// multiple of 128, i.e. when the eye seam does not fall on a tile boundary.
    ///
    /// On by default: the un-split grid mislights every forward-lit family, and the flag remains so
    /// the two can be compared in the headset.
    pub clustered_per_eye: bool,
    /// Requires [`single_pass_clustered_per_eye`](Self::clustered_per_eye): also assign
    /// each eye's lights from **that eye's** position rather than from the collapsed (cyclopean)
    /// camera's.
    ///
    /// The light-assignment vertex shader transforms proxies that the CPU has already made relative
    /// to the render camera's world position, and under the collapse that is the centre head pose --
    /// the per-eye offset lives in the patched vertex shaders, not in the render context. Folding the
    /// eye's world offset into the translation row of the uploaded view matrix restores it. The
    /// per-eye display canting is *not* applied, only the positional offset.
    ///
    /// On by default, as the more faithful assignment. Whether the difference is visible at 64-pixel
    /// tile granularity is a judgement call, which is why it stays its own flag: turn it off to A/B
    /// against [`single_pass_clustered_per_eye`](Self::clustered_per_eye) alone.
    pub clustered_per_eye_light_view: bool,
    /// Keep both viewport slots bound to the same region outside the G-buffer range, so a patched
    /// vertex shader's `SV_ViewportArrayIndex = SV_InstanceID & 1` resolves to the same place whichever
    /// parity it computes. The rewrite writes that index unconditionally -- the bytecode cannot tell
    /// which pass it is in -- but only the G-buffer geometry ever binds an eye-half pair, and the
    /// collapse's per-draw split leaves the two slots holding *different* halves after the range ends.
    /// Until the next engine viewport bind, every odd-numbered instance of an already-instanced draw
    /// then rasterises into the other half. Slot 0 is never touched, so this can only make dropped
    /// instances reappear, never move what already rendered. Requires the collapse. On by default.
    pub uniform_viewport_slots: bool,
}

/// Stereo rendering toggles. The live per-eye runtime state is [`crate::stereo::StereoState`].
#[derive(Clone, Serialize, Deserialize)]
pub struct StereoConfig {
    /// Single-pass stereo; see [`SinglePassConfig`].
    pub single_pass: SinglePassConfig,
    /// Master switch: render the scene twice, once per eye.
    pub enabled: bool,
    /// Apply the per-eye IPD camera offset.
    pub cameras: bool,
    /// Which eye reaches the screen (debug A/B).
    pub present_eye_0: bool,
    /// Skip SetupRenderFrameData on eye 1 (experimental; normally inert).
    pub gate_setup_render_frame_data: bool,
    /// Skip HandBackBuffers on eye 1.
    pub gate_hand_back_buffers: bool,
    /// Zero the post-effect dt on eye 1 (so once-per-frame accumulators do not double-step).
    pub gate_eye1_dt: bool,
    /// Diagnostic: during a stereo frame, record each eye's per-pass GPU-op count and log a diff --
    /// which passes draw identically between eyes (replayable from one walk) and which diverge (the
    /// per-eye special-casing burden). The feasibility probe for single-pass stereo; see
    /// [`crate::debug::stereo_diff`]. Logs on target `"stereo_diff"`.
    pub diagnose_stereo_draw_diff: bool,
    /// Diagnostic: hash a curated set of engine render targets after each eye's Draw and record the
    /// per-eye hashes into the active render trace. Run with `cameras` off (both eyes share one
    /// camera) so any RT whose two eyes' hashes differ is being accumulated across the two Draws --
    /// the "stronger in one eye" bug. See [`crate::debug::rt_hash`].
    pub diagnose_rt_hashes: bool,
    /// Diagnostic: while a render trace collects, encode eye 0's final `BackBufferLinear` to a PNG each
    /// frame into the trace's `traces/<stamp>/` folder (alongside `trace.ndjson`), named by frame index
    /// so the sequence reassembles and aligns 1:1 with the per-frame trace events. For localizing
    /// per-frame visual artifacts (e.g. the shadow flicker) the numeric trace cannot place spatially.
    /// Heavy (a full readback + PNG encode per frame); only meaningful during a manual trace.
    pub diagnose_rt_screenshots: bool,
    /// Diagnostic: while tracing, record the MainColor mean at the fixed pipeline stage brackets
    /// (after the deferred resolve, around the aerial-perspective composite, at the post block
    /// entry, and at the post-chain start). Bracketing a global change between two stages names the
    /// frame segment that injects it. Each sample drains the GPU, so traces run slower.
    pub diagnose_main_color_means: bool,
    /// Diagnostic: while tracing, record the MainColor mean after every late-scene render pass
    /// (`RP_SKY_GRADIENT` through `RP_PARTICLE_ONSCREEN`, eye 0, every 2nd frame) -- the per-pass
    /// ladder that walks a global change to the pass that injects it. Much heavier than the stage
    /// brackets.
    pub diagnose_pass_sweep: bool,
    /// Diagnostic: while tracing, dump the render engine's FP/VP global constant staging blocks at
    /// the scene composite and at frame end, for good-vs-bad frame diffing.
    pub diagnose_global_constants: bool,
    /// Automatically start a render trace when the raw-brightness histogram mid-point (the
    /// auto-exposure divisor) jumps against its recent median -- the CPU-visible signature of the
    /// latched global-brightness step, which lags the visual flip by only a few frames. Saves
    /// having to time a manual capture to a spontaneous flip. An arm flag the capture clears when
    /// it fires, so it is not saved.
    #[serde(skip)]
    pub auto_trace_brightness_step: bool,
    /// Diagnostic: skip the SSAO pass on both eyes in stereo, to confirm whether SSAO drives the
    /// "stronger in one eye" darkening. (Equivalent to lowering the in-game AO setting, but toggleable
    /// live.)
    pub disable_ssao: bool,
    /// Experiment: skip the SSAO pass on the second eye only, so the first eye's screen AO is absent
    /// from the second. A crude test of whether the AO asymmetry is the artifact (a real shared-AO fix
    /// needs reprojection, not omission).
    pub ssao_eye0_only: bool,
    /// Diagnostic: restore the `RenderEngine` back-buffer copy ring index (`m_CurrBackBufferCopy`)
    /// between the two stereo eyes. This ring advances once per `Draw` and is *not* one of the engine
    /// frame counters the between-eye restore rewinds, so the two eyes
    /// otherwise land on different back-buffer copies -- any pass that reads the previous copy then
    /// sees different data per eye. Test whether pinning it converges the per-eye MainColor.
    pub restore_cb_ring: bool,
    /// Diagnostic: skip the screen-space reflections pass (`RP_SCREEN_SPACE_REFLECTIONS`) on both eyes.
    /// SSR reads a previous-frame scene-color capture that is regenerated every `Draw`, so eye 1 reads
    /// what eye 0 just wrote -- a content-based per-eye divergence no counter restore can fix. If
    /// dropping SSR converges the per-eye MainColor, the SSR feedback is the source.
    pub skip_ssr: bool,
    /// Diagnostic: skip the global-illumination pass (`RP_GLOBAL_ILLUMINATION`) on both eyes. GI can
    /// carry a temporal/probe history that differs per eye; a companion to [`skip_ssr`](Self::skip_ssr)
    /// for isolating the residual per-eye MainColor divergence that survives SSR-off and SSAO-off.
    pub skip_gi: bool,
    /// Diagnostic: skip the AO-volumes pass (`RP_AO_VOLUMES`) on both eyes. AO volumes are
    /// artist-placed darkening volumes rendered as depth-tested proxy geometry; a volume whose proxy
    /// faces are borderline against nearby geometry can flip its entire contribution on a sub-pixel
    /// depth shift, so the temporal jitter cycles it -- the prime suspect for the blob-scale
    /// "shadows flicker in and out" artifact in MainColor (issue #10's residual flicker).
    pub skip_ao_volumes: bool,
    /// Diagnostic: skip the [`skip_pass_range`](Self::skip_pass_range) passes on both eyes. A
    /// separate flag (rather than an `Option` around the range) so the range can be preset while
    /// disarmed -- dragging the bounds live sweeps through intermediate ranges, some of which are
    /// unsafe to skip. Armed for one session at a time, so it is not saved.
    #[serde(skip)]
    pub skip_pass_range_enabled: bool,
    /// Diagnostic: the inclusive render-pass index range `[start, end]` to skip while
    /// [`skip_pass_range_enabled`](Self::skip_pass_range_enabled), for bisecting which pass an
    /// artifact originates in ([`RenderPassId`](jc3gi::graphics_engine::render_engine::RenderPassId)
    /// maps every index; GBuffer `RP_Z_OCCLUDERS`..`RP_LAST_GBUFFER`, lighting/main
    /// `RP_REFLECTIVE_WATER_PLANES`..`RP_POSTEFFECTS`).
    pub skip_pass_range: (i32, i32),
    /// Patch the screen-space PCF rotation hash out of the sun-shadow shaders at creation, so both
    /// eyes use the same unrotated 38-tap PCF (removes the per-eye shadow shimmer + foliage grain).
    /// Applies only to shaders created after the hook installs; trigger a shader reload (e.g. change
    /// shadow quality) if injected mid-session. See [`crate::hooks::graphics_engine::shader`].
    pub patch_shadow_pcf_hash: bool,
    /// Close a leaked G-buffer range from the **draw thread**, in the dispatch prologue
    /// (`RenderEngine::PreDraw`), instead of from the game thread at frame start.
    ///
    /// Diagnostic: disable the sun-shadow system entirely through the engine's own settings path
    /// (`CShadowManager` enabled flag, synced by the sim-side `UpdateRender` via `SetEnabled`). The
    /// sharpest shadow-pipeline discriminator: an artifact that survives with no shadows at all
    /// cannot be shadow data.
    pub disable_sun_shadows: bool,
    /// Diagnostic: freeze the sun-shadow atlas by re-clearing the pass-enable flags after
    /// `CommitRenderPassSettings` sets them, so no shadow pass renders and the atlas keeps its last
    /// contents. Shadows stay visible but stop updating: an artifact that survives the freeze is in
    /// the shadow *sampling*; one that dies with it is in the atlas *contents*.
    pub freeze_shadow_maps: bool,
    /// Widen the spawn system and character occlusion BFBC frustums to cover both eyes. Both
    /// `CSpawnSystem::Update` and `CGameWorldObjectManager::ProcessCharacterOcclusion` build their BFBC
    /// cull frustums against the camera manager's active camera (not the occluder manager's cull camera),
    /// so the scene-cull widen does not reach them. This widens the active camera's projection for those
    /// calls (with a full save/restore so the per-eye render projections are untouched), so the spawn
    /// visibility gate and character occlusion cull account for the combined VR eye frusta. VR only.
    pub widen_spawn_cull: bool,
    /// Scale factor for spawn system budget limits. The spawn system enforces per-resource-def max
    /// object counts (loaded from `settings/spawn_budget_pools.bin`); when the count is exceeded it
    /// despawns the highest-rank object, which under VR's wider FOV can pop vehicles/NPCs in front of
    /// the player. This multiplies every budget entry by the given factor (e.g. `2.0` doubles all
    /// limits) via a one-time patch on first `CSpawnSystem::Update` call (auto-reverted on uninject).
    /// `1.0` leaves them unchanged. Read once at startup; requires restart to change.
    pub spawn_budget_scale: f32,
    /// Refresh every sun-shadow cascade every frame, defeating the engine's update-pattern amortisation.
    /// The engine re-fits and re-renders cascade level L only every `2^L` frames (via
    /// `ShadowManager::m_CascadeUpdateLevels`), copying each cascade's fit forward between refreshes, so
    /// the coarse cascades hold still then snap to a new texel alignment periodically. Flatscreen's T2X
    /// averages those snaps away, but the mod forces SMAA 1x (no temporal history), so they surface as
    /// sun-shadow flicker (issue #31). Zeroing the per-cascade levels forces all cascades to update every
    /// frame -- smooth, at the cost of redrawing the coarse cascades each frame. VR only.
    pub shadow_update_every_frame: bool,
    /// Diagnostic (issue #31 isolation, Test A): render each eye through a *symmetric* (zero-shear)
    /// off-axis frustum instead of the true asymmetric HMD one. The replacement preserves each eye's
    /// horizontal and vertical FOV *extent* but re-centres it, so the off-centre shear terms
    /// (`(tl+tr)`, `(td+tu)`) go to zero -- reproducing the known-good symmetric-stereo projection
    /// *inside* the full VR path (per-eye offset, double-draw, and the off-axis depth reconstruction all
    /// still run). The flicker occurs only under the asymmetric off-axis projection, not symmetric
    /// stereo, so this is the direct discriminator: if the flicker dies with the shear removed, the
    /// shear is the amplifier; if it survives, the reconstruction inverse (or another projection oddity)
    /// is. The headset image will look slightly stretched (the compositor still composites at the true
    /// asymmetric FOV) -- diagnostic only. VR only; no-op on flatscreen. **Default off.**
    pub symmetrize_eye_frusta: bool,
    /// Diagnostic (issue #31 isolation, Test B): render *both* eyes with eye 0's exact projection and
    /// world offset, so the two dispatches draw the identical off-axis view (mono in stereo). This
    /// removes all per-eye divergence while keeping the off-axis projection and its depth reconstruction,
    /// isolating "the two eyes disagree" from "each single off-axis frame wanders frame to frame": if
    /// the flicker survives two identical off-axis draws, it is not inter-eye divergence at all. VR only;
    /// no-op on flatscreen. **Default off.**
    pub mirror_eye0_to_both: bool,
    /// The multiplier for the terrain detail-tessellation GPU budget buffers — the fix for the
    /// world-locked black cliff-wall/cave-ceiling tiles (issue #40). The detail rock skin is built
    /// by a GPU pipeline that allocates vertices/indices/texels from fixed-size buffers with
    /// unbounded cursors; whatever overflows is silently dropped and the losing tiles render
    /// black. The shipped sizes fit the flatscreen FOV; VR's wide FOV admits more quads and
    /// oversubscribes them. The buffer-size immediates in the setup types' `Create` functions are
    /// patched to `shipped * scale` (auto-reverting on uninject) and the two setup types are
    /// re-created, automatically at the first frame start after injection and on demand from the
    /// debug UI. `1` leaves the shipped sizes (and skips the automatic apply). VR and flatscreen;
    /// **default 4.**
    pub terrain_detail_budget_scale: u32,
}
impl SinglePassConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            dual_eye: false,
            double_wide: false,
            collapse: false,
            patch_dryrun: false,
            dump_vs_name_census: false,
            reproject: false,
            reproject_camera_only: true,
            terrain: false,
            tree_impostors: false,
            bark: true,
            foliage: true,
            occluder: false,
            instanced_per_eye: true,
            indirect_per_eye: true,
            reconstruct_per_eye: true,
            atmospheric_per_eye: true,
            water_uv_per_eye: true,
            nvwater_per_eye: true,
            ssdecal_geometry_per_eye: true,
            slot13_per_eye: true,
            collapse_viewport_follows_target: true,
            ssao_per_eye: true,
            ssr_per_eye: true,
            subsurface_per_eye: true,
            dof_per_eye: true,
            ssdecal_per_eye: true,
            clustered_per_eye: true,
            clustered_per_eye_light_view: true,
            uniform_viewport_slots: true,
        }
    }
}

impl StereoConfig {
    pub const fn new() -> Self {
        Self {
            single_pass: SinglePassConfig::new(),
            enabled: true,
            cameras: true,
            present_eye_0: false,
            gate_setup_render_frame_data: false,
            gate_hand_back_buffers: false,
            gate_eye1_dt: true,
            diagnose_stereo_draw_diff: false,
            diagnose_rt_hashes: false,
            diagnose_rt_screenshots: true,
            diagnose_main_color_means: false,
            diagnose_pass_sweep: false,
            diagnose_global_constants: false,
            auto_trace_brightness_step: false,
            disable_ssao: false,
            ssao_eye0_only: false,
            restore_cb_ring: false,
            skip_ssr: false,
            skip_gi: false,
            skip_ao_volumes: false,
            skip_pass_range_enabled: false,
            skip_pass_range: (
                jc3gi::graphics_engine::render_engine::RenderPassId::RP_REFLECTIVE_WATER_PLANES
                    as i32,
                jc3gi::graphics_engine::render_engine::RenderPassId::RP_REFLECTIVE_WATER_PLANES
                    as i32,
            ),
            patch_shadow_pcf_hash: true,
            disable_sun_shadows: false,
            freeze_shadow_maps: false,
            widen_spawn_cull: true,
            spawn_budget_scale: 2.0,
            shadow_update_every_frame: false,
            symmetrize_eye_frusta: false,
            mirror_eye0_to_both: false,
            terrain_detail_budget_scale: 4,
        }
    }
}
