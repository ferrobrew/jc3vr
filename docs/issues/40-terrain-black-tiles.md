# Issue #40: terrain walls and cave ceilings render black at grazing angles in VR

Static reverse-engineering of the base VolumetricTerrain (`CRenderBlockTerrain`) draw pipeline
against the IDB and the shipped shader bundles. The prior in-headset
work (issue #40, commit `8fa3521`) had confirmed the black tiles are absent from the G-buffer and
had ruled out every block-level lever; this pass maps the full pipeline, corrects two earlier
misconceptions, and lands two live toggles that discriminate the remaining candidates.

## How a base-terrain tile gets drawn

The base terrain is streamed as four patch maps (`terrain_patch_lod9..12`) in the generic Avalanche
patch system; each patch owns a `CRenderBlockTerrain` constructed in the map's constant memory
([`TerrainPatchSystemCreateLayers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/patch_system##func-TerrainPatchSystemCreateLayers), `OldTerrainPatchSystem::TerrainPatchSystemCreateLayers`). Per frame:

1. **Cull.** [`LandscapeManager::UpdatePatchSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::LandscapeManager##func-UpdatePatchSystem) assembles up to [`PatchViewDesc::MAX_FRUSTUMS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchViewDesc::MAX_FRUSTUMS) cull frusta and runs a BFBC AABB cull per patch per frustum, writing one bit per frustum entry into the patch header's [`PatchHeader::m_VisibilityBits`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchHeader##field-m_VisibilityBits). The entries are identified by [`PatchFrustumId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchFrustumId): [`FRUSTUM_ID_MAIN_CAMERA`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchFrustumId##variant-FRUSTUM_ID_MAIN_CAMERA) is the occluder manager's main cull camera, the `FRUSTUM_ID_SHADOW_PASS_*` entries are the shadow cascades, [`FRUSTUM_ID_REFLECTIVE_SHADOW_0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchFrustumId##variant-FRUSTUM_ID_REFLECTIVE_SHADOW_0) and `FRUSTUM_ID_REFLECTIVE_SHADOW_1` are the two RSM frusta, and the final entry, [`FRUSTUM_ID_DEPTH_PREPASS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchFrustumId##variant-FRUSTUM_ID_DEPTH_PREPASS), is a *second run of the same main cull camera*. Both main-camera entries share one cached parameter set ([`OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::OccluderCollectionManager##func-GetBFBCFrustumParamsForCameraAndTime), keyed by camera + time), so **the color-pass and depth-prepass bits are computed from identical inputs and cannot disagree**. The frustum planes are generic Gribb–Hartmann extractions from `m_View` × `m_ProjectionF` ([`BFBCCalculateCameraFrustum`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::bfbc##func-BFBCCalculateCameraFrustum)) — convention-agnostic (0 ≤ z ≤ w volume), so the
   mod's stamped union projection yields a geometrically sound frustum, and the mod's
   `disable_bfbc_occlusion` (default on) already truncates the occluder frustums to just the camera.
2. **Submit.** The per-patch fragment [`TerrainPatchUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/patch_system##func-TerrainPatchUpdate) (registered under the `TerrainPatchUpdate` hash) updates alpha fade, sort id, and the tessellated index count, then adds the block to one render pass per set visibility bit: `FRUSTUM_ID_MAIN_CAMERA` → the color passes ([`RP_TERRAIN_APPLY_NEAR_DETAILED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAIN_APPLY_NEAR_DETAILED) for base-LOD tiles within one tile of the camera, [`RP_TERRAIN_APPLY_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAIN_APPLY_NEAR) for other high-detail-LOD tiles, [`RP_TERRAIN_APPLY_FAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAIN_APPLY_FAR) otherwise), the `FRUSTUM_ID_SHADOW_PASS_*` ids → shadow passes, the `FRUSTUM_ID_REFLECTIVE_SHADOW_*` ids → RSM, and `FRUSTUM_ID_DEPTH_PREPASS` → the depth prepass.
3. **Draw.** Both the prepass and the color pass run the same tessellation pipeline ([`RenderBlockTerrain::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-Setup)/[`RenderBlockTerrain::SetupZ`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-SetupZ) and [`RenderBlockTerrain::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-Draw); [`RenderBlockTerrain::DrawZ`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-DrawZ) is a nullsub — the Z-family `SetupZ`/`DrawZ` path in the passes from [`RP_Z_OCCLUDERS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_Z_OCCLUDERS) to [`RP_Z_AND_VELOCITY_PASS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_Z_AND_VELOCITY_PASS) and in [`RP_Z_FINAL_TRANSPARENT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_Z_FINAL_TRANSPARENT) is not how terrain reaches the depth prepass). The hull program is selected by [`RenderBlockTerrain::HullClipType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-HullClipType).

## Corrected: what `HullClipType` actually selects

The semantics of [`TerrainHullClipType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::TerrainHullClipType) differ from the earlier assumption that `TERRAIN_HULL_CLIP_UNDER_WATER` was the color-pass LOD clip:

- **[`TERRAIN_HULL_CLIP_BETWEEN_LODS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::TerrainHullClipType##variant-TERRAIN_HULL_CLIP_BETWEEN_LODS) is the LOD clip.** Selected **unconditionally for every tile coarser than the base LOD** (the threshold is on `RenderBlockTerrain::HullClipType`), in every pass
  (`m_DisableLodClipping` is not even compiled into the function). The clip hulls
  (e.g. `sh_1493` in `Shaders_F`) sample a `VisibilityMask` texture — the global terrain mask that
  `UpdateGlobalMask` writes from each finer tile's streaming/alpha state — at the patch's four
  corners, and zero the tessellation factors (discarding the patch) when all four read < 0.05,
  i.e. when a finer-LOD tile has *fully faded in* over that footprint.
- **[`TERRAIN_HULL_CLIP_UNDER_WATER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::TerrainHullClipType##variant-TERRAIN_HULL_CLIP_UNDER_WATER) is the water clip.** For base-LOD tiles at the context's high-detail LOD when the camera is
  above water: discard patches below the cached water level (the water render pass draws there
  instead). This is what the earlier `force_terrain_hull_clip` toggle overrode — which is why it
  had no effect on the black tiles.
- **[`TERRAIN_HULL_CLIP_DISABLED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::TerrainHullClipType##variant-TERRAIN_HULL_CLIP_DISABLED) is no clip.**

The GPU hull culls (back-patch, frustum, cull-by-detail) are separate, flag-gated constants in the
hull/domain CB; the in-headset tests already disabled them through their verified fields with no effect on the
*detail-skin* black tiles, and they were assumed per-eye. That assumption was wrong — the constants
are baked once per frame per slot, so both eyes share them; see "Recurrence under the single-pass
collapse" below.

## The mechanism that fits every observation

The LOD clip keys on **residency** (the mask), but whether the finer tile actually draws keys on
**visibility** (the BFBC bits). The engine's implicit invariant is: a resident finer tile that got
culled is off-screen, so the coarse tile covering the same footprint is culled too. If that
invariant breaks — the finer tile's AABB fails the main-camera cull while its footprint is on
screen — then the coarse tile has carved the footprint out (in the prepass *and* the color pass)
and the finer tile fills it in neither: **nothing draws there in any pass**. The pixels keep the
clear/far depth, the sky/atmosphere resolve leaves them dark, and the froxel fog tints them the
observed deep blue — which also explains why they don't read as see-through holes. (The issue's
"the depth prepass still draws them" was an inference, not a measurement; the heavy fog tint is
actually evidence of *far* depth — a genuinely near wall would accumulate almost no fog.)

This fits every data point: tile-quantized staircase boundaries (the finer tile's footprint on the
mask grid), discrete flips on small rotations (the finer tile's AABB crossing a cull plane),
world-locked and identical in both eyes (one CPU cull per frame), immune to every GPU-side lever,
and present since VR terrain first worked.

What it does *not* yet pin down is **why the cull drops a visible finer tile** — the widened union
frustum is conservative and covers both eyes by construction. Candidates, in rough order: the cull
pose lagging or diverging from the render pose (game-camera vs HMD orientation coupling); the
union projection not actually reaching this cull at the moment the cached frustum recomputes; or
the wrong-invariant half being the *mask* (a finer tile marked fully-resident whose draw is
legitimately absent for another reason, e.g. missing stream data).

## The two discriminators (landed as config/debug-UI toggles)

- `force_terrain_lod_clip` — maps `HullClipType`'s `TERRAIN_HULL_CLIP_BETWEEN_LODS` to `TERRAIN_HULL_CLIP_DISABLED`, disabling the LOD clip so coarse tiles draw
  their full geometry. If the black fills with (coarser) rock, the mechanism above is confirmed:
  the black was a finer tile's mask footprint that nobody drew.
- `force_terrain_patch_visibility` — a detour on `TerrainPatchUpdate` that ORs the `FRUSTUM_ID_MAIN_CAMERA` and `FRUSTUM_ID_DEPTH_PREPASS` bits into every patch's `m_VisibilityBits`, so every *resident* tile is submitted to the
  color passes and depth prepass regardless of the cull. If this alone fixes the black, the BFBC
  cull of the finer tile is the root cause, and this toggle (scoped to the main-view bits) is also
  a viable fix — the cost is drawing all resident base-terrain tiles, which is bounded by the
  streaming radius.

Expected outcomes: both fix it → cull is the root cause (prefer the visibility fix, or repair the
union-cull path); only the LOD-clip force fixes it → the finer tile is missing for a non-cull
reason (chase its `Draw` early-outs: displacement texture, indices, alpha); neither fixes it → the
mechanism is wrong and the next probe is a per-pass draw-count comparison from inside
`CRenderBlockTerrain::Draw`.

## Bindings established this session

The functions and fields located in this session are bound in the pyxis defs, chiefly in the [`patch_system`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/patch_system) module and on [`RenderBlockTerrain`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain), [`RenderBlockTypeTerrainPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch), and [`PatchHeader`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchHeader). They include `TerrainPatchUpdate` (the base system's per-patch fragment, dormant in retail; see the correction below), [`TerrainPatchSystemFragment`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/patch_system##func-TerrainPatchSystemFragment) (the **live** patch system's per-patch fragment), the construction/destruction stub fragment [`TreeDataPatch`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/patch_system##func-TreeDataPatch), [`RenderBlockTypeTerrainPatch::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##func-Setup) (which inlines the live hull-clip selection), [`UpdateTessellatedTriangles`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/patch_system##func-UpdateTessellatedTriangles), the terrain block's [`UpdateSortID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-UpdateSortID), [`UpdateAlpha`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-UpdateAlpha), [`SetAlpha`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-SetAlpha), and [`UpdateGlobalMask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrain##func-UpdateGlobalMask), [`BFBCCalculateOccluderFrustums`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::bfbc##func-BFBCCalculateOccluderFrustums), [`RenderEngine::SetGlobalShaderProgramCameraConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetGlobalShaderProgramCameraConstants) (which uploads [`RenderContext::m_FrustumPlanes`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_FrustumPlanes) into the frustum-plane rows of [`RenderEngine::m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData)), and the patch header's `m_VisibilityBits`, [`m_Lod`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchHeader##field-m_Lod), [`m_StatusBits`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchHeader##field-m_StatusBits), and [`m_BFBCOcludee`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/patch_system::PatchHeader##field-m_BFBCOcludee).

Terrain hull/domain shaders in `Shaders_F.shader_bundle`: hulls `sh_1492..sh_1504` (the
mask-sampling LOD-clip variants declare a `VisibilityMask` t0), domains `sh_1513..sh_1521`.

## Correction: the base VolumetricTerrain system is dormant in the retail world

Live process inspection (reading `/proc/<pid>/mem` while the game ran) proved that the toggles above
were written correctly — the type singleton [`RenderBlockTypeTerrain`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrain) held the UI's values — and that both
detours were installed, yet neither `CRenderBlockTerrain::HullClipType` nor `TerrainPatchUpdate`
was ever called. **The base "VolumetricTerrain" system never draws in the retail world**; every
base-terrain lever ever tested (this session's and the earlier ones) was a no-op because the system
it targets is inactive. The visible terrain — tops and walls — is `CRenderBlockTerrainPatch`, the
volumetric-patch system.

The mechanism analysis transfers to the live system, where it is untested:

- The live per-patch fragment is **`TerrainPatchSystemFragment`** — same visibility-bit submission, same frustum-id routing (`FRUSTUM_ID_MAIN_CAMERA` → color, `FRUSTUM_ID_DEPTH_PREPASS` → depth prepass), plus LOD-9 quadrant sub-blocks for the near passes. (`TerrainPatchUpdate` is the dormant base system's fragment; the fragment-name table pairs names and functions across rows, not within them.)
- The live LOD clip has **no callable seam**: the patch type's `Setup` inlines the selection from [`RenderBlockTypeTerrainPatch::m_HullProgramHolders`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##field-m_HullProgramHolders). The near passes ([`RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR) and [`RP_TERRAINPATCH_BASEMESH_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_BASEMESH_NEAR)) bind the near hull holder, the other tessellating passes ([`RP_TERRAINPATCH_BASEMESH_TESSELLATE_FAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_BASEMESH_TESSELLATE_FAR) and [`RP_TERRAINPATCH_BASEMESH_TESSELLATE_COLOR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_BASEMESH_TESSELLATE_COLOR)) bind the LOD-clip holder, and `m_DisableLodClipping` is compiled out. The retargeted `force_terrain_lod_clip` therefore swaps the LOD-clip holder with the near holder on the type object (auto-restored on toggle-off).
- The earlier `HullClipType`-forcing test from `8fa3521` targeted the dormant base type's `RenderBlockTerrain::HullClipType`, so the original "hull clip ruled out" result is void for the live system.
- The `terrain heartbeat (#40)` log line now counts both systems: `hull_calls`/`patch_calls`
  (dormant base) and `fragment_calls`/`setup_calls` (live patch system).

Retest matrix, now against the live system: `force_terrain_lod_clip` (holder swap) and
`force_terrain_patch_visibility` (now also hooked into `TerrainPatchSystemFragment`), with the
heartbeat confirming the hooks fire.

## Identified: the wall surface is `TerrainDetail`

The registry bisect (the new "Render block types" debug panel — the engine's `CRenderBlockFactory` vector, bound as [`RenderBlockTypeRegistry`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderBlockTypeRegistry); the game stubs `Enable`/`Disable`/`IsEnabled` to constants, so the panel patches each type's [`IsEnabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderBlockTypeBase##vfunc-IsEnabled) virtual to a return-false stub) found
the owner: **disabling `TerrainDetail` (`CTerrainRenderBlockDetail`) removes the entire cliff rock
skin** — the surface the black tiles live on. The volumetric-patch debug modes recolor only the
terrain *top* skin, confirming the split.

What is known about the detail system so far (unconfirmed): per-tile blocks with
`m_DetailPatchIndex`/`m_DetailLod`; `m_Mesh` points at the patch system's `STerrainPatchMesh`, so
the detail geometry is the GPU-built quad output (`CRenderBlockTerrainSetup`); each patch owns an
`SDetailPatchData` of 5×{`CRenderBlockTerrainDetailSetup`, high-detail, detail} sub-blocks, which
`TerrainPatchSystemFragment` submits for LOD-9 patches in the camera's quadrant — gated by
*position*, not visibility bits, which is why the visibility force could not affect the black.

Next RE target: the GPU quad-selection pipeline feeding these blocks — the
`volumetricterrainenumeratetexels` / `calculatetriangles` / `calculatehistogram` /
`calculatemorphtarget` compute passes and the detail block's own `Draw`/`DrawZ`
([`DetailRBType::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::DetailRBType##func-Setup), `CTerrainRenderBlockDetail::CDetailRBType::Setup`, is bound). A view-dependent texel enumeration that assumes the
flatscreen FOV would produce exactly the observed eye-identical, tile-quantized, angle-dependent
dropout that no submission-side lever can fix.

## The detail-quad selection pipeline (decoded from the compute shaders)

[`RenderBlockTerrainDetail::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainDetail##func-Draw) (`CTerrainRenderBlockDetail::Draw`) is a `DrawIndexedInstancedIndirect` from a per-tile 16-byte args slot (indexed by [`m_DetailPatchIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainDetail##field-m_DetailPatchIndex)/[`m_DetailLod`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainDetail##field-m_DetailLod)) in a buffer owned by the terrain setup type ([`RenderBlockTypeTerrainSetup::m_DetailDrawList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainSetup##field-m_DetailDrawList)). The args are written by GPU compute: the `CRenderBlockTerrainDetailSetup` blocks dispatch three indirect compute stages in the terrain setup passes ([`RP_TERRAINPATCH_SETUP`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_SETUP), [`RP_TERRAINPATCH_MORPH_TARGET`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_MORPH_TARGET), and [`RP_TERRAINPATCH_ENUMERATION`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_ENUMERATION), all pointed at [`STerrainPatchSystem::m_TerrainCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::STerrainPatchSystem##field-m_TerrainCamera) by [`TerrainPatchSystemUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::render_engine##func-TerrainPatchSystemUpdate)), and the compute shaders (`sh_1434`–`sh_1440` in
`Shaders_F`: `patch_meta_data`/`quad_meta_data`/`QuadGroup`/`DetailDrawList`/`DetailDispatchList`)
build the per-tile draw lists.

The draw-list builder (`sh_1434`) selects each quad with, per 6 frustum planes from **the frustum-plane rows of the VS global constants** (`RenderEngine::m_VPGlobalConstData`, uploaded from the pass frustum camera — the terrain camera — by `SetGlobalShaderProgramCameraConstants`): keep when `dot(n, center) + d < radius` for all six
(outward normals; inside is negative), AND the quad is within `radius+64` (high-detail bucket) or
`radius+128` (detail bucket) world units of the camera position (the camera-position row of the same constants), AND not deeper than 80
units below sea level while the camera is above water. So the detail rock skin exists only within
~64–128 m of the head, and **the only view-direction-dependent input is the terrain camera's
frustum planes** — the planes `widen_terrain_cull` rebuilds.

New discriminator: `terrain_cull_accept_all` (config + "Culling & geometry" checkbox) stamps the
terrain camera's planes with zero normals and a hugely *negative* distance each frame (the engine's
plane convention is outward/negative-inside), making every plane test pass. If the black fills in
with it on, the quad cull is the discard stage and the union widen is somehow not covering what the
eyes see (sign, timing, or coverage); comparing the accept-all result against `widen_terrain_cull`
on/off then isolates the widen's defect.

## Root cause (near-certain): detail-budget exhaustion

The accept-all test *inverted*: a ~170° terrain camera made the black dramatically worse, a
narrower one made it better. The frustum planes do not discard the black tiles — they *admit* quads
into fixed-size GPU buffers. The draw-list builder's tail (`sh_1434`) bumps global vertex/index/
texel cursors (`GlobalData` +24/+28/+32) with unbounded `imm_atomic_iadd` and no capacity check;
when the admitted quad set exceeds the buffers, the overflow writes go out of bounds and D3D11
silently drops them — the losing tiles end up with empty draw args and render black.

The budgets are the buffer sizes in the setup types' `Create` functions, sized for the flatscreen FOV and bound as [`TerrainDetailBudgetValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::TerrainDetailBudgetValues): [`VERTEX_COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::TerrainDetailBudgetValues::VERTEX_COUNT) detail vertices of 16 B, [`DEBUG_VERTEX_COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::TerrainDetailBudgetValues::DEBUG_VERTEX_COUNT) debug vertices of 80 B, [`INDEX_BYTES`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::TerrainDetailBudgetValues::INDEX_BYTES) of indices, and [`TEXEL_COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::TerrainDetailBudgetValues::TEXEL_COUNT) texels of 16 B (in `CRenderBlockTerrainDetailSetup::Create`), and [`INDEX_VIEW_COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::TerrainDetailBudgetValues::INDEX_VIEW_COUNT) entries in the 4-byte index view (in `CRenderBlockTerrainSetup::Create`). The patch sites of these immediates are [`TerrainDetailBudgetPatchSites`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::TerrainDetailBudgetPatchSites). VR's wide FOV admits roughly 2–3× the
quads within the 64/128 m detail radii and oversubscribes them. This explains every symptom:
mid-view victims (allocation order, not screen position), discrete angle-dependent flips (the
admitted set shifts with view), eye-identical (one shared build per frame), the widen doing nothing
(already overflowing either way), 170° being catastrophic, and flatscreen at default FOV fitting
just under budget.

**Fix (built, awaiting in-headset confirmation):** `terrain_detail_budget_scale` (default 4) with a
debug-UI "Apply" button — patches the five buffer-size immediates
(`TerrainDetailBudgetPatchSites` in the pyxis defs) to `shipped × scale` through the patcher
(auto-reverting on uninject) and re-creates the two setup types via their own `Recreate` with the
render engine's resource context ([`RenderEngine::m_ResourceContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_ResourceContext)) — the engine's own
settings-change path — processed at frame start alongside the shader-reload request.

## Outcome (the detail skin)

**Confirmed in-headset: scaling the detail budget fixes the black tiles** (even 2× sufficed in the
test scene; the shipping default is 4× for headroom, ~20 MB of VRAM). The shipping change is the
budget scale (a constant 4, applied once at the first frame start after injection) plus the render-block-type registry bisect panel that
found the owning block. The full diagnostic state used along the way — the LOD-clip/visibility/
terrain-cull forces, the heartbeat instrumentation, and the base-terrain probes — is preserved on
the `issue-40-investigation` branch.

## Recurrence under the single-pass collapse: the hull's view-dependent culls

The budget fix stands, but black gaps came back with the single-pass collapse — reported as
diamond/rhombus-shaped holes in the landscape at middle distance, "where one terrain render block
hands over to another". That is a *different* discard, one stage later: the terrain patch's own hull
shader.

### What the hull actually does (shader-verified)

Disassembling the terrain hulls out of `Shaders_F.shader_bundle` (`volumetricterrainpow2hullclip`,
blob 1498 — the family is 1492..1504) settles the mechanism. The patch-constant phase runs three
independent discards, all from `cb1`, and zeroes the tessellation factors when any fires:

1. **Back-patch cull**, when `cb1[0].w > 0`: `dot(n_i, ViewDir) < BackPatchCullThreshold` for all
   three control-point normals.
2. **Frustum cull**, when `FrustumCull > 0`: each control point, expanded `±radius` along its normal,
   is projected through `m_OffsetViewProjection`; the patch dies if all six results are outside the
   same clip plane (`x > w`, `x < -w`, likewise `y`, or `z < 0`).
3. **LOD clip** (the `*hullclip` variants only, and only for `Patch_LOD > 9`): four `VisibilityMask`
   taps at the patch corners, discard when all four read `< 0.05`. This is the "HullClipType"
   mechanism from the earlier pass — **residency-driven and view-independent**, so it is not the
   FOV-sensitive stage and no camera change can move it.

`cb1` is `SHullDomainTypeConsts`, baked by [`RenderBlockTypeTerrainPatch::SetupConstantBuffers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##func-SetupConstantBuffers) once per frame per constant-buffer slot: `ViewDir` = the *render* camera's normalized forward
axis, `.w` = the back-patch enable, which is set only in the reflection passes ([`RENDERSTATUS_REFLECTIONS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderStatus##flag-RENDERSTATUS_REFLECTIONS)) and only when `m_EnableBackPatchCulling` is on;
`m_OffsetViewProjection` = [`RenderContext::m_OffsetViewProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_OffsetViewProjection); `FrustumCull` = 1.0 unless the pass is a shadow cascade or one of the passes from `RP_TERRAINPATCH_BASEMESH_NEAR` to [`RP_TERRAINPATCH_BASEMESH_COLOR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_BASEMESH_COLOR), and gated on `m_EnableFrustumPatchCulling`. Both flags live on the type singleton `RenderBlockTypeTerrainPatch`, as [`m_EnableBackPatchCulling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##field-m_EnableBackPatchCulling) and [`m_EnableFrustumPatchCulling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##field-m_EnableFrustumPatchCulling), with the threshold in [`m_BackPatchCullThreshold`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeTerrainPatch##field-m_BackPatchCullThreshold). The threshold's shipped default is not confirmed: it is not set in the type's constructor or `Create`, and is likely data- or console-driven.

### Why a wide FOV breaks them

- The **back-patch** test approximates the view vector for a whole patch by a single camera axis, and the threshold allows only a small angular slack past perpendicular. At 45–55° off-axis — most of a headset's field of view — a patch that
  squarely faces the eye is still beyond that threshold from the axis, and is discarded while
  visible. This is FOV-sensitive in *any* VR mode, not just the collapse.
- The **frustum** test uses one baked matrix for both eyes (the CB is stamped once per frame per
  slot; that same caching is what `invalidate_terrain_cb` works around). Under the collapse the mod
  deliberately leaves the render camera centred, so that matrix is the engine's own projection built
  from the injected **90° vertical** FOV and the *double-wide* aspect —
  `Camera::RecalcProjection` → `CMatrix4f::PerspectiveFov(fov, aspect, …)` writes
  `m[1][1] = cot(fov/2)` and `m[0][0] = m[1][1] / aspect`, so `m_FOV` is vertical and the aspect only
  stretches the horizontal. Session logs confirm the aspect: the engine is resized to 4030×2240
  (`aspect_ratio=1.799`) when the collapse engages, versus 2015×2240 (`0.8996`) per eye. Horizontally
  that frustum is generous (~121°); **vertically it stays 90°**, while an eye's vertical FOV is
  larger and the ±5° display cant rotates it off-axis besides. Patches out there are discarded in the
  one walk, for both eyes.

A patch the hull discards is drawn in no pass, and the coarser tile over the same footprint is
independently removed by the LOD clip, so nothing backfills: far depth, dark resolve, patch-shaped
world-locked gaps that flip with head rotation — exactly the reported shape.

### The fix

`relax_terrain_patch_hull_culls` (config + a "Culling & geometry" checkbox, **on by default**, VR
only), in `payload/src/hooks/graphics_engine/terrain_cull.rs`: a detour on `SetupConstantBuffers`
clears `m_EnableBackPatchCulling`/`m_EnableFrustumPatchCulling` around the type's own bake and
restores them immediately after, so only the uploaded constants change — the engine's settings, and
the debug UI's view of them, are untouched. The LOD clip and the cull-by-detail term are left alone
(neither is view-dependent). Cost: the margin patches are tessellated and rasterized instead of
dropped, bounded by the CPU-side patch cull (already widened to the binocular union); back-facing and
off-screen triangles still die at the rasterizer and clipper.

**Falsifiable prediction.** With the checkbox off, the black patch gaps are present and shift in
discrete steps as the head turns; toggling it on refills them in the same frame, with no other change
in the scene (the terrain silhouette and LOD boundaries stay put) and a small drop in terrain
tessellation throughput. If the gaps survive with it on, the hull culls are exonerated and the
remaining suspect is the LOD clip's other half — a finer tile whose draw is missing for a non-hull
reason — which the "Show debug culling" flag (`m_ShowDebugCulling`, rasterizes culled patches with
`CULLFACE_NONE`) can then discriminate.

*(Not confirmed in-headset: the mechanism is established statically from the shader, the engine bake,
and the session logs; the repair is inferred from it.)*
