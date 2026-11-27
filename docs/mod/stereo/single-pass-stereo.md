# Single-pass stereo

Multipass stereo renders the scene twice per frame, one full CPU walk of the render-pass list per eye. The frame is bound by draw-call submission rather than by the GPU (about 20,000 draw calls per frame, half per eye, with the GPU far from saturated; [performance.md](../performance/performance.md#what-limits-the-frame) has the measurement). Single-pass collapses the two geometry walks into one, so the GPU is fed from a single submission.

The single-pass material is split across five pages:

- this page: the technique, the shader rewrites, the payload integration, the configuration, and the known gaps;
- [single-pass-collapse-defects.md](single-pass-collapse-defects.md): what goes wrong when one walk renders both eyes into a double-wide target and a pass runs once over it, and what fixes each defect;
- [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md): how a draw that covers both eyes in one submission gets each eye's camera, lights, screen position, and motion, and what changes under asymmetric and canted eyes;
- [single-pass-draw-calls.md](single-pass-draw-calls.md): the draw census, the accounting against multipass, and the draws converted to one submission for both eyes;
- [single-pass-validation.md](single-pass-validation.md): how single-pass is compared against the isolated eyes and multipass, and what the comparison is expected to show.

[single-pass-render-blocks.md](single-pass-render-blocks.md) covers the per-render-block treatment.

Single-pass is the default rendering mode while a VR session runs. Multipass stays available: the debug UI's single-pass section (the Experimental page) has one button that switches the whole feature off or back on (`jc3vr_view::stereo::config::set_single_pass_feature`), and the choice is saved. Single-pass is inert without a running VR session (on the desktop, with the runtime missing, or with the headset disconnected the game renders as it ships) and without the DXVK viewport-routing capability.

## Turning on and off

The feature is live when its master switch is on, a VR session is running, and the device reports the routing capability (`jc3vr_render::stereo::single_pass::active`). The session state is latched at the frame start's configuration refresh, from the previous frame's runtime pump, so a session that starts or stops changes every reader of the next frame at once.

Single-pass's rewrites are applied when the engine creates a shader, so the live shader set has to be re-created whenever that state changes. The frame driver (`payload/src/hooks/game.rs`) compares it each frame with the state the live set was created under (`jc3vr_render::hooks::graphics_engine::shader::reconcile_single_pass`) and requests the reload, which runs in the same frame before the draws. This brings single-pass up after a late injection into a running game, where the game already holds its pristine shaders, and on the first frame of the first VR session after any injection; nothing needs to be toggled. The double-wide target and the native resolution follow from the same state the next time the resolution driver runs, and the draw-path detours install with the first rewritten shader. A session that stops takes the shaders back to the pristine set the same way, and an eject restores them as well (`restore_original_shaders_on_eject`), with the scene targets and the game's own resolution.

The settings file stores only what differs from the defaults (see [settings.md](../settings.md)), so a player who never touched single-pass follows its default, on. Switching it off with the button writes the group's switches as `false` under `[stereo.single_pass]`, which loads as multipass. A file written before the format carried a version held every setting, and so held the group off as it then shipped; its upgrade drops the group's `false` values, so the group loads whole rather than half on.

## The technique

Scene geometry is instance-doubled, rendered into a double-wide target, and routed to each eye's half by viewport index. The fullscreen passes run once over the whole target; the ones that need an eye take it inside the draw (see [single-pass-collapse-defects.md](single-pass-collapse-defects.md)).

- Routing. A vertex shader writes `SV_ViewportArrayIndex` directly, through the D3D11.3 `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` capability, which DXVK reports and compiles when the Vulkan `shaderOutputViewportIndex` feature is present. No geometry shader is involved. `jc3vr_view::stereo::layout::probe` checks the capability at runtime, and the whole feature stays inert without it.
- Target topology. The target is double-wide rather than a texture array. A two-slice array would force the roughly 300 pixel shaders that sample the scene targets as `Texture2D` to be retyped to `Texture2DArray`. A double-wide target needs none of that: the engine's own `CreateRenderSetups` re-runs with a doubled width, `SetRenderSetup` synthesises the viewport from the target size, and the SRV types never change. Each eye renders into its half, selected by the viewport index.
- Instance doubling. A patched geometry draw is issued with twice its instances. The vertex shader reads `eye = SV_InstanceID & 1`, picks that eye's view-projection out of `cb13`, and routes to its half.

Retrofitting single-pass onto a deferred renderer is usually blocked by per-shader position math. JC3 has one scene view-projection, [`RenderContext::m_OffsetViewProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_OffsetViewProjection), uploaded to the global vertex constant buffer (`cb0` registers 29 to 32, with the camera position at register 4; the engine stages these in [`RenderEngine::m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData)). Multipass already renders correct stereo, so every single-pass change has a per-pass correctness oracle to compare against ([single-pass-validation.md](single-pass-validation.md)).

### What the routed range covers

Under the collapse there is only one walk, so the range that routes to the eyes is the whole camera scene: `first >= RP_Z_OCCLUDERS`, which excludes the shadow and reflection prepasses (`<= RP_LAST_PREPASS`) that reuse the same shaders under the sun or reflection view. The range has to include the later geometry passes (the water planes at `RP_REFLECTIVE_WATER_PLANES`, the sky from `RP_STARS` to `RP_FOG_GRADIENT`, and the transparents), because there is no second dispatch to render them in.

A single contiguous pass range cannot separate those from the fullscreen deferred-lighting and post passes interleaved with them, which must keep the full width. So the viewport split happens per draw rather than per pass: `rs_set_viewports_detour` only records the full viewport, and `ensure_collapse_viewport` binds per draw the left and right halves for a patched geometry draw, one eye's half twice for an unpatched one (re-issued per eye), and the full width for the non-indexed `Draw` the fullscreen passes use. Without that last reset the lighting pass would inherit the eye half the previous geometry draw left bound and light one eye only, leaving the other eye's opaque geometry black.

#### The viewport record

What the split is cut from and what the slots hold live in one record (`viewport_record`), read on every routed draw: the last scene-sized viewport the engine bound, the engine's latest bind of any size, and the pair the two slots hold now. Every bind of the slots writes it: the engine's through `rs_set_viewports_detour` (whatever its count; a set of one slot or more than two leaves the pair unknown), and the mod's own through the trampoline where it makes them. A sequence lock makes the read a few plain loads, where a lock taken on every draw would cost a locked instruction that waits for the runtime's stores to drain. The eye camera's per-target rows (`eye_camera::viewport_changed`) read the record, so an engine bind is recorded before they are laid out.

With `single_pass.viewport_bind_cache` on, `ensure_collapse_viewport` binds nothing when the slots already hold the pair it wants, which is most draws, since a pass's patched draws all want the same split. The pair is forgotten at every pass boundary, so a change no detour saw (a state reset, which nothing calls inside a dispatch) cannot outlive the pass it happened in. DXVK's viewport bind is cheap, so the saving is small.

#### The range is entered three times per dispatch

[`RenderEngine::DrawRenderPassRange`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawRenderPassRange) is not one call per frame. [`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask) calls it three times, with bounds that are compile-time constants at each call site. [`RenderEngine::DrawGBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawGBuffer) draws `[RP_Z_OCCLUDERS, RP_LAST_GBUFFER)`, [`RenderEngine::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-Draw) draws `[RP_REFLECTIVE_WATER_PLANES, RP_POSTEFFECTS)`, and [`RenderEngine::DrawPosteffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawPosteffects) draws `[RP_POSTEFFECTS, RP_LAST_MAIN)`; the bounds are [`RenderPassId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId) values. [`RenderEngine::PreDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-PreDraw) runs the shadow, reflection, and vegetation prepasses before all three and does not go through `DrawRenderPassRange`. All three calls qualify for the routed range, which means three `enter_gbuffer_range` and guard-drop pairs per dispatch.

Anything hung off a range boundary therefore does not run once per frame under the collapse. The per-frame diagnostics (the instanced eye-parity exposure fold) run from `single_pass::begin_frame` instead, and the per-range draw-split log line is tagged with the `[first, last)` window it covers.

#### The range flag belongs to the draw thread

The flag is raised, read, and lowered entirely on the draw thread, inside one `HandleDrawThreadTask`. The safety clear that catches a range left open by an interrupted dispatch runs there too, in the `PreDraw` prologue (`single_pass::begin_dispatch`), the one point in that thread's sequence where no range can be live.

The clear cannot run on the game thread at the top of [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender). That is safe only while every dispatch is drained before `UpdateRender` returns, and with the frame tail deferred (`defer_tail` in `payload/src/hooks/game.rs`) the last dispatch is not drained: the draw thread keeps walking the previous frame's draws while the game thread runs the next frame's simulation and re-enters `UpdateRender`. A clear there would land in the middle of a live range, and every draw after it would be treated as out of range: no eye split, `cb13` mirrored instead of per eye, and the per-eye reprojection matrices dropped. Whole geometry families would blink in and out, since how far the draw thread got varies from frame to frame. A guard that finds the flag already down when it drops counts a "torn" range and warns, which measures this directly.

The single-pass flags the draw thread pins for its dispatch end with the dispatch's last seam (`single_pass::end_dispatch`). The engine runs its draw dispatch on a worker of its job pool, and other jobs on the same workers (the planar reflection camera's update, for one) read those flags, so a pin that outlived its dispatch would hand them the flags of whatever dispatch that worker last drew.

