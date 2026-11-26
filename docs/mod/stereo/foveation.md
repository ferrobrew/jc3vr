# Static foveated rendering (issue #29)

The mod renders the scene twice per frame (once per eye) at full shading rate everywhere, including
the periphery where the lenses and human vision barely resolve detail. Static (fixed-centre) foveation
reduces the peripheral shading cost. This is the design; the survey of alternatives (compositor FFR,
NVAPI VRS, quad views) lives in issue #29 — the short version is that the only **cross-vendor, D3D11**
path is Valve-style **radial density masking** via the stencil buffer, so that is what this documents.

## The idea

Before the scene render, write a radial mask into the main depth-stencil's **stencil** bits: peripheral
pixels get a mask bit set. Then, for the expensive scene passes, enable a stencil **test** that discards
the masked (peripheral) pixels before the pixel shader runs. After the scene render, a cheap fill-in
pass reconstructs the discarded pixels from their neighbours. Works on any GPU and any API that has a
stencil buffer — JC3's main depth surface is `D32FS8`, so the 8-bit stencil is there.

## Why it is feasible in JC3 (the RE)

### One central depth-stencil seam

JC3 does not build a fresh `ID3D11DepthStencilState` per pass or scatter `OMSetDepthStencilState` across render blocks. It uses a deferred, dirty-tracked, cached model:

