# FSR anti-aliasing

The game's own anti-aliasing is SMAA, with an optional T2X temporal variant. In stereo the mod runs SMAA 1x, never T2X, whose shared history would ghost across the two eye dispatches (`anti_aliasing_apply`). AMD's FidelityFX Super Resolution 2 (FSR) replaces it by default: a temporal reconstruction pass, run at 1:1 as anti-aliasing. The mod's own lighter resolve, which runs at the same hook from the same inputs, is in [taa.md](taa.md). The engine side (the post-effects chain, the buffers, and the stereo double dispatch) is in [rendering.md](../../engine/rendering/rendering.md).

## Why FSR, and which version

DLSS is NVIDIA-only and its license excludes this use. FSR 4 is an ML model gated to AMD hardware. XeSS's D3D11 backend is a shim into a driver component that only Intel's Windows driver has, and its cross-vendor path needs D3D12 and Shader Model 6.4; reaching it would take a D3D11on12 bridge (OptiScaler reports a 10 to 15% penalty), a second device and submission timeline, and a proprietary binary that may not be modified. XeSS's one advantage here is that its dispatch takes sub-rectangle offsets, so it could resolve the two halves of the single-pass target in place where FSR 2 copies them out ([under single-pass](#under-single-pass-stereo)). The question is worth reopening only if Intel Arc becomes a first-class target or those copies become the binding cost.

FSR is MIT-licensed, runs on every vendor, and has a Direct3D 11 path. The mod ships FSR 2.2.1 through the MIT community DX11 port (`optiscaler/FidelityFX-FSR2-DX11`), vendored and wrapped by the `fsr-sys` crate. FSR 3.1 is the newest version that is still plain compute, and the candidate for an upgrade, but its DX11 backend is less mature, and the 2.2.1 port builds, unpacks its shader permutations, and dispatches reliably under Proton.

## The inputs

Every input FSR needs exists on the `GraphicsEngine` singleton and is valid at the start of the post-effects block:

- Colour: the post-tonemap image in the chain's current slot (see the next section).
- Depth: `m_MainDepthTexture` (`MainDepthBuffer`, D32FS8, reverse-Z). FSR is told the depth is inverted and infinite.
- Motion vectors: `m_VelocityBufferTexture` (`motion_blur_velocity_buffer`), the per-object screen-space velocity the velocity pass writes, which needs a decode pass ([motion vectors](#motion-vectors)).

The one input the mod builds itself is a per-eye previous-frame view-projection, because the engine snapshots a single previous view-projection in the simulation path (`Camera::UpdateRender`), not per dispatch. The `SetupRenderCamera` hook snapshots each eye's view-projections into `jc3vr_view::stereo::VpHistory`.

The context is created with FSR's auto-exposure and without motion-vector jitter cancellation, display-resolution motion vectors, or high dynamic range. No reactive or transparency mask is passed and FSR's automatic reactive mask is off: particles, the water, and other alpha-blended surfaces rely on FSR's own shading-change detection. FSR takes a single vertical field of view to linearize depth, read from each eye's projection; a headset's asymmetric frustum makes that an approximation, and depth-reconstruction artifacts at the frustum edges under strongly canted projections would point at it.

## Where it dispatches

FSR dispatches post-tonemap, at the engine's anti-aliasing hook, as a drop-in for SMAA. The `anti_aliasing_apply` detour feeds FSR the LDR image SMAA would read, in the post-effects manager's slot ring (through its texture arrays at [`PostEffectsManager::SRGB_RESULT_TEXTURE_SLOT_BASE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::SRGB_RESULT_TEXTURE_SLOT_BASE) and [`PostEffectsManager::RESULT_TEXTURE_SLOT_BASE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::RESULT_TEXTURE_SLOT_BASE)), resolves into the mod's own output texture, and copies it back into the slot. The image is already exposed, so no exposure input is needed; the cost is that FSR cannot recover highlight detail the tonemapper compressed. FSR's canonical position before the tonemap is driven by upscaling, which must run before the resolution-dependent effects; at 1:1 only the highlight argument remains. The sun halo and the fade render on top un-anti-aliased, which does not matter for full-screen overlays. While FSR or the mod's TAA runs, the detour drops [`AntiAliasingEffect::m_Mode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect##field-m_Mode) to [`AAMode::AA_NONE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AAMode##variant-AA_NONE), the passthrough branch of `Apply`.

The render size is always the display size. FSR's anti-aliasing and upscaling modes differ only in `renderSize < displaySize`, so the integration carries both sizes. Upscaling would also need the render size decoupled from the OpenXR swapchain size (both come from `vr::eye_resolution`) and a display-size resolve target, which cannot go back into the render-size slot ring.

`crates/jc3vr-render/src/fsr/mod.rs` holds one `fsr_sys::Context` and one output texture per eye, recreated whenever the render resolution changes, behind `FSR_STATE` on the render thread.

## Choosing the resolve

`fsr.antialiasing` picks the resolve at the engine's anti-aliasing stage: `Fsr` (the default), `Taa` ([taa.md](taa.md)), or `Engine` (the game's SMAA 1x in stereo, with no jitter). The debug UI's stereo section has the three as a row of choices, with the switches for the stereo correction, the hybrid decode, and the jitter beside them. A settings file that still holds the older `fsr.enabled` switch is migrated on load by the payload's `config::format`.

## Motion vectors

FSR reprojects the last frame's history through the current frame's motion vectors, so a wrong motion-vector path is the dominant source of ghosting and smearing, and head motion leans on it hardest.

### The engine's encoding

The velocity buffer (an 8-bit four-channel target) does not store raw motion vectors. The velocity-write pixel shader (`rendervelocitybuffer`) stores

```
stored.xy = clamp((curUV - prevUV) * 8, -1, 1) * 0.5 + 0.5
```

with `curUV` and `prevUV` in UV space, Y down: bias-encoded with 0.5 as zero motion and clamped to plus or minus 0.125 UV per frame. The disassembly is in [shaders.md](../../engine/rendering/shaders.md#the-velocity-passs-motion-vectors). FSR's `motionVectorScale` is a pure multiply and cannot subtract the bias, so the mod decodes it:

```
motion_uv = (stored.xy - 0.5) * 0.25     // = curUV - prevUV
```

Zero motion falls between two codes (one code is 3.5 pixels at the headset's width), so the decode reads the clear value as no motion and a drawn pixel within one code of the camera's motion as the camera's alone. The mod decodes the engine's buffer rather than computing its own because FSR needs object motion, which needs each object's previous world matrix, and the velocity pass is the engine's one materialization of it.

A compute shader (`crates/jc3vr-render/shaders/velocity_decode.hlsl`, driven by `fsr/velocity_decode.rs`) writes an `R16G16_FLOAT` buffer that FSR reads with `motionVectorScale = (renderWidth, renderHeight)`. FSR 2 wants the previous position less the current one, so the decode negates both axes (`MV_SIGN` in `fsr/mod.rs`).

### The stereo correction

The engine's velocity pass computes `curUV` with the per-eye view-projection but `prevUV` with the single previous view-projection of the centre camera, so in stereo every static pixel carries a spurious motion equal to the eye's parallax from the centre, opposite in each eye. FSR would mis-reproject each eye's history by that amount, which shows as per-eye shimmer on static edges.

The decode corrects it (`fsr.stereo_mv_correction`, on). The CPU forms the clip-to-previous-clip matrices in `f64` (`prevVP * inv(curVP)`; the world-scale translations cancel only at double precision), and the shader reconstructs each pixel from depth, reprojects it with both, and adds `prevUV_center - prevUV_eye`, so object motion survives and only the camera term is swapped. The "centre" matrix is the one the velocity pass actually encoded against: the render camera's `m_PreviousViewProjectionF` for an eye rendered whole, and that matrix taken through the single-pass velocity rewrite's map for a half of the single-pass target (`encoded_previous_view_projection`).

### The hybrid decode

The velocity pass draws only what is enqueued on it, in practice the vegetation and a few hardware-instanced models, so every other pixel decodes to zero motion and FSR would reproject the static world as if the camera had not moved. The hybrid decode (`fsr.hybrid_velocity`, on) gives an undrawn pixel, and a drawn one whose motion saturated at the clamp, the camera's motion: reconstructed from depth and reprojected through the eye's own clip-to-previous-clip matrix, unclamped and at full precision. A pixel beyond about a hundred metres can hold the clear value although drawn, which is harmless because its motion is the camera's. The same matrices serve multipass and single-pass, canted and asymmetric eyes included, and the sky's infinite depth reprojects by the rotation alone. This removes the 8-bit format's and the clamp's limits for camera motion; object motion keeps them.

With no history (the first frame, a rebuilt context) the decode sends no motion and FSR resets. A camera cut, the view centre landing more than the render tuning's `camera_cut.ndc` away in the previous frame (`camera_cut` in `fsr/mod.rs`), resets that eye's history too. Near geometry the velocity pass does not draw that moves with the head (the player's body and weapon) gets the camera's motion instead of its own, which FSR's disocclusion handling has to catch.

### Under single-pass stereo

The single-pass collapse renders both eyes side by side into one double-wide target and runs the post chain once. `single_pass.fsr_per_eye` (on) resolves each half through its own eye's context: the decode reads the eye's window of the velocity and depth buffers and copies that depth into an eye-sized R32F texture (a depth-stencil resource cannot be copied by region), the eye's half of the colour is copied into an eye-sized input, and the result is copied back. Each eye keeps its own history with nothing reaching across the seam. The correction is taken from each eye's own matrices, since the collapse keeps the render camera centred on double-wide matrices. "The shipped settings under single-pass" in [single-pass-collapse-defects.md](../stereo/single-pass-collapse-defects.md) covers the collapse's side.

## Sub-pixel jitter

At the display size FSR gains on a single-sample render only when every frame samples the scene at a different sub-pixel position; without that it is a temporal filter. The mod jitters each eye's projection every VR frame and tells FSR the offset (`fsr.jitter`, on).

The engine's own jitter ([`RenderEngine::ApplyJitterTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-ApplyJitterTransform), from [`Camera::SetupRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetupRenderCamera)) translates both the projection and the previous projection by two alternating samples, only for the scene render camera and only at the SMAA T2X mode, which stereo never runs. The mod keeps it off and drives its own.

The mod drives FSR's Halton(2, 3) sequence (`ffxFsr2GetJitterOffset`), eight phases at 1:1. One offset per VR frame is chosen on the game thread when the frame's per-eye render parameters are published (`jc3vr_view::fsr::jitter::frame_offset`) and carried with them (`EyeRenderParams::jitter`), so every pass of the frame and the FSR dispatch read the same offset, each eye in its own pixels. An offset `(x, y)` in pixels moves the content by that much: `T(2x / width, -2y / height)` pre-multiplied onto the projection, with the width the eye's in both modes, scaled by the size FSR last resolved at (`jitter::publish_eye_size`).

The jitter is folded into the per-eye projections at their source, so it reaches every pass that rasterizes or reconstructs an eye image: in multipass each eye's render camera and every constant built from it, and in single-pass the `cb13` rows, the eye globals, the per-eye pixel buffers, the reconstruction inverses, and the velocity rewrite's map, where it cancels. What must not see it keeps the projection as it was before the jitter, bit for bit (`EyeRenderParams::unjittered_projection_reverse_z` and its siblings): the visibility cull, the terrain cull matrix, and the cascade fit; what draws after FSR has resolved the eye (the HUD panel and its markers, the controller overlay, and the scope lens), which hold still while the scene jitters under them, though their depth-tested edges can move by half a pixel; the foveation's mask centre; and the motion vectors.

FSR wants motion without jitter, and the mod does not set `FFX_FSR2_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION`. The hybrid decode reconstructs through the jittered current matrix and reprojects into the previous frame with this frame's offset applied to the previous view-projection, `M = T(j) P C^-1 T(-j)`, the engine's own construction (`jitter::jitter_free_reprojection`). The engine-written velocity carries a fraction of a code of offset, which the stereo correction takes back out exactly (`jitter::reprojection_from_jittered`). The view-projection history holds every matrix without the jitter, with the offset beside it (`VpHistory::cur_jitter`): a jitter folded into an `f32` world-space matrix thousands of metres from the origin rounds differently every frame, which would hand FSR several pixels of motion with the camera still.

There is no jitter unless `fsr.antialiasing` is temporal (`AntiAliasing::temporal`) and `fsr.jitter` is on, and none outside a VR frame. In the capture harness the sequence restarts at every FSR history reset (`fsr::request_history_reset`) and as a motion begins ([fast-comparison.md](../testing/fast-comparison.md)), so every mode compares at the same offset.

## Sharpening

FSR's RCAS sharpening is off by default (`fsr.sharpen`, with its strength `fsr.sharpness` at 0.2 beside it). With the jitter, FSR's resolve is already sharper than SMAA against a supersampled reference, and every step of sharpening moves the image further from it, adds spurious gradient, and makes a still image change more from frame to frame. The switch and the strength are on the pause menu's display page and in the debug UI.

## What the harness measures

The capture harness checks the integration offline ([fast-comparison.md](../testing/fast-comparison.md); the scenes are in `tools/rendering/suites/fsr.toml`). `python3 -m tools.rendering fsr-motion` reprojects each pixel from the depth FSR read through the recorded matrices and compares it with the decoded vector: the camera motion FSR is handed is right to within 0.01 pixels on average (0.12 at most) in both modes, rotating, translating, and under the canted `index` rig, and exactly zero with the camera still. `fsr-jitter` confirms that the image FSR is handed moves by the told offset with the told sign. `fsr-quality` scores each method against the left eye rendered alone at twice the pixels with no anti-aliasing.

The standing results, at the headset's size:

- With the jitter, FSR is the closest to that reference in nearly every scene, sharper at the edges than SMAA, and the only method that draws a power line as a continuous line and a dithered tree crown as foliage. The jitter is the whole difference at rest and costs nothing measurable.
- FSR costs 1.9 to 2.1 ms of GPU time a frame. SMAA with 15% more pixels each side, or no anti-aliasing at 18% more, costs the same; those are steadier on a slow pan and keep stair-steps on held edges. Which reads worse at the panel's pixel size needs the headset.
- Its price is temporal: at a still camera, 2 to 5% of an eye's pixels change by a fraction of a code from frame to frame, against almost none with SMAA.
- FSR's history takes about 64 frames to settle after a reset, so a still-camera comparison with FSR on needs about 120 warm-up frames.
- A single motion snapshot lands on one of the eight jitter offsets, and FSR's score in motion swings by 1.5 dB with the offset's vertical sign, for reasons not established. Motion scores are averaged over all eight phases (`fast --motion-phase` with `--next-frame-repeat`).
- Under multipass and the isolated eyes, the pixels the velocity pass draws (the grass above all) reach FSR with 3 to 13 pixels of false motion, a per-eye parallax the encoded previous view-projection does not account for, and the jitter turns it into flicker on that geometry. Single-pass decodes them exactly. The harness's held camera may be the cause; it needs the headset, or the multipass encoding understood.