## The vertex-shader transform

The per-eye data in the position path is exactly five `cb0` rows: `cb0[4]` (the camera position) and `cb0[29..32]` (the view-projection).

Referencing one of those rows does not mean taking the clip position from them. `cb0[29..32]` can only be a clip transform, but `cb0[4]` is a camera position, and shaders read it for shading, for a distance fade, and for world-space lookups while taking their clip position from a baked matrix elsewhere. The whole vegetation set does that, as does `generaljc3`. A shader the remap claims this way gets viewport routing and instance doubling but no per-eye clip, so under the collapse's centred render camera both eye halves are drawn from the centre viewpoint, and the family sits at a rigid half-IPD offset from everything around it within a single eye's image. That is visible in the desktop mirror, and it grows as the camera approaches, because a wrong optical centre displaces geometry by `offset/distance` while a wrong projection or rotation displaces it by a fixed angle. "The vegetation `cb0[4]` misclassification" in [single-pass-render-blocks.md](single-pass-render-blocks.md) covers this and the other families the remap claims the same way.

The transform binds a mod-owned constant buffer at the free slot `cb13` holding both eyes' five rows, laid out `[eye0: 0..4][eye1: 5..9]`, and rewrites the shader to index it per eye:

1. Add an `SV_InstanceID` input (a new `v` register and ISGN entry).
2. Add an `SV_ViewportArrayIndex` output (a new `o` register and OSGN entry).
3. Declare `cb13` and bump `dcl_temps` by one for the eye register.
4. Prologue: `and rBase.x, vInstanceID.x, l(1)`; `mov oViewport.x, rBase.x`; `imul null, rBase.x, rBase.x, l(5)`. The viewport write reads the eye before the `imul` overwrites the temp with the row base, so one temp suffices.
5. Rewrite every operand referencing `cb0[4]` and `cb0[29..32]` to the register-relative `cb13[rBase.x + k]` (`k = 4` for the camera row, `n - 29` for the view-projection rows). Because the eye base is a register-relative index, this is a uniform operand remap: the same edit for every shader of the model idiom.
6. Fix the SHEX length dword, the ISGN and OSGN chunks, the container's total size, and the DXBC checksum (`dxbc_stereo::refresh_checksum`, shared with the fragment-program patching in `jc3vr_render::hooks::graphics_engine::shader`).

The new interface registers come from the signatures, not from a `dcl_input`/`dcl_output` scan: fxc keeps an ISGN entry for an input the shader never reads (`ReadWriteMask = 0`) and emits no declaration for it, so a declaration scan under-counts and the appended element collides with an occupied register.

The patch is applied in flight in a `CreateVertexProgram` hook, symmetric to the `CreateFragmentProgram` patching, before the underlying `CreateVertexShader` copies the bytecode ([shader-patching.md](../rendering/shader-patching.md) lists every creation-time patch).

A remapped shader that reads the view-projection rows but not `cb0[4]` takes the camera position it subtracts before them from `cb0[9]` instead: the render camera's position, which [`RenderEngine::SetGlobalShaderConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetGlobalShaderConstants) stages and the per-pass camera staging leaves alone. "The road surfaces' clip origin, per eye" in [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md) covers how those shaders get each eye's copy.

### Reference structures (what fxc emits)

Compiling a reference vertex shader that writes `SV_ViewportArrayIndex` (`scripts/dxbc.sh compile`, a `D3DCompile` harness over the `dxbc-tool` crate) settles the encodings the transform reproduces. It is committed as `crates/dxbc-stereo/tests/data/ref_vs50.dxbc`, and the unit tests compare against it byte for byte.

