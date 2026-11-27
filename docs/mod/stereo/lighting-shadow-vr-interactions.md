# Lighting, shadow, and camera interactions with the stereo double-draw

In stereo the mod drives the game's `Draw` more than once per real frame, and the engine advances per-frame state inside each `Draw`. This page lists each modification the mod makes to the camera, the frame counters, the lighting, and the shadows, and how it is handled between dispatches; check it when a per-frame value alternates or flickers in VR. The engine's frame pipeline is in [rendering.md](../../engine/rendering/rendering.md).

## The double-draw loop

`game_update_render` (`payload/src/hooks/game.rs`) calls `game.Draw` once per dispatch: two under multipass, in a session and on flatscreen, and one under the single-pass collapse ([single-pass-stereo.md](single-pass-stereo.md)), where the between-dispatch restores below do not run. The aim is that every dispatch makes the same per-frame decisions, so the scene advances once per real frame. Per real frame:

1. The simulation tick runs `ShadowManager::UpdateRender` (the cascade fit), the camera update, and animation; the mod's sim-side shadow and camera changes run here.
2. Before the first dispatch the driver snapshots the state the ledger marks as restored.
3. The first dispatch runs: the prologue advances the counters, `PreDraw` renders the shared prepasses, then the scene and post chain. The driver waits for the draw thread and drains its fragment.
4. Before each later dispatch the snapshots are written back, so its prologue advances from the same value and the net advance is one.

## Save and restore ledger

