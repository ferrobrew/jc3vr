# Where the frame goes at the headset's size

Measured with the frame-time benchmark ([benchmark.md](../testing/benchmark.md)) in runs 300 to 302, and in runs 304 to 306 for the overlapped frame ordering, to answer whether multipass and single-pass stereo fit a 72 Hz headset's 13.9 ms at the Bigscreen Beyond 2e's recommended 3584 by 3561 per eye, what limits each, and what is left to gain. The machine is an AMD Ryzen 9 9950X3D (32 threads) with an NVIDIA GeForce RTX 5090 (32 GB, driver 595.71.05), running the game under Proton Experimental and DXVK in gamescope's headless backend with Monado's simulated headset and null compositor. Every run is the release payload with FSR and the foveation at their shipped settings and the motion blur and depth of field off; the clock and the world run, and only the camera is held. The results are in `target/harness-probe/benchmark/` (`300-pristine-multipass`, `300-main`, `300-sweep`, `301-main`, `301-sweep-town`, `301-variants`, `301-detours`, `302-*`).

The comparison between modes on this machine is the point. A real headset adds its compositor's own GPU work, lens correction, and reprojection, and a player's GPU is not this one: the render size at which the GPU starts to limit the frame moves down on a weaker card.

## A caveat on the frame interval

The null compositor was set to pace at 1000 fps, so the frame interval would show the cost of a frame rather than a display period. It does not entirely: in some runs `xrWaitFrame` blocks for several milliseconds while the GPU sits idle (`gpu_idle_ms` rises with it), and the same scene and mode can land in either state from one session to the next. The capital in single-pass took 19.2 to 19.7 ms per frame in runs 300 and 301's variant schedule and 22.9 ms in run 301's main schedule, the difference being 8 ms of `xrWaitFrame` against 0.03 ms. Monado's application pacing, which delays the frame's wake-up to cut latency, is the cause; [Why the compositor's wait moves](#why-the-compositors-wait-moves) has how it estimates the wake-up. The work inside the frame (the game thread's busy time, the draw task, the GPU spans) does not depend on it, and where the two disagree the work is the number to read.

## At 3584 by 3561