- A vs_5_0 writing `SV_ViewportArrayIndex` compiles; vs_5_1 is not needed. But fxc adds an `SFI0` (shader feature info) chunk, an 8-byte body with bit 13 (`0x2000`) set: `D3D_SHADER_REQUIRES_VIEWPORT_AND_RT_ARRAY_INDEX_FROM_ANY_SHADER_FEEDING_RASTERIZER`. The transform adds this chunk (or ORs the bit into an existing `SFI0`); without it the viewport output is invalid. The chunk order is `RDEF, ISGN, OSGN, SHEX, SFI0, STAT`.
- ISGN gains `SV_InstanceID` (`sysvalue = 8`, INSTANCE_ID, `uint` component type, mask `.x`). OSGN gains `SV_ViewportArrayIndex` (`sysvalue = 5`, `uint`, mask `.x`, never-written mask `.yzw`).
- `cb13` is declared dynamically indexed (`dcl_constantbuffer CB13[10], dynamicIndexed`), because it is indexed by a register (`rBase.x + k`) rather than an immediate, unlike the game's `cb0` (`immediateIndexed`).
- Reflection (`RDEF`) is not updated: DXVK binds and compiles from the `SHEX` declarations, not `RDEF`, and ignores the `STAT` chunk, so instruction-count bookkeeping is unnecessary and only the `SHEX` length dword is fixed.

### The corpus, and how the rewriter is validated

`dxbc-stereo`'s tests run the rewrites over all 455 vertex shaders extracted from the game's shader bundle. The `cb0` remap patches the shaders with per-eye `cb0[4]` or `cb0[29..32]` operands (single-pass); the shaders with no per-eye reference (baked world-view-projection, no position, or the `cb2` terrain) are reprojected or drawn per eye; the shaders that already declare `SV_InstanceID` are declined and drawn per eye (see "Already-instanced draws"). None errors or comes out structurally invalid.

Validation runs at four levels:

1. the single-shader unit tests of the rewriters in `crates/dxbc-stereo/src/rewrite/` check the exact injected encodings against the fxc reference;
2. `corpus_patch_is_sound` structurally validates every success: the output re-parses, carries the `SFI0` viewport bit, has no residual per-eye `cb0` operand, and its checksum is self-consistent;
3. `corpus_patch_signatures_have_no_register_collision` and `corpus_patch_body_and_signature_agree` re-read the rewritten signatures with an independent parser, so the signature is both internally consistent and consistent with the body it describes;
4. the patched blobs are accepted by Microsoft's `D3DDisassemble` under wine (`scripts/dxbc.sh disasm`), the closest offline proxy to DXVK's `CreateVertexShader` accepting them.

The extract is game-derived and git-ignored. A missing extract fails the corpus tests; set `JC3VR_ALLOW_MISSING_SHADER_CORPUS=1` to skip them instead.

## Reprojection: the non-`cb0` families

The `cb0` remap covers the model shaders. Everything else writes clip from a CPU-baked copy of the same view-projection, and one identity handles the whole group. Every scene shader writes `clip_center = VP_center · world`, whatever buffer the view-projection came from, so:

    clip_eye = VP_eye · world = (VP_eye · VP_center⁻¹) · clip_center = M_eye · clip_center

Post-multiplying the shader's own `SV_Position` by a per-eye `M_eye` lands it in each eye. The camera-relative idiom (`OffsetVP·(P − campos)` equals `FullVP·P`) folds in when `M_eye` is built from the full, translation-carrying per-eye view-projections: the IPD parallax rides inside `M_eye`, so the eye offset never reaches the shader. Skinned characters are the baked world-view-projection case: skinning is local to world, which reprojection never sees.

`M_eye` is near the identity (a few centimetres of IPD and a few degrees of cant), so reverse-Z depth survives it. `VP_center` is inverted in f64 on the CPU, because the reverse-Z view-projection is near-singular, and stored as f32.

### The DXBC recipe (`dxbc_stereo::reproject_vertex_shader`)

Reprojection reuses everything the remap builds (the `SV_InstanceID` input, the `SV_ViewportArrayIndex` output, the `SFI0` bit, the signature append, the checksum, and the prologue) and swaps only the core:

1. Find the `SV_Position` output register from `OSGN` and `dcl_output_siv position` (normally `o0`).
2. Rename every write to it to a fresh temp `rClip`, bumping `dcl_temps`. `SV_Position` is written on every path, so `rClip` ends fully defined, and the rename absorbs masked or multi-instruction writes (a separate `o0.z` clip-Z bias). SM5 output registers are write-only, so redirecting the writes is the only option. Renaming the write also leaves the shader's own source temp intact; the terrain domain shader reuses its centre-clip temp for an LOD fade after writing position.
3. Before each `ret`, emit `o0 = M_eye · rClip` as four `dp4`s. Each `dp4` reads an `M_eye` row as `cb13[rBase + (10 + j)]`, with `rBase = 4 * eye`.
4. `M_eye` occupies `cb13` rows 10 to 17 (four rows per eye) after the ten remap rows (`STEREO_REPROJ_CB_ROWS`), on the same `b13` binding.

A shader with a `retc` is rejected: a conditional early return would need the epilogue on that path too.

After the clip-origin copies, `cb13` carries a selector row (`EYE_ROWS_SELECT_ROW`, set in a dual-eye upload and zero in a mirror) and each eye's copy of the full, translation-bearing view-projection (from `FULL_VIEW_PROJECTION_ROW_BASE`, four rows per eye, the `M_eye` block's stride). Those serve the shaders that read an eye's own rows in place of being reprojected (below).

### Exact only in real arithmetic

`M_eye · clip_centre` equals `VP_eye · world` in real arithmetic. In floats the two round differently: the centre's clip position is rounded once through the centre's matrix and again through `M_eye`, while an eye rendered on its own rounds once through its own. At the headset's size the difference is a few to a few hundred floats of depth on every reprojected surface, a different amount per triangle or patch, and a sub-pixel shift that changes a code of albedo or normals or flips an alpha-tested edge. Where a shader's own math is "a position times a view-projection", the exact form is to give it the eye's view-projection and camera position at the point it reads them, so it does the arithmetic an isolated render does. The `cb0` remap is that form for the offset view-projection.

### Each eye's own rows, not the centre's reprojected

Two more groups read the eye's own rows rather than being reprojected:

