# FSR anti-aliasing

The game's own anti-aliasing is SMAA, with an optional T2X temporal variant. In stereo the mod runs SMAA 1x, never T2X, whose shared history would ghost across the two eye dispatches (`anti_aliasing_apply`). AMD's FidelityFX Super Resolution 2 (FSR) replaces it by default: a temporal reconstruction pass, run at 1:1 as anti-aliasing. The measurements behind the shipped configuration are in [fsr-measurements.md](fsr-measurements.md), and the mod's own lighter resolve, which runs at the same hook from the same inputs, is in [taa.md](taa.md). The engine side (the post-effects chain, the buffers, and the stereo double dispatch) is in [rendering.md](../../engine/rendering/rendering.md).

## Why FSR, and which version

DLSS is NVIDIA-only and its license excludes this use. XeSS's cross-vendor path is D3D12-only, and its D3D11 backend runs only on Intel hardware ([upscaling-evaluation.md](upscaling-evaluation.md)). FSR 4 is an ML model gated to AMD hardware. FSR is MIT-licensed, runs on every vendor, and has a Direct3D 11 path.

The mod ships FSR 2.2.1 through the MIT community DX11 port (`optiscaler/FidelityFX-FSR2-DX11`), vendored and wrapped by the `fsr-sys` crate. FSR 3.1 is the newest version that is still plain compute with no ML hardware gate, but its DX11 backend is less mature, and the 2.2.1 port builds, unpacks its shader permutations, and dispatches reliably under Proton.

## The inputs

Every input FSR needs exists on the `GraphicsEngine` singleton and is valid at the start of the post-effects block:

- Colour: the post-tonemap image in the chain's current slot (see the next section).
- Depth: `m_MainDepthTexture` (`MainDepthBuffer`, D32FS8, reverse-Z). FSR is told the depth is inverted and infinite.
- Motion vectors: `m_VelocityBufferTexture` (`motion_blur_velocity_buffer`), the per-object screen-space velocity the velocity pass writes. The engine's encoding needs a decode pass ([motion vectors](#motion-vectors)).

The one input the mod builds itself is a per-eye previous-frame view-projection, because the engine snapshots a single previous view-projection in the simulation path (`Camera::UpdateRender`), not per dispatch. The `SetupRenderCamera` hook snapshots each eye's view-projections into `jc3vr_view::stereo::VpHistory`, and the decode pass re-anchors each eye's vectors at that eye's own previous pose without touching engine state.

## Where it dispatches

FSR dispatches post-tonemap, at the engine's anti-aliasing hook, as a drop-in for SMAA. SMAA runs after the HDR-to-LDR tonemap, on the LDR texture in the post-effects manager's three-slot ring. The `anti_aliasing_apply` detour feeds FSR that same image, in the slot SMAA reads (through the manager's texture arrays at [`PostEffectsManager::SRGB_RESULT_TEXTURE_SLOT_BASE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::SRGB_RESULT_TEXTURE_SLOT_BASE) and [`PostEffectsManager::RESULT_TEXTURE_SLOT_BASE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::RESULT_TEXTURE_SLOT_BASE)). FSR resolves into the mod's own output texture, which is copied back into the slot. The image is already exposed, so no HDR flag or exposure input is needed. The cost is that FSR cannot recover highlight detail the tonemapper has already compressed. The effects after the anti-aliasing stage (the sun halo and the fade) render on top un-anti-aliased, which does not matter for full-screen overlays.

FSR's canonical position, on HDR colour before the tonemap, is driven mostly by upscaling: an upscaler must run before the resolution-dependent effects. At 1:1 only the highlight-quality argument remains.

While FSR or the mod's TAA runs, the same detour drops [`AntiAliasingEffect::m_Mode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect##field-m_Mode) to [`AAMode::AA_NONE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AAMode##variant-AA_NONE), the passthrough branch of `Apply`.

The render size is always the display size. FSR's anti-aliasing and upscaling modes are the same context, inputs, and dispatch, differing only in `renderSize < displaySize`, so the integration carries both sizes as parameters. Upscaling would also need the render size decoupled from the OpenXR swapchain size (both come from `vr::eye_resolution` today) and a display-size target for the resolve, which cannot go back into the render-size slot ring.

`crates/jc3vr-render/src/fsr/mod.rs` holds one `fsr_sys::Context` and one output texture per eye, recreated whenever the render resolution changes, behind `FSR_STATE` on the render thread.

## Choosing the resolve

`fsr.antialiasing` picks the resolve at the engine's anti-aliasing stage: `Fsr` (the default), `Taa` ([taa.md](taa.md)), or `Engine` (the game's SMAA 1x in stereo, with no jitter). The debug UI's stereo section has the three as a row of choices, with the switches for the stereo correction, the hybrid decode, and the jitter beside them. A settings file that still holds the older `fsr.enabled` switch is migrated on load by the payload's `config::format`: `false` becomes `Engine`, and `true` the default.