The sections from here to the ranked opportunities were measured with the serial frame ordering, before the render update was overlapped with the draw; [Overlapping the render update](#overlapping-the-render-update) has the same scenes both ways.

Run 301, three ten-second repeats per row, the compositor at 1000 fps. Frame is the median, the 99th percentile (the 1% low), and the worst frame, in milliseconds, with the spread of the repeats' medians; the other times are per-frame medians; the waits are the game thread's (`xrWaitFrame`, and the drains plus the runtime lock the frame tail holds); the last two columns are the host's busy fraction of DXVK's command-stream thread and of the game thread.

| Scene | Mode | Frame | Spread | Game busy | Draw task | GPU | GPU idle | xrWaitFrame | Drain and lock | FSR (GPU) | Submissions | dxvk-cs | Game thread |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| capital | multipass | 20.3 / 25.9 / 31.5 | 20.2-20.9 | 7.4 | 16.2 | 18.5 | 1.8 | 0.0 | 13.6 | 2.2 | 20,612 | 46% | 32% |
| capital | single-pass | 22.9 / 26.7 / 28.4 | 22.9-23.1 | 7.2 | 12.0 | 17.0 | 6.4 | 8.0 | 9.5 | 2.3 | 12,750 | 33% | 30% |
| town | multipass | 14.4 / 19.2 / 23.5 | 14.1-14.7 | 2.7 | 7.4 | 11.4 | 2.9 | 4.2 | 7.1 | 2.0 | 8,106 | 36% | 27% |
| town | single-pass | 9.2 / 13.0 / 16.8 | 9.1-9.3 | 3.0 | 6.0 | 8.8 | 0.3 | 0.0 | 5.6 | 2.2 | 6,036 | 43% | 37% |
| town, moving | multipass | 13.7 / 18.1 / 23.5 | 13.7-13.9 | 2.5 | 7.2 | 11.4 | 2.2 | 3.9 | 6.9 | 2.1 | 7,797 | 36% | 26% |
| town, moving | single-pass | 9.2 / 12.5 / 16.9 | 9.1-9.3 | 3.4 | 5.9 | 8.8 | 0.3 | 0.0 | 5.5 | 2.2 | 5,928 | 43% | 41% |
| tunnel | multipass | 10.8 / 14.4 / 20.2 | 10.7-11.0 | 3.5 | 6.0 | 10.3 | 0.3 | 1.8 | 5.7 | 2.2 | 7,425 | 37% | 35% |
| tunnel | single-pass | 10.6 / 13.2 / 17.1 | 10.5-10.7 | 2.3 | 4.7 | 10.3 | 0.3 | 3.0 | 4.9 | 2.3 | 4,876 | 30% | 28% |
| shore lake | multipass | 10.6 / 13.5 / 18.1 | 10.6-10.6 | 7.0 | 3.8 | 10.3 | 0.3 | 0.0 | 3.7 | 2.2 | 3,987 | 28% | 66% |
| shore lake | single-pass | 10.8 / 13.2 / 16.5 | 10.8-10.8 | 1.2 | 3.3 | 10.4 | 0.4 | 5.9 | 3.5 | 2.4 | 2,826 | 23% | 16% |
| meadow | multipass | 9.9 / 12.4 / 19.4 | 9.9-9.9 | 4.9 | 4.8 | 9.7 | 0.2 | 0.0 | 4.6 | 2.2 | 4,318 | 34% | 48% |
| meadow | single-pass | 10.1 / 12.6 / 15.6 | 10.1-10.1 | 1.5 | 4.1 | 9.7 | 0.3 | 3.8 | 4.5 | 2.3 | 3,468 | 29% | 20% |

Run 300, the same matrix in another session, agreed within the repeats' spread except where `xrWaitFrame` differed: the capital in single-pass took 19.7 ms (19.5 to 21.6) with 0.03 ms of `xrWaitFrame`, and in multipass 21.3.

**Does it fit 13.9 ms?** Single-pass does in every scene but the capital, with a 1% low of 12.5 to 13.2 ms, just inside. Multipass fits at the tunnel, the shore lake, and the meadow (their 1% lows at 12.4 to 14.4 ms) and misses at the town (14.4 ms median, 19.2 ms 1% low) and the capital (20.3 ms). Neither mode fits the capital.

**What limits each.** No thread is saturated: the busiest single thread in any run, a fragment worker running the draw task, was busy 48 to 60% of the time, DXVK's command-stream thread 23 to 46%, and the game thread 16 to 66%. Two shapes account for every row:

- **The GPU**, in the light scenes. The shore lake, the meadow, the tunnel, and single-pass at the town keep the GPU busy for 8.8 to 10.4 ms with under 0.4 ms idle, and their frame is that plus a little. Single-pass does not shorten the shore lake or the meadow at all (their GPU time is the same in both modes), and at the town it saves 2.6 ms of GPU, most of it the prepasses (`PreDraw`, 2.1 ms in multipass, 0.6 ms in single-pass, because multipass's second eye renders some prepass work again).
- **The serial CPU chain**, at the capital and multipass at the town. The deferred frame tail lets the next frame's simulation overlap the draw task, but the next frame's render update needs the OpenXR runtime lock, which the tail holds until it submits the frame, and it cannot submit until the draw task has recorded the whole frame. So the frame is roughly the draw task (12.0 ms in single-pass, 16.2 ms over two dispatches in multipass at the capital) plus the game thread's work between taking the lock and dispatching (the engine's render update, 3.5 ms at the capital) plus `xrWaitFrame`. At the capital in single-pass the game thread spent 9.8 ms per frame waiting on the lock and the tail 12.2 ms waiting on the draw task: one thread's work hands off to the next, and the rest of the machine waits. Multipass adds the mandatory drain between its two dispatches (9.3 ms at the capital).

The GPU spans at the capital (17 to 18.5 ms) do not mean the GPU limits it: the resolution sweep below shows the capital's G-buffer span shrinking from 10.7 to 6.4 ms as the render size grows, which is the signature of a GPU waiting inside the dispatch for the commands to arrive; at a small size the span measures the CPU's pace of submission, not GPU work.

## The resolution sweep

`vr.resolution_scale` was swept live at the capital (run 300) and the town (run 301), three repeats each, the swapchain staying at 3584 by 3561. Medians in milliseconds.

| Scene | Mode | Per eye | MP per eye | Frame | Game busy | Draw task | GPU | G-buffer (GPU) | Scene (GPU) | Post (GPU) | FSR (GPU) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| capital | multipass | 1792x1781 | 3.2 | 21.4 | 7.7 | 17.1 | 17.2 | 10.7 | 2.1 | 0.8 | 0.5 |
| capital | multipass | 2560x2518 | 6.4 | 21.4 | 7.6 | 17.1 | 17.8 | 10.0 | 2.5 | 1.5 | 1.1 |
| capital | multipass | 3584x3561 | 12.8 | 21.6 | 7.8 | 17.2 | 19.5 | 8.5 | 4.3 | 2.9 | 2.2 |
| capital | multipass | 4416x4361 | 19.3 | 22.0 | 7.8 | 17.5 | 21.5 | 7.0 | 6.3 | 4.4 | 3.3 |
| capital | multipass | 5120x5036 | 25.8 | 23.3 | 8.0 | 18.1 | 22.8 | 6.4 | 8.3 | 5.9 | 4.4 |
| capital | single-pass | 1792x1781 | 3.2 | 15.9 | 4.6 | 11.9 | 12.5 | 6.9 | 1.5 | 0.8 | 0.6 |
| capital | single-pass | 2560x2518 | 6.4 | 16.0 | 4.8 | 11.9 | 13.9 | 6.9 | 2.3 | 1.6 | 1.1 |
| capital | single-pass | 3584x3561 | 12.8 | 19.2 | 7.4 | 12.6 | 17.3 | 7.1 | 4.2 | 3.2 | 2.3 |
| capital | single-pass | 4416x4361 | 19.3 | 23.6 | 7.6 | 13.0 | 22.0 | 7.3 | 6.3 | 4.8 | 3.5 |
| capital | single-pass | 5120x5036 | 25.8 | 19.5 | 8.6 | 12.5 | 18.9 | 2.3 | 8.3 | 6.5 | 4.7 |
| town | multipass | 1792x1781 | 3.2 | 10.5 | 2.2 | 7.8 | 8.5 | 3.5 | 1.4 | 0.8 | 0.5 |
| town | multipass | 2560x2518 | 6.4 | 10.8 | 2.2 | 7.9 | 9.2 | 3.0 | 2.0 | 1.5 | 1.0 |
| town | multipass | 3584x3561 | 12.8 | 15.4 | 2.7 | 8.3 | 11.8 | 2.9 | 3.4 | 2.8 | 2.1 |
| town | multipass | 4416x4361 | 19.3 | 14.3 | 3.4 | 8.5 | 13.8 | 2.8 | 4.9 | 4.3 | 3.2 |
| town | multipass | 5120x5036 | 25.8 | 16.6 | 7.6 | 8.8 | 16.2 | 2.3 | 6.4 | 5.8 | 4.3 |
| town | single-pass | 1792x1781 | 3.2 | 9.6 | 2.1 | 6.4 | 7.5 | 3.2 | 1.1 | 0.8 | 0.5 |
| town | single-pass | 2560x2518 | 6.4 | 12.8 | 2.2 | 6.7 | 9.0 | 3.3 | 1.8 | 1.5 | 1.1 |
| town | single-pass | 3584x3561 | 12.8 | 10.6 | 2.5 | 6.7 | 10.2 | 2.6 | 3.2 | 3.1 | 2.2 |
| town | single-pass | 4416x4361 | 19.3 | 13.1 | 2.9 | 6.9 | 12.6 | 1.8 | 4.7 | 4.8 | 3.4 |
| town | single-pass | 5120x5036 | 25.8 | 17.8 | 5.4 | 7.5 | 16.9 | 2.2 | 6.6 | 6.3 | 4.7 |

The frame column carries the pacing caveat: the town's 2560 single-pass row (4.5 ms of `xrWaitFrame`) and 3584 multipass row (5.2 ms) are pacing, not cost. At three times the pixels (6208 by 6168) the capital's frames grew to 51 ms in multipass and over a second in single-pass, and at four times (7168 by 7122) to seconds in both: the GPU's memory was exhausted, and the session crashed in `d3d11.dll` a few runs later, after the town had resized several more times (`target/harness-probe/persistent-300/sessions/2026-11-20_05-30-13_396/crash.log`). The town's rows above come from a fresh session that went no higher than twice the pixels.

**The knee.** The draw task's time is flat across the whole sweep (it is the CPU cost of the draws, independent of the pixels), and the scene and post passes grow linearly with the pixels: about 0.25 ms per megapixel per eye for the scene and 0.23 ms for the post chain, FSR 0.17 ms of that. The GPU therefore takes over from the CPU chain where its pixel-proportional work passes the draw task:

- the capital in multipass stays CPU-bound at 21.4 ms up to the headset's size, and rises only past 1.5 times its pixels (22.0 ms at 19.3 MP, 23.3 ms at 25.8 MP);
- the capital in single-pass sits at 16 ms up to half the headset's pixels and is limited by the GPU and the game thread from there (19.2 ms at the headset's size);
- the town is GPU-bound from about 6 MP per eye in both modes: at the headset's size single-pass's GPU takes 10.2 ms in this session and 8.8 ms in the main schedule, against a draw task of 6 to 7 ms.

