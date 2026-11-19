# Single-pass stereo

Double-draw stereo renders the scene twice per frame — one full CPU walk of the render-pass list per
eye. Profiling showed the frame is **draw-call-submission bound**, not GPU bound: ~20k draw calls per
frame (~9,900 per eye, doubled), GPU only ~41% utilised, no queue syncs. Single-pass collapses the two
geometry walks into one, so the GPU is fed from a single submission. See
[`profiler.md`](../performance/profiler.md) for how the bottleneck was measured, and
[`single-pass-render-blocks.md`](single-pass-render-blocks.md) for the per-render-block treatment.

Single-pass is opt-in and off by default. It is inert without the DXVK viewport-routing capability.

## The technique

**Instance-doubled, double-wide, viewport-routed** — for scene geometry. The fullscreen passes run
once over the whole target by default; the ones that reconstruct position from depth are re-issued
per eye under a scissor mask instead (see "Four ways content goes wrong under the collapse").

- **Routing.** A vertex shader writes `SV_ViewportArrayIndex` directly — the D3D11.3
  `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` capability, which DXVK reports and compiles (gated
  on the Vulkan `shaderOutputViewportIndex` feature). No geometry shader involved.
  `stereo::single_pass::probe` checks it at runtime and the whole feature stays inert if it is absent.
- **Target topology: double-wide, not a texture array.** A two-slice array would force the ~300 pixel
  shaders that sample the scene targets as `Texture2D` to be retyped to `Texture2DArray`. A
  double-wide render target needs none of that — the engine's own `CreateRenderSetups` re-runs with a
  doubled width, `SetRenderSetup` synthesises the viewport from the target size, and SRV types never
  change. Each eye renders into its half, selected by the viewport index.
- **Instance doubling.** A patched geometry draw is issued with `2x` instances; the VS reads
  `eye = SV_InstanceID & 1`, picks that eye's view-projection out of `cb13`, and routes to its half.

### What the routed range covers

Under the collapse there is only one walk, so the range that routes to the eyes is the whole camera
scene: `first >= RP_Z_OCCLUDERS`, which excludes the shadow and reflection prepasses (`<=
RP_LAST_PREPASS`) that reuse the same shaders under the sun or reflection view. That has to include
the later *geometry* passes — water planes at `RP_REFLECTIVE_WATER_PLANES`, sky
`RP_STARS..RP_FOG_GRADIENT`, transparents — because there is no second dispatch to render them in.

A single contiguous pass range cannot separate those from the *fullscreen* deferred-lighting and post
passes interleaved with them, which must keep the full width. So the viewport split moves from pass
level to draw level: `rs_set_viewports_detour` only records the full viewport, and
`ensure_collapse_viewport` binds per draw — the L/R halves for a patched geometry draw, one eye's half
twice for an unpatched one (re-issued per eye), and the full width for the non-indexed `Draw` the
fullscreen passes use. Without that last reset the lighting pass would inherit the eye half the
previous geometry draw left bound and light one eye only, leaving the other eye's opaque geometry
black.

Without the collapse the routed range is just the G-buffer (`..RP_FIRST_SCENE`), which is a
diagnostic shape rather than a usable one.

#### The range is entered three times per dispatch

[`RenderEngine::DrawRenderPassRange`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawRenderPassRange) is not one call per frame. [`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask) calls it three times, with bounds that are compile-time constants at each call site. [`RenderEngine::DrawGBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawGBuffer) draws `[RP_Z_OCCLUDERS, RP_LAST_GBUFFER)`, [`RenderEngine::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-Draw) draws `[RP_REFLECTIVE_WATER_PLANES, RP_POSTEFFECTS)`, and [`RenderEngine::DrawPosteffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawPosteffects) draws `[RP_POSTEFFECTS, RP_LAST_MAIN)`; the bounds are [`RenderPassId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId) values. [`RenderEngine::PreDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-PreDraw) runs the shadow, reflection, and vegetation prepasses before all three and does not go through `DrawRenderPassRange`. Without the collapse exactly one call qualifies (the G-buffer), and with it all three do, which means three `enter_gbuffer_range` / guard-drop pairs per dispatch.

Anything hung off a range boundary is therefore *not* per-frame under the collapse. The per-frame
diagnostics (the instanced eye-parity exposure fold) run from `single_pass::begin_frame` instead, and
the per-range draw-split line is tagged with the `[first, last)` window it covers.

#### The range flag belongs to the draw thread

The flag is raised, read, and lowered entirely on the draw thread, inside one
`HandleDrawThreadTask`. The safety clear that catches a range left open by an interrupted dispatch
must run there too — it does, in the `PreDraw` prologue (`single_pass::begin_dispatch`), the one point
in that thread's sequence where no range can be live.

It cannot run on the game thread at the top of [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender), which is safe only while every
dispatch is drained before `UpdateRender` returns. With the frame tail deferred (`defer_tail` in `hooks/game.rs`) the
last dispatch is *not* drained: the draw thread keeps walking the previous frame's ~20k draws while
the game thread runs the next frame's sim and re-enters `UpdateRender`. A clear there lands in the
middle of a live range, and every draw after it is treated as out of range — no eye split, `cb13`
mirrored instead of per-eye, the per-eye reprojection matrices dropped. How far the draw thread got by
then varies frame to frame, so whole geometry families (instanced buildings, bark, foliage) blink in
and out. The clear therefore belongs to the thread that raises and lowers the range flag, and happens
in the draw thread's dispatch prologue. A guard that finds the flag already down when it drops counts
a "torn" range and warns, which is the direct measurement of this happening.

Why this is tractable at all: the blocker for retrofitting single-pass onto a deferred game is
normally per-shader bespoke position math. JC3 has **one** scene view-projection —
[`RenderContext::m_OffsetViewProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_OffsetViewProjection), uploaded to the global VS constant buffer (`cb0` registers 29 to 32, with the camera position at register 4; the engine stages these in [`RenderEngine::m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData)). And double-draw already renders correct stereo, so every single-pass change
has a per-pass correctness oracle to diff against.

## The vertex-shader transform

The per-eye data in the position path is exactly five `cb0` rows: `cb0[4]` (camera position) and
`cb0[29..32]` (the view-projection).

> **Referencing one of those rows is not the same as taking your position from them.** `cb0[29..32]`
> can only be a clip transform, but `cb0[4]` is a camera *position*, and shaders read it for shading, for
> a distance fade, and for world-space lookups while getting their clip position from a baked matrix
> elsewhere. The whole vegetation set does exactly that, as does `generaljc3`. Being claimed by the
> remap buys such a shader viewport routing and instance doubling but no per-eye clip, so under the
> collapse's centred render camera *both* eye halves are drawn from the centre viewpoint — and the
> family sits at a rigid half-IPD offset from everything around it, **within a single eye's image**.
> That is visible in the desktop mirror with no headset on, and it grows as you approach, because a
> wrong optical centre displaces geometry by `offset/distance` while a wrong projection or rotation
> displaces it by a fixed angle. See "The vegetation `cb0[4]` misclassification" in
> [single-pass-render-blocks.md](single-pass-render-blocks.md), which also covers the other families
> the remap claims this way.

The transform binds a mod-owned constant buffer at the free slot
**cb13** holding *both* eyes' five rows, laid out `[eye0: 0..4][eye1: 5..9]`, and rewrites the shader
to index it per eye:

1. Add an `SV_InstanceID` input (new `v` register + ISGN entry).
2. Add an `SV_ViewportArrayIndex` output (new `o` register + OSGN entry).
3. Declare `cb13` and bump `dcl_temps` by one for the eye register.
4. Prologue: `and rBase.x, vInstanceID.x, l(1)`; `mov oViewport.x, rBase.x`;
   `imul null, rBase.x, rBase.x, l(5)`. The viewport write reads the eye before the `imul`
   overwrites the temp with the row base, so one temp suffices.
5. Rewrite every operand referencing `cb0[4]` and `cb0[29..32]` to the register-relative
   `cb13[rBase.x + k]` (`k = 4` for the camera row, `n - 29` for the VP rows). Because the eye base is
   a register-relative index, this is a *uniform* operand remap — the same edit for every shader of
   the model idiom.
6. Fix the SHEX length dword, the ISGN/OSGN chunks, the container total size, and the DXBC checksum
   (`dxbc_stereo::refresh_checksum`, shared with the fragment-program patching in
   `hooks/graphics_engine/shader.rs`).

The new interface registers come from the **signatures**, not from a `dcl_input`/`dcl_output` scan:
fxc keeps an ISGN entry for an input the shader never reads (`ReadWriteMask = 0`) and emits no
declaration for it, so a declaration scan under-counts and the appended element collides with a
register that is already occupied.

The patch is applied in-flight in a `CreateVertexProgram` hook, symmetric to the existing
`CreateFragmentProgram` patching, before the underlying `CreateVertexShader` copies the bytecode.

## Reference structures (what fxc emits)

Compiling a reference VS that writes `SV_ViewportArrayIndex` (`scripts/dxbc.sh compile`, a
`D3DCompile` harness over the `dxbc-tool` crate) settles the encodings the transform reproduces. It is
committed as `dxbc-stereo/tests/data/ref_vs50.dxbc` and the unit tests diff against it byte for byte.

- **A vs_5_0 writing `SV_ViewportArrayIndex` compiles** — vs_5_1 is not needed. But fxc adds an
  **`SFI0` (shader-feature-info) chunk**, an 8-byte body with **bit 13 (`0x2000`)** set:
  `D3D_SHADER_REQUIRES_VIEWPORT_AND_RT_ARRAY_INDEX_FROM_ANY_SHADER_FEEDING_RASTERIZER`. The transform
  adds this chunk (or ORs the bit into an existing `SFI0`), else the viewport output is invalid.
  Chunk order is `RDEF, ISGN, OSGN, SHEX, SFI0, STAT`.
- **ISGN** gains `SV_InstanceID` (`sysvalue = 8` INSTANCE_ID, `uint` component type, mask `.x`).
  **OSGN** gains `SV_ViewportArrayIndex` (`sysvalue = 5`, `uint`, mask `.x`, never-written mask
  `.yzw`).
