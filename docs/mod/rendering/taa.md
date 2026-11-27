# The mod's temporal anti-aliasing resolve

FSR 2 runs at 1:1 as the mod's anti-aliasing ([fsr.md](fsr.md)), and most of its passes exist for upscaling (the depth clip, the lock, the reactive mask, the luma pyramid, the sharpening): at the headset's size it costs about 2 ms of a frame budget of 8.3. The mod's own resolve (`crates/jc3vr-render/src/fsr/taa.rs`, `crates/jc3vr-render/shaders/taa_resolve.hlsl`) is a single compute pass at the same hook, reading the same inputs, that does what a modern TAA does and nothing an upscaler needs. It is selectable with `fsr.antialiasing = "Taa"` ([fsr.md, "Choosing the resolve"](fsr.md#choosing-the-resolve)); FSR stays the default, for the reasons in [the verdict](#verdict). The jitter and the motion-vector decode it shares with FSR are described in [fsr.md](fsr.md).

## Design

The resolve runs where FSR does, in `anti_aliasing_apply`, on the chain's current slot: the post-tonemap image, R8G8B8A8 in the engine's display encoding, which the engine's later passes and the swapchain read as sRGB. It resolves in that encoding, as SMAA and FSR do at this hook. The values are close to perceptually even, so the colour box and the blend weigh a dark and a bright edge alike, and no conversion is spent on either side. Averaging encoded values darkens a sub-pixel bright line slightly against averaging light, which every method at this hook shares. It also reads the engine's depth and velocity buffers and each eye's reprojection matrices, the same `EyeMotion` FSR's dispatch builds (the render camera's history for an eye rendered whole, each eye's own matrices for a half of the single-pass target).

Under single-pass the chain texture holds both eyes side by side, and the resolve dispatches once over the whole of it. Each thread finds its eye from its column and clamps every neighbourhood tap to its eye's half, and every history tap half a texel inside it, so nothing is read across the seam and no copy into eye-sized textures is needed. In multipass each eye's chain texture is resolved by its own dispatch. Each layout keeps its own pair of RGBA16F history textures, ping-ponged, so the history never mixes layouts.

Per pixel, in order:

1. The 3x3 neighbourhood of the current colour in YCoCg: its mean and standard deviation, its minimum and maximum, its nearest and farthest depth, and a reconstruction of the colour at the pixel's unjittered centre. The jitter moves the content by its offset, so the neighbour at offset `d` holds the scene at `d - jitter` from that centre and is weighted `exp(-k |d - jitter|^2)`, `k = current_sharpness`.
2. The motion of the nearest-depth pixel in the neighbourhood (velocity dilation), so a thin foreground edge carries its own motion over the background around it. It is decoded inline from the engine's velocity buffer exactly as `velocity_decode.hlsl` decodes it for FSR, the stereo correction and the hybrid camera motion included, so no decoded motion buffer is written or read.
3. The history at the pixel's previous position through the five-tap Catmull-Rom filter (bilinear taps at the combined centre and the four arms), which keeps a still image as sharp as its history over many frames of reprojection.
4. Rejection: the history is dropped where the previous position is outside the eye, and, for a pixel whose motion is the camera's, where the history's stored depth (its alpha holds each pixel's raw depth) lies outside what the neighbourhood's nearest and farthest depths reproject to, with the render tuning's `taa.depth_tolerance` as relative slack ([tuning.md](../tuning.md)). Otherwise it is clipped toward the centre of the variance box: the mean plus or minus `gamma` standard deviations, intersected with the minimum and maximum, with `gamma` going from `clip_gamma_rest` at rest to the tuning's `taa.clip_gamma` at 16 pixels of motion a frame.
5. The blend: the history's share is `history_weight` at rest, falling to `motion_history_weight` at 16 pixels of motion; none on a reset or a rejection.

`history_weight`, `motion_history_weight`, `clip_gamma_rest`, and `current_sharpness` are the settings under `fsr.taa` (`TaaConfig`), shown in the debug UI's stereo section while TAA is chosen. The history resets with FSR's: the harness's per-configuration reset (`fsr::request_history_reset` resets both), a camera cut (`fsr::camera_cut` on the eye's reprojection), a resize or a layout change (a new lane), and no history yet. The result goes to an output texture in the chain's format and is copied back into the slot, after which the engine's anti-aliasing runs as a passthrough, as it does for FSR.

The foveation's fill-in reconstructs the dropped pixels before the post chain, so the resolve sees a complete image in which the periphery's dropped quads are interpolated from their neighbours. Those pixels jitter with the rest and accumulate like any other. The neighbourhood statistics there are of interpolated values, slightly smoother, which loosens the clip box a little in the periphery.