| Modification | Site | State touched | Between dispatches |
|---|---|---|---|
| Per-eye camera | `setup_render_camera` (`jc3vr_view::hooks::camera`) | Render camera matrices; publishes the shadow-anchor delta and the per-eye view-projection history | Rebuilt each dispatch; the history rotates on eye 0 only. |
| Frame counters | `snapshot_frame_counters` / `restore_frame_counters` | [`RenderFrameCounters`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderFrameCounters) (jitter phase, shadow-atlas parity) | Restored. |
| Shared prepasses | `pre_draw` (`jc3vr-render`, `hooks/graphics_engine/render_pass.rs`) | Clears [`RenderPassState::m_Enabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPassState##flag-m_Enabled) on the view-independent prepasses (`SHARED_PREPASS_CATEGORIES`) on later dispatches | The shadow atlas renders once and both eyes sample it; relies on the counter restore for the parity slot. |
| Shadow lookup anchor | `set_global_shader_constants` | Shifts the cascade transform's translation by the eye's offset and, with `stereo.shadow_anchor_to_fit`, the fit camera's | Re-staged each dispatch (below). |
| Shadow fit widen (VR) | `shadow_manager_update_render` | Widens the active camera's projection to the eyes' union for the fit (`vr::shadow_fit_projection_standard`), or the padded cull projection with `stereo.shadow_fit_unpadded` off | Saved and restored inside the sim-side call. |
| Shadow fit stabilization | `shadow_manager_update_render` | Flattens the camera forward to yaw for the fit, so head pitch and roll do not slide the cascades | Saved and restored inside the call. |
| SSAO | `ssao_draw` (`screen/ssao.rs`), `snapshot_ssao_history` / `restore_ssao_history_indices` | Forces `SSAOPass.m_FirstPass`; [`SSAOPass::m_PrevFrameIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::ssao::SSAOPass##field-m_PrevFrameIndex) and `m_CurrFrameIndex` | Each eye computes AO from its own depth; indices restored. |
| GI cascade | `snapshot_gi_cascade` / `restore_gi_cascade_index` | `GISolver.m_CascadeToUpdate` | Restored, so both eyes refresh one cascade. |
| `EffectInfo` | `snapshot_effect_info` / `restore_effect_info` | [`GraphicsEngine::m_EffectInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_EffectInfo) frame indices and `m_EffectInfoIndex`, the reflection proxies' depth history | Restored; without it the water reflections flicker. |
| Add-buffer parity | `get_current_add_buffer` (`game.rs`) | The `CKeep1000Frames` add-list parity | Restored, so the later dispatch clears the first's draw-time additions. |
| Post block dedupe | `render_block_post_effects_draw` (`post/post_effects.rs`) | `RenderBlockPostEffects::Draw` once per dispatch (`WORLD_POST_BLOCK_RAN`) | The add-list restore cannot remove the entry `ApplyWorldFilters` enqueues, so without it the post chain and FSR run twice. |
| Eye-1 `dt` | `apply_world_filters`, `apply_global_filters` | Passes `dt = 0` on eye 1 | Fades, sun direction, and heat haze step once per frame. |
| Eye-1 exposure | `post/tone_mapping.rs` | Skips the exposure smoother and histogram meters on eye 1 | Both eyes use eye 0's exposure. |
| Anti-aliasing mode | `anti_aliasing_apply`, and the jitter drop in `setup_render_camera` | With FSR or the mod's TAA, sets `AntiAliasingEffect.m_Mode` to none; with the engine's, drops SMAA T2X to 1x; drops the jitter whenever T2X is gone | Restored after each call. |
| Off-axis reconstruction (VR) | `perspective_fov_inverse` (`screen/reconstruction`) | Substitutes the eye's exact inverse for `Matrix4::PerspectiveFovInverse` | Per eye by design. |
| Terrain constant buffers | `invalidate_terrain_cbs` (`game.rs`) | Stamps the terrain block types' `m_WasCBApplied` stale before later dispatches | The bake is keyed on the restored frame number, so otherwise eye 1 reuses eye 0's distant-terrain projection. |
| Fragment drain | `drain_draw_thread_fragment` (`game.rs`) | Waits on `GraphicsEngine.m_DrawThreadWorkSignal` after each dispatch | A barrier so the restore does not race; the last drain moves to the tail worker when the tail is deferred. |

`stereo.shadow_update_every_frame` (off) zeroes [`ShadowManager::m_CascadeUpdateLevels`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##field-m_CascadeUpdateLevels) so every cascade re-renders every frame; it is a debug UI diagnostic for the cascade cadence's flicker (issue #31).

## Per-frame state the dispatches leave free

These alternate once per real frame by the engine's own cadence, or are not pinned, and without T2X to average two frames an alternation can show:

- [`RenderEngine::m_CurrBackBufferCopy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_CurrBackBufferCopy), the refraction source's ring index, advances in the `Draw` prologue outside `RenderFrameCounters` and is not restored, so under multipass it advances once per dispatch: the one global whose sequence the double-draw changes.
- The sun-shadow atlas splits its slices between the parities of [`RenderFrameCounters::m_FrameIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderFrameCounters##field-m_FrameIndex), and with the cascade amortization the coarse cascades in the two halves hold content from different frames.
- The exposure histogram's ping-pong, the GI cascade freshness, and the sim-side shadow fade.

## The shadow lookup's anchor

The cascade fit ([`ShadowManager::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##func-UpdateRender)) bakes the cascade transform relative to the active camera's position as it reads it ([`Camera::m_TransformF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_TransformF)), while the material shaders subtract the render camera's position before applying it. Any gap between the two shifts every sun shadow by the gap, the same in both eyes. The per-eye offset is one such gap, which the correction removes; `stereo.shadow_anchor_to_fit` also removes the gap between the render camera's centre and the fit camera, measured each frame. A shift that grows with speed points at this gap; the debug UI's Stereo section shows it.

Each cascade's sphere is sized from the fit camera's field of view, so the widen makes every cascade cover more of the world at the same resolution. `stereo.shadow_fit_unpadded` (on) fits the eyes' exact union rather than the padded cull projection, so the padding does not coarsen every cascade. Neither switch has been confirmed in a headset: grapple fast toward a parked car and watch whether its shadow stays under it with the gap readout visible.