- **`cb13`** is declared **dynamically indexed** (`dcl_constantbuffer CB13[10], dynamicIndexed`),
  because it is indexed by a register (`rBase.x + k`) rather than an immediate — unlike the game's
  `cb0` (`immediateIndexed`).
- Reflection (`RDEF`) is not updated: DXVK binds and compiles from the `SHEX` declarations, not
  `RDEF`, and the `STAT` chunk is ignored, so instruction-count bookkeeping is unnecessary — only the
  `SHEX` length dword is fixed.

## The corpus, and how the rewriter is validated

`dxbc-stereo`'s tests run the rewrites over all 455 extracted vertex shaders. The `cb0` remap:

| Outcome | Count | Disposition |
|---|---|---|
| Patched (per-eye `cb0[{4,29..32}]` remapped to `cb13`) | 196 | single-pass |
| No per-eye references (baked-WVP / no-position / cb2-terrain) | 245 | reprojected, or double-drawn |
| Already declares `SV_InstanceID` | 14 | double-drawn (the `>> 1` consumer rewrite is not built) |
| Errored / structurally invalid | 0 | — |

Validation runs at four levels:

1. the single-shader unit tests (`patch.rs`, `reproject.rs`, `terrain.rs`) check the exact injected
   encodings against the fxc reference;
2. `corpus_patch_is_sound` structurally validates every success — the output re-parses, carries the
   `SFI0` viewport bit, has no residual per-eye `cb0` operand, and its checksum is self-consistent;
3. `corpus_patch_signatures_have_no_register_collision` and `corpus_patch_body_and_signature_agree`
   re-read the rewritten signatures with an independent parser, so the signature is both internally
   consistent and consistent with the body it describes;
4. the patched blobs are accepted by real Microsoft `D3DDisassemble` under wine (via
   `scripts/dxbc.sh disasm`) — the closest offline proxy to DXVK's `CreateVertexShader` accepting
   them.

The extract is game-derived and git-ignored. A missing extract **fails** the corpus tests; set
`JC3VR_ALLOW_MISSING_SHADER_CORPUS=1` to skip them instead.

## Reprojection: the non-`cb0` families

The `cb0` remap covers the model shaders. Everything else writes clip from a CPU-baked copy of the
same view-projection, and one identity handles the whole group. Every scene shader writes
`clip_center = VP_center · world`, whatever buffer the VP came from, so:

    clip_eye = VP_eye · world = (VP_eye · VP_center⁻¹) · clip_center = M_eye · clip_center

Post-multiplying the shader's own `SV_Position` by a per-eye `M_eye` lands it exactly in each eye. The
camera-relative idiom (`OffsetVP·(P − campos)` equals `FullVP·P`) folds in when `M_eye` is built from
the full, translation-carrying per-eye VPs — the IPD parallax rides along inside `M_eye`, so the eye
offset never reaches the shader. Skinned characters are just the baked-WVP case: skinning is
local→world, which reprojection never sees.

`M_eye` is near-identity (a few-cm IPD, a few-degree cant), so the error is a couple of ULP and
reverse-Z depth survives. `VP_center` is inverted in f64 on the CPU — the reverse-Z VP is
near-singular — and stored as f32.

### The DXBC recipe (`dxbc_stereo::reproject_vertex_shader`)

Reprojection reuses everything the remap builds — the `SV_InstanceID` input, `SV_ViewportArrayIndex`
output, `SFI0` bit, signature append, checksum, and the same prologue — and swaps only the core:

1. Find the `SV_Position` output register from `OSGN`/`dcl_output_siv position` (normally `o0`).
2. Rename every write to it to a fresh temp `rClip`, bumping `dcl_temps`. `SV_Position` is written on
   every path, so `rClip` ends fully defined, and the rename absorbs masked or multi-instruction
   writes (a separate `o0.z` clip-Z bias). SM5 output registers are write-only, so redirecting the
   writes is the only move. Renaming the write also leaves the shader's own source temp intact — the
   terrain DS reuses its centre-clip temp for an LOD fade after writing position.
3. Before each `ret`, emit `o0 = M_eye · rClip` as four `dp4`s. Each `dp4` reads an `M_eye` row as
   `cb13[rBase + (10 + j)]`, with `rBase = 4 * eye`.
4. `M_eye` rides `cb13` rows 10–17 (four rows per eye) after the 10 remap rows
   (`STEREO_REPROJ_CB_ROWS = 18`), on the same `b13` binding.

A shader with a `retc` is rejected: a conditional early return would need the epilogue on that path
too.

### The NDC-writer problem

Reprojection is shader-agnostic, so it would equally transform sky, UI and fullscreen shaders that
write `o0` in raw NDC — which `M_eye` corrupts — and the bytecode cannot reliably tell those from
scene meshes. Substitution happens at shader creation, before the draw's pass is known, so the runtime
pass gate cannot decide it. Reprojection is therefore gated on a **positive allowlist of
scene-geometry families**, keyed on the shader name in `CreateVertexProgramParams`
(`REPROJECT_NAME_PREFIXES` in `stereo/single_pass/shader_policy.rs`): unknown shaders stay double-drawn — correct,
just slower — and no NDC writer is ever reprojected.

## The terrain tessellation path

The tessellated base terrain builds clip in the *domain* shader, from `cb1`'s
`m_OffsetViewProjection` (byte-identical to `cb0`'s), so neither the remap nor the VS reprojection
applies. Instead the eye index rides through the pipeline on the free `.z` of the `TEXCOORD3`
interpolant:

- the **VS** (`inject_eye_forward_vertex_shader`) adds an `SV_InstanceID` input and writes
  `eye = id & 1` into that lane, widening the output to `.xyz`;
- the **HS** (`forward_eye_hull_shader`) widens the lane its control-point phase forwards, leaving the
  fork and join phases (tessellation factors) untouched — they reuse the same `o` registers for
  unrelated system values;
- the **DS** (`reproject_domain_shader`) reads the eye from `vicp[0][lane].z`, reprojects its own
  `SV_Position` by `M_eye` exactly as the VS reprojection does, and writes `SV_ViewportArrayIndex` —
  legal from the last pre-rasterization stage under the capability. `cb13` is bound on the domain
  stage for this.

The hull and domain shaders are gated *structurally* (only shaders carrying the free lane transform;
the rest return an error and are left untouched), since they are created without a paired-VS identity.
Shadow-pass permutations are excluded by name, and an unnamed shader counts as a shadow pass — failing
closed costs a terrain draw its single-pass treatment, failing open would eye-transform a shadow-atlas
draw.

### Pipelines that cannot carry the eye: the pinned-eye reprojection

The terrain patch system ([`RenderBlockTerrainPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch)) has two pipelines the lane path cannot reach. Its tessellated permutations (`volumetricterrain4*`, with the `volumetricterrain4`, `volumetricterrain4near`, and `volumetricterrain4silhouette` domain shaders) use vertex shaders that write all four components of `TEXCOORD3`, so there is no free lane, the eye-inject declines, and the domain shader builds clip as `(world − camera) · m_OffsetViewProjection` from its own `cb1` (the [`RenderContext::m_OffsetViewProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_OffsetViewProjection) the block type bakes there) with no eye to choose by. Its GPU-indirect basemesh permutations (`volumetricterrain4notessellation*instanced`) project through the full view-projection in `cb0` rows 0 to 3, which the remap does not cover, and already read `SV_InstanceID`, so neither the remap nor the instance-parity reprojection takes them. Neither kind of draw can be instance-doubled either: the near passes are GPU-indirect, and the far tessellated ones have unpatched vertex shaders. Every one of them was already submitted once per eye (the indirect re-issue and the unpatched `DrawIndexed` re-issue), but both submissions were drawn from the centred camera. At the midday shore-lake view that was the near terrain under the whole lower half of each eye: about 4,400 pixels per eye whose G-buffer differed from an isolated render of that eye, at a depth that matched the isolated render shifted by exactly the half-IPD parallax.

`dxbc_stereo::reproject_pinned_eye` rewrites such a shader to post-multiply its own `SV_Position` by the `M_eye` in `cb13`'s first eye slot, with no instance input and no viewport output: the same epilogue as the reprojection, with the row base held at zero. The creation hooks apply it to a terrain domain shader that the lane reprojection declines for want of a lane, and to a terrain vertex shader that already declares `SV_InstanceID`, and tag the resulting D3D object with a private-data marker. The vertex shader stays out of the patched set, so its draws keep their per-eye submission. Both per-eye re-issues check the bound vertex and domain shader for the marker and, when one carries it, pin both `cb13` eye slots to the eye being drawn for each submission (`stereo::single_pass::pinned_eye`), the pin the already-instanced re-issue uses, and restore the dual-eye rows afterwards. The domain shader's `cb13` binding is the one the lane path already relies on.

What stays centre-derived is what should: the hull shader's tessellation factors and its patch culling (`TessOrigin` and the culling direction in the hull `cb1`), the domain shader's distance outputs and LOD fade, and the patch selection. Outside the routed range `cb13`'s `M_eye` is the identity, so the reflection passes that reuse these shaders are unchanged, and the shadow permutations are excluded by name as on the lane path. No draw is added and none is removed. Instance-doubling these draws instead would need an eye index in their instance count, which for the GPU-indirect passes lives in an argument buffer the CPU never sees (the same blocker as the vegetation's, under "Known gaps"), and for the far tessellated ones a new interpolant forwarded through the hull stage.

`single_pass.terrain_patch_per_eye` (**on**, with `single_pass.terrain`) gates it; it is a shader rewrite, so a change needs a shader reload.

## Payload integration

`stereo::single_pass` is a directory module. The parent holds the capability probe, the `*_active()`
gates, the routed-range guard, and the install and teardown that own every detour's lifetime; the rest
is split by concern:

