# Static foveated rendering

The periphery of each eye, where the lenses and human vision resolve little, is shaded at a reduced rate. On Direct3D 11 the only cross-vendor way is Valve-style radial density masking through the stencil buffer (issue #29 surveys the alternatives). Before the scene passes a fullscreen pass writes a radial, dithered mask into a stencil bit of the main depth-stencil surface; the expensive passes run with a stencil test that rejects the masked pixels before their pixel shader; and a fill-in pass reconstructs the dropped pixels from their kept neighbours.

`foveation.enabled` (on) is the pause menu's `FOVEATED RENDERING`, and `foveation.debug_show_mask` paints the dropped pixels magenta from the debug UI. The rings are the view tuning's `foveation.inner_fraction`, `outer_fraction`, and `max_drop` ([tuning.md](../tuning.md)).

## The engine's depth-stencil path

Render blocks stage a packed depth-stencil state index on the context through setters such as [`SetStencilTestEnable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-SetStencilTestEnable) that make no Direct3D call, and [`SetupRenderStates`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-SetupRenderStates), the flush at the top of every draw wrapper, looks the index ([`HContext_t::m_DepthStencilStateIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::HContext_t##field-m_DepthStencilStateIndex), whose doc comment has the bit layout) up in a per-context cache, creates the state lazily, and issues the only `OMSetDepthStencilState`, rebinding when the index or reference changed. A detour on `SetupRenderStates` can therefore rewrite the staged index just before the flush, and the engine's cache keeps the extra state object.

The engine uses stencil itself on the main surface, staged per pass by [`RenderPass::PreDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-PreDraw): the SMAA edge mask, skin and subsurface scattering, the outline blur, and the road and decal masks. No engine path found references the top bit, so it is the mask bit (`MASK_BIT`); the per-pass write masks have not been audited, so a collision is possible and would show as corruption in one effect. D3D11 has one stencil test per draw, so draws that already enable stencil render unmasked. The main depth and stencil are cleared early in the G-buffer range, so the mask is written after that clear.

## Implementation

- `crates/jc3vr-view/shaders/foveation_mask_ps.hlsl` writes the mask with interleaved-gradient noise decided per drop cell, and `foveation_fill_ps.hlsl` reconstructs. They share a constant buffer and must keep their noise and drop decision identical, so the fill reproduces the mask's choice.
- `crates/jc3vr-view/src/vr/foveation.rs` holds the two passes (on the immediate context under `Context::m_Mutex`), the index rewrite (`apply_force_test`, `packed_stencil_test`), and the pass set (`FoveatedPasses`). The foveal centre is each eye's projection's principal point (`foveal_center_uv`), so off-axis eyes fovea about their optical centre.
- `crates/jc3vr-render/src/hooks/graphics_engine/render_pass.rs` holds the `SetupRenderStates` detour and the bracketing in `run_scene_range`: the mask before the first foveated pass (once, even when the scene range is split across calls), the forced test through each run of foveated passes, and the fill-in before `FoveatedPasses::fill_before` or at the start of the post effects.
- The fill-in binds every pipeline stage it depends on, including an explicit opaque blend, since a stage left unbound inherits the last scene block's (an inherited additive blend doubles the frame's brightness); the state backup restores the engine's afterwards.

## Which passes skip the dropped pixels

A pass belongs in `FoveatedPasses` only if dropping its pixels leaves every kept pixel unchanged: it must not write anything a later pass reads at a neighbouring pixel before the fill-in, nor write depth the prepasses have not, since a dropped pixel keeps the depth it had when the test began and the SSAO, depth downsample, fog, water, and FSR read depth around it.

- Always: the models, the vegetation, and the creatures (`RP_MODELS_DYNAMIC` through `RP_CREATURES`).
- `single_pass.foveation_gbuffer`: the terrain's apply passes, the road junctions and layers, and the decals.
- `single_pass.foveation_lighting`: the deferred lights, the light billboards, the sky passes, and the fog gradient, each of which writes the lit colour from its own pixel's inputs. The fill-in then runs right after the fog gradient, under a stencil test passing only dropped pixels, so every later pass (the scene-colour copy, water, clouds, fog volumes, particles, and transparents) sees a reconstructed neighbourhood and shades dropped pixels normally. The occlusion queries between them stay out, since a dropped pixel would stop counting.

Left out: the depth prepasses; the terrain's detail and base-mesh passes, which write depth the prepasses do not (masked, they changed hundreds of thousands of kept pixels); draws that write stencil without testing it; and the passes that read neighbours (SSAO, reflections, global illumination, subsurface skin, and depth downsample).

All three switches are on and sit in the single-pass switch table so the harness can compare them within one schedule, though they gate multipass too.

### Dropping whole quads

A pixel shader runs per 2x2 quad, so with a per-pixel drop few quads lose all four pixels and the test saves little. `single_pass.foveation_quads` decides the drop per quad-aligned 2x2 cell, so a dropped cell runs nothing, and the fill-in reconstructs it from the nearest pixel of each of the eight neighbouring kept cells, weighted by inverse squared distance. The far periphery is blockier, at edges especially, but the quad drop is what makes the stencil test save shading at all.

## Under single-pass stereo

The collapse renders both eyes into one double-wide target, so the mask and fill-in run once, and laid out for one eye they would put a single fovea on the seam. `single_pass.foveation_per_eye` hands both passes each eye's centre as a UV of its own half, an eye-sized half-diagonal, and the first column of eye 1's half; each pixel measures its radius, dither, and neighbourhood from its half's left edge, and a neighbour across the seam reads as zero, as past the edge of an eye-sized target.

## Unverified

- The forced test relies on `SetupRenderStates` rebinding because the rewritten index differs from the last one flushed; whether the mask-write's own state can leak into the first foveated draw, and whether the first draw after the range restores the normal state, is not confirmed.
- `packed_stencil_test` sets one set of stencil operations; if the back face stays at `ALWAYS`, back-facing peripheral geometry still shades.
- The ring radii, the drop, the fill neighbourhood, and the sharper periphery the late fill-in gives the water, clouds, and transparents have not been judged through the lenses.
