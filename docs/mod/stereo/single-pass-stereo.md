# Single-pass stereo

Multipass stereo walks the render-pass list once per eye. The frame is bound by draw-call submission rather than by the GPU ([performance.md](../performance/performance.md#what-limits-the-frame)), so single-pass collapses the two geometry walks into one and feeds the GPU from a single submission. This page covers the technique, the shader rewrites, and the payload integration. The rest of the material:

- [single-pass-collapse-defects.md](single-pass-collapse-defects.md): what goes wrong when a pass runs once over the double-wide target, and the correction for each;
- [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md): how a draw covering both eyes gets each eye's camera, lights, screen position, and motion, and what changes under asymmetric and canted eyes;
- [single-pass-render-blocks.md](single-pass-render-blocks.md): the render blocks that need their own intercept, and which draws are still submitted once per eye;
- [single-pass-validation.md](single-pass-validation.md): comparing single-pass with the isolated eyes and multipass.

Single-pass is the default while a VR session runs. The debug UI's single-pass section (the Experimental page) has one button that turns the whole feature off or on (`jc3vr_view::stereo::config::set_single_pass_feature`), and the choice is saved.

## Turning on and off

The feature is live when its master switch is on, a VR session is running, and the device reports the viewport-routing capability (`jc3vr_render::stereo::single_pass::active`). The session state is latched at the frame start's configuration refresh, so a session that starts or stops changes every reader of the next frame at once. Without a session (on the desktop, with the runtime missing, or with the headset disconnected) the game renders as it ships.

The rewrites are applied when the engine creates a shader, so the live shader set has to be re-created whenever that state changes. The frame driver (`payload/src/hooks/game.rs`) compares it each frame with the state the live set was built under (`jc3vr_render::hooks::graphics_engine::shader::reconcile_single_pass`) and requests a reload, which runs in the same frame before the draws. That brings single-pass up after a late injection and on the first frame of a session with nothing toggled; a session that stops, and an eject (`restore_original_shaders_on_eject`), take the shaders back to the pristine set. The double-wide target follows from the same state the next time the resolution driver runs.

Switching single-pass off writes the group's switches as `false` under `[stereo.single_pass]` ([settings.md](../settings.md)). A file from before the format carried a version held the group off as it then shipped, so its upgrade drops the group's `false` values.

## The technique

Scene geometry is instance-doubled, rendered into a double-wide target, and routed to each eye's half by viewport index. The fullscreen passes run once over the whole target, and the ones that need an eye take it inside the draw.

- Routing. A vertex shader writes `SV_ViewportArrayIndex` directly, through the D3D11.3 `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` capability, which DXVK reports when the Vulkan `shaderOutputViewportIndex` feature is present. No geometry shader is involved. `jc3vr_view::stereo::layout::probe` checks the capability, and the feature stays inert without it.
- Double-wide rather than a texture array. A two-slice array would force the several hundred pixel shaders that sample the scene targets as `Texture2D` to be retyped. With a double-wide target the engine's own `CreateRenderSetups` re-runs at a doubled width, `SetRenderSetup` synthesises the viewport from the target size, and no SRV type changes.
- Instance doubling. A patched draw is issued with twice its instances. The vertex shader takes `eye = SV_InstanceID & 1`, picks that eye's rows out of `cb13`, and routes to its half.

What makes the retrofit tractable is that JC3 has one scene view-projection, [`RenderContext::m_OffsetViewProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_OffsetViewProjection), uploaded to the global vertex constants (`cb0[29..32]`, with the camera position at `cb0[4]`; staged in [`RenderEngine::m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData)). Multipass already renders correct stereo, so every change has a per-pass oracle.

### The routed range

There is only one walk, so the range that routes to the eyes is the whole camera scene, `first >= RP_Z_OCCLUDERS`, which excludes the shadow and reflection prepasses that reuse the same shaders under another view. It includes the water planes, the sky, and the transparents, because there is no second walk to render them in.

One pass range cannot separate those from the full-width lighting and post passes interleaved with them, so the viewport split happens per draw. `rs_set_viewports_detour` only records the engine's viewport, and `ensure_collapse_viewport` binds the eye halves for a patched draw, one eye's half twice for an unpatched draw re-issued per eye, and the full width for the non-indexed `Draw` the fullscreen passes use. Without that last reset the lighting pass would inherit the half the previous geometry draw left and light one eye only.

The viewport record (`viewport_record`) holds the last scene-sized viewport, the engine's latest bind of any size, and what the two slots hold now. Every bind writes it, the engine's through the detour and the mod's through the trampoline, and a sequence lock makes the per-draw read a few plain loads. With `single_pass.viewport_bind_cache` a draw binds nothing when the slots already hold what it wants; the pair is forgotten at every pass boundary, so a change no detour saw cannot outlive its pass.

[`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask) calls [`RenderEngine::DrawRenderPassRange`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawRenderPassRange) three times per dispatch (from `DrawGBuffer`, `Draw`, and `DrawPosteffects`), and all three qualify, so the range is entered and left three times. Anything that must run once per frame hangs off `single_pass::begin_frame`, not a range boundary.

The range flag is raised, read, and lowered on the draw thread. The safety clear for a range left open by an interrupted dispatch runs in the `PreDraw` prologue (`single_pass::begin_dispatch`), the one point in that thread's sequence where no range can be live. It cannot run on the game thread at the top of [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender): with the frame tail deferred (`defer_tail`) the draw thread is still walking the previous frame there, and a clear mid-range would drop every later draw out of the eye split, so whole geometry families would blink. The flags the draw thread pins for its dispatch end with the dispatch (`single_pass::end_dispatch`), because the engine's job-pool workers run other jobs (the planar reflection camera's update, for one) that read them.

## The vertex-shader transform

The per-eye data in the position path is five `cb0` rows: `cb0[4]` (the camera position) and `cb0[29..32]` (the offset view-projection). The transform binds a mod-owned constant buffer at the free slot `cb13`, holding both eyes' five rows (`[eye0: 0..4][eye1: 5..9]`), and rewrites the shader:

1. add an `SV_InstanceID` input and an `SV_ViewportArrayIndex` output, taking the new registers from the signatures (fxc keeps an ISGN entry for an input the shader never reads and emits no declaration for it, so a declaration scan under-counts);
2. declare `cb13` dynamically indexed and add one temp;
3. in a prologue, compute the eye from the instance id, write the viewport index, and turn the eye into a row base;
4. rewrite every operand on `cb0[4]` or `cb0[29..32]` to `cb13[rBase.x + k]`, a uniform remap for every shader of the idiom;
5. add the `SFI0` chunk (or set its bit) that declares the viewport-index requirement, without which the output is invalid, and fix the lengths and the checksum (`dxbc_stereo::refresh_checksum`). `RDEF` and `STAT` are not updated, since DXVK reads neither.

The patch runs in a `CreateVertexProgram` hook before `CreateVertexShader` copies the bytecode ([shader-patching.md](../rendering/shader-patching.md) lists every creation-time patch). A fxc-compiled reference shader that writes the viewport index, `crates/dxbc-stereo/tests/data/ref_vs50.dxbc` (built with `scripts/dxbc.sh compile`), pins the encodings byte for byte.

Referencing `cb0[4]` does not mean taking the clip position from it. Shaders read the camera position for shading, a distance fade, or a world-space lookup while taking clip from a baked matrix; a shader claimed that way gets routing and doubling but both halves are drawn from the centre, at a rigid half-IPD offset from everything around it. [single-pass-render-blocks.md](single-pass-render-blocks.md) covers the families claimed on `cb0[4]` alone and how each is handled. A shader that reads the view-projection rows but subtracts `cb0[9]` (the render camera's position) rather than `cb0[4]` is covered under "The road surfaces' clip origin, per eye" in [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md).

`dxbc-stereo`'s corpus tests run every rewrite over all the game's vertex shaders and check each output structurally, and `scripts/dxbc.sh disasm` feeds results to Microsoft's `D3DDisassemble` under wine. The extract is game-derived and git-ignored; a missing extract fails the corpus tests unless `JC3VR_ALLOW_MISSING_SHADER_CORPUS=1`.

## Reprojection

Everything outside the remap writes clip from a CPU-baked copy of the same view-projection. Every scene shader computes `clip_center = VP_center · world`, so

    clip_eye = VP_eye · world = (VP_eye · VP_center⁻¹) · clip_center = M_eye · clip_center

and post-multiplying the shader's own `SV_Position` by a per-eye `M_eye` lands it in each eye. Built from the full, translation-bearing view-projections, `M_eye` carries the IPD parallax, so the camera-relative idiom folds in. It is near the identity, so reverse-Z depth survives it; `VP_center` is inverted in f64 because it is near-singular.

`dxbc_stereo::reproject_vertex_shader` reuses the remap's input, output, prologue, and fix-ups. It renames every write of the `SV_Position` output to a fresh temp (output registers are write-only, and the rename absorbs masked writes) and emits `o0 = M_eye · rClip` as four `dp4`s before each `ret`, reading `M_eye` from `cb13` after the remap rows. A shader with a conditional return is rejected. After the `M_eye` block, `cb13` carries a selector row (set in a dual-eye upload, zero in a mirror), each eye's full view-projection, and each eye's clip origin (`dxbc_stereo::CLIP_ORIGIN_ROW_BASE`).

### Exact only in real arithmetic

`M_eye · clip_center` equals `VP_eye · world` in real arithmetic, but rounds twice where an isolated eye rounds once: a few floats of depth on every reprojected surface, a code of albedo here and there, and alpha-tested edges that flip. Where a shader's math is a position times a view-projection, the exact form is to hand it the eye's own rows, which is why these read the eye's rows rather than being reprojected:

- the remapped families (the eye's offset view-projection);
- the terrain domain shaders, whose terrain constants hold a copy of the offset view-projection and camera position; `dxbc_stereo::rebase_domain_shader` and `rebase_pinned_domain_shader` choose between the eye's `cb13` rows and the shader's own by the selector row, so the reflection passes that reuse them are unchanged;
- the readers of the full view-projection in `cb0[0..3]` (`treeimpostor*`, `lightsource*`, `aobox`, and the non-tessellated terrain base mesh), through `dxbc_stereo::remap_full_view_projection` and its pinned twin;
- the terrain-detail block's per-eye re-issue, which builds each eye's matrix from that eye's `cb13` rows through the engine's own [`Matrix4::Multiply4x4`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::math::Matrix4##func-Multiply4x4).

What stays reprojected has no exact form in the shader: the baked world-view-projection families (`general*`, `character*`, `building*`, `prop*`, the bark), the foliage, the moon, the water planes, and the decals.

### The allowlist

Reprojection is shader-agnostic, so it would equally transform sky, UI, and fullscreen shaders that write raw NDC, which `M_eye` corrupts, and the bytecode cannot tell them apart. Substitution happens at creation, before the pass is known, so it is gated on a positive allowlist of scene families keyed on the shader name (`REPROJECT_NAME_PREFIXES` and the tables beside it in `jc3vr_render::stereo::single_pass::shader_policy`). Unknown shaders are drawn per eye, which is correct but slower. The terrain and vegetation shaders are off it because they have their own paths, `terraindetailrt*` because its block intercept needs it pristine, and three scene models of unclear family (`box`, `notex`, and `lrpc`) until someone looks at them.

## The terrain tessellation path

The tessellated base terrain builds clip in the domain shader from its own `cb1` copy of the offset view-projection. The eye rides through the pipeline on the free `.z` of the `TEXCOORD3` interpolant: the vertex shader writes it (`inject_eye_forward_vertex_shader`), the hull shader's control-point phase forwards it (`forward_eye_hull_shader`, leaving the fork and join phases alone since they reuse the same registers for other values), and the domain shader reads it, writes `SV_ViewportArrayIndex`, and reads its camera rows from that eye's `cb13` slot, which is bound on the domain stage. Hull and domain shaders are gated structurally, and shadow permutations are excluded by name, an unnamed shader counting as a shadow pass, since failing open would eye-transform a shadow-atlas draw.

### The pinned-eye reprojection

The patch system ([`RenderBlockTerrainPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch)) has two pipelines the lane cannot reach: its tessellated permutations write all four lanes of `TEXCOORD3`, and its GPU-indirect base-mesh permutations already read `SV_InstanceID` and project through `cb0[0..3]`. Both are submitted once per eye. The creation hooks rewrite those shaders to read the first `cb13` eye slot with no instance input or viewport output (`dxbc_stereo::rebase_pinned_domain_shader`, `remap_pinned_full_view_projection`, and `reproject_pinned_eye` for a shader of neither shape) and tag the D3D object with a private-data marker. Both per-eye re-issues check the bound shaders for the marker and pin both `cb13` slots to the eye being drawn (`jc3vr_render::stereo::single_pass::pinned_eye`), then restore the dual-eye rows. The tessellation factors and patch selection stay centre-derived, as in an isolated eye. `single_pass.terrain_patch_per_eye` (with `single_pass.terrain`) gates it.

