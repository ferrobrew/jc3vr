# #32 monoscopic far-field — distance-split mechanism investigation

RE groundwork for [#32](https://github.com/ferrobrew/jc3vr/issues/32): render the distant scene
once and share it between the eyes. The issue's first open question — *how* to split the scene by
distance when the engine cleaves passes by type, not range — has an engine-native answer: the
render-pass **sort machinery** already computes a per-instance camera distance every frame, and
supports quantizing it into per-pass **depth buckets** that become contiguous runs in the sorted
draw list. The concrete addresses and layouts are captured in the pyxis defs
(`graphics_engine/render_pass.pyxis`, `render_engine.pyxis`); this doc is the wiring and the
consequences for the mod.

The mod's far field built on this has been removed; [far-field.md](far-field.md) records what it did and where to find it.

## The sort machinery (engine ground truth)

Each pass's draw list is an array of [`RenderInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderInstance) (`SRenderInstance`) entries, each holding a sort ID, the render block, its instance info, the render-block type, and a depth key. Once per list rotation, [`RenderPass::SortList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SortList) (mislabelled `CRigidObject::Destruct` in the IDB, now renamed):

1. Recomputes each entry's keys through the block's virtual functions: [`RenderBlock::GetSortID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderBlock##vfunc-GetSortID) and [`RenderBlock::GetSqDistanceToCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderBlock##vfunc-GetSqDistanceToCamera), which returns the squared distance from the instance's buffered world translation to the sort camera's position.
2. Sorts by `(m_Depth, m_RenderBlockType, m_SortID)` with `std::sort`, per the pass's [`RenderPass::m_SortMethod`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_SortMethod): `SortID` (no depth key), `BackToFront` (descending raw distance), `FrontToBack` (ascending raw distance), or `FrontToBackBucketed` (ascending bucket index).
3. Latches the [`RenderPassSortState::m_Sorted`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPassSortState##flag-m_Sorted) flag in [`RenderPass::m_SortStateFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_SortStateFlags) under the pass spinlock, [`RenderPass::m_SortLock`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_SortLock); [`RenderPass::SaveRenderFrameData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SaveRenderFrameData) clears it at the per-frame rotation. **The sort therefore
   runs at most once per real frame and both stereo eye dispatches share the same sorted order.**

Two callers funnel into it: the render-thread sort task [`RenderEngine::SortRenderPasses`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SortRenderPasses), which builds the sort context from the pass's external camera if set, else the render camera, and a lazy call at the top of [`RenderPass::DoDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-DoDraw).

### Depth buckets

`m_SortMethod` defaults to `Auto`, which resolves to `FrontToBackBucketed` (or `BackToFront` for
alpha-blend passes). Bucketed sorting quantizes the squared distance through the pass's
[`RenderPass::m_DepthSqTable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_DepthSqTable) (a fixed-capacity table of ascending squared boundaries, with the live count in [`RenderPass::m_NumDepthBuckets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_NumDepthBuckets)) and
stores the **bucket index** in `m_Depth`. With a single bucket — the constructor default — no depth
key is computed and the sort degenerates to type-then-sortID batching.

**The only stock user of multiple buckets is the Z-and-velocity pass ([`RenderPassId::RP_Z_AND_VELOCITY_PASS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_Z_AND_VELOCITY_PASS)), with boundaries at 40 m and 120 m**. They are stored squared, as [`RenderPass::Z_AND_VELOCITY_DEPTH_SQ_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass::Z_AND_VELOCITY_DEPTH_SQ_NEAR) and [`RenderPass::Z_AND_VELOCITY_DEPTH_SQ_FAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass::Z_AND_VELOCITY_DEPTH_SQ_FAR), and the registration is inlined into [`RenderEngine::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-InitializeSystem). So the engine already ships, and
exercises, exactly the mechanism a far-field split needs: register a boundary, and the sorted draw
list becomes `[near bucket entries][far bucket entries]`, with type batching preserved *within*
each bucket.

## Implication for #32: a draw-list partition, not a depth composite

The proposed approach in the issue sketched a depth-threshold composite. The sort machinery offers
a cheaper primitive:

1. **Register a far boundary** on the scene passes (append `threshold²` to `m_DepthSqTable`, bump
   `m_NumDepthBuckets`, keep the table sorted — the engine's `AddDepthBucket` is inlined, so the
   mod does the three writes itself at pass-creation or reinit time).
2. After the frame's `SortList` has run, each pass's draw list is partitioned: entries with
   `m_Depth < farBucketIndex` are near, the rest far. The split index is found by scanning (or
   binary-searching) `m_Depth`, which is monotonic post-sort.
3. **Far phase (once, centre eye):** draw only `[split..end]` — e.g. by temporarily advancing
   `m_List`/reducing `m_NumElements` on the current draw list around `DoDraw`, or by a `DoDraw`
   detour that offsets the walk. Drawing is non-destructive, so this composes with the existing
   between-eye machinery.
4. **Near phase (per eye):** draw only `[0..split]` over the composited far backdrop.

Supporting facts, verified in the IDB:

- `DoDraw` walks `min(m_ListSize, m_NumElements)` entries linearly from `m_List` and never writes
  the list; window-clamping the walk is safe and both eyes can re-walk the same list.
- The distance is **instance-centre** distance (`CRBIInfo` world translation), not
  nearest-bounding-box distance. Large objects (bridges, big buildings) whose centre is beyond the
  threshold but whose extent reaches near the camera will land in the far bucket and render with
  zero disparity — the threshold must be conservative or the classification per-type refined
  ([`RenderBlock::GetBoundingBox`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderBlock##vfunc-GetBoundingBox) is available for a radius margin). Only
  `CRenderBlockWindow` and the particle blocks override `GetSqDistanceToCamera`.
- The sort camera for the main scene passes is the **render camera** — under stereo, whichever eye
  was live when the sort ran. At a 100 m+ threshold the IPD offset is negligible for
  classification.
- Back-to-front (transparency) passes keep raw distances in `m_Depth`, so the same threshold
  splits them as a suffix/prefix in reverse.

## Interaction with the engine's own 40 m/120 m buckets

Appending a boundary above 120 m to the Z-and-velocity pass gives it four buckets and leaves the
stock near ordering untouched. For passes currently running with one bucket, adding a boundary
*changes their draw order* from pure type-batched to two type-batched runs — more type switches
(one extra `ChangeRenderBlockType` run per type spanning the boundary), but the engine already pays
exactly this cost on the Z-and-velocity pass, so it is a known-safe ordering.

## Still open (not answered by this pass)

- **Deferred lighting over a composited far field.** The far phase must fully resolve lighting for
  far pixels; the per-eye near phase's `RP_DEFERRED_LIGHTS` must not re-light stale far G-buffer
  texels. Whether the light passes are stencil-bounded or light-volume-bounded needs its own dig
  ([`LightManager::InitDeferredLighting`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::light_manager::LightManager##func-InitDeferredLighting) is the entry point).
- **Terrain.** Base terrain (`CRenderBlockTerrain`) and terrain-patch blocks ride the same draw
  lists, so they partition by patch-centre distance like everything else, but patch sizes are
  large; the parallax bound may want a per-type threshold.
- **Bandwidth.** The far colour+depth copy/composite cost vs the geometry saved — empirical, via
  the RT-hash diagnostic and GPU profiler.
- The `m_RenderBlockType` int passed to `RBILists::Add` is the type index used as the second sort
  key; its exact registry semantics were not chased.

## Correction: how the visible terrain actually renders (empirical, 2026-07-17)

Live inspection and in-game bisects during the increment-1 build corrected two claims above:

- The `TERRAIN_APPLY_*` pass categories ([`RenderPassId::RP_TERRAIN_APPLY_NEAR_DETAILED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAIN_APPLY_NEAR_DETAILED), [`RP_TERRAIN_APPLY_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAIN_APPLY_NEAR), and [`RP_TERRAIN_APPLY_FAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAIN_APPLY_FAR)) hold **zero passes**. The claim in `40-terrain-black-tiles.md` that patch colour draws go to those three passes does not hold. The `BASEMESH_*_COLOR` passes ([`RP_TERRAINPATCH_BASEMESH_TESSELLATE_COLOR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_BASEMESH_TESSELLATE_COLOR) and [`RP_TERRAINPATCH_BASEMESH_COLOR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_TERRAINPATCH_BASEMESH_COLOR)) hold the patch
  blocks but windowing their draw lists has **no visible effect**: they are patch-space
  processing, not the screen G-buffer.
- The visible distant terrain is drawn by the **`VolumetricTerrainPatch` render-block type**, and
  it is *inherently far-regime*: near terrain hands off to other block types as the camera
  approaches. Disabling that type (registry `IsEnabled` kill switch) removes exactly the distant
  terrain. The same holds for `TreeImpostor` (distant trees), `TerrainForest`, `Occluder`, and `Window`.
  Known residuals accepted for the baseline: minor distant "bleed", and the non-ocean **water
  tiles**, which straddle the boundary and divide cleanly into neither near nor far — revisit
  after the sharing phase lands. The far-field
  terrain unit is therefore **type gating**, not a per-entry distance split; only the model-family
  passes (`RP_MODELS_*`, `RP_CREATURES`) split per entry.
- For the sharing increment: the far image relates to each eye by an exact 2D homography (same
  camera centre, different off-axis projection), so the per-eye composite is a full-screen warp of
  the shared far colour+depth — not a plain copy — with the IPD translation as the only residual,
  threshold-bounded error.
