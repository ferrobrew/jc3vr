# Where the frame goes at the headset's size

Measured with the frame-time benchmark ([benchmark.md](../testing/benchmark.md)) in runs 300 to 302, in runs 304 to 306 for the overlapped frame ordering, and in runs 321 to 326 for where the draw task's time goes and where its threads run, to answer whether multipass and single-pass stereo fit a 72 Hz headset's 13.9 ms at the Bigscreen Beyond 2e's recommended 3584 by 3561 per eye, what limits each, and what is left to gain. The machine is an AMD Ryzen 9 9950X3D (32 threads) with an NVIDIA GeForce RTX 5090 (32 GB, driver 595.71.05), running the game under Proton Experimental and DXVK in gamescope's headless backend with Monado's simulated headset and null compositor. Every run is the release payload with FSR and the foveation at their shipped settings and the motion blur and depth of field off; the clock and the world run, and only the camera is held. The results are in `target/harness-probe/benchmark/` (`300-pristine-multipass`, `300-main`, `300-sweep`, `301-main`, `301-sweep-town`, `301-variants`, `301-detours`, `302-*`).

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

## Where the draw task's time goes

The draw task at the capital was taken to be the engine's own walk of its render blocks. Sampling it says otherwise. Runs 321 and 323 sampled the draw task's thread about once a millisecond with `--sample-draw-task` ([benchmark.md](../testing/benchmark.md#where-the-draw-tasks-time-goes)) at the capital in single-pass with FSR off under 72 Hz pacing, two ten-second runs pooled (13,450 samples, a median draw task of 10.1 ms, the threads where the scheduler put them). Each sample is charged to the module its instruction pointer was in, and time in DXVK or Wine to the innermost game or payload function that called it, by unwinding the sampled stack:

| Whose code, and where the instruction pointer was | Share | ms per frame |
|---|---|---|
| the game, inside DXVK | 46.6% | 4.72 |
| the game's own code | 14.4% | 1.46 |
| the game, inside Wine's `ntdll` | 12.2% | 1.24 |
| the payload's own code | 12.1% | 1.23 |
| the payload, inside DXVK | 7.2% | 0.73 |
| the game or the payload, inside the C runtime | 3.7% | 0.38 |
| the payload, inside Wine's `ntdll` | 1.7% | 0.17 |
| the WaveWorks library | 1.7% | 0.17 |

By engine seam, the G-buffer range took 5.6 ms, the prepasses 2.1 ms, the scene range 1.8 ms, the post effects under 0.1 ms, and the stretches outside any pass 0.6 ms. By render pass the models dominate ([`RP_MODELS_DYNAMIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_DYNAMIC) 2.2 ms, [`RP_MODELS_STATIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_STATIC) 1.5 ms, [`RP_MODELS_TRANSPARENT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_TRANSPARENT) 1.0 ms), then the second shadow cascade (0.5 ms), the opaque vegetation (0.5 ms), the water (0.4 ms), the depth and velocity prepass and the screen-space decals (0.4 ms each); by render-block type [`GeneralMkIII`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMkIII) took 3.8 ms, and the car paint, the windows, the foliage, the terrain patches, the bark, the characters, the water, and the decals 0.35 to 0.7 ms each.

Nothing in the draw task waits. Wine's share is the uncontended fast path of the engine's own context lock ([`Graphics::AcquireThreadOwnership`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw##func-AcquireThreadOwnership) and its siblings take a critical section around each call into Direct3D), not a blocked thread, and the task is one engine fragment on one thread from its start to its end. DXVK's command-stream thread was busy 48% of the time. So the frame at the capital was bound by one thread's CPU time, and most of that time was DXVK recording the engine's state changes: the engine's binds of a vertex constant buffer ([`Graphics::SetVertexProgramConstantBufferNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw##func-SetVertexProgramConstantBufferNoMutex)) alone took 1.1 ms, its vertex stream binds 0.5 ms, and its constant-buffer maps 0.45 ms. The instructions DXVK spends that time on are, by the sampled addresses in its `d3d11.dll`, the atomic reference-count increments of a binding that changes, and a pointer chase in a free list: memory traffic, not computation.

What the per-eye submissions cost on the draw task, read off the same samples: the second eye of a render block's per-eye re-issue took 0.32 ms (the water 0.13, the screen-space decals 0.10, the vegetation's hardware-instanced draws 0.08), and the first eye of those blocks 0.57 ms. The draw-level per-eye submissions (the terrain patches' passes, the instanced and indirect re-issues, the split fullscreen passes) carry no marker, but their families come to under 0.7 ms at the capital. The prepasses are not re-issued per eye: no sample in a shadow or reflection pass was inside a re-issue, and the collapse runs `PreDraw` once (see "Why `PreDraw` is outside the collapse" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)).

### The payload's share, and what removing its locks bought

The payload's own instructions were 1.9 ms of the draw task in run 321's first sampled run, 0.66 ms of it on the two mutexes the collapse's viewport split took on every routed draw and 0.29 ms on the census's submission counter. The viewport records are now read through a sequence lock and the per-draw counters bumped with a plain load and store (see "The viewport record" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)), and a routed draw whose viewport slots already hold its split binds nothing. That took the payload's own instructions to 1.2 ms, but most of what it removed reappeared in the next locked instruction: a locked instruction waits for the stores before it to drain, and after DXVK has recorded a call there are many, so removing one moves the wait to the engine's context lock that follows. Interleaving the two payloads at the capital in one session (run 325, four injections each, alternating) put the draw task at 11.70 and 11.50 ms with FSR off and 11.68 and 11.43 ms in single-pass, before and after: a quarter of a millisecond, inside the runs' spread of 10.7 to 12.2 ms. Skipping the redundant viewport binds was worth under a tenth of a millisecond (run 323, interleaved with `single_pass.viewport_bind_cache` off).

## Where the draw task's threads run

A draw task that is memory traffic depends on where it runs. The machine's processor has two dies with eight cores each: one die's cores share a 96 MB L3 (the stacked cache), the other's a 32 MB one, and the scheduler, left to itself, put the draw task and DXVK's command-stream thread on the smaller-cache die most of the time (sampled from `/proc`). Run 325 pinned the whole game process to one die or the other, or left it free, three cycles with the order rotated, at the capital under 72 Hz pacing:

| Cores | Mode | Draw task | Per 1,000 submissions | GPU | Frame 99th percentile |
|---|---|---|---|---|---|
| free | single-pass, FSR off | 11.45 | 0.86 | 13.44 | 16.43 |
| the 32 MB die | single-pass, FSR off | 11.44 | 0.87 | 13.50 | 16.20 |
| the 96 MB die | single-pass, FSR off | 6.39 | 0.49 | 10.98 | 14.94 |
| free | single-pass | 11.60 | 0.88 | 13.44 | 17.13 |
| the 32 MB die | single-pass | 11.53 | 0.89 | 13.50 | 16.66 |
| the 96 MB die | single-pass | 6.51 | 0.50 | 12.73 | 14.85 |

On the die with the larger cache the draw task takes 56% of the time, for the same submissions: the engine's and DXVK's working set misses the smaller cache. The GPU's span falls with it (13.4 to 11.0 ms with FSR off): the span had been the GPU waiting for commands. `vr.large_cache_cores` (on by default; [`cpu_placement`](../../../crates/jc3vr-core/src/cpu_placement.rs)) narrows the process's affinity to the cores of the largest L3 when the L3 caches differ in size, and leaves a processor with one size of L3 alone. Threads created afterwards inherit the affinity; the threads the graphics driver created natively before it (its shader compilers and its event handler) keep theirs.

Run 325, the switch on against off in one schedule with the modes rotated, three repeats each, under 72 Hz pacing. Frame is the median, the 99th percentile, and the worst frame; the rest are medians.

| Scene | Mode | Cores | Frame | Game busy | Draw task | GPU | GPU idle | Submissions |
|---|---|---|---|---|---|---|---|---|
| capital | single-pass, FSR off | large cache | 13.89 / 14.90 / 28.7 | 3.37 | 6.61 | 11.06 | 2.81 | 13,473 |
| capital | single-pass, FSR off | free | 13.91 / 19.64 / 31.2 | 5.35 | 11.88 | 13.49 | 0.34 | 13,332 |
| capital | single-pass | large cache | 13.90 / 14.99 / 28.8 | 3.38 | 6.61 | 12.73 | 1.04 | 12,882 |
| capital | single-pass | free | 13.85 / 16.33 / 34.3 | 4.61 | 11.36 | 13.47 | 0.35 | 13,127 |
| capital | multipass | large cache | 13.90 / 15.95 / 28.8 | 3.58 | 8.73 | 12.51 | 1.35 | 21,266 |
| capital | multipass | free | 41.65 / 45.22 / 47.9 | 8.11 | 18.22 | 20.35 | 21.39 | 21,211 |
| town | single-pass, FSR off | large cache | 13.90 / 14.40 / 28.0 | 1.65 | 4.16 | 8.37 | 5.52 | 5,802 |
| town | single-pass, FSR off | free | 13.91 / 14.85 / 28.5 | 2.40 | 6.77 | 10.45 | 3.47 | 6,057 |
| town | single-pass | large cache | 13.89 / 14.38 / 28.0 | 1.67 | 4.09 | 9.83 | 4.04 | 6,237 |
| town | single-pass | free | 13.89 / 14.72 / 28.0 | 2.41 | 6.75 | 12.04 | 1.84 | 5,961 |
| town | multipass | large cache | 13.89 / 15.20 / 28.7 | 1.85 | 4.75 | 9.79 | 4.09 | 8,222 |
| town | multipass | free | 13.90 / 18.14 / 30.6 | 2.64 | 7.60 | 11.58 | 2.33 | 8,392 |
| tunnel | single-pass, FSR off | large cache | 13.89 / 14.33 / 27.9 | 1.56 | 3.10 | 8.73 | 5.14 | 5,148 |
| tunnel | single-pass, FSR off | free | 13.91 / 14.62 / 28.3 | 2.12 | 5.00 | 9.77 | 4.13 | 4,864 |
| tunnel | single-pass | large cache | 13.89 / 14.34 / 14.7 | 1.51 | 3.04 | 10.54 | 3.33 | 4,928 |
| tunnel | single-pass | free | 13.88 / 14.66 / 27.8 | 2.10 | 5.01 | 11.52 | 2.29 | 4,881 |
| tunnel | multipass | large cache | 13.89 / 15.26 / 28.1 | 1.60 | 3.92 | 10.44 | 3.44 | 7,361 |
| tunnel | multipass | free | 13.88 / 16.54 / 31.2 | 2.34 | 6.21 | 11.80 | 2.06 | 7,678 |

The capital in single-pass now has headroom under 13.9 ms rather than sitting on it: a draw task of 6.6 ms, the GPU idle 1 to 2.8 ms a frame, and a 99th percentile of 14.9 to 15.0 ms with FSR on or off (16.3 and 19.6 ms on the free cores). The capital in multipass, which fell to every third period on the free cores, holds the period too. The game thread's busy time falls by a third as well: the render update and the simulation share the large cache.

Unpaced (run 326, the compositor at 1000 fps, the same schedule), where the frame interval is the frame's cost:

| Scene | Mode | Cores | Frame | Game busy | Draw task | GPU | GPU idle |
|---|---|---|---|---|---|---|---|
| capital | single-pass, FSR off | large cache | 9.09 / 13.71 / 17.7 | 3.80 | 6.72 | 8.59 | 0.34 |
| capital | single-pass, FSR off | free | 13.66 / 21.32 / 24.0 | 11.71 | 12.10 | 13.02 | 0.35 |
| capital | single-pass | large cache | 10.61 / 16.34 / 17.9 | 5.33 | 6.69 | 10.20 | 0.36 |
| capital | single-pass | free | 13.82 / 20.76 / 22.7 | 8.99 | 11.88 | 13.35 | 0.36 |
| capital | multipass | large cache | 11.61 / 16.29 / 24.8 | 4.29 | 9.35 | 11.23 | 0.28 |
| capital | multipass | free | 22.93 / 31.27 / 33.3 | 8.48 | 17.24 | 18.89 | 4.51 |
| town | single-pass, FSR off | large cache | 7.26 / 11.59 / 13.6 | 3.25 | 4.23 | 6.93 | 0.33 |
| town | single-pass, FSR off | free | 8.21 / 13.11 / 17.6 | 2.87 | 6.51 | 7.55 | 0.32 |
| town | single-pass | large cache | 9.66 / 14.36 / 17.1 | 1.73 | 4.24 | 8.57 | 0.34 |
| town | single-pass | free | 8.85 / 13.44 / 17.3 | 4.01 | 6.97 | 8.56 | 0.33 |
| town | multipass | large cache | 8.94 / 13.64 / 19.3 | 3.76 | 5.11 | 8.63 | 0.26 |
| town | multipass | free | 9.70 / 15.45 / 22.8 | 4.08 | 8.01 | 9.32 | 0.26 |
| tunnel | single-pass, FSR off | large cache | 9.42 / 14.85 / 18.4 | 2.34 | 3.11 | 8.42 | 0.34 |
| tunnel | single-pass, FSR off | free | 8.80 / 14.12 / 15.6 | 4.02 | 4.64 | 8.40 | 0.34 |
| tunnel | single-pass | large cache | 10.64 / 20.62 / 22.0 | 4.20 | 3.17 | 10.25 | 0.35 |
| tunnel | single-pass | free | 11.28 / 17.71 / 21.5 | 4.02 | 4.97 | 10.26 | 0.35 |
| tunnel | multipass | large cache | 10.61 / 15.59 / 17.7 | 4.26 | 4.20 | 10.22 | 0.28 |
| tunnel | multipass | free | 10.67 / 16.13 / 18.6 | 4.59 | 6.44 | 10.25 | 0.27 |

The capital in single-pass costs 10.6 ms a frame on the large cache's cores against 13.8 ms on the free ones, and 9.1 ms with FSR off, with its GPU busy to within 0.4 ms of the frame: the GPU, not the draw task, now limits the heaviest scene on this machine. The town and the tunnel were GPU-bound before and stay so; their draw tasks shrink by a third, and their frame times move within the unpaced runs' pacing noise (see "Unpaced" under [Overlapping the render update](#overlapping-the-render-update), whose 99th percentiles moved the same way).

The sampled draw task with the placement on (run 326, the capital, single-pass, FSR off, unpaced, 13,521 samples, a median of 6.8 ms): DXVK's share fell from 5.5 to 2.9 ms and the game's own code from 1.5 to 0.9 ms, while the engine's context lock in Wine (1.4 ms) and the payload's own work (1.2 ms, plus 0.5 ms of its calls into DXVK) stayed where they were, so the locked instructions and the mod's per-draw bookkeeping are now the larger shares of a smaller task.

### The multipass detour tax

Run 326 also ran multipass in the fresh session before anything installed the stereo draw path's detours, and again after, unpaced, three repeats each. With the placement on, the detours cost 0.74 ms of draw task at the capital (8.61 to 9.35 ms), 0.55 ms at the town, and 0.35 ms at the tunnel; on the free cores 1.8, 0.6, and 0.7 ms. Not made free when single-pass is off: what the detours do for a multipass draw (the census's count, the harness's probes, the water and velocity bookkeeping) is a few hundredths of a millisecond each in the samples, and a path that skipped them would have to know every probe that wants multipass draws.

### The per-eye submissions at the capital

The census at the capital ([`coverage_capital_census_headset.toml`](../../../tools/rendering/experiments/coverage_capital_census_headset.toml), run 326, the second of three frames, the accounting of [single-pass-stereo.md](../stereo/single-pass-stereo.md#accounting-for-the-draw-calls)): 14,314 game calls, 14,978 single-pass submissions, 20,094 multipass submissions. Single-pass submits 664 beyond one per game call:

| Family | How | Extra submissions |
|---|---|---|
| `SSDecal` | block re-issue | 170 |
| `VolumetricTerrainPatch` (tessellated colour, near, and far, the GPU-indirect near basemesh, the detail) | per-eye re-issue | 136 |
| `VegetationFoliage`, `vegetationfoliage*hwinstanced` | hardware-instanced re-issue inside a block drawn once | 124 |
| `NvWaterHighEnd` | block re-issue | 122 |
| `VegetationBark`, `vegetationbark*hwinstanced` | hardware-instanced re-issue inside a block drawn once | 72 |
| `ParticleRB`, tessellated | per-eye re-issue | 16 |
| split fullscreen passes, the occluder, the sun, the moon | second run or re-issue | 24 |

No prepass submits anything per eye: every shadow cascade, the reflection passes, and the vegetation and rain prepasses submit once, as the game calls them. At the placed draw task's 0.5 µs a submission, the 664 cost about 0.3 ms, which the samples' 0.3 ms for the second eye of the block re-issues agrees with.

### Correctness

The standing fast suite ran with the previous commit's payload (run 324) and with these changes (run 326). Every scene's depth, albedo, and normals against the isolated eyes and against multipass stayed within a few percent of the previous payload's or fell, and the final image of single-pass against multipass stayed within the repeat's range (the town 53,323 and 61,645 pixels per eye against 51,491 and 59,585 before; the capital at night 213 and 205 against 219 and 206; the meadow, the shore, and the tunnel, which came out high against their isolated references in the first schedule, back within their earlier values in a repeat: the meadow 159,963 and 199,192 against 154,712 and 195,602, the tunnel 2,378 and 5,957 against 3,769 and 9,296). The isolated references themselves moved between schedules by up to 700,000 pixels at the shore, so the comparisons against them carry that noise. An intermediate build that recorded the engine's viewport bind after the eye camera read it put the left eye's final image 800,000 pixels off in every town view while leaving the G-buffer untouched; the suite caught it and the order is fixed. The placement changes no image.

## Where the GPU's time goes

Run 333 timed every render pass on the GPU, and each post effect and the foveation's two passes, with `--gpu-passes` ([benchmark.md](../testing/benchmark.md)): single-pass at the town and the capital (midday), 3584 by 3561 per eye, FSR and the foveation as shipped, the null compositor pacing at 72 Hz, three ten-second repeats each, the release payload (`target/harness-probe/benchmark/333-passes-town`, `333-passes-capital`). The capital was staged with the payload the session launched with and measured after the release payload was re-injected. Each figure is a pass's GPU time per frame, the mean over the repeats; the share is of the dispatch's GPU span, 9.9 ms at the town and 12.4 ms at the capital, with 4.0 and 1.5 ms of idle between frames (the 72 Hz pace's headroom). A pass's time is its execution window, idle inside it included, so a geometry pass the draw task feeds slowly reads long; with the draw task at 4.2 and 6.0 ms against these spans the GPU leads, so the windows are mostly work. The pass timestamps cost nothing measurable: two runs at the capital without them took a span of 12.8 ms against 12.3 with them, inside the scene's own drift. Engine pass names are the engine's own, without the `RP_` prefix.

Town (GPU span 9.9 ms):

| Pass | GPU ms | Share |
|---|---|---|
| FSR (the mod's resolve, inside the post block) | 1.89 | 19% |
| SSAO and the AO volumes | 0.87 | 9% |
| Terrain patches, the forest, and the roads (`TERRAINPATCH_DETAIL_MID` 0.42) | 0.87 | 9% |
| Deferred lights | 0.70 | 7% |
| Vegetation (opaque 0.47) | 0.60 | 6% |
| Shadow cascades, dynamic, static, and reflective | 0.55 | 6% |
| Between the seams | 0.50 | 5% |
| Inside the seams but in no pass | 0.49 | 5% |
| Fog volumes (generate, upsample, apply) | 0.44 | 4% |
| Models, static, dynamic, and transparent | 0.40 | 4% |
| Depth prepasses (`Z_AND_VELOCITY_PASS` 0.28, coarse, occluders) and the depth downsample | 0.40 | 4% |
| Reflections (SSR 0.14, scene capture, environment, reflection passes) | 0.38 | 4% |
| Skybox and clouds | 0.33 | 3% |
| Post: blur and composite (the tone map) | 0.28 | 3% |
| Water | 0.27 | 3% |
| Foveation mask-write (0.05) and fill-in (0.21), the mod's | 0.26 | 3% |
| Post: glare | 0.25 | 3% |
| Subsurface skin | 0.18 | 2% |
| Post: the rest (histograms, sun halo, fade) | 0.08 | 1% |
| Post: the engine anti-aliasing's passthrough blit after FSR | 0.06 | 1% |
| Other passes, particles, and transparents | 0.10 | 1% |

Capital (GPU span 12.4 ms):

| Pass | GPU ms | Share |
|---|---|---|
| FSR (the mod's resolve, inside the post block) | 2.07 | 17% |
| Models, static (0.83), dynamic (1.02), and transparent (0.12) | 1.97 | 16% |
| SSAO (1.17) and the AO volumes (0.19) | 1.37 | 11% |
| Deferred lights | 1.05 | 8% |
| Shadow cascades, dynamic (cascade 2 0.38), static, and reflective | 0.79 | 6% |
| Vegetation (opaque 0.43) | 0.57 | 5% |
| Between the seams | 0.50 | 4% |
| Terrain patches, the forest, and the roads | 0.49 | 4% |
| Inside the seams but in no pass | 0.48 | 4% |
| Fog volumes (generate, upsample, apply) | 0.46 | 4% |
| Reflections (SSR 0.20, scene capture, environment, reflection passes) | 0.46 | 4% |
| Depth prepasses and the depth downsample | 0.42 | 3% |
| Skybox and clouds | 0.35 | 3% |
| Post: blur and composite (the tone map) | 0.31 | 2% |
| Foveation mask-write (0.07) and fill-in (0.20), the mod's | 0.26 | 2% |
| Post: glare | 0.26 | 2% |
| Water | 0.20 | 2% |
| Subsurface skin | 0.17 | 1% |
| Post: the rest (histograms, sun halo, fade) | 0.10 | 1% |
| Post: the engine anti-aliasing's passthrough blit after FSR | 0.07 | 1% |
| Other passes, particles, and transparents | 0.08 | 1% |

"Between the seams" is GPU time inside the dispatch but outside `PreDraw`, the G-buffer, the scene, and the post effects; "inside the seams but in no pass" is time within a seam between its passes (the engine's clears and resolves, the mod's per-dispatch work there, and idle between passes). The mod's work after `PostDraw` (the HUD, the eye copies, and the swapchain blits) is outside the dispatch and not in either table; it falls in the idle between frames. The world post block as a whole took 2.57 ms at the town and 2.80 ms at the capital, FSR three quarters of it.

### What the unnamed time is

Run 333 timed it again at the town with the holes between the timed intervals named by where they lie and the mod's post-draw work bracketed as a seam of its own (`target/harness-probe/benchmark/333-gaps-town` and `333-copies-town`, three repeats each, the same settings as above). Since then the dispatch's span includes the mod's post-draw work, so its spans read about 0.15 ms longer than the tables above.

- **Between the seams, 0.50 ms**, is one engine copy: the whole-surface copy of the reflection-proxy depth into the VFX depth history slot, which the engine's draw task issues between `DrawGBuffer` and `RenderEngine::Draw` (the EffectInfo slot machine in [rendering.md](../../engine/rendering/rendering.md) §5.1). Bracketing [`CopySurfaceToTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture) with its own interval accounts for all of it (0.496 ms against a 0.496 ms hole), once per frame. The source is sized like the scene targets, so under single-pass it is double-wide. Nothing reads the destination, and single-pass now skips the copy ([below](#the-vfx-depth-copy)).
- **Inside the seams but in no pass, about 0.6 ms**, is spread thin: the foveation's two passes (0.23, which the tables above already list separately), the hole before `SCREEN_SPACE_SUBSURFACE_SKIN` (0.10), before `SHADOW_0` and the other cascades (0.10 and about 0.2 together, the atlas clears), before `COPY_FRAMEBUFFER` (0.06 to 0.09, the scene-colour copy; with the foveation on this hole also holds the fill-in, see [below](#the-framebuffer-copy)) and before `DOWNSAMPLE_DEPTH` (0.06), and a tail of holes under 0.04 each. None is above 0.1 ms on its own.
- **The mod's post-draw work, 0.15 ms**, inside the dispatch now: the two half-target eye copies into the eye captures under single-pass (0.085) and the HUD and world-locked overlays (0.046). The single-pass path copies each eye out of the double-wide back buffer and the frame tail then blits the captures into the swapchain, where two-pass rendering blits each eye straight from the back buffer; single-pass now blits each eye straight from its half ([below](#the-eye-blits)), and the frame tail's swapchain blit is timed on its own.

### The foveation's net value

The same run measured the foveation on against off (the `foveation-off` variant) at the town: the GPU span was 10.40 ms with it on and 10.22 ms with it off, so as it then stood it cost about 0.18 ms net. Two things kept it from saving: the forced stencil test covered only the models, the vegetation, and the creatures, and the drop was decided per pixel, so nearly every 2 by 2 quad kept a live pixel and ran its pixel shader for all four (15% of an eye's quads lost all four pixels against 54% of its pixels dropped).

Later in run 333 the foveation drops whole quads, skips the dropped pixels in the terrain's apply passes, the road junctions, the decals, the deferred lights, the sky passes, and the fog gradient, and fills them in right after the fog gradient under a stencil test (see "Which passes skip the dropped pixels" in [foveation.md](../stereo/foveation.md)). Measured at the town with `--gpu-passes`, three interleaved repeats of each, the release payload, FSR on (`target/harness-probe/benchmark/333-fov-final-town`), the GPU span's repeat medians were:

| Variant | GPU span (ms), mean of the repeats (range) |
|---|---|
| As shipped now | 10.12 (10.06 to 10.17) |
| `foveation-before` (the three switches off: the foveation as it was) | 10.45 (10.41 to 10.48) |
| `foveation-passes-off` (quads only) | 10.49 (10.48 to 10.50) |
| `foveation-lighting-off` | 10.52 (10.45 to 10.61) |
| `foveation-quads-off` | 10.44 (10.37 to 10.50) |
| `foveation-off` | 10.22 (10.13 to 10.27) |

So the foveation now saves about 0.1 ms net against none at all, and 0.33 ms against what it was. By pass, against `foveation-before`: the deferred lights 0.65 to 0.36 ms, the skybox 0.27 to 0.21, and the fill-in 0.17 to 0.22 (it reconstructs from a wider neighbourhood, but draws only the dropped pixels); the mask-write stays at 0.05. Each switch alone saves nothing: the quads without the extra passes widen the fill-in for no gain, and the extra passes without the quads skip only the 15% of quads that were already empty (the lights 0.58 ms). The vegetation, the models, and the terrain's detail pass take the same time in every variant: the vegetation and the models are geometry-bound here, and the detail pass is left out because it writes depth the prepasses do not. The capital was not measured (it needs a freshly launched session).

The SSAO, at 0.85 ms the largest screen-space pass, still shades every pixel: it reads the depth of its neighbours, which is complete at the dropped pixels, but its blur reads the occlusion of its neighbours, so it cannot drop pixels before the fill-in without changing kept ones.

Where savings look plausible, by rough size, none of them tried:

1. **FSR**, 1.9 to 2.1 ms: still the largest single cost. Rendering a little under the headset's size and letting FSR upscale buys back pixel-proportional work across the whole frame (about 0.5 ms per megapixel per eye of scene and post, [the knee](#the-resolution-sweep)) at a cost in sharpness; at 0.85 per side, roughly 1.5 to 2 ms over the frame.
2. **SSAO**, 0.9 to 1.4 ms: screen-space and scaled with the double-wide target. It cannot skip the foveation's dropped pixels, since its blur reads its neighbours' occlusion (see [above](#the-foveations-net-value)); SSAO at a lower resolution than it runs (which was not checked) would save more. Skipping its taps for far quads and dropping its temporal copy were tried and save nothing ([below](#the-ssao)). The deferred lights now skip the dropped pixels (0.65 to 0.36 ms at the town).
3. **The models at the capital**, 2.0 ms: geometry-bound passes whose cost follows the draw count more than the pixels; the per-eye re-issues still drawn twice ([above](#the-per-eye-submissions-at-the-capital)) are part of it, and converting them is worth tenths of a millisecond on the GPU.
4. **The VFX depth copy**, 0.5 ms: the whole of the time between the seams. Done: nothing reads it, and single-pass skips it ([below](#the-vfx-depth-copy)).
5. **Fog volumes**, 0.45 ms: the volume is generated from the camera, and under single-pass from the shared centre; if its grid is sized from the double-wide target's width, it is twice as wide as it needs to be. Unverified; up to 0.2 ms.
6. **Skybox and clouds**, 0.35 ms: drawn over the whole double-wide target; the skybox now skips the foveation's dropped pixels (0.27 to 0.21 ms at the town), and drawing it only where depth leaves sky could save a little more.
7. **The foveation**, now a 0.1 ms net saving at the town ([above](#the-foveations-net-value)); the terrain's detail pass (0.42 ms) would skip the dropped pixels if its depth came from a prepass. The capital is unmeasured.
8. **Shadows**, 0.55 to 0.8 ms: rendered once per frame in single-pass already; the dynamic cascade 2 at the capital (0.38 ms) is the one candidate for a slower update cadence, which the mod forces off against flicker (`stereo.shadow_update_every_frame`). Up to 0.3 ms at the cost of the flicker that setting fixes.
9. **The engine's anti-aliasing passthrough** after FSR, 0.06 to 0.07 ms: a full-target blit of FSR's result onward. Writing FSR's result where the chain reads it next would remove it.

## Toward 120 Hz (runs 338 and 339)

The target is 8.33 ms a frame at 3584 by 3561 per eye. Run 337 (the baseline, `target/harness-probe/benchmark/337-120hz`) had every scene GPU-bound and 0.2 to 2.3 ms over that at the median with FSR and the foveation as shipped. Runs 338 and 339 measured four changes against it, each as a variant without it in the same session: single-pass, unpaced, three seven-second repeats, `--gpu-passes`, the release payload. The image checks used the fast comparison with each variant also compared directly with the single-pass snapshot (`tools/rendering/fast/plan.py` now does that), on the town, the tunnel, the meadow, the capital at night, the town in rain, and the shore.

### The VFX depth copy

The copy of the reflection-proxy depth into the effect depth-history slots, the whole of the time between the seams ([above](#what-the-unnamed-time-is)), has no reader: [`EffectInfo::m_DepthTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::EffectInfo##field-m_DepthTexture) is copied into and never sampled. The single-pass collapse returns from the [`CopySurfaceToTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture) detour when the destination is one of the five slots' textures, and leaves the slots' lifecycle as the engine drives it. With the copy on as a variant, the final image differed from the skip at the repeat floor in every scene; a no-op control variant in the same schedule differed as much (the clouds, the water, and the wind move between configurations). Run 338 (`338-item1b`), skip on against off:

| Scene | GPU busy (ms) | Frame median (ms) |
|---|---|---|
| Capital | 9.25 against 9.74 | 9.71 against 9.85 |
| Town | 7.89 against 8.36 | 8.17 against 8.63 |
| Tunnel | 9.31 against 9.84 | 9.45 against 10.16 |
| Shore | 8.93 against 9.45 | 9.14 against 9.62 |
| Meadow | 8.97 against 9.39 | 9.23 against 9.58 |

So 0.42 to 0.53 ms of GPU time in every scene. Only single-pass skips it; multipass makes the copy once per eye and still does.

### The SSAO

In stereo the collapse clears the SSAO pass's temporal filter while stereo forces its first-pass state, so the block skips the resolve that only copied the blurred occlusion into the history and the composite reads the blur instead (the composite is outside the temporal branch; see the pass's [pyxis doc](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::ssao::SSAOPass)). The SSAO planes and the final image match at the repeat floor, but the saving is nil: the SSAO pass took 1.30, 0.87, 1.35, 1.29, and 0.97 ms at the capital, the town, the tunnel, the shore, and the meadow with it and the same to 0.003 ms without. The resolve is a half-resolution copy and costs next to nothing.

A rewrite of the generation shader that skipped the 25 depth taps for a 2 by 2 quad wholly beyond the occlusion range was measured and removed: the range test the shader already makes near its top is reduced over the quad through fine derivatives, and the shader's own far branch is copied ahead of the taps behind it. The test is quad-uniform, so the derivatives the shader takes at its end read the same neighbours as before, and the generation output (`ssao_step1`) and the blur after it were bit-identical with it on and off in all five scenes. It is slower: the SSAO pass took 1.30 against 1.16 ms at the capital, 0.87 against 0.83 at the town, 1.35 against 1.23 in the tunnel, 1.29 against 1.17 at the shore, and 0.97 against 0.89 in the meadow, with it on and off. The added branch costs more than the far quads save even at the shore, where sky and lake fill much of the view, so it was removed. The game's NVIDIA variant of the shader is chosen by the adapter's vendor ID ([`SSAOPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::ssao::SSAOPass) at its initialisation); which variant ran here was not observed (DXVK reports an NVIDIA GPU as another vendor unless NVAPI is enabled, which the harness does not), and the rewrite applies to both.

### The framebuffer copy

The 0.33 ms the baseline listed as the hole before `COPY_FRAMEBUFFER` is mostly the foveation's fill-in, which the pass timings count as a detail and the hole subdivision does not: with the foveation off the hole is 0.087 ms at the capital, and with it on it is 0.33 ms with the fill-in's 0.24 ms inside. The scene-colour copy itself is about 0.09 ms, so the two copies the baseline put at 0.85 ms together are 0.59 ms, of which the VFX depth copy above was 0.50. What the framebuffer copy feeds and whether any frame can skip it was not traced; at 0.09 ms it is below the other candidates.

### The eye blits

The single-pass collapse takes the direct route two-pass rendering already had: each eye is blitted into its swapchain slice from its half of the double-wide back buffer in the post-draw, the blit's pixel shader mapping the slice's UV onto the half and clamping it to the half's outermost texel centres, and the half is copied into the eye's capture only for a reader of it (the mirror, a screenshot, the recorder, the previews, the capture harness). The frame tail's swapchain blit, now timed on the GPU while a run times passes (`frame tail: swapchain blit`), took 0.12 to 0.14 ms with the copies; with the direct blit it has nothing to draw, the GPU's idle between dispatches fell from 0.17 to 0.04 ms, and the frame median from 9.86 to 9.57 ms at the capital, 8.16 to 8.10 at the town, 9.53 to 9.42 in the tunnel, 9.43 to 9.30 at the shore, and 9.05 to 8.83 in the meadow (runs 338 and 339, `338-items24`, `339-items24`). The direct blit is checked by the region arithmetic's unit test, not by a capture of the swapchain, which the harness does not read back.

The 0.7 to 1.2 ms by which the light scenes' frame exceeded their GPU spans with FSR off in the baseline is not the tail blit: at 0.13 ms the blit is a small part of it, and with FSR on the frame now sits within 0.15 ms of the GPU busy time in those scenes. The remainder appears only where the GPU finishes well inside the frame, and lies in the runtime's wait.

### Where the shipped settings land

With all of the above as shipped (the far early-out off), the single-pass frame median is 9.37 ms at the capital, 8.03 at the town, 9.38 in the tunnel, 9.11 at the shore, and 8.74 in the meadow, against 9.65, 8.56, 10.66, 9.94, and 9.46 in the baseline (different sessions, so read the differences to a tenth or two). The town is now under 8.33 ms; the other scenes are 0.4 to 1.1 ms over, still GPU-bound, with FSR the largest single item.

### The frame-time tail

The 99th percentile sits 4 to 9 ms above the median in every scene. In the baseline's per-frame records (`337-120hz`, every single-pass repeat), the GPU's dispatch span has a 99th percentile only 1.1 to 3 ms above its median, while the slow frames' extra time is in `xrWaitFrame`: frames over the median by more than 3 ms spend about 3.1 ms more in it than the rest, and `xrEndFrame` and the simulation grow by about 0.5 ms each. The slow frames come in clusters a frame or two apart, not on a fixed period, and are different frames from repeat to repeat. They lean on the simulation tick: a frame that runs a tick (about a quarter of them) is slow 2 to 5 times as often as one that does not (34 against 7% at the capital, 21 against 6% at the town; the shore is the exception). The rare slow GPU dispatches (one to four per run) grow in `PreDraw` and the G-buffer, about 1 and 1.7 ms each, which fits the static shadow cascades' refreshes or streaming. So most of the tail is the runtime holding the next frame back after a frame the simulation tick lengthened, under Monado's null compositor, rather than GPU work; a headset's compositor paces differently, and the tail should be measured there before it is worked on.

## What is left

The capital in single-pass at the headset's size now runs at 10.6 ms a frame unpaced (9.1 ms with FSR off) and holds 72 Hz with a 99th percentile of 15 ms; the GPU limits it. By expected saving, largest first:

1. **FSR's GPU time**, 2.1 ms a frame, now on the critical path at the capital (see item 3 of the ranked opportunities).
2. **The engine's context lock**, 1.3 to 1.4 ms of the draw task in Wine's critical-section fast paths, and the locked instructions generally: each waits for the runtime's recorded stores to drain. The engine takes the lock around nearly every call into Direct3D; it is the engine's, not the mod's, to remove.
3. **The payload's per-draw work**, about 1.2 ms of its own instructions and 0.5 ms of its own calls into DXVK on the placed draw task, spread over many small sites (the `cb13` pins of the instanced re-issue, the viewport record's reads, the water and velocity probes, the census); no one site is above 0.15 ms.
4. **The per-eye submissions**, 664 at the capital, about 0.3 ms on the placed draw task (the table above); converting the decals, the terrain patches, the water, and the vegetation's hardware-instanced draws needs the shader and layout work "What blocks the largest families" in [single-pass-stereo.md](../stereo/single-pass-stereo.md) describes.
5. **The multipass detour tax**, 0.35 to 0.74 ms on the placed draw task, gone whenever single-pass is on.

On a processor with one size of L3 the placement does nothing, and how long the draw task takes there depends on how much of its working set that cache holds; where it limits the frame, items 2 to 4 are the levers.

## The mod's own cost per draw

With `--detour-timing` (run 301), the five draw entry detours took 4.3 ms per frame at the capital in single-pass over 12,960 calls, of which 1.0 ms was the forwarding calls into DXVK (13,311 calls at 75 ns each). A clock-read pair costs 120 ns under Wine, and the timing makes one pair per detour and per forward, 3.2 ms of the remaining 3.3 ms; what is left, the detours' own work around the draws, is under 0.1 ms per frame at the capital (5 ns per draw) and 0.35 ms at the town (59 ns per draw), at the resolution this method allows. In multipass the remainder was within the clock reads' own cost.

The whole set of stereo draw-path detours (the draw entries, the shader binds, the viewports, and the constant stager) costs more than the draw entries' own work: in run 300, multipass in a fresh session with no detours installed took a draw task of 15.3 ms at the capital and 7.3 ms at the town, and the same scenes with the detours installed (as every session that has run single-pass has them) 17.1 and 7.9 ms, so 60 to 90 ns per draw and 0.5 to 1.9 ms per frame in multipass. Single-pass needs them.

The forwarding call itself is cheap: DXVK records a draw in 75 ns on the draw task and replays it on its command-stream thread. The draw task's 0.9 µs per submission was read here as the engine's walk of its render blocks; sampling it ([above](#where-the-draw-tasks-time-goes)) shows most of it is DXVK recording the engine's state changes, and that the mod's own work is larger than these timers suggest: 1.2 to 1.9 ms of its own instructions and 0.7 ms of its own calls into DXVK at the capital in single-pass, the clock-read pairs of `--detour-timing` having hidden most of it in their own cost.

## The ranked opportunities

By expected saving at the headset's size on this machine, largest first:

1. **Overlap the next frame's render update with the draw task.** Done (`vr.overlap_render_update`, [below](#overlapping-the-render-update)): at the capital single-pass went from 19.1 to 14.2 ms at the 1000 fps pace, and under 72 Hz pacing from every third display period to every one.
2. **The draw task's CPU time**, which the overlap above leaves as the floor: 12.0 ms for 12,750 submissions at the capital. Done for this machine's kind of processor: on the cores of the larger L3 the same draw task takes 6.6 ms (`vr.large_cache_cores`, [above](#where-the-draw-tasks-threads-run)). What remains of it is ranked in [What is left](#what-is-left).
3. **FSR's GPU time**, 2.2 to 2.4 ms per frame at the headset's size in both modes, 22 to 25% of the GPU's frame in the scenes the GPU limits, and growing with the pixels (4.7 ms at twice them). It runs at the full render size as an anti-aliasing and sharpening pass; rendering below the headset's size and letting FSR upscale, or a cheaper resolve, would buy back most of it on a GPU-bound headset, at a cost in image quality. On this GPU every scene the GPU limits already fits 13.9 ms. Run 321 measured FSR off against on in single-pass under 72 Hz pacing: the seam is 1.9 to 2.1 ms, and at the capital FSR turns the 1% low from 17.8 into 27 ms, while unjittered at 1:1 it renders further from a supersampled reference than the game's SMAA (see "Verdict" in [fsr.md](../rendering/fsr.md)).
4. **The multipass detour tax**, 0.5 to 1.9 ms per frame, paid by multipass whenever single-pass has run in the session; 0.35 to 0.74 ms on the large cache's cores ([above](#the-multipass-detour-tax)). It goes away when single-pass becomes the default.
5. **The mod's own CPU work**: its scoped work is 0.4 to 1.0 ms per frame across all threads (`mod_ms`). Its per-draw work in the draw path is larger than the detour timers said, about 1.7 ms of the capital's draw task by sampling ([above](#where-the-draw-tasks-time-goes)).

With the overlap and the placement in, the capital in single-pass is limited by the GPU at 10.6 ms a frame unpaced, so the next gains there are the GPU's (FSR first); see [What is left](#what-is-left).

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