At the capital, the game thread's median busy time also grows with the render size (4.6 to 8.6 ms in single-pass). Most of that is not a hidden wait but the median of two populations: a frame that runs the 30 Hz simulation tick is busy about 7.8 ms and one that does not about 3.8 ms, at every size from half the headset's pixels to 1.5 times them, and the tick lands in more than half the frames once the frame takes longer than 16.7 ms (48% of the frames at half the pixels, 64% at the headset's size, from run 300's sweep). At twice the pixels both populations grow by about 2.5 ms, in the engine's render update; what it waits on there was not found.

## What the remaining single-pass submissions cost

Run 301 ran single-pass at the capital and the town with one switch at a time forced off: the draw-count reductions (`vegetation_once`, `hw_instanced_once`), which adds the submissions they save, and the per-eye re-issues of the families still drawn twice (`ssdecal_geometry_per_eye`, `nvwater_per_eye`, `indirect_per_eye`, `instanced_per_eye`), which removes them and draws those families in one eye only (a wrong image, but the submission count and cost of drawing each family once). At the capital (single-pass 19.2 ms, 13,166 submissions, draw task 12.5 ms):

| Variant | Submissions | Draw task | GPU | Frame |
|---|---|---|---|---|
| `hw_instanced_once` off | +590 | +0.73 | +0.69 | +0.05 |
| `vegetation_once` off | +397 | +0.03 | +0.09 | +0.24 |
| `indirect_per_eye` off | -195 | -0.14 | +0.03 | 0.00 |
| `instanced_per_eye` off | -196 | +0.06 | +0.15 | 0.00 |
| `ssdecal_geometry_per_eye` off | -98 | +0.04 | +0.03 | -0.16 |
| `nvwater_per_eye` off | +214 | +0.12 | +0.21 | +0.16 |
| `fsr_per_eye` off | -98 | -0.13 | -0.14 | +0.33 |
| `foveation_per_eye` off | -98 | -0.16 | -0.05 | -0.02 |

