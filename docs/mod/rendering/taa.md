# The mod's temporal anti-aliasing resolve

FSR 2 runs at 1:1 as the mod's anti-aliasing ([fsr.md](fsr.md)), and most of its passes exist for upscaling (the depth clip, the lock, the reactive mask, the luma pyramid, the sharpening), so at the headset's size it costs about 2 ms of an 8.3 ms budget. The mod's own resolve (`crates/jc3vr-render/src/fsr/taa.rs`, `crates/jc3vr-render/shaders/taa_resolve.hlsl`) is a single compute pass at the same hook, reading the same inputs, that does what a modern TAA does and nothing an upscaler needs. It is selected with `fsr.antialiasing = "Taa"` ([fsr.md](fsr.md#choosing-the-resolve)) and shares FSR's jitter and motion-vector decode; FSR stays the default, for the reasons [below](#against-fsr).

## Design

The resolve runs in `anti_aliasing_apply` on the chain's current slot: the post-tonemap image in the engine's display encoding, which it resolves in, as SMAA and FSR do at this hook. Those values are close to perceptually even, so the colour box and the blend weigh a dark and a bright edge alike. It also reads the engine's depth and velocity buffers and each eye's reprojection matrices, the same `EyeMotion` FSR's dispatch builds.

Under single-pass it dispatches once over the double-wide texture: each thread finds its eye from its column and clamps every neighbourhood tap to its eye's half, and every history tap half a texel inside it, so nothing is read across the seam and no eye-sized copies are needed. In multipass each eye's texture gets its own dispatch. Each layout keeps its own ping-ponged pair of RGBA16F history textures.

Per pixel, in order:

1. The 3x3 neighbourhood in YCoCg: mean, standard deviation, minimum, maximum, nearest and farthest depth, and a reconstruction of the colour at the pixel's unjittered centre, each neighbour weighted by `exp(-k |d - jitter|^2)` with `k = current_sharpness`.
2. The motion of the nearest-depth neighbour (velocity dilation), so a thin foreground edge carries its own motion. It is decoded inline exactly as `velocity_decode.hlsl` decodes it for FSR, correction and hybrid camera motion included, so no motion buffer is written.
3. The history at the previous position through a five-tap Catmull-Rom filter, which keeps a still image sharp over many frames of reprojection.
4. Rejection: the history is dropped where the previous position is outside the eye, or, for a pixel whose motion is the camera's, where the history's stored depth (in its alpha) lies outside what the neighbourhood's depths reproject to, with `taa.depth_tolerance` as slack ([tuning.md](../tuning.md)). Otherwise it is clipped toward the centre of the variance box, `gamma` standard deviations wide, with `gamma` going from `clip_gamma_rest` at rest to `taa.clip_gamma` at 16 pixels of motion a frame.
5. The blend: the history's share goes from `history_weight` at rest to `motion_history_weight` at 16 pixels of motion; none on a reset or a rejection.

The four weights are the `fsr.taa` settings (`TaaConfig`, whose `new` holds the shipped values), shown in the debug UI's stereo section while TAA is chosen. The history resets with FSR's (`fsr::request_history_reset`, a camera cut, a resize or layout change, and no history yet). The result is copied back into the slot and the engine's anti-aliasing runs as a passthrough. The foveation's fill-in runs before the post chain, so the resolve sees complete images. There is no reactive or transparency mask, no sharpening, and no upscaling, and the harness saves no motion planes or dispatch records for it.

In the harness, the visual override `taa = "enabled"` puts TAA in FSR's place, an FSR variant's `taa = true` does the same within a schedule (with `taa_*` overrides for the four weights), and the benchmark's `taa` variant runs it. `tools/rendering/suites/taa.toml` lists the quality scenes.

## Against FSR

At the headset's size TAA costs 0.66 to 0.70 ms of GPU time, a third of FSR's 1.9 to 2.1 ms and about 0.3 ms more than SMAA. Scored as FSR is ([fsr.md](fsr.md#what-the-harness-measures)), averaged over the jitter's eight offsets:

- At rest it is the closest to the reference of all the methods, slightly above FSR, with the least spurious gradient and a sixth to a third of FSR's frame-to-frame change.
- It is softer at the edges than FSR everywhere, below SMAA too, from the unjitter kernel and the history's exponential average where FSR's Lanczos reconstruction is sharper.
- In rotation and slow pans it is level with FSR, with less ghosting.
- Under fast translation it falls short, 0.5 to 1.9 dB below FSR: the near ground and the grass move by tens of pixels a frame, and a history resampled every frame and clipped to the current neighbourhood loses their fine detail, so the grass reads as a smear. No edge trails its previous position.

That last case is the player walking and driving, so FSR stays the default. In motion, `motion_history_weight` is the lever that matters; the other weights move the scores by tenths of a decibel at most. Not measured: the headset, multipass quality (where multipass's grass motion error applies to TAA's inputs as to FSR's), the foveation on, transparencies, and moving objects.