### The patch prep, from the centre camera in every mode

The patch system's GPU prep passes (`RP_TERRAINPATCH_CLEAR` through `RP_TERRAINPATCH_ENUMERATION`) rebuild the patches' detail and morph targets from the camera position row of the vertex globals. In multipass or an isolated eye that would be the eye's, so each eye would morph the ground from its own position, a per-patch depth difference with opposite signs in the two eyes. `jc3vr_render::stereo::terrain_prep` stages the centre camera for those passes in every dispatch that renders one eye, which makes the two multipass eyes agree with each other and with single-pass.

## Payload integration

`jc3vr_render::stereo::single_pass` holds the gates, the routed-range guard, and the install and teardown that own every detour's lifetime; its submodules are named for what they own (`shader_policy`, `cb13`, `viewport_record`, `draw_detours`, `per_eye_reissue`, and one per correction). `config_snapshot` publishes the per-frame and per-dispatch flags to `jc3vr_view::stereo::layout` for code outside single-pass, and `instanced_exposure`, `frame_diagnostics`, and `detour_timing` only measure. The capture harness reaches the draw path through the render crate's `probes` table ([architecture.md](../../architecture.md), "Crate layering").

Besides the engine hooks named on these pages, single-pass detours the live immediate context and device lazily and tears them down on eject: the viewport and scissor setters, `DrawIndexed` (double a patched draw, re-issue an unpatched one per eye), `Draw` (full width for the fullscreen passes), `VSSetShader` (cache whether the bound shader is patched), and `CreateVertexShader` (catch the resource-recreate path that bypasses `CreateVertexProgram`). Install and uninstall are serialized, because both suspend every other thread and would suspend each other.