The resolve has no reactive or transparency mask (like FSR here, it relies on the clip box for particles and water), no sharpening pass, and no upscaling. The harness saves no `fsr_motion` or `fsr_depth` planes for it, since it writes no decoded motion buffer, and it records no per-eye dispatch records.

## Harness

The visual override `taa = "enabled"` puts TAA in FSR's place with FSR's shipped inputs. An FSR variant's `taa = true` does the same within a schedule, with the `taa_history_weight`, `taa_clip_gamma_rest`, `taa_current_sharpness`, and `taa_motion_history_weight` overrides for tuning. The benchmark takes a `taa = true` request field, used by the `taa` variant in `tools/rendering/suites/benchmark.toml`. `tools/rendering/suites/taa.toml` lists the quality scenes below.

## What it costs

The benchmark at the town and the capital, single-pass, unpaced, 3584x3561 per eye, three repeats, the release payload, `--gpu-passes`. Medians in milliseconds per frame. The resolve seam is the resolve's own GPU time (the `Fsr` seam, which times whichever resolve runs, the copy-back included); the engine AA pass is the pass after it, which is SMAA itself in the SMAA rows. No anti-aliasing is derived as the SMAA row less the SMAA pass plus the passthrough blit:

| Scene | Mode | Resolve seam | Engine AA pass | Post block | GPU frame | Frame |
|---|---|---|---|---|---|---|
| Town | FSR | 1.87 | 0.06 | 2.57 | 7.75 | 7.90 |
| Town | TAA | 0.58 | 0.08 | 1.30 | 6.48 | 6.50 |
| Town | SMAA 1x | 0 | 0.43 | 1.06 | 6.22 | 6.28 |
| Town | None (derived) | 0 | 0.06 | 0.69 | 5.85 | |
| Capital | FSR | 2.07 | 0.06 | 2.81 | 9.11 | 9.18 |
| Capital | TAA | 0.60 | 0.10 | 1.38 | 8.53 | 8.99 |
| Capital | SMAA 1x | 0 | 0.40 | 1.07 | 9.21 | 9.44 |
| Capital | None (derived) | 0 | 0.06 | 0.73 | 8.87 | |

TAA costs 0.66 to 0.70 ms with its passthrough, a third of FSR's 1.93 to 2.13, and 0.24 to 0.31 ms more than SMAA. The capital's GPU and frame totals swing by about half a millisecond between repeats with the simulation, so its totals do not order the modes; the post block and the seams do. The seam includes the copy of the result back into the slot, a full-screen copy of both eyes.

## Against FSR and SMAA

The scenes are FSR's quality scenes ([fsr-measurements.md](fsr-measurements.md#against-the-games-anti-aliasing)) with TAA in place of the jitter-off variant: the town and the meadow, still, rotating, translating, and panning slowly, single-pass, the left eye against the reference (the left eye alone at twice the pixels with no anti-aliasing, downsampled by area). Each motion scene ran at four motion phases with `--next-frame-repeat` (`fast --motion-phase`), and each first snapshot and repeat is scored against the reference's own snapshot of that frame, so every motion figure is the mean over the jitter's eight offsets (`fsr-quality` given the four reports). Each cell holds the PSNR, the edge sharpness relative to the reference's, and the ghosting (the gradient added where the reference is flat), then, in motion, the flicker: each image's frame-to-frame change less the reference's, in codes. TAA is at the shipped tuning:

| Scene | FSR | TAA | SMAA 1x | No anti-aliasing |
|---|---|---|---|---|
| Town, still | 32.35 dB, 0.94, 0.121 | 32.56 dB, 0.83, 0.110 | 32.00 dB, 0.88, 0.122 | 31.87 dB, 0.94, 0.117 |
| Town, rotating | 31.98 dB, 0.91, 0.157; 3.41 | 32.02 dB, 0.84, 0.139; 3.39 | 31.88 dB, 0.89, 0.144; 3.25 | 31.82 dB, 0.95, 0.141; 3.10 |
| Town, translating | 31.66 dB, 0.83, 0.170; 3.27 | 31.13 dB, 0.73, 0.169; 3.75 | 31.75 dB, 0.89, 0.160; 2.71 | 31.70 dB, 0.96, 0.156; 2.52 |
| Town, slow pan | 32.33 dB, 0.84, 0.159; 2.07 | 31.94 dB, 0.79, 0.158; 1.99 | 32.33 dB, 0.89, 0.153; 1.90 | 32.25 dB, 0.95, 0.149; 1.76 |
| Meadow, still | 30.36 dB, 0.95, 0.386 | 30.75 dB, 0.84, 0.300 | 29.39 dB, 0.91, 0.438 | 28.84 dB, 0.98, 0.464 |
| Meadow, rotating | 29.79 dB, 0.90, 0.283; 5.34 | 29.73 dB, 0.83, 0.258; 5.37 | 29.40 dB, 0.91, 0.279; 5.18 | 28.88 dB, 0.99, 0.289; 5.22 |
| Meadow, translating | 29.04 dB, 0.80, 0.416; 5.52 | 27.14 dB, 0.61, 0.460; 7.19 | 29.31 dB, 0.91, 0.432; 4.79 | 28.74 dB, 0.99, 0.458; 4.81 |
| Meadow, slow pan | 29.74 dB, 0.81, 0.363; 4.82 | 29.68 dB, 0.77, 0.354; 4.85 | 29.37 dB, 0.91, 0.419; 4.56 | 28.84 dB, 0.99, 0.442; 4.60 |

