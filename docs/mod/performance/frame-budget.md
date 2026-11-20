# Where the frame goes at the headset's size

Measured with the frame-time benchmark ([benchmark.md](../testing/benchmark.md)) in runs 300 to 302, to answer whether multipass and single-pass stereo fit a 72 Hz headset's 13.9 ms at the Bigscreen Beyond 2e's recommended 3584 by 3561 per eye, what limits each, and what is left to gain. The machine is an AMD Ryzen 9 9950X3D (32 threads) with an NVIDIA GeForce RTX 5090 (32 GB, driver 595.71.05), running the game under Proton Experimental and DXVK in gamescope's headless backend with Monado's simulated headset and null compositor. Every run is the release payload with FSR and the foveation at their shipped settings and the motion blur and depth of field off; the clock and the world run, and only the camera is held. The results are in `target/harness-probe/benchmark/` (`300-pristine-multipass`, `300-main`, `300-sweep`, `301-main`, `301-sweep-town`, `301-variants`, `301-detours`, `302-*`).

The comparison between modes on this machine is the point. A real headset adds its compositor's own GPU work, lens correction, and reprojection, and a player's GPU is not this one: the render size at which the GPU starts to limit the frame moves down on a weaker card.

## A caveat on the frame interval

The null compositor was set to pace at 1000 fps, so the frame interval would show the cost of a frame rather than a display period. It does not entirely: in some runs `xrWaitFrame` blocks for several milliseconds while the GPU sits idle (`gpu_idle_ms` rises with it), and the same scene and mode can land in either state from one session to the next. The capital in single-pass took 19.2 to 19.7 ms per frame in runs 300 and 301's variant schedule and 22.9 ms in run 301's main schedule, the difference being 8 ms of `xrWaitFrame` against 0.03 ms. Monado's application pacing, which delays the frame's wake-up to cut latency, is the likely cause; it was not confirmed. The work inside the frame (the game thread's busy time, the draw task, the GPU spans) does not depend on it, and where the two disagree the work is the number to read.

## At 3584 by 3561

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

At the capital, the game thread's own busy time also grows with the render size (4.6 to 8.6 ms in single-pass), which the simulation and the render update cannot do on their own: something on the game thread waits on the GPU without being timed as a wait. Which call it is was not found.

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

1. **Overlap the next frame's render update with the draw task.** At the capital the game thread waits 9.8 ms per frame for the runtime lock while the draw task records the previous frame, then does 3.5 ms of render update, then dispatches. OpenXR's frame pipelining lets a frame's `xrWaitFrame` come after the previous frame's `xrBeginFrame` and before its `xrEndFrame` (only the next `xrBeginFrame` must follow that `xrEndFrame`; how Monado and the headset's runtime pace such a wait is unverified), so the next frame's wait, view location, and render update could run while the tail is still draining, taking the lock only to begin the frame and dispatch. That would bring the capital from about 19 ms toward the larger of the draw task and the game thread's work, about 12.5 ms in single-pass: 4 to 7 ms at the capital. The GPU-bound scenes would gain little, since the GPU already runs nearly without idle there. It is a change to the frame loop's ownership of the runtime lock (`docs/mod/threading.md`), not to rendering.
2. **The draw task's CPU time**, which the overlap above leaves as the floor: 12.0 ms for 12,750 submissions at the capital. Converting the families still re-issued per eye takes 0.5 to 1.8 ms of it (above). The rest is the engine's own per-draw work, which the mod can only shorten by issuing fewer draws.
3. **FSR's GPU time**, 2.2 to 2.4 ms per frame at the headset's size in both modes, 22 to 25% of the GPU's frame in the scenes the GPU limits, and growing with the pixels (4.7 ms at twice them). It runs at the full render size as an anti-aliasing and sharpening pass; rendering below the headset's size and letting FSR upscale, or a cheaper resolve, would buy back most of it on a GPU-bound headset, at a cost in image quality. On this GPU every scene the GPU limits already fits 13.9 ms.
4. **The multipass detour tax**, 0.5 to 1.9 ms per frame, paid by multipass whenever single-pass has run in the session. It goes away when single-pass becomes the default.
5. **The mod's own CPU work** is not where the time goes: its scoped work is 0.4 to 1.0 ms per frame across all threads (`mod_ms`), and its per-draw work in the draw detours under 0.4 ms.

The game thread's unexplained growth with the render size at the capital (up to 4 ms between half and twice the headset's pixels) is worth finding before any of these, since a wait hidden in the render update would sit on the critical path the overlap creates.

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

The scenes whose work fits hold the period, with single-pass's 1% low closer to it (14.7 to 14.9 ms against 16.0 to 16.9 for multipass). The capital, whose work takes 19 to 20 ms, did not fall to every second period but to every third: 41.7 ms, 24 fps, with the game thread waiting 20 to 25 ms in `xrWaitFrame`. That is the null compositor's pacing, not a headset's, but it shows what missing 13.9 ms costs once a compositor paces: not the 6 ms of overrun but whole periods. A session in the headset (`sessions/2026-11-14_02-05-36_243`) shows 19 to 22 ms frames with about 12.5 ms of wait under a real runtime, the same chain.

## The motion blur and the depth of field

Both are forced off in VR (see "The motion blur and the depth of field" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)). Run 302 measured the capital and the town with a build that let them run as configured before the change (the motion blur switch at its default, the depth of field's gate the engine's own) and then with the change, in the same session. The post effects' GPU time less FSR's fell by 0.10 to 0.17 ms per frame in all four scene and mode pairs (0.83 to 1.03 ms before, 0.72 to 0.92 after); the frame time did not move outside the repeats' spread. The motion blur's pass was what ran: the depth of field's own passes were already inactive in these views, and the draw census of the town (`target/harness-probe/persistent-302/config/captures/blur_census_town_8fd40ee2_*_1.json`, named with `python3 -m tools.rendering census --bundle`) shows the post chain drawing the plain composite (`comp_pg_cc_ef`) and no motion blur, bokeh, or depth-of-field program in either mode.