| Module | What it owns |
|---|---|
| `shader_policy` | which transform each shader gets, the name tables, the census |
| `cb13` | the per-eye constant buffer, from derivation through upload to release |
| `viewport` | keeping both viewport slots on the eye halves, and off them outside the range |
| `draw_detours` | the draw-path vtable entry points and their per-eye re-issues |
| `shader_detours` | substitution, and tracking which shader is bound |
| `per_eye_reissue` | blocks that bake their own transform and must draw twice |
| `pinned_eye` | the marker on pinned-eye shaders, and the `cb13` pin their per-eye re-issues take |
| `eye_camera` | the pixel shaders' per-eye camera buffer, kept in step with the engine's fragment constants and the bound target |
| `impostor_pixel_size` | the tree impostors' pixel size at one eye's width |
| `config_snapshot` | the per-frame and per-dispatch flag snapshots |
| `instanced_exposure`, `frame_diagnostics` | measurement only; nothing here changes a draw |

### Hook points

| Purpose | Function |
|---|---|
| Patch VS bytecode in-flight | `Graphics::CreateVertexProgram` |
| Patch HS/DS bytecode in-flight | `Graphics::CreateHullProgram` / `CreateDomainProgram` |
| Mirror/diverge `cb13` per view | `RenderEngine::SetAllGlobalShaderProgramConstants` |
| Mark the routed pass range | `RenderEngine::DrawRenderPassRange` |
| Close a leaked range on the draw thread | `RenderEngine::PreDraw` |
| Rebuild scene RTs at a new size | `GraphicsEngine::CreateRenderSetups`, driven by `ApplyResize` |
| Per-eye render-block re-issue | `RenderBlockBark::Draw`/`DrawZ`, `RenderBlockFoliage::Draw`, `RenderBlockOccluder::DrawZ`, `RenderBlockTerrainDetail::Draw` |
| Stage the impostors' pixel size at one eye's width | `TreeImpostorRBType::Setup` |
| Baked-constant reprojection | `Graphics::SetVertexProgramConstants` |

Addresses live in the pyxis defs, not here.

On top of those, single-pass installs COM-vtable detours on the live immediate context and device,
lazily on first use and torn down on eject: `RSSetViewports` and `RSSetScissorRects` (mirror or split
the bound viewport), `DrawIndexed` (promote a patched draw to two instances; re-issue an unpatched one
per eye), `Draw` (reset the fullscreen passes to full width), `VSSetShader` (cache whether the bound VS
is patched), and `CreateVertexShader` (record the patched shaders, and catch the resource-recreate path
that bypasses `CreateVertexProgram`). Install and uninstall are serialized against each other: both
suspend every other thread while they patch, so two running concurrently would suspend each other.

### The `cb13` upload

`cb13` is refreshed per pass, at the same cadence as the engine's own `cb0` upload, so it always
matches whatever view is current. Inside the routed range with divergence on it holds **distinct**
per-eye view-projections, computed in mod code from the pristine centre transform and the per-eye
`EyeRenderParams` (replicating the double-draw camera math). Everywhere else — the shadow and
reflection passes, and any frame where divergence is off — both eye slots get the current view and the
`M_eye` block is identity, so a patched shader renders exactly what it would have from `cb0`. That
shadow-safety is why `cb13` tracks the current view rather than being written once.

### Render blocks that bake their own transform

Several render blocks bake a view-projection into a per-draw constant buffer inside their own `Draw`,
across draw kinds that cannot be instance-doubled (CPU-instanced, GPU-indirect). Rather than replicate
each block's bake, the block's whole `Draw` is re-issued once per eye with the eye's half-viewport
bound, and for the duration of each call the `SetVertexProgramConstants` detour is armed to reproject
that block's own upload by the eye's `M_eye`. The arm is keyed to the block's graphics context, and it
reports whether it actually matched — a block that took an internal path staging no such matrix logs
rather than silently rendering both eyes from the centre.

The re-issue marks itself, so the draw and viewport detours leave the block's own calls alone instead
of compounding the split.

The baked matrices are stored **column-wise**: every such vertex shader builds clip with a
multiply-add chain over the four registers (`clip = Σ_i p_i · cb[k+i]`) rather than four `dp4`s, so the
per-eye buffer is the entry-wise `cb[k+i]_eye = M_eye · cb[k+i]_mono`. `cb13`'s own `M_eye` block is
the opposite convention, because the rewriter's epilogue *is* a `dp4` chain. See
[`single-pass-render-blocks.md`](single-pass-render-blocks.md).

### Collapse

With `single_pass.collapse`, `hooks/game.rs` runs one `game.Draw` (dispatch list `&[(0, false)]`)
instead of the per-eye loop, so the between-eye snapshot and restore (gated on `ordinal > 0`) is
skipped for free; `hooks/camera.rs` keeps the render camera centred, since both eyes come from `cb13`
and the shadow-anchor delta is zeroed to match; and `render_engine_post_draw` splits the one back
buffer into the two eye textures. This is the actual draw-submission win.

The collapse is mutually exclusive with the far-field share mode
([`far-field.md`](far-field.md)), which needs the per-eye dispatches it composites into. The UI
enforces that.

### Double-wide

With `single_pass.double_wide`, `vr::engine_render_resolution` returns **2x the per-eye width**, which
the deferred-`ApplyResize` native-resolution driver (`vr::resolution`) targets — so the whole scene RT
set (back buffer, `m_BackBufferLinear`, the G-buffers) is re-created double-wide and the per-pass
viewport follows the RT size automatically. The **XR swapchain stays per-eye width**, and the per-eye
capture textures are sized to half the back buffer, so the collapse's capture split copies each
full-width half straight into its eye texture with no squish.

## Four ways content goes wrong under the collapse

Once the two geometry walks became one, a set of things stopped tracking the world and slid across the
screen as the camera moved: sun shadows, terrain patches, foliage, water, decals, clouds and smoke.
They look like one bug and are four, sharing only a shape — a per-eye quantity paired with a
full-width one. Each has its own seam and its own flag.

They also share a signature worth recognising, because it is what makes the symptom *sliding* rather
than a static mis-scale: a fixed 2x horizontal scale error is a **2x horizontal motion gain**. Content
rendered or sampled at the double-wide scale sweeps past at twice the camera's rate, which reads as
sliding over the world.