At the town every variant landed within the scene's own drift (the repeats of plain single-pass spread from 9.3 to 11.6 ms in that schedule), so no single family moved it measurably. The marginal cost of a submission on the draw task is 0.5 to 1.2 µs: from the buildings' 590 extra submissions, 1.2 µs each, and from multipass against single-pass at the capital (7,900 submissions and 4.2 ms of draw task apart), 0.53 µs. The families still drawn twice add about 1,000 to 1,500 submissions at the capital over the game's own calls (the draw accounting in [single-pass-stereo.md](../stereo/single-pass-stereo.md#accounting-for-the-draw-calls) lists them), so converting all of them would take 0.5 to 1.8 ms off the capital's draw task, and less elsewhere. On the GPU they cost tenths of a millisecond. `fsr_per_eye` and `foveation_per_eye` change how FSR and the foveation treat the two halves, not how much they cost: FSR's own GPU time (2.2 to 2.4 ms a frame at the headset's size) is the same either way.

## The mod's own cost per draw

With `--detour-timing` (run 301), the five draw entry detours took 4.3 ms per frame at the capital in single-pass over 12,960 calls, of which 1.0 ms was the forwarding calls into DXVK (13,311 calls at 75 ns each). A clock-read pair costs 120 ns under Wine, and the timing makes one pair per detour and per forward, 3.2 ms of the remaining 3.3 ms; what is left, the detours' own work around the draws, is under 0.1 ms per frame at the capital (5 ns per draw) and 0.35 ms at the town (59 ns per draw), at the resolution this method allows. In multipass the remainder was within the clock reads' own cost.

The whole set of stereo draw-path detours (the draw entries, the shader binds, the viewports, and the constant stager) costs more than the draw entries' own work: in run 300, multipass in a fresh session with no detours installed took a draw task of 15.3 ms at the capital and 7.3 ms at the town, and the same scenes with the detours installed (as every session that has run single-pass has them) 17.1 and 7.9 ms, so 60 to 90 ns per draw and 0.5 to 1.9 ms per frame in multipass. Single-pass needs them.

The forwarding call itself is cheap: DXVK records a draw in 75 ns on the draw task and replays it on its command-stream thread. The draw task's 0.9 µs per submission is the engine's walk of its render blocks and its state changes around each draw.

## The ranked opportunities

By expected saving at the headset's size on this machine, largest first:

1. **Overlap the next frame's render update with the draw task.** Done (`vr.overlap_render_update`, [below](#overlapping-the-render-update)): at the capital single-pass went from 19.1 to 14.2 ms at the 1000 fps pace, and under 72 Hz pacing from every third display period to every one.
2. **The draw task's CPU time**, which the overlap above leaves as the floor: 12.0 ms for 12,750 submissions at the capital. Converting the families still re-issued per eye takes 0.5 to 1.8 ms of it (above). The rest is the engine's own per-draw work, which the mod can only shorten by issuing fewer draws.
3. **FSR's GPU time**, 2.2 to 2.4 ms per frame at the headset's size in both modes, 22 to 25% of the GPU's frame in the scenes the GPU limits, and growing with the pixels (4.7 ms at twice them). It runs at the full render size as an anti-aliasing and sharpening pass; rendering below the headset's size and letting FSR upscale, or a cheaper resolve, would buy back most of it on a GPU-bound headset, at a cost in image quality. On this GPU every scene the GPU limits already fits 13.9 ms. Run 321 measured FSR off against on in single-pass under 72 Hz pacing: the seam is 1.9 to 2.1 ms, and at the capital FSR turns the 1% low from 17.8 into 27 ms, while unjittered at 1:1 it renders further from a supersampled reference than the game's SMAA (see "Verdict" in [fsr.md](../rendering/fsr.md)).
4. **The multipass detour tax**, 0.5 to 1.9 ms per frame, paid by multipass whenever single-pass has run in the session. It goes away when single-pass becomes the default.
5. **The mod's own CPU work** is not where the time goes: its scoped work is 0.4 to 1.0 ms per frame across all threads (`mod_ms`), and its per-draw work in the draw detours under 0.4 ms.

