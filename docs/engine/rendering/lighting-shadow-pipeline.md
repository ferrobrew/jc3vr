# JC3 per-frame sun-shadow and global-lighting pipeline

This doc maps how the game computes and stages its per-frame sun-shadow and global-lighting state — the constants the material shaders read out of `cb0` GlobalConstants each frame, where each value comes from, and, critically, **which of them are selected by a frame counter or frame parity** (the set of things that legitimately differ frame-to-frame in the base game). Names follow the pyxis-defs, which hold every address and offset; follow a linked name into the defs for the binding.

Companion to [`rendering.md`](rendering.md) (the frame pipeline and the stereo double-Draw machinery) and [`shaders.md`](shaders.md) (the shader bytecode side). Where `rendering §N` is cited, it refers to that doc.

## 1. The frame counters

Three engine-global counters and one per-context stamp select frame-varying state. The three counters live in [`render_frame_counters`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::render_frame_counters) (a [`RenderFrameCounters`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderFrameCounters)), advanced once per real frame in the [`GraphicsEngine::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-Draw) prologue (rendering §1.3):

| Counter | Advance | Selects |
|---|---|---|
| [`m_Counter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderFrameCounters##field-m_Counter) | `m_FrameIndex = m_Counter++` — post-increment each frame | The **rolling counter** for shadow-cascade amortization (§3.1) and the light triple-buffer (§6). Read as the full value, not just parity. |
| [`m_FrameIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderFrameCounters##field-m_FrameIndex) | takes the pre-increment `m_Counter` | The **frame parity** `m_FrameIndex & 1`: shadow atlas double-buffer (§3), TAA jitter phase (rendering §5.7), shadow-matrix storage stride. |
| [`m_RingIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderFrameCounters##field-m_RingIndex) | `m_FrameIndex % 3` | The three-slot constant-buffer / light-lookup ring. |
| [`RenderContext::m_RenderFrameNo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_RenderFrameNo) | copied from `m_FrameIndex` in `SetupRenderContext` | The per-dispatch stamp the draw side keys shadow parity on. **Equals `m_FrameIndex`**, so `m_RenderFrameNo & 1 == m_FrameIndex & 1`. |

**Parity consistency (the key invariant).** The shadow *simulation* writes its parity storage indexed by `m_Counter & 1` (in `ShadowManager::UpdateRender`), while the *draw* side reads by `m_FrameIndex & 1` / `m_RenderFrameNo & 1`. These agree because the sim shadow update runs in `CGameStateRun::UpdateShadows` (sim phase, **before** the `Draw` prologue increments the counter): at sim time `m_Counter == C`, and the prologue then assigns `m_FrameIndex = C`. So sim-write parity `C & 1` == draw-read parity `m_FrameIndex & 1`. Both dispatches of a stereo frame share one prologue, hence one parity — the second eye reads the same shadow half the first eye did (rendering §5.3).

[`RenderPass::SetupRenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetupRenderContext) also stamps the context's ring indices: [`RenderContext::m_RenderFrameIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_RenderFrameIndex) (`m_FrameIndex % 3`) and [`RenderContext::m_PreviousRenderFrameIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_PreviousRenderFrameIndex) (`(m_FrameIndex + 2) % 3`). It then copies the live per-frame light and sun values into the context: the point and spot light counts, the light manager's global light direction and diffuse colour, the render engine's sun vector, and the time-of-day light modulator.

## 2. Cast of state owners

- **[`LightManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager)** (a singleton): the scene light gather. Holds the global light's direction and diffuse colour and the environment and VFX light intensity scales; gathers the frame's visible point/spot lights into a parity/triple-buffered update list ([`LightManager::CopyLightsToUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##func-CopyLightsToUpdate)).
- **[`ShadowManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager)** (reached through [`GraphicsEngine::m_ShadowManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_ShadowManager)): the cascaded sun-shadow system (§3). Its global enable flag is [`shadow_system_enabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::shadow_system_enabled).
- **[`RenderEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine)** (a singleton): owns the GlobalConstants staging blocks and the time-of-day sun and fog scalars; `SetGlobalShaderConstants` (§5) assembles the CB here.
- **[`RenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext)** (per dispatch, [`GraphicsEngine::m_RenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_RenderContext)): the per-view scratch the passes fill and read.

## 3. Sun-shadow simulation (sim phase)

The sun shadows are an 8-slot cascaded atlas ([`ShadowManager::m_AtlasTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_AtlasTexture), a depth `Texture2DArray`), parity double-buffered: each shadow frustum's [`CascadeData::m_RenderPassSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::CascadeData##field-m_RenderPassSettings) holds one entry per frame parity naming the atlas slices its passes draw into, so alternate frames render into (and the shaders sample) different slices of the array.

### 3.1 `SetActiveShadowPassCount`: the amortization schedule

[`ShadowManager::SetActiveShadowPassCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##func-SetActiveShadowPassCount) is called from `CGameStateRun::UpdateShadows` before the fit. It sets the active cascade+spot-shadow count and rebuilds the per-cascade update schedule. Each cascade `c` carries an update *level* in [`m_CascadeUpdateLevels`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_CascadeUpdateLevels) (default pattern `{0,1,2,3}` — nearest every frame, each further one half as often). A cascade **refreshes this frame** only when `((1 << level) - 1) & m_Counter == 0`; otherwise it copies its previous fit forward. This is the mechanism that amortizes cascade re-renders across frames, and it is gated on the **full rolling `m_Counter`**, not on parity — so which cascades are freshly fitted cycles with period `2^level`, independently of the parity double-buffer.

### 3.2 `UpdateRender`: the per-frame fit

[`ShadowManager::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##func-UpdateRender) runs once per sim frame. `v6 = m_Counter & 1` is the parity it writes. For the active camera ([`CameraManager::m_ActiveCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##field-m_ActiveCamera), whose `m_ProjectionF` gives the fit frustum) it, per scheduled cascade:

- computes the cascade's world frustum (`CFrustum::Compute`) and calls `UpdateCascade` to fit the slice;
- writes the **parity-indexed** cascade box params and per-cascade blend/scale into [`m_ShadowCascades[parity]`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_ShadowCascades), a pair of per-parity cascade reference vectors, and the **per-slice shadow projection matrices** into [`m_ShadowTextureMatrix[parity][slice]`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_ShadowTextureMatrix). The reference vectors are `m_ActiveTessellationOrigin` and `m_PendingTessellationOrigin`, which are not yet located;
- unscheduled cascades (the amortized ones, §3.1) fall through the `else` branch and keep the prior parity's fit;
- finally `GenerateCullPlanes` regenerates the cull planes.

So the shadow manager holds two full parity copies of every cascade's transform and box params; the fresh half is the one just written, the stale half is the previous frame's — and amortized cascades leave even the "fresh" half carrying an older fit.

### 3.3 `CommitRenderPassSettings`: the per-dispatch gate

[`ShadowManager::CommitRenderPassSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##func-CommitRenderPassSettings) runs per dispatch from [`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask), gated by `shadow_system_enabled`. `v3 = m_FrameIndex & 1`. It clears [`RenderPassState::m_Enabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPassState##flag-m_Enabled) on every static/dynamic shadow pass, then re-enables the passes the schedule marked active this frame and re-points each enabled pass's render target at the parity's atlas slice, read from the cascade's `CascadeData::m_RenderPassSettings[parity]`. This is where the parity chosen at sim time becomes the atlas half actually rendered and sampled.

### 3.4 `GetShadowFade` and `m_ShadowTransparency`

[`ShadowManager::GetShadowFade`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##func-GetShadowFade) returns [`ShadowManager::m_ShadowTransparency`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_ShadowTransparency) when shadows are enabled, else `1.0`. **This is a static config value, not a per-frame dynamic term:** its only writer is `CShadowManager::SetShadowTransparency` (a one-line setter, not yet located), and it is registered as the debug console variable `"Dev|Graphics|Shadows|Transparency"` (default `0.0` from the constructor). It is **stable** frame-to-frame unless the console var is changed. `SetGlobalShaderConstants` stages it as `1 - fade` and `fade` (§5). It does **not** track sun visibility.

### 3.5 The sun-grazing shadow swing

There is no dynamic sun-occlusion query feeding the shadow strength. The whole-terrain shadow swing seen when the sun grazes the horizon comes from two ground-truth mechanisms, both continuous rather than a discrete flag:

1. **The minimum sun-elevation clamp** in `SetGlobalShaderConstants` (§5.1): the staged lighting sun direction is rotated up whenever its elevation drops below a floor angle, so near the horizon a small change in the true sun direction produces a comparatively large change in the clamped direction the shaders light with.
2. **Cascade re-fit** (§3.2): rotating the camera re-fits the scheduled cascades to the new frustum; when the sun is near-grazing, the fit's basis is ill-conditioned (light direction nearly in the fit plane), so the per-slice shadow matrices swing as the frustum turns. The amortized cascades (§3.1) update on their own schedule, so different cascades swing on different frames.

## 4. Deferred lighting combine

[`RenderBlockDeferredLighting::DrawClustered`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting##func-DrawClustered) (pass [`RP_DEFERRED_LIGHTS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_DEFERRED_LIGHTS)) does the clustered light assignment and the deferred combine into MainColor. It reads the per-view constants out of the render context `a2`: the light counts ([`RenderContext::m_ActivePointLightCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ActivePointLightCount), [`RenderContext::m_ActiveSpotLightCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ActiveSpotLightCount)), the camera position ([`RenderContext::m_RenderCameraPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_RenderCameraPosition)), and the projection scalars ([`RenderContext::CameraFOV`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-CameraFOV), [`RenderContext::m_DisplayHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_DisplayHeight), [`RenderContext::m_DisplayRatio`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_DisplayRatio)). It binds the global CB ([`RenderContext::m_FPGlobals`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_FPGlobals) on fragment constant buffer 0) and calls `LightManager::SetupLightingTextures`. It consumes the GlobalConstants and the shadow atlas the previous stages staged; it does not itself select frame-indexed state. The sun contribution and cascade sampling ride the GlobalConstants block (§5).

### 4.1 How the froxel grid is built

Local lights are assigned to a **64-pixel tile lattice over the framebuffer**, `Tx = m_DisplayWidth / 64` by `Ty = m_DisplayHeight / 64` tiles by 64 depth slices. `DrawClustered` builds it in two phases before the deferred resolve.

**Phase 1, light assignment.** Binds [`RenderContext::m_LightAssignmentRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_LightAssignmentRenderSetup) as the render setup — a target one texel per tile, arrayed over light chunks — then:

```c
Graphics::Clear(ctx, CLEAR_RENDERBUFFER_R | _G | _B | _A, {1,1,1,1}, ...);  // ClearRenderTargetView on every colour RT
Graphics::SetVertexProgramConstants(ctx, 2, 0, &view_rows, 4);   // lightassignment VS  cb2.ViewMatrix
Graphics::SetGeometryProgramConstants(ctx, 0, 0, M, 4);          // lightassignment GS  cb0.ProjMatrix
Graphics::SetFragmentProgramConstants(ctx, 1, 0, tile_bounds);   // lightassignment PS  cb1  (2 vec4s)
Graphics::SetBlendEnable(ctx, 1); SetBlendEquation(ctx, …); SetBlendFunc(ctx, …);  // a commutative blend
Graphics::DrawIndexedInstanced(ctx, nullptr, 240, point_light_count);  // sphere proxies
Graphics::DrawIndexedInstanced(ctx, nullptr, 48, spot_light_count);    // cone proxies
```

where `M = m_ProjectionF · T(+1,-1,0) · Scaling(Tx/ceil(Tx), Ty/ceil(Ty), 1) · T(-1,+1,0)` — the projection plus the NDC nudge that snaps the grid to whole tiles when the display size is not a multiple of 64. The clear flags are [`ClearFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw::ClearFlags).

The `cb2` view matrix comes from [`RenderContext::m_View`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_View) with its translation row forced to `(0,0,0,1)`, over light positions the CPU has already biased by `RenderContext::m_RenderCameraPosition` (the main render camera's world position, distinct from [`RenderContext::m_CameraPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_CameraPosition), which follows whichever camera the pass draws with). The light-assignment vertex shaders (`sh_0090` point, `sh_0092` spot) end with `add o0.xyz, r0.xyzx, cb2[3].xyzx`, so `cb2` row 3 *is* the translation the engine bothers to zero.

The geometry shader (`sh_1483`) emits `SV_RenderTargetArrayIndex` from each light's packed chunk index and passes the proxy triangle's three view-space vertices down as packed f16. The pixel shader (`sh_0572`) reconstructs them and tests them against the tile's frustum, derived affinely from its own pixel coordinate:

```
cbuffer PerFrameConstants {          // cb1 -- 8 floats
  float scale_x, bias_x0, bias_x1, dummy_x;
  float scale_y, bias_y0, bias_y1, dummy_y;
}
dcl_input_ps_siv linear noperspective v0.xy, position
mad r1.xz, v0.xxxx, cb1[0].xxxx, cb1[0].yyzy     // tile x max/min frustum bound
mad r4.xy, v0.yyyy, cb1[1].xxxx, cb1[1].yzyy     // tile y max/min frustum bound
```

Because `SV_Position` is the absolute render-target coordinate (rendering §4.3), `v0.x` is the absolute tile index plus `0.5`.

**Phase 2, the fill.** Binds [`RenderContext::m_LightAssignmentFillRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_LightAssignmentFillRenderSetup) as the render setup, binds phase 1's output ([`RenderContext::m_LightAssignmentTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_LightAssignmentTexture)) as a texture, uploads the four light-chunk counts on `cb1` (`count = 1`), and issues `Graphics::Draw(ctx, nullptr, 192)`. Its geometry shader (`sh_1484`) turns each of the 192 vertices' 64 primitives into a fullscreen triangle with `SV_RenderTargetArrayIndex = SV_PrimitiveID`, one pass over each of the 64 depth slices. Each output texel reads only its own tile's chunk entries, so this phase is entirely per-tile-local.

**Then the resolve** — `SetupLightingTextures`, `PerspectiveFovInverse` (rendering §4.1), and a fullscreen `Graphics::Draw(ctx, nullptr, 3)`.

Three properties of the build fall out and are worth stating explicitly, because they determine what a partial rebuild can and cannot do: the phase-1 clear is viewport- and scissor-independent and covers the whole target; the assignment blend is commutative, so disjoint tile sets accumulate independently of order; and the fill is per-tile-local.

### 4.2 Who reads the grid, and when

The grid is **live well outside `DrawClustered`**. The engine binds it through exactly one function pair: [`LightManager::SetupLightingTextures`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##func-SetupLightingTextures) is called only from `DrawClustered`, and [`LightManager::SetupForwardLightingResources`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##func-SetupForwardLightingResources) is the forward-lit path; both delegate to [`RenderBlockTypeBase::SetupLightingTextures`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderBlockTypeBase##func-SetupLightingTextures), which does the binding. `SetupForwardLightingResources`' callers are the complete consumer list, the `Setup` functions of 20 render-block types. Three of them (the character, character skin, and road junction blocks) are per-block `Setup` functions rather than type `Setup` functions:

| Render block `Setup` | Render block `Setup` |
|---|---|
| [`RenderBlockTypeBavariumShield::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeBavariumShield##func-Setup) | [`RenderBlockCharacter::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockCharacter##func-Setup) |
| [`RenderBlockTypeBuildingJC3::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeBuildingJC3##func-Setup) | [`RenderBlockCharacterSkin::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockCharacterSkin##func-Setup) |
| [`RenderBlockTypeCarLight::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeCarLight##func-Setup) | [`RenderBlockTypeWindow::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeWindow##func-Setup) |
| [`RenderBlockTypeCarPaintMM::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeCarPaintMM##func-Setup) | [`RenderBlockTypeGeneralMkIII::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMkIII##func-Setup) |
| [`RenderBlockTypeGeneral::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneral##func-Setup) | [`RenderBlockTypeMaterialTune::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeMaterialTune##func-Setup) |
| [`RenderBlockTypeGeneralJC3::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralJC3##func-Setup) | [`RenderBlockTypeMeshParticle::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeMeshParticle##func-Setup) |
| [`RenderBlockTypeGeneralMaskedJC3::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMaskedJC3##func-Setup) | [`RenderBlockTypeProp::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeProp##func-Setup) |
| [`RenderBlockTypeLayered::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeLayered##func-Setup) | [`RenderBlockRoadJunction::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockRoadJunction##func-Setup) |
| [`RenderBlockTypeSkidmarks::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeSkidmarks##func-Setup) | [`RenderBlockFoliageType::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockFoliageType##func-Setup) |
| [`RenderBlockTypeWeather::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeWeather##func-Setup) | [`RenderBlockTypeParticle::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeParticle##func-Setup) |

The shader side agrees: 172 of the 1551 blobs in `Shaders_F` bind `LightLookup` (`texture3d`, `uint4`, `t31`). Seven are the `deferred_clusteredlighting*` resolves; the rest are the forward-lit families — every `*_blend` material permutation, `vegetationfoliageblend*`, `windowsimple*`/`windowdamage`, `skidmarks_normal_diffuse`, and ~90 `particleeffect*` permutations. They index it by the **absolute** framebuffer tile (rendering §4.3).

`RP_DEFERRED_LIGHTS` comes after [`RP_LAST_GBUFFER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_LAST_GBUFFER) in pass order, so `DrawClustered` runs after the whole G-buffer range, and the consumers straddle it:

- **Before.** The foliage block draws in `RP_VEGETATION_OPAQUE`, inside the G-buffer range, and its `Setup` calls `SetupForwardLightingResources` unconditionally; the road junction block is likewise in `RP_ROAD_JUNCTION` / `RP_ROAD_JUNCTION_OPAQUE`. **These read the grid the frame before it is built.**
- **After.** The transparent and particle families: `RP_SKIDMARKS`, `RP_MODELS_TRANSPARENT`, `RP_VEGETATION_TRANSPARENT`, `RP_MODELS_GLINT`, `RP_PARTICLE_RIBBON`, `RP_PARTICLE_LOWRES`, `RP_WINDOW_DECALS`, `RP_MODELS_REFRACT`, `RP_PARTICLE_GENERAL`, and `RP_PARTICLE_LOWRES_OVERLAY`.

Nothing binds or unbinds `t31` outside those two functions, so a grid left half-valid stays half-valid for the rest of the frame *and* into the next one.

## 5. `SetGlobalShaderConstants`: the centerpiece

[`RenderEngine::SetGlobalShaderConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetGlobalShaderConstants) is called per dispatch from `GraphicsEngine::HandleDrawThreadTask` after `SetRenderContextCamera`. It assembles the `cb0` GlobalConstants into the render engine's two staging blocks, [`RenderEngine::m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData) (vertex) and [`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData) (fragment), which are later bound as the global vertex and fragment CBs. The rows below are `cb0` rows of those blocks. It runs only when the `LightManager` singleton is present, first calling `LightManager::ApplyDynamicLights` and [`LightManager::UpdateRenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##func-UpdateRenderContext). What it stages, grouped:

### 5.1 Sun / light direction (with the elevation clamp)

- **Raw sun direction** `(v7,v8,v9)` = [`LightManager::m_GlobalLightDirection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##field-m_GlobalLightDirection) → fragment row 3 `.xyz`.
- **Elevation-clamped sun direction** `(v95,v96,v97)`: starts as the raw direction, then `v15 = dot(sunDir, worldUp)` with `worldUp` = [`vector3_up`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::math::vector3_up), which is `(0,1,0)`. If `v15 < sinf(minElevation)` where `minElevation` is [`RenderEngine::m_SunBacklightHeightClamp`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_SunBacklightHeightClamp), it builds `RotationAxis(cross(sunDir, worldUp), -(minElevation - asin(v15)))` and rotates the direction up to the floor. Staged at fragment row 31 `.xyz`. This is the term behind the near-horizon lighting swing (§3.5).
- **Sun backlight and shadow colour products** from the `RenderEngine` fields [`RenderEngine::m_SunBacklightColor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_SunBacklightColor) through [`RenderEngine::m_SunShadowSpecularMultiplier`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_SunShadowSpecularMultiplier) (time-of-day colours and their diffuse and specular multipliers, set by the environment/lighting update) → fragment rows 29 to 33. Continuous, same both dispatches.

### 5.2 Shadow terms (the parity surface)

- **Horizon-map blend time** `v20 =` [`m_HorizonTime[m_RenderFrameNo & 1]`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_HorizonTime) → vertex row 4 `.w`, with a second copy in the fragment block. **Parity-indexed.** It is a blend time, not a shadow atlas slice base.
- **Horizon-map occlusion scale** `v21 =` [`ShadowManager::m_HorizonScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_HorizonScale) when [`ShadowManager::m_EnableHorizonMapping`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_EnableHorizonMapping) is set, else `0` → vertex row 5 `.w`.
- **Shadow fade** `v47 = m_ShadowTransparency` (or `1.0` when disabled, §3.4) → `1 - v47` and `v47` at fragment row 9 `.xy`. Stable.
- **Cascade block** — a `memcpy` of the forward-material [`RenderContext::m_ShadowCascades`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ShadowCascades) (a [`ShadowCascades`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::ShadowCascades): per-cascade scale/blend + offset/slice + box-test params) into fragment rows 45 onward. Filled by `SetRenderContextCamera` from the shadow manager's **parity** storage. **Parity-indexed.**
- **Shadow matrices** — a `memcpy` of the 8 per-slice projective [`RenderContext::m_ShadowMatrices`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ShadowMatrices) into fragment rows 63 to 94, likewise parity-filled. **Parity-indexed.**

### 5.3 Camera-derived constants

- **Full (translation-bearing) view-projection**: a `memcpy` of the render camera's [`Camera::m_ViewProjectionF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_ViewProjectionF) into vertex rows 0 to 3. It drives screen-space / non-geometry work (rendering §2.4). Off-axis, so **per-eye** in stereo.
- **Camera world position** from the translation row of the render camera's [`Camera::m_TransformF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_TransformF) → vertex rows 4 and 5 and fragment rows 4 to 6. **Per-eye.**
- **Depth-unproject constants** from the render camera near/far: `1/m_Near` ([`Camera::m_Near`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_Near)) and `1/m_Far` ([`Camera::m_Far`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_Far)) → fragment row 9 `.z = 1/near − 1/far` and `.w = 1/far`. Stable within a frame.
- **Inverse display size** from [`Graphics::GetDeviceInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetDeviceInfo): `1/width`, `1/height` → fragment row 8 `.zw`. The fragment `GlobalConstants` cbuffer therefore carries `cb0[8].zw = (1/DisplayWidth, 1/DisplayHeight)`, sourced from `Graphics::SDeviceInfo` and not from the bound render setup or the viewport. This is the one screen-size constant the shaders have; see rendering §4.3 for how they use it.

### 5.4 Wetness and fog

- **Wet properties**: a `memcpy` of `GetWetProperties(ctx)`, the four-row weather wetness block, into fragment rows 40 to 43. Continuous.
- **Fog / atmosphere** via [`RenderEngine::SetFog`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetFog) at the tail: builds two fog layers from the render engine's time-of-day fog scalars. The fog colour goes to vertex rows 23 and 24 and fragment rows 27 and 28; the per-layer distance/density ramp (start/inv-range/height) goes to vertex rows 14 and 15 and fragment rows 14 and 15; and a normalized fog light direction goes to fragment row 44. The fog scalars on `RenderEngine` are not yet bound in the defs. Fog reads the render camera height (the `.y` of `RenderContext::m_RenderCameraPosition`) and the time-of-day fog fields; continuous, same both dispatches.

## 6. Light gather buffering

`LightManager::CopyLightsToUpdate` copies the frame's visible point/spot lights into `m_FrameLightInfo[3]` / the [`LightManager::m_LightLookup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##field-m_LightLookup) / `m_LightIndexBuffer[3]` triple-buffer, indexed off `m_Counter`, and configures the volumetric spot-light passes ([`enable_low_res_spot_light_volume`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::enable_low_res_spot_light_volume)). The dynamic light list the deferred combine (§4) reads is therefore selected by the rolling counter, one of the three slots per frame.

## 7. The ping-pong table

Every per-frame global lighting/shadow value the material shaders consume, and what selects it frame-to-frame. "Stable" = deterministic within a frame and identical across both dispatches of a stereo frame; "per-eye" = legitimately differs between the two dispatches by design; "parity" = `m_FrameIndex & 1` (== `m_RenderFrameNo & 1`); "counter" = full `m_Counter`. CB rows are `cb0` rows of the vertex (VP) or fragment (FP) GlobalConstants.

| Value | Where computed | Backing field / global | Indexed by |
|---|---|---|---|
| Raw sun direction | `SetGlobalShaderConstants` | `LightManager::m_GlobalLightDirection` → FP row 3 | stable |
| Elevation-clamped sun direction | `SetGlobalShaderConstants` (RotationAxis clamp) | FP row 31; floor angle `RenderEngine::m_SunBacklightHeightClamp`; up = `vector3_up` | stable (continuous fn of sun dir) |
| Sun colour / basis products | `SetGlobalShaderConstants` | `RenderEngine::m_SunBacklightColor` .. `m_SunShadowSpecularMultiplier` → FP rows 29 to 33 | stable (time-of-day) |
| Time-of-day light modulator | `SetupRenderContext` | [`LightManager::m_TimeOfDayLightModulator`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##field-m_TimeOfDayLightModulator) → [`RenderContext::m_TimeOfDayEmissiveScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_TimeOfDayEmissiveScale) | stable |
| Global light diffuse colour | `SetupRenderContext` | `LightManager::m_GlobalLightDiffuse` → [`RenderContext::m_GlobalLightDiffuse`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_GlobalLightDiffuse) | stable |
| Sun vector | `SetupRenderContext` | [`RenderEngine::m_SunVector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_SunVector) → [`RenderContext::m_SunVector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_SunVector) | stable |
| **Horizon-map blend time** | `SetGlobalShaderConstants` | `ShadowManager::m_HorizonTime[parity]` → VP row 4 `.w` | **parity** |
| **Shadow cascade block** (scale/blend, offset/slice, box test) | filled by `SetRenderContextCamera`, staged `SetGlobalShaderConstants` | `RenderContext::m_ShadowCascades` → FP rows 45 onward | **parity** |
| **Per-slice shadow matrices [8]** | `SetRenderContextCamera`, staged `SetGlobalShaderConstants` | `RenderContext::m_ShadowMatrices` → FP rows 63 to 94 | **parity** |
| Active cascade count | `SetRenderContextCamera` | [`ShadowManager::m_ActiveCascades`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_ActiveCascades) → [`RenderContext::m_ActiveCascadeCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ActiveCascadeCount) | **parity** |
| **Which cascades are freshly fit** (atlas contents) | `SetActiveShadowPassCount` schedule + `UpdateRender` | `m_CascadeUpdateLevels`; gate `((1<<lvl)-1)&counter` | **counter** (period `2^lvl`) |
| Shadow fade / transparency | `SetGlobalShaderConstants` | `ShadowManager::m_ShadowTransparency` | stable (console var) |
| Shadow enable | throughout | `shadow_system_enabled` | stable |
| Full view-projection (`m_ViewProjectionF`) | `SetGlobalShaderConstants` | `Camera::m_ViewProjectionF` → VP rows 0 to 3 | per-eye |
| Camera world position | `SetGlobalShaderConstants` | render cam `m_TransformF` translation → VP rows 4 and 5, FP rows 4 to 6 | per-eye |
| Depth-unproject (1/near, 1/far) | `SetGlobalShaderConstants` | `Camera::m_Near` / `Camera::m_Far` → FP row 9 `.zw` | stable within frame |
| Inverse display size (`cb0[8].zw`) | `SetGlobalShaderConstants` | `GetDeviceInfo` → FP row 8 `.zw` | stable |
| Wet properties block | `SetGlobalShaderConstants` | `GetWetProperties(ctx)` → FP rows 40 to 43 | stable |
| Fog colour / ramps / fog light dir | `SetFog` | `RenderEngine` fog scalars → VP rows 14 to 24, FP rows 14 to 44 | stable (time-of-day) |
| Dynamic light list | `CopyLightsToUpdate` | `m_FrameLightInfo[3]`, `LightManager::m_LightLookup` | **counter** (3-slot ring) |
| TAA jitter phase (perturbs projection) | [`ApplySubsampleJitter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-ApplySubsampleJitter) | reads `m_FrameIndex` | **parity/ring** (rendering §5.7) |
| Auto-exposure | [`ToneMappingEffect::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::tone_mapping::ToneMappingEffect##func-Update) | [`m_ExposureBrightPoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::tone_mapping::ToneMappingEffect##field-m_ExposureBrightPoint) | frame-counted (rendering §5.4) |

**The shadow ping-pong surface, in one line:** the horizon-map blend time, the cascade block, the per-slice matrices, and the active-cascade count all switch on `m_FrameIndex & 1`; *which* cascades carry a fresh fit switches on the full `m_Counter`. Everything else in the lighting/fog/sun set is recomputed every frame from continuous inputs and is identical across a stereo frame's two dispatches. Any lighting/shadow flicker that is *not* explained by the exposure meter (rendering §5.4) or TAA jitter must come from one of the parity- or counter-indexed shadow rows above being read at a parity/counter that disagrees with the half that was written.

## 8. Bindings

Every function, field, and global named here is bound in the pyxis defs, in the [`ShadowManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager), [`LightManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager), [`RenderEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine), and [`RenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext) types and the [`RenderBlockDeferredLighting`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting) render block.