### The `cb13` upload

`cb13` is refreshed per pass, at the cadence of the engine's own `cb0` upload. Inside the routed range it holds distinct per-eye rows computed from the pristine centre transform and the per-eye `EyeRenderParams`. Everywhere else, including the shadow and reflection passes, both slots get the current view and `M_eye` is the identity, so a patched shader renders what it would have from `cb0`. That is why `cb13` tracks the current view rather than being written once.

### Collapse and double-wide

Under single-pass the frame driver runs one `game.Draw` instead of the per-eye loop, so the between-eye snapshot and restore is skipped ([lighting-shadow-vr-interactions.md](lighting-shadow-vr-interactions.md)); the camera hook (`jc3vr_view::hooks::camera`) keeps the render camera centred and zeroes the shadow-anchor delta; and the post-draw splits the back buffer into the two eye textures. `engine_render_resolution` (`jc3vr_view::stereo::layout`) returns twice the per-eye width, which the native-resolution driver (`jc3vr_view::vr::resolution`) targets, so the whole scene target set is re-created double-wide. The XR swapchain stays per-eye, and each half copies straight into its eye texture. The double-wide target requires `vr.native_resolution`.

### Already-instanced draws

A patched shader that takes per-instance data from a vertex-buffer slot (one reading `SV_InstanceID` is declined) arrives with the game's own instance ids, which `& 1` reads as an eye parity: half the batch per eye, flickering as head motion re-sorts them. Doubling the count would read past the batch's per-instance data. `instanced_per_eye` makes the parity irrelevant instead: for each eye it fills both `cb13` slots with that eye's rows, binds both viewport slots to that eye's half, and calls the original draw unchanged, then restores `cb13` (through a `WRITE_DISCARD` map of the bound buffer, from the rows cached in `Cb13Buffer::rows`) and the viewport slots. The mod's own re-issues are excluded by the `PER_EYE_REISSUE` marker. The buildings take one submission instead through a layout twin ([single-pass-render-blocks.md](single-pass-render-blocks.md)). The debug UI's single-pass section reports handled and exposed draws per shader.

