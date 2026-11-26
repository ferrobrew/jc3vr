# Single-pass stereo

Double-draw stereo renders the scene twice per frame — one full CPU walk of the render-pass list per
eye. Profiling showed the frame is **draw-call-submission bound**, not GPU bound: ~20k draw calls per
frame (~9,900 per eye, doubled), GPU only ~41% utilised, no queue syncs. Single-pass collapses the two
geometry walks into one, so the GPU is fed from a single submission. See
[`profiler.md`](../performance/profiler.md) for how the bottleneck was measured, and
[`single-pass-render-blocks.md`](single-pass-render-blocks.md) for the per-render-block treatment.

Single-pass is the default rendering mode while a VR session runs. Multipass stays available: the debug UI's single-pass section has one button that switches the whole feature off or back on (`stereo::config::set_single_pass_feature`), and the choice is saved. Single-pass is inert without a running VR session (on the desktop, with the runtime missing, or with the headset disconnected the game renders as it ships) and without the DXVK viewport-routing capability.

### Turning on and off

The feature is live when its master switch is on, a VR session is running, and the device reports the routing capability (`stereo::single_pass::active`). The session state is latched at the frame start's configuration refresh, from the previous frame's runtime pump, so a session that starts or stops changes every reader of the next frame at once. Single-pass's rewrites are applied when the engine creates a shader, so the live shader set has to be re-created whenever that state changes: the frame driver compares it each frame with the state the live set was created under (`hooks::graphics_engine::shader::reconcile_single_pass`) and requests the reload, which runs in the same frame before the draws. This is what brings single-pass up after a late injection into a running game, where the game already holds its pristine shaders, and on the first frame of the first VR session after any injection; nothing needs to be toggled. The double-wide target and the native resolution follow from the same state the next time the resolution driver runs, and the draw-path detours install with the first rewritten shader. A session that stops takes the shaders back to the pristine set the same way, and an eject restores them as well (`restore_original_shaders_on_eject`), with the scene targets and the game's own resolution.

The settings file stores only what differs from the defaults (see [settings.md](../settings.md)), so a player who never touched single-pass follows its default, on. Switching it off with the button writes the group's switches as `false` under `[stereo.single_pass]`, which loads as multipass. A file written before the format carried a version held every setting, and so held the group off as it then shipped; its upgrade drops the group's `false` values, so the group loads whole rather than half on.

## The technique

**Instance-doubled, double-wide, viewport-routed** — for scene geometry. The fullscreen passes run
once over the whole target by default; the ones that reconstruct position from depth are re-issued
per eye under a scissor mask instead (see "Four ways content goes wrong under the collapse").

- **Routing.** A vertex shader writes `SV_ViewportArrayIndex` directly — the D3D11.3
  `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` capability, which DXVK reports and compiles (gated
  on the Vulkan `shaderOutputViewportIndex` feature). No geometry shader involved.
  `stereo::layout::probe` checks it at runtime and the whole feature stays inert if it is absent.
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

#### The viewport record

What the split is cut from and what the slots hold live in one record (`viewport_record`), read on every routed draw: the last scene-sized viewport the engine bound, the engine's latest bind of any size, and the pair the two slots hold now. Every bind of the slots writes it: the engine's through `rs_set_viewports_detour` (whatever its count; a set of one slot or more than two leaves the pair unknown), and the mod's own through the trampoline where it makes them. A sequence lock makes the read a few plain loads, where a lock taken on every draw costs a locked instruction that waits for the runtime's stores to drain. The eye camera's per-target rows (`eye_camera::viewport_changed`) read the record, so an engine bind is recorded before they are laid out.

With `single_pass.viewport_bind_cache` on, `ensure_collapse_viewport` binds nothing when the slots already hold the pair it wants, which is most draws: a pass's patched draws all want the same split. The pair is forgotten at every pass boundary, so a change no detour saw (a state reset, which nothing calls inside a dispatch) cannot outlive the pass it happened in. At the capital the skipped binds were worth under a tenth of a millisecond of the draw task (run 323, interleaved with the switch off): DXVK's viewport bind is cheap.

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

A remapped shader that reads the view-projection rows but not `cb0[4]` takes the camera position it subtracts before them from `cb0[9]` instead: the render camera's position, which [`RenderEngine::SetGlobalShaderConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetGlobalShaderConstants) stages and the per-pass camera staging leaves alone. See "The road surfaces' clip origin, per eye".

## Reference structures (what fxc emits)

Compiling a reference VS that writes `SV_ViewportArrayIndex` (`scripts/dxbc.sh compile`, a
`D3DCompile` harness over the `dxbc-tool` crate) settles the encodings the transform reproduces. It is
committed as `crates/dxbc-stereo/tests/data/ref_vs50.dxbc` and the unit tests diff against it byte for byte.

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

After the clip-origin copies, `cb13` carries a selector row (`EYE_ROWS_SELECT_ROW`, set in a dual-eye upload and zero in a mirror) and each eye's copy of the full, translation-bearing view-projection (from `FULL_VIEW_PROJECTION_ROW_BASE`, four rows per eye, the `M_eye` block's stride). Those serve the shaders that read an eye's own rows in place of being reprojected; see "Each eye's own rows, not the centre's reprojected".

### Exact only in real arithmetic

`M_eye · clip_centre` equals `VP_eye · world` in real arithmetic. In floats the two round differently: the centre's clip position is rounded once through the centre's matrix and again through `M_eye`, while an eye rendered on its own rounds once through its own. At the headset's size the difference is a few to a few hundred floats of depth on every reprojected surface, a different amount per triangle or patch, and a sub-pixel shift that changes a code of albedo or normals or flips an alpha-tested edge. Where a shader's own math is "a position times a view-projection", the exact form is to give it the eye's view-projection and camera position at the point it reads them, so it does the arithmetic an isolated render does. The `cb0` remap is that form for the offset view-projection; "Each eye's own rows, not the centre's reprojected" lists the families converted since and the ones for which no such form exists.

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
- the **DS** reads the eye from `vicp[0][lane].z` and writes `SV_ViewportArrayIndex` — legal from
  the last pre-rasterization stage under the capability. It then reads its camera rows, the offset
  view-projection and camera position of its terrain constants, from that eye's `cb13` slot
  (`dxbc_stereo::rebase_domain_shader`). `cb13` is bound on the domain stage for this.

The hull and domain shaders are gated *structurally* (only shaders carrying the free lane transform;
the rest return an error and are left untouched), since they are created without a paired-VS identity.
Shadow-pass permutations are excluded by name, and an unnamed shader counts as a shadow pass — failing
closed costs a terrain draw its single-pass treatment, failing open would eye-transform a shadow-atlas
draw.

### Pipelines that cannot carry the eye: the pinned-eye reprojection

The terrain patch system ([`RenderBlockTerrainPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch)) has two pipelines the lane path cannot reach. Its tessellated permutations (`volumetricterrain4*`, with the `volumetricterrain4`, `volumetricterrain4near`, and `volumetricterrain4silhouette` domain shaders) use vertex shaders that write all four components of `TEXCOORD3`, so there is no free lane, the eye-inject declines, and the domain shader builds clip as `(world − camera) · m_OffsetViewProjection` from its own `cb1` (the [`RenderContext::m_OffsetViewProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_OffsetViewProjection) the block type bakes there) with no eye to choose by. Its GPU-indirect basemesh permutations (`volumetricterrain4notessellation*instanced`) project through the full view-projection in `cb0` rows 0 to 3, which the remap does not cover, and already read `SV_InstanceID`, so neither the remap nor the instance-parity reprojection takes them. Neither kind of draw can be instance-doubled either: the near passes are GPU-indirect, and the far tessellated ones have unpatched vertex shaders. Every one of them was already submitted once per eye (the indirect re-issue and the unpatched `DrawIndexed` re-issue), but both submissions were drawn from the centred camera. At the midday shore-lake view that was the near terrain under the whole lower half of each eye: about 4,400 pixels per eye whose G-buffer differed from an isolated render of that eye, at a depth that matched the isolated render shifted by exactly the half-IPD parallax.

`dxbc_stereo::reproject_pinned_eye` rewrites such a shader to post-multiply its own `SV_Position` by the `M_eye` in `cb13`'s first eye slot, with no instance input and no viewport output: the same epilogue as the reprojection, with the row base held at zero. That is exact only in real arithmetic (see "Exact only in real arithmetic"), so the two terrain shapes take the eye's own rows instead: a patch domain shader reads its camera rows from the first slot (`dxbc_stereo::rebase_pinned_domain_shader`), and a basemesh vertex shader reads the first slot's full view-projection in place of `cb0` rows 0 to 3 (`dxbc_stereo::remap_pinned_full_view_projection`). The creation hooks apply it to a terrain domain shader that the lane reprojection declines for want of a lane, and to a terrain vertex shader that already declares `SV_InstanceID`, and tag the resulting D3D object with a private-data marker. The vertex shader stays out of the patched set, so its draws keep their per-eye submission. Both per-eye re-issues check the bound vertex and domain shader for the marker and, when one carries it, pin both `cb13` eye slots to the eye being drawn for each submission (`stereo::single_pass::pinned_eye`), the pin the already-instanced re-issue uses, and restore the dual-eye rows afterwards. The domain shader's `cb13` binding is the one the lane path already relies on.

What stays centre-derived is what should: the hull shader's tessellation factors and its patch culling (`TessOrigin` and the culling direction in the hull `cb1`), and the patch selection. The tessellation origin is the active camera's position, which is the centred head camera in every mode, so an isolated eye tessellates from the same origin; a hull shader made to read its eye's camera position for it moved the tunnel's patches further from the isolated eyes, not closer. Under the eye rows the domain shader's distance outputs and LOD fade are the eye's, as in an isolated render. Outside the routed range `cb13`'s `M_eye` is the identity, so the reflection passes that reuse these shaders are unchanged, and the shadow permutations are excluded by name as on the lane path. No draw is added and none is removed. Instance-doubling these draws instead would need an eye index in their instance count, which for the GPU-indirect passes lives in an argument buffer the CPU never sees (the same blocker as the vegetation's, under "Known gaps"), and for the far tessellated ones a new interpolant forwarded through the hull stage.

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
| `static_lights` | the static light sprites' inverse screen width at one eye's width |
| `config_snapshot` | the per-frame and per-dispatch flag snapshots, and the collapse and double-wide bits they publish to `stereo::layout` for the code outside single-pass |
| `instanced_exposure`, `frame_diagnostics` | measurement only; nothing here changes a draw |
| `probes` | the capture harness's probes into the draw path, registered by the harness at install and skipped behind one relaxed flag until the first capture or snapshot request |
| `detour_timing` | the draw detours' own cost, armed by a benchmark run |

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
| Stage the light sprites' inverse screen width at one eye's width | `RenderBlockTypeLights::Setup` |
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

Under single-pass, `hooks/game.rs` runs one `game.Draw` (dispatch list `&[0]`)
instead of the per-eye loop, so the between-eye snapshot and restore (gated on `ordinal > 0`) is
skipped for free; `hooks/camera.rs` keeps the render camera centred, since both eyes come from `cb13`
and the shadow-anchor delta is zeroed to match; and `render_engine_post_draw` splits the one back
buffer into the two eye textures. This is the actual draw-submission win.

### Double-wide

Under single-pass, `vr::engine_render_resolution` returns **2x the per-eye width**, which
the deferred-`ApplyResize` native-resolution driver (`vr::resolution`) targets — so the whole scene RT
set (back buffer, `m_BackBufferLinear`, the G-buffers) is re-created double-wide and the per-pass
viewport follows the RT size automatically. The **XR swapchain stays per-eye width**, and the per-eye
capture textures are sized to half the back buffer, so the collapse's capture split copies each
full-width half straight into its eye texture with no squish.

## Four ways content goes wrong under the collapse

Once the two geometry walks became one, a set of things stopped tracking the world and slid across the
screen as the camera moved: sun shadows, terrain patches, foliage, water, decals, clouds and smoke.
They look like one bug and are four, sharing only a shape — a per-eye quantity paired with a
full-width one. Each has its own seam and its own fix.

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

The consumer set is closed at seven (engine `rendering.md` §4.1).
`single_pass.reconstruct_per_eye` splits the deferred resolve, and atmospheric scattering is always
split with it (splitting only the deferred resolve leaves atmospheric scattering painting the mistake
back over it). `single_pass.ssao_per_eye` and `single_pass.ssr_per_eye` (both **on** by default) split
their blocks, and the subsurface skin and the depth of field are always split, each with its own
hazard noted below.
`DrawPassThrough` needs nothing — it is reached only under wireframe.

