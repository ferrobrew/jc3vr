# The monoscopic far field (removed)

The far field (issue #32) rendered the distant scene once per frame and shared it between the eyes. It was removed in the deep-cut cleanup. The last commit with the full implementation is `dd03f03fbaab29788361768ecd7cc9efe84ad5a0`: `git show dd03f03fbaab29788361768ecd7cc9efe84ad5a0:docs/mod/stereo/far-field.md` has its design doc, and `git checkout dd03f03fbaab29788361768ecd7cc9efe84ad5a0` restores the code. The engine findings it was built on stay in [32-monoscopic-far-field.md](32-monoscopic-far-field.md).

## What it did

The code lived in `crates/jc3vr-render/src/far_field/` (`mod.rs` and `share.rs`), with its settings in `crates/jc3vr-view/src/far_field/`, a composite pixel shader (`far_field_composite_ps`), a debug UI feature (Experimental > Far field), and a summary line in the performance readout. It was off by default (`far_field.enabled`).

- Classification. On the model-family passes (`RP_MODELS_*`, `RP_CREATURES`) the `RenderPass::DoDraw` detour appended a depth-bucket boundary at a distance threshold (250 m by default) plus a sentinel, so the engine's once-per-rotation sort produced a contiguous `[near][far]` draw list, and narrowed the list header to one run for the walk. The inherently far block types (`VolumetricTerrainPatch`, `TreeImpostor`, `TerrainForest`, `Occluder` by default) were gated whole by pointing their `IsEnabled` vtable slots at a stub that followed the mode and the dispatch, synced once per frame from the configured list.
- Dial-in modes: `Collect` (counters only, the default mode), `SkipFar`, `SkipNear`, and `SkipFarEye1`.
- `Share` mode: a third, far-only dispatch at eye 0's pose drew the far runs and the gated types into the G-buffer, skipped the scene and post ranges, and had its MainDepth and GBuffer0..3 captured at the end of the G-buffer range. Each eye's near dispatch then split its G-buffer range at `RP_ROAD_STENCIL`, composited the captured G-buffer there through a per-axis affine warp (a plain copy for eye 0) with a `GREATER_EQUAL` depth merge, and drew its near runs on top. Lighting stayed per eye, so the saving was geometry only.

## Why it was removed

- It cannot run with single-pass stereo, which is the default. The collapse renders both eyes from one dispatch, so there is no per-eye dispatch for a far G-buffer to be shared between, and the share's own range splitting bypassed the single-pass G-buffer marking, leaving `cb13` mirrored and the viewport unsplit (both eyes got the same image cropped in half). Turning single-pass on moved `Share` back to `Collect`, and the settings migration did the same for old files.
- Its own settings doc recorded that `Share` produced severe compositing artifacts in VR and was opt-in until that was resolved.
- The near/far split did not follow the render order cleanly:
  - The composite injected between the clear and Z-prepass passes and the geometry passes. The Z passes were not windowed, so both dispatches prepassed far models, and equal depths took the far G-buffer content.
  - Gated types rendered only in the far dispatch's G-buffer range, so a transparent type could not be gated: `Window` (car and building glass) vanished entirely when gated, and transparent far content stayed per eye.
  - The classification is by instance-centre distance, so large objects whose centre lies past the threshold but whose extent reaches near landed in the far run, and the non-ocean water tiles straddled the boundary.
  - Gated far types carried zero velocity, and the composite did not carry the far G-buffer's stencil bits.
- Its cost stayed in the default path while it was off: the `DoDraw` detour called into it on every pass walk, the frame top synced the type gates, and the draw-thread hooks read the far-phase flag per dispatch and per post-draw capture.

## What a revival would have to solve

- A single-pass form. The collapse draws once into a double-wide target with each eye in a half, so a shared far image would have to be composited into both halves, each with its own eye's warp, within the one dispatch or in a dispatch added ahead of it, without bypassing the single-pass G-buffer range marking.
- The warp was per-axis affine, which holds only for parallel eyes with a shared centre. Canted displays need the full homography `H = P₁ · ΔR · P₀⁻¹`.
- Coverage: the far image was eye 0's frustum, so eye 1's outer edge sampled outside it. The far projection needs the union of both eyes' frusta, which the mod already builds for the culls and the shadow fit.
- The open items the design doc listed: whether `RP_CLEAR` clears only the G-buffer colours, FSR and velocity behaviour over composited far content, the stencil bits, transparent far content (a per-entry split on the back-to-front passes, whose sort keys are raw distances), the water tiles, and sharing the lit far colour instead of the G-buffer (which needs the deferred lighting resolve masked away from far-only pixels).
