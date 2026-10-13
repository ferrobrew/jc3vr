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

Depth-stencil clears are per-pass and data-driven through [`Clear`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-Clear), whose flags are [`ClearFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw::ClearFlags) (`CLEAR_DEPTHBUFFER`, `CLEAR_STENCILBUFFER`). The main depth and stencil are cleared early in the GBuffer range, around [`RenderPassId::RP_Z_OCCLUDERS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_Z_OCCLUDERS). The mask must be written after that clear. The existing `DrawRenderPassRange` hook (`payload/src/hooks/graphics_engine/render_pass.rs`) already splits pass ranges, so it hosts the insertion. After `RP_Z_OCCLUDERS`, it draws a full-screen radial-mask pass and then continues the range. The mask pass binds the MainDepth DSV with no colour target, and its DS state is `StencilEnable`, func `ALWAYS`, `StencilOp REPLACE`, write mask and ref equal to the mask bit, and `DepthEnable=0`/`DepthWriteMask=0`. Its pixel shader discards foveal-centre pixels, so only the periphery writes the bit.

## Implementation

The design above is implemented, on by default, across:

- pyxis defs: [`SetupRenderStates`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-SetupRenderStates), [`CreateDepthStencilState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-CreateDepthStencilState), and the [`HContext_t::m_DepthStencilStateIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::HContext_t##field-m_DepthStencilStateIndex) and [`HContext_t::m_StencilRef`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::HContext_t##field-m_StencilRef) fields with the full packed-index bit layout.
- **shaders** — `payload/src/shaders/foveation_mask_ps.hlsl` (radial dithered mask, interleaved-gradient
  noise) and `foveation_fill_ps.hlsl` (neighbour-average reconstruction), compiled via `shadergen`. Both
  take a 32-byte cbuffer `{ centre_px, inner_px, outer_px, max_drop }`; the two must keep their `ign` and
  drop decision identical so the fill reproduces the mask's choice.
- **`payload/src/vr/foveation.rs`** — the D3D pipeline (mask-write and fill-in full-screen passes on the
  immediate context under `Context::m_Mutex`, the `blit.rs` pattern), the `SetupRenderStates` force-test
  index rewrite (`apply_force_test`), and the `packed_stencil_test` bit math.
- **`payload/src/hooks/graphics_engine/render_pass.rs`** — the `SetupRenderStates` detour, and the
  `DrawRenderPassRange` bracketing that runs the mask-write before the foveated pass range, forces the
  stencil test through it, and runs the fill-in after. The foveal centre comes from the eye's
  `projection_standard` principal point (`foveal_center_uv`), so canted/off-axis eyes fovea about their
  true optical centre, not screen centre.
- **config + UI** — `foveation.{enabled, inner_fraction, outer_fraction, max_drop, mask_bit,
  foveal_first_pass, foveal_last_pass}` (`config.rs`), surfaced in the "Foveation" debug section.
  `enabled` defaults **true**.

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
or debug UI) if it perturbs concurrent validation work. What still needs confirming in a headset:

- The default foveated pass range ([`RenderPassId::RP_MODELS_DYNAMIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_DYNAMIC) to [`RenderPassId::RP_CREATURES`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_CREATURES)) is a starting
  guess. It must sit *after* the depth prepass (so dropped pixels keep full-resolution depth) and cover
  the expensive shading without breaking a pass that reads neighbour pixels. Tune `foveal_first_pass` /
  `foveal_last_pass` against what actually saves frametime and what corrupts.
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