- The terrain domain shaders (`volumetricterrain4`, `volumetricterrain4near`, and `volumetricterrain4silhouette` on the pinned-eye path, and `volumetricterrain2`, `volumetricterrainpow2`, and `volumetricterrainsilhouette` on the eye-lane path) build clip as `(world − camera) · m_OffsetViewProjection` from their terrain constants, which [`RenderBlockTypeTerrainPatch::SetupConstantBuffers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##func-SetupConstantBuffers) copies from the render context's offset view-projection and camera position: the values `cb13`'s slots hold for each eye. The rewrite (`dxbc_stereo::rebase_domain_shader` and `rebase_pinned_domain_shader`) chooses, in a prologue, between the eye's `cb13` rows and the shader's own by the selector row, so outside the routed range (the reflection passes that reuse these shaders) they read their own constants. The distance outputs and the LOD fade move to the eye with the rows.
- The full view-projection's readers. The shaders that project an absolute world position through `cb0` rows 0 to 3 read the eye's copy of those rows: `treeimpostor*`, `lightsource*`, `aobox`, and the non-tessellated basemesh `volumetricterrain4notessellation*` on the instance-parity path (`dxbc_stereo::remap_full_view_projection`), and its GPU-indirect permutations on the pinned-eye path (`dxbc_stereo::remap_pinned_full_view_projection`). Outside the routed range both copies mirror `cb0`.

The terrain-detail block's per-eye re-issue builds each eye's buffer the same way: the patch translation relative to that eye's camera position times that eye's offset view-projection, both read from the eye's `cb13` slot, through the engine's own [`Matrix4::Multiply4x4`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::math::Matrix4##func-Multiply4x4). Reprojecting the centre's matrix by `M_eye` column by column instead rounded badly enough in its depth row to scale one eye's ground depth by a few parts in 100,000.

What stays reprojected has no exact form in the shader: the baked world-view-projection families (`general*`, `character*`, `building*`, `prop*`, and the bark's `WorldViewProj`), the foliage, whose `OffsetWorld` translation the block rounds relative to the centred camera before the shader sees it, the moon, the water planes, and the decals. Their surfaces land a few floats from an isolated eye's, and their alpha-tested edges flip where the sub-pixel shift crosses them.

### The NDC-writer problem

Reprojection is shader-agnostic, so it would equally transform sky, UI, and fullscreen shaders that write `o0` in raw NDC, which `M_eye` corrupts, and the bytecode cannot reliably tell those from scene meshes. Substitution happens at shader creation, before the draw's pass is known, so the runtime pass gate cannot decide it. Reprojection is therefore gated on a positive allowlist of scene-geometry families, keyed on the shader name in `CreateVertexProgramParams` (`REPROJECT_NAME_PREFIXES` and the per-family name tables beside it in `jc3vr_render::stereo::single_pass::shader_policy`). Unknown shaders stay drawn per eye, which is correct but slower, and no NDC writer is reprojected.

The census of the shipped set (`m_Name` against the rewrite outcome) only sees the shaders an area loads, so it is a floor rather than a complete set. Outside the allowlist it finds:

- the terrain tessellation shaders, which take the eye through the tessellation path below rather than the vertex-shader reprojection (`terraindetailrt*` is absent on purpose: the terrain-detail block is GPU-indirect and is handled by a render-block intercept that rebuilds its `cb1`, so its vertex shader must stay pristine);
- the vegetation, handled by the bark and foliage blocks (see "Drawing the vegetation once" in [single-pass-draw-calls.md](single-pass-draw-calls.md));
- NDC and non-scene writers, kept off the allowlist because `M_eye` would corrupt them: the sky and atmosphere, the post and screen-space passes, the UI, most particles, effects, and light sprites, the `nvwater*` and legacy `water*` families, the decals (held back as surface-projected), and the occluders, probes, depth-only, and shadow-only shaders;
- three candidate scene models of unclear family (`box`, `notex`, and `lrpc`), held back until someone looks at them.

## The terrain tessellation path

The tessellated base terrain builds clip in the domain shader, from `cb1`'s `m_OffsetViewProjection` (byte-identical to `cb0`'s), so neither the remap nor the vertex-shader reprojection applies. Instead the eye index rides through the pipeline on the free `.z` of the `TEXCOORD3` interpolant:

- the vertex shader (`inject_eye_forward_vertex_shader`) adds an `SV_InstanceID` input and writes `eye = id & 1` into that lane, widening the output to `.xyz`;
- the hull shader (`forward_eye_hull_shader`) widens the lane its control-point phase forwards, leaving the fork and join phases (the tessellation factors) untouched, since they reuse the same `o` registers for unrelated system values;
- the domain shader reads the eye from `vicp[0][lane].z` and writes `SV_ViewportArrayIndex`, which is legal from the last pre-rasterization stage under the capability. It then reads its camera rows, the offset view-projection and camera position of its terrain constants, from that eye's `cb13` slot (`dxbc_stereo::rebase_domain_shader`). `cb13` is bound on the domain stage for this.

The hull and domain shaders are gated structurally (only shaders carrying the free lane transform; the rest return an error and are left untouched), since they are created without a paired vertex shader identity. Shadow-pass permutations are excluded by name, and an unnamed shader counts as a shadow pass: failing closed costs a terrain draw its single-pass treatment, while failing open would eye-transform a shadow-atlas draw.

### Pipelines that cannot carry the eye: the pinned-eye reprojection

The terrain patch system ([`RenderBlockTerrainPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch)) has two pipelines the lane path cannot reach. Its tessellated permutations (`volumetricterrain4*`, with the `volumetricterrain4`, `volumetricterrain4near`, and `volumetricterrain4silhouette` domain shaders) use vertex shaders that write all four components of `TEXCOORD3`, so there is no free lane, the eye-inject declines, and the domain shader builds clip from its own `cb1` (the [`RenderContext::m_OffsetViewProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_OffsetViewProjection) the block type bakes there) with no eye to choose by. Its GPU-indirect basemesh permutations (`volumetricterrain4notessellation*instanced`) project through the full view-projection in `cb0` rows 0 to 3, which the remap does not cover, and already read `SV_InstanceID`, so neither the remap nor the instance-parity reprojection takes them. Neither kind of draw can be instance-doubled either: the near passes are GPU-indirect, and the far tessellated ones have unpatched vertex shaders. Both are submitted once per eye (the indirect re-issue and the unpatched `DrawIndexed` re-issue), and without a per-eye transform both submissions would be drawn from the centred camera.

The creation hooks rewrite such a shader to read the first `cb13` eye slot with no instance input and no viewport output: a patch domain shader reads its camera rows from the first slot (`dxbc_stereo::rebase_pinned_domain_shader`), and a basemesh vertex shader reads the first slot's full view-projection in place of `cb0` rows 0 to 3 (`dxbc_stereo::remap_pinned_full_view_projection`). `dxbc_stereo::reproject_pinned_eye` is the same epilogue as the reprojection with the row base held at zero, for a shader of neither shape. The hooks apply the rewrite to a terrain domain shader that the lane path declines for want of a lane, and to a terrain vertex shader that already declares `SV_InstanceID`, and tag the resulting D3D object with a private-data marker. The vertex shader stays out of the patched set, so its draws keep their per-eye submission. Both per-eye re-issues check the bound vertex and domain shader for the marker and, when one carries it, pin both `cb13` eye slots to the eye being drawn for each submission (`jc3vr_render::stereo::single_pass::pinned_eye`, the pin the already-instanced re-issue uses), and restore the dual-eye rows afterwards.

What stays centre-derived is what should: the hull shader's tessellation factors and its patch culling (`TessOrigin` and the culling direction in the hull `cb1`), and the patch selection. The tessellation origin is the active camera's position, which is the centred head camera in every mode, so an isolated eye tessellates from the same origin. Under the eye rows the domain shader's distance outputs and LOD fade are the eye's, as in an isolated render. Outside the routed range `cb13`'s `M_eye` is the identity, so the reflection passes that reuse these shaders are unchanged, and the shadow permutations are excluded by name as on the lane path. No draw is added and none is removed.

`single_pass.terrain_patch_per_eye` (with `single_pass.terrain`) gates it; it is a shader rewrite, so a change needs a shader reload. The terrain hull's frustum cull is a separate matter, covered under "The terrain hull cull" in [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md).

### The patch prep, from the centre camera in every mode

The patch system's prep passes ([`RP_TERRAINPATCH_CLEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_CLEAR) through [`RP_TERRAINPATCH_ENUMERATION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_ENUMERATION)) rebuild the patches' detail, histogram, triangles, and morph targets on the GPU every frame, from the camera position row of the vertex globals, which `RenderEngine::PreDraw` gives the render camera's. Under the collapse that is the centre. In an isolated eye, and in each multipass dispatch, it would be the eye's, so each eye would morph the ground from its own position: a per-patch depth difference, zero at each patch's border and rising into its interior, with opposite signs in the two eyes. `jc3vr_render::stereo::terrain_prep` stages the centre camera's position for those passes in every dispatch that renders one eye at a time, which makes the shared choice the references' too and makes the two multipass eyes agree about the ground. It costs one extra upload of the vertex globals per prep pass in those dispatches, and nothing under single-pass.

## Payload integration

`jc3vr_render::stereo::single_pass` is a directory module. The parent holds the capability gate, the `*_active()` gates, the routed-range guard, and the install and teardown that own every detour's lifetime. The main submodules:

| Module | What it owns |
|---|---|
| `shader_policy` | which transform each shader gets, the name tables, and the census |
| `cb13` | the per-eye constant buffer, from derivation through upload to release |
| `per_eye_cb` | the other mod-owned constant buffers the rewritten shaders read per-eye rows from |
| `viewport`, `viewport_record` | keeping both viewport slots on the eye halves, and off them outside the range |
| `draw_detours`, `detour_lifecycle` | the draw-path vtable entry points, their per-eye re-issues, and their installation |
| `shader_detours` | substitution, and tracking which shader is bound |
| `per_eye_reissue` | blocks that bake their own transform and must draw twice |
| `pinned_eye` | the marker on pinned-eye shaders, and the `cb13` pin their per-eye re-issues take |
| `hw_instanced`, `vegetation_once` | the draws converted to one submission for both eyes ([single-pass-draw-calls.md](single-pass-draw-calls.md)) |
| `eye_camera`, `eye_globals` | the pixel shaders' per-eye camera rows, and the per-eye fragment globals of a split pass |
| `ambient_occlusion`, `ao_volumes`, `screen_space_reflection`, `post_filters`, `sun`, `impostor_pixel_size`, `static_lights`, `water_fragment_state`, `water_pixel_variants` | the per-eye corrections of [single-pass-collapse-defects.md](single-pass-collapse-defects.md) and [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md) |
| `config_snapshot` | the per-frame and per-dispatch flag snapshots, and the collapse and double-wide bits they publish to `jc3vr_view::stereo::layout` for the code outside single-pass |
| `instanced_exposure`, `frame_diagnostics`, `detour_timing` | measurement only; nothing here changes a draw |

The capture harness's probes into the draw path live in the render crate's `probes` table, which the capture crate fills at install (see [architecture.md](../../architecture.md), "Crate layering").

### Hook points

| Purpose | Function |
|---|---|
| Patch vertex shader bytecode in flight | `Graphics::CreateVertexProgram` |
| Patch hull and domain shader bytecode in flight | `Graphics::CreateHullProgram` / `CreateDomainProgram` |
| Mirror or diverge `cb13` per view | `RenderEngine::SetAllGlobalShaderProgramConstants` |
| Mark the routed pass range | `RenderEngine::DrawRenderPassRange` |
| Close a leaked range on the draw thread | `RenderEngine::PreDraw` |
| Rebuild the scene targets at a new size | `GraphicsEngine::CreateRenderSetups`, driven by `ApplyResize` |
| Per-eye render-block re-issue | `RenderBlockBark::Draw`/`DrawZ`, `RenderBlockFoliage::Draw`, `RenderBlockOccluder::DrawZ`, `RenderBlockTerrainDetail::Draw` |
| Stage the impostors' pixel size at one eye's width | `TreeImpostorRBType::Setup` |
| Stage the light sprites' inverse screen width at one eye's width | `RenderBlockTypeLights::Setup` |
| Baked-constant reprojection | `Graphics::SetVertexProgramConstants` |

On top of those, single-pass installs COM-vtable detours on the live immediate context and device, lazily on first use, and tears them down on eject: `RSSetViewports` and `RSSetScissorRects` (mirror or split the bound viewport), `DrawIndexed` (promote a patched draw to two instances, or re-issue an unpatched one per eye), `Draw` (reset the fullscreen passes to full width), `VSSetShader` (cache whether the bound vertex shader is patched), and `CreateVertexShader` (record the patched shaders, and catch the resource-recreate path that bypasses `CreateVertexProgram`). Install and uninstall are serialized against each other: both suspend every other thread while they patch, so two running concurrently would suspend each other.

### The `cb13` upload

`cb13` is refreshed per pass, at the same cadence as the engine's own `cb0` upload, so it always matches the current view. Inside the routed range it holds distinct per-eye view-projections, computed in mod code from the pristine centre transform and the per-eye `EyeRenderParams` (replicating the multipass camera math). Everywhere else (the shadow and reflection passes, and any frame without the collapse) both eye slots get the current view and the `M_eye` block is the identity, so a patched shader renders exactly what it would have from `cb0`. That shadow safety is why `cb13` tracks the current view rather than being written once.

### Render blocks that bake their own transform

Several render blocks bake a view-projection into a per-draw constant buffer inside their own `Draw`, across draw kinds that cannot be instance-doubled (CPU-instanced or GPU-indirect). Rather than replicate each block's bake, the block's whole `Draw` is re-issued once per eye with the eye's half-viewport bound, and for the duration of each call the `SetVertexProgramConstants` detour is armed to reproject that block's own upload by the eye's `M_eye`. The arm is keyed to the block's graphics context, and it reports whether it matched: a block that took an internal path staging no such matrix logs rather than silently rendering both eyes from the centre.

The re-issue marks itself, so the draw and viewport detours leave the block's own calls alone instead of compounding the split.

The baked matrices are stored column-wise: every such vertex shader builds clip with a multiply-add chain over the four registers (`clip = Σ_i p_i · cb[k+i]`) rather than four `dp4`s, so the per-eye buffer is the entry-wise `cb[k+i]_eye = M_eye · cb[k+i]_mono`. `cb13`'s own `M_eye` block uses the opposite convention, because the rewriter's epilogue is a `dp4` chain. See [single-pass-render-blocks.md](single-pass-render-blocks.md).

### Collapse

Under single-pass, the frame driver (`payload/src/hooks/game.rs`) runs one `game.Draw` instead of the per-eye loop, so the between-eye snapshot and restore (gated on `ordinal > 0`) is skipped; the camera hook (`jc3vr_view::hooks::camera`) keeps the render camera centred, since both eyes come from `cb13`, and the shadow-anchor delta is zeroed to match; and the post-draw (`jc3vr_view::hooks::graphics_engine::post_draw`) splits the one back buffer into the two eye textures. This is where the submission saving comes from.

### Double-wide

Under single-pass, `engine_render_resolution` (`jc3vr_view::stereo::layout`) returns twice the per-eye width, which the deferred-`ApplyResize` native-resolution driver (`jc3vr_view::vr::resolution`) targets, so the whole scene target set (the back buffer, `m_BackBufferLinear`, and the G-buffers) is re-created double-wide and the per-pass viewport follows the target size. The XR swapchain stays at the per-eye width, and the per-eye capture textures are sized to half the back buffer, so the collapse's capture split copies each full-width half straight into its eye texture with no squish. The double-wide target requires `vr.native_resolution`; a `vr.resolution_scale` below one shrinks each eye's half.

### Already-instanced draws

A draw whose patched vertex shader takes its per-instance data through a vertex-buffer slot (rather than `SV_InstanceID`, which would have declined the shader) arrives at `DrawIndexedInstanced` with the game's own instance ids. `SV_InstanceID & 1` then reads them as an eye parity, so instance `i` goes to eye `i & 1`: half the batch per eye, or the left eye alone at one instance. Head motion re-sorts which instances are odd, so the geometry flickers.

Promoting the instance count cannot fix it, because per-instance vertex-buffer stepping is indexed by the instance id, so a doubled count reads past the batch's per-instance data. Instead `instanced_per_eye` makes the parity irrelevant: for each eye it fills both `cb13` eye slots with that eye's view-projection, camera position, and `M_eye`, binds both viewport slots to that eye's half of the double-wide target, and calls the original `DrawIndexedInstanced` with the caller's arguments unchanged. Whichever parity the shader computes, and whichever `SV_ViewportArrayIndex` it writes, it resolves to the same eye. Afterwards `cb13`'s dual-eye contents and the exact viewport slots bound on entry are restored; `cb13` unconditionally, since every patched shader in the pass reads it and a pinned one would collapse the frame to mono.

The `cb13` pin writes through a `WRITE_DISCARD` map of the already-bound buffer rather than `upload_and_bind`, so a handled draw costs three maps and no COM reference counting. The rows it pins and restores are the ones `mirror_and_bind_cb13` last uploaded, cached in `Cb13Buffer::rows`, since the draw detour has no `RenderEngine` to recompute them from.

The gate is exactly the case's predicate: a patched vertex shader bound, the render thread inside the routed range, the collapse on, and `single_pass.instanced_per_eye`. The mod's own draws are excluded: a promoted `DrawIndexed` is re-issued through the trampoline, and a bark, foliage, or occluder per-eye re-issue by the `PER_EYE_REISSUE` marker, which this re-issue raises too. Everything else is forwarded once, unchanged. The cost is one extra submission per such draw; the buildings' hardware-instanced draws take one submission through a twin of their input layout instead (see [single-pass-draw-calls.md](single-pass-draw-calls.md)).

The counters that measure the case split it into handled (re-issued per eye) and exposed (the switch off, or the re-issue declined for want of a recorded full viewport, a live context, or `cb13` contents). Exposed draws are further split by instance count: one instance means the geometry is missing from the right eye, more than one means the batch is halved. Each draw is attributed to the bound `ID3D11VertexShader`, named where the `CreateVertexProgram` path supplied one (the re-acquire path does not, and those show as `<unnamed 0x…>`). The debug UI's single-pass section reports the last frame plus a mean over the measured frames with a per-shader breakdown, and `single_pass`-target log lines carry the same figures at the draw split's cadence. The counters reset with the patched-shader set (a shader reload), since both are keyed by shader pointer. Every instanced draw is also bucketed by whether a patched shader was bound and whether the draw was inside the routed range, so the log distinguishes the draws the re-issue covers from the ones it never sees; the much larger out-of-range population is the shadow, reflection, and post work, where the parity is neutralised as described next.

### Viewport slots outside the routed range

A patched vertex shader writes `SV_ViewportArrayIndex = SV_InstanceID & 1` unconditionally, since the bytecode cannot tell which pass it is in. Only the routed geometry binds an eye-half pair, so everywhere else slot 1 has to duplicate slot 0 for the odd-parity instances to rasterise at all. `rs_set_viewports_detour` keeps that true for every viewport the engine binds, but the collapse's own per-draw split leaves the slots holding different halves, and that state outlives the range: until the next engine bind, an already-instanced draw's odd instances would go to the other eye's half.

The uniform viewport slots close that window. `VIEWPORT_SLOTS_UNIFORM` tracks whether the two slots hold the same region (every path that binds them records what it left behind, and anything unknown counts as non-uniform), and `unify_viewport_slots` re-binds slot 0's region to both slots when they do not. It runs when the routed range guard drops (which also covers the GPU-indirect draws nothing detours) and on an out-of-range `DrawIndexedInstanced` with a patched shader bound. Slot 0 is never touched, so the repair can only make dropped instances reappear; it cannot move what already rendered. The log reports how many repairs a window needed as `slot-1 repairs`.

## Why `PreDraw` is outside the collapse

`PreDraw` is one of the largest GPU items in the frame (see [performance.md](../performance/performance.md)) and does not come through `DrawRenderPassRange`, so it reads as a large pool of draws the collapse has not claimed. It is not. Under the collapse there is exactly one dispatch, and `GraphicsEngine::HandleDrawThreadTask` calls `RenderEngine::PreDraw` once per dispatch, so `PreDraw` already runs once per frame. There is no second walk to collapse, and extending the collapse over `PreDraw` would save no submissions. The measurement agrees: `PreDraw` costs about the same with two dispatches as with one, because the shared prepasses (`pre_draw` in `jc3vr_render::hooks::graphics_engine::render_pass`) already elide the view-independent categories on the second eye of the multipass path.

[`RenderPass::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-Draw) (the prepass entry) selects the pass camera: the pass's own external camera if it has one, otherwise the camera manager's render camera. [`RenderPass::SetRenderContextCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetRenderContextCamera) then overrides that from the render context's pass id for the shadow families. It has three disjoint branches, each selecting a uniform light camera indexed by the pass id relative to the first pass of its family: [`PRE_RP_STATIC_SHADOW_0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-PRE_RP_STATIC_SHADOW_0), [`PRE_RP_SHADOW_0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-PRE_RP_SHADOW_0), and [`PRE_RP_SHADOW_REFLECTIVE_SUN_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-PRE_RP_SHADOW_REFLECTIVE_SUN_NEAR). The passes from `PRE_RP_STATIC_SHADOW_0` through `PRE_RP_SHADOW_REFLECTIVE_CAMERA` are therefore rendered from the sun or light view by construction and cannot depend on which eye is drawn.

| Category | `RenderPassId` range | Camera | Per eye? |
|---|---|---|---|
| Terrain-patch prep | `RP_TERRAINPATCH_CLEAR` to `RP_TERRAINPATCH_ENUMERATION` | falls through to the render camera | yes |
| Sky-lighting LUT | `PRE_RP_SKY_LIGHTING` | render camera | held per eye, conservatively |
| Planar and environment reflections | `PRE_RP_REFLECTION_PRE` to `PRE_RP_ENVREFLECTION` | the pass's own reflection camera | no |
| Cloud shadows | `PRE_RP_CLOUDSHADOWS` | world space | no |
| Vegetation | `PRE_RP_VEGETATION_UPDATE` to `PRE_RP_VEGETATION_INTERACTION` | render camera | held per eye, conservatively |
| Sun-shadow cascade atlas, static, dynamic, and reflective | `PRE_RP_STATIC_SHADOW_0` to `PRE_RP_SHADOW_REFLECTIVE_CAMERA` | light camera, from the pass id | no, verified in the binary |
| Water simulation compute | `PRE_RP_WATER_CS_PRE` to `PRE_RP_WATER_DISPLACEMENT_PRE` | own targets and viewports | no |
| Rain occluder | `RP_RAIN_OCCLUDER` | render camera | held per eye, conservatively |

`SHARED_PREPASS_CATEGORIES` in `jc3vr_render::hooks::graphics_engine::render_pass` encodes the "no" rows validated in game and elides them on the second eye of the multipass path. Under the collapse that elision is inert, since nothing is duplicated.

What remains in `PreDraw` is the game's own shadow, reflection, and vegetation cost, the same as flatscreen, and reducing it is a quality decision (cascade count, atlas resolution, update cadence) rather than a stereo one. Two stereo-attributable riders exist, both deliberate: `shadow_manager_update_render` widens the cascade fit to the union field of view of both eyes, which puts more geometry inside each cascade, and `shadow_update_every_frame` (off) would defeat the engine's cascade update amortization; it is a flicker diagnostic and stays off.

The instanced draws issued outside the routed range (the shadow-pass families) are issued once per frame under the collapse too. The draw and viewport detours are gated on `in_gbuffer_range()`, which is raised only for `first >= RP_Z_OCCLUDERS`, so no prepass draw is instance-doubled or eye-split. The only thing that reaches them is a patched shader's unconditional `SV_ViewportArrayIndex` write, which the uniform viewport slots neutralise.

## Configuration

All switches live under `stereo.single_pass` and are on by default. Each row of the switch table in `crates/jc3vr-view/src/stereo/config/single_pass.rs` generates the configuration field and its default, the `SinglePassSwitch` variant the capture overrides and the shader build name it by, whether changing it needs a shader reload, its debug UI row and hover text, the switch it requires, and its entry in the manifest the Python capture protocol reads (`tools/rendering/protocol/single_pass_switches.json`, kept current by a test in the table's module, which rewrites it when run with `JC3VR_WRITE_MANIFEST=1`). Adding a switch is adding a row there. The debug UI's single-pass section shows every row.

The master switch, `stereo.single_pass.enabled`, substitutes the rewritten shaders, installs the routing, makes the eyes diverge (distinct per-eye `cb13`, eye-half viewports, and instance doubling), collapses the per-eye walk to one `game.Draw`, and re-creates the scene targets at twice the per-eye width. It is inert without the viewport-routing capability.

Shader rewrites, which need a shader reload to apply:

| Switch | Effect |
|---|---|
| `reproject` | Reprojects the non-`cb0` scene families (characters, props, buildings, roads) instead of drawing them per eye. |
| `terrain` | Carries the eye index through the terrain tessellation pipeline; also gates the render-block re-issue that reprojects the GPU-indirect terrain-detail pass. |
| `terrain_patch_per_eye` | On top of `terrain`: the terrain patch pipelines that cannot carry an eye index take the pinned-eye rewrite, and their per-eye re-issues pin `cb13` to each eye. |
| `pixel_camera_per_eye` | Every pixel shader that reads the camera position picks the rows of the eye whose half its pixel lies in. Turning it off applies at once. |
| `local_lights_per_eye` | In the same rewritten shaders, local light positions and spot projections are measured from the pixel's eye. Turning it off applies at once. |
| `screen_door_per_eye` | Screen-door patterns hashed from the pixel position are laid out from the origin of the pixel's eye half. |
| `clip_origin_per_eye` | The remapped shaders that subtract `cb0[9]` before the view-projection (the roads and the `roadbias` models) read each eye's copy of it. |
| `ao_volumes_per_eye` | The ambient-occlusion volumes are drawn once for both eyes, with the volume pixel shaders mapping each pixel's depth through its eye. |
| `clouds_per_eye` | The cloud layers (`clouds`, `cirrusclouds`, `cloudflythrough`, and `softclouds`) take the reprojection. |
| `ssdecal_per_eye` | Screen-space decals: a per-eye basis plus a spliced depth-UV bias in the `ssdecal` pixel shaders. |
| `ssao_per_eye` | SSAO computed for each eye in the block's single run. |
| `ssao_exact_sampling` | The SSAO passes compute their coordinates, texels, and filter weights in the shader in every mode, multipass and the isolated eyes included. |
| `ssr_per_eye` | Screen-space reflections traced for each eye in the block's single run. |

Draw handling, which applies at once:

| Switch | Effect |
|---|---|
| `vegetation_once` | Draws a bark or foliage block once for both eyes instead of re-issuing it. |
| `instanced_per_eye` | Re-issues an already-instanced `DrawIndexedInstanced` with a patched shader once per eye. |
| `hw_instanced_once` | Draws such a draw once for both eyes instead when its input layout has a step-rate-doubled twin. |
| `indirect_per_eye` | Re-issues the GPU-indirect draws once per eye. |
| `slot13_per_eye` | Re-issues non-indexed world geometry once per eye, by pass allowlist. |
| `nvwater_per_eye` | Re-issues the WaveWorks water blocks per eye with each eye's view. |
| `ssdecal_geometry_per_eye` | On top of `ssdecal_per_eye`: reprojects the screen-space decal box geometry. |
| `terrain_cull_both_eyes` | The terrain hull programs' frustum cull bounds both eyes rather than the centred camera. Needs `terrain`. |
| `collapse_viewport_follows_target` | Derives the eye halves from the bound render target rather than the scene viewport. |
| `viewport_bind_cache` | Skips a routed draw's viewport bind when the slots already hold what it wants. |

Fullscreen and screen-space passes, which apply at once:

| Switch | Effect |
|---|---|
| `reconstruct_per_eye` | Splits the deferred clustered-lighting resolve per eye. |
| `fullscreen_camera_per_eye` | Moves the fragment `GlobalConstants` camera position, with the matching sun-shadow cascade translation, to the eye for each masked run of a split pass. |
| `clustered_per_eye` | Builds the clustered froxel light grid per eye. |
| `fsr_per_eye` | Resolves FSR once per eye over that eye's half, through that eye's own context and history. |
| `foveation_per_eye` | Lays the static foveation's mask and fill-in out over each eye's half. |
| `foveation_gbuffer`, `foveation_lighting`, `foveation_quads` | Extend the foveation's peripheral stencil test to more passes and drop whole quads. They gate multipass too and sit here so the harness can compare them within one schedule; see [foveation.md](foveation.md). |

The rest of the collapse's per-eye behaviour has no switch and applies whenever the collapse is active: the tree impostors, the static lights, the moon, the water planes, and the stars; the bark, foliage, and occluder block re-issues; the per-eye velocity, atmospheric, fog-volume, glare, subsurface, depth-of-field, and composite-overlay passes; the legacy water's UV and the WaveWorks water's reflection, camera, and reflection-camera size; the terrain detail's per-eye matrices; the terrain domain shaders' and full view-projection readers' eye rows; the once-drawn non-indexed geometry; the uniform viewport slots; the clustered light grid's per-eye light view; the SSAO temporal skip; the skipped effect-depth copy; and the direct per-eye blit.

## Known gaps

- The baked-matrix families are still reprojected. The models, characters, buildings, bark, foliage, decals, moon, and water planes take their clip position from a matrix their block bakes on the CPU per draw from the centred camera, and no per-eye form of that product exists in the shader. Their surfaces land a few floats from an isolated eye's, and their alpha-tested edges flip where the sub-pixel shift crosses them; this is most of what remains at the shore, the meadow, and the cliff (see [single-pass-validation.md](single-pass-validation.md)).
- The right eye's half rasterizes one eye width further right than an eye rendered on its own, which rounds interpolated depth and attributes differently. It is a property of the rasterizer, not of any transform, and nothing in the mod reaches it ("The right eye's half of the target" in [single-pass-validation.md](single-pass-validation.md)).
- Under the canted rigs, some geometry and surface choices (a billboard facing the camera's axis, a level of detail, a fade) are made once from the centred camera where an isolated eye makes them from its own canted one: the vegetation at mid distance and distant terrain at the outer edge of a canted eye differ. Which choices these are is not attributed.
- Unpatched geometry has no parallax and, under asymmetric or canted eyes, no per-eye projection. It is re-issued per eye, so it is present and correctly sized in both, but it reads the centred `cb0`, which carries eye 0's projection and no cant and is double-wide, so it renders horizontally squashed. A per-eye binding of the vertex globals around those re-issues was tried and removed, because a shader that takes its clip from a transform its block baked from the centred camera while reading the camera rows for something else breaks when only the rows move. The terrain patch system is not in this set.
- The sun halo is one sprite per screen. [`SunHaloEffect::PreApply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::SunHaloEffect##func-PreApply) projects the sun through the render context's single projection onto the scene texture's pixel size and draws one halo there, so under the collapse it lands once, somewhere across the double-wide target. It runs only while the sun is low.
- The other screen-space light sprites are unexamined. The particle flares (`particleeffectflare*`), the light glows (`lightglow`), and the halos place their sprites from the centred camera's screen position on the CPU, and the flares and glows test occlusion by sampling depth at one full-target UV staged per draw. Their unpatched indexed draws are re-issued into each eye's half, where that UV samples the wrong pixel. This is read from the bytecode and is unverified in game.
- The particles keep the centred light vectors: the particle vertex and domain shaders light from `LightingFrameConsts` per vertex and are not rewritten.
- The motion blur's camera reconstruction reads the double-wide previous view-projection. The motion blur does not run in VR, so this has no effect on what a player sees.
- The screen-space reflection block's compute blur, which [`m_ComputeBlur`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceReflection##field-m_ComputeBlur) selects in place of the two blur draws, is not rewritten, so its horizontal pass still mixes texels either side of the eye seam.
- The 14 already-instanced shaders are declined by the remap and drawn per eye.
- The water box surface grid is drawn from the centred camera with the legacy family's clip path and has no per-eye treatment (see "The water box surface, and the legacy family's clip path" in [single-pass-collapse-defects.md](single-pass-collapse-defects.md)).
- The occluder intercept assumes `gfx.occluders.use_instancing` is off, and nothing forces that cvar. The cvar's storage and default are not located.
- The WaveWorks water and legacy water fixes are not verified in the headset.
- Submissions still drawn twice (the terrain patches, the screen-space decals, the vegetation's hardware-instanced draws, the tessellated particles, and the split fullscreen passes) are listed under "What is still drawn twice" in [single-pass-draw-calls.md](single-pass-draw-calls.md).