## Motion vectors

FSR reprojects the last frame's history through the current frame's motion vectors, so a wrong motion-vector path is the dominant source of ghosting and smearing, and VR head motion leans on it hardest.

### The engine's encoding

The velocity buffer (`motion_blur_velocity_buffer`, an 8-bit four-channel target) does not store raw motion vectors. The velocity-write pixel shader (`rendervelocitybuffer` in `Shaders_F.shader_bundle`) computes, per pixel:

```
stored.xy = clamp((curUV - prevUV) * 8, -1, 1) * 0.5 + 0.5
```

where `curUV` and `prevUV` are the current and previous screen positions in UV space, Y down. The buffer is therefore bias-encoded into [0, 1] with 0.5 as zero motion, scaled by 8, and clamped to plus or minus 0.125 UV per frame. The motion-blur filter (`compositehighprecisionvelocityfilter`) decodes it with the exact inverse. The disassembly of both is in [shaders.md](../../engine/rendering/shaders.md#the-velocity-passs-motion-vectors).

FSR's `motionVectorScale` is a pure multiply and cannot subtract the bias, so the mod decodes the buffer itself:

```
motion_uv = (stored.xy - 0.5) * 0.25     // = curUV - prevUV, UV space, Y down
```

One code of the encoding is a quarter of a 255th of the screen, 3.5 pixels at the headset's width, and zero motion falls between two codes, so a still pixel the velocity pass drew holds half a code of motion either way, as does the clear value. The decode reads the clear value as no motion at all, and a drawn pixel whose motion is within one code of the camera's own (see the correction below) as the camera's alone.

The decode is the engine's own buffer rather than a buffer the mod computes, because FSR needs object motion (vehicles, characters, vegetation), and that needs each object's previous world matrix. The velocity pass is the engine's one materialization of it.

### The decode pass

A compute shader (`crates/jc3vr-render/shaders/velocity_decode.hlsl`, driven by `fsr/velocity_decode.rs`) reads the velocity buffer, applies the decode, and writes an `R16G16_FLOAT` buffer that FSR reads with `motionVectorScale = (renderWidth, renderHeight)`. FSR 2 reads a pixel's history at its current UV plus its motion vector (`ffx_fsr2_reproject.h`), so it wants the previous position less the current one. The engine stores the current less the previous, so the decode negates both axes (`MV_SIGN` in `fsr/mod.rs`).

### The stereo correction

The engine's velocity pass computes `curUV` with the per-eye current view-projection (the camera the mod offsets) but `prevUV` with the single simulation-side previous view-projection of the centre camera. In stereo every static pixel therefore carries a spurious lateral motion vector equal to the eye's parallax from the centre: depth-dependent, vanishing at infinity, and of opposite sign in each eye. FSR would mis-reproject each eye's history by that amount, which shows as per-eye shimmer on high-contrast static edges, worst under head motion.

The decode pass corrects it (`fsr.stereo_mv_correction`, on). The CPU forms the two clip-to-previous-clip reprojection matrices in `f64` (`prevVP * inv(curVP)`; the world-scale translations cancel only at double precision), and the shader reconstructs each pixel from the depth, reprojects it with both, and adds `prevUV_center - prevUV_eye` to the decoded vector. Object motion survives; only the camera term is swapped. The "centre" matrix is the previous view-projection the velocity pass actually encoded against where the payload can read it: the render camera's own `m_PreviousViewProjectionF` for an eye rendered whole, and that matrix taken through the single-pass velocity rewrite's map into the eye's clip space for a half of the single-pass target (`encoded_previous_view_projection`). The correction runs while the stereo cameras are on and is a no-op without disparity.

### The hybrid decode

The velocity pass draws only what is enqueued on it: in the scenes measured, the vegetation and a few hardware-instanced models ([shaders.md](../../engine/rendering/shaders.md#the-velocity-passs-motion-vectors)). Every other pixel keeps the cleared value, which decodes to zero motion, so without more FSR would reproject the static world's history as if the camera had not moved. The correction leaves those pixels alone, since the engine encoded no camera motion there.

The hybrid decode (`fsr.hybrid_velocity`, on) gives the static world its real motion. An undrawn pixel holds the target's clear value, which no velocity pixel shader writes, except a pixel beyond about a hundred metres whose motion rounds to the same code (every velocity shader writes its third channel from the clip `w` and saturates there). For an undrawn pixel, and for a drawn one whose encoded motion saturated at the clamp, the decode reconstructs the pixel from its depth, reprojects it through the eye's own clip-to-previous-clip matrix (formed in `f64` from the eye's own view-projections), and writes that motion, unclamped and at full precision. Every other pixel keeps the engine's motion with the correction, so object motion survives where the engine wrote some. The same matrices serve multipass (the render camera's per-eye history) and single-pass (each eye's own matrices), canted and asymmetric eyes included. The sky's depth is the infinite far plane, where the reprojection is the rotation alone.

With no history (the first frame, a rebuilt context) the decode sends no motion and FSR resets. A camera cut, the view centre landing more than the render tuning's `camera_cut.ndc` away in the previous frame at the far plane or at `camera_cut.probe_depth_m` (`camera_cut` in `fsr/mod.rs`), resets that eye's history too. A far drawn pixel misread as undrawn gets the camera's motion instead of its own, and so does near geometry the velocity pass does not draw that moves with the head (the player's body and weapon), which FSR's own disocclusion handling then has to catch.

The velocity buffer's precision has two limits: the 8-bit format and the clamp, which saturates on fast head turns. The hybrid decode removes both for camera motion, which is where head turns put it; object motion keeps them.

### Under single-pass stereo

The single-pass collapse renders both eyes side by side into one double-wide target and runs the post chain once. `single_pass.fsr_per_eye` (on) resolves each half through its own eye's context: the decode reads the eye's window of the velocity and depth buffers and copies that window's depth into an eye-sized R32F texture (a depth-stencil resource cannot be copied by region), the eye's half of the colour is copied into an eye-sized input, and the result is copied back into the half. Each eye then has its own history at its own size, with no neighbourhood or history reaching across the seam, and the velocity pass's single-pass rewrite already writes each eye's motion in that eye's own screen space. The correction is taken from each eye's own matrices (the centre camera's view with the eye's projection, and the eye's view-projection, this frame's and last frame's), since the collapse keeps the render camera centred and the render camera's history holds the centred double-wide matrices. [single-pass-stereo.md](../stereo/single-pass-stereo.md) has the measurement.

## Sub-pixel jitter

At the display size FSR gains on a single-sample render only when every frame samples the scene at a different sub-pixel position; without that it is a temporal filter. The mod jitters each eye's projection every VR frame and tells FSR the offset (`fsr.jitter`, on).

### The engine's own jitter

[`Camera::SetupRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetupRenderCamera), with its jitter flag set, has [`RenderEngine::ApplyJitterTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-ApplyJitterTransform) post-multiply a clip-space translation onto both the projection and the previous projection, with this frame's offset, before it rebuilds the view-projections, so the velocity pass's current and previous positions carry the same offset and its motion none. Only the scene render camera gets it; the reflection and shadow cameras are set up without it ([`RenderPass::GetSortCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-GetSortCamera)), and the visibility cull and the cascade fit read the active camera, which is never jittered. The offset is two alternating samples ([`jittered_samples`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::jittered_samples)), applied only at the SMAA T2X mode, which stereo never runs. The mod keeps it off in stereo and drives its own.

### What the mod drives

The mod drives FSR's own Halton(2, 3) sequence, `ffxFsr2GetJitterOffset`, at the length `ffxFsr2GetJitterPhaseCount` gives for the render-to-display ratio, which at 1:1 is eight phases. One offset per VR frame is chosen on the game thread when the frame's per-eye render parameters are published (`jc3vr_view::fsr::jitter::frame_offset`, called from `vr::frame`) and carried with them (`EyeRenderParams::jitter`), so every pass of that frame and the FSR dispatch read the same offset. Both eyes take the same offset in their own pixels.

An offset `(x, y)` in pixels, x right and y down, means the content moves by that many pixels, so a pixel centre samples the unjittered image at its centre less the offset (`ffx_fsr2_upsample.h`: `fSrcUnjitteredPos = iSrcInputPos + 0.5 - Jitter()`). In clip space it is `T(2x / width, -2y / height)`, a translation by that times `w`, pre-multiplied onto the projection in the column-vector layout: the FSR documentation's `jitterTranslationMatrix * projectionMatrix`, and the engine's own `proj * jitterMat` in its row-vector layout. FSR's `jitterOffset` is the `(x, y)` in pixels. Under single-pass each eye's clip space spans its own half of the double-wide target, so the width is the eye's and the offset in clip space is the same in both modes. The eye size the offset is scaled by is the size FSR last resolved at (`jitter::publish_eye_size`), and FSR is told the offset converted back with the size it resolves at, so the two always agree.

### What sees the jitter

The jitter is folded into the per-eye projections at their source, so it reaches every pass that rasterizes or reconstructs an eye image:

- Multipass: each eye's render camera, and so every engine constant built from it, the deferred passes' reconstruction from depth included.
- Single-pass: the `cb13` rows and the reprojection's `M_eye`, the eye vertex globals, the per-eye pixel buffers (the eye camera, the screen-space reflections, the ambient occlusion and its volumes, the decals, the fog volumes, the water, and the clustered lights' tile bounds), the per-eye reconstruction inverses, and the velocity rewrite's map, whose eye pose is recovered from `M_eye` and the eye's projection, so the jitter cancels in it.

What must not see it keeps the projection as it was before the jitter, bit for bit (`EyeRenderParams::unjittered_projection_reverse_z` and its siblings):

- The visibility cull, the cull-both-eyes terrain matrix, and the sun-shadow cascade fit, which are built from their own union projections.
- What draws after FSR has resolved the eye: the HUD panel and its world markers (`hud::quad::fetch_view_projection` in the HUD crate, `single_pass::collapse_ui_eye_override`), the controller overlay (`controllers::overlay::view::current` in the render crate), and the scope lens. They hold still while the scene jitters under them. They test against the jittered depth, so where an overlay passes behind a silhouette its cut-off edge moves by up to half a pixel from frame to frame.
- The static foveation's mask centre, which would otherwise slide by half a pixel a frame.
- The motion vectors.

### The motion vectors stay free of it

FSR wants motion without jitter, and the mod does not set `FFX_FSR2_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION`. The hybrid decode's camera motion reconstructs each pixel through the jittered current matrix, where the render put it, and reprojects it into the previous frame with this frame's offset applied to the previous view-projection, `M = T(j) P C^-1 T(-j)`, which is the engine's own construction (`jitter::jitter_free_reprojection`, derived in its comment). The engine-written velocity of the drawn pixels takes its current position from the jittered raster and its previous one through the unjittered previous camera, so it carries the offset, a fraction of one code; the stereo correction takes it back out exactly, since its "encoded" matrix is built the same way (`jitter::reprojection_from_jittered`).

The view-projection history the decode reads holds every matrix without the jitter, with the offset beside it (`VpHistory::cur_jitter`). A jitter folded into an `f32` world-space view-projection rounds its translation (thousands of metres from the origin) differently every frame, which would hand FSR several pixels of motion at near pixels with the camera still. Rebuilt from the eye's projection before the jitter, a still camera's matrices are identical from frame to frame.

### When it is off

With the engine's anti-aliasing chosen there is no jitter: `frame_offset` returns none unless `fsr.antialiasing` is temporal (`AntiAliasing::temporal`) and `fsr.jitter` is on. Outside a VR frame (flatscreen stereo, a replay) there are no per-eye render parameters and no jitter. The capture harness's profile turns FSR off, so the standing suite renders without jitter. Where a harness configuration puts FSR back, the jitter is deterministic: the sequence restarts at every configuration's FSR history reset (`fsr::request_history_reset`), a still camera's snapshot waits for a named offset, and a motion restarts the sequence as it begins ([fast-comparison.md, "FSR's jitter in a snapshot"](../testing/fast-comparison.md)), so every mode compares at the same offset. A full-camera freeze keeps cycling the sequence, so a frozen pose still accumulates sub-pixel samples.

## FSR's other inputs

The context is created with the depth inverted and infinite and FSR's auto-exposure, and without motion-vector jitter cancellation, display-resolution motion vectors, or high dynamic range: the motion vectors are free of jitter and at the render size, which is the display size, and the input is tone-mapped. No reactive mask and no transparency-and-composition mask are passed, and FSR's automatic reactive mask is off: particles, the water, and other alpha-blended surfaces rely on FSR's own shading-change detection, which unlocks a pixel whose colour changes abruptly. Whether particles and water smear more with the jitter is not measured.

FSR takes a single vertical field of view to linearize depth, read from each eye's projection matrix (`1 / tan(fovV / 2)`, invariant under reverse-Z). A headset's eye frustum is asymmetric, which one scalar cannot represent; the symmetric-equivalent field of view is an approximation that holds for mild asymmetry, and depth-reconstruction artifacts at the frustum edges under strongly canted projections would point at it.

## Sharpening

FSR's RCAS sharpening pass is off by default (`fsr.sharpen`, with its strength, `fsr.sharpness`, kept at 0.2 beside it). With the jitter, FSR's own resolve is already sharper than SMAA against a supersampled reference, and every step of sharpening moves the image further from it, adds spurious gradient, and makes a still image change more from frame to frame ([fsr-measurements.md, "Sharpening"](fsr-measurements.md#sharpening)). The switch and the strength are on the pause menu's display page and in the debug UI.
