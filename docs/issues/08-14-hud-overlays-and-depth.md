# HUD overlays and depth (issues #8 and #14)

Resolved: both issues are closed. The full-screen overlays are suppressed and the HUD's depth comes from the atlas ([hud.md, "HUD depth"](../mod/hud.md#hud-depth)). What remains here are the constraints that the deleted per-element depth split established; they bind any design that captures the HUD movie or edits its render tree. How the engine updates, captures, and draws the UI is in [ui-rendering.md](../engine/rendering/ui-rendering.md).

## Constraints established

1. One safe capture per frame. `Render::Context::Capture` mutates the active snapshot, which every game-thread UI write touches, with no writer lock; the capture lock only orders `Capture` against `NextCapture`. The engine is safe because it captures inside `CUIManager::PreRender` on the game thread. Extra captures from the render worker raced the UI writes, which showed as flicker, late updates, and a crash in `PrimitiveBundle::InsertEntry`.
2. The display side owns child indices. `GFx::DisplayList` addresses each child's render node by a cached `TreeIndex`, so a structural edit it does not know about corrupts later operations. Swapping a moved node for an empty "tombstone" container at the same index holds, because only structural operations are index-based; matrix, visibility, and alpha writes follow the node.
3. The renderer's cache reconciliation requires unique parentage. `TreeCacheContainer::HandleChanges` walks the child array and the cache list in lockstep and converges only when every node has exactly one parent. A node with two parents hangs the render thread inside the deferred render lock and deadlocks the game.
4. The render-buffer manager is not built for churn. `RBGenericImpl::RenderBufferManager` links a cache-list record and charges its budget for every `CreateRenderTarget`, so a target per draw slows the UI worker until the engine skips UI updates. Wrap targets once and reuse them.
5. Content projected from the world needs full-rate sampling. A layer refreshed below the frame rate strobes under camera motion, because its elements are projected with another frame's matrices.
6. Releasing clip handles while a visibility mask hides clips lets the new handles read the mask back as the game's intent. Restore the game's visibility before any release.

The last design, a partition of the HUD's render tree into extra `TreeRoot`s drawn from the frame's single capture by a reimplemented `CUIManager::Render`, respected all of these and ran well in gameplay, but the first pause permanently stopped the UI update pump (`MovieImpl::Capture` was never called again) with every lock and tree structure healthy. The cause was not found.