At rest TAA is the closest to the reference of the four, 0.2 dB above FSR at the town and 0.4 dB at the meadow, with the least spurious gradient. Its next frame changes at a sixth to a third as many pixels as FSR's (46,600 against 300,000 at the town, 162,000 against 510,000 at the meadow); SMAA's changes at 1,100 and 35,000. It is softer than FSR at the edges (0.83 to 0.84 against 0.94 to 0.95, below SMAA's 0.88 to 0.91), mostly from the current frame's unjitter kernel and the history's exponential average over the jitter's eight positions, where FSR's Lanczos reconstruction is sharper.

In rotation and on the slow pan TAA is within 0.1 dB of FSR at the meadow and level with it in rotation at the town, 0.4 dB below it on the town's slow pan, with about FSR's flicker and less ghosting.

Translating is where it falls short: 0.5 dB below FSR at the town and 1.9 dB below at the meadow, with an eighth to a quarter less edge sharpness and 15 to 30% more flicker. The near ground and the grass move by tens of pixels a frame there, and the history, resampled every frame and clipped to a box of the current neighbourhood, loses its fine detail: the grass reads as a smear of its blades rather than as ghost trails. FSR's lock and its reconstruction keep more of it. The difference closes under a slower camera.

In the crops, no edge of the poles, the power lines, the turbine, or the trees trails its previous position in any motion scene. The town's power lines at rest are continuous even-weight lines with FSR, TAA, and SMAA, TAA's a touch softer than FSR's; the meadow's dithered tree crowns become soft foliage with TAA, more so than with FSR. Translating, the ground's gravel and the grass are visibly softer in TAA than in FSR. None of the quality scenes has a vehicle or a hand in view, so trails behind a moving object are not measured.

### The tuning

At rest, a longer history (`history_weight` 0.95 against 0.9) cut the frame-to-frame change by three to six times for 0.04 to 0.08 dB, but on the slow pans, which keep most of the rest weight, it lost 0.4 dB and gained flicker. A looser clip box at rest (`clip_gamma_rest` 2.5 against 1.5) moved the scores by 0.02 dB, and a tighter one (1.0) lost 0.15 to 0.2 dB with more change from frame to frame. A sharper unjitter kernel (`current_sharpness` 8 or 16 against 4) raised the edge sharpness by 0.02 to 0.03 but lost up to 0.1 dB at the meadow. In motion the history weight is the lever: `motion_history_weight` 0.4 against 0.8 gained 1.7 dB on the translating town and 0.6 dB on the translating meadow, 0.16 to 0.32 dB in rotation and on the pans, 2 to 25% more edge sharpness, and less flicker in every motion scene; 0.6 fell between the two almost everywhere. The shipped values are those in `TaaConfig::new`.

## Verdict

TAA does at rest what FSR does, slightly closer to the reference and with a sixth to a third of FSR's frame-to-frame change, and holds level with FSR in rotation and slow pans, for 1.3 to 1.4 ms less GPU time. It is softer at the edges than FSR everywhere, and under fast translation it blurs the near ground and the grass, 0.5 to 1.9 dB below FSR and SMAA. That case is the player walking and driving, and the meadow's grass in motion reads worst, so FSR stays the default.

Not measured: the headset (everything above is the harness's simulated headset at a still or scripted camera), the quality tables in multipass (there TAA converges and draws nothing wrong at the eye edges; multipass's grass motion error, in [fsr-measurements.md](fsr-measurements.md#under-multipass-and-the-isolated-eyes), applies to TAA's inputs as to FSR's), the foveation on (the capture profile turns it off), transparencies and particles, and moving objects.
