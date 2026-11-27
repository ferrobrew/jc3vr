# Visibility culling in VR

The engine culls against the centre camera, a symmetric frustum narrower than the two eyes' off-axis frusta combined, so geometry an eye sees past its edge is culled for both eyes, showing as black voids and pop-in at each eye's outer edge. The mod widens each of the engine's cull frusta to the binocular union and leaves the render projections alone. Every widen is VR-only: on flatscreen no union projection is published and each hook passes through.

The engine has several visibility gates against three cameras: the occluder manager's cull camera, the camera manager's active camera, and the landscape system's terrain camera ([model-culling.md](../../engine/rendering/model-culling.md), and [terrain.md](../../engine/rendering/terrain.md) for terrain). The hooks are in `crates/jc3vr-render/src/hooks/graphics_engine/scene/culling.rs` and `scene/terrain_cull.rs`, and the union projection in `crates/jc3vr-view/src/vr/frame.rs`. The tuning is the view tuning's `frame.cull_fov_padding` and the render tuning's `culling.size_cull_fov_deg` and `culling.spawn_budget_scale` ([tuning.md](../tuning.md)).

## The union projection

`begin_render_frame` builds one cull projection per VR frame and publishes it through `cull_projection_standard()`: the outermost tangent of the two eyes on each side, scaled outward by the padding, plus a margin of half the IPD over the near plane, with each half-angle clamped short of a degenerate frustum. The padding covers motion, since the engine culls at one interpolated pose and fast flight would otherwise pop geometry in at the edges; it applies vertically too.

## The scene cull

The broad-phase BFBC cull that terrain, models, streaming, and AO volumes share ([model-culling.md](../../engine/rendering/model-culling.md#1-instance-level-bfbc-cull-build-phase)) builds its frustum in [`OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::OccluderCollectionManager##func-GetBFBCFrustumParamsForCameraAndTime). The `get_bfbc_frustum_params` detour recognises the manager's own cull camera by address ([`OccluderCollectionManager::CULLING_CAMERA_OFFSET`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::OccluderCollectionManager::CULLING_CAMERA_OFFSET)) and writes the union over its `m_ProjectionF`. The write is not restored: the cull camera is the manager's per-frame copy, and the write is idempotent when parallel cull jobs re-enter. Two more main-view corrections ride on it:

- `culling.size_cull_fov_deg` sets the cull camera's `m_FOVT1`. BFBC's size cull scales by the field of view, and VR's camera context field of view is far wider than flatscreen's, so without this it drops small and distant geometry and vehicle parts. Only the size and AO-volume culls read it.
- [`BFBCFrustumCullParameters::m_FrustumCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::BFBCFrustumCullParameters##field-m_FrustumCount) is cut to the camera frustum alone, dropping the occluder frustums, because software occlusion casts silhouettes from the centre viewpoint and an offset eye sees past an occluder's edge. The cost is overdraw. The write is skipped if the count is out of range, which would mean the layout does not match.

## The spawn and character-occlusion frusta

`CSpawnSystem::Update` and the character-occlusion pass build their frusta from the active camera ([model-culling.md](../../engine/rendering/model-culling.md#related-spawn-visibility)), so the same detour writes the union over the active camera's projection for the duration of the call and restores it. The spawn system's "do not (de)spawn while visible" gate then covers both eyes. Under the wider view, an exceeded spawn budget despawns objects in front of the player, so `spawn_system_update` scales every budget once, on the first `Update`, through the patcher, which reverts on uninject.

## The per-render-block frustum cull

`CModelInstance::AddToRender` re-culls each render block against the active camera's frustum planes ([model-culling.md](../../engine/rendering/model-culling.md#2-per-render-block-frustum-cull-draw-list-build)), which pops large buildings out at the edges. The `camera_update_frustum` detour calls `Camera::UpdateFrustum` a second time, through the trampoline, for the active camera with `m_ViewProjection` set to `m_View * union`, then restores it, so only the planes widen. The same planes gate the instance fade, road meshes, and far lights, which therefore fade normally at the view edge instead of hiding.

## Terrain

Terrain patches are selected against [`STerrainPatchSystem::m_TerrainCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::STerrainPatchSystem##field-m_TerrainCamera), a per-frame copy of the active camera, so after [`TerrainPatchSystemUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::render_engine##func-TerrainPatchSystemUpdate) refreshes it, the `terrain_patch_system_update` detour writes the union onto it and rebuilds its view-projection and planes, every frame. The tessellated terrain also discards patches in its hull shader ([terrain.md](../../engine/rendering/terrain.md#the-hulls-discards)); in stereo, `terrain_patch_setup_constant_buffers` clears both enable flags around the bake, and the patch system's hulls, which test regardless of the flag, get a both-eyes frustum under single-pass ("The terrain hull cull" in [single-pass-per-eye-inputs.md](single-pass-per-eye-inputs.md)).

The detail rock skin allocates from fixed GPU buffers sized for the flatscreen view, and overflowing tiles render black ([terrain.md](../../engine/rendering/terrain.md#the-detail-skin)). At the first frame after injection, `scene::terrain` scales the buffer sizes by `terrain.detail_budget_scale` and re-creates the two terrain setup types.

Every widen changes visibility only. The sun-shadow cascade fit is widened separately ([lighting-shadow-vr-interactions.md](lighting-shadow-vr-interactions.md#the-shadow-lookups-anchor)).
