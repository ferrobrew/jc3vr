# Single-pass stereo, per render block

The general model path of single-pass stereo ([single-pass-stereo.md](single-pass-stereo.md)) handles most draws with one instance-doubled submission. The render blocks that do not fit it each need an intercept, and some draws are still submitted once per eye. This page classifies the blocks, records the intercept for each, and lists what is drawn twice.

## The buckets

Every scene draw falls into one of these:

- (a) `cb0` remap and instance doubling: the vertex shader takes clip from the global view-projection, and the draw's instance count is the CPU's.
- (b) Reprojecting a baked matrix: the vertex shader reads a CPU-baked world-view-projection from a per-draw constant buffer. The block is drawn once with reprojected shader variants, or re-issued once per eye with the baked buffer multiplied by `M_eye`. Bark, foliage, and the occluders are here; the terrain detail re-issues the same way but builds each eye's matrix from that eye's own `cb13` rows, because the reprojection rounds its depth badly.
- (c) Tessellation with the eye injected: the domain shader builds clip and reads its eye's rows from `cb13` ("The terrain tessellation path" in [single-pass-stereo.md](single-pass-stereo.md)).
- (d) GPU-indirect per-eye re-issue: the instance count is in a GPU buffer, so the draw is re-issued per eye with the eye's half bound and the transform overridden, either `cb13` pinned to one eye or a reprojected baked buffer.
- (e) Screen-space and NDC draws, which run once and are not stereo.

The bucket-(b) vertex shaders all consume their baked registers as a multiply-add chain (`clip = Σ_i p_i · cb[k+i]`), so each register is a column and the per-eye buffer is `cb[k+i]_eye = M_eye · cb[k+i]_mono`, one register at a time. `cb13`'s own `M_eye` block uses rows, because the rewriter's epilogue is a `dp4` chain.

## The shared re-issue

A bucket-(b) block bakes its buffer inside its own `Draw`, so rather than replicate each bake, the whole `Draw` is called once per eye with the eye's half bound, and for the duration of each call the `SetVertexProgramConstants` detour is armed to reproject the block's own upload by `M_eye` (`per_eye_reissue`). The arm is keyed to the block's graphics context, since the detour sees every upload in the process, and it reports whether it fired: a block that took a path staging no such matrix logs rather than silently rendering both eyes from the centre (`REPROJECT_FIRED`). The re-issue raises the `PER_EYE_REISSUE` marker so the draw and viewport detours leave the block's own calls alone.

## Terrain patches

The world's terrain is [`RenderBlockTerrainPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch), driven by [`RenderBlockTypeTerrainPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch). [`RenderBlockTerrainPatch::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch##func-Draw) routes by the active pass: the near base-mesh passes are GPU-indirect, the far and colour passes are `DrawIndexed` over parts of the patch range, and the reflection and shadow prepasses draw the full range. There is no block intercept. `single_pass.indirect_per_eye` re-issues the indirect draws per eye, the far tessellated draws, whose vertex shaders decline the eye injection, take the unpatched `DrawIndexed` re-issue, and `single_pass.terrain_patch_per_eye` supplies the parallax through the pinned-eye rewrite. Without the indirect re-issue the near patches inherit the full double-wide viewport and slide across the world.

## Vegetation