**Note on what is not the mechanism.** The obvious hypothesis — that some engine constant holds the
wrong screen size — was investigated and is wrong. The engine has exactly one screen-size constant,
`cb0[8].zw` (staged in [`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData)), derived from the device display size, which the mod's `ResizeBuffers` substitute already
sets to the double-wide width; the buffers it addresses are double-wide too, so it is self-consistent,
as are the `ftoi SV_Position` texel fetches (engine `rendering.md` §4.3). Do not go looking there
again.

### 1. Draws that never get an eye viewport bound

The collapse moves the eye split from pass level to draw level, so a draw arriving on an entry point
nothing detours inherits whatever the two viewport slots happen to hold — dominated by
`CollapseViewport::Full`. Two families arrive that way:

- **GPU-indirect draws** (`DrawIndexedInstancedIndirect` slot 39, `DrawInstancedIndirect` slot 40;
  engine `rendering.md` §4.2). The near tessellating terrain patches and the dominant foliage path
  submit here. Fixed by `single_pass.indirect_per_eye` (**on**): re-issue once per eye with both
  viewport slots pinned. The instance counts live in a GPU buffer and cannot be doubled, so re-issue
  is the only option — the geometry is present and correctly sized in both eyes, without parallax.
- **Non-indexed world geometry** (`Draw`, slot 13). Slot 13 is overwhelmingly the fullscreen-pass
  entry point, so the collapse resets to the full viewport for it — which stretches the decal, road
  and skidmark blocks that also submit there across both halves. Skidmarks are the sharp case: their
  vertex shader *is* patched and the blanket reset defeated the mod's own routing. Fixed by
  `single_pass.slot13_per_eye` (**on**), routing an allowlist of passes through `instanced_per_eye`. It
  is an allowlist, not a heuristic, because a fullscreen triangle and a decal box are identical at the
  draw call, and misclassifying a fullscreen pass as geometry is a visibly wrong frame, while a pass
  missing from the allowlist only sees that pass fall back to the un-split, double-wide-stretched
  behaviour — so a pass is added when it is evidenced, never on suspicion.

### 2. One eye-0 reconstruction basis over the whole double-wide target

A fullscreen pass is one draw with one constant buffer, so a pass that rebuilds world position from
depth gets one basis while its quad spans both halves: the left half gets eye 0's frustum compressed
2x horizontally and the right half a basis unrelated to it. The error is a function of the view
matrix, so it turns with the camera — and the two passes that sample the sun cascade over the
reconstructed positions are what make the shadows slide.

The consumer set is closed at seven (engine `rendering.md` §4.1), and each has its own flag:
`single_pass.reconstruct_per_eye` and `single_pass.atmospheric_per_eye` (both **on** — splitting only
the deferred resolve leaves atmospheric scattering painting the mistake back over it),
`single_pass.ssao_per_eye`, `single_pass.ssr_per_eye`, `single_pass.subsurface_per_eye`, and
`single_pass.dof_per_eye` (all **on** by default too, each with its own hazard noted below).
`DrawPassThrough` needs nothing — it is reached only under wireframe.

Two of those blocks do part of their work in **compute**, which no mask can reach: a dispatch ignores
the scissor as thoroughly as it ignores the viewport, its reach is fixed by its thread-group counts,
and both blocks size those from the target's full width and address their textures straight off
`SV_DispatchThreadID` with no origin term in any constant buffer (nor could a UAV supply one — a D3D11
texture UAV picks a mip and an array slice, never a sub-rectangle). So that work is *scheduled* rather
than masked: `reconstruction::DispatchPhase` issues it whole-target on exactly one run of the split —
the **first** for a prologue the masked draws consume (the bokeh near-field coverage), the **last** for
an epilogue that consumes what they produced (the SSR blur). That is the same single whole-target pass
the un-split block makes, so the eye-seam bleed from the horizontal half of each separable blur (six
texels of the block's own working resolution) is inherent to the collapse, not introduced by the split,
and is there with the flags off too.

The shared machinery is `reconstruction::split_fullscreen_pass`, which holds the preconditions and the
demotion rule. Each run is masked with a **scissor**, not a half viewport, so the quad keeps its
one-to-one NDC-to-pixel mapping and its G-buffer sampling stays correct; the per-eye basis comes from
folding the full-target-NDC to eye-NDC remap into the substituted inverse. The demotion rule matters:
a run that never masked has already drawn the whole target, so the split stops after one run rather
than double-exposing an accumulating pass. It reports which of the three happened -- `Split`,
`Demoted`, or `NotTaken` -- because a caller cannot infer it: only `NotTaken` means the pass still
needs issuing, and the froxel split below needs to distinguish a demotion specifically.

`RenderBlockSSDecal` has the same defect without being a `PerspectiveFovInverse` consumer — its
type-level [`RenderBlockTypeSSDecal::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeSSDecal##func-Setup) builds a basis inline into fragment `cb1[0..3]` and its pixel shader derives the
depth-fetch UV projectively from its own clip position. `single_pass.ssdecal_per_eye` (**on**) fixes both
halves together: the block is re-issued per eye restaging that eye's basis, and the 12 `ssdecal` pixel
shaders get one spliced instruction biasing the depth UV into that eye's half. The same `uv` feeds the
reconstruction matrix (which wants the per-eye value) and the depth fetch (which wants the double-wide
one), so they cannot be corrected as one.

The volumetric fog generation pass has the defect too, also without being a `PerspectiveFovInverse` consumer. [`RenderBlockFogVolume::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockFogVolume##func-Draw), in `RP_FOG_VOLUME_GENERATE`, binds the block type's own half-resolution fog target and builds a texture-UV-to-world matrix by inverting the render context's [`m_ViewProjectionF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ViewProjectionF) itself. Each pixel samples depth at its UV, reconstructs through that matrix, and ray-marches the sun cascade and cloud shadow from the camera towards the result, so one centred basis over the double-wide target is wrong in both halves, most visibly around the sun. `single_pass.fog_volume_per_eye` (**on**) runs the block once per eye with the at-entry mask, which follows the block onto its reduced-resolution target. For each masked run it gives the render context that eye's view-projection, pre-multiplied by the inverse of the full-target-NDC to eye-NDC remap, so the inverse the block takes is the eye's with the remap folded in. The depth sample keeps the double-wide UV, the march starts from the eye position the shared camera staging puts in the fragment globals, and the render context is put back after the call. The upsample pass that follows only compares depths at matching UVs and the composite maps the whole fog texture over the whole target, so neither needs a split.

SSAO has the defect too, though its occlusion generation is not where it calls `PerspectiveFovInverse` (that call feeds only the temporal resolve). [`RenderBlockSSAO::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockSSAO##func-Draw) builds the generation pass's basis and normal rotation from the render context's own projection and view (engine `rendering.md` §4.1), so under the collapse one centred basis serves both halves. That pass is also wrong in three ways no basis can fix: its taps are isotropic offsets in screen UV, which span twice the view horizontally on a double-wide target; taps near the seam read the other eye's depth; and its spiral rotation and quad parity come from `SV_Position`, which in the right half is offset by the eye's width. The bilateral blur's outer taps and the composite's bilinear upsample also reach across the seam, by a few texels and one pixel. With SSAO on, the error dominated the shore-lake comparison: 42% and 34% of final pixels differed from the isolated eyes, against about 10% with SSAO off.

`single_pass.ssao_per_eye` (**on**) fixes it without a split. `dxbc_stereo::localize_ambient_occlusion` rewrites the generation shader to take its eye from the half its screen UV falls in, read that eye's basis and rotation rows from a mod-owned pixel constant buffer, halve each tap's horizontal offset in place, clamp every depth and normal sample to the eye's half, and subtract the eye's half of the target width (measured from the UV's own derivative) from the pixel position. `dxbc_stereo::localize_ambient_occlusion_filter` clamps the blur's and the composite's occlusion samples the same way. `stereo::single_pass::ambient_occlusion` builds each eye's rows exactly as the block derives them from an eye's render context, with the horizontal basis terms remapped onto the eye's half, and binds them for the length of the block. Everything else in the block is per pixel or steps whole target texels, and the temporal resolve is seeded fresh on every stereo frame (the `m_FirstPass` force in `hooks/graphics_engine/screen/ssao.rs`), so the block keeps its one run and the same draw count as a single-eye render. The earlier per-eye split, which ran every SSAO draw twice and gave the generation pass no per-eye basis at all, is gone. What remains of the SSAO difference at the shore lake tracks the G-buffer differences the collapse already has (mostly foliage): where the depth and normals agree, the occlusion agrees.

### 3. A projective screen UV in per-eye space against a double-wide buffer

The legacy `Water*`/`WaterBox*` family takes its reflection, refraction, and depth UV from a
CPU-staged matrix, projectively, normalized over the **viewport** while the buffers it indexes are the
whole target (engine `rendering.md` §4.3). `single_pass.water_uv_per_eye` (**on**) composes one more
bias per eye, `u' = (u + eye) · 0.5`, onto the rows the block type staged. Because the type stages them
once per pass, ahead of the `Draw` being re-issued, the fix restages the rows per eye rather than
transforming an upload in flight, and puts the type's own rows back afterwards. It is deliberately
*not* reprojected by `M_eye`: the geometry still rasterizes from the collapsed centre view, so the UV
has to describe where that geometry actually landed. On by default, but the hardest of this group to
A/B: the affected family may not be on screen at all at the water-quality setting in use, which makes a
headset comparison the only way to tell the fix from a no-op. The re-issue lives in
`hooks/graphics_engine/scene/water.rs` and has not been verified in the headset; the
[playtest checklist](../vr-runtime.md#playtest-checklist) carries the check.

> **Not `NvWaterHighEnd`, and not a shader rewrite.** This mechanism is not `NvWaterHighEnd`'s, and its
> fix is not a DXBC rewrite of nine water pixel shaders with eye parity plumbed into a pixel shader.
> `NvWater*` builds its UV from `SV_Position × cb0[8].zw`, which is
> already consistent under double-wide; the projective-UV family is the legacy non-NV one. And the
> defect lives entirely in a CPU-staged matrix, so no bytecode rewrite and no pixel-shader eye parity
> is needed. `NvWater*` has a different defect — its vertex shader writes clip from a baked
> model-view-projection in its own constant buffer, so both eyes see the collapsed centre view. Flat
> water at the wrong depth is invisible in a screenshot and wrong only in a headset, which is why it
> is easy to miss; `single_pass.nvwater_per_eye` (**on**) addresses it, and
> `single_pass.ssdecal_geometry_per_eye` (**on**) is the same shape for the decal box.
>
> The WaveWorks fix substitutes the eye's view and reverse-Z projection into the render context,
> re-runs [`NvWaterHighEndRenderBlock::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::NvWaterHighEndRenderBlock##func-Setup)
> so the block rebakes its matrices and constant buffers from them, and re-issues `Draw` into that
> eye's half, skipping the block's auxiliary foam and wake passes. The second eye's
> [`WaveWorksSimulationStep`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::WaveWorksSimulationStep)
> is suppressed, since it is the one non-idempotent call in the block. It has not been verified in
> the headset either.
>
> One `NvWater*` screen-space read is not consistent under double-wide after all: the planar
> reflection. The reflection pass renders one eye-sized frame from the mirrored camera (the
> [`WaterPatchManager::m_ReflectionCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/water_patch_manager::WaterPatchManager##field-m_ReflectionCamera)),
> and every `nvwater*` pixel shader that binds `g_ReflectionTexture` forms its horizontal coordinate
> as the screen UV divided by `g_UVScale.x`, so each eye reads half of the reflection stretched over
> its own half. At the shore lake that shows as a low-amplitude colour shift over the whole surface,
> opposite in sign in the two eyes, with the water's depth unchanged. `single_pass.water_reflection_per_eye`
> (**on**) gives that division the eye-local numerator `2u - eye`; the rewrite finds the division by
> the resource and constant names, covers the four permutations that bind the reflection, and is
> bound per eye by the re-issue (`stereo/single_pass/water_pixel_variants.rs`) together with the lake
> permutation's halved refraction displacement. The refraction, depth, ambient-occlusion, and
> dynamic local reflection lookups keep the full-target UV, because those buffers are double-wide.
>
> Two more per-eye inputs ride on the same re-issue. The water pixel shaders measure the view vector
> of their Fresnel, reflection, and specular terms from the fragment `GlobalConstants` camera
> position, which the engine stages once per pass from the centred camera; `single_pass.water_camera_per_eye`
> (**on**) moves it to each eye for that eye's draw, with the staging split fullscreen passes use. And
> the second eye's draw can find pixel state the first eye's left bound unbound, so
> `stereo/single_pass/water_fragment_state.rs` captures it during the first eye and fills any slot
> that comes back empty: the shadow maps and their samplers, the WaveWorks constants and samplers,
> and the sky-lighting textures and `LightingFrameConsts`. Without the sky lighting, the second eye's
> water came out uniformly about 2% darker than an isolated render of that eye.

#### The water box surface, and what the legacy family's clip path actually is

The water-box *surface* grid — `NWater::DrawWaterBoxSurface`, run over every registered box from
inside `CNvWaterHighEndRenderBlock::Draw` — was recorded as deliberately uncovered because its type's
`SetupSurface` stages no screen-UV matrix. That is true but incidental; the interesting part is its
clip path, which is the legacy family's and settles what "needs per-eye draw wiring first" meant.

The block stages a box transform (a scale by the half-extents plus a translation of
`centre − camera position`) and `waterboxsurface` does:

    world_rel = box_transform(cb1[0..3]) · position
    clip      = cb0[0..3] · (world_rel + cb0[4])

`cb0[0..3]` is the **full**, translation-bearing view-projection — not the translation-free
`m_OffsetViewProjection` at `cb0[29..32]`. The per-eye register remap covers only `cb0[4]` and
`cb0[29..32]`, so the projection is out of its reach. Disassembling the shipped water vertex shaders
shows this is the whole legacy family's idiom, not one permutation's: `waterbox`, `waterboxbelow`,
`waterboxsurface`, `watershader_lod0`, and `watershader_lod1` all read `cb0[0..3]` plus `cb0[4]`, and
`waterboxclear`, `watergodraysshader`, and `waves` read `cb0[0..3]` alone. (`nvwaterbox` and
`nvwaterbox_tess` read `cb0[4]` alone, with clip from the block's baked matrix — the shape
`baked_cb_block_owns_vs` exists for, and `nvwater*` is on its list.)

The `cb0[4]` reference alone is enough for the remap to claim these shaders, and it is the
"camera-only" misclassification in its most damaging form: the eye offset gets added to the shader's
reconstructed **world position** and the result is then viewed from the centre, so the geometry is
displaced by the eye offset in the *opposite* direction to the parallax it should have had. In
practice a per-eye re-issue draws one instance, so `SV_InstanceID & 1` resolves to eye 0 in both
halves and both eyes get eye 0's displacement — a rigid ~32 mm world-space offset of the water
relative to everything around it, growing as `offset / distance`, visible in a single eye.

Reprojection is the transform these want: it replaces the clip position wholesale with
`M_eye · clip_center`, so it does not care that the source was `cb0[0..3]` rather than a baked matrix,
and it fixes the parallax, the wrong-direction displacement, and the double-wide squash together. It
is unreachable only because `should_reproject_camera_only` is gated on `REPROJECT_NAME_PREFIXES` and
the `water*` family is on the excluded list.

**It cannot simply be un-gated, and this is the coupling to be careful of.** Reprojecting the
geometry moves where it rasterises, and the same shaders emit a *projective screen UV* on `TEXCOORD1`
from a CPU-staged matrix built out of the centre view-projection — the one mechanism 3 above
corrects, deliberately *without* `M_eye`, because "the geometry still rasterizes from the collapsed
centre view, so the UV has to describe where that geometry actually landed". Reproject the geometry
and that stops being true: the staged rows would then have to be rebuilt from the *eye's* full
view-projection rather than the render context's centre one. The two changes are one change. Doing
either alone trades a known-shape defect for a subtler one, so neither has been made.

The surface grid is the awkward member of the set: it is drawn from `NWater::DrawWaterBoxSurface`
rather than from `WaterBoxRenderBlock::Draw`, so the per-eye re-issue in
`hooks/graphics_engine/scene/water.rs` does not reach it, and it would need its own intercept before the
paired fix could be applied to it.

### 4. A reduced-resolution target handed the scene's viewport

The collapse's viewport split was target-blind: it re-derived the eye halves from the recorded *scene*
viewport before every draw in the range, regardless of what the engine actually had bound. Several
passes draw into a shared quarter-resolution off-screen target (engine `rendering.md` §9), and a draw
into a `W × H/2` target handed a `2W × H` viewport is magnified 2x about the target origin and
cropped to a quadrant — for the right eye, whose half starts at `x = W`, clipped away entirely. That
is the clouds-and-smoke sliding. `single_pass.collapse_viewport_follows_target` (**on**) keeps a second record that
is always the live bind, so the split follows the bound target. The single record could not simply be
made to follow the engine, because it is also the scene notion and following a half-resolution post
target would have mis-split the scene itself.

The composes need no matching change and must not get one: they map the whole low-resolution texture
over the whole target with a baked UV, so a half-and-half texture composites correctly on its own, and
both blend rather than overwrite, so re-issuing either per eye would double-composite.

## The glare chain at the eye seam

The glare chain (engine `rendering.md` §3.3) builds the bloom in about two dozen fullscreen draws over targets at a half down to a sixteenth of the screen. Under the collapse every one of them is double-wide with the eyes side by side, and every draw reads its source by the screen UV, so each pixel reads its own eye except at the seam. There the Gaussian and saw blurs' taps, the box downsample's far corner (its tap offsets reach one source texel past the destination texel's footprint), the bilinear upsample that accumulates each level into the next, and the opening's three-by-three neighbourhood all read the other eye, where an eye-sized target would clamp to its own edge. At the sixteenth level one blur spans a large share of an eye, and the levels accumulate, so a sun near one eye's inner edge brightens the other eye's whole inner half and is missing from its own edge. In the midday sky capture with bloom running, 53% and 54% of final pixels differed from the isolated eyes by more than three codes, with the left eye brighter towards the seam and the right eye, whose sun sat near the seam, darker towards it, against 3% of the HDR main colour.

`single_pass.glare_per_eye` (**on**) fixes it without re-issuing anything. `dxbc_stereo::localize_post_filter` rewrites a filter so the eye is the half its screen UV falls in, every filtered sample's horizontal coordinate is clamped to the texel centres of that eye's half of the sampled texture (measured with `resinfo`, so no size constant is involved), every gather is clamped a whole texel in, and every texel load outside the half returns zero, which is what the clamping sampler and an out-of-range load give at the edges of an eye-sized texture. The chain draws with the generic copy program and the box downsample, which other passes share, so the rewrite cannot replace them at creation: `stereo::single_pass::post_filters` builds each qualifying program's rewrite as a variant when the engine creates it (the programs are found by the names their resource tables bind their sources under), and the `Draw` detour binds the variant for the draws issued inside [`GlareEffect::Apply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::GlareEffect##func-Apply) only. The composite's `BloomTexture` and `SecondaryBloomTexture` lookups cross the seam the same way through their bilinear footprint, and since the composite always draws over the whole target, `dxbc_stereo::localize_composite_bloom` rewrites them at creation, on top of the overlay rewrite.

The chain keeps the draw count of a single-eye render, one run per frame, against two under double-draw; the treatment adds a shader lookup and swap per draw on the CPU and a clamp per sample on the GPU. In run 247 the share of final pixels differing from the isolated eyes fell to 1.9% and 2.6% (2,200 and 3,006 pixels per eye, against 1,747 and 2,589 at `after_fog_volume_apply`), with all four modes repeatable. Most of what remains is a stripe two pixels wide on the left eye's inner edge and four on the right eye's, which first appears when the volumetric fog is composited and so predates the post chain.

The level sizes are the screen size shifted right, so the eye boundary falls on a texel boundary at every level only while the eye's width is a multiple of sixteen; otherwise the half the clamp keeps to straddles one texel at the coarser levels.

## Each eye's camera position in the pixel shaders

The engine stages the camera world position into the fragment `GlobalConstants` ([`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData)) once per pass, which under the collapse means from the centred render camera, and 432 of the game's 961 pixel shaders read it: for the view vector of their specular, Fresnel, sheen, and environment terms, for camera distances, and as the origin the forward-lit families' sun-shadow lookup subtracts (engine [`shaders.md`](../../engine/rendering/shaders.md#pixel-shaders-that-read-the-camera-position)). A split fullscreen pass and the water's per-eye draws have each eye's rows staged for them (`single_pass.fullscreen_camera_per_eye`, `single_pass.water_camera_per_eye`, through `stereo::single_pass::eye_globals`), but an instance-doubled draw covers both halves of the double-wide target in one submission, so everything it drew was shaded from the point between the eyes.

The cliff view shows it on the near surfaces. The `character` material folds a cloth-sheen term measured from the camera into the albedo it writes to the G-buffer, and at midday the player's own clothing and gear, a metre from the camera, had a G-buffer albedo differing from an isolated render of the eye at about 25,000 pixels per eye (about 5,900 and 5,300 by more than three codes), at matching depth. G-buffer differences of any size were under 20,300 and 20,800 of the 23,500 pixels per eye whose lit colour (`post_resolve`) differed by more than 3%; the screen-space reflections, which the resolve reads, accounted for 2,800 and 1,700 of the rest, and those reach the car's bodywork, whose own material reads no camera position and whose G-buffer matched.

`single_pass.pixel_camera_per_eye` (**on**) fixes the whole class without adding a draw. `dxbc_stereo::localize_eye_camera` rewrites, at creation, every pixel shader that reads the camera rows: the shader compares its `SV_Position.x` with a boundary and reads the rows from its eye's block of a mod-owned pixel constant buffer at slot 11, which no game pixel shader declares (slot 13, the other free one, carries the ambient-occlusion rows while that block runs, and the screen-space decals' bias). Each eye's block holds the two camera position rows and the cascade translation row, which is baked relative to the camera position and has to move with it: the same three rows `eye_globals` moves for a split pass. `stereo::single_pass::eye_camera` rewrites the buffer at every [`RenderEngine::SetAllGlobalShaderProgramConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetAllGlobalShaderProgramConstants), from the rows the engine has just staged, which is the same cadence `cb13` follows on the vertex side:

- Outside the routed range, and in any frame without the collapse, both blocks hold the staged rows unchanged and the boundary lies past every pixel, so a rewritten shader reads exactly what it would read from `cb0`. The shadow, reflection, environment, and reflective-shadow passes all run in `PreDraw`, outside the range, and keep their own camera's rows. The range guard puts the buffer back to the staged rows when the range ends, for the draws before the next upload.
- Inside the routed range under the collapse, each block holds the staged rows moved to its eye, and the boundary is the first pixel of the right eye's half of whatever target is bound, derived from the viewport the collapse splits (`collapse_viewport_follows_target`), so it follows the reduced-resolution targets. A boundary change rewrites the buffer; the per-bind check costs two relaxed loads.
- The per-eye re-issues and the scissor-masked runs of a split pass land in one half, so they read that eye's block. For a split pass and the water, that block holds the values `eye_globals` stages into `cb0` for the run, so the rows are not applied twice.

Every camera read moves to the eye, including the ones that only feed a fade or a distance threshold. That is what a separately rendered eye computes, and the choices the design keeps on the centre are not made in a pixel shader: the LOD and tessellation selection happen on the CPU and in the hull shader. Two draws crossfading at one pixel both read the same eye, so their fades stay complementary. On the vertex side, the four camera-only reprojected families (`generaljc3`, `landmark`, `layered`, `layeredblend`) keep `cb0[4]` centred on purpose, since their position path adds it back; in all four it feeds only a squared camera-distance fade, so no centred view vector reaches their pixel shaders through an interpolant.

All 432 camera readers are rewritten and none is declined: 183 already declare `SV_Position` with its `.x`, 8 declare it without and have the declaration widened, and 241 gain one from the input signature. The rewrite would decline a shader with no `SV_Position` input, one that already declares slot 11, and one that indexes `cb0` dynamically from a base that could reach a camera row; the 113 that index `cb0` dynamically all start past the cascade translation row. The rewrite composes after the others in the fragment hook. The water's per-eye variants are still built from the engine's own bytecode, since its per-eye draws stage the rows into `cb0`, and the resolve probe in the capture harness accepts the rewritten resolve. It is a shader rewrite, so turning it on needs a shader reload; turning it off takes effect at once, because the buffer then holds the staged rows everywhere.

In run 250 (a new session, so the player's pose and the animated scene differ from run 249's), the midday cliff view's G-buffer albedo differed from the isolated eye by more than three codes at 218 and 67 pixels, against 5,895 and 4,933, and the lit colour at 13,680 and 8,501 pixels, against 23,557 and 23,322; the final image differed at 6.0% and 4.6% of pixels, against 7.0% and 9.3%. At dusk the albedo differences fell from 8,773 and 6,271 pixels to 19 and 45. The deferred resolve, which now reads the camera through the buffer in its masked runs, reconstructs world positions within a millimetre of the isolated eye's. What still differs on the near surfaces is the screen-space reflection buffer the resolve reads (see "Known gaps"), and at dusk also the local-light data in the resolve's `LightingFrameConsts`, which differed from the isolated eye's in run 249 as well. The far-terrain, sky, and shore-lake views stay within their run-to-run range; the shore lake's water stage varies between sessions by more than any change attributable here (from 13,400 to 19,400 differing pixels per eye across runs 247 to 249).

## The clustered froxel grid, per eye

Not a sliding defect, but the same shape and a defect in every collapsed frame: the grid is built with
one eye's projection paired with the **double-wide** tile count, so it is twice as wide as the frustum
it describes and every local light lands in the wrong tiles (each [`LightManager::LIGHT_GRID_TILE_SIZE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager::LIGHT_GRID_TILE_SIZE) pixels wide) — for both eyes, and for the ~20
forward-lit render-block types that sample it as well as the deferred resolve. `foliage` reads it a
frame early, inside the G-buffer range. See engine `lighting-shadow-pipeline.md` §4.1–4.2 for how the
grid is built and who consumes it.

`single_pass.clustered_per_eye` (**on**) rides the per-eye resolve re-issue: each run narrows the
light-assignment viewport to its own half of the tile grid, rebuilds the geometry transform from that
eye's projection, makes the tile bounds affine in the *absolute* tile index, and suppresses the second
run's `Graphics::Clear`. The two halves then compose, on the three properties §4.1 records — the clear
is the only whole-target step, the assignment blend is commutative over disjoint tiles, and the fill is
per-tile-local — so the grid ends the frame valid in **both** halves, which is what the forward
consumers need this frame and next. Nothing is written into engine memory; every substitution is a
constant-buffer upload or viewport/clear state the mod already intercepts.

`single_pass.clustered_per_eye_light_view` (**on**) additionally assigns each eye's lights from that
eye's position, by writing the eye's world offset into the translation row of the uploaded `cb2` view
matrix — algebraically identical to biasing the light positions, and it needs no engine write because
that row is exactly where the engine's own zeroed translation goes. The per-eye display canting is not
applied, only the positional offset. It keeps its own flag because the difference may fall below the
`LIGHT_GRID_TILE_SIZE` tile granularity.

The split **declines itself**, leaving the un-split behaviour and logging once, when the eye seam
would not fall on a whole tile column (double-wide width not a multiple of twice `LIGHT_GRID_TILE_SIZE`), when the bound
assignment target is not the tile grid it sized for, or when the dispatch has no lights.

One decline is worth spelling out, because the two preconditions involved disagree. The resolve mask
refuses unless the bound viewport is the collapse's double-wide target -- a pass binding anything else
is not the one being split -- whereas the assignment split engages as soon as the bound target is the
tile grid it sized for, which an off-scene dispatch can satisfy. When that happens the assignment fills
eye 0's half and the resolve then proves unmaskable, so there is no second run to fill the rest, and
because the grid is shared the missing half shows up on the main scene rather than on the dispatch
responsible. Rebuilding it would mean re-issuing the whole block, resolve included, which is the double
exposure the demotion rule exists to prevent. So the split stands down for that graphics context
instead: maskability is a property of the pass rather than of the run, so one observation settles it,
and the scene's own context -- which does mask -- is unaffected.

## Configuration

All under `stereo.single_pass`, except where a row says otherwise. The master switch
(`single_pass.enabled`) is off by default, and most of the flags that depend on it default on —
reflecting that they fix a defect the collapse introduces, not that they are validated. Each row below
is marked with its actual default.

### Structure

| Flag | Default | Effect |
|---|---|---|
| `single_pass` | Off | Master switch. Substitutes the rewritten shaders and installs the routing. Forced inert without the viewport-routing capability. |
| `single_pass.patch_dryrun` | Off | Runs the rewrite and tallies the outcomes at shader creation without substituting anything. No rendering change; validates the rewriter against the live shader set. |
| `single_pass.dual_eye` | Off | Makes the eyes diverge: distinct per-eye `cb13`, eye-half viewports, instance doubling. On its own (no collapse, no double-wide) each eye renders into half a per-eye target — squished; a bisection step. |
| `single_pass.collapse` | Off | Collapses the per-eye double-draw to one `game.Draw` walk. Requires `single_pass.dual_eye`. |
| `single_pass.double_wide` | Off | Re-creates the scene targets at 2x per-eye width so each eye half is full resolution. Requires `single_pass.collapse` and `vr.native_resolution`. |

### Shader rewrites (need a shader reload to apply)

| Flag | Default | Effect |
|---|---|---|
| `single_pass.reproject` | Off | Reprojects the no-`cb0` scene families (characters, props, buildings, roads) instead of leaving them double-drawn. |
| `single_pass.reproject_camera_only` | On | Extends that to the allowlisted families the `cb0` remap claims on a camera-position reference alone (`generaljc3`, `landmark`, `layered`, `layeredblend`), which otherwise get viewport routing but no per-eye clip. |
| `single_pass.terrain` | Off | Rides the eye index through the terrain tessellation pipeline (VS → HS → DS); also gates the render-block re-issue that reprojects the GPU-indirect terrain-detail pass. |
| `single_pass.terrain_patch_per_eye` | On | On top of `single_pass.terrain`: the terrain patch system's tessellated and GPU-indirect basemesh pipelines, which cannot carry an eye index, take the pinned-eye reprojection, and their per-eye re-issues pin `cb13` to each eye (see "Pipelines that cannot carry the eye"). |
| `single_pass.tree_impostors` | Off | Reprojects the far-distance tree impostors. |
| `single_pass.pixel_camera_per_eye` | On | Every pixel shader that reads the camera position picks the rows of the eye whose half its pixel lies in (see "Each eye's camera position in the pixel shaders"). Turning it off applies at once; turning it on needs a shader reload. |
| `single_pass.tree_impostor_pixel_size_per_eye` | On | Stages the impostors' `PixelSize` at one eye's width under the double-wide collapse (see [single-pass-render-blocks.md](single-pass-render-blocks.md#treeimpostor)). Applies without a shader reload. |

### Per-eye re-issue of a render block

| Flag | Default | Effect |
|---|---|---|
| `single_pass.bark` | On | `RenderBlockBark`'s colour and depth draws. Also declines the `cb0` remap on `vegetationbark*` (see below). |
| `single_pass.foliage` | On | `RenderBlockFoliage`'s colour and depth draws. Also declines the `cb0` remap on `vegetationfoliage*` (see below). |
| `single_pass.occluder` | Off | `RenderBlockOccluder`'s depth prime. **Blind-implemented, unvalidated.** |
| `single_pass.nvwater_per_eye` | On | The WaveWorks water blocks, whose baked model-view-projection leaves both eyes on the centre view. Not yet verified in the headset. |
| `single_pass.water_reflection_per_eye` | On | On top of `single_pass.nvwater_per_eye`: each eye's water re-issue binds a rewrite of the water pixel shader that looks the eye-sized planar reflection up by an eye-local coordinate (`dxbc_stereo::localize_planar_reflection`). Applies without a shader reload. |
| `single_pass.water_camera_per_eye` | On | On top of `single_pass.nvwater_per_eye`: each eye's water re-issue moves the fragment `GlobalConstants` camera position (and the matching sun-shadow cascade translation) to that eye, the staging `single_pass.fullscreen_camera_per_eye` gives a split fullscreen pass, because the water's Fresnel, reflection, and specular view vector is measured from it. |
| `single_pass.ssdecal_geometry_per_eye` | On | The screen-space decal box geometry, same shape. On top of `single_pass.ssdecal_per_eye`. |

### Per-eye viewport and draw routing

| Flag | Default | Effect |
|---|---|---|
| `single_pass.instanced_per_eye` | On | Per-eye re-issue of an already-instanced `DrawIndexedInstanced` with a patched shader bound. |
| `single_pass.indirect_per_eye` | On | Per-eye re-issue of the GPU-indirect draws (mechanism 1). |
| `single_pass.slot13_per_eye` | On | Per-eye re-issue of non-indexed world geometry, by pass allowlist (mechanism 1). |
| `single_pass.collapse_viewport_follows_target` | On | Derives the eye halves from the bound render target rather than the scene viewport (mechanism 4). |
| `single_pass.uniform_viewport_slots` | On | Puts both viewport slots back to one region once the G-buffer range ends, so a patched shader's instance parity is a no-op in the passes that are not eye-split. |

### Per-eye fullscreen and screen-space passes

| Flag | Default | Effect |
|---|---|---|
| `single_pass.reconstruct_per_eye` | On | Deferred clustered-lighting resolve (mechanism 2). |
| `single_pass.fullscreen_camera_per_eye` | On | Every masked per-eye run of a split fullscreen pass: the fragment `GlobalConstants` camera position is moved to that eye for the run, with the matching sun-shadow cascade translation, and put back after. |
| `single_pass.atmospheric_per_eye` | On | Atmospheric scattering / aerial perspective (mechanism 2). |
| `single_pass.fog_volume_per_eye` | On | Volumetric fog generation (mechanism 2): the eye's view-projection, remapped onto its half of the block's half-resolution target, is substituted into the render context for each masked run. |
| `single_pass.ssao_per_eye` | On | SSAO, computed for each eye in the block's single run: the generation, blur, and composite pixel shaders are rewritten (`dxbc_stereo::localize_ambient_occlusion` and `localize_ambient_occlusion_filter`) and the block binds each eye's basis and view rotation for the generation pass. No split, so the temporal history advances once per frame as in an eye-sized render. Needs a shader reload. |
| `single_pass.ssr_per_eye` | On | Screen-space reflections. The `m_UseComputeBlur` epilogue's two dispatches are issued on the second run only, once, over both halves. |
| `single_pass.subsurface_per_eye` | On | Screen-space subsurface skin. |
| `single_pass.dof_per_eye` | On | Depth of field: splits `CDownScale2x2PackFocus::Apply`, whose closing pack draw is the sole consumer of the DoF basis. Its five-dispatch near-field prologue reads no view or projection matrix, so it runs whole-target on the first run only and the second run reads the same coverage. |
| `single_pass.composite_overlays_per_eye` | On | The final composite's vignette and lens dirt, looked up by an eye-local UV in every `comp_*` and `dof_*` permutation that binds them (`dxbc_stereo::localize_screen_overlays`). Needs a shader reload. |
| `single_pass.glare_per_eye` | On | The glare chain's filters and the composite's bloom lookups kept to each eye's half of the double-wide target, in the chain's single run: the filters bind an eye-half variant (`dxbc_stereo::localize_post_filter`) while `GlareEffect::Apply` runs, which applies without a reload, and the composite's bloom lookups are rewritten at creation (`dxbc_stereo::localize_composite_bloom`), which needs a shader reload. |
| `single_pass.ssdecal_per_eye` | On | Screen-space decals: per-eye basis plus a spliced depth-UV bias in the 12 `ssdecal` pixel shaders (mechanism 2). |
| `single_pass.water_uv_per_eye` | On | Legacy water's projective screen UV (mechanism 3). Not yet verified in the headset. |
| `single_pass.clustered_per_eye` | On | Per-eye clustered froxel light grid. |
| `single_pass.clustered_per_eye_light_view` | On | Also assign each eye's lights from that eye's position. |

## Why `PreDraw` is outside the collapse, and why extending it over it would save nothing

`PreDraw` is the second-largest GPU item in the frame (~2.16 ms, `docs/mod/performance/performance.md`) and does
not come through `DrawRenderPassRange`, so it is natural to read it as a large pool of draws the
collapse has not claimed. It is not. **Under the collapse there is exactly one dispatch, so `PreDraw`
already runs exactly once per frame.** `hooks/game.rs` builds the dispatch list as `&[(0, false)]`
when `collapse_active()`, and `GraphicsEngine::HandleDrawThreadTask` calls `RenderEngine::PreDraw`
once per dispatch. There is no second walk to collapse; the ceiling on submission savings from
extending the collapse over `PreDraw` is **zero**.

The measurement agrees, and is the reason to trust this rather than the reading above. In the
off/on comparison in `performance.md`, `PreDraw` is 2.29 ms with two dispatches and 2.25 ms with one.
If it were duplicated per eye, halving the dispatch count would have roughly halved it. It does not,
because the shared pre-passes (`pre_draw` in `hooks/graphics_engine/render_pass.rs`) already elide the view-independent categories on the
second eye of the double-draw path. The ~0.04 ms difference is the residual: the categories that path
still runs twice.

### The classification

[`RenderPass::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-Draw) (the pre-pass entry) selects the pass camera: the pass's own external camera if it has one, otherwise the camera manager's render camera. [`RenderPass::SetRenderContextCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetRenderContextCamera) then overrides that from the render context's pass id for the shadow families. It has three disjoint branches, each selecting a uniform light camera indexed by the pass id relative to the first pass of its family: [`PRE_RP_STATIC_SHADOW_0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-PRE_RP_STATIC_SHADOW_0), [`PRE_RP_SHADOW_0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-PRE_RP_SHADOW_0), and [`PRE_RP_SHADOW_REFLECTIVE_SUN_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-PRE_RP_SHADOW_REFLECTIVE_SUN_NEAR). The passes from `PRE_RP_STATIC_SHADOW_0` through `PRE_RP_SHADOW_REFLECTIVE_CAMERA` are therefore rendered from the sun or light view by construction, not by convention, and cannot depend on which eye is being drawn.

| Category | `RenderPassId` range | Camera | Per-eye? |
|---|---|---|---|
| Terrain-patch prep | `RP_TERRAINPATCH_CLEAR` to `RP_TERRAINPATCH_ENUMERATION` | falls through to the render camera | yes |
| Sky-lighting LUT | `PRE_RP_SKY_LIGHTING` | render camera | held per-eye, conservatively |
| Planar + environment reflections | `PRE_RP_REFLECTION_PRE` to `PRE_RP_ENVREFLECTION` | the pass's own reflection camera | no |
| Cloud shadows | `PRE_RP_CLOUDSHADOWS` | world-space | no |
| Vegetation | `PRE_RP_VEGETATION_UPDATE` to `PRE_RP_VEGETATION_INTERACTION` | render camera | held per-eye, conservatively |
| Sun-shadow cascade atlas, static + dynamic + reflective | `PRE_RP_STATIC_SHADOW_0` to `PRE_RP_SHADOW_REFLECTIVE_CAMERA` | light camera, from the pass id | **no**, verified in the binary |
| Water simulation compute | `PRE_RP_WATER_CS_PRE` to `PRE_RP_WATER_DISPLACEMENT_PRE` | own targets and viewports | no |
| Rain occluder | `RP_RAIN_OCCLUDER` | render camera | held per-eye, conservatively |

`SHARED_PREPASS_CATEGORIES` in `hooks/graphics_engine/render_pass.rs` encodes the "no" rows that have
been validated in game (`PRE_RP_REFLECTION_PRE` to `PRE_RP_CLOUDSHADOWS`, and `PRE_RP_STATIC_SHADOW_0` to `PRE_RP_WATER_DISPLACEMENT_PRE`) and elides them on the second eye of the double-draw path.
Under the collapse that elision is inert and correctly so — nothing is duplicated to elide.

### What is left

The prepasses are already at their floor of one issuance per frame. What remains in `PreDraw` is the
game's own baseline shadow, reflection and vegetation cost, the same as flatscreen, and reducing it is
a shadow/reflection quality decision (cascade count, atlas resolution, update cadence), not a stereo
one. Two stereo-attributable riders are worth knowing about, both small and both deliberate:

- `shadow_manager_update_render` widens the cascade fit to the union FOV of both eyes, which puts more
  geometry inside each cascade and so costs some shadow draws. That is the price of the cascades
  covering both eyes at all.
- `shadow_update_every_frame` (off) would defeat the engine's `2^L` cascade update amortization and
  multiply the shadow render cost. It is a flicker diagnostic; leave it off.

The related item — "roughly 450 instanced draws per frame issued outside the G-buffer range (the
shadow-pass families)" — is the same answer. Those are issued once per frame under the collapse. The
collapse's draw and viewport detours are gated on `in_gbuffer_range()`, which is raised only for
`first >= RP_Z_OCCLUDERS`, so no prepass draw is instance-doubled or eye-split. The only thing that
leaks into them is a patched shader's unconditional `SV_ViewportArrayIndex` write, which
`single_pass.uniform_viewport_slots` (on) already neutralises. There is no submission saving there
either: they are correctness surface, not duplicated work.

## Known gaps

- **Unpatched geometry has no parallax.** It is re-issued per eye so it is present and correctly
  sized in both, but it still reads the centred `cb0`. Patching more of the geometry is what removes
  this. The terrain patch system is no longer in this set: its pipelines take the pinned-eye
  reprojection inside the same re-issue.
- **The terrain patch system is drawn twice.** Its GPU-indirect near passes and its far tessellated
  passes are per-eye re-issues with the eye pinned in `cb13`, correct in both eyes but one submission
  per eye. Bringing them into one draw needs the indirect argument doubling below for the near passes
  and an eye interpolant through the hull stage for the far ones.
- **`cb0`'s projection is double-wide.** Unpatched geometry that reads `cb0` rather than `cb13`
  renders horizontally squashed under double-wide.
- **The sun halo is one sprite per screen.** [`SunHaloEffect::PreApply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::SunHaloEffect##func-PreApply) projects the sun through the render context's single projection onto the scene texture's pixel size and draws one halo there, so under the collapse it lands once, somewhere across the double-wide target, rather than once per eye. It runs only while the sun is low, so the midday sky capture does not exercise it. A per-eye halo needs two projections and two composites, which is a draw-count decision, and has not been made.
- **Screen-space passes leak across the eye seam.** FSR and the unsplit screen-space passes run once
  over the double-wide target and sample neighbourhoods; they need a UV clamp at the seam.
- **The 14 already-instanced shaders** need their `SV_InstanceID` consumers rewritten to
  `SV_InstanceID >> 1` to survive the doubling. The DXBC transform is offline-testable; the
  per-instance semantics are not.
- **GPU-indirect vegetation** needs an in-place compute pre-pass doubling each indirect draw's
  `InstanceCount` (dword +1 of the 5-dword args, in the GPU-only `veg_draw_indirect` buffer, which has
  no CPU copy) before it can be instance-doubled rather than re-issued.
- **Screen-space reflections on near glossy surfaces.** With the pixel shaders shading from each eye, most of what still differs from an isolated eye at the cliff view's car bodywork and the player's gear is in the reflection buffer the deferred resolve reads, at matching G-buffer and depth.
- **Bark's velocity pass** reprojects only the current-frame matrix; the previous-frame copy that [`RenderBlockBark::DrawZ`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockBark##func-DrawZ) stages in `cb1` stays centred, so bark's motion vectors are slightly wrong for SMAA and FSR.
- **The occluder intercept assumes `gfx.occluders.use_instancing` is off**, and nothing forces that cvar. The cvar's storage and default are not yet located.

## Already-instanced draws

A draw whose patched VS takes its per-instance data through a vertex-buffer slot (rather than
`SV_InstanceID`, which would have deferred the shader) arrives at `DrawIndexedInstanced` with the
game's own instance ids. `SV_InstanceID & 1` then reads them as an eye parity nobody asked for, so
instance `i` goes to eye `i & 1` — half the batch per eye, or the left eye alone at one instance. This
is what made the buildings flicker: head motion re-sorts which instances are odd.

**Promoting the instance count cannot fix it.** Per-instance vertex-buffer stepping is indexed by the
instance id, so a doubled count reads past the batch's per-instance data. Instead `instanced_per_eye`
makes the parity irrelevant: for each eye it fills **both** `cb13` eye slots with that eye's
view-projection, camera position and `M_eye`, binds **both** viewport slots to that eye's half of the
double-wide target, and calls the original `DrawIndexedInstanced` with the caller's arguments
unchanged. Whichever parity the shader computes, and whichever `SV_ViewportArrayIndex` it writes, it
resolves to the same eye. Afterwards `cb13`'s dual-eye contents and the exact viewport slots that were
bound on entry are restored — `cb13` unconditionally, since every patched shader in the pass reads it
and a pinned one would collapse the frame to mono.

The `cb13` pin writes through a `WRITE_DISCARD` map of the already-bound buffer rather than
`upload_and_bind`, so a handled draw costs three maps and no COM ref-counting. The rows it pins and
restores are the ones `mirror_and_bind_cb13` last uploaded, cached in `Cb13Buffer::rows` — the draw
detour has no `RenderEngine` to recompute them from.

The gate is exactly the case's predicate: a patched vertex shader bound, the render thread inside the
G-buffer range, the collapse on, and `single_pass.instanced_per_eye` (default **on**). The mod's own
draws are excluded — a promoted `DrawIndexed` is re-issued through the trampoline, and a
bark/foliage/occluder per-eye re-issue by the `PER_EYE_REISSUE` marker, which this re-issue raises too.
Everything else is forwarded once, unchanged.

Cost is one extra submission per such draw: measured at a mean of ~131 draws per frame against ~20k
total, dominated by `generalmkiiihwinstanced` (the buildings), then the `vegetationfoliage*hwinstanced`
and `vegetationbark*hwinstanced` families.

The counters that measure the case split it into **handled** (re-issued per eye)
and **exposed** (the flag off, or the re-issue declined for want of a recorded full viewport, a live
context, or `cb13` contents). Exposed draws are further split by instance count — one instance means
the geometry is simply missing from the right eye, more than one means the batch is halved. Each draw
is attributed to the bound `ID3D11VertexShader`, named where the `CreateVertexProgram` path supplied
one (the re-acquire path does not, and those show as `<unnamed 0x…>`). The Experimental page's Single-pass stereo feature reports the last
frame plus a mean over the measured frames with a per-shader breakdown, and `single_pass`-target log
lines carry the same figures every 120 frames. The counters reset with the patched-shader set (a shader
reload), since both are keyed by shader pointer.

Every instanced draw is also bucketed by whether a patched shader was bound and whether the draw was
inside the G-buffer range, so the log distinguishes the draws the re-issue covers from the ones it
never sees. Only the in-range patched draws are eye-split; the ~10x larger out-of-range population is
the shadow, reflection and post work, where the parity has to be neutralised a different way (below).
The per-shader table carries the same in/out split, so a family that only draws outside the range is
visible rather than absent.

## Viewport slots outside the G-buffer range

A patched vertex shader writes `SV_ViewportArrayIndex = SV_InstanceID & 1` **unconditionally** — the
bytecode cannot tell which pass it is in. Only the G-buffer geometry ever binds an eye-half pair, so
everywhere else slot 1 has to be a duplicate of slot 0 for the odd-parity instances to rasterise at
all. `rs_set_viewports_detour` keeps that true for every viewport the engine binds, but the collapse's
own per-draw split (`ensure_collapse_viewport(Split)`) leaves the slots holding *different* halves, and
that state outlives the range: until the next engine bind, an already-instanced draw's odd instances go
to the other eye's half.

`single_pass.uniform_viewport_slots` (default **on**) closes that window. `VIEWPORT_SLOTS_UNIFORM`
tracks whether the two slots hold the same region — every path that binds them records what it left
behind, and anything unknown counts as non-uniform — and `unify_viewport_slots` re-binds slot 0's
region to both slots when they do not. It runs when the G-buffer range guard drops (which also covers
the GPU-indirect draws nothing detours) and, behind the flag, on an out-of-range `DrawIndexedInstanced`
with a patched shader bound. **Slot 0 is never touched**, so the repair can only make dropped instances
reappear; it cannot move what already rendered. The log reports how many repairs a window needed as
`slot-1 repairs`.

## The non-`cb0` shader census

From a census of the shipped set (`m_Name` against the rewrite outcome; the payload re-dumps it on the next
shader reload with `single_pass.dump_vs_name_census`). **The census only sees shaders an area actually loads, so this list
may miss families from biomes or weather not visited; it is a floor, not a complete set.**

**Reprojected — the scene-geometry allowlist** (`REPROJECT_NAME_PREFIXES`, matched by name prefix):
`character*`, `creature` (skinned and rigid NPCs, ~34 permutations including `characterskin*`,
`characterdepth*`, `characteroutline`, `charactersphdecal*`, `charactervelocity*`); `general*`
(`general`, `generalglint`, `generalmaskedjc3`, `generaloutline`, `generalprez`, `generalprezvelocity`,
`generalrsm`, `generalshadow` — the static and dynamic models); `prop`, `propdecal`; `buildingjc3`,
`buildingrsm`; `window`; `materialtune`; `open`; `flag`, `flagdepthonly`; `snow`; `skidmarks`,
`skidmarks_normal`; roads `junctionroad*`, `splineroad*`, `dirtroad`. Plus `treeimpostor*` under
`single_pass.tree_impostors`, whose `PixelSize` is restaged per eye by
`single_pass.tree_impostor_pixel_size_per_eye`.

**Candidate scene models not on the allowlist** (unclear family, held back pending a look): `box`,
`notex`, `lrpc`.

**Terrain — the tessellation path, not the VS reprojection.** The VS writes no `SV_Position`, so these
skip the VS reproject: `volumetricterrain*` (~30 permutations: `*4`, `*blend`, `*instanced`,
`*notessellation*`, `*offset*`, `*shadow*`), `terrainscroller`, `terrainshaderforest*`, `controlpoint`.
The non-tessellated permutations write `SV_Position` and take the VS reprojection. The patch
system's `volumetricterrain4`, `volumetricterrain4blend`, `volumetricterrain4instanced`, and
`volumetricterrain4blendinstanced` fill all of `TEXCOORD3`, so their eye-inject declines and their
`volumetricterrain4*` domain shaders take the pinned-eye reprojection instead; the
`volumetricterrain4notessellation*instanced` basemesh shaders already read `SV_InstanceID` and take
it too. `terraindetailrt*` is deliberately absent — the terrain-detail block is GPU-indirect and is reprojected
by a render-block intercept that rebuilds its `cb1`, so its vertex shader must stay pristine.

**Vegetation — GPU-indirect.** `vegetationbark*`, `vegetationfoliage*`, `leaves`, `grass`,
`veginteractionvolume`, `vegintrecenter`, `vegintrecovery`. Handled by the per-eye render-block
re-issue where a block flag covers them, double-drawn otherwise.

**Excluded — NDC / non-scene** (`M_eye` would corrupt these; kept double-drawn):

- Sky and atmosphere: `skybox`, `skygradientshader`, `skymodelshader`, `atmosphereprecompute`,
  `atmosphericscattering`, `starsshader`, `softclouds`, `cirruscloudsshadow`, `foggradientshader`,
  `underwaterfoggradientshader`, `addfogvolume`, `fogvolumeapplyfs`, `fogvolumeblur`.
- Post and screen-space: `fxaa`, `temporalaafilter`, `testnoaa`, `motionblur`, `depthoffield`,
  `depthoffieldwithvpi`, `edgedetectionfilter`, `screenspace`, `screenspacetex*`,
  `screenspacesubsurfaceskinseparableblur`, `patternrecognitionfilter*`, `pixelreconstructionfilter`,
  `recreatepositionfromscreenspacedepth`, `ssao_sao`, `ssao_temporalfilter`,
  `deferred_clusteredlighting`, `lightassignmentfill`, `scenevisinit`, `gionly`,
  `skindiffuselightingcapture`.
- UI: `gui`, `quickui`, `2dtex1`, `2dtex2`, `3dtext`, `line3d`, `video`.
- Particles, effects, lights: `particleeffect*`, `beam`, `contrails`, `trail`, `billboard`,
  `meshparticle`, `distortionparticle`, `fxmeshfire`, `bulletsmoke`, `bullet`, `halo`, `halomask`,
  `lensflare`, `sunbeam`, `lightglow`, `lightsource`, `lightsource_fakelight`, `lightningshader`,
  `bavariumshield`, `spotlightcone`, `pointlightreflection`, `alphamask`, `aobox`.
- Water: `nvwater*`, `water*` (`waterboxclear`, `waterbumpcomposite`, `waterdisplacementoverride`,
  `waterfoamsub`, `watergodraysshader`, `watermask`, `waterpaintfoam`, `watersurface`, `waterwake`),
  `waves`, `mirror`.
- Decals (held back — surface-projected, want a look first): `decal`, `decaldeformable`, `decalsimple`,
  `decalskinned`, `decalskinnedgeneralmkiii`, `decalskinnedgeneralmkiiidestructible`, `ssdecal`,
  `ssdefault`.
- Occluders, probes, prez, shadow-only: `rainoccluder`, `rainoccluderblur`, `sphericalharmonicprobe`,
  `renderprez*`, `renderpreznotex`, `rendershadow`, `layeredrsm`, `depthwrite`, `tex0shadow`,
  `tex0shadownoalpha`.