With the overlap in, the capital in single-pass is limited by the GPU at this machine's 13.7 ms, so the next gains there are the GPU's (FSR first), and the draw task's CPU time becomes the floor again only on a faster GPU.

## Under pacing at 72 Hz

Run 302 launched the session with the null compositor pacing at 72 fps, the headset's slowest refresh rate, and ran the capital, the town, and the meadow, three repeats each.

| Scene | Mode | Frame | Game busy | Draw task | GPU | GPU idle | xrWaitFrame |
|---|---|---|---|---|---|---|---|
| capital | multipass | 41.7 / 45.7 / 48.6 | 7.2 | 17.6 | 19.8 | 21.8 | 19.8 |
| capital | single-pass | 41.7 / 42.9 / 43.3 | 6.8 | 12.3 | 17.8 | 23.9 | 25.4 |
| town | multipass | 13.9 / 16.9 / 30.2 | 2.4 | 7.4 | 11.4 | 2.5 | 4.2 |
| town | single-pass | 13.9 / 14.9 / 28.3 | 2.2 | 6.4 | 11.8 | 2.1 | 5.4 |
| meadow | multipass | 13.9 / 16.0 / 29.6 | 1.9 | 5.0 | 10.8 | 3.1 | 7.0 |
| meadow | single-pass | 13.9 / 14.7 / 83.4 | 1.8 | 4.5 | 11.0 | 2.9 | 7.7 |

The scenes whose work fits hold the period, with single-pass's 1% low closer to it (14.7 to 14.9 ms against 16.0 to 16.9 for multipass). The capital, whose work takes 19 to 20 ms, did not fall to every second period but to every third: 41.7 ms, 24 fps, with the game thread waiting 20 to 25 ms in `xrWaitFrame`. That is the null compositor's pacing, not a headset's, but it shows what missing 13.9 ms costs once a compositor paces: not the 6 ms of overrun but whole periods. A session in the headset (`sessions/2026-11-14_02-05-36_243`) shows 19 to 22 ms frames with about 12.5 ms of wait under a real runtime, the same chain. With the render update overlapped, the capital in single-pass holds the 72 Hz period in the same setup ([below](#at-72-hz-pacing)).

## Overlapping the render update

The serial chain above is now broken where the measurements located it. With `vr.overlap_render_update` on (the default), the next frame's `xrWaitFrame`, view location, and render update run while the previous frame's draw task and deferred tail are still running, and the frame waits for the tail only at its `xrBeginFrame`, just before it dispatches; the per-frame values the draw task reads are kept per frame so it never sees the next frame's. The frame order, why the engine allows it, and what keeps it safe are in [threading.md](../threading.md#who-holds-what-across-a-frame). The benchmark's `multipass-serial` and `single-pass-serial` variants turn it off, so each pair below ran in the same session with the modes rotated, three ten-second repeats each, at 3584 by 3561 with the release payload. Frame is the median, the 99th percentile, and the worst frame interval in milliseconds; the rest are per-frame medians. Tail lock is the game thread's wait for the previous frame's tail, and pose age is how old the head pose a submitted frame was rendered from is at its `xrEndFrame`.

### At 72 Hz pacing

Run 306, the null compositor pacing at 72 fps (`target/harness-probe/benchmark/306-paced`):