Tree trunks are [`RenderBlockBark`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockBark) and grass is `RenderBlockFoliage`. [`RenderBlockBark::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockBark##func-Draw) and `DrawZ` stage a baked offset world-view-projection at vertex `cb1` before any draw-kind routing, and [`RenderBlockFoliage::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockFoliage##func-Draw) and [`DrawZ`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockFoliage##func-DrawZ) stage a world matrix and a copy of the offset view-projection at vertex `cb2`; the register layouts are on those defs. Each block picks one of three draw kinds from its instance data (plain, hardware-instanced, or GPU-indirect, the last being the main grass path), and all three read the same baked registers, so one reprojection covers every kind. Both entry points of foliage must be corrected the same way: a centred depth prepass rejects the per-eye colour.

### The vegetation drawn once

`single_pass.vegetation_once` (`crates/jc3vr-render/src/stereo/single_pass/vegetation_once/`) draws a bark or foliage block once for both eyes. The per-block setup ([`RenderBlockBark::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockBark##func-Setup) and its foliage and `SetupZ` counterparts) binds the vertex shader by draw kind before `Draw` runs, so the shader bound when `Draw` begins is the one its draws use, except on the billboard path ([`RENDERSTATUS_BILLBOARD_ARRAY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderStatus##flag-RENDERSTATUS_BILLBOARD_ARRAY)), which binds its own.

- Each eye-pass permutation (colour, `prez`, and velocity of each draw kind) gets a variant built at creation (`vegetation_once/variants.rs`) and attached to the engine's shader as private data: `dxbc_stereo::reproject_vertex_shader` for the plain and hardware-instanced ones, and `reproject_instanced_vertex_shader` for the GPU-indirect ones, which reuses the shader's own `SV_InstanceID` for the eye and renames its one other read to `SV_InstanceID >> 1`. The engine's shaders stay pristine for the shadow passes.
- `Draw` runs once in a scope the draw detours consult. A plain draw becomes a two-instance draw with the viewport split. A GPU-indirect draw reads a copy of the vegetation update's argument buffer with every instance count doubled (`vegetation_once/indirect_args.rs`, by the compute shader `crates/jc3vr-render/shaders/indirect_args_double_cs.hlsl`, once per routed pass). A hardware-instanced draw takes a layout twin if one exists and otherwise the `instanced_per_eye` re-issue with the variant bound.
- The whole-block re-issue is the fallback when the switch is off, outside the eye split, on the billboard path, for a shader with no variant, or once the argument copy has failed.

The wind, fades, LOD, and the foliage's wind-noise origin stay the centred camera's single computation, as under the re-issue; the pixel shaders take their eye from the pixel position ([single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md)), which also rebuilds the velocity the variants leave centred.

### Foliage lighting

The visible grass is deferred: the four-target colour permutations write the G-buffer inside the G-buffer range and the deferred resolve lights them. Only the one-target blend permutations are forward-lit, and their clustered-light section is purely additive, so a wrong grid removes local lights but cannot darken the rest. The per-eye render width is rounded so the double-wide seam falls on a light-grid tile boundary (`EYE_WIDTH_ALIGNMENT` in `vr::eye_resolution`). In the deferred permutation the G-buffer albedo carries a per-instance colour and fade (`TEXCOORD4`), so foliage that is both dark and speckled points at that attribute reading wrong.

## The shaders claimed on `cb0[4]` alone

The remap patches any vertex shader that references `cb0[4]` or `cb0[29..32]`. `cb0[29..32]` can only be a clip transform; `cb0[4]` is the camera's world position, the translation of [`RenderContext::m_CameraTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_CameraTransform), read for many reasons. Fifty of the bundle's vertex shaders reference only `cb0[4]`, pinned and classified by the corpus tests in `crates/dxbc-stereo/tests/corpus_patch.rs`. A claimed shader is doubled and routed but drawn from the centre, at a rigid half-IPD offset from the ground in each eye's image, an error that grows as the camera approaches (a wrong optical centre displaces by `offset / distance`, a wrong projection by a fixed angle). Declining the claim alone does not help, because an unpatched instanced draw is then forwarded once at whatever viewport is bound. By where they take clip from:

- A baked matrix in their own constant buffer. The vegetation (`vegetationfoliage*` uses `cb0[4]` for a wind-noise lookup, `vegetationbark*instanced` to make the instance camera-relative) and `nvwater` are declined at creation and owned end to end by their block intercepts (`baked_cb_block_owns_vs`, `BAKED_CB_VS_NAME_PREFIXES`; the prefix match is confirmed against the bytecode, and `nvwater` is gated on `single_pass.nvwater_per_eye`, since the decline and its intercept must go together). The owned families skip the intercept's bound-shader check (`BoundVsGate::Owned`), which would otherwise read the previous draw's shader. `generaljc3`, `landmark`, `layered`, and `layeredblend` share one body and take the reprojection.
- The full view-projection in `cb0[0..3]`: the legacy water surfaces, the clouds (reprojected under `single_pass.clouds_per_eye`), and `weather`. The water is left claimed (see "The water box surface" in [single-pass-collapse-defects.md](single-pass-collapse-defects.md)).
- Direct NDC writes: `lpvinit` and `lpvinitbilinear`, where the claim is inert.
- No clip position: the tessellation control-point shaders, which the reprojection refuses.

## Tree impostors

Far tree cards are `TreeImpostorRB`, one non-instanced `DrawIndexed` per [`TreeImpostorRB::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::TreeImpostorRB##func-Draw), with the vertex shader building each card from `SV_VertexID` and a texture buffer and projecting through the full view-projection. It reads the eye's full view-projection rows and is instance-doubled. The card faces the centre camera, a negligible angle at impostor range. [`TreeImpostorRBType::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::TreeImpostorRBType##func-Setup) stages a pixel size from the display width, from which the shaders derive each card's footprint and atlas derivatives, so the collapse halves the width for the call (`stereo::single_pass::EyeDisplayWidth`); without it every card's alpha-tested silhouette moved.

## Occluders

`RenderBlockOccluder` primes the main depth for early-Z in `RP_Z_OCCLUDERS` through [`RenderBlockOccluder::DrawZ`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockOccluder##func-DrawZ), baking an offset world-view-projection into vertex `cb1` per box. Nothing reads that depth back on the CPU, so drawing it per eye is safe, and necessary, since a mono prime gives the other eye wrong early-Z. `DrawZ` takes the shared re-issue. The instanced path (`gfx.occluders.use_instancing`) bakes no matrix; nothing in the mod sets the cvar, and the intercept's fired check would log it.

## The hardware-instanced buildings, once

The buildings' hardware-instanced draws take per-instance data from vertex-buffer elements. Doubling every per-instance element's `InstanceDataStepRate` along with the count keeps instances `2k` and `2k+1` on element `k` (D3D11 fetches at `StartInstanceLocation + SV_InstanceID / InstanceDataStepRate`, confirmed against the DXVK Proton ships), so one draw through a twin of the input layout serves both eyes.

D3D11 cannot read a layout back, and the engine builds the elements on the stack in [`RenderBlockTypeGeneralMkIII::Create`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMkIII##func-Create) before the payload is injected. So the payload (`single_pass::hw_instanced`) runs that `Create` again on a scratch copy of the live type object, with a thread-local flag that makes the detours in `jc3vr_render::hooks::graphics_engine::declaration_replay` turn every other effect into a no-op and call [`CreateVertexDeclarationInstanced`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine##func-CreateVertexDeclarationInstanced) with the step rates doubled. Pairing the copy's declaration fields with the live object's maps each live layout to its twin; an inconsistent pairing is refused and its twins destroyed. The replay runs at the start of a dispatch once single-pass is active and again if the live declarations change, and the twins are destroyed through [`DestroyVertexDeclaration`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine##func-DestroyVertexDeclaration) on eject after the draw detours are removed. An eject and re-injection in one game process has not been exercised.

At an already-instanced routed draw with a patched shader, `single_pass.hw_instanced_once` binds the twin, splits the viewport, submits once with the count doubled, and rebinds the engine's layout. It declines without a recorded full viewport, with a domain or geometry shader bound, or on overflow, and a layout with no twin falls to `instanced_per_eye`.

## Patched non-indexed geometry, once

A patched shader on a geometry pass of the non-indexed entry point picks its eye from the instance parity, so one two-instance `DrawInstanced` with the viewport split replaces the per-eye re-issue, as for the promoted draws in [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md). It is left to the re-issue when a domain or geometry shader is bound, because only the last stage before the rasteriser routes: the tessellated particles' unpatched domain shader would put both instances in the left eye.

## What is still drawn twice

- The vegetation's hardware-instanced draws: the bark and foliage types' `Create`s do more than the buildings' (registering callbacks and console variables), so the replay does not cover them.
- The screen-space decals, the WaveWorks water, and the terrain detail, re-issued as blocks.
- The terrain patch system's tessellated and GPU-indirect passes, pinned-eye re-issues.
- The tessellated particles (`particleeffecttess*`).
- The fullscreen passes split per eye (the subsurface skin, the light assignment and clustered resolve, the atmospheric scattering, and the fog volume's blur), a second masked run each.

Nothing in the prepasses is issued twice: under the collapse `PreDraw` runs once, and the per-dispatch passes multipass runs twice (the vegetation interaction volumes and the rain occluder) run once. The census scenes measured about a third fewer routed-range submissions than multipass ([single-pass-validation.md](single-pass-validation.md) has the census).
