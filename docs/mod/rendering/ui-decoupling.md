# Decoupling HUD parts

Status: a proposal. Nothing in this document is built except the parked render-root split it reviews; the experiments are pending.

The HUD renders into one texture shown on one floating panel ([hud.md](../hud.md)). Several VR features want a part of it somewhere else: the reticle at the hand, button hints on the wrist, the weapon selection cross at the off hand ([weapon-selector.md](../weapon-selector.md)), markers and the centre group at their own depths (issue #14). This document reviews why the earlier attempts to split the HUD failed, records a latent alpha bug in the current composite, compares the techniques, and proposes one. How the engine captures and draws the UI is in [ui-rendering.md](../../engine/rendering/ui-rendering.md).

## Why the earlier attempts failed

The three depth-split mechanisms built for issue #14, their failures, and the constraints they established are in [the post-mortem](../../issues/08-14-hud-overlays-and-depth.md#the-split-post-mortem). Read together, they failed on time, not on edge quality. Scaleform hands the renderer one snapshot per capture, and a capture is safe only on the game thread inside `PreRender` (constraint 1). The multi-pass render captured from the render worker and raced the game thread; the time-multiplexed refresh spread its layers over several frames, so they disagreed under motion and strobed (constraint 5). The render-root partition respected the one-capture rule, but paid for it with structural edits to the render tree (constraints 2 to 4) and a reimplemented `UIManager::Render`, and one of those interactions wedges the UI on the first pause for reasons still unknown. No attempt got far enough to see premultiplied fringes, stencil leakage, filter bleed, or clear-colour mismatches; those classes are untested, not ruled out.

One lead on the pause wedge has not been followed: the engine's own off-screen render ([`RenderOffScreenTexturesImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-RenderOffScreenTexturesImpl)) calls the manager's `Submit` before popping each target, and the partition's `render_partitioned` (`payload/src/hud/split/roots.rs`) does not. Whether that matters on the UI's deferred context is untested.

## The alpha of the HUD texture

The HUD texture very likely holds premultiplied colour (see [ui-rendering.md](../../engine/rendering/ui-rendering.md#colour-alpha-masks-and-filters)), and the panel composites it as straight alpha: `hud/quad.rs` and `hud/warp.rs` blend with `SRC_ALPHA, INV_SRC_ALPHA` on colour, and the pixel shader returns the sample unchanged. If the texture is premultiplied, every partly transparent pixel is darkened by its alpha a second time, so antialiased text edges, glows, drop shadows, and fading elements come out dark. This already affects the single panel. A split would make it more visible, because a small part seen close up (a reticle at the hand, hints on the wrist) is mostly edge. The fix is `ONE, INV_SRC_ALPHA` on colour for every HUD quad, once experiment 0 confirms the texture is premultiplied.

## Techniques

(a) Render the movie once per part, with only that part visible. This cannot work here. A visibility change reaches the renderer only through a capture, a capture is safe only on the game thread in `PreRender`, and the renderer consumes only the latest snapshot, so several captures in a row at the seam would collapse to the last one. Capturing from the worker is the multi-pass mechanism that failed. Toggling visibility on the render side without a capture would leave the renderer's cached bundles stale, which shows as parts missing, ghosted, or a frame old.

(b) A second movie instance per part, kept in sync. Hosting is possible ([scaleform.md](../../engine/scaleform.md#standalone-movies-crendertotextureui)), but the game writes the HUD three ways: invokes by method name on the root movie, display-info writes through handles it holds into the root movie's objects (the markers, every frame), and engine-side tweens. Mirroring needs a handle translation between two object graphs, and the clone's ExternalInterface calls land on the same native handlers, doubling their side effects. Drift between the clone and the HUD (stale values, timelines out of phase, a frame of offset) is the likely result. It is worth it only for a part the mod would rebuild anyway, and then it is a mod-owned movie ([scaleform-movie.md](../scaleform-movie.md)), not a clone.

(c) One render into an atlas, with each part moved to its own cell for the capture. The proposal below.

(d) Cropping one render. A second quad samples one UV rectangle of the HUD texture, and the panel shader discards that rectangle. It is cheap and changes nothing in Scaleform, but whatever else the game draws into the rectangle comes along (markers crossing the reticle, pickups, the large button hint, centre notices), filters that spill past the rectangle are cut, and the part's sharpness is bounded by the HUD texture. A variant redraws the root per part through the HAL's user matrix into a small target, which adds resolution but not isolation and doubles the draw cost. Cropping suits parts that never overlap anything, such as the minimap, and is a stepping stone.

(e) The engine's render-to-texture UI. It hosts a separate movie in its own texture and is the engine's way of doing (b), with the same feeding problem.

(f) Two others. Reviving the partition gives true per-part textures and is proven in gameplay, but keeps the tree surgery and the pause wedge; building extra roots only for parts that are not pooled (the cross, `MCI_reticles`) would remove most of the reconciliation. It stays the fallback if the atlas cannot get room outside the stage. Suppressing a part and drawing a mod version in the world-space overlay gives full control at the cost of the most new code per part and of the game's look.

## Proposal: the atlas

One capture and one draw, into a larger HUD texture, with each part moved into its own cell of that texture for the capture and moved back straight after it.

In the existing detour on [`MovieImpl::CaptureImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##func-CaptureImpl), for each part: read its x and y through its cached handle ([`GetDisplayInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ValueObjectInterface##func-GetDisplayInfo)), write its cell's position in whole stage pixels, optionally with a scale ([`SetDisplayInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ValueObjectInterface##func-SetDisplayInfo)), call the original capture, and write the saved values back. The snapshot carries the cell positions; the display side, hit tests, ActionScript, the engine's tweens, and the next `Advance` all see the original layout. Each eye's `PreRender` repeats the same writes, so both per-eye captures agree. Each cell is then drawn on its own quad, and the panel samples only the stage cell.

How it stands against each constraint in the post-mortem:

- One capture per frame, on the game thread, at the seam the engine already uses, so constraint 1 holds by construction, and every part updates at the full rate (constraint 5).
- Only node-local matrix writes: no index shifts, no reparenting, and no tombstones (constraints 2 and 3). The mod's overlay and reticle suppression already makes display-info writes at the same seam every frame, across pauses, without trouble.
- No extra render targets (constraint 4), and the engine's own `Render` runs unchanged, so whatever wedged the partition on pause is not involved.
- A part in its own cell cannot be overdrawn by a marker or a centre notice, which is what cropping cannot avoid.
- Masks are all inside widgets, so they travel with the moved container, and the target is cleared every frame.
- The glyph and filter caches see the same transform every frame, so they settle; a translation does not change glyph raster size, and a fixed scale changes it once.

What it can still get wrong, and the mitigation for each:

- Filter bleed: glows and blurs extend past a part's bounds. Each cell gets a transparent gutter wider than the largest filter radius measured in that part.
- Bilinear bleed between cells: each quad's UV rectangle is inset by half a texel. There are no mips, so a part shown much smaller than its cell aliases; small parts get small cells, or are scaled to the size they are viewed at.
- Premultiplied fringes: every quad uses the premultiplied blend.
- Room outside the stage, the main unknown. The cells must lie inside the movie viewport or the D3D viewport clips them. The mod already sets the viewport ([`SetMovieViewport`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-SetMovieViewport)) apart from the layout inputs ([`m_CachedViewportWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_CachedViewportWidth) and its height, and [`ComputeSafeArea`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-ComputeSafeArea)), and the HUD already reflows to non-16:9 textures. The candidate is to enlarge the texture and the viewport by a strip while the layout inputs keep the stage cell's size. If the movie's scale mode and alignment move the stage instead, the cell positions come from the live [`MovieImpl::ViewportMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##field-ViewportMatrix). At worst, the stage cell shrinks with the safe area so the cells fit in the existing texture.
- Marker clamping: [`ClampToScreen`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-ClampToScreen) and the `Get2DInfo` hook must keep markers inside the stage cell, or edge-clamped markers wander into other cells. The hook already rewrites the view-projection and aspect, so it can clamp to the stage cell.
- Containers the engine moves every frame, such as the weapon selection cross, are fine as long as the read happens at the seam after them, which it does.
- A change of cell layout can draw a snapshot captured under the old layout for a frame, showing a part twice or not at all. The layout stays fixed for a whole HUD mode, or is tagged with a capture sequence number.
- A text field moved directly takes the text-rectangle path, so the container is moved instead; objects flagged as 3D take another branch and are avoided or checked.
- ActionScript that reads a moved part's position during `Advance` reads the restored value. Anything reading render-side bounds would see the cell position; none was found, but the search was not exhaustive.

## Experiments (pending)

Experiment 0 settles the alpha question. A one-shot readback of the HUD texture to a raw RGBA image (the staging-copy and writer-thread pattern of `payload/src/screenshot.rs`), with the alpha saved as its own image, and the maximum of `max(r, g, b) - a` over all pixels logged. If that is not above zero beyond rounding, the texture is premultiplied and the panel blend becomes `ONE, INV_SRC_ALPHA`. A close-up of HUD text in the F12 screenshot under both blend states confirms it.

Experiment 1 moves one part to a cell: `MCI_safe_area_center.MCI_reticles`, shown on a small quad at the dominant hand. It is static, not pooled, sits in the busiest area of the HUD, and the mod already resolves handles for its children (`RETICLE_CLIPS` in `payload/src/hud/scaleform.rs`).

1. A viewport probe with nothing moved: enlarge the HUD texture by a strip, set the movie viewport to the full size while keeping the layout inputs at the stage size, screenshot the raw texture, and log [`MovieImpl::Viewport`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##field-Viewport) and `ViewportMatrix`. It passes when the stage cell looks as before and the strip is empty.
2. Resolve a handle for `MCI_reticles` with the same discovery that serves `RETICLE_CLIPS`.
3. In the capture detour, in the gameplay HUD mode only: save x and y, write the strip cell's position, call the original, restore. This needs the x and y bits of `DisplayInfo::VarsSet` bound in pyxis first.
4. Composite: the panel samples only the stage cell; a new quad samples the cell, inset by half a texel, at the hand, with the premultiplied blend.
5. Debug-UI toggles for the move, the restore, and the gutter size.

What to log and capture: the capture-seam telemetry line plus the counts of moves, restores, and handle failures, with the capture count unchanged from vanilla; the raw texture with the cell rectangles drawn into a copy, to see bleed, clipping, and markers entering the cell; a read-back of x and y at the start of the next seam, asserting the saved value, which proves the game never sees the move; F12 screenshots during fast turns with markers on screen, while firing (the reticle's bloom animation), and across pause and unpause; and a recorder session ([capture.md](../capture.md)) under motion.

It passes with no flicker or one-frame pops, the reticle animating at the full rate, no dark edges, nothing else in the cell, the UI alive across pause and unpause, and capture counts unchanged.

## Work after the experiments

1. pyxis: bind the x, y, and scale bits of `DisplayInfo::VarsSet` and the UI manager's system-ready flag.
2. An atlas layout type: cells with gutters, a stage cell, and per-part transforms (a translation and an optional scale), fixed per HUD mode.
3. A `hud/atlas/` module at the capture seam that moves, captures, and restores, holding each part's handle and saved position, with the handle registry generalised to arbitrary clip paths (the minimap, the hints stage, the objective text, health).
4. The composite: one quad per part with its own anchor (hand, wrist, panel, or depth), all premultiplied; the panel samples only the stage cell.
5. `Get2DInfo` clamps markers to the stage cell.
6. Debug UI: a cell overlay, per-part toggles, and gutter and scale sliders.
7. Retire `hud/split/` once the atlas covers the depth layers too; the marker layer is then the stage cell.

## Confidence

Medium-high that the earlier artifacts came from snapshot timing and tree surgery rather than edge quality, since the post-mortem says so directly. High that the atlas's per-frame mechanics are safe: one seam, one capture, and node-local writes of a kind the mod already ships. Medium that room outside the stage can be put in the viewport without disturbing the layout, which depends on the movie's scale mode and alignment; the fallback of shrinking the stage cell works either way. Medium that the texture is premultiplied, which experiment 0 settles.