| Scene | Mode | Frame | Game busy | Tail lock | Draw task | GPU | GPU idle | xrWaitFrame | Pose age |
|---|---|---|---|---|---|---|---|---|---|
| capital | multipass, serial | 41.6 / 45.7 / 48.3 | 7.1 | 4.3 | 16.8 | 19.1 | 22.5 | 20.6 | 21.0 |
| capital | multipass | 41.7 / 45.3 / 49.6 | 7.0 | 0.0 | 17.1 | 19.4 | 22.3 | 24.7 | 21.3 |
| capital | single-pass, serial | 41.6 / 43.0 / 43.4 | 6.8 | 9.4 | 12.2 | 17.5 | 24.0 | 25.5 | 16.0 |
| capital | single-pass | 13.9 / 16.1 / 34.2 | 4.3 | 0.0 | 10.8 | 13.5 | 0.3 | 9.4 | 14.6 |
| town | multipass, serial | 13.9 / 17.2 / 30.1 | 2.5 | 2.3 | 7.3 | 11.5 | 2.5 | 4.2 | 9.6 |
| town | multipass | 13.8 / 16.6 / 29.2 | 2.5 | 0.0 | 7.3 | 11.5 | 2.4 | 5.8 | 9.7 |
| town | single-pass, serial | 13.9 / 14.7 / 28.3 | 2.3 | 5.6 | 6.3 | 11.8 | 2.1 | 5.4 | 8.4 |
| town | single-pass | 13.9 / 14.8 / 28.0 | 2.3 | 0.0 | 6.3 | 11.7 | 2.1 | 11.5 | 8.4 |
| town, moving | multipass, serial | 13.9 / 24.1 / 30.3 | 2.5 | 2.4 | 7.6 | 11.6 | 2.2 | 3.7 | 9.9 |
| town, moving | multipass | 13.9 / 18.3 / 31.6 | 2.6 | 0.0 | 7.6 | 11.6 | 2.2 | 5.6 | 10.0 |
| town, moving | single-pass, serial | 13.9 / 14.8 / 28.3 | 2.2 | 5.5 | 6.4 | 11.9 | 2.0 | 5.3 | 8.5 |
| town, moving | single-pass | 13.9 / 14.7 / 28.3 | 2.3 | 0.0 | 6.4 | 11.9 | 1.9 | 11.4 | 8.6 |
| tunnel | multipass, serial | 13.9 / 16.1 / 29.0 | 2.3 | 2.0 | 6.2 | 11.9 | 2.0 | 5.3 | 8.5 |
| tunnel | multipass | 13.9 / 16.1 / 29.5 | 2.3 | 0.0 | 6.1 | 11.7 | 2.2 | 7.3 | 8.2 |
| tunnel | single-pass, serial | 13.9 / 14.8 / 28.1 | 2.1 | 4.5 | 5.0 | 11.7 | 2.2 | 6.9 | 7.0 |
| tunnel | single-pass | 13.9 / 14.7 / 28.2 | 2.1 | 0.0 | 4.9 | 11.6 | 2.3 | 11.6 | 6.8 |
| shore lake | multipass, serial | 13.9 / 15.6 / 28.9 | 1.5 | 1.4 | 4.2 | 10.9 | 3.0 | 8.1 | 5.7 |
| shore lake | multipass | 13.9 / 15.5 / 28.6 | 1.5 | 0.0 | 4.1 | 10.9 | 3.0 | 9.4 | 5.6 |
| shore lake | single-pass, serial | 13.9 / 14.4 / 28.2 | 1.4 | 3.2 | 3.7 | 11.0 | 2.8 | 8.9 | 5.0 |
| shore lake | single-pass | 13.9 / 14.3 / 14.8 | 1.4 | 0.0 | 3.7 | 11.0 | 2.8 | 12.5 | 5.0 |
| meadow | multipass, serial | 13.9 / 16.0 / 30.9 | 1.8 | 1.7 | 5.4 | 11.0 | 2.9 | 6.7 | 7.1 |
| meadow | multipass | 13.9 / 16.1 / 30.1 | 1.8 | 0.0 | 5.1 | 10.8 | 3.0 | 8.3 | 6.8 |
| meadow | single-pass, serial | 13.9 / 14.4 / 14.8 | 1.6 | 4.0 | 4.5 | 11.0 | 2.9 | 7.9 | 6.0 |
| meadow | single-pass | 13.9 / 14.5 / 27.9 | 1.6 | 0.0 | 4.4 | 10.9 | 2.9 | 12.2 | 5.9 |

**The capital in single-pass now holds the period.** Serially it fell to every third period (41.6 ms, 24 fps) with the game thread waiting 9.4 ms on the tail; overlapped it holds 13.9 ms with a 99th percentile of 16.1 ms and the GPU busy 13.5 ms with 0.3 ms idle: the GPU, not the CPU chain, is now its limit on this machine. Multipass at the capital does not move: its two dispatches are serial by construction (the drain between them), so its draw task takes 17 ms and its GPU 19 ms, and no ordering of the frame's other work brings that under 13.9 ms. Every other scene held the period in both orderings, with the same 99th percentiles within a millisecond (the moving town's multipass 99th percentile fell from 24.1 to 18.3 ms), and the overlap's tail lock is zero everywhere: the render update and the compositor's wait now cover the whole of the previous frame's tail.

**Head-pose latency did not get worse under pacing.** The pose's age at the submit is the same within 0.3 ms in every scene that held the period in both orderings, and at the capital in single-pass it fell from 16.0 to 14.6 ms, because the frame no longer waits periods for its slot. The overlap does locate the views earlier relative to the frame's own work (before the render update instead of after it), which costs latency only when nothing else waits: see the unpaced numbers below.

### Unpaced

Run 304, the compositor at 1000 fps (`target/harness-probe/benchmark/304-main`):