Two of those blocks do part of their work in **compute**, which no mask can reach: a dispatch ignores
the scissor as thoroughly as it ignores the viewport, its reach is fixed by its thread-group counts,
and both blocks size those from the target's full width and address their textures straight off
`SV_DispatchThreadID` with no origin term in any constant buffer (nor could a UAV supply one — a D3D11
texture UAV picks a mip and an array slice, never a sub-rectangle). The depth-of-field prepass is
split, so its compute work is *scheduled* rather than masked: `reconstruction::DispatchPhase` issues it
whole-target on the **first** run of the split, since the masked draws consume what it produces (the
bokeh near-field coverage). That is the same single whole-target pass the un-split block makes, so the
eye-seam bleed from the horizontal half of its separable blur is inherent to the collapse, not
introduced by the split, and is there with the flag off too. The screen-space reflection block is not
split (see "Screen-space reflections, per eye" below); its compute blur, which the block takes only
when [`m_ComputeBlur`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceReflection##field-m_ComputeBlur) selects its compute path, runs once over the whole target as it always did.

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

The volumetric fog generation pass has the defect too, also without being a `PerspectiveFovInverse` consumer. [`RenderBlockFogVolume::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockFogVolume##func-Draw), in `RP_FOG_VOLUME_GENERATE`, binds the block type's own half-resolution fog target and builds a texture-UV-to-world matrix by inverting the render context's [`m_ViewProjectionF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ViewProjectionF) itself. Each pixel samples depth at its UV, reconstructs through that matrix, and ray-marches the sun cascade and cloud shadow from the camera towards the result, so one centred basis over the double-wide target is wrong in both halves, most visibly around the sun. The collapse runs the block once per eye with the at-entry mask, which follows the block onto its reduced-resolution target. For each masked run it gives the render context that eye's view-projection, pre-multiplied by the inverse of the full-target-NDC to eye-NDC remap, so the inverse the block takes is the eye's with the remap folded in. The depth sample keeps the double-wide UV, the march starts from the eye position the shared camera staging puts in the fragment globals, and the render context is put back after the call. The upsample pass that follows only compares depths at matching UVs and the composite maps the whole fog texture over the whole target, so neither needs a split.

SSAO has the defect too, though its occlusion generation is not where it calls `PerspectiveFovInverse` (that call feeds only the temporal resolve). [`RenderBlockSSAO::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockSSAO##func-Draw) builds the generation pass's basis and normal rotation from the render context's own projection and view (engine `rendering.md` §4.1), so under the collapse one centred basis serves both halves. That pass is also wrong in three ways no basis can fix: its taps are isotropic offsets in screen UV, which span twice the view horizontally on a double-wide target; taps near the seam read the other eye's depth; and its spiral rotation and quad parity come from `SV_Position`, which in the right half is offset by the eye's width. The bilateral blur's outer taps and the composite's bilinear upsample also reach across the seam, by a few texels and one pixel. With SSAO on, the error dominated the shore-lake comparison: 42% and 34% of final pixels differed from the isolated eyes, against about 10% with SSAO off.

`single_pass.ssao_per_eye` (**on**) fixes it without a split. `dxbc_stereo::localize_ambient_occlusion` rewrites the generation shader to take its eye from the half its screen UV falls in, read that eye's basis and rotation rows from a mod-owned pixel constant buffer, halve each tap's horizontal offset in place, clamp every depth and normal sample to the eye's half, and subtract the eye's half of the target width (measured from the UV's own derivative) from the pixel position. `dxbc_stereo::localize_ambient_occlusion_filter` clamps the blur's and the composite's occlusion samples the same way. `stereo::single_pass::ambient_occlusion` builds each eye's rows exactly as the block derives them from an eye's render context, with the horizontal basis terms remapped onto the eye's half, and binds them for the length of the block. Everything else in the block is per pixel or steps whole target texels, and the temporal resolve is seeded fresh on every stereo frame (the `m_FirstPass` force in `hooks/graphics_engine/screen/ssao.rs`), so the block keeps its one run and the same draw count as a single-eye render. The earlier per-eye split, which ran every SSAO draw twice and gave the generation pass no per-eye basis at all, is gone. What remains of the SSAO difference at the shore lake tracks the G-buffer differences the collapse already has (mostly foliage): where the depth and normals agree, the occlusion agrees.

#### Screen-space reflections, per eye

[`RenderBlockScreenSpaceReflection::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceReflection##func-Draw) captures the scene colour and G-buffer gloss into its half-resolution targets, traces the reflections with one fullscreen draw (`screenspaceraytracereflection`), and blurs the result with two separable draws. The ray march rebuilds each pixel's world position along a view ray its vertex shader interpolates from the `PerspectiveFovInverse` basis, then projects the reflected ray's two ends through the render context's view and projection, steps between them in the pixels of the target (whose size it is handed), samples the linear depth at each step, turns a hit back into the world through the camera transform, and reads the reflected colour from the previous frame's scene through the previous view-projection (and masks out the reflective water planes through the current one). None of those matrices is the basis the per-eye split substituted, so splitting the block per eye, which is what `single_pass.ssr_per_eye` did first, left both runs projecting through the same eye-sized projection onto the double-wide depth texture: a left-eye pixel's ray, in the eye's own UV, read depth from the whole double-wide texture, and the history lookup through the engine's previous view-projection, which is the centred camera's over the double-wide aspect, landed both eyes on the middle of the previous frame.

At the midday cliff view (run 253) that was most of what still differed on the near glossy surfaces. The deferred resolve's probe, which records each input the resolve reads, found the world position matching the isolated eye at all but 93 and 229 of the observed pixels per eye, and of the 11,867 and 8,457 pixels on those matching positions whose lit colour (`post_resolve`) differed by more than 3%, the reflection sample the resolve read differed at 10,149 and 6,531; the shadow sample differed at 6 and 1,485 and the skin sample at none. Between two isolated captures of the same eye in the same schedule, the reflection sample differed at 4 and 0 of those pixels.

`single_pass.ssr_per_eye` (**on**) now traces both eyes in the block's single run. `dxbc_stereo::localize_screen_space_reflection` rewrites the ray march to take its eye from the half its screen UV falls in, rebuild the view ray from three per-eye coefficients of that UV, read the eye's view, projection, camera transform, and the two view-projections from a mod-owned pixel constant buffer, march against a target size halved horizontally (so the march runs in the eye's own pixels and stops at the eye's edge, as it does at the edge of an eye-sized target), move every depth sample along the march into the eye's half, and make the pixel position its start is jittered by eye-local. `stereo::single_pass::screen_space_reflection` builds the rows from each eye's projection and pose as an eye-sized render stages them; the view-projection for the water-plane mask is remapped onto the eye's half, and the previous view-projection is the engine's staged one moved onto the eye's half, which is exactly the lookup an isolated eye render makes (the engine's previous view-projection is the centred camera's in an isolated render too, so matching it, not correcting it, is what keeps the eyes equal to their isolated renders). `dxbc_stereo::localize_reflection_blur` clamps the blur's taps to the eye's half, which is what the clamping sampler does at the edge of an eye-sized target. The scene-colour capture is per pixel and needs nothing. The camera position the ray march starts from is already per pixel (see "Each eye's camera position in the pixel shaders"). The simpler `screenspacereflection` permutation, which marches through the current view-projection over the target's own UV, gets the per-eye view ray and view-projections and keeps its double-wide march.

The block draws four times under the collapse, as an eye-sized render does, where the split drew eight.

In run 254 the reflection sample the resolve reads differed from the isolated eye's, on pixels whose world position matched, at 3,692 and 6,042 pixels per eye at the midday cliff view and 2,302 and 2,455 at dusk, against 27,668 and 17,811 and 10,488 and 17,191 with the split in run 253, and against 11,317 and 16,884 and 7,993 and 15,051 in run 255, a session built with the switch off (one unsplit run, unrewritten). Among the pixels whose lit colour differed by more than 3%, those with a differing reflection sample fell to 1,918 and 816 at midday and 1,308 and 384 at dusk; the car's bodywork no longer differs. What is left there tracks the reflected scene itself: the previous frame's scene the march reads differs wherever the final image does. The lit-colour differences that remain at the cliff view are not reflections (the resolve's world position, reflection, shadow, and skin inputs all match on them): a speckle over the grass, present with the switch off too, and, in run 254, the player's body near a held device, whose resolve local-light data (`LightingFrameConsts`) differed from the isolated eye's. Each session places the player and the held items differently, so the final-image counts of runs 253, 254, and 255 are not comparable with each other.

### 3. A projective screen UV in per-eye space against a double-wide buffer

The legacy `Water*`/`WaterBox*` family takes its reflection, refraction, and depth UV from a
CPU-staged matrix, projectively, normalized over the **viewport** while the buffers it indexes are the
whole target (engine `rendering.md` §4.3). The collapse composes one more
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
> reflection. The reflection pass renders one frame from the mirrored camera (the
> [`WaterPatchManager::m_ReflectionCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/water_patch_manager::WaterPatchManager##field-m_ReflectionCamera)),
> which the collapse keeps at one eye's field (see "The planar reflection
> camera, at one eye's size"),
> and every `nvwater*` pixel shader that binds `g_ReflectionTexture` forms its horizontal coordinate
> as the screen UV divided by `g_UVScale.x`, so each eye reads half of the reflection stretched over
> its own half. At the shore lake that shows as a low-amplitude colour shift over the whole surface,
> opposite in sign in the two eyes, with the water's depth unchanged. The collapse gives that division the eye-local numerator `2u - eye`; the rewrite finds the division by
> the resource and constant names, covers the four permutations that bind the reflection, and is
> bound per eye by the re-issue (`stereo/single_pass/water_pixel_variants.rs`) together with the lake
> permutation's halved refraction displacement. The refraction, depth, ambient-occlusion, and
> dynamic local reflection lookups keep the full-target UV, because those buffers are double-wide.
>
> Two more per-eye inputs ride on the same re-issue. The water pixel shaders measure the view vector
> of their Fresnel, reflection, and specular terms from the fragment `GlobalConstants` camera
> position, which the engine stages once per pass from the centred camera; the re-issue moves it to each eye for that eye's draw, with the staging split fullscreen passes use. And
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

The collapse fixes it without re-issuing anything. `dxbc_stereo::localize_post_filter` rewrites a filter so the eye is the half its screen UV falls in, every filtered sample's horizontal coordinate is clamped to the texel centres of that eye's half of the sampled texture (measured with `resinfo`, so no size constant is involved), every gather is clamped a whole texel in, and every texel load outside the half returns zero, which is what the clamping sampler and an out-of-range load give at the edges of an eye-sized texture. The chain draws with the generic copy program and the box downsample, which other passes share, so the rewrite cannot replace them at creation: `stereo::single_pass::post_filters` builds each qualifying program's rewrite as a variant when the engine creates it (the programs are found by the names their resource tables bind their sources under), and the `Draw` detour binds the variant for the draws issued inside [`GlareEffect::Apply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::GlareEffect##func-Apply) only. The composite's `BloomTexture` and `SecondaryBloomTexture` lookups cross the seam the same way through their bilinear footprint, and since the composite always draws over the whole target, `dxbc_stereo::localize_composite_bloom` rewrites them at creation, on top of the overlay rewrite.

The chain keeps the draw count of a single-eye render, one run per frame, against two under double-draw; the treatment adds a shader lookup and swap per draw on the CPU and a clamp per sample on the GPU. In run 247 the share of final pixels differing from the isolated eyes fell to 1.9% and 2.6% (2,200 and 3,006 pixels per eye, against 1,747 and 2,589 at `after_fog_volume_apply`), with all four modes repeatable. Most of what remains is a stripe two pixels wide on the left eye's inner edge and four on the right eye's, which first appears when the volumetric fog is composited and so predates the post chain.

The level sizes are the screen size shifted right, so the eye boundary falls on a texel boundary at every level only while the eye's width is a multiple of sixteen; otherwise the half the clamp keeps to straddles one texel at the coarser levels.

## Each eye's camera position in the pixel shaders

The engine stages the camera world position into the fragment `GlobalConstants` ([`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData)) once per pass, which under the collapse means from the centred render camera, and 432 of the game's 961 pixel shaders read it: for the view vector of their specular, Fresnel, sheen, and environment terms, for camera distances, and as the origin the forward-lit families' sun-shadow lookup subtracts (engine [`shaders.md`](../../engine/rendering/shaders.md#pixel-shaders-that-read-the-camera-position)). A split fullscreen pass and the water's per-eye draws have each eye's rows staged for them (through `stereo::single_pass::eye_globals`, with `single_pass.fullscreen_camera_per_eye` for the split pass), but an instance-doubled draw covers both halves of the double-wide target in one submission, so everything it drew was shaded from the point between the eyes.

The cliff view shows it on the near surfaces. The `character` material folds a cloth-sheen term measured from the camera into the albedo it writes to the G-buffer, and at midday the player's own clothing and gear, a metre from the camera, had a G-buffer albedo differing from an isolated render of the eye at about 25,000 pixels per eye (about 5,900 and 5,300 by more than three codes), at matching depth. G-buffer differences of any size were under 20,300 and 20,800 of the 23,500 pixels per eye whose lit colour (`post_resolve`) differed by more than 3%; the screen-space reflections, which the resolve reads, accounted for 2,800 and 1,700 of the rest, and those reach the car's bodywork, whose own material reads no camera position and whose G-buffer matched.

`single_pass.pixel_camera_per_eye` (**on**) fixes the whole class without adding a draw. `dxbc_stereo::localize_eye_camera` rewrites, at creation, every pixel shader that reads the camera rows: the shader compares its `SV_Position.x` with a boundary and reads the rows from its eye's block of a mod-owned pixel constant buffer at slot 11, which no game pixel shader declares (slot 13, the other free one, carries the ambient-occlusion rows while that block runs, and the screen-space decals' bias). Each eye's block holds the two camera position rows and the cascade translation row, which is baked relative to the camera position and has to move with it: the same three rows `eye_globals` moves for a split pass. A fourth row carries the eye's offset for the local lights (see "The local lights, from each eye"). `stereo::single_pass::eye_camera` rewrites the buffer at every [`RenderEngine::SetAllGlobalShaderProgramConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetAllGlobalShaderProgramConstants), from the rows the engine has just staged, which is the same cadence `cb13` follows on the vertex side:

- Outside the routed range, and in any frame without the collapse, both blocks hold the staged rows unchanged and the boundary lies past every pixel, so a rewritten shader reads exactly what it would read from `cb0`. The shadow, reflection, environment, and reflective-shadow passes all run in `PreDraw`, outside the range, and keep their own camera's rows. The range guard puts the buffer back to the staged rows when the range ends, for the draws before the next upload.
- Inside the routed range under the collapse, each block holds the staged rows moved to its eye, and the boundary is the first pixel of the right eye's half of whatever target is bound, derived from the viewport the collapse splits (`collapse_viewport_follows_target`), so it follows the reduced-resolution targets. A boundary change rewrites the buffer; the per-bind check costs two relaxed loads.
- The per-eye re-issues and the scissor-masked runs of a split pass land in one half, so they read that eye's block. For a split pass and the water, that block holds the values `eye_globals` stages into `cb0` for the run, so the rows are not applied twice.

Every camera read moves to the eye, including the ones that only feed a fade or a distance threshold. That is what a separately rendered eye computes, and the choices the design keeps on the centre are not made in a pixel shader: the LOD and tessellation selection happen on the CPU and in the hull shader. Two draws crossfading at one pixel both read the same eye, so their fades stay complementary. On the vertex side, the four camera-only reprojected families (`generaljc3`, `landmark`, `layered`, `layeredblend`) keep `cb0[4]` centred on purpose, since their position path adds it back; in all four it feeds only a squared camera-distance fade, so no centred view vector reaches their pixel shaders through an interpolant.

All 432 camera readers are rewritten and none is declined: 183 already declare `SV_Position` with its `.x`, 8 declare it without and have the declaration widened, and 241 gain one from the input signature. The rewrite would decline a shader with no `SV_Position` input, one that already declares slot 11, and one that indexes `cb0` dynamically from a base that could reach a camera row; the 113 that index `cb0` dynamically all start past the cascade translation row. The rewrite composes after the others in the fragment hook. The water's per-eye variants are still built from the engine's own bytecode, since its per-eye draws stage the rows into `cb0`, and the resolve probe in the capture harness accepts the rewritten resolve. It is a shader rewrite, so turning it on needs a shader reload; turning it off takes effect at once, because the buffer then holds the staged rows everywhere.

In run 250 (a new session, so the player's pose and the animated scene differ from run 249's), the midday cliff view's G-buffer albedo differed from the isolated eye by more than three codes at 218 and 67 pixels, against 5,895 and 4,933, and the lit colour at 13,680 and 8,501 pixels, against 23,557 and 23,322; the final image differed at 6.0% and 4.6% of pixels, against 7.0% and 9.3%. At dusk the albedo differences fell from 8,773 and 6,271 pixels to 19 and 45. The deferred resolve, which now reads the camera through the buffer in its masked runs, reconstructs world positions within a millimetre of the isolated eye's. What still differs on the near surfaces is the screen-space reflection buffer the resolve reads (see "Known gaps"), and at dusk also the local-light data in the resolve's `LightingFrameConsts`, which differed from the isolated eye's in run 249 as well. The far-terrain, sky, and shore-lake views stay within their run-to-run range; the shore lake's water stage varies between sessions by more than any change attributable here (from 13,400 to 19,400 differing pixels per eye across runs 247 to 249).

## The local lights, from each eye

The engine builds the local point and spot lights' frame constants relative to the render camera (engine [`lighting-shadow-pipeline.md`](../../engine/rendering/lighting-shadow-pipeline.md#43-the-lighting-frame-constants-are-camera-relative)): [`LightManager::SetupLightingData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##func-SetupLightingData) subtracts `RenderContext::m_RenderCameraPosition` from every light position and bakes it into every spot projection. Under the collapse that is the centred camera, once per dispatch, while every pixel's position is measured from its own eye (the deferred resolve's view ray in its per-eye runs, and the camera row the forward-lit families subtract, through "Each eye's camera position in the pixel shaders"). Each light vector was therefore off by the eye's offset from the centre. In the resolve captures, the point lights' rows under single-pass were exactly the midpoint of the two isolated eyes' rows; the lights a held device carries sit a few tens of centimetres from the camera, so a few centimetres of error moved their glow visibly on the player's body, a fixed patch at eye 0's lower left in the dusk and midday cliff views.

`single_pass.local_lights_per_eye` (**on**) measures them from each eye without adding a draw or touching the engine's buffer. It is part of the eye-camera rewrite: each eye's block of the slot-11 buffer gains a row holding the eye's world offset from the centred camera (zero outside an eye-split pass, so a rewritten shader reads what the engine built), and `rewrite::lights` corrects the light reads of every shader that binds `LightingFrameConsts`. A light position read, the addend of an `add` or `mad` whose other term is the negated pixel position, becomes a read of the position minus the offset; a projection's translation row becomes the row plus the projection's linear part applied to the offset. A read is corrected only when its pixel position is relative to the pixel's eye: a temp the shader last wrote as a position minus a camera row (checked per component along the program, and not rewritten later in an enclosing loop), or the resolve's depth times its view ray. That takes 141 shaders: the ten resolve permutations and the hair with both lights and the projection, and 130 forward-lit families with both lights. The two vegetation blend permutations measure from a position their vertex shader made relative to its camera, so their light data and pixel position already share one camera and are left alone. The rest of the buffer stays centred on purpose: the light-propagation-volume transforms are placed around the camera like the cascade fit, and their volume is built once.

The particle vertex and domain shaders also light from the buffer, per vertex; they are not rewritten, so particles keep the centred light vectors.

In run 257, measured against a variant with the switch off in the same schedule, the final image at the dusk cliff view differed from the isolated eyes at 30 and 184 pixels per eye, against 1,562 and 257 with it off (repeats of the default: 31 to 34 and 184 to 186), and at the night far-terrain view at 1 and 5 pixels, against 337 and 157. At the midday cliff view this pose's lit colour (`post_resolve`) differed at 2,855 and 2,068 pixels, against 3,749 and 2,393, though the final image moved less (1,933 and 2,481 against 2,073 and 2,577), since the device's light is faint beside the sun. The differences the switch removes have the light's colour: in run 256's midday cliff captures, the lit colour on the player's body differed by a mean of about 0.000, 0.011, and 0.015 in red, green, and blue, the 0.7 green-to-blue ratio of the device's lights, over 11,700 and 10,100 pixels per eye, which was most of that schedule's 8,459 and 9,476 differing final pixels. The rewrite adds no draw: the dusk view's census is the same 747 instance-doubled calls, 203 calls submitted per eye, and 2,749 submitted once.

## The screen-door pattern, from each eye

The alpha-tested and fading materials discard against a threshold hashed from the pixel position, `frac(dot(SV_Position.xy, k))` with a fixed vector (engine [`shaders.md`](../../engine/rendering/shaders.md#the-screen-door-hash)): the vegetation and `general_lod*` distance and LOD crossfades, the `generalmkiii*` alpha tests, `prop`, `buildingjc3`, and the vehicle paint's stipple. The volumetric fog dithers its march by the same hash. The vector is not a whole number of cycles per pixel, so the pattern does not repeat at any horizontal offset, the 320-pixel eye of the capture harness included: shifting `x` by the left eye's width moves every threshold by the fractional part of `k.x` times that width (about 0.74 of the threshold range at 320), so in the right half of the double-wide target a different set of pixels survives each fade than in an eye rendered on its own, while the left half, whose pixel positions are an eye-sized target's, matches. In run 257's midday cliff view the band of coverage differences at the vegetation fade distance gave 353 pixels of depth differences in the right eye against 31 in the left.

`single_pass.screen_door_per_eye` (**on**) rewrites every `dp2` of the `SV_Position` input's `.xy` with an immediate vector (`dxbc_stereo::localize_screen_door`) to read an eye-local position instead: `x` minus the eye boundary where the pixel lies at or past it, `y` unchanged. The boundary is the one the eye-camera rewrite reads from its buffer at slot 11, so `stereo::single_pass::eye_camera` now publishes it in every eye-split pass, with both eyes' blocks holding the staged rows when the camera rows are not moved per eye. Outside an eye-split pass the boundary lies past every pixel, so the shadow, reflection, and other `PreDraw` passes keep the engine's pattern; an unbound buffer reads as a boundary of zero and leaves the position unchanged too. The rewrite composes after the eye-camera rewrite and reads the buffer that one declared; 101 pixel shaders take it, 6 of them over the eye-camera rewrite. The sun-shadow filter's rotation, the other hash of the pixel position in the game's shaders, has its vector zeroed by the shadow PCF patch before this rewrite runs, and a zero vector is left alone. The ambient-occlusion generation pass makes its position eye-local in its own rewrite first. The tree impostors build the same hash from their own projected position over the screen size rather than from `SV_Position`, so they are not covered.

In run 258, against a variant with the switch off in the same schedule, the midday cliff view's right eye had 60 to 77 pixels of depth differences from the isolated eye against 407 to 414 with it off, and its final image differed at a median 1,833 pixels against 2,660 (the default's repeat: 1,856); the left eye was unchanged at about 110 depth pixels and a median 1,585 final pixels against 1,703. At the dusk cliff view the right eye's final image differed at 47 pixels against 248 (repeat 53), and at the night far-terrain view at 1 against 6. Run 259, on the final tree, gave 19 against 208 at dusk and 1 against 5 at night, the left eyes unchanged by the switch. The rewrite adds no draw.

## The motion vectors, in each eye's screen space

The velocity pass encodes, per pixel, the current screen UV less the previous frame's, from a current and a previous clip position its vertex shader hands down (engine [`shaders.md`](../../engine/rendering/shaders.md#the-velocity-passs-motion-vectors)). In an eye-sized render the current position's NDC is the pixel's own, and the previous one comes through the render camera's previous view-projection, which for an isolated eye or a multipass dispatch is that eye's own previous camera: in runs 276 and 277 the isolated eyes and multipass agreed to the code. Under the double-wide collapse both positions leave the eye's space:

- The previous position comes through the centred camera's previous view-projection over the double-wide target, whose horizontal row is the eye-sized one's halved, from the centred camera's position rather than the eye's. This holds for all three of its sources: the vertex `GlobalConstants` rows the model families read, the camera-relative ones the hardware-instanced buildings read, and the matrices the vegetation and the other blocks bake into their own constant buffers. Rescaling only the global rows (an earlier trial) therefore left most of the buffer unchanged: the meadow's vectors are almost all the vegetation's, whose previous matrix is baked on the CPU.
- The current position is in the eye's half only where the vertex shader computes it from the remap's per-eye rows. The reprojected families and the vegetation drawn once move `SV_Position` alone, and leave the copy the pixel shader reads at the centred camera's.

So every eye's horizontal motion carried a ramp of about half a pixel per pixel from its centre, clamping at the buffer's limit over the outer parts of each eye, plus the parallax between the eye and the centred camera.

The collapse rewrites the twelve velocity pixel shaders (`dxbc_stereo::localize_velocity`). Right after the encoding's divide, the current NDC is rebuilt from `SV_Position`: `x` from the left edge of the eye whose half the pixel lies in (against the eye boundary of the eye-camera buffer at slot 11) over one eye's width, `y` from the target's top edge over its height. The previous NDC is rebuilt from the interpolated previous clip position taken through a per-eye matrix. An eye rendered on its own reprojects through its own previous camera position and orientation and the render camera's previous projection, which the engine builds for an eye-sized target. Under the collapse that projection is the double-wide one, whose horizontal row is the eye-sized one's halved, and the previous view is the centred camera's. The matrix is `S P_prev E⁻¹ P_prev⁻¹`: `P_prev` the render camera's previous projection, `S` the doubling of the horizontal clip coordinate, and `E` the eye's pose in the camera's frame, recovered from the eye's `M_eye = P_eye E⁻¹ P⁻¹` (the map from the camera's current clip space to the eye's that the `cb13` reprojection reads) as `P_eye⁻¹ M_eye P`. The eyes are rigid with respect to the camera, so the previous frame's eye cameras relate to the previous render camera as this frame's do to the current one. The rows live after the boundary in the same buffer, and `stereo::single_pass::eye_camera` writes them in the eye-split passes from the viewport the collapse splits, the `M_eye` the `cb13` upload has just computed, and the render camera's projections. The shader blends the rebuilt positions in by a weight in those rows, one in an eye-split pass, so outside one, or with the buffer unbound (all zeros) it subtracts exactly what the engine's shader does; the previous position is blended in clip space, before its divide, so zero rows cannot produce a NaN. Building the current position from the pixel serves every family, whatever its vertex shader did to `SV_Position`, and it is what an eye-sized render's interpolated copy amounts to, since that copy is `SV_Position`'s `x`, `y`, and `w`. The rewrite composes after the eye-camera and screen-door rewrites and reads the buffer they declared; four of the twelve shaders (the bark, foliage, stippled, and alpha-tested model ones) take a screen-door rewrite first. Nothing is drawn differently.

Run 276 tried a first version that only doubled the previous position's horizontal clip coordinate. At the meadow with the camera rotating, it removed the ramp (0.515 eye pixels per pixel before the rewrite, 0.001 after) and the clamping (44.5% of the velocity-pass pixels clamped in each eye before the rewrite, none after), but 82% of the pixels still differed from the isolated eye by more than a code, by up to 7 codes with opposite signs in the two eyes. That residual fell off as one over depth, matching the parallax of the eyes' 31 mm offset from the centred camera to within a few percent in every depth band from 3 to 100 m: the isolated eyes reproject through their own previous camera, not the centred one. The matrix replaced the doubling.

The velocity stage against the isolated eyes in run 287 (the second schedule after each teleport; per eye, the velocity-pass pixels differing by more than a code, of those either side wrote, with the rewrite on, then off in the same schedule): the meadow with the camera rotating 1 and 0 of about 35,600, against 35,319 and 35,530 off; the meadow translating 0 and 4 of about 37,100, against 37,009 and 36,896; the town rotating 10 and 6 of about 690, against 691 and 683; the town translating 34 and 34 of about 620, against 631 and 612. The default's repeat matched the default to the pixel in every scene. Every pixel left over is one only one side wrote (a velocity-pass draw covering a pixel in one render and not the other); among the pixels both wrote, none differs by more than a code in any scene, against all but a few hundred without the rewrite, whose ramp measured 0.51 eye pixels per pixel and clamped on 41% to 67% of them. Bark and the other families fare alike (at the meadow, 0 of about 5,600 bark pixels in each eye, against 5,549 and 5,540 off). The velocity pass drew only the vegetation and a few hardware-instanced `GeneralMkIII` draws in these scenes, so those are the families measured; the static models draw no velocity. Multipass matched the isolated eyes exactly throughout.

With the motion blur and its reconstruction running (the `motion_blur = "enabled"` override of the time, since removed: the motion blur no longer runs in VR), the final image moved little: at the town rotating, 1,935 and 788 pixels per eye differed from the isolated eyes with the switch on, 2,023 and 968 off, and 1,887 and 773 for the repeat; at the meadow rotating, 1,082 and 1,451 on, 1,232 and 1,434 off, and 1,188 and 1,478 for the repeat, within the repeat's range. Running the blur raised the meadow's differences from 975 and 980 to about 1,100 to 1,450 with the switch on, so the blur adds differences the velocity buffer does not account for: its camera-motion reconstruction reprojects depth through the render context's previous view-projection, which under the collapse is the double-wide centred one (see "Known gaps"). FSR is compared under "The shipped settings under single-pass": its history is reset on every configuration, so every mode accumulates it from its own warm-up.

The static regression scenes did not move (final image, pixels per eye differing from the isolated eyes, second schedule): the town 476 and 649 (run 287), the tunnel 9 and 5, the shore lake 428 and 557, and the dusk cliff 8 and 4 (run 288, which carries the deferred resolve's identity hashes for the eye-camera buffer's new length).

## The ambient-occlusion volumes, per eye

An ambient-occlusion volume ([`RenderBlockAOBox`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockAOBox), a box or a sphere drawn in [`RP_AO_VOLUMES`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_AO_VOLUMES)) darkens the scene inside it. Its `aobox` vertex shader projects the corners through the full view-projection (`cb0[0..3]`, which the remap does not touch) and hands the pixel shader a projective screen UV derived from that clip position; the pixel shader (`aobox` or `aosphere`) samples the scene depth and the reflective water planes' depth at that UV and maps each through `ScreenToLocal`, a matrix [`RenderBlockAOBox::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockAOBox##func-Draw) builds from the render context's camera and the box's transform, into the box's unit space, where the occlusion falls off from the centre. The result is multiplied into the target the SSAO composite then reads (`AOVolumeSrc`), so it reaches the image only through the SSAO block: leaving that block out hides it too.

Under the collapse the unpatched vertex shader was re-issued into each eye's half with the centred camera's transform, so its UV was the eye-local position of the pixel, while the depth texture spans both eyes: each eye's left half sampled the depth of the left eye stretched across a whole eye, and its right half the right eye's. The matrix was the centred camera's too. In the tunnel at midday that was the sharp vertical edge at the centre column of each eye and the straight occlusion wedges, and in the capital the copy of a tree trunk and its planter half an eye to the left of the real one (the depth at twice the pixel's eye-local position). Leaving out `RP_AO_VOLUMES` darkened the single-pass capture while it brightened the isolated eyes; that the volume target is filled only by that pass, and the double-wide copy the collapse re-creates holds no neutral value without it, is the likely reading and is unverified.

`single_pass.ao_volumes_per_eye` (**on**) draws the volumes once for both eyes. The `aobox` vertex shader takes the reprojection (by name), so its indexed draw is instance-doubled and each eye's half rasterizes the volume from that eye. `dxbc_stereo::localize_ambient_occlusion_volume` rewrites both pixel shaders to work from `SV_Position` alone: the eye is the half the pixel lies in, both depths are sampled at the pixel's double-wide UV, and every product of a depth with the matrix's depth row becomes the product of the whole matrix with `P_eye · (x, y, depth, 1)`, divided through by its `w`. `stereo::single_pass::ao_volumes` builds `P_eye = T⁻¹ · M_eye⁻¹ · T · S_eye` per eye (with `T` the UV-to-NDC flip, `M_eye` the reprojection, and `S_eye` the eye's pixel to the UV an eye-sized render reads there, scaled by [`m_UVScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_UVScale)), so the centred matrix applied to it is the eye's own matrix applied to the eye's own UV, and binds the rows at pixel slot 13 for the pass's draw walk. The UV products the shader shares between both depths stay in place, unused.

In run 264, against a variant with the switch off in the same schedule (`tools/rendering/experiments/coverage_*_fixes.toml`), the tunnel at midday differed from the isolated eyes at 8 and 13 pixels per eye against 42,517 and 41,613, with a same-schedule floor of 0; the capital at 3,239 and 938 against 11,402 and 12,256, though the isolated eyes' own repeats there differed at about 5,500, so that scene is not comparable pixel for pixel. The town and the farm road were within their floors either way, and the shore lake and the dusk cliff view barely moved (1,523 and 1,154 against 1,505 and 1,146; 62 and 57 against 173 and 84). The pass's draws are halved: in the tunnel's draw census its 45 calls are one instance-doubled submission each, where the per-eye re-issue made two. What the SSAO block leaves in daylight is then within the floor: the generation, blur, and composite rewrites were not the source.

## The road surfaces' clip origin, per eye

The road surfaces (`layeredroad`) and 31 `generalmkiii*roadbias` model permutations subtract the camera position from the world position before the translation-free view-projection, like the rest of the remapped scene, but take that position from `cb0[9]`, not `cb0[4]`. Row 9 is the render camera's position, staged by [`RenderEngine::SetGlobalShaderConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetGlobalShaderConstants), and the remap gave each eye its own view-projection and `cb0[4]` but left row 9 at the centred camera's position. Each eye therefore projected the road from half the eye distance away from where it is: the depth of a flat surface barely moves under a shift within its own plane, but its texturing slides by that much, which on the roads' high-frequency layers is a dense speckle in the G-buffer normals, albedo, and specular across the whole road, in both eyes, at matching depth.

`single_pass.clip_origin_per_eye` (**on**) has the remap (`dxbc_stereo::patch_vertex_shader_with` with `ClipOrigin::PerEye`) read `cb0[9]` from each eye's copy of the row in `cb13`, in the shaders that read the view-projection rows without `cb0[4]`; in the others `cb0[9]` is a distance fade and is left alone. The copies sit after the reprojection block, five rows apart so the remap's row base reaches each (`dxbc_stereo::CLIP_ORIGIN_ROW_BASE`), and `cb13` grows to hold them. Outside the routed range they hold the staged row itself, so the passes that are not eye-split keep the engine's value. No draw is added.

In run 264, against a variant with the switch off in the same schedule, the town at midday differed from the isolated eyes at 906 and 1,043 pixels per eye against 27,464 and 20,954 (same-schedule floor 446 and 389), the farm road at 3,008 and 2,382 against 18,916 and 18,054 (floor 1,283 and 1,286, the isolated eyes' own repeats 1,369 and 1,045), and the tunnel's road at 8 and 13 against 3,190 and 3,157. The shore lake and the dusk cliff view were unchanged by it.

## The planar reflection camera, at one eye's size

The WaveWorks water's planar reflection is rendered in `PreDraw` from the water patch manager's reflection camera, which [`WaterPatchManager::UpdateReflectionCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/water_patch_manager::WaterPatchManager##func-UpdateReflectionCamera) sizes every frame from the active camera's dimensions and the camera manager's aspect ratio. Under the double-wide collapse both are the double-wide target's, so the reflection texture held twice an eye's horizontal field: the shore lake's reflection capture, at the lake draw, showed the same sky squeezed to half its width, best fitted by a horizontal scale of 2 about the frame's centre, and differed from an isolated eye's in 34% of its bytes, while the two isolated eyes' reflections were byte-identical (the reflection camera mirrors the head camera, not an eye). The eye-local lookup the water re-issue gives each eye then read that frame as one eye's, minified horizontally by two about its centre; without it, each eye read half the frame, offset by half a frame. Either way the reflection under the water's normal disturbance landed on different texels than in an isolated render, which over the far, grazing part of the lake, where the disturbance and the sun's glitter are strongest, was the fine speckle the water step added. The bound wave cascades were byte-identical to an isolated render's, and disabling the sun shadows in every mode left the speckle in place (about 25,000 differing water pixels per eye), which ruled out the WaveWorks simulation and the shadow cascade fit.

The collapse halves the reflection camera's width and aspect ratio right after the engine sets them, under the double-wide collapse only; the engine's caller rebuilds the projection from them straight after, and sets them again on the next frame.

In run 259, at the shore lake and against a variant without the halving in the same schedule, the water step added 2,037 and 846 differing pixels per eye against 13,448 and 12,300, and the final image differed from the isolated eyes at a median 1,607 and 1,021 pixels against 6,326 and 5,756 (the default's repeat: 1,598 and 996). The shore lake's reflection texture at the lake draw then differed from an isolated eye's in 0.03% of its bytes. At the dusk cliff view the halving made no difference (44 and 31 pixels against 45 and 31). It adds no draw.

The first version corrected the engine's values after its call instead of replacing them, and in run 258 the reflection kept the double-wide field for the first two samples after every reconfiguration (about ten seconds) before the correction took. The reflection camera is live, and the draw thread can render the previous frame's reflection passes from it while the game thread updates it, so a correction leaves a window in which the camera holds the double-wide parameters with its projection marked dirty; setting the final values directly removed the effect from every variant in run 259. One sample was still affected: the first capture after the harness switched from multipass into single-pass showed the double-wide reflection in two of three such switches (runs 258 and 259), and in both shore-lake runs of run 273 (the first single-pass sample differing at 4,614 and 4,360 pixels per eye, and 2,922 and 2,956, against about 500 to 800 after it). The reflection camera's update runs as a job on the workers of the engine's job pool (the log shows it on five of them), and the engine runs its draw dispatch on a worker of the same pool. The draw thread pins the single-pass flags for its dispatch, and the pin outlived the dispatch on its worker, so the update read the flags of whatever dispatch that worker last drew. Run 273 logged the reflection camera's parameters every time they changed: in steady single-pass they alternated frame by frame between one eye's size and the double-wide target's, on the frames the update landed on a worker last used for a multipass dispatch. Steady single-pass came out right only because the reflection was rendered from the eye-sized camera on enough frames. The pin now ends with the dispatch's last seam (`single_pass::end_dispatch`). In run 274 the parameters changed only at the mode switches (21 changes in the session, against one or two per frame), and no sample stood out: the first single-pass sample of the two shore-lake runs fell within 539 to 564 and 621 to 622 pixels per eye, and 471 to 484 and 460, of the default's other samples.

## The tree impostors' screen door, per eye

The tree impostors fade in over a distance band by a screen-door dissolve: a card's pixel is discarded where the fade lies below a threshold hashed from the pixel's position, the same hash and vector the other fading materials use (see "The screen-door pattern, from each eye"). The impostor permutations build the hash's input not from `SV_Position` but from the clip position their vertex shader passes down, divided by `w` and by twice the inverse target size in the vertex globals, which in an eye-sized render is the pixel's offset from the target's centre. Under the double-wide collapse that clip position is the centred camera's, ahead of the reprojection, and the target is both eyes wide, so the pattern came out twice as wide and centred between the eyes, and a card in the fade band kept a different set of pixels in each eye than in an isolated render. At the meadow the cards in the band differed as whole trees in both eyes.

The collapse rewrites the 16 pixel shaders that build the hash that way (`dxbc_stereo::localize_projected_screen_door`) to read the offset an eye-sized render computes from the pixel position: `x` made eye-local against the boundary `stereo::single_pass::eye_camera` publishes, less half the eye's width, and half the target's height less `y`. Outside an eye-split pass, where the boundary lies past every pixel, the rewritten shader hashes the engine's own offset, so the shadow and reflection passes are unchanged.

At the meadow with a moving camera (`coverage_forest_midday.toml`, the second schedule after the teleport), the final image differed from the isolated eyes at a median 757 and 1,286 pixels per eye against 1,439 and 1,876 without the rewrite in the same schedule (the default's repeat: 774 and 1,210) in run 273, and at 1,284 and 1,387 against 1,877 and 2,046 (repeat: 1,196 and 1,352) in run 274. The whole trees are gone from the difference; what remains on the trees is a speckle, and the right eye's extra pixels in run 273 lie on the ground (see "What the first matrix found", the terrain under a moving camera).

## The static light sprites

At dusk and night the distant town and street lights along the horizon came out far too many, too bright, and in the wrong places under the collapse: at the night far-terrain view, eye 0's main colour gained 460 pixels with a summed brightness of 73 between the water and the cloud composite, against 88 pixels and 6 in an isolated render of that eye, and eye 1 gained 30 pixels in rows where the isolated eye had none. The draw census (see "Naming the draws in a pass range") showed what draws there: [`RenderBlockTypeLights`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeLights), the static light sprites, in [`RP_LENSFLARE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_LENSFLARE), with the `lightsource` program pair and one non-indexed `Draw` per block, each submitted once. Nothing else in the passes from the water to the cloud composite draws besides the two cloud blocks.

The sprite vertex shader projects each light's world position through the full view-projection in the global vertex constants (not one of the remap's per-eye rows) and expands the quad around it in clip space by a pixel size times the clip `w`. Under the collapse three things were wrong with that:

- A non-indexed draw outside the geometry passes is treated as a fullscreen pass and drawn with the whole double-wide viewport, so the centred camera's view was stretched over both eyes: each light landed at twice its distance from the target's centre, and eye 1 showed the right half of the centre view.
- The sprites are occluded by the hardware depth test alone (the `lightsource` pixel shader samples no depth), so each stretched sprite was tested against the depth of a pixel that is not the one it represents. Lights the terrain hides came out in front of it, which is where most of the extra brightness came from.
- The inverse screen width that turns the pixel size into clip units is one over the back buffer's width, which is both eyes' width.

The collapse draws them once for both eyes. The `lightsource` and `lightsource_fakelight` vertex shaders take the reprojection (their clip position is a scene view-projection times a world position, and `M_eye` is linear, so the clip-space offset moves with the sprite), and `draw_detour` promotes a non-indexed draw with a patched vertex shader in the two passes the light type draws in within the routed range ([`RP_DEFERRED_LIGHTS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_DEFERRED_LIGHTS), where `Setup` selects the fake-light programs, and `RP_LENSFLARE`) to one two-instance `DrawInstanced` with the viewport split, as the indexed path promotes its draws. The promotion is a pass allowlist, not every patched non-indexed draw: the first version promoted all of them outside the geometry passes, which also caught the light-propagation-volume initialisation (`lpvinit`) and the moon's `open` model, and the split moved the midday sky capture's lit colour (`post_resolve`) away from the isolated eyes at about 5,800 and 6,000 pixels, against 655 and 1,054 with the allowlist. The sprites then rasterise in each eye's half from that eye's projection and are depth-tested against that eye's depth. A detour on [`RenderBlockTypeLights::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeLights##func-Setup) rewrites the sprite constants' inverse screen width to one over the width of the eye half the collapse splits the bound target into (`stereo::single_pass::static_lights`), so a sprite is as wide as in an eye-sized render. The pixel shader's camera reads are already per eye (see "Each eye's camera position in the pixel shaders"). The sprites' distance fade and size curve read the centred camera row, as the scene's other LOD decisions do.

The draw count is unchanged: the same 38 calls at the dusk cliff view, each now one submission carrying both eyes, where a per-eye re-issue would have been 76. Outside the routed range (the planar reflection's distant-light pass, which reuses the program) `M_eye` is the identity and the draw is not promoted, so the reflection is unchanged.

In run 253 (a new session, so the pose and the scene's animation differ from run 250's), the same-run comparison at the dusk cliff view had the eyes add 831 and 829 pixels between the water and the cloud composite where the isolated eyes added 830 and 829, all but one in the same places, with a summed absolute difference of 0.1 and 0.7 against totals of 36; the final image differed from the isolated eyes at 30 and 191 pixels (0.03% and 0.17%), against 4,051 and 3,325 (3.5% and 2.9%) in run 250. At the night far-terrain view the eyes added exactly the isolated eyes' 54 pixels each, the final image differed at 40 and 12 pixels, against 576 and 156 (0.50% and 0.14%) in run 250, and the comparison passes in all four modes. The midday sky, midday cliff, and shore-lake captures show no stage clearly worse than in run 250; the midday sky's water step adds about 660 differing pixels per eye against about 250, all in the patch of lake at the foot of the frame where run 250's came from, which this pose shows more of.

## The moon and the water reflection planes, once for both eyes

Two more draws in the routed range were drawn once across the whole double-wide target. Both are non-indexed, outside the geometry passes, so the draw path treated them as fullscreen triangles and bound the full viewport.

The moon is one quad in [`RP_MOON`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MOON), drawn by the `open` vertex shader, which already takes the reprojection. With one instance and the full viewport it took eye 0's transform over both halves: missing from eye 0 and twice as wide in eye 1. The collapse promotes it to one two-instance `DrawInstanced` with the viewport split, as the static light sprites are promoted. The `open` shader can pin a sky body's clip depth to the far plane, and `M_eye`'s depth row mixes the other clip components back into that depth, which would push the moon off the plane; for the promoted draw both `cb13` eye slots carry `M_eye` with its depth row replaced by the identity row, the correction the sun's per-eye re-drive takes (`stereo::single_pass::sun`). It needs `single_pass.reproject` for the `open` rewrite.

The water reflection planes' proxy ([`RenderBlockWaterReflectionPlane::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockWaterReflectionPlane##func-Draw), in [`RP_REFLECTIVE_WATER_PLANES`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_REFLECTIVE_WATER_PLANES)) is a world-space quad whose depth the screen-space reflections trace water against. Its `watermask` vertex shader is a bare multiply-add chain over the offset world-view-projection the block stages at vertex `cb1`, and its `waterplane` pixel shader writes the quad's depth where it lies in front of the downsampled scene depth, loaded at the pixel's own position. Drawn once from the centred camera across the double-wide target, each eye's water depth was half of a stretched quad, tested against the other eye's depth at the seam. The collapse puts `watermask` on the reprojection allowlist and promotes the proxy draw to two instances routed to the eye halves; the pixel shader's load then reads each eye's own depth. The water mask volumes (`CRenderBlockWaterMask`, not bound in the defs) share the vertex shader and stage the same matrix, and their indexed draws are promoted like any patched draw. The reprojection needs a shader reload.

In run 265, against variants without each fix in the same schedule: at night in the capital the moon was the only difference from the isolated eyes, at 179 pixels per eye without the moon's promotion and 0 with it. At the midday shore lake the water planes' promotion took the final image from 1,569 and 939 differing pixels per eye to 796 and 759 (the default's repeat: 744 and 710), and in the town from 761 and 830 to 508 and 596 against the repeat (the default's own samples, the first after the teleport there, ranged from 494 to 1,548). At the dusk cliff view it changed nothing (12 and 19 against 13 and 19).

Neither adds a draw: each is still one submission, now carrying both eyes.

The light-propagation-volume initialisation (`lpvinit`) is the other patched shader the census records on a non-indexed draw in the routed range, and it is correct drawn once. Its vertex shader turns each texel of the reflective shadow map into a cell of the light-propagation volume, and a geometry shader routes each point to its volume slice, so it writes a volume texture, not the scene. The remap claims it on a `cb0[4]` reference that reads only the row's `w`, which `cb13` carries unchanged in both eye slots, and the viewport index its rewritten vertex shader writes is dropped at the geometry shader. Drawn with the volume target's own viewport (`single_pass.collapse_viewport_follows_target`), the one draw injects one volume placed by the reflective shadow camera, which both eyes then sample, the shared centre choice the design keeps.

The draw census at six coverage scenes (the town at midday and in rain, the capital, the hangar, the tunnel, and the dusk cliff) lists, as of run 264, these draws treated as `once` inside the routed range outside a render block's per-eye re-issue, besides the moon and the water planes: the fullscreen passes (the deferred lighting and its light assignment, the sky's atmospheric scattering, the screen-space reflection trace, blur, and composite, the low-resolution cloud and particle composites, the fog volumes, the subsurface skin, the scene capture, and the depth downsample), which run once over the whole target by design and take their eye inside the draw where they need one (see "Four ways content goes wrong under the collapse" and the per-eye fullscreen switches under "Configuration"); `lpvinit`, above; and two non-indexed draws with shaders the identity census cannot name, outside every tracked pass after the road passes, which the census files under the road block types because those were the last it saw. What those two write is unexamined.

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

The collapse additionally assigns each eye's lights from that
eye's position, by writing the eye's world offset into the translation row of the uploaded `cb2` view
matrix — algebraically identical to biasing the light positions, and it needs no engine write because
that row is exactly where the engine's own zeroed translation goes. The per-eye display canting is not
applied, only the positional offset.

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

## Asymmetric and canted eyes

Every comparison before run 291 ran on the simulated headset's symmetric, parallel eyes, where an eye differs from the centred render camera only by its offset. A real headset's eyes have asymmetric frusta, mirrored between the two, and some headsets cant each display outward, so an eye also differs from the centred camera in its projection and its orientation. Under the collapse the render camera is centred, carries eye 0's projection, and has no cant, so anything that took one of those from the render camera rather than from the eye was right only for symmetric, parallel eyes. Run 291 put the harness's headset-like rigs (see "A headset-like rig" under "Capturing varied scenes") on the town at midday and found the final image differing from the isolated eyes at 73,885 and 73,305 pixels per eye under the Index-like rig (5° of cant), 1,390 and 47,278 under the Quest-like rig (asymmetric, parallel: only the right eye broke), and 79,583 and 59,199 under the stress rig, against 2,753 and 778 under the default one. The depth and the G-buffer albedo matched the isolated eyes to within a few thousand pixels in every rig, so the geometry was right and the defects sat in the passes that read the camera.

**The clouds.** The cloud layers' vertex shaders take their clip position from the full view-projection (`cb0[0..3]`), which the remap does not make per eye; `clouds` and `cirrusclouds` are claimed by it on their camera-position read alone (see the triage in [single-pass-render-blocks.md](single-pass-render-blocks.md)), and `softclouds` is unclaimed. Both halves therefore drew the clouds through the centred camera: invisible while the eyes were symmetric and parallel and the cloud distance makes the offset vanish, but with eye 0's projection in eye 1 under asymmetric frusta, and without the cant in both eyes under canted ones. This was the whole of the Quest-like rig's right-eye difference (38,573 pixels with `single_pass.clouds_per_eye` off in run 292, 2,799 on). `single_pass.clouds_per_eye` puts the four cloud vertex shaders on the reprojection, by exact name since the cloud name space also holds the cloud-shadow pass. Their clip depth is never pinned to a plane, so `M_eye`'s depth row is harmless to them.

**The reconstruction basis of a split pass.** A fullscreen pass split per eye is handed the eye's clip-to-view inverse with the full-target-to-eye remap folded in, and then multiplies by the render camera's transform, which under the collapse is the centred one. For a canted eye that turned every reconstructed position by the cant about the camera, so the sun shadow sampled over them (the deferred resolve, the atmospheric scattering, the subsurface skin, the depth of field) landed in the wrong place. The substituted inverse now also carries the eye's turn from the centred camera (`reconstruction::eye_half_inverse`), which is the identity on parallel panels. The screen-space decals rebuild their basis from the centred view the same way and now use the eye's view (`ss_decal::eye_view`), and the clustered light assignment's per-eye view, which moved the centred view to the eye, now also turns it to the eye (`clustered_lighting::split::eye_assignment_view`). The screen-space ambient occlusion and reflection rows, the fog volume, the water, and FSR's per-eye matrices were already built from the eye's resolved pose.

**The motion vectors' previous view.** The velocity rewrite maps the centred camera's previous clip position into an eye's through the eye's pose relative to the camera. The vertex globals captured in run 291 show that an eye rendered on its own takes its previous view from the centred camera's orientation (the engine's previous view-projection carries none of the cant the camera hook applies after the fact) and from the eye's own position for the families that bake their previous transform relative to the current camera position. The map now carries only the eye's offset (`eye_camera::eye_offset_view`). Under the Index-like rig with the camera rotating, the velocity stage differed at 502 and 460 of the velocity-pass pixels in run 291 and at 59 and 34 in run 292.

**The cull and shadow-fit frusta.** Both are a centred symmetric frustum bounding the two eyes, built from each eye's half-angles. A canted eye reaches past its own half-angles on its outer side in the head frame (the Index-like left eye's left edge is 59° out, not 54°), so the exact shadow fit, which has no padding, stopped short of the outer edges. The bound is now taken in the head frame, over each eye's frustum corners turned by its orientation (`frame::head_frame_tangents`), in every mode.

**Unpatched draws re-issued per eye.** A draw whose vertex shader the collapse does not rewrite is re-issued once into each eye's half, still reading the engine's vertex globals, which hold the centred camera. Binding each eye's copy of the block around those re-issues (the full and offset view-projections, the camera position and axis, and the render camera's position, as an eye rendered on its own stages them) was tried and has been removed, so those draws read the centred camera. No scene in run 292 drew a family the binding corrects, and at night in the capital it moved re-issued draws away from the isolated eyes: 13 and 20 pixels per eye differed with it on, none without it. A shader that takes its clip from a transform its block baked from the centred camera while reading the camera rows for something else breaks when only the rows move, the same mismatch the vegetation's `cb0[4]` read shows (see [single-pass-render-blocks.md](single-pass-render-blocks.md)), so the binding would need a per-family allowlist; the families it would serve (`halo`, `beam`, the remaining sky shaders) did not draw in any scene measured.

**Multipass under asymmetric frusta.** With the shipped settings put back, multipass itself differed from the isolated eyes under the rigs in run 291 (37,034 and 3,364 pixels per eye under the Quest-like rig at the still town, against 1,356 and 1,843 under the default one): FSR's stereo correction took the centred camera's view-projection from one snapshot that each dispatch overwrote, so eye 0 was corrected through eye 1's projection. The snapshot is now kept per eye (`stereo::VpHistory`).

**After the fixes** (run 292, the second schedule after each teleport; pixels per eye differing from the isolated eye, final image by more than three codes with the repeat's in brackets, then the depth by a relative 1e-4, all of 115,200):

| Scene | Default rig | Index-like | Quest-like | Stress |
|---|---|---|---|---|
| Town | 1,228 / 1,253 (1,200 / 1,228); depth 12 / 1 | 3,186 / 781 (3,221 / 797); 1,786 / 0 | 1,532 / 2,799 (1,518 / 2,793); 53 / 1,142 | 4,311 / 4,873 (4,376 / 4,895); 49,141 / 1,165 |
| Town, rotating | 1,627 / 812 (1,580 / 804); 1,611 / 2 | 3,028 / 2,522 (3,047 / 2,521); 2,118 / 2,610 | 1,234 / 1,473 (1,097 / 1,452); 1 / 20 | 2,466 / 2,818 (2,423 / 2,795); 2,230 / 190 |
| Town, translating | 1,012 / 1,767 (1,017 / 1,752); 1 / 1,519 | 3,112 / 2,278 (3,174 / 2,297); 2,356 / 2,856 | 1,634 / 2,844 (1,648 / 2,824); 506 / 1,422 | 4,944 / 5,312 (5,034 / 5,290); 1,922 / 1,332 |
| Tunnel | 36 / 36 (36 / 36); 376 / 9 | 17 / 17 (17 / 17); 282 / 239 | 26 / 47 (26 / 47); 248 / 17 | 21 / 20 (22 / 21); 435 / 116 |
| Shore lake | 551 / 495 (551 / 497); 903 / 876 | 9,398 / 9,239 (9,398 / 9,236); 6,913 / 7,832 | 647 / 1,425 (648 / 1,424); 1,119 / 1,235 | 11,133 / 37,245 (11,133 / 37,241); 7,428 / 10,553 |
| Dusk cliff | 84 / 184 (49 / 152); 721 / 27,550 | 155 / 180 (155 / 182); 897 / 2,153 | 20 / 64 (22 / 66); 196 / 9,980 | 174 / 349 (196 / 362); 12,404 / 10,359 |
| Meadow | 1,359 / 1,700 (1,348 / 1,568); 2,777 / 3,492 | 5,048 / 5,874 (5,105 / 5,921); 6,341 / 7,346 | 867 / 1,241 (892 / 1,295); 2,310 / 2,995 | 5,446 / 9,109 (5,317 / 9,137); 6,635 / 9,612 |
| Capital at night | 72 / 86 (72 / 86); 2,865 / 2,928 | 14 / 26 (14 / 26); 1,522 / 1,463 | 13 / 21 (13 / 21); 2,071 / 2,216 | 2 / 96 (2 / 96); 1,695 / 1,912 |

With the shipped settings put back, the engine's metering gives every rendering one exposure from what it sees, and an isolated eye of an asymmetric rig sees different content from the pair, so the isolated eyes' exposures differed by up to 9% under the stress rig and multipass and single-pass both differed from them by tens of thousands of pixels for that reason alone. With the metering left pinned and FSR and the foveation put back, multipass matched the isolated eyes in every rig (at most 1,056 pixels per eye), and single-pass measured 3,339 and 575 (3,342 and 597 for the repeat) at the still town and 1,733 and 1,875 (1,735 and 1,882) rotating under the Index-like rig.

What remains under the canted rigs is geometry and surface choice that the collapse makes once from the centred camera while an isolated eye makes it from its own canted one: the depth and the albedo differ on the vegetation at the shore lake and the meadow (whole trees at mid distance, in both eyes), and on distant terrain at the outer edge of a canted eye. Under the Quest-like rig, whose eyes are parallel, the same scenes stay near the default rig's numbers, which is what ties the remainder to the cant rather than the asymmetry. Which choices these are (a billboard facing the camera's axis, a level of detail, a fade) is not yet attributed.

**At a real headset's eye size.** Run 293 took the comparison at 3584 by 3561 per eye, the size a Bigscreen Beyond 2e's runtime recommends (from a session on that headset), so the double-wide target was 7168 by 3561; nothing hit a texture-size or memory limit, but each schedule wrote about 6.5 GB even with `stages = "geometry"` and took five minutes, so only part of the matrix ran. Percentages are of the eye's 12,762,624 pixels, final image then normals:

| Scene | Default rig | Beyond-like rig | At 320 by 360 (default, run 292) |
|---|---|---|---|
| Town | 6.06% / 1.77%; normals 23.8% / 4.2% | 1.95% / 2.46%; 4.3% / 6.4% | 1.07% / 1.09%; 3.5% / 5.6% |
| Town, rotating | 0.65% / 4.77%; 0.15% / 21.8% | 1.61% / 2.01%; 3.3% / 4.1% | 1.41% / 0.70%; 12.9% / 4.3% |
| Town, shipped settings | 0.78% / 6.95%; 0.11% / 30.7% | 1.08% / 2.88%; 3.8% / 17.4% | 2.45% / 3.13% |
| Tunnel | 0.11% / 0.13%; 31.8% / 34.1% | not run | 0.03% / 0.03%; 28.2% / 22.4% |

The repeats matched the defaults in every case, so these are deterministic. Two differences are materially worse than at 320 by 360. At the town, one eye's ground (which eye changes from run to run) differs in the G-buffer normals over almost all of its surface, by up to tens of ten-bit codes rather than one, at matching depth, and that eye's final image differs at 5% to 7% where the same artefact cost about 1% to 2.4% at the small size; it is the terrain patch surface state described under "What the first matrix found" (the detail and morph state the patch system keeps from whichever camera last rebuilt it under the held clock), which at the full size resolves into visible shading. In the tunnel the G-buffer albedo differs in a speckle over the rock walls in both eyes (0.42% and 0.49% of the eye, against 3 and 5 pixels at the small size) at matching depth; the first stage that differs is the G-buffer, and the likely mechanism, unverified, is a screen-space choice the tessellated terrain makes once for the double-wide target (a tessellation factor measured in pixels of a target twice as wide) where an isolated eye makes it for its own width.

## The shipped settings under single-pass

The capture profile (`apply_profile` in `crates/jc3vr-capture/src/testing/visual/mod.rs`) pins or turns off several things a player runs with, so the comparisons above say nothing about them. Each can be put back in every mode with a visual override (see "Running the shipped settings" under "Capturing varied scenes").

| Setting | Shipped | Profile | What it controls | Under the collapse |
|---|---|---|---|---|
| `fsr.antialiasing` | FSR (sharpening 0.2) | the engine's SMAA, or the mod's TAA ([taa.md](../rendering/taa.md)) | FSR in place of the engine's anti-aliasing, post-tonemap | once per eye over its half with `single_pass.fsr_per_eye`; once over the double-wide target without |
| `fsr.stereo_mv_correction` | on | on (inert while FSR is off) | the parallax term FSR's velocity decode adds | per eye from each eye's own matrices with `single_pass.fsr_per_eye` |
| `foveation.enabled` | on | off | the peripheral stencil drop before the shading passes and the fill-in after | one draw each over the double-wide target, per-pixel eye with `single_pass.foveation_per_eye` |
| `exposure.force`, `post_fx.skip_histogram` | off, off | pinned, skipped | the engine's histogram metering and exposure adaptation | metered once per frame over both eyes; one exposure for both |
| `post_fx.skip_fade` | off | skipped | the screen fade's colour multiply | once over the double-wide target |
| `post_fx.skip_player_damage` | off | skipped | the player-damage pass | once over the double-wide target |
| `post_fx.skip_motion_blur`, `skip_motion_blur_recon`, `skip_dof` | off | motion blur skipped | the motion blur and the depth of field | never run in VR, whatever the switches say (see below) |
| `post_fx.dof_no_reproject` | on | on | the depth of field's underwater composite | unchanged by the profile |
| `vr.native_resolution`, `vr.resolution_scale` | on, 1.0 | on, left as configured | the per-eye render size | the double-wide target requires native resolution |
| `director.clean_view.scope` | the desktop overlay only | every overlay | the mod's own overlays | composited outside the eye targets the comparison reads |
| `director.paused`, `vr.freeze_mode` | running, off | held, full camera | the clock and the camera | what makes the four modes comparable at all |

**Foveation.** Run as for one eye, the mask and fill-in put one fovea on the seam, sized for the double-wide target, so each eye drops pixels at its own centre of view; see "Under single-pass stereo" in [`foveation.md`](foveation.md) for `single_pass.foveation_per_eye`. With the foveation alone put back at the town (run 289, static camera, final image per eye, pixels differing from the isolated eye by more than three codes), the fix took the difference from 32,661 and 57,617 to 2,647 and 1,943, against 2,649 and 1,894 for the repeat. The image FSR reads matches the isolated eyes at 399 and 649 pixels with the fix and 22,772 and 44,915 without, against 9 and 3 for multipass.

**FSR.** Run once over the double-wide target, FSR scales the motion vectors, which the velocity pass writes per eye, to the double-wide width, reprojects one history across the seam, and gives both eyes the correction of a centred camera whose eye and centre matrices agree, so it adds no parallax where an eye rendered on its own adds its eye's. `single_pass.fsr_per_eye` resolves each half through its own eye's context; see "Under single-pass stereo" in [`fsr.md`](../rendering/fsr.md). Measured with FSR alone put back (run 290): at the town with the camera still, 2,287 and 2,624 pixels per eye, against 1,751 and 1,994 for the repeat and 26,065 and 35,575 with the switch off; with the camera rotating, 1,834 and 771, against 1,872 and 725 for the repeat and 19,848 and 16,682 off. Run 289, which had the per-eye resolve without the per-eye correction, measured 32,580 and 32,140 at the still town: nearly all of the difference was the correction the isolated eyes apply and the resolve did not. With the correction left out in every mode (`fsr_mv_correction = "disabled"`), the still town measured 6,731 and 5,007 against 6,127 and 4,274 for the repeat (18,898 and 18,110 with the switch off), and the rotating town 2,271 and 1,454 against 2,223 and 1,446 (5,530 and 5,131 off).

FSR is not deterministic from capture to capture: at the shore lake its input matched between two isolated captures of the same eye to the pixel while its output differed at 3,800 to 4,700 pixels. Its noise floor is that self-repeat, not the profile's.

**Exposure.** Under the collapse the histogram is metered once per frame over both eyes, so both eyes get one exposure, as both eyes of multipass get eye 0's. The applied exposure each capture reports (`visual.exposure`) settled within 0.05% between all four modes after the warm-up at the town and the shore lake (0.0963 at the town in run 289). With the metering alone put back at the town, the final image differed at 2,588 and 1,552 pixels against 2,568 and 1,520 for the repeat. The first capture after a large change of scene had not converged in thirty warm-up frames: after the shore lake, the dusk cliff's first isolated capture applied 0.702 against 0.755 for every later one, and that one capture differed at about 45,000 pixels. The first schedule after a teleport is discarded for this reason too.

**The screen fade and the player damage.** Both run once over the double-wide target. The `playerdamageeffect` pixel shader reads the scene at the pixel's own screen UV and has no other screen-space input, so it is the same per pixel as in an eye-sized target, and the fade is a colour multiply. Neither draws in a harness scene (the screen is not fading and the player is undamaged), so neither is measured; whether `PlayerDamageEffect::Apply` draws anything other than that pass is unverified.

**The motion blur and the depth of field.** Neither runs while a VR session runs, whatever the `post_fx` switches say: both smear the view with the camera's motion and focus, which in the headset are the player's own head and gaze. The payload zeroes the camera contexts' motion and radial blur factors on the head-tracked path (`clear_blurs` in `hooks/camera/mod.rs`), and the post chain's hooks (`hooks/graphics_engine/post/post_effects.rs`) skip [`MotionBlurEffect::Apply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::MotionBlurEffect##func-Apply) and answer [`PostEffectsManager::IsBokehActive`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-IsBokehActive) with false, which drops the depth of field's own passes (the downscale-and-focus prepass, the bokeh blur, and the depth-of-field composite family) and leaves the plain blur, which the glare reads, and the plain composite. [`DepthOfFieldEffect::Apply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::DepthOfFieldEffect##func-Apply) itself always runs in VR, since its composite is also the HDR-to-LDR tonemap; `post_fx.skip_dof`, which skips it whole, is ignored in VR. The motion blur's `Apply` also carries the heat-haze distortion and, with the engine's SMAA T2x mode, a temporal composite; neither runs in VR either (FSR replaces the engine's anti-aliasing, and multipass already drops T2x to SMAA 1x). The debug UI locks the three switches while VR renders. Forcing them off took 0.10 to 0.17 ms of GPU time per frame at 3584 by 3561 per eye in run 302, the motion blur's pass, with no frame-time change outside the repeats' spread (see "The motion blur and the depth of field" in [frame-budget.md](../performance/frame-budget.md)). Outside VR they keep their bisection roles. The harness's former `motion_blur = "enabled"` visual override is removed, since it could no longer put the blur back.

**Non-native resolution.** The double-wide target requires `vr.native_resolution`, so single-pass never runs at the desktop size. A `vr.resolution_scale` below one shrinks each eye's half; the foveation and FSR size themselves from the live target, but no capture was taken at a scale other than one.

**Standing check.** `tools/rendering/experiments/shipped_town_static.toml` and `shipped_town_rotation.toml` run the town with every shipped setting above put back, with a repeat and each fix's switch off as variants. In run 290: still, 2,806 and 3,029 pixels per eye against 2,446 and 2,513 for the repeat (22,532 and 33,306 with `fsr_per_eye` off, 32,279 and 54,512 with `foveation_per_eye` off); rotating, 1,749 and 1,356 against 1,860 and 1,292 (16,798 and 15,415, and 29,654 and 52,944). The profile's own capture of the still town in the same session differed at 1,216 and 766. At the shore lake (second schedule), 4,848 and 4,449 against 4,907 and 4,600 for the repeat (15,188 and 18,237 with `fsr_per_eye` off, 37,334 and 62,430 with `foveation_per_eye` off), where the profile measured 643 and 579 and FSR's own self-repeat about 4,000; at the dusk cliff (second schedule), 2,564 and 1,637 against 2,622 and 1,694 (18,191 and 19,077, and 39,355 and 59,128), where the profile measured 30 and 59.

## Configuration

All under `stereo.single_pass`, except where a row says otherwise. The master switch
(`single_pass.enabled`) and the rest of the structural group are on by default, as are most of the
flags that depend on them, since they fix a defect the collapse introduces. Each row below is marked
with its actual default.

Every switch below the master switch is one row of the switch table in `crates/jc3vr-view/src/stereo/config/single_pass.rs`, which generates the configuration field and its default, the `SinglePassSwitch` the capture overrides and the shader build name it by, whether changing it needs a shader reload, its bit in the render path's per-frame snapshot, its debug UI row, and the manifest the Python capture protocol reads (`tools/rendering/protocol/single_pass_switches.json`, kept current by a test in the table's module, which rewrites it when run with `JC3VR_WRITE_MANIFEST=1`). Adding a switch is adding a row there.

### Structure

| Flag | Default | Effect |
|---|---|---|
| `single_pass` | On | Master switch. Substitutes the rewritten shaders, installs the routing, makes the eyes diverge (distinct per-eye `cb13`, eye-half viewports, instance doubling), collapses the per-eye double-draw to one `game.Draw` walk, and re-creates the scene targets at 2x per-eye width (which needs `vr.native_resolution`). Forced inert without the viewport-routing capability. |

### Shader rewrites (need a shader reload to apply)

| Flag | Default | Effect |
|---|---|---|
| `single_pass.reproject` | On | Reprojects the no-`cb0` scene families (characters, props, buildings, roads) instead of leaving them double-drawn. |
| `single_pass.terrain` | On | Rides the eye index through the terrain tessellation pipeline (VS → HS → DS); also gates the render-block re-issue that reprojects the GPU-indirect terrain-detail pass. |
| `single_pass.terrain_patch_per_eye` | On | On top of `single_pass.terrain`: the terrain patch system's tessellated and GPU-indirect basemesh pipelines, which cannot carry an eye index, take the pinned-eye reprojection, and their per-eye re-issues pin `cb13` to each eye (see "Pipelines that cannot carry the eye"). |
| `single_pass.pixel_camera_per_eye` | On | Every pixel shader that reads the camera position picks the rows of the eye whose half its pixel lies in (see "Each eye's camera position in the pixel shaders"). Turning it off applies at once; turning it on needs a shader reload. |
| `single_pass.local_lights_per_eye` | On | In the same rewritten shaders, every local point and spot light position, and every spot projection, is measured from the pixel's eye rather than the centred camera the engine built them from (see "The local lights, from each eye"). Turning it off applies at once; turning it on needs a shader reload. |
| `single_pass.screen_door_per_eye` | On | Every screen-door pattern a pixel shader hashes from its pixel position is laid out from the origin of the eye half the pixel lies in (see "The screen-door pattern, from each eye"). Needs a shader reload. |
| `single_pass.clip_origin_per_eye` | On | The remapped shaders that subtract the render camera's position (`cb0[9]`) before the view-projection, the road surfaces and the `roadbias` models, read each eye's copy of it (see "The road surfaces' clip origin, per eye"). Needs a shader reload. |
| `single_pass.ao_volumes_per_eye` | On | The ambient-occlusion volumes drawn once for both eyes: the `aobox` vertex shader takes the reprojection and the volume pixel shaders map each pixel's depth through its eye (see "The ambient-occlusion volumes, per eye"). Needs a shader reload. |
| `single_pass.clouds_per_eye` | On | The cloud layers (`clouds`, `cirrusclouds`, `cloudflythrough`, `softclouds`) take the `M_eye` reprojection: they take their clip position from the full view-projection, which the remap leaves centred (see "Asymmetric and canted eyes"). Needs a shader reload. |

### Per-eye re-issue of a render block

| Flag | Default | Effect |
|---|---|---|
| `single_pass.vegetation_once` | On | Draws a bark or foliage block once for both eyes instead of re-issuing it: each draw binds a reprojected variant of its vertex shader, plain draws take two instances, GPU-indirect draws read a copy of their arguments with every instance count doubled, and hardware-instanced draws are re-issued per eye unless their layout has a twin (see "Drawing the vegetation once"). A block without a variant keeps the re-issue. Applies at once, given variants, which are built at shader creation whenever single-pass is enabled. |
| `single_pass.nvwater_per_eye` | On | The WaveWorks water blocks, whose baked model-view-projection leaves both eyes on the centre view. Not yet verified in the headset. |
| `single_pass.ssdecal_geometry_per_eye` | On | The screen-space decal box geometry, same shape. On top of `single_pass.ssdecal_per_eye`. |
| `single_pass.terrain_cull_both_eyes` | On | The terrain hull programs' frustum cull, inside the G-buffer range under the collapse, against a frustum bounding both eyes rather than the centred camera's (see "The canted rigs"). Needs `single_pass.terrain`. Applies at once. |

### Per-eye viewport and draw routing

| Flag | Default | Effect |
|---|---|---|
| `single_pass.instanced_per_eye` | On | Per-eye re-issue of an already-instanced `DrawIndexedInstanced` with a patched shader bound. |
| `single_pass.hw_instanced_once` | On | Draws such a draw once for both eyes instead when its input layout has a step-rate-doubled twin, which the buildings' `GeneralMkIII` layouts get after injection (see "Drawing the hardware-instanced buildings once"). Applies at once. |
| `single_pass.indirect_per_eye` | On | Per-eye re-issue of the GPU-indirect draws (mechanism 1). |
| `single_pass.slot13_per_eye` | On | Per-eye re-issue of non-indexed world geometry, by pass allowlist (mechanism 1). |
| `single_pass.collapse_viewport_follows_target` | On | Derives the eye halves from the bound render target rather than the scene viewport (mechanism 4). |
| `single_pass.viewport_bind_cache` | On | Skips a routed draw's viewport bind when the two slots already hold what it wants (see "The viewport record"). Applies at once. |

### Per-eye fullscreen and screen-space passes

| Flag | Default | Effect |
|---|---|---|
| `single_pass.reconstruct_per_eye` | On | Deferred clustered-lighting resolve (mechanism 2). |
| `single_pass.fullscreen_camera_per_eye` | On | Every masked per-eye run of a split fullscreen pass: the fragment `GlobalConstants` camera position is moved to that eye for the run, with the matching sun-shadow cascade translation, and put back after. |
| `single_pass.ssao_per_eye` | On | SSAO, computed for each eye in the block's single run: the generation, blur, and composite pixel shaders are rewritten (`dxbc_stereo::localize_ambient_occlusion` and `localize_ambient_occlusion_filter`) and the block binds each eye's basis and view rotation for the generation pass. No split, so the temporal history advances once per frame as in an eye-sized render. Needs a shader reload. |
| `single_pass.ssao_exact_sampling` | On | The SSAO passes' coordinates, texels, and filter weights computed in the shader in every mode, so the left eye's occlusion is the isolated eye's wherever their inputs agree (see "The ambient occlusion's sampling, the same in every mode"). Changes multipass and the isolated eyes too. Needs a shader reload. |
| `single_pass.ssr_per_eye` | On | Screen-space reflections, traced for each eye in the block's single run: the ray march is rewritten (`dxbc_stereo::localize_screen_space_reflection`) to read each eye's view ray, view, projection, camera transform, and view-projections from rows the block binds, and to march in the eye's own pixels; the blur's taps are clamped to the eye's half (`dxbc_stereo::localize_reflection_blur`). Needs a shader reload. |
| `single_pass.foveation_per_eye` | On | The static foveation's mask and fill-in laid out over each eye's half, in their one draw each: every pixel takes its own eye's foveal centre, an eye-sized radius, and eye-local dither and neighbourhood (see "The shipped settings under single-pass"). Applies at once. |
| `single_pass.foveation_gbuffer` | On | The foveation's peripheral stencil test forced through the terrain's apply passes, the road junctions, and the decals as well as the models, the vegetation, and the creatures. Gates multipass too; it sits here so the harness can compare it within one schedule. See "Which passes skip the dropped pixels" in [`foveation.md`](foveation.md). |
| `single_pass.foveation_lighting` | On | The foveation's peripheral stencil test forced through the deferred lights, the light billboards, the sky passes, and the fog gradient, with the fill-in moved to just after them. Gates multipass too. See [`foveation.md`](foveation.md). |
| `single_pass.foveation_quads` | On | The foveation drops whole 2 by 2 pixel quads rather than single pixels, so a dropped quad runs no pixel shader; the fill-in reconstructs a dropped quad from the kept quads around it. Gates multipass too. See "Dropping whole quads" in [`foveation.md`](foveation.md). |
| `single_pass.fsr_per_eye` | On | FSR resolved once per eye over that eye's half, through that eye's own context and history, from eye-sized copies of the colour, depth, and motion vectors, and copied back (see "The shipped settings under single-pass"). Applies at once. |
| `single_pass.ssdecal_per_eye` | On | Screen-space decals: per-eye basis plus a spliced depth-UV bias in the 12 `ssdecal` pixel shaders (mechanism 2). |
| `single_pass.clustered_per_eye` | On | Per-eye clustered froxel light grid. |

The rest of the collapse's per-eye behaviour has no switch and applies whenever the collapse is active: the tree impostors, the static lights, the moon, the water planes, and the stars; the bark, foliage, and occluder block re-issues; the per-eye velocity, atmospheric, fog-volume, glare, subsurface, depth-of-field, and composite-overlay passes; the legacy water's UV and the WaveWorks water's reflection, camera, and reflection-camera size; the terrain detail's per-eye matrices; the once-drawn non-indexed geometry; the uniform viewport slots; the clustered light grid's per-eye light view; the SSAO temporal skip; the skipped effect-depth copy; and the direct per-eye blit.

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
the uniform viewport slots already neutralise. There is no submission saving there
either: they are correctness surface, not duplicated work.

## Known gaps

- **The motion blur's camera reconstruction reads the double-wide previous view-projection.** The motion blur is not part of what a VR player runs (see "The shipped settings under single-pass"), so this is recorded, not pursued. The motion blur rebuilds the camera's motion from depth through the render context's previous view-projection (staged by the motion blur's constant-buffer setup, per the symbol dump; unverified against the release IDB), which under the collapse is the centred camera's over the double-wide target; the velocity buffer, which the shaders now write per eye, carries only what the velocity pass draws (the vegetation and the instanced models). Its effect on the final image under motion was within the repeat's range at the meadow and about 100 to 200 pixels per eye at the town in run 287. FSR's velocity decode under the collapse is covered under "The shipped settings under single-pass", and what its stereo correction does to the pixels the velocity pass does not draw in [`fsr.md`](../rendering/fsr.md).
- **The coverage matrix's defects.** Distant trees' LOD: see "What the first matrix found" under "Capturing varied scenes". The road surfaces, the ambient occlusion in daylight, the moon, and one eye's terrain patches (see "Comparing at the headset's size") are fixed.
- **The baked-matrix families are still reprojected.** The models, characters, buildings, bark, foliage, decals, moon, and water planes take their clip position from a matrix their block bakes on the CPU per draw from the centred camera, and no per-eye form of that product exists in the shader. Their surfaces land a few floats from an isolated eye's and their alpha-tested edges flip where the sub-pixel shift crosses them, which is most of what remains at the shore, the meadow, and the cliff (see "Each eye's own rows, not the centre's reprojected").
- **The right eye's half rasterizes at a larger screen position.** In the double-wide target the right eye's pixels lie one eye width to the right of an eye rendered on its own. With both halves drawn from eye 0's rows, the right half differs from the isolated left eye as the right eye differs from the isolated right eye, while the left half matches (see "The right eye's half of the target"). It is a property of rasterizing one eye width further right, not of any transform, and nothing in the mod reaches it.
- **Multipass reads the other eye's frame in the screen-space reflections.** See "The reference modes' final images".
- **Unpatched geometry has no parallax, and under asymmetric or canted eyes no per-eye projection.** It is re-issued per eye so it is present and correctly sized in both, but it still reads the centred `cb0`, which carries eye 0's projection and no cant. A per-eye binding of the vertex globals around those re-issues was tried and removed (see "Asymmetric and canted eyes"), so an unpatched re-issued draw reads the centred camera. The terrain patch system is not in this set: its pipelines take the pinned-eye reprojection inside the same re-issue.
- **The terrain patch system is drawn twice.** Its GPU-indirect near passes and its far tessellated
  passes are per-eye re-issues with the eye pinned in `cb13`, correct in both eyes but one submission
  per eye, about 140 extra submissions per scene. Bringing them into one draw needs an indirect argument doubling like the vegetation's for the near passes
  and an eye interpolant through the hull stage for the far ones (see "What blocks the largest families").
- **`cb0`'s projection is double-wide.** Unpatched geometry that reads `cb0` rather than `cb13`
  renders horizontally squashed under double-wide.
- **The sun halo is one sprite per screen.** [`SunHaloEffect::PreApply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::SunHaloEffect##func-PreApply) projects the sun through the render context's single projection onto the scene texture's pixel size and draws one halo there, so under the collapse it lands once, somewhere across the double-wide target, rather than once per eye. It runs only while the sun is low, so the midday sky capture does not exercise it. A per-eye halo needs two projections and two composites, which is a draw-count decision, and has not been made.
- **The first frames after entering single-pass can reflect the double-wide field.** See "The planar reflection camera, at one eye's size": in two of three captures taken right after the harness switched from multipass into single-pass, the shore lake's reflection still had the double-wide field. Run 265's first single-pass sample at the shore lake differed from the isolated eyes at 5,407 and 5,385 pixels against 794 and 754 for the next two, consistent with the same transient; its reflection texture was not captured. Unexplained.
- **The other screen-space light sprites are unexamined.** The particle flares (`particleeffectflare*`), the light glows (`lightglow`), and the halos place their sprites from the centred camera's screen position on the CPU, and the flares and glows test occlusion by sampling depth at one full-target UV staged per draw. Their unpatched indexed draws are re-issued into each eye's half, where that UV samples the wrong pixel. None of them drew in the dusk cliff view's draw census, so this is read from the bytecode and the symbol dump and is unverified in game.
- **The tessellated particles are drawn twice.** `particleeffecttess*` pass the world position through a vertex shader the `cb0` remap patches and build clip in an unpatched domain shader, so they cannot take the non-indexed promotion and stay on the per-eye re-issue (up to 85 submissions at the town view). One draw needs the eye carried to the domain shader, the terrain's problem.
- **Screen-space passes leak across the eye seam.** The unsplit screen-space passes run once
  over the double-wide target and sample neighbourhoods; they need a UV clamp at the seam. FSR no
  longer does, with `single_pass.fsr_per_eye` (see "The shipped settings under single-pass").
- **The 14 already-instanced shaders** need their `SV_InstanceID` consumers rewritten to
  `SV_InstanceID >> 1` to survive the doubling. The DXBC transform is offline-testable; the
  per-instance semantics are not.
- **The vegetation's hardware-instanced draws are drawn twice.** Inside a bark or foliage block drawn once, they are re-issued per eye for want of layout twins (see "What blocks the largest families").
- **The reflection blur's compute path reaches across the eye seam.** The screen-space reflection block's compute blur, which [`m_ComputeBlur`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceReflection##field-m_ComputeBlur) selects in place of the two blur draws, is not rewritten, so its horizontal pass still mixes six texels either side of the seam. The cliff view's captures take the draw path.
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

Cost is one extra submission per such draw. The buildings' `generalmkiiihwinstanced` family, between 3 and 644 such draws at the six views of "Accounting for the draw calls", now takes one draw through a twin of its input layout instead (see "Drawing the hardware-instanced buildings once"), which leaves the re-issue with the draws whose layout has no twin: in run 268 only the occluder's instanced depth prime, one call per frame. Since run 270 the vegetation's hardware-instanced draws reach it as well, from inside a bark or foliage block drawn once (see "Drawing the vegetation once").

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

The uniform viewport slots close that window. `VIEWPORT_SLOTS_UNIFORM`
tracks whether the two slots hold the same region — every path that binds them records what it left
behind, and anything unknown counts as non-uniform — and `unify_viewport_slots` re-binds slot 0's
region to both slots when they do not. It runs when the G-buffer range guard drops (which also covers
the GPU-indirect draws nothing detours) and on an out-of-range `DrawIndexedInstanced`
with a patched shader bound. **Slot 0 is never touched**, so the repair can only make dropped instances
reappear; it cannot move what already rendered. The log reports how many repairs a window needed as
`slot-1 repairs`.

## Naming the draws in a pass range

A capture can carry a census of the draw calls its frame made (`crates/jc3vr-capture/src/screenshot/harness/draw_census.rs`), requested by `draw_census = true` under an experiment's `[diagnostics]` alongside `shader_identity = true`, which it needs to name the shaders; `tools/rendering/experiments/cliff_view_dusk_draw_census.toml` is the dusk cliff view with it on. For every draw call made inside a render pass on the capture's frame it records the pass, the render-block type running, the bound vertex shader (by name, and whether the single-pass rewrite patched it) and pixel shader (by the hash of its pristine bytecode), the device-context entry point, and the treatment the draw path gave the call: `instance_doubled`, `per_eye` (submitted once per eye), `once`, `suppressed`, or `repeated`, plus the eye when the call was made inside a render block's per-eye re-issue (`reissue_eye`) or inside one run of a fullscreen pass split per eye (`split_eye`). Calls that agree on all of that are folded into one entry with call and submission counts, which keeps a frame to about two hundred entries. It only observes. It hangs off the draw detours, which the harness also has installed in a multipass capture, so a multipass capture records both eyes' walks as dispatches 0 and 1. A multipass capture's vertex shader names are not reliable: they come from a registry keyed by the shader object, and the shader reload between modes can leave them stale. The pass, the block, and the pixel shader hashes are unaffected.

`python -m tools.rendering census <capture.json> --bundle <Shaders_F.shader_bundle> --passes FIRST:LAST --totals` prints the entries for a pass range with the pixel shaders named from the bundle, and the call and submission totals per treatment. `--accounting` prints the accounting below instead of the entries, and `--multipass <capture.json>` adds a multipass capture's submissions to it.

## Accounting for the draw calls

Single-pass exists to cut the submissions multipass makes, so the census is also the measure of how far it gets. `python -m tools.rendering census <single-pass capture.json> --bundle <bundle> --accounting --multipass <multipass capture.json>` (`tools/rendering/comparison/draw_accounting.py`) counts, for each render pass, each render-block type, and each block and vertex shader:

- the game's calls: every call the census recorded, leaving out the collapse's own repeats, the second eye of a render block re-driven per eye (`reissue_eye` 1) and the second run of a split fullscreen pass (`split_eye` 1). Within the routed range this agrees with the multipass capture's first dispatch to within a few calls, which is the check that it is one walk of the render-pass list;
- the single-pass submissions;
- the multipass submissions for the same pass or block, from both of its dispatches;
- the submissions single-pass spends beyond one per game call, which is what instance-doubling the group would save.

The vertex shader key is compared against single-pass only, since a multipass capture's vertex shader names can be stale (see "Naming the draws in a pass range").

### Six scenes

Run 266, fixed cameras at six views: the coastal town at midday, the road tunnel, the midday shore lake, the dusk cliff view, the meadow below the wooded hill, and the capital's old town. The configs are the census configs under `tools/rendering/experiments/` (`coverage_town_census.toml`, `coverage_tunnel_census.toml`, `coverage_capital_census.toml`, `cliff_view_dusk_draw_census.toml`) `shore_lake_census.toml`, and `coverage_forest_census.toml`. The single-pass column is the variant without the non-indexed promotion, which in these six frames draws exactly as the final tree does (see below). The prepasses (everything before [`RP_RAIN_OCCLUDER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_RAIN_OCCLUDER)) change from frame to frame, because the engine amortises its shadow cascade updates over frames, so they are the mean of three frames in each mode; the routed range is one frame, and it repeats within a few submissions between frames.

| Scene | Prepass calls | Prepass, single-pass | Prepass, multipass | Routed game calls (multipass eye 0) | Routed, single-pass | Routed, multipass | Block re-issue extra | Draw-level per-eye extra | Split fullscreen extra | Game calls | Single-pass | Multipass | Fewer than multipass |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Town | 2,687 | 2,687 | 3,278 | 2,841 (2,733) | 4,249 | 5,464 | 1,064 | 330 | 14 | 5,528 | 6,936 | 8,742 | 21% |
| Tunnel | 1,575 | 1,575 | 1,691 | 2,726 (2,717) | 3,625 | 5,432 | 508 | 377 | 14 | 4,301 | 5,200 | 7,123 | 27% |
| Shore lake | 483 | 483 | 413 | 1,229 (1,221) | 1,807 | 2,439 | 416 | 148 | 14 | 1,712 | 2,290 | 2,852 | 20% |
| Dusk cliff | 645 | 645 | 837 | 1,933 (1,932) | 3,086 | 3,861 | 893 | 247 | 13 | 2,578 | 3,731 | 4,698 | 21% |
| Meadow | 968 | 968 | 863 | 1,720 (1,710) | 2,709 | 3,418 | 771 | 204 | 14 | 2,688 | 3,677 | 4,281 | 14% |
| Capital | 2,820 | 2,820 | 3,535 | 8,372 (8,348) | 10,171 | 16,693 | 982 | 803 | 14 | 11,192 | 12,991 | 20,228 | 36% |

The multipass submissions are measured, not estimated: the census records in a multipass capture too. Multipass submits about one walk of the prepasses (its second eye skips the view-independent ones, see "Why `PreDraw` is outside the collapse") and two of the routed range. Single-pass submits one walk of the prepasses and one walk of the routed range plus the extras, which come to between 21% (the capital, whose buildings are mostly instance-doubled) and 60% (the dusk cliff, mostly vegetation) of the routed game calls. Were every routed call drawn once for both eyes, single-pass would submit the game-calls column: 37% to 45% fewer than multipass rather than 14% to 36%.

### Where the extra submissions go

Summed over the six scenes, by render-block type and vertex shader (run 266, the same captures):

| Block, vertex shader | How | Extra submissions (town, tunnel, shore, cliff, meadow, capital) | Total |
|---|---|---|---|
| `GeneralMkIII`, `generalmkiiihwinstanced` (+ `alphatest`, `vanim`, `prezvelocity` permutations) | already-instanced draw, re-issued per eye (drawn once since run 268) | 101, 235, 4, 53, 34, 644 | 1,071 |
| `VegetationBark`, `vegetationbark*instanced` (colour and velocity) | block re-issue, GPU-indirect (drawn once since run 270) | 332, 136, 206, 306, 202, 214 | 1,396 |
| `VegetationFoliage`, `vegetationfoliage*instanced` (colour and velocity) | block re-issue, GPU-indirect (drawn once since run 270) | 194, 80, 62, 240, 222, 114 | 912 |
| `VegetationFoliage` and `VegetationBark`, `*hwinstanced` | block re-issue, hardware-instanced (a per-eye re-issue of the one draw since run 270) | 184, 14, 24, 222, 178, 188 | 810 |
| `SSDecal`, `ssdecal` | block re-issue | 169, 102, 100, 100, 100, 170 | 741 |
| `VegetationFoliage` and `VegetationBark`, plain indexed | block re-issue (drawn once since run 270) | 108, 147, 1, 0, 49, 235 | 540 |
| `VolumetricTerrainPatch`, `volumetricterrain4blend` (tessellated far and colour passes) | unpatched indexed, re-issued per eye | 69, 62, 76, 102, 89, 78 | 476 |
| `VolumetricTerrainPatch`, near basemesh and tessellated near | GPU-indirect, re-issued per eye | 64, 68, 66, 70, 60, 58 | 386 |
| `NvWaterHighEnd` | block re-issue | 68, 20, 14, 16, 11, 52 | 181 |
| Split fullscreen passes (subsurface skin, deferred lights, atmospheric scattering, fog) | second masked run | 14, 14, 14, 13, 14, 14 | 83 |
| `TerrainDetail` | block re-issue, GPU-indirect | 8 each | 48 |
| `ParticleRB`, tessellated particles | non-indexed, re-issued per eye | 85, 3, 1, 1, 7, 22 | 119 |
| everything else (`renderprezvelocitynotex`, the untessellated and refraction particles, the occluder, the sun) | | | 63 |

The vegetation was the largest group by a distance (3,658 between its three draw kinds; since run 270 only its 810 hardware-instanced submissions remain, see "Drawing the vegetation once"), then the hardware-instanced buildings (1,071, two thirds of it in the capital), the screen-space decals (741), and the terrain patches (862).

### Drawing patched non-indexed geometry once

The geometry passes on the non-indexed entry point (`GEOMETRY_SLOT13_PASSES`) used to re-issue every draw per eye through `instanced_per_eye`, patched vertex shader or not. A patched shader picks its eye from the instance parity and writes the viewport index, so for it one two-instance `DrawInstanced` with the viewport split is enough, the promotion the static light sprites take. The collapse promotes such a draw, and leaves it to the per-eye re-issue when a domain or geometry shader is bound: only the last stage before the rasteriser routes a draw, and the tessellated particles (`particleeffecttess*`), whose vertex shader the `cb0` remap patches, build clip in an unpatched domain shader that writes no viewport index, so both instances of a promoted draw would land in the left eye's half.

In run 266 the promotion did not yet exclude those pipelines. The only patched non-indexed draws in the six frames were the tessellated particles, and the promotion took them from two submissions to one (85 fewer at the town, 22 in the capital, 1 to 7 elsewhere); the final image against the isolated eyes did not move outside the repeat's range in any scene (town 10 and 12 px per eye with the promotion on, off, and the repeat; capital 682 and 665 on, 625 and 626 off, 725 and 491 repeat, ranges 461 to 887; meadow 1,370 and 3,878 on, 1,389 and 3,924 off, 1,411 and 3,884 repeat; shore lake 1,095 and 811 in all three). Those particles evidently contribute nothing above the comparison's threshold at these views, so the frames cannot tell a correct promotion from the left-eye-only one the pipeline implies, and the tree now excludes them. What is left for the promotion is untessellated patched geometry drawn without an index buffer: the skidmarks (two calls at the shore lake and in the capital in run 265, none in run 266's frames) and any index-buffer-less model. Its saving in these scenes is a few submissions at most. Run 267 ran the exclusion: the tessellated particles were back on the per-eye re-issue in every single-pass frame, two submissions per call (16 and then 46 to 48 calls at the town over two schedules, 11 to 17 in the capital, 5 at the meadow), exactly as without the promotion, and the routed range's submissions matched the variant without it within one; the only patched non-indexed draws promoted were the water reflection planes and the moon, which take their own promotions. The final image did not move outside the repeat's range (town 444 and 747 px per eye with the promotion on, 467 and 742 off, 649 and 706 repeat, then 424 and 753 on, 463 and 777 off, 557 and 778 repeat on the second schedule; the capital's first schedule was unsettled, 1,280 and 1,287 on, 1,347 and 1,529 off, 3,213 and 880 repeat, with ranges up to 7,110; meadow 2,340 and 3,733 on, 2,372 and 3,813 off, 2,370 and 3,782 repeat).

### Drawing the hardware-instanced buildings once

The buildings' hardware-instanced draws take their per-instance data from vertex-buffer elements, so doubling the instance count alone reads past the batch's data. Doubling every per-instance element's `InstanceDataStepRate` as well keeps instances 2k and 2k+1 on element k: D3D11 fetches a per-instance element at `StartInstanceLocation + SV_InstanceID / InstanceDataStepRate` with `SV_InstanceID` counted from zero, which a standalone check of the DXVK that Proton Experimental ships confirmed at several start instances and buffer offsets. With the eye in the instance parity, one draw through a twin of the layout with doubled step rates serves both eyes.

D3D11 cannot read a layout's elements back, and the engine builds them on the stack inside [`RenderBlockTypeGeneralMkIII::Create`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMkIII##func-Create) at registration, before the payload is injected. The handle [`CreateVertexDeclarationInstanced`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine##func-CreateVertexDeclarationInstanced) returns keeps only the layout and the streams' byte sizes. So the payload runs that `Create` again, on a scratch copy of the live type object (the `RenderBlockTypeGeneralMkIII` singleton), with every other effect suppressed for that one call. `Create` calls nothing but [`TextureCacheGetTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine##func-TextureCacheGetTexture) for the wind noise texture, the video card query, [`LoadShaders`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMkIII##func-LoadShaders), the two declaration creators, [`CreateSamplerStates`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMkIII##func-CreateSamplerStates), and [`CreateConstantBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine##func-CreateConstantBuffer), and stores every result in the object. The detours in `hooks/graphics_engine/declaration_replay.rs` check a thread-local flag the replay raises: inside it the texture, shader-holder, sampler, and constant-buffer calls and [`CreateVertexDeclaration`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine##func-CreateVertexDeclaration) return without acquiring anything, and `CreateVertexDeclarationInstanced` is called through with a copy of its element array whose step rates are doubled. The copy already holds the live object's shader holders, which is all the declaration code reads (each declaration's vertex program), and the engine creates every twin itself, so the mod holds no element table. Each twin lands in the field the live object keeps the original in, so pairing the copy's declaration fields (`m_VertexDeclaration`, `m_VertexDeclarationHWInstancingVelocity`, `m_VertexDeclarationDepth`, `m_VertexDeclarationDepthAlphaTest`) with the live object's maps each live `ID3D11InputLayout` to its twin. The pairing refuses a twin in a field the live object leaves empty, one twin in fields with different live declarations, and a twin in no field, and then destroys what it created.

The replay runs on the draw thread at the start of a dispatch (`begin_dispatch`), once single-pass is active, and runs again if the live declarations change; a failed build is not retried until they do. The twins are destroyed through [`DestroyVertexDeclaration`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine##func-DestroyVertexDeclaration) on eject, after the draw detours are removed, and a draw still finishing holds its own reference to the layout it bound. Because nothing depends on the payload being present at registration, a late injection and each re-injection build their own twins; run 268 exercised the late injection, and an eject and re-injection within one game process have not been exercised. The replay detours stay installed for the session and forward unchanged outside the replay.

At an already-instanced draw in the routed range with a patched vertex shader bound, `single_pass.hw_instanced_once` (**on**) looks the bound layout up; with a twin it binds the twin, splits the viewport, submits the draw once with its instance count doubled, and binds the engine's layout again. It declines without a recorded full viewport, with a domain or geometry shader bound, or when the doubled count overflows, and a layout with no twin is logged once and left to `instanced_per_eye`. The shadow and other unrouted passes are untouched. The vegetation's hardware-instanced draws reach it from inside a block drawn once (see "Drawing the vegetation once"), find no twin, and are re-issued per eye.

Run 268 measured it at the town, the tunnel, and the capital (`coverage_*_census.toml`, whose `hwi-off` variant turns the switch off in the same schedule), running each config twice and reading the second, since the first schedule after a teleport is unsettled. The replay twinned all 10 of the type's instanced layouts once, at the first single-pass dispatch, and the debug UI's single-pass section shows the count. Every routed `GeneralMkIII` hardware-instanced call became one instance-doubled submission, and the routed range's submissions fell by exactly the buildings' re-issue:

| Scene | Routed, switch off | Routed, switch on | Routed, multipass | Fewer than multipass, off and on |
|---|---|---|---|---|
| Town | 3,940 | 3,838 | 4,990 | 21%, 23% |
| Tunnel | 3,827 | 3,571 | 5,798 | 34%, 38% |
| Capital | 10,272 | 9,629 | 16,920 | 39%, 43% |

The final image against the isolated eyes stayed within the repeat's range (left and right eye, px differing): town 472 and 686 with the switch on, 507 and 707 off, 576 and 772 repeat (the first schedule 418 and 744 on, 382 and 810 off, 561 and 640 repeat); tunnel 6 and 8 in all three (8 and 18 in all three on the first schedule); capital 1,585 and 816 on, 1,441 and 1,456 off, 1,491 and 971 repeat, with ranges of 616 to 4,279 and 321 to 2,903 on against 370 to 4,618 and 271 to 3,592 for the repeat (the first schedule 1,310 and 929 on, 1,032 and 1,016 off, 1,378 and 1,057 repeat). The census still records the occluder's instanced depth prime on the re-issue, its layout untwinned, and the shadow passes' hardware-instanced draws once and unchanged.

### Drawing the vegetation once

The bark and foliage blocks bake the centred camera's view-projection into a vertex constant buffer inside their own `Draw` and `DrawZ`, so the fallback re-issues the whole block once per eye with that constant reprojected (see [`single-pass-render-blocks.md`](single-pass-render-blocks.md)). `single_pass.vegetation_once` (**on**) draws such a block once instead (`crates/jc3vr-render/src/stereo/single_pass/vegetation_once/`).

What a single draw has to cover, from the shader bundle and the blocks' code:

- **Which shader a draw uses.** The per-block setup, [`RenderBlockBark::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockBark##func-Setup) and the foliage block's `Setup` (unverified against the release IDB, so not bound) (and their `SetupZ` for the depth and velocity passes), binds the vertex shader and input layout by the block's draw kind, plain, GPU-indirect, or hardware-instanced, before `Draw` runs; `Draw` stages the constants and issues the draw. The only exception is the billboard path, which `Draw` selects from [`RENDERSTATUS_BILLBOARD_ARRAY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderStatus##flag-RENDERSTATUS_BILLBOARD_ARRAY) and which binds its own vertex and geometry shaders. The vertex shader bound when `Draw` begins is therefore the one its draws use. No hull, domain, or geometry stage is bound outside the billboard path.
- **The eye-pass shaders.** Per family, a colour, depth (`prez`), and velocity permutation of each draw kind: `vegetationbark`, `vegetationbarkhwinstanced`, `vegetationbarkinstanced`, their `prez` and `velocity` counterparts, and the same for `vegetationfoliage` plus the `_osnormalmap` colour permutations. The shadow, reflective-shadow, and billboard permutations draw outside the eye split. Every one builds clip as a multiply-add chain over the baked matrix, so the reprojection that serves the scene families applies.
- **How each kind fetches its instance.** The plain permutations have no instance. The hardware-instanced ones read the instance's transform from `INST_TEXCOORD0..2` and `INST_NORMAL` on a per-instance vertex stream, like the buildings. The GPU-indirect (`*instanced`) ones read it from the structured buffers bound at `t0`/`t1` (bark) or `t1`/`t4` (foliage) by `SV_InstanceID` plus an instance offset: the offset is staged in the baked constant buffer, either directly or as an index into the per-pass offset buffer the vegetation update fills. Each reads `SV_InstanceID` exactly once, in the `iadd` that forms the index.
- **Where the indirect arguments live.** Both block types draw from one argument buffer of the vegetation update, `veg_draw_indirect` per pass: five dwords per draw for every mesh type and every vegetation pass, filled from compute in the vegetation update prepass, which runs before any routed pass. Each draw passes its own offset into it. The buffer has no CPU copy.
- **What stays shared.** The wind, the per-instance fades and LOD, the foliage's wind-noise lookup origin, and the bark's camera-relative offset all read the engine's single computation from the centred camera, as they do under the re-issue. The pixel shaders read the eye from the pixel position already: the camera position, the local lights, and the screen-door pattern are covered by the pixel-shader rewrites ("Each eye's camera position in the pixel shaders" and the two sections after it).

The single draw:

- Each eye-pass permutation gets a variant, built from its bytecode when the engine creates it (`vegetation_once/variants.rs`) and attached to the engine's shader as private data. The plain and hardware-instanced permutations take `dxbc_stereo::reproject_vertex_shader`. The GPU-indirect ones take `dxbc_stereo::reproject_instanced_vertex_shader`, which reuses the shader's own `SV_InstanceID` for the eye parity and renames its one other read to `SV_InstanceID >> 1`. The engine's shaders stay pristine, so the shadow passes and the re-issue are unchanged, and the variants are built whenever single-pass is enabled, so the switch applies at once.
- The block's `Draw` runs once inside a scope the draw detours consult. Each draw binds the variant of the bound shader and puts the engine's back after it. A plain draw becomes one two-instance draw with the viewport split. A GPU-indirect draw is issued against a copy of the argument buffer at the draw's own offset (`vegetation_once/indirect_args.rs`): a compute pass of the mod's (`crates/jc3vr-render/shaders/indirect_args_double_cs.hlsl`) copies the engine's buffer with every record's instance count doubled, once per routed pass of each dispatch at the first draw that needs it, putting back the compute bindings it touched. A hardware-instanced draw takes a layout twin when its layout has one and otherwise is re-issued per eye with `cb13` pinned to each eye (the `instanced_per_eye` re-issue, with the variant bound), which costs what the whole-block re-issue spent on it.
- A block takes the re-issue instead when the switch is off, outside the eye split, while drawing billboards, when the shader its setup bound has no variant, or once the argument copy has failed. A draw inside a single-drawn block that turns out not to be convertible after all is re-issued per eye without parallax and logged once; none did in the runs below.

The single draw's velocity permutations reproject only `SV_Position`, so the clip positions they hand their pixel shader are the centre camera's, and under the re-issue the current one is the eye's and the previous one the centre camera's. Neither reaches the velocity buffer as it stands: the velocity pixel shaders rebuild both positions per eye (see "The motion vectors, in each eye's screen space").

Runs 270 and 272 measured it at the meadow, the dusk cliff view, the town, and the shore lake (the census configs, whose `veg-off` variant turns the switch off in the same schedule), reading the second schedule after each teleport. Every routed bark and foliage GPU-indirect and plain call became one instance-doubled submission; the hardware-instanced calls stayed at two submissions each. No draw fell back, and the argument copy was made at the first routed vegetation pass of each run:

| Scene | Routed, switch off | Routed, switch on | Routed, multipass | Vegetation extra, off and on | Fewer than multipass, off and on |
|---|---|---|---|---|---|
| Meadow | 2,717 | 2,244 | 3,507 | 653, 180 | 23%, 36% |
| Dusk cliff | 4,381 | 3,804 | 5,529 | 793, 216 | 21%, 31% |
| Town | 3,912 | 3,276 | 5,149 | 816, 182 | 24%, 36% |
| Shore lake | 1,874 | 1,605 | 2,574 | 293, 24 | 27%, 38% |

The final image against the isolated eyes stayed within the repeat's range (left and right eye, px differing, switch on, off, and the repeat): meadow 2,142 and 3,573, 2,165 and 3,556, 2,251 and 3,561 (the first schedule 1,595 and 3,794, 1,502 and 3,818, 1,570 and 3,753); dusk cliff 152 and 90, 220 and 139, 183 and 110 (a fresh session's first schedule 31 and 17, 30 and 17, 30 and 16); town 823 and 864, 610 and 771, 670 and 666, with ranges of 541 to 1,111 and 673 to 1,095 on (the first schedule 691 and 805, 665 and 748, 719 and 780); shore lake 1,095 and 831, 1,094 and 835, and a repeat caught in the shore's transient at 6,487 and 6,275 (the first schedule 1,033 and 842 on, 1,019 and 842 off and repeat). The meadow at dusk (`coverage_forest_dusk.toml`) gave 46 and 53 in all three, and the town with the camera moving (`coverage_town_translation.toml`) 1,582 and 2,244 on, 1,518 and 2,237 off, 1,586 and 2,133 repeat, so nothing new depends on the motion.

### What blocks the largest families

**The vegetation's hardware-instanced draws** (`vegetation*hwinstanced`, about a fifth of the vegetation's former extra submissions) are the part of the bark and foliage blocks still drawn twice: inside a block drawn once (see "Drawing the vegetation once") each is re-issued per eye, because their layouts have no step-rate-doubled twin. The bark and foliage types create theirs in their own `Create` through [`CreateVertexDeclarationInstanced`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine##func-CreateVertexDeclarationInstanced), two layouts each (the colour one and the one the depth and velocity passes share), but replaying those `Create`s is a larger job than the buildings' was: besides acquiring shader holders, sampler states, and declarations, the bark type's registers a callback with the environment graphics manager and console variables, and both would have to be suppressed and their callees bound before the replay could run on a scratch copy. The GPU-indirect and plain draws, the other four fifths, are drawn once.

**The screen-space decals** (about 120 per scene) are re-issued as a block because each eye needs its own reconstruction basis and depth-UV offset staged on fragment `cb1`, and the box's baked matrix reprojected. The `ssdecal` vertex shader is a bare multiply-add chain over vertex `cb1[0..3]`, like `watermask`, so the reprojection would move the box; but it also derives the projective screen UV from the clip position before the epilogue, which would leave that UV the centre's, and the 12 pixel shaders would have to pick the basis and the depth offset by the eye their pixel lies in, the way the pixel-camera rewrite picks camera rows. Both are rewrites of a kind the project already has; neither exists for this family.

**The terrain patch system** (about 140 per scene: the tessellated far and colour passes, and the GPU-indirect near passes). The tessellated permutations' vertex shaders write all four lanes of `TEXCOORD3`, so the eye cannot ride the free lane the base terrain uses; a single draw needs a new interpolant added to the vertex shader's output signature, forwarded by the hull shader's control-point phase, and read by the domain shader, which then writes the viewport index. That is a signature-changing rewrite across three stages, where the lane path only widens a mask. The near passes are `DrawInstancedIndirect` with a vertex shader that already reads `SV_InstanceID`, so they need an argument doubling like the vegetation's (see "Drawing the vegetation once"), over four-dword `DrawInstancedIndirect` records rather than its five-dword indexed ones, and their tessellated half needs the interpolant as well. The non-tessellated near basemesh is the one part that needs only the argument doubling, at about 32 submissions per scene.

**Particles** (up to 85 per scene at the town, a few elsewhere). The tessellated permutations (`particleeffecttess*`) pass the world position through their vertex shader, which the `cb0` remap patches for a camera-row read, and build clip in an unpatched domain shader, so they have the terrain's problem; the others are on the policy's NDC-writer exclusion list. Not worth doing before the above.

**Fullscreen passes split per eye** (about 14 submissions per scene): the screen-space subsurface skin's blurs and lighting capture, the deferred lighting's light assignment and clustered resolve, the atmospheric scattering, and the fog volume's blur. Each could become one run with an eye-picking pixel shader, as SSAO, SSR, and the glare chain now are, for a handful of submissions each.

Nothing in the prepasses is issued twice: under the collapse `PreDraw` runs once (see "Why `PreDraw` is outside the collapse"), and the per-dispatch passes that multipass runs twice (the vegetation interaction volumes and the rain occluder) run once.

## Capturing single-pass with a switch off

The sections from here on describe the original capture path, which writes every capture and its stages to files for the Python comparison. For hill-climbing, the fast comparison renders each mode once, compares on the GPU, and reruns a list of scenes against a re-injected payload in about a minute; see [fast-comparison.md](../testing/fast-comparison.md).

Scene content at the capture views changes between schedules and sessions (the water, the vegetation, the player's pose), so a before-and-after measured across two runs mixes a fix's effect with the scene's. An experiment can instead list single-pass variants, each with some of the `single_pass.*` switches forced:

```toml
[[single_pass_variants]]
name = "ssr-off"
switches = { ssr_per_eye = false }

[[single_pass_variants]]
name = "repeat"
```

After its configured modes, the experiment configures the single-pass mode once per variant and captures `samples` more frames, in the same schedule as the default single-pass capture and the isolated eyes. The switches travel in the configuration request as `[visual_overrides.single_pass]`, beside the request's other overrides, and the capture profile resets them on the next configuration, so a variant lasts exactly as long as its captures. The names are the configuration field names in the flag tables above, and the overridable set is `SinglePassSwitch`, the switch table's rows (see "Configuration"): every production switch except the master switch and the diagnostics. The Python side reads the set from the table's manifest. An unknown name fails the configuration, on the Python side when the experiment loads and in the payload when it parses the request. A variant with no switches captures the default configuration again, which gives the others a same-schedule noise floor. Variant captures carry only the shader identity census of the experiment's diagnostics, which keeps them small, unless the variant sets `diagnostics = true`. Variants need all four modes configured and exclude a matched treatment and a feature check.

A switch that is a rewrite at shader creation (the ones the tables mark as needing a shader reload) only takes effect once the shaders are re-created. The payload records the state the shader set was last created under, `ShaderBuild`: whether single-pass and its collapse were live, and the value of every creation-time switch. A configuration whose state differs from that record reloads the shaders before its warm-up starts; the reload is synchronous (the engine re-creates every shader before `LoadShaderBundle` returns), and a reload during which the configuration changed records no state, so the next configuration reloads again. The configuration fails if the record still differs after the reload, and every warm-up frame requires it to match, so a capture is never taken over bytecode built for other switches. Each capture's metadata carries the record as `visual.shader_build` next to the effective configuration, and the experiment checks, for every single-pass capture of a run with variants, that the effective configuration holds the requested values and that the shaders were built under the effective configuration.

Each variant becomes its own manifest case, named `<experiment>-variant-<name>`, which borrows the default case's isolated-eye and multipass samples, so the comparator measures it against the same references. The experiment report's `single_pass_variants` lists, for the default capture and each variant, the forced switches and per eye the final-image pixels that differ from the isolated eye by more than three codes, over every pairing of an isolated-eye sample with a single-pass sample (the median, the range, and each pair), and the log states the same as one line per variant.

## Capturing varied scenes

The comparison so far reads the views it has been run at. To reach the shader families those views never draw, an experiment can move the player somewhere else, pin the weather, put objects in front of the camera, and move the camera.

**Staging.** A `[scene.stage]` table makes the experiment send a `visual_stage` request before its first configuration (`crates/jc3vr-tooling/src/testing/stage.rs`, `tools/rendering/protocol/stage.py`). The payload teleports the player through the director's teleport, which streams the destination in when it is far, waits until the player has stood in gameplay with no teleport in flight for sixty frames, submits the spawns through the director's spawner, and then lets the game run for `settle_seconds` of wall-clock time so streaming, physics, and the spawned entities come to rest before a configuration holds the clock. The acknowledgement reports where the player landed, which the experiment report carries as `stage`:

```toml
[scene.stage]
settle_seconds = 20.0

[scene.stage.location]
position = [2091.2, 1040.1, 5005.0]
yaw_deg = 270.0        # heading about world up, from +Z toward +X

[[scene.stage.spawn]]
tags = "vehicle,ground,car,civilian,vintage,mugello,sports,unarmed,unarmored,ambient,liberated,highend"
position = [2087.0, 1041.1, 5007.5]
yaw_deg = 0.0
```

A spawn takes any tag set the spawn system matches (the spawner's `spawn_defs.json` lists the shipped ones). Spawned entities persist, so the payload submits each tag set and position once per session and reports a repeat as already present; a timed explosive barrel explodes during the first settle and is gone by the next. A stage is refused while a configuration holds the clock. The player stays where the stage left them, so a config without a stage after one with it runs wherever that was, and the configs for the earlier views need the player back near them.

**Weather.** `scene.environment.weather` takes `sunny`, `rain`, or `snow`, each fired as the named weather event that pins it ([`WeatherController::EVENT_RAIN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/environment::WeatherController##const-EVENT_RAIN) and [`EVENT_SNOW`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/environment::WeatherController##const-EVENT_SNOW) beside the existing sunny one), followed by the instant event. Every capture's observed weather control must name the requested weather, and the observed severity must be clear for `sunny` and a storm for the other two. The game exposes no overcast or fog state of its own: the weather is one severity scalar with rain and snow ratios, and the named events are the states it can hold. Pinned at the configuration alone, a storm never runs with the clock: in run 260 the observed severity was the storm's but the town looked barely different from clear weather. So a staged scene's stage also carries the scene's environment: the payload sets the hour and fires the weather event before the teleport, without holding the day cycle, and the weather runs for the whole settle before the configuration pins it again. The stage saves nothing to restore; the configuration's restore returns the weather to the ambient system, and the hour stays the staged one.

**A moving camera.** A `[motion]` table in place of `scene.position` and `scene.orientation` drives the camera along the payload's periodic trajectory about the game's frozen camera, one step per rendered frame, in every configuration: the angles swing as `sin θ` (pitch as `sin 2θ`) and the camera-local translation as `sin θ`, `θ` being the frame's fraction of `period_frames`. Every capture is taken at `phase`, after a full cycle and with the eight frames before the phase rendered on the trajectory, so the previous-frame matrices the temporal effects read are the same trajectory frames in every mode. The clock is held, so only the camera moves:

```toml
[motion]
period_frames = 120
yaw_deg = 15.0      # amplitudes, 0 to 45 degrees
pitch_deg = 5.0
forward_m = 3.0     # right_m, up_m, forward_m: -20 to 20 metres
phase = 15
```

All amplitudes at zero gives a static camera at the game's own camera, which is the simplest way to frame a staged location without knowing its terrain height. A motion experiment's captures take no diagnostics beyond the shader identity census, but every capture still writes the stage images the comparison reads, and its variants work as in a fixed-pose experiment. To name a scene's draws, a fixed-pose config at the camera a motion run observed (the capture metadata's `visual.pose.anchor`) can carry `draw_census`.

**Leaving a source out.** To tell whether a difference comes from one producer, `[visual_overrides]` takes `ssao = "disabled"` and `ao_volumes = "disabled"`, which leave out the screen-space ambient occlusion block and the ambient-occlusion volumes' pass ([`RP_AO_VOLUMES`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_AO_VOLUMES)) in every mode, isolated eyes included, so the comparison measures single-pass without them. `global_illumination = "disabled"` leaves out [`RP_GLOBAL_ILLUMINATION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_GLOBAL_ILLUMINATION), `screen_space_reflections = "disabled"` the reflections, and `fsr_hybrid_velocity = "disabled"` FSR's hybrid motion vectors. The capture profile resets them on the next configuration. Leaving out the reflections or the AO volumes is not a clean subtraction under single-pass: with either skipped, single-pass differs from the isolated eyes far more than with it running (about 200,000 pixels per eye at the town without the reflections, and 7 to 8 million without the AO volumes, where multipass stays within its floor), so a consumer still reads what the skipped pass would have written. Neither is a shipped setting, and the cause is not found.

**Running the motion blur.** The motion blur never runs in VR (see "The motion blur and the depth of field" under "The shipped settings under single-pass"), so the final images do not read the velocity buffer and no override puts the blur back. The velocity buffer itself is captured in every mode as the `velocity` stage.

**A headset-like rig.** The harness's simulated headset has symmetric, parallel eyes, which leaves every path that only holds for those eyes unexercised. `headset` in `[visual_overrides]` names a rig that the frame loop substitutes for the located views in every mode, where it reads them (`crates/jc3vr-capture/src/testing/visual/headset.rs`): each eye's field of view, its turn from the located head, and the distance between the eyes. Everything downstream sees the rig, from the per-eye projections and offsets to the cull and shadow-fit frusta and the single-pass rows, and the compositor is handed the same views. The presets approximate the shape of each headset's views; they are not vendor data:

| `headset` | Left eye's field of view (left, right, up, down) | Turn of each eye | IPD |
|---|---|---|---|
| `"index"` | −54°, 51°, 55.8°, −55.8° | 5° outward | 64 mm |
| `"quest"` | −50°, 43°, 48°, −52° | none | 63 mm |
| `"stress"` | −60°, 35°, 49°, −58° | 12° outward, pitched 1° up (left) and down (right), rolled 1° outward | 70 mm |
| `"beyond"` | −51°, 45°, 48.2°, −48.2° | none (no measured cant is available; parallel displays assumed) | 63 mm |

The right eye mirrors the left. Each preset keeps the tangent aspect of the eye target it is meant for, so its pixels stay square there: the harness's default 320 by 360 for the first three, and a square eye for `beyond`, approximating a Bigscreen Beyond 2e. The per-eye resolution is the session's, never the rig's. A session launched with `--eye-size WIDTHxHEIGHT` (`python -m tools.rendering.session.matched_run` passes it, and `--resolution-scale`, through) seeds `vr.recommended_size_override` in place of the simulated headset's recommended size, so the harness can capture at a real headset's per-eye size; the width is still rounded up to the 64-pixel tile the payload aligns every eye to. The stage images grow with the eye, so `stages = "geometry"` in `[visual_overrides]` keeps only the depth, the albedo and normal G-buffers, and the velocity. The first frame that locates the views logs the runtime's own field of view and turn from the head for each eye, so a session on a real headset records what a preset can only approximate. Each capture records the rig it was rendered under as `visual.headset`, and the experiment checks that it is the requested preset and that each eye's recorded projection, turn from the centre camera, and offset carry it (`tools/rendering/protocol/headset.py`). The full-camera freeze holds the eye parameters it latched under the rig in force, and latches them again when a configuration names another. A rig applies to every mode of an experiment, so comparing rigs takes one experiment per rig; `tools/rendering/experiments/headset_*.toml` are the town at midday under each, with variants that flip the switches of the fixes below.

**Running the shipped settings.** The capture profile also pins or turns off what a player runs with: FSR, the foveation, the engine's exposure metering and its histogram, the screen fade, and the player-damage overlay. `[visual_overrides]` puts each back at the mod's shipped value in every mode with `fsr`, `foveation`, `auto_exposure`, `screen_fade`, and `player_damage` set to `"enabled"`. FSR's history is reset on every configuration, so every mode accumulates it from its own warm-up. Every capture's metadata carries the exposure the post chain applied as `visual.exposure`, so captures with the engine's metering can be checked for one shared exposure. See "The shipped settings under single-pass" for what each one does under the collapse.

**Determinism.** The configuration holds the clock, which freezes everything the game animates on it: the capital's pedestrians, the traffic on a distant road, a wind turbine, and the rain's particles stood still, and the isolated eyes' captures repeated within a mode at up to about 2,300 pixels per eye on the mountain top and the farmland, a few hundred elsewhere, and none at all in the hangar, the harbour, and the tunnel. The exception is the capital's old town, where the stage had set off a timed explosive barrel: the lit walls drifted between captures with the clock held, and the isolated eyes' own repeats differed at 2,000 to 21,000 pixels per eye, so that scene cannot be compared pixel for pixel. A fire's light animating on real time is the likely source; it is unverified. Read every scene's single-pass difference against the same-schedule floor the `repeat` variant gives (see "Capturing single-pass with a switch off") and against the isolated eyes' own repeats.

The coverage configs are `tools/rendering/experiments/coverage_*.toml`: eight staged locations (the capital's old town, a coastal town, a hangar at a military airport, a military harbour, a road tunnel, a mountain-top base, a meadow below a wooded hill, and farmland) at midday and at night, two at dusk and in rain and snow, the town with a spawned car and pedestrian, a rotating and a translating camera at the town, and fixed-camera configs at the poses those runs observed, carrying the draw census, switch-off variants, or a left-out source for attribution. The `coverage_*_fixes.toml` configs measure the road and ambient-occlusion volume fixes at four of those poses.

**What the first matrix found** (runs 260 to 263, final-image pixels differing from the isolated eye by more than three codes, per eye of 115,200):

- Road surfaces. The `SplineRoad` and `RoadJunction` blocks wrote G-buffer normals, albedo, and specular that differed from the isolated eye's across the whole road at matching depth, as a dense speckle: 17,000 to 30,000 pixels in the town and on the farm road. The road surfaces and the `roadbias` model permutations subtract the render camera's position (`cb0[9]`), which the remap left centred, so each eye's road slid half the eye distance within its own plane; `single_pass.clip_origin_per_eye` fixes it (see "The road surfaces' clip origin, per eye").
- Ambient occlusion in daylight. With the screen-space ambient occlusion left out in every mode the tunnel fell from 43,900 to 5,500 pixels and the capital from 11,200 to 900, but the source was not the occlusion generation: it was the ambient-occlusion volumes, whose result reaches the image only through the SSAO block's composite, sampled at the wrong place through the centred camera. `single_pass.ao_volumes_per_eye` fixes it (see "The ambient-occlusion volumes, per eye"); the tunnel's remaining 5,500 pixels were its road, which `single_pass.clip_origin_per_eye` fixes.
- The terrain under a moving camera. With the camera rotating or translating, one eye's depth differs from the isolated eye's in polygonal patches of mid-distance ground (3,700 and 5,300 pixels in eye 0 in runs 260 and 261), where the static camera has 46. Runs 264 and 265 settled what it is not. Which eye shows the patches changes from run to run at the same views: eye 1 at the rotating view in runs 264 and 265 (4,276, 2,032, and 3,526 pixels; the second with the single-pass mode configured before the isolated eyes rather than after), none at all in a third rotating run, eye 1 at the translating view in run 264 (3,382) and eye 0 in run 265 (4,197). The other eye matches the isolated eye at 0 to 20 pixels in every run, and multipass matches the isolated eyes at 0. Turning `single_pass.terrain_patch_per_eye` off leaves the patches unchanged (4,291 against 4,276), so the pinned-eye re-issue is not their source. The depth error is uniform and small, a ratio of 0.9999 to the isolated eye's, the size of a surface choice such as a tessellation or morph level differing, not of a camera a frame out of date, which would also have moved the eye that matched. That one eye always matches also rules out stale per-eye constants staged for one eye: `cb13`, the pixel shaders' camera rows, and the eye globals are derived per pass from the frame's own centre transform. The same eye's ground differs in the G-buffer normals by a code or so across the whole surface (13,000 to 18,500 pixels, against 2,000 to 6,000 in the other eye and 2,400 to 3,200 in both with a static camera, and 0 for multipass), and that eye's final image differs at about 900 more pixels than the other's (run 265: 1,261 and 2,244 rotating, 1,653 and 871 translating, against a same-schedule floor of 534 and 513, and 107 and 105), of which the depth patches themselves account for about 100. Run 309 at the headset's size found the cause, and it is not the detail state: the terrain-detail draw's per-eye transform was the centre's reprojected by `M_eye`, which rounds badly enough to scale one eye's ground depth by about that ratio, and which eye takes the larger error depends on the view. Building each eye's transform from its own rows removes it; see "Comparing at the headset's size". The patch system's prepasses rebuild the detail every frame from the camera position they read, so the detail state is not history.
- The moon. At night in the capital the moon (`open` in [`RP_MOON`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MOON)) was missing from eye 0 and stretched to twice its width in eye 1; the moon's promotion fixes it (see "The moon and the water reflection planes, once for both eyes").
- Distant trees. Whole trees at mid distance differed in both eyes in the meadow (about 1,500 to 3,700 pixels with the grass): the tree impostors in their distance fade, dissolved by a pattern laid out over the double-wide target. The impostors' screen-door rewrite fixes it (see "The tree impostors' screen door, per eye").

The particle families drew in the tunnel, the hangar, the town, and the capital (`particleeffect`, `particleeffectadditiveshadow`, `particleeffectsoftadditiveshadow`, `particleeffecttess*`, and in the tunnel `particleeffectflare` in [`RP_PARTICLE_ONSCREEN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_PARTICLE_ONSCREEN)), all re-issued per eye; the rain added `particleeffectrefraction`, re-issued per eye too. None left a difference that stood out from the scene's others, but in the tunnel the flares sit inside the occlusion difference and were not isolated. The decal families that drew (`generalmkiiidecal`, `propdecal`, and screen-space decal permutations not seen before, all as expected for their paths) left nothing that stood out. `lightglow`, the halos, the lightning, and skidmarks did not draw in any view.

**The content run 292 added.** With the draw census on, at the coastal town (a sports car spawned broadside six metres ahead and a pedestrian three metres ahead, at night, and in a staged storm), the capital's old town at night with a timed explosive barrel, and the shore lake at dusk facing the setting sun, under the default and the Index-like rigs. The car's paint, glass, wheels, and lights (`carpaintmm*`, `carlight_deform`) and the pedestrian's skin and hair (`characterskin*_cam`, `characterhair_msk`, `characterhair_blend`) are all instance-doubled through the remap, as are the skidmarks (`skidmarks_normal`, one draw in `RP_SKIDMARKS` in every scene), and none left a difference beyond the repeat's: 731 and 731 pixels per eye with the car and pedestrian in frame under the default rig against 771 and 673 for the repeat, and 1,995 and 719 under the Index-like rig against 2,071 and 709. The storm drew the rain's refraction (`particleeffectrefraction`, re-issued per eye) and no lightning or `weather` draw, and differed at 421 and 246 pixels against 415 and 249. The night scenes drew the stars (`starsshader`) once across the whole double-wide target, the moon's defect before its promotion; at 320 by 360 the stars are too faint to register (38 and 0 pixels differed at the town), and the collapse promotes and reprojects them as the moon is. The timed barrel's explosion had burnt out by the end of the settle, so no fire or smoke was in frame, and whether a held clock freezes one is still unmeasured. The light glows, the halos, the particle flares, the lens flares, and the sun halo did not draw in any of these views either: the sun had set behind the hills at the dusk view (its glow at the horizon matched to 4 and 5 pixels), and no street lamp at night drew a glow.



## Comparing at the headset's size

Runs 306 to 311 compared the standing suite at 3584 by 3561 per eye, the Bigscreen Beyond's size, with a release payload and the compositor paced at 72 fps, through the fast comparison ([fast-comparison.md](../testing/fast-comparison.md)).

### One eye's ground, and the depth counted in millions

With the exact depth rule, single-pass differed from the isolated eyes in 4 to 6 million depth pixels per eye of 12.76 million at the town. The depth planes showed two things on top of each other. Over nearly every surface the depth was a float or two away, all in one direction in one eye; and over the whole ground in front of the camera one eye's depth was scaled by about 1.00007 (some 600 floats), the other's by a few floats, which eye depending on the view. Both were the terrain-detail draw ([`RenderBlockTerrainDetail`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainDetail)), the ground nearest the camera: its per-eye re-issue staged each eye's transform as the centre's reprojected column by column by `M_eye`, and that product rounds badly enough in its depth row to scale the depth. Nothing about the build, the eye size, the pacing, or the session was involved: the same draw covered a similar share of the image at 320 by 360 (run 297's 50,000 pixels of 115,200 is the same 40 % or so), and the counts moved between repeats by up to three times because each run's frozen camera lands slightly elsewhere (the isolated left eye's depth differed between two runs of the town at 8.0 million pixels), which changes how much ground is in view.

The collapse builds each eye's buffer as the block builds it for an eye rendered on its own: the patch translation relative to that eye's camera position times that eye's offset view-projection, both read from the eye's `cb13` slot, through the engine's own [`Matrix4::Multiply4x4`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::math::Matrix4##func-Multiply4x4). Built that way, the bottom 3.44 million pixels of the town's left eye, all terrain detail, are bit for bit the isolated eye's in depth and albedo. Same schedule, the earlier reprojection by `M_eye` (off) against the eye's own rows (on), pixels per eye (left, right):

| Scene | Depth beyond 64 floats | Depth, any change | Albedo | Normals | Final | Final floor |
|---|---|---|---|---|---|---|
| Town, off | 5,851,385 / 4,960,407 | 6,137,296 / 5,358,280 | 2,380,033 / 2,069,064 | 3,512,187 / 3,037,202 | 1,514,591 / 1,054,914 | 57,655 / 45,534 |
| Town, on | 1,176 / 2,849 | 287,133 / 1,232,918 | 3,809 / 113,005 | 30,366 / 248,749 | 833,561 / 453,942 | 57,655 / 45,534 |
| Tunnel, off | 282,036 / 5,202,503 | 6,890,433 / 7,491,138 | 1,501,086 / 2,180,315 | 3,157,922 / 4,292,976 | 9,035 / 13,294 | 81 / 70 |
| Tunnel, on | 215,308 / 174,784 | 952,147 / 1,825,512 | 186,467 / 285,703 | 639,210 / 895,853 | 2,880 / 3,204 | 81 / 70 |
| Meadow, off | 4,628,893 / 4,582,939 | 6,695,051 / 6,875,689 | 2,263,319 / 2,417,334 | 4,412,541 / 4,587,837 | 1,273,558 / 1,324,506 | 45,283 / 44,196 |
| Meadow, on | 922,799 / 969,219 | 2,807,083 / 3,055,994 | 319,671 / 339,357 | 2,179,905 / 2,235,968 | 263,359 / 235,570 | 45,283 / 44,196 |
| Cliff at dusk, off | 430,302 / 1,991,676 | 6,675,059 / 5,705,042 | 612,566 / 1,564,691 | 1,453,668 / 2,640,890 | 1,581 / 18,571 | 1 / 6 |
| Cliff at dusk, on | 426,714 / 172,285 | 5,510,332 / 4,141,148 | 611,610 / 493,788 | 1,452,191 / 1,476,127 | 1,580 / 1,658 | 1 / 6 |

The shore lake and the capital at night have no terrain detail in view and did not change. The town's final images in that run differed between modes far beyond the repeat (multipass against the isolated left eye at 448,370 pixels), which the standing suite below did not repeat; read the town's final row against multipass's.

The patch system's prepasses ([`RP_TERRAINPATCH_CLEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_CLEAR) through [`RP_TERRAINPATCH_ENUMERATION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_ENUMERATION)) were the earlier suspect. Their compute shaders rebuild the detail every frame from the vertex `GlobalConstants`' camera position (the detail pass also reads the frustum planes), so the detail is a function of the camera that frame rather than history. Staging the centre camera's position for those passes in the isolated eyes and multipass changed nothing measurable against the reprojection's millions of differing pixels; once the domain shaders read their eye's rows it was the largest remainder on the patches, and it is now on by default (see "Each eye's own rows, not the centre's reprojected").

### What a depth difference is

Two renders of one surface through transforms that agree in real arithmetic but round differently land a few floats apart: every reprojected family against the isolated eye's own matrices does. The exact rule counted those in the millions at this size and hid the displaced geometry among them, so the fast comparison now counts a depth as differing when it is more than 64 floats from the reference (about one part in 130,000, 0.08 mm at ten metres), and reports the exact count beside it as `changed`. Multipass against the isolated eyes stays at 0 to 50 beyond the tolerance and 17 to 205 changed, so the references themselves agree to the bit almost everywhere.

### The right eye's half of the target

With the terrain detail exact, the town's left-eye ground is bit for bit the isolated eye's and the right eye's is not: one float of depth at about 740,000 pixels and a code of albedo at about 94,000. Run 310 tested whether that follows the half of the target or the eye by drawing eye 0's rows into both halves (every `cb13` slot, `M_eye`, and the terrain detail's rows pinned to eye 0), so the right half held the left eye's image one eye width further right. Against the isolated left eye, pixels per eye:

| Scene | Left half, depth changed | Right half, depth changed | Left half, albedo | Right half, albedo |
|---|---|---|---|---|
| Town | 119,747 | 799,726 | 2,492 | 129,295 |
| Tunnel | 28,052 | 1,439,581 | 18,011 | 187,108 |

The right half's depth differs by one to three floats almost everywhere it differs, the same size and shape as the right eye's own difference from the isolated right eye in a normal run (1.1 to 1.7 million changed depth pixels at the town). The error follows the half, not the eye: rasterizing the same vertices one eye width to the right rounds the interpolated depth and attributes differently, and the terrain's texture coordinates, interpolated from patch-local positions hundreds of metres in size, cross a filter boundary at some pixels. No fix is expected.

### Each eye's own rows, not the centre's reprojected

Runs 309 to 311 replaced the `M_eye` reprojection with the eye's own rows wherever the shader's arithmetic allows it (see "Exact only in real arithmetic"):

- **The terrain domain shaders** (`volumetricterrain4`, `volumetricterrain4near`, and `volumetricterrain4silhouette` on the pinned-eye path, `volumetricterrain2`, `volumetricterrainpow2`, and `volumetricterrainsilhouette` on the eye-lane path) build clip as `(world − camera) · m_OffsetViewProjection` from their terrain constants, which [`RenderBlockTypeTerrainPatch::SetupConstantBuffers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##func-SetupConstantBuffers) copies from the render context's offset view-projection and camera position, the values `cb13`'s slots hold for each eye. `single_pass.terrain_eye_rows` rewrites them to choose, in a prologue, between the eye's `cb13` rows and their own by the selector row, so outside the routed range (the reflection passes that reuse them) they read their own constants, and every later read of those rows reads the choice. The distance outputs and the LOD fade move to the eye with the rows.
- **The full view-projection's readers.** The shaders that project an absolute world position through `cb0` rows 0 to 3 the eye's copy of those rows: `treeimpostor*`, `lightsource*`, `aobox`, and the non-tessellated basemesh `volumetricterrain4notessellation*` on the instance-parity path (`dxbc_stereo::remap_full_view_projection`), and its GPU-indirect permutations on the pinned-eye path (`dxbc_stereo::remap_pinned_full_view_projection`). Outside the routed range both copies mirror `cb0`, so the substitution reads the same values there.

Same schedule, at 3584 by 3561 per eye (run 310), with each change reverted to the `M_eye` reprojection against the default, depth beyond 64 floats (changed) and albedo, left / right (the switches that reverted them have since been removed with the older rewrites):

| Scene | Default | Full view-projection reprojected | Terrain domain reprojected |
|---|---|---|---|
| Meadow, depth | 617,227 (3,870,733) / 622,354 (4,261,998) | 920,365 (3,889,724) / 930,456 (4,282,992) | 620,886 (3,979,125) / 622,972 (4,353,360) |
| Meadow, albedo | 174,300 / 218,009 | 297,402 / 341,675 | 179,266 / 221,286 |
| Tunnel, depth | 1,073 (12,142) / 10,034 (1,109,614) | unchanged | 5,894 (65,034) / 12,298 (1,125,982) |
| Town, depth | 1,022 (68,148) / 1,129 (1,350,990) | 1,049 (93,440) / 1,143 (1,351,623) | 1,657 (243,305) / 1,492 (1,462,346) |

The meadow's tree impostors were most of its alpha-tested edge flips. The terrain rows matter less here than they did before the prep fix below, which removed most of what the tunnel's patches differed by.

**The patch prep, from the centre camera in every mode.** With the domain shaders exact, the tunnel's rock wall still differed per patch: zero at each patch's border, rising smoothly into its interior to a few hundred floats, with opposite signs in the two eyes. The patch system's prep passes ([`RP_TERRAINPATCH_CLEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_CLEAR) through [`RP_TERRAINPATCH_ENUMERATION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_ENUMERATION)) rebuild the patches' detail, histogram, triangles, and morph targets on the GPU every frame, and their compute shaders read the camera position row of the vertex globals, which [`RenderEngine::PreDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-PreDraw) gives the render camera's. Under the collapse that is the centre; in an isolated eye, and in each multipass dispatch, it was the eye's, so each eye morphed the ground from its own position. `stereo::terrain_prep` stages the centre camera's position for those passes in every dispatch that renders one eye at a time, which makes the shared choice the references' too and the two multipass eyes agree about the ground. Its first run took the tunnel's left eye from 254,292 (769,397) beyond 64 floats (changed) to 4,707 (27,946), and its albedo from 145,744 to 18,306 (runs 309, adjacent schedules). It costs one extra upload of the vertex globals per prep pass in those dispatches, and nothing under single-pass.

What is left reprojected has no exact form in the shader: the baked world-view-projection families (`general*`, `character*`, `building*`, `prop*`, the bark's `WorldViewProj`), the foliage, whose `OffsetWorld` translation the block rounds relative to the centred camera before the shader sees it, the moon, the water planes, and the decals. Their remainder at the shore, the cliff, and the meadow is a few floats per triangle (the depth's median absolute difference at the shore is about 6 floats), with the alpha-tested edges and the right half's rounding on top.

### The canted rigs

Under `index` and `stress` the left eye's depth differed beyond the tolerance at 15,000 to 19,000 pixels in run 310, 75,000 to 194,000 in run 311, and 45,000 to 284,000 in run 312's runs: distant terrain kilometres away at the eye's outer edge, missing outright where the sky or a farther hillside showed through. In the final image it is a hard, patch-shaped cut through the far hills of the left eye (the town view under the stress rig loses most of the mountains across the bay in the outer quarter of the eye).

It is the terrain's hull programs. Both terrain block types bake an offset view-projection into their hull and domain constants once per frame per slot ([`RenderBlockTypeTerrain::SetupConstantBuffers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrain##func-SetupConstantBuffers), [`RenderBlockTypeTerrainPatch::SetupConstantBuffers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##func-SetupConstantBuffers)), and the hull discards a patch whose control points, pushed out by the patch radius, fall outside one side of that frustum. The base terrain's hulls gate the test on the baked `FrustumCull` flag; the patch system's `volumetricterrain4`, `volumetricterrainhullclip4`, and `volumetricterrainwaterclip4` hulls test unconditionally, so clearing the type's flag (which `terrain_cull.rs` already did for issue #40) does not reach them. An eye rendered on its own, and each multipass dispatch, bakes its own frustum there, so the cull is exact. Under the collapse the bake is the centred camera's: eye 0's projection with no cant, its clip range stretched over the double-wide target. A canted left eye sees past its left edge, and the right eye's outer side lies inside the stretched clip range, which is why only the left eye lost geometry. The domain programs read each eye's own rows inside the routed range (`single_pass.terrain_eye_rows`), so the bake reaches nothing but the cull.

The measured cut agrees with that edge. Under `index` (54° outer half-angle, 5° of cant) the outermost missing pixel in run 312's dump sat at column 285 of 3,584, against 312 predicted for eye 0's uncanted edge; under `stress` (60°, 12°) at 885 against 917. The shortfall is the patch radius the hull adds, and none of the scene-cull frusta (the cull projection, the terrain patch camera, the active camera's planes) has an edge there: they are padded well past both eyes. Multipass matched the isolated eyes at 0 to 3 depth pixels in the same runs, and the right eye under a mirrored view (the town at a yaw of 90°, where the far hills lie on the right) lost nothing.

`single_pass.terrain_cull_both_eyes` (**on**, `hooks::graphics_engine::scene::terrain_cull`) substitutes, for a bake inside the G-buffer range under the collapse, an offset view-projection whose `x`, `y`, and `w` columns come from the projection bounding both eyes exactly (each eye's frustum corners taken in the head frame, plus the lateral margin for the eyes' offset; `vr::eyes_union_projection_standard`) and whose depth column is the engine's, so the near and far tests are unchanged. The per-slot cache is stamped stale whenever the substitution changes between bakes, so a reflection pass outside the range still bakes the engine's matrix. The cull only drops what the clipper would also drop, so the substitution changes no pixel an eye's own frustum covers. Same schedule, switch off (`cull-off` variant) against on, the left eye's depth beyond 64 floats, then its albedo:

| Scene | Off | On |
|---|---|---|
| Town, `index` | 45,600; 49,390 | 2,848; 6,798 |
| Town, `stress` | 145,436; 148,907 | 2,351; 5,893 |
| Town at a yaw of 90°, `stress` | 113,969; 116,984 | 2,219; 4,501 |
| Town, `beyond` | 471; 2,268 | 438; 2,231 |

What remains is the parallel rigs' level. The `beyond` and `quest` rigs have parallel eyes and lost nothing to begin with. The amount lost moved from run to run (15,000 to 284,000 pixels at the same view under the same rig) with which terrain patches exist across the cut that frame: the patch selection is rebuilt from what has streamed in at that distance, so a run whose far patches straddle the edge loses more of them. The substitution adds no submission (it changes a constant, not a draw); the benchmark (run 312, the default rig, three repeats, `terrain-cull-both-eyes-off` against single-pass) put the GPU time at 12.03 against 12.15 ms at the town and 11.84 against 11.77 ms in the tunnel, within the repeats' spread, with the submission counts differing only as the running world's streaming does (6,046 against 6,156 and 5,116 against 5,062).

### The reference modes' final images

Multipass differed from the isolated eyes' final images far beyond either mode's repeat: 20,000 to 43,000 pixels in the left eye at the town against a floor of 2,000 to 6,000, and 60,000 at the shore in one run of three. The geometry agreed to the bit, and the difference was a speckle over the sunlit ground. With the screen-space reflections left off (the harness's default; the coverage configs turn them on), multipass against the isolated eyes fell within the floor in every run: 5,754 and 3,240, 7,301 and 3,144, and 9,214 and 7,447 at the town, and 221 and 1,256 and 4,995 and 2,771 at the shore. Turning off the SSAO or the sun shadows instead left it in place. The reflection march samples the previous frame's scene, and in multipass the previous image was the other eye's, so it is multipass that is the odd one out; single-pass and the isolated eyes each read their own. The final-image rows below therefore compare single-pass with the isolated eyes; multipass's column is a reference for the references, not a floor. From run 315, with the light-propagation volume held at the frozen camera (see "What the final images differ by"), multipass matches the isolated eyes within the repeat with the reflections on (3,400 to 7,600 pixels at the town against a floor of 2,500 to 6,200), so most of what this paragraph measured was the volume following the live head between snapshots.

### What the final images differ by

With the geometry matched, the town's final images still differ from the isolated eyes at 90,000 to 135,000 pixels per eye (0.7 % to 1.1 % of the eye) against a repeat floor of a few thousand. Run 312 dumped whole eyes and binned each differing pixel by its largest channel difference. At the town with the coverage settings (screen-space reflections on), single-pass against the isolated left eye:

| | Single-pass | Multipass |
|---|---|---|
| Differing (more than 3 codes) | 89,935 | 29,198 |
| 4 to 8 codes | 70,975 (79 %) | 21,745 (74 %) |
| 9 to 16 codes | 15,407 (17 %) | 6,166 (21 %) |
| more than 16 codes | 3,553 (4 %) | 1,287 (4 %) |
| Mean of the differing pixels | 6.9 codes | 7.3 codes |
| Mean over every pixel and channel | 0.15 codes | 0.08 codes |
| Largest | 112 codes | 111 codes |

The right eye is alike (106,741 differing, 86 % within 8 codes, a mean of 0.12 codes over the eye). More than 90 % of the differing pixels lie at mid depth in the lower half of the eye: the sunlit ground between a few metres and a few hundred metres, as a speckle a few codes deep, with the sky, the far field, and the near geometry contributing a few thousand each. With the shipped settings (FSR, the foveation, and the exposure metering back) the same comparison is 33,121 and 78,530 pixels against multipass's 20,249 and 14,733, 92 % and 82 % of them within 8 codes.

Run 312's attempt to attribute the speckle by switching sources off in every mode did not hold up, because separate runs did not render one scene: the frozen camera started wherever the live camera was (10 to 20 cm and several degrees apart from run to run), and the light-propagation volume followed the live head rather than the frozen camera. Runs 313 to 316 pinned both (see "The pinned anchor" in [fast-comparison.md](../testing/fast-comparison.md)). With them multipass matches the isolated eyes within the repeat, and single-pass is steady from run to run (five runs of the town, 74,775 to 86,778 pixels against the isolated left eye). Run 315, dumped whole, left eye unless named (the right eye is alike):

| Pair | Final over 3 codes | 4 to 8 | 9 to 16 | over 16 | Mean, every pixel | Over 8 | Over 16 |
|---|---|---|---|---|---|---|---|
| Single-pass against the isolated eye | 73,936 | 56,217 | 14,116 | 3,603 | 0.073 codes | 0.139 % | 0.028 % |
| Single-pass, right eye | 77,134 | 58,726 | 14,298 | 4,110 | 0.074 codes | 0.144 % | 0.032 % |
| Multipass against the isolated eye | 4,231 | 3,167 | 886 | 178 | 0.008 codes | 0.008 % | 0.001 % |
| The isolated eye against its repeat | 5,895 | 4,546 | 1,112 | 237 | 0.007 codes | 0.011 % | 0.002 % |
| Single-pass against its repeat | 5,233 | 3,963 | 1,037 | 233 | 0.006 codes | 0.010 % | 0.002 % |

92 % of single-pass's differing pixels lie in the lower half of the eye, and of those over 16 codes only 15 % are within two pixels of a depth edge: it is the sunlit sand, not edges. What it is, from the bottom up:

- **Not the geometry.** The G-buffer (depth, albedo, and normals) is bit for bit the isolated eye's at 12,637,215 of the left eye's 12,762,624 pixels, and 98 % of the lit colour's differences (`post_resolve`) and of the final image's lie inside that identical region.
- **One step of R11G11B10.** At the lit colour, 560,000 to 670,000 pixels per eye differ, nearly all by exactly one representable value (19,000 to 43,000 by two or more), against 97,000 to 190,000 for the repeats. The scene's average over the ground moves by three parts in 100,000. A one-step change at the bright, high-contrast sand is one or two codes after the tone map.
- **The anti-aliasing turns codes into the speckle.** Before SMAA, single-pass differs from the isolated eye by more than 3 codes at 22,000 pixels and by one or two codes at 2.2 million; after it, at 108,000 (in one run). 44,000 of the final pixels over 3 codes had a bit-identical input to SMAA: its edge detection, a luma threshold over a high-frequency texture, flips where a neighbour moved by a code, and the blend it then applies moves the pixel by 4 to 8. Every comparison has this amplifier, the repeats included.
- **The screen-space ambient occlusion was most of the lit difference.** Same session, the sources left out in every mode: with the SSAO out, single-pass fell to 56,000 and 58,000 pixels and its one-step lit differences to 192,000 and 227,000; with the sun shadows out it stayed at 83,000 to 89,000; with the global illumination out at 69,000 to 83,000. Over the ground in the left eye the SSAO's share had a period of exactly 14 columns (3,584 / 256). See "The ambient occlusion's sampling, the same in every mode" below for what it was and the fix.
- **The right eye's half** carries the rest of the right eye's difference: its albedo and normals differ where the rasterization one eye width further right rounds differently (see "The right eye's half of the target").

#### The ambient occlusion's sampling, the same in every mode

Run 317 captured each draw of the SSAO block as a snapshot plane (`ssao_step0` to `ssao_step7`, at each target's own size; see [fast-comparison.md](../testing/fast-comparison.md)) and compared them with the isolated left eye at the town. The block draws six: the full-resolution linear depth, the half-resolution generation, the bilateral blur's vertical and horizontal passes, a pass that leaves the horizontal result unchanged, and the full-resolution composite into the scene target's alpha. The stripes were not one rounding but three places where the occlusion samples on or next to a texel boundary, so that which texel, or which weight, the sampler takes rests on the coordinate's last bits:

- The generation's centre depth and normal samples sit at the centre of a half-resolution pixel, which is exactly the boundary between two full-resolution texels.
- The blur steps its taps by about half a texel times a depth weight close to one, so its odd taps land within a hair of a boundary; the horizontal pass alone created about 220,000 differing pixels from an input differing at 4,000, while the vertical one created almost none (the two targets' heights are equal).
- The composite's bilinear upsample quantizes its weights.

The interpolated UV is not the same function of the pixel in a 1,792-wide target and a 3,584-wide one, and the sampler's conversion of a normalized coordinate carries one bit less of the position in a texture twice as wide, so the two disagreed at columns where the UV's rounding (7 half-resolution texels apart, 1,792 being 7 × 256) put a coordinate on the wrong side. Moving the taps into the eye's own UV (`2u − eye`, exact) changed nothing measurable on its own: the tap arithmetic was not the source. Neither did the size each pass recovered from the UV's derivative, which at these sizes is wrong by a pixel in some quads.

`single_pass.ssao_exact_sampling` (**on**) makes every mode do the same arithmetic. Under the collapse `dxbc_stereo::localize_ambient_occlusion` computes the eye's UV from the pixel position over sizes read from the depth texture, places the taps in it, and takes only the samples onto the eye's half; outside it `dxbc_stereo::snap_ambient_occlusion_uv` rewrites the same passes to compute the UV the same way. In both, the generation's centre samples move half a full-resolution texel off the boundary, the blur's samples read through loads of the texel `floor(coordinate × size)` names, and the composite blends its four texels in the shader. In the left eye every product is the eye-sized target's halved or doubled exactly, so the two agree to the bit wherever their inputs do. It changes the isolated eyes' and multipass's occlusion too, by which of two adjacent texels a sample reads and by a weight's last bits.

At the town (run 317), the left eye against the isolated eye, pixels. "Before" is the tree before this work; "single-pass off" is the switch off in single-pass alone, in the same schedule as "after", against references that compute their sampling the new way, which is the mismatch's size rather than the old state's (the step planes did not exist before):

| Plane | Before | Single-pass off | After |
|---|---|---|---|
| Generation (`ssao_step1`) | | 285,000 | 3,400 |
| Horizontal blur (`ssao_step3`) | | 652,000 | 4,100 |
| Composite (`ssao_step5`, as `ssao`) | 872,000 | 4,090,000 | 26,000 |

What remains follows the inputs: the linear depth differs where the reprojected families' depth does (a few floats, 100,000 to 200,000 pixels with the cars the traffic drove into view), and the occlusion volumes' target at 8,000 to 13,000. The right eye's composite fell from 2.7 million to 200,000, its remainder the right half's rasterization. At the meadow and the shore the G-buffer itself differs at hundreds of thousands of foliage pixels, so the occlusion does too (838,000 and 1.4 million). The final images over the same runs: the town's left eye 60,000 against 75,000 before (floor 3,200), the meadow 160,000 against 175,000, the shore 117,000 against 135,000; the town's scene had changed by then (traffic), so read those as indicative.

What is left of the lit colour's one-step differences after this (the per-eye constant computed by another route that the brief suspects) is not attributed.

The global illumination is also why every mode's repeat is not zero: the solver refreshes one of its two volumes per frame, so consecutive frames differ by a step at a few hundred thousand pixels of the lit colour (with the pass left out, the repeats fall to a few pixels). It is the same in every mode.

The ocean band at the horizon differs between any two snapshots taken at different times, in every mode, because the ocean animates on its own clock.

### The standing suite

Run 312, on the final tree, pixels per eye (left / right) against the isolated eyes, single-pass and then multipass, each with the larger of the two modes' repeat floors (final image floor in brackets). Depth counts beyond 64 floats, with the exact-bit count in parentheses:

| Scene | Depth beyond 64 (changed) | Albedo | Normals | Final [floor] | Multipass final [floor] |
|---|---|---|---|---|---|
| town | 405 (65,400) / 1,002 (1,019,445) | 2,619 / 163,252 | 22,674 / 332,544 | 93,064 [3,989] / 124,630 [3,590] | 15,556 [2,467] / 5,491 [2,379] |
| town-rotation | 1,290 (174,828) / 1,786 (666,937) | 4,046 / 52,249 | 27,799 / 136,340 | 85,612 [17,907] / 70,152 [30,763] | 101,089 [8,867] / 68,759 [30,763] |
| town-translation | 954 (136,463) / 2,405 (1,346,840) | 2,718 / 109,284 | 25,284 / 229,006 | 100,038 [26,266] / 123,696 [26,124] | 63,274 [21,743] / 15,079 [12,183] |
| shipped-static | 811 (106,224) / 2,678 (538,508) | 1,764 / 161,558 | 19,597 / 412,602 | 54,281 [6,202] / 51,148 [4,804] | 18,920 [6,202] / 12,497 [4,804] |
| shipped-rotation | 7,116 (141,201) / 960 (3,222,510) | 4,992 / 61,811 | 25,887 / 192,921 | 45,116 [6,862] / 47,870 [8,005] | 12,841 [6,862] / 112,766 [8,005] |
| town-index | 1,876 (93,928) / 2,821 (1,449,567) | 3,013 / 71,106 | 23,902 / 252,041 | 82,262 [4,715] / 72,480 [2,904] | 27,017 [4,715] / 14,800 [2,904] |
| town-quest | 1,635 (85,477) / 2,118 (925,463) | 3,410 / 130,551 | 27,664 / 335,819 | 91,985 [5,005] / 130,277 [1,568] | 29,626 [5,005] / 64,865 [2,424] |
| town-stress | 1,670 (100,758) / 4,074 (3,327,864) | 4,136 / 111,189 | 42,717 / 286,921 | 89,907 [11,544] / 247,120 [2,931] | 16,667 [9,723] / 51,799 [2,504] |
| town-beyond | 2,681 (122,265) / 1,234 (1,627,872) | 6,549 / 55,671 | 39,044 / 182,214 | 78,044 [3,425] / 81,132 [2,122] | 4,981 [4,456] / 12,408 [2,802] |
| tunnel | 2,875 (17,282) / 16,777 (650,141) | 26,998 / 89,692 | 159,055 / 404,210 | 2,580 [0] / 2,968 [1] | 21 [0] / 23 [1] |
| meadow | 559,172 (2,649,482) / 556,090 (2,918,368) | 158,434 / 191,778 | 1,494,677 / 1,545,447 | 175,446 [6,048] / 184,395 [5,753] | 6,628 [5,106] / 5,116 [3,068] |
| capital-night | 1,222 (1,147,765) / 4,086 (2,173,147) | 10,888 / 34,909 | 41,349 / 122,236 | 286 [0] / 284 [0] | 0 [0] / 0 [0] |
| shore | 566,344 (4,753,413) / 552,567 (5,205,187) | 460,229 / 514,922 | 1,796,501 / 1,974,984 | 134,551 [511] / 102,884 [365] | 7,234 [738] / 3,144 [750] |
| cliff-dusk | 146,900 (4,956,401) / 165,400 (6,787,186) | 464,147 / 613,078 | 1,064,677 / 1,213,433 | 1,044 [0] / 1,232 [0] | 40,566 [0] / 878 [0] |

Against run 311 the canted rigs' left-eye depth fell from 74,744 (`index`) and 193,616 (`stress`) to 1,876 and 1,670, the parallel rigs' level (see "The canted rigs"). The other rows are within the run-to-run movement run 311 showed. The right eye's large changed counts are the half's rounding.

## The non-`cb0` shader census

From a census of the shipped set (`m_Name` against the rewrite outcome). **The census only sees shaders an area actually loads, so this list
may miss families from biomes or weather not visited; it is a floor, not a complete set.**

**Reprojected — the scene-geometry allowlist** (`REPROJECT_NAME_PREFIXES`, matched by name prefix):
`character*`, `creature` (skinned and rigid NPCs, ~34 permutations including `characterskin*`,
`characterdepth*`, `characteroutline`, `charactersphdecal*`, `charactervelocity*`); `general*`
(`general`, `generalglint`, `generalmaskedjc3`, `generaloutline`, `generalprez`, `generalprezvelocity`,
`generalrsm`, `generalshadow` — the static and dynamic models); `prop`, `propdecal`; `buildingjc3`,
`buildingrsm`; `window`; `materialtune`; `open`; `flag`, `flagdepthonly`; `snow`; `skidmarks`,
`skidmarks_normal`; roads `junctionroad*`, `splineroad*`, `dirtroad`. Plus `treeimpostor*` under
the tree impostors, whose `PixelSize` is restaged per eye, and `lightsource`, `lightsource_fakelight`
for the static lights (see "The static light sprites").

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
  `lensflare`, `sunbeam`, `lightglow`, `lightningshader`,
  `bavariumshield`, `spotlightcone`, `pointlightreflection`, `alphamask`. `aobox` was here until it
  took the reprojection under `single_pass.ao_volumes_per_eye` (see "The ambient-occlusion volumes,
  per eye").
- Water: `nvwater*`, `water*` (`waterboxclear`, `waterbumpcomposite`, `waterdisplacementoverride`,
  `waterfoamsub`, `watergodraysshader`, `watermask`, `waterpaintfoam`, `watersurface`, `waterwake`),
  `waves`, `mirror`.
- Decals (held back — surface-projected, want a look first): `decal`, `decaldeformable`, `decalsimple`,
  `decalskinned`, `decalskinnedgeneralmkiii`, `decalskinnedgeneralmkiiidestructible`, `ssdecal`,
  `ssdefault`.
- Occluders, probes, prez, shadow-only: `rainoccluder`, `rainoccluderblur`, `sphericalharmonicprobe`,
  `renderprez*`, `renderpreznotex`, `rendershadow`, `layeredrsm`, `depthwrite`, `tex0shadow`,
  `tex0shadownoalpha`.
