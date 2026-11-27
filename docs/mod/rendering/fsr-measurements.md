# FSR measurements

These are the capture harness's measurements of the FSR integration in [fsr.md](fsr.md): that the motion vectors and the jitter are what FSR is told, how FSR's image compares with the game's anti-aliasing and with a higher render resolution, and what it costs. The mod's own TAA has its measurements in [taa.md](taa.md). Every figure is the harness's simulated headset at 3584x3561 per eye, at a still or scripted camera, with the foveation off (the capture profile turns it off). How the harness takes the snapshots and saves the planes is in [fast-comparison.md](../testing/fast-comparison.md). The scenes are listed in `tools/rendering/suites/fsr.toml`, and the benchmark variants in `tools/rendering/suites/benchmark.toml`.

## Verifying the motion vectors

The harness saves what FSR is handed (the `fsr_motion` and `fsr_depth` planes and the snapshots' FSR records) and `python3 -m tools.rendering fsr-motion` checks it offline. For every eye it reconstructs each pixel from the depth FSR read, reprojects it through the eye's recorded previous view-projection in double precision, and compares that camera motion, in FSR's convention and pixels, with the decoded vector times FSR's scale. For the pixels the hybrid decode gives the camera's motion (99.3 to 99.8% of each eye), with the jitter on, the error's mean, 99th percentile, and largest value in pixels, against a camera motion of 22 to 26 pixels mean and up to 183:

| Scene | Multipass, each eye | Single-pass, each eye |
|---|---|---|
| Still | 0 / 0 / 0 | 0 / 0 / 0 |
| Rotating | 0.009 / 0.027 / 0.03 | 0.009 / 0.027 / 0.03 |
| Translating | 0.009 / 0.062 / 0.12 | 0.009 / 0.062 / 0.12 |
| Rotating, `index` rig (canted) | 0.008 / 0.027 / 0.04 | 0.008 / 0.027 / 0.04 |
| Rotating, shipped settings | 0.009 / 0.027 / 0.03 | 0.009 / 0.027 / 0.03 |

With the jitter on, the check takes the expected motion from its definition: the pixel's point, where the jittered render put it, in the previous frame less the same point unjittered. The residual is the reprojection in single precision and the 16-bit output. With the hybrid decode off those pixels carry no camera motion at all, an error of the whole camera motion. The records are rounded back to single precision before the reconstruction, because their shortest decimal form differs from the stored value enough that the near-plane division would turn it into apparent error near the camera.

The pixels the velocity pass drew (0.2 to 0.7% of an eye, vegetation in these views) keep the engine's object motion on top of the camera's. Under single-pass they decode to within 0.03 pixels of the camera's motion, and to exactly zero with the camera still. In multipass they are 2.5 to 3.3 pixels off on average while still or rotating and 8 to 15 while translating: the engine's encoding agrees with the render camera's previous view-projection only to within an eye-dependent parallax there (see [Under multipass and the isolated eyes](#under-multipass-and-the-isolated-eyes)). The tool's `engine vs encoded` check of the engine's encoding against the recorded matrices is independent of the decode, and it is what confirmed the encoding's convention.

With a still camera every pixel the hybrid decode gives the camera's motion decodes to exactly zero, and FSR keeps its history. That history takes about 64 frames to settle after a reset: at the town in multipass, the next frame differs from the last at 0.8 to 2.6 million pixels per eye after 2 to 16 frames of warm-up, 100,000 after 32, about 2,000 after 64, and 550 to 1,100 after 128 or 300. A comparison of two configurations at a still camera with FSR on therefore needs about 120 warm-up frames. Settled, multipass and single-pass differ at 67,000 and 110,000 pixels per eye with FSR against 53,000 and 75,000 with the game's anti-aliasing: what remains is the single-pass render's own difference, which FSR enlarges a little.

## The jitter in the image

`python3 -m tools.rendering fsr-jitter` estimates the shift between two consecutive frames' `pre_aa` planes (the image FSR is handed) and compares it with the change in the offset FSR was told. At the town at a still camera, for every consecutive pair of the sequence, in multipass and single-pass, both eyes, and under the canted `index` rig, the image moved by the told offset to within 0.02 to 0.07 pixels in both axes and with the told sign, and multipass and single-pass gave the same shift to a thousandth of a pixel. The FSR output of the same frames moved by at most 0.02 pixels against 0.5 to 0.8 pixels of input shift: FSR places each sample where the image put it.

A jitter that excites a blob-scale shading flicker would show here. What changes from frame to frame at full resolution is aliasing (thin edges, chiefly the power lines, covering a pixel in one frame and missing it in the next), which averages away with area: from a largest change of 11 to 20 codes over 16 by 16 pixel blocks to 1.2 to 3.2 codes over 64 by 64 and 0.12 to 0.25 over 256 by 256. A shading change of a blob's size would not average away. With the jitter off, the same blocks change by under 1.3 codes at any size.

## Against the game's anti-aliasing

The quality scenes (`quality-*`) render, in one schedule at one clock and pose, FSR as shipped, FSR without its jitter (`jitter-off`), the game's SMAA 1x (`fsr-off`), no anti-aliasing at all (`no-aa`), and a reference: the left eye alone at twice the pixels (1.414 each side) with no anti-aliasing, downsampled by area to the eye's size, so that it favours no anti-aliasing method (`python3 -m tools.rendering fsr-quality`). The scenes are the town and the meadow, whose grass and trees are the thin geometry that shimmers, each still, rotating, translating, and panning slowly (a degree of yaw each way, at most two pixels a frame). The left eye of single-pass against the reference, with the sharpening at 0.2; each cell is the PSNR, the edge sharpness relative to the reference's, and the ghosting (the gradient added where the reference is flat):

| Scene | FSR | FSR, no jitter | SMAA 1x | No anti-aliasing |
|---|---|---|---|---|
| Town, still | 33.33 dB, 1.02, 0.190 | 31.10 dB, 1.14, 0.249 | 33.18 dB, 0.89, 0.165 | 33.13 dB, 0.96, 0.162 |
| Town, rotating | 32.77 dB, 0.98, 0.195 | 32.70 dB, 1.01, 0.189 | 32.64 dB, 0.89, 0.161 | 32.61 dB, 0.96, 0.159 |
| Town, translating | 32.57 dB, 0.89, 0.222 | 32.25 dB, 0.92, 0.256 | 32.16 dB, 0.89, 0.199 | 32.19 dB, 0.95, 0.195 |
| Town, slow pan | 33.18 dB, 0.90, 0.193 | 33.12 dB, 0.92, 0.196 | 33.13 dB, 0.89, 0.171 | 33.09 dB, 0.96, 0.167 |
| Meadow, still | 30.00 dB, 1.00, 0.436 | 26.68 dB, 1.16, 0.736 | 29.36 dB, 0.91, 0.422 | 28.82 dB, 0.98, 0.445 |
| Meadow, rotating | 29.73 dB, 0.98, 0.305 | 29.75 dB, 1.01, 0.299 | 29.47 dB, 0.91, 0.265 | 28.93 dB, 0.99, 0.276 |
| Meadow, translating | 29.29 dB, 0.86, 0.449 | 29.42 dB, 0.88, 0.462 | 29.27 dB, 0.91, 0.432 | 28.71 dB, 0.98, 0.457 |
| Meadow, slow pan | 29.83 dB, 0.85, 0.403 | 29.89 dB, 0.86, 0.409 | 29.40 dB, 0.91, 0.419 | 28.87 dB, 0.99, 0.442 |

With the jitter, FSR is the closest to the reference in every scene but the meadow's rotation, translation, and slow pan, where FSR without the jitter is 0.02 to 0.13 dB closer still. Jittered FSR is 0.05 to 0.41 dB closer than SMAA at the town and 0.02 to 0.64 dB at the meadow. The jitter's own gain is at rest, where FSR without it is a temporal filter of one sample position under the sharpening: 2.2 dB at the town and 3.3 dB at the meadow, with a quarter to two fifths less spurious gradient. In motion FSR keeps the reprojected history either way, and the two are within 0.35 dB. The gains over SMAA are small in PSNR because most of an eye is smooth surface that every method renders alike.

At the edges the difference is large. The town's power lines against the sky are continuous, even-weight curves with FSR, as in the reference, where FSR without the jitter, SMAA, and no anti-aliasing all draw them as stair-stepped segments with gaps where the sag crosses a pixel row. The meadow's tree crowns, whose leaves the engine draws with a screen-door dither, come out with FSR as soft, partly transparent foliage, close to the reference, where the others keep the hard checkerboard. The grass blades are smooth-edged with FSR; without the jitter they are crunchy and over-sharpened, and SMAA leaves them nearly as aliased as no anti-aliasing.

FSR's price is temporal. At a still camera, the next frame changes at 381,000 pixels of an eye (by more than three codes; a mean change of 0.43 codes) at the town and 662,000 (0.68) at the meadow with jittered FSR, against 1,300 and 2,200 with SMAA and 2,600 and 5,300 with no anti-aliasing: each frame samples new positions and FSR's accumulation keeps a share of each, so the resolved image moves by a fraction of a code where the scene has sub-pixel detail, spread over the grass and the edges. Turning the sharpening off takes a quarter of it out. In motion, the flicker (each configuration's frame-to-frame change against the reference's, `fsr-quality`'s `flicker`) is dominated by the edges' own motion: on the slow pan it is 2.20 codes for FSR against 2.06 for SMAA and 1.91 for no anti-aliasing at the town, and 4.93 against 4.72 and 4.74 at the meadow. The measure cannot separate shimmer from the edges moving.

### Under multipass and the isolated eyes

The left eye rendered on its own (the isolated eye, which takes multipass's path) scores like single-pass at the town at rest (33.27 dB), but 0.5 to 1.6 dB lower in motion, and at the meadow 0.7 to 1.2 dB lower throughout, below SMAA; at the meadow at rest its next frame changes at 2.5 million pixels, about what the jittered input itself changes by. The cause is the motion of the pixels the velocity pass draws, the grass above all. The image FSR is handed is the same in both modes to a thousandth of a pixel, but where single-pass decodes those pixels to the camera's motion exactly, multipass leaves them 13 pixels off on average at the meadow and 2.8 at the town, with the camera still. Most of the town's offset is a constant per eye, 3.7 pixels horizontally and opposite in the two eyes, which the encoded previous view-projection does not account for. Without the jitter FSR settles under that error to a fixed, slightly misregistered image; with the jitter it rejects its history on those pixels every frame and the grass shows the raw jitter. The harness's held camera is not the camera the engine's simulation saw last frame, so the harness cannot say whether a headset sees this error. A dead zone of two and a half codes on the engine's object motion only took the meadow's isolated eye from 2.0 to 1.6 codes of frame-to-frame change and is not used.

## Sharpening

The sharpness scenes (`sharpness-*`; single-pass, still, the left eye against the reference; PSNR, edge sharpness, ghosting, then the next frame's change in pixels and mean codes):

| Sharpening | Town | Meadow |
|---|---|---|
| 0.5 | 32.69 dB, 1.06, 0.181; 492,000 px, 0.49 | 29.83 dB, 1.05, 0.484; 823,000 px, 0.81 |
| 0.2 | 33.01 dB, 1.01, 0.163; 399,000 px, 0.44 | 30.20 dB, 1.01, 0.428; 686,000 px, 0.70 |
| 0 (RCAS at its mildest) | 33.12 dB, 1.00, 0.151; 361,000 px, 0.40 | 30.33 dB, 0.99, 0.402; 631,000 px, 0.65 |
| Off | 33.29 dB, 0.95, 0.138; 282,000 px, 0.35 | 30.58 dB, 0.95, 0.359; 556,000 px, 0.57 |

Every step down moves the image closer to the reference, with less spurious gradient and less change from frame to frame. Off is 0.28 and 0.38 dB closer than 0.2, and still sharper at the edges than SMAA (0.95 against 0.89 at the town and 0.91 at the meadow), so the sharpening ships off. Figures from different runs differ by a few tenths of a decibel with the clouds and the haze, which is why each comparison is made within one schedule.

## What it costs

The benchmark with the jitter (variants `fsr-jitter-off` and `fsr-off`; single-pass, 72 Hz pacing, three repeats, the release payload, the sharpening off), medians in milliseconds:

| Scene | Mode | FSR (GPU) | GPU | Frame, median / 1% low |
|---|---|---|---|---|
| Capital | FSR | 2.06 | 12.53 | 13.90 / 14.78 |
| Capital | FSR, no jitter | 2.04 | 12.37 | 13.89 / 14.74 |
| Capital | SMAA 1x | 0 | 10.68 | 13.90 / 14.86 |
| Town | FSR | 1.88 | 9.83 | 13.90 / 14.39 |
| Town | FSR, no jitter | 1.86 | 9.81 | 13.90 / 14.39 |
| Town | SMAA 1x | 0 | 8.27 | 13.89 / 14.40 |

The jitter costs nothing measurable: its own work is a matrix product per eye and frame on the CPU. The hybrid decode (variant `fsr-hybrid-off`) costs nothing measurable either. FSR itself is 1.9 to 2.1 ms of GPU time and adds 1.6 to 1.9 ms to the GPU's frame. Every mode holds the 72 Hz pace at both scenes, the 1% low included.

## Against a higher render resolution at the same cost

With the frame GPU-bound, FSR's GPU time could instead buy more pixels with no temporal anti-aliasing at all.

The benchmark at the town (single-pass, 72 Hz pacing, three repeats of six seconds, the release payload, `--gpu-passes` in every run so the per-pass timestamps load each mode alike), median GPU milliseconds per frame. The benchmark has no switch for no anti-aliasing, so that column is the SMAA run less the engine anti-aliasing pass's own time (`post: engine anti-aliasing`) plus the 0.06 ms passthrough blit the engine's no-AA branch still draws:

| Scale (each side) | Per eye | FSR | SMAA 1x | No AA (derived) |
|---|---|---|---|---|
| 1.0 | 3584x3561 | 9.63 | 8.17 | 7.79 |
| 1.15 | 4160x4095 | 11.86 | 9.76 | 9.28 |
| 1.25 | 4480x4451 | 13.26 | 10.79 | 10.23 |
| 1.35 | 4864x4807 | 14.14 | 12.15 | 11.50 |

SMAA at 1.15 costs what FSR does at 1.0, and no anti-aliasing matches it at about 1.18 each side (1.39 times the pixels).

With `vr.resolution_scale` set in the settings, the OpenXR swapchain is created at the scaled size (`vr::swapchain`), the mod's blit copies each eye into it at 1:1, and the runtime's compositor resamples it to the panel inside its lens-distortion pass. The benchmark and the harness change the scale live and keep the swapchain at the session's size, so there the mod's own blit (`vr_blit_ps`) downsamples, with one bilinear tap per output pixel on the sRGB-encoded values. The quality comparison below uses that same filter offline; a compositor's distortion resample is a different filter, which the comparison does not model.

One schedule per scene at one pinned pose, single-pass, the left eye: FSR with the jitter at 1.0, no anti-aliasing and SMAA 1x at 1.15 brought down with the blit's bilinear tap, no anti-aliasing at 1.0, and the usual reference. PSNR against the reference, then the edge sharpness relative to the reference's:

| Scene | FSR, 1.0 | No AA, 1.15 | SMAA, 1.15 | No AA, 1.0 |
|---|---|---|---|---|
| Town, still | 33.71 dB, 0.95 | 34.72 dB, 0.89 | 34.54 dB, 0.85 | 33.27 dB, 0.95 |
| Town, rotating | 32.13 dB, 0.89 | 34.56 dB, 0.89 | 34.33 dB, 0.85 | 32.85 dB, 0.95 |
| Town, translating | 31.13 dB, 0.81 | 34.14 dB, 0.89 | 33.91 dB, 0.85 | 32.38 dB, 0.95 |
| Town, slow pan | 32.59 dB, 0.83 | 34.59 dB, 0.89 | 34.41 dB, 0.85 | 33.00 dB, 0.95 |
| Meadow, still | 30.37 dB, 0.95 | 29.97 dB, 0.87 | 29.95 dB, 0.83 | 28.86 dB, 0.98 |
| Meadow, rotating | 29.44 dB, 0.89 | 30.06 dB, 0.87 | 30.03 dB, 0.83 | 28.95 dB, 0.99 |
| Meadow, translating | 28.82 dB, 0.79 | 30.09 dB, 0.87 | 30.08 dB, 0.83 | 29.00 dB, 0.98 |
| Meadow, slow pan | 32.62 dB, 0.79 | 32.97 dB, 0.86 | 33.01 dB, 0.83 | 31.83 dB, 0.97 |

A reference made by supersampling with no anti-aliasing favours the supersampled candidates, which are the same kind of image at a smaller size, so the PSNR gap overstates their lead. Brought down by area instead of the blit's bilinear tap, no anti-aliasing at 1.15 scores 1.7 to 1.9 dB lower than in the table, below FSR at rest, so the filter matters as much as the method. FSR's motion rows here are at its least favourable jitter offset (next section); averaged over the sequence, FSR on the translating town is 32.0 dB rather than 31.1, still about 2 dB below no anti-aliasing at 1.15.

At a still camera, FSR's next frame changes at 293,000 pixels of an eye at the town and 544,000 at the meadow; no anti-aliasing at 1.15 changes at 32 and 34 pixels, SMAA at 1.15 at 2,900 and 24,000. On the slow pan the flicker is 2.25 codes for FSR against 1.86 for no anti-aliasing at 1.15 and 1.99 for SMAA at 1.15 at the town, and 3.49 against 3.17 and 3.20 at the meadow. The higher resolution is steadier at rest and on the slow pan.

At the edges (crops at four times, the left eye at a still camera, after the blit's downsample), the town's power lines are continuous even-weight curves with FSR; at 1.15 with no anti-aliasing they are still stair-stepped, with shorter, softer steps than at 1.0; SMAA at 1.15 draws smooth but blurrier lines. A wind turbine blade at a shallow angle shows the same. The pole's crossarms and insulators are crisper at 1.15 than with FSR, which softens small dark details a little. The meadow's dithered tree crowns look alike in all four.

At the same GPU time, FSR at 1.0 removes the stair-steps from a still edge and a higher resolution does not. In exchange FSR adds a sub-code shimmer over 2 to 4% of an eye at rest and changes more from frame to frame on a slow pan. For jaggies on a held edge FSR is better; for crawl and shimmer in motion, which is what a headset shows most of the time, the higher resolution is somewhat better. Neither is clearly better overall, and whether the stairs at 1.15 or FSR's shimmer reads worse at the headset's pixel size can only be judged in the headset. SMAA at 1.15 combines the higher resolution's steadiness with FSR's smooth line edges, at a slight softness, and is the third option for that comparison.

## One motion snapshot is one jitter offset

A moving camera's snapshot is taken at a fixed frame of the motion, and the motion restarts the jitter sequence as it begins, so every motion snapshot of a run lands on the same one of the eight offsets, and FSR's score in motion depends on which. A sweep on the town's translating scene, one schedule per pair of phases (each first snapshot and its next frame), single-pass, the left eye, FSR as shipped against each snapshot's own reference (PSNR over the eye, then over its bottom 40%, the near ground that moves fastest):

| Jitter phase (offset in pixels) | FSR | Bottom 40% | No AA |
|---|---|---|---|
| 0 (0, -0.17) | 32.53 dB | 31.45 dB | 32.09 dB |
| 1 (-0.25, +0.17) | 31.86 dB | 30.21 dB | 32.13 dB |
| 2 (+0.25, -0.39) | 32.32 dB | 31.16 dB | 32.11 dB |
| 3 (-0.38, -0.06) | 32.21 dB | 30.88 dB | 32.06 dB |
| 4 (+0.13, +0.28) | 31.39 dB | 29.52 dB | 32.04 dB |
| 5 (-0.13, -0.28) | 32.33 dB | 31.25 dB | 32.03 dB |
| 6 (+0.38, +0.06) | 31.99 dB | 30.52 dB | 32.02 dB |
| 7 (-0.44, +0.39) | 31.06 dB | 29.16 dB | 32.08 dB |

FSR spans 1.5 dB over the sequence, almost all of it on the moving ground, while no anti-aliasing (unjittered) stays within 0.1 dB. The spread follows the offset's vertical component: every phase with a downward offset (positive y, content moved down) scores 0.2 to 1.5 dB below every phase with an upward one, on a scene whose ground moves down the image. Why FSR's resolve in motion is asymmetric in the jitter's vertical sign is not established; it does not show at a still camera. A single motion snapshot is therefore not a fair FSR figure: motion scores are averaged over all eight phases (`fast --motion-phase` with `--next-frame-repeat`), and a figure at one phase carries about plus or minus 0.7 dB on FSR alone. The motion rows of [Against the game's anti-aliasing](#against-the-games-anti-aliasing) are at phase 0, FSR's best; those of the higher-resolution comparison at phase 7, its worst.

## Verdict

With the jitter, FSR under single-pass is the closest to a reference at twice the pixels in nearly every scene measured, sharper at the edges than SMAA, and the only method of the three that draws a power line as a continuous line and a dithered tree crown as foliage. The jitter is the whole difference at rest and costs nothing measurable; FSR's 1.9 to 2.1 ms stays. Its price is temporal: a still image changes by a fraction of a code at 2 to 5% of an eye's pixels from frame to frame, against almost none with SMAA, and a little more than SMAA in motion. Averaged over the jitter sequence, FSR on the translating town is at or slightly below SMAA and no anti-aliasing. Whether the shimmer reads in the headset, at the panel's pixel size and with the compositor's reprojection, the harness cannot say.

Under multipass and the isolated eyes, the grass and the other geometry the velocity pass draws arrive at FSR with several to tens of pixels of false motion, which the jitter turns into flicker on that geometry, and FSR scores below SMAA in motion and at the meadow. The error may be the harness's held camera; it needs a look in the headset, or the multipass velocity encoding understood, before jittered FSR can be called better there.