### Viewport slots outside the routed range

A patched shader writes the viewport index unconditionally, so outside the routed range slot 1 has to duplicate slot 0 for odd instances to rasterise at all. `rs_set_viewports_detour` keeps that true for engine binds, but the collapse's per-draw split outlives the range. `VIEWPORT_SLOTS_UNIFORM` tracks whether the slots agree, and `unify_viewport_slots` re-binds slot 0's region to both when the range guard drops and on an out-of-range instanced draw with a patched shader bound. Slot 0 is never touched, so the repair can only make dropped instances reappear.

## Why `PreDraw` is outside the collapse

`PreDraw` is one of the largest GPU items in the frame and does not go through `DrawRenderPassRange`, but it already runs once per dispatch, so once per frame under the collapse; there is no second walk to remove, and no prepass draw is doubled or split. Under multipass, `pre_draw` (`jc3vr_render::hooks::graphics_engine::render_pass`) elides the view-independent categories (`SHARED_PREPASS_CATEGORIES`) on the second eye: the sun-shadow families, whose light camera [`RenderPass::SetRenderContextCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetRenderContextCamera) selects from the pass id, the reflections with their own camera, the world-space cloud shadows, and the water simulation. The sky-lighting LUT, the vegetation update, and the rain occluder take the render camera and stay per eye.

## Configuration

Every switch lives under `stereo.single_pass` and is on by default. Each row of the table in `crates/jc3vr-view/src/stereo/config/single_pass.rs` generates the field and default, the `SinglePassSwitch` variant the capture overrides by, whether a change needs a shader reload, the debug UI row and hover text, the switch it requires, and its entry in the manifest the Python capture protocol reads (`tools/rendering/protocol/single_pass_switches.json`, kept current by a test that rewrites it under `JC3VR_WRITE_MANIFEST=1`). Adding a switch is adding a row. The master switch, `stereo.single_pass.enabled`, does everything on this page; the others gate one correction each and are named where that correction is described. Three foveation switches sit in the table so the harness can compare them in one schedule, though they gate multipass too ([foveation.md](foveation.md)).

## Known gaps

- The baked-matrix families are still reprojected, and their alpha-tested edges flip where the sub-pixel shift crosses them; this is most of what remains at the shore, the meadow, and the cliff.
- The right eye's half rasterizes one eye width further right than an isolated eye, which rounds interpolated depth and attributes differently. Nothing in the mod reaches it ("The right eye's half of the target" in [single-pass-validation.md](single-pass-validation.md)).
- Under canted rigs, some choices (a billboard facing the camera's axis, a level of detail, a fade) are made once from the centred camera; mid-distance vegetation and distant terrain at a canted eye's outer edge differ. Which choices these are is not attributed.
- Unpatched geometry re-issued per eye reads the centred, double-wide `cb0`, so it has no parallax and renders squashed under asymmetric or canted eyes. Binding per-eye vertex globals around those re-issues breaks shaders that take clip from a baked matrix while reading the camera rows.
- The sun halo is one sprite per screen: [`SunHaloEffect::PreApply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::SunHaloEffect##func-PreApply) projects the sun through the single projection onto the scene texture, so it lands once across the double-wide target.
- The particle flares, light glows, and halos place their sprites from the centred camera and test occlusion at one full-target UV, so the per-eye re-issues sample the wrong pixel (read from the bytecode, unverified in game).
- The screen-space reflection block's compute blur ([`m_ComputeBlur`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceReflection##field-m_ComputeBlur)) is not rewritten and mixes texels across the eye seam.
- The water box surface grid has no per-eye treatment ([single-pass-collapse-defects.md](single-pass-collapse-defects.md)).
- The occluder intercept assumes `gfx.occluders.use_instancing` is off, and nothing forces it.
- The WaveWorks and legacy water fixes are not verified in the headset.