| Scene | Mode | Frame | Game busy | Tail lock | Draw task | GPU | GPU idle | xrWaitFrame | Pose age |
|---|---|---|---|---|---|---|---|---|---|
| capital | multipass, serial | 21.2 / 26.7 / 38.0 | 7.7 | 4.4 | 17.0 | 19.3 | 1.8 | 0.0 | 21.0 |
| capital | multipass | 23.6 / 32.6 / 36.1 | 8.5 | 0.0 | 17.8 | 19.3 | 5.2 | 9.4 | 22.5 |
| capital | single-pass, serial | 19.1 / 28.3 / 30.4 | 7.3 | 10.1 | 12.6 | 17.2 | 0.4 | 0.0 | 16.6 |
| capital | single-pass | 14.2 / 21.9 / 23.9 | 11.5 | 0.0 | 12.5 | 13.7 | 0.3 | 1.4 | 20.2 |
| town | multipass, serial | 14.9 / 19.8 / 24.0 | 2.7 | 2.4 | 7.8 | 11.7 | 3.3 | 4.9 | 10.3 |
| town | multipass | 9.4 / 15.4 / 21.0 | 4.3 | 0.0 | 7.9 | 9.1 | 0.3 | 0.0 | 11.8 |
| town | single-pass, serial | 9.5 / 13.5 / 16.8 | 3.0 | 5.8 | 6.1 | 8.9 | 0.3 | 0.0 | 9.0 |
| town | single-pass | 9.2 / 14.0 / 15.8 | 4.4 | 0.0 | 6.5 | 8.9 | 0.3 | 5.1 | 10.8 |
| town, moving | multipass, serial | 14.7 / 19.5 / 23.0 | 2.6 | 2.4 | 7.7 | 11.5 | 3.1 | 4.4 | 10.2 |
| town, moving | multipass | 9.7 / 15.1 / 23.7 | 4.2 | 0.0 | 8.1 | 9.4 | 0.3 | 0.0 | 11.9 |
| town, moving | single-pass, serial | 9.3 / 12.9 / 17.0 | 3.1 | 5.7 | 6.1 | 8.9 | 0.3 | 0.0 | 8.9 |
| town, moving | single-pass | 9.1 / 13.9 / 18.5 | 4.5 | 0.0 | 6.8 | 8.9 | 0.3 | 4.7 | 11.0 |
| tunnel | multipass, serial | 10.8 / 14.0 / 21.1 | 3.7 | 2.2 | 6.1 | 10.3 | 0.3 | 1.8 | 9.0 |
| tunnel | multipass | 10.8 / 16.0 / 21.7 | 4.6 | 0.0 | 6.6 | 10.4 | 0.3 | 0.0 | 10.8 |
| tunnel | single-pass, serial | 10.7 / 13.1 / 17.0 | 2.2 | 4.7 | 4.6 | 10.3 | 0.3 | 3.1 | 7.5 |
| tunnel | single-pass | 11.4 / 18.0 / 21.0 | 4.1 | 0.0 | 5.2 | 10.4 | 0.3 | 8.6 | 7.9 |
| shore lake | multipass, serial | 9.9 / 12.3 / 17.5 | 4.6 | 2.0 | 4.9 | 9.7 | 0.2 | 0.0 | 9.2 |
| shore lake | multipass | 10.0 / 15.4 / 17.5 | 3.7 | 0.0 | 5.2 | 9.7 | 0.2 | 3.0 | 9.4 |
| shore lake | single-pass, serial | 10.1 / 12.7 / 18.1 | 1.3 | 4.4 | 3.9 | 9.8 | 0.3 | 4.2 | 5.9 |
| shore lake | single-pass | 10.7 / 17.4 / 20.5 | 2.6 | 0.0 | 4.1 | 9.9 | 0.3 | 9.0 | 5.8 |
| meadow | multipass, serial | 10.1 / 12.4 / 19.6 | 4.8 | 1.8 | 4.8 | 9.8 | 0.2 | 1.1 | 9.3 |
| meadow | multipass | 10.2 / 15.3 / 19.7 | 4.0 | 0.0 | 5.1 | 9.8 | 0.2 | 1.2 | 9.6 |
| meadow | single-pass, serial | 10.1 / 12.7 / 17.7 | 1.5 | 4.3 | 3.9 | 9.8 | 0.3 | 4.1 | 6.0 |
| meadow | single-pass | 10.7 / 17.4 / 20.6 | 3.2 | 0.0 | 4.1 | 9.9 | 0.3 | 8.6 | 6.3 |

Unpaced, the overlap takes 4.9 ms off the capital in single-pass (19.1 to 14.2 ms, the GPU now busy 13.7 ms with 0.3 ms idle) and 5 ms off the town in multipass (14.9 to 9.4 ms; its GPU idle fell from 3.3 to 0.3 ms). Three things got worse, and all three are this pacing's:

- **The 99th percentile of the light scenes**, which the GPU limits in either ordering, rose by 3 to 5 ms (the shore lake in single-pass from 12.7 to 17.4 ms). In the shore lake's first single-pass repeat the slowest 3% of the frames spent 14.7 ms on average in `xrWaitFrame` against 5.2 ms serially: the runtime's application pacing, not work. At 72 Hz the same scenes show no difference.
- **The capital in multipass** got 2.4 ms slower, all of it in `xrWaitFrame` (9.4 ms against none).
- **The pose's age** grew by 1 to 3.6 ms (16.6 to 20.2 ms at the capital in single-pass), because the views are located before the render update and the compositor did not hold the frame anywhere else.

The game thread's busy time grows under the overlap when the frames are unpaced (the capital in single-pass from 7.3 to 11.5 ms): the render update now shares the CPU and the engine's locks with the draw task, so the same work takes longer on the clock. Under 72 Hz pacing it does not (at the capital in single-pass, 6.8 ms serially against 4.3 overlapped).

### Why the compositor's wait moves

Monado's application pacing (`u_pacing_app` in its source, read at a 2022 revision) predicts each frame from three filtered times: from the wake-up out of `xrWaitFrame` to `xrBeginFrame` (the CPU time), from `xrBeginFrame` to `xrEndFrame` (the draw time), and from `xrEndFrame` to the GPU's completion. It rounds the frame period up to a multiple of the display period that is longer than each of the three, picks the first display time after the last one it returned by at least half that period with room for all three, and wakes the application that long before it. That is the wait the earlier runs saw with the GPU idle even at 1000 fps: with the frame begun before the render update, the draw time covered the render update and the draw task (16 to 20 ms at the capital), so the period was held at that length whatever the GPU did, and the filtered estimates drifting between sessions put the wake-up sometimes early and sometimes late. Overlapped, the render update moves into the CPU time and the draw time shrinks to the dispatch and the draw task, which is why the single-pass capital drops to every period at 72 Hz. In multipass the draw time still spans both dispatches and the drain between them, about 18 ms; why the unpaced multipass capital then waits 9.4 ms rather than running at that length was not traced through the estimates.

### Correctness

The fast comparison ran the town, the tunnel, the meadow, and the capital at night with the overlap on and off, in run 304 (1000 fps) and run 306 (72 fps), and with the previous commit's payload once in run 304 (`target/harness-probe/fast/304-*` and `306-*`). Under the visual harness the frame tail runs inline, so the comparison exercises the reordered frame, the split runtime locks, and the per-frame slots, but not the overlap's concurrency. The two orderings came out alike: multipass against the isolated eyes passed or stayed within its noise in the tunnel and the capital at night in every run (at most 54 differing pixels per eye) and differed by 5,000 to 45,000 pixels per eye in the town and the meadow with all three payloads, the noise floor of those scenes at this pace being 2,000 to 40,000. Single-pass differed from the references by a few hundred pixels at the capital at night and 10,000 to 1,200,000 in the other three scenes in the final image, with 1 to 7 million differing depth pixels per eye, in every run with every payload, the previous commit's included, and by factors of up to three between repeats of the same payload; so it does not come from the ordering. Its cause (the release build, the headset's size, or these sessions) was not found.

A first launch of the change (run 303) crashed in `d3d11.dll` inside the tone mapping's histogram readback during the render update, ten seconds in: the runtime's swapchain release and `xrEndFrame` on the frame tail recorded on the immediate context without the engine's context mutex while the overlapped render update read a query under it. Those calls now take the mutex, and runs 304 and 306 ran every benchmark and comparison above without a fault. Run 305 crashed once, seven seconds after re-injecting the release payload, while staging the capital: a streaming worker loading a model faulted in the texture cache's lookup (`TextureCacheGetTexture`) on a null resource handle. Run 306 staged the same scene with the same payload without a fault, and no path was found from the overlap to that lookup, but it was not ruled out.

## The motion blur and the depth of field

Both are forced off in VR (see "The motion blur and the depth of field" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)). Run 302 measured the capital and the town with a build that let them run as configured before the change (the motion blur switch at its default, the depth of field's gate the engine's own) and then with the change, in the same session. The post effects' GPU time less FSR's fell by 0.10 to 0.17 ms per frame in all four scene and mode pairs (0.83 to 1.03 ms before, 0.72 to 0.92 after); the frame time did not move outside the repeats' spread. The motion blur's pass was what ran: the depth of field's own passes were already inactive in these views, and the draw census of the town (`target/harness-probe/persistent-302/config/captures/blur_census_town_8fd40ee2_*_1.json`, named with `python3 -m tools.rendering census --bundle`) shows the post chain drawing the plain composite (`comp_pg_cc_ef`) and no motion blur, bokeh, or depth-of-field program in either mode.