- Render blocks mutate a bit-packed 64-bit DS-state index and a stencil ref on the context through granular setters: [`SetStencilTestEnable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-SetStencilTestEnable), [`SetStencilFunc`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-SetStencilFunc), [`SetStencilOp`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-SetStencilOp), [`SetStencilMask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-SetStencilMask), `SetDepthFunc`, and others. These only write context fields and make no D3D call.
- The actual bind happens in exactly one place: [`SetupRenderStates`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-SetupRenderStates), the pre-draw state flush at the top of every `Draw*` wrapper. It reads the packed index and looks it up in a per-context `map<u64, ID3D11DepthStencilState*>`, creating the state lazily through [`CreateDepthStencilState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-CreateDepthStencilState) on first use. It then issues the sole `OMSetDepthStencilState` (device-context vtable +0x120 / index 36) with the current stencil ref. It re-binds only when the index or ref changed.

The context fields involved are the wrapped immediate context ([`Context::m_Context`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Context##field-m_Context)), the packed DS-state index staged by the setters and flushed by `SetupRenderStates` ([`HContext_t::m_DepthStencilStateIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::HContext_t##field-m_DepthStencilStateIndex)), and the stencil ref bound alongside the DS state ([`HContext_t::m_StencilRef`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::HContext_t##field-m_StencilRef)). The doc comment on `HContext_t::m_DepthStencilStateIndex` gives the bit layout of the packed index, as decoded by `CreateDepthStencilState`.

So a hook on `SetupRenderStates` can OR a stencil test into the packed index just-in-time, and the lazy cache self-manages the extra state object. This is the force-test seam.

### The engine already uses stencil — the override must be selective

Stencil test and write are baked into the shipped per-pass render settings, which [`RenderPass::PreDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-PreDraw) stages onto the context: [`RenderPass::m_StencilTestEnable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilTestEnable), [`RenderPass::m_StencilFunc`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilFunc), [`RenderPass::m_StencilMask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilMask) (the read mask), [`RenderPass::m_StencilWriteMask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilWriteMask), [`RenderPass::m_StencilRef`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilRef), and the three operations [`RenderPass::m_StencilOpZFail`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilOpZFail), [`RenderPass::m_StencilOpZPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilOpZPass), and [`RenderPass::m_StencilOpSFail`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_StencilOpSFail). Observed engine usage on the main surface:

- The SMAA edge mask, whose bits are [`edge_stencil_bit_mask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::edge_stencil_bit_mask).
- Character skin and SSS, on a stencil bit that is not yet confirmed.
- The outline-effect blur, on another stencil bit that is not yet confirmed.
- Data-driven decal and road masks ([`RenderPassId::RP_ROAD_STENCIL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_ROAD_STENCIL)), plus [`RenderPassId::RP_CLEAR_STENCIL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_CLEAR_STENCIL).

No engine code path found references the top stencil bit, so it is the mask-bit candidate. This is not yet confirmed, and the shipped per-pass write masks (`RenderPass::m_StencilWriteMask`) could not be audited offline because the repo has no Gibbed tooling. The mask bit is therefore a config value (default `0x80`). A collision shows as corruption in one effect and is a one-line change. The override must skip passes that already enabled stencil, which it detects through the staged `RenderPass::m_StencilTestEnable`. D3D11 has one stencil test per draw, so a second test cannot be stacked on SSS, outline, SMAA, or road passes; those passes are cheap and render unmasked.

### Clear timing and the mask-write injection

Depth-stencil clears are per-pass and data-driven through [`Clear`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-Clear), whose flags are [`ClearFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw::ClearFlags) (`CLEAR_DEPTHBUFFER`, `CLEAR_STENCILBUFFER`). The main depth and stencil are cleared early in the GBuffer range, around [`RenderPassId::RP_Z_OCCLUDERS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_Z_OCCLUDERS). The mask must be written after that clear. The existing `DrawRenderPassRange` hook (`crates/jc3vr-render/src/hooks/graphics_engine/render_pass.rs`) already splits pass ranges, so it hosts the insertion. After `RP_Z_OCCLUDERS`, it draws a full-screen radial-mask pass and then continues the range. The mask pass binds the MainDepth DSV with no colour target, and its DS state is `StencilEnable`, func `ALWAYS`, `StencilOp REPLACE`, write mask and ref equal to the mask bit, and `DepthEnable=0`/`DepthWriteMask=0`. Its pixel shader discards foveal-centre pixels, so only the periphery writes the bit.

## Implementation

The design above is implemented, on by default, across:

- pyxis defs: [`SetupRenderStates`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-SetupRenderStates), [`CreateDepthStencilState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-CreateDepthStencilState), and the [`HContext_t::m_DepthStencilStateIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::HContext_t##field-m_DepthStencilStateIndex) and [`HContext_t::m_StencilRef`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::HContext_t##field-m_StencilRef) fields with the full packed-index bit layout.
- **shaders** — `crates/jc3vr-render/shaders/foveation_mask_ps.hlsl` (radial dithered mask, interleaved-gradient
  noise, decided per drop cell) and `foveation_fill_ps.hlsl` (neighbour-average reconstruction), compiled
  via `shadergen`. Both take the same constant buffer (the eyes' centres, the radii, the drop, the seam
  column, and the cell size); the two must keep their `ign` and drop decision identical so the fill
  reproduces the mask's choice.
- **`crates/jc3vr-render/src/vr/foveation.rs`** — the D3D pipeline (mask-write and fill-in full-screen passes on the
  immediate context under `Context::m_Mutex`, the `blit.rs` pattern), the `SetupRenderStates` force-test
  index rewrite (`apply_force_test`), and the `packed_stencil_test` bit math.
- **`crates/jc3vr-render/src/hooks/graphics_engine/render_pass.rs`** — the `SetupRenderStates` detour, and the
  `DrawRenderPassRange` bracketing that runs the mask-write before the first foveated pass, forces the
  stencil test through each run of foveated passes (`FoveatedPasses` in `foveation.rs`), clears the mask
  bit after the last, and runs the fill-in (see "Which passes skip the dropped pixels"). The foveal centre comes from the eye's
  `projection_standard` principal point (`foveal_center_uv`), so canted/off-axis eyes fovea about their
  true optical centre, not screen centre.
- **config + UI** — `foveation.{enabled, inner_fraction, outer_fraction, max_drop, mask_bit,
  debug_show_mask}` (`config.rs`); the radii, the drop, and the mask view are in the debug UI's
  Foveation feature, and `enabled` (default **true**) is the pause menu's FOVEATED RENDERING. Four
  switches, all on, sit with the single-pass switches so the harness can compare them within one
  schedule, but gate multipass as well: `single_pass.foveation_gbuffer`, `foveation_lighting`,
  `foveation_quads`, and `foveation_clear_mask` (the debug UI's single-pass page).

## Which passes skip the dropped pixels

The forced test covers a set of render passes (`FoveatedPasses` in `crates/jc3vr-render/src/vr/foveation.rs`), and a pass belongs in it only if dropping its pixels leaves every kept pixel as it was. Two things decide that: whether the pass writes anything a later pass reads at a neighbouring pixel before the fill-in runs, and whether it writes depth the prepasses have not already written. A dropped pixel keeps whatever depth it had when the forced test began, and the SSAO, the depth downsample, the fog volumes, the water's depth test, and FSR all read depth at and around it.

- Always: the models, the vegetation, and the creatures ([`RenderPassId::RP_MODELS_DYNAMIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_DYNAMIC) through [`RenderPassId::RP_CREATURES`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_CREATURES)), as before.
- `single_pass.foveation_gbuffer`: the terrain's apply passes ([`RenderPassId::RP_TERRAIN_APPLY_NEAR_DETAILED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAIN_APPLY_NEAR_DETAILED) through `RP_TERRAIN_APPLY_FAR`), the road junctions and road layers (`RP_ROAD_JUNCTION` through `RP_ROAD_JUNCTION_OPAQUE`), and the decals (`RP_DECALS` through `RP_SCREEN_SPACE_ROAD_DECALS`). Per draw, a draw that already uses stencil is left alone, as before.
- `single_pass.foveation_lighting`: the deferred lights ([`RenderPassId::RP_DEFERRED_LIGHTS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_DEFERRED_LIGHTS)), the light billboards (`RP_BILLBOARD`), the stars, sun, moon, skybox, and sky gradient (`RP_STARS` through `RP_SKY_GRADIENT`), and the fog gradient (`RP_FOG_GRADIENT`). Each writes the lit colour at its own pixel from that pixel's inputs. The fill-in then runs right after the fog gradient, under a stencil test that passes only the dropped pixels, instead of at the start of the post effects. That move is what makes the lighting safe to drop: from the scene-colour copy the water and the refraction read onward, every pass sees a reconstructed neighbourhood, and the passes after the fill-in (the water, the clouds, the fog volumes, the particles, and the transparents) shade the dropped pixels as they shade every other. The occlusion queries ([`RenderPassId::RP_OCCLUSION_QUERY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_OCCLUSION_QUERY)) between them stay out, since a dropped pixel would stop counting towards a query.

Left out, and why, measured at the town by masking each group alone and comparing the G-buffer's depth and the final image against the variant without it (run 333):

- The depth prepasses: they give the dropped pixels their depth.
- The terrain's detail passes (`RP_TERRAINPATCH_DETAIL_MID` and `_LOW`, the largest terrain cost) write depth the prepasses do not: masked, 2.9 and 2.0 million dropped pixels per eye lost their depth and 0.8 and 0.6 million kept pixels changed in the final image (the repeat floor is about 3,000). The base-mesh passes (`RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR` through `RP_TERRAINPATCH_BASEMESH_COLOR`) do so over a smaller area: 136,000 dropped pixels' depth and 12,600 kept pixels. The apply passes, the roads, and the decals changed neither.
- Adding the test to draws that write stencil without testing it (keeping their writes) changed 190,000 and 700,000 kept pixels, so the draws that use stencil stay unmasked.
- The SSAO and its blur, the reflections, the global illumination, the subsurface skin, and the depth downsample read neighbours; the post effects and FSR run after the fill-in anyway.

Making the detail passes skippable would take their depth written by a prepass (the engine's own, or a mod-side depth-only draw of the same patches), which would cost part of what masking them saves; it is not done.

### Dropping whole quads

A pixel shader runs per 2 by 2 pixel quad: a quad with one live pixel runs all four lanes, the dropped ones as helpers. With the drop decided per pixel by the dither, only 15% of the quads in an eye lose all four pixels at the shipped radii, against 54% of the pixels dropped, so the stencil test saved little shading: the deferred lights went from 0.64 to 0.57 ms. `single_pass.foveation_quads` decides the drop per 2 by 2 cell aligned to the quads (both shaders take the cell size), so a dropped cell runs nothing, and the same lights take 0.36 ms. The fill-in then reconstructs a dropped cell from the nearest pixel of each kept cell around it, weighted by inverse squared distance (8 neighbouring cells rather than 8 neighbouring pixels); in the far periphery the image is visibly blockier than the per-pixel dither's at 1:1, at edges especially (a 2-pixel stair on a pole against the sky).

### What it costs and what it changes

At the town the foveation now saves about 0.1 ms of GPU time per frame against none, and 0.33 ms against the models-only, per-pixel version; see "The foveation's net value" in [frame-budget.md](../performance/frame-budget.md) for the per-pass figures.

Kept pixels: single-pass with the shipped settings' foveation, the new passes on against off (`foveation_gbuffer` and `foveation_lighting`, quads in both so the two drop the same pixels), compared inside the kept set read from the stencil's mask bit (run 333, saved planes). The scene colour at the start of the post chain differed by more than 2% at 7,400 and 14,800 kept pixels per eye at the town against 10,200 and 15,100 for the repeat, at 45 and 33 at the capital at night, at 116 and 118 in the tunnel (14 for the repeat), and at 3,400 in the meadow against 52,000 for its repeat (its trees move between snapshots). The final image differs at more kept pixels than the scene colour (11,300 and 6,700 at the town against 4,300 and 3,700): the dropped pixels now hold a different reconstruction, and the post effects that read neighbours (the glare, the engine's anti-aliasing) carry that into kept pixels next to them. The G-buffer's depth at the dropped pixels is unchanged.

The reconstructed periphery: with the lighting passes dropped and the fill-in moved before the water, the clouds, and the transparents, those now render at the dropped pixels instead of being averaged from neighbours, so the sea at the horizon, the clouds' edges, and thin wires are sharper there than before. The quads make the far periphery blockier at 1:1: textured ground shows a 2-pixel grain where the per-pixel dither gave a finer one, and a dark pole against the sky gets a 2-pixel stair on its edge. A few light specks showed on the occupants of a car in the far periphery, likely dropped cells with no kept cell around them, which keep their unshaded value. None of this was viewed in the headset.

## Under single-pass stereo

The single-pass collapse renders both eyes into one double-wide target in one dispatch, so the mask and the fill-in run once. Laid out as for one eye, they put a single fovea on the seam with a radius sized for the double-wide target: each eye's centre of view lands in the drop ramp and its inner edge stays at full rate. `single_pass.foveation_per_eye` (on) hands both passes both eyes' centres, each as a UV of its own half, an eye-sized half-diagonal, and the first column of eye 1's half. Each pixel takes its eye from the half it lies in and measures its radius, its dither, and its fill-in neighbourhood in pixels from that half's left edge; a neighbour across the seam reads as zero, as one past the edge of an eye-sized target does. Each pass stays one draw. See "The shipped settings under single-pass" in [`single-pass-stereo.md`](single-pass-stereo.md) for the measurement.

## The fill-in draw's pipeline state

The fill-in draw runs on the immediate context with the pipeline state it binds itself, so
`bind_fullscreen` binds every stage it depends on, an explicit opaque blend included (the state
backup restores the engine's blend afterwards). A stage it left unbound would inherit whatever the
engine's last scene block left on the context: an inherited additive or alpha blend makes the fill
add its neighbour-averaged reconstruction over the entire already-shaded frame (about twice the
global brightness), latched for as long as the scene keeps producing the same leftover state and
flipping with head pose. The rule generalizes: a raw pass on the engine's context must bind *every*
state stage it depends on, not just the ones it obviously uses.

## Open items (needs in-headset validation)

The feature ships on by default and touches the hottest render path, so it can be disabled (config
or the pause menu) if it perturbs concurrent validation work. What still needs confirming in a headset:

- The foveated passes are chosen by the measurements in "Which passes skip the dropped pixels"; how
  the quads' blockier periphery and the reconstruction look through the lenses is not yet judged.
- **`DrawRenderPassRange` call structure.** The bracketing assumes the scene range arrives as one call (or
  a few sequential sub-ranges); the mask/fill are gated to run once each even if split. If the engine
  interleaves the range differently, the injection points need revisiting.
- **Engine depth-stencil state cache.** The force-test relies on `SetupRenderStates` re-flushing because
  the rewritten index differs from the last-flushed one. Confirm the first foveated draw actually rebinds
  (so the mask-write's transient DS state does not leak into a scene pass), and that the first post-range
  draw restores the normal state.
- **Back-face stencil.** The packed index is assumed to drive both faces; if `CreateDepthStencilState`
  leaves the back face `ALWAYS`, back-facing peripheral geometry would still shade. Verify against a scene
  with visible back faces.
- **Stencil-bit audit.** Confirm no shipped pass writes the mask bit (`0x80`); mitigated by the bit being
  configurable.
- **Quality tuning** — the density ramp, radii, and fill neighbourhood need validation under lens
  distortion; not reproducible offline.

## Future: dynamic (eye-tracked) foveation

If the runtime exposes `XR_EXT_eye_gaze_interaction`, the mask centre recentres on gaze each frame — the
same masking machinery, only the `foveal_centre_uv` cbuffer moves. Out of scope until the static path is
proven.
