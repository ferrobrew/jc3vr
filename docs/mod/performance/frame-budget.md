# Where the frame goes at the headset's size

The frame-time benchmark ([benchmark.md](../testing/benchmark.md)) answers whether multipass and single-pass stereo fit a headset's period at the Bigscreen Beyond 2e's recommended 3584x3561 per eye, what limits each, and what is left to gain. The targets are 13.9 ms (72 Hz, the headset's slowest refresh rate) and 8.33 ms (120 Hz).

The machine is an AMD Ryzen 9 9950X3D (32 threads, two dies with different L3 sizes) with an NVIDIA GeForce RTX 5090, running the game under Proton Experimental and DXVK in gamescope's headless backend with Monado's simulated headset and null compositor. Every run uses the release payload, FSR and the foveation as shipped, and the motion blur and depth of field off. The clock and the world run, and only the camera is held. The scenes are the benchmark suite's (`tools/rendering/suites/benchmark.toml`): the capital (the heaviest), the town, the tunnel, the shore lake, and the meadow. Results are under `target/harness-probe/benchmark/`, by run number.

The comparison between modes on this machine is the point of these numbers. A real headset adds its compositor's GPU work, lens correction, and reprojection, and on a weaker GPU the render size at which the GPU starts to limit the frame moves down.

## Reading the frame interval

The null compositor is set either to pace at 72 fps or to 1000 fps ("unpaced"), where the frame interval should show the cost of a frame rather than a display period. It does not always: in some unpaced runs `xrWaitFrame` blocks for several milliseconds while the GPU sits idle, and the same scene and mode can land in either state from one session to the next. The work inside the frame (the game thread's busy time, the draw task, the GPU spans) does not depend on pacing, and where the frame interval and the work disagree the work is the number to read.

The wait comes from Monado's application pacing (`u_pacing_app`). It predicts each frame from three filtered times: from the wake-up out of `xrWaitFrame` to `xrBeginFrame` (the CPU time), from `xrBeginFrame` to `xrEndFrame` (the draw time), and from `xrEndFrame` to the GPU's completion. It rounds the frame period up to a multiple of the display period longer than each of the three, picks the first display time at least half that period after the last one it returned with room for all three, and wakes the application that long before it. Drift in the filtered estimates moves the wake-up early or late between sessions. The frame order matters for the same reason: with the render update overlapped ([below](#overlapping-the-render-update)) it counts as CPU time, and the draw time shrinks to the dispatch and the draw task.

## Where the shipped settings land

Single-pass with the render update overlapped and the process on the large-cache cores (both defaults), unpaced, three repeats each (run 326). Frame is the median, the 99th percentile, and the worst frame in milliseconds; the rest are per-frame medians.

| Scene | Mode | Frame | Game busy | Draw task | GPU | GPU idle |
|---|---|---|---|---|---|---|
| capital | single-pass, FSR off | 9.09 / 13.71 / 17.7 | 3.80 | 6.72 | 8.59 | 0.34 |
| capital | single-pass | 10.61 / 16.34 / 17.9 | 5.33 | 6.69 | 10.20 | 0.36 |
| capital | multipass | 11.61 / 16.29 / 24.8 | 4.29 | 9.35 | 11.23 | 0.28 |
| town | single-pass, FSR off | 7.26 / 11.59 / 13.6 | 3.25 | 4.23 | 6.93 | 0.33 |
| town | single-pass | 9.66 / 14.36 / 17.1 | 1.73 | 4.24 | 8.57 | 0.34 |
| town | multipass | 8.94 / 13.64 / 19.3 | 3.76 | 5.11 | 8.63 | 0.26 |
| tunnel | single-pass, FSR off | 9.42 / 14.85 / 18.4 | 2.34 | 3.11 | 8.42 | 0.34 |
| tunnel | single-pass | 10.64 / 20.62 / 22.0 | 4.20 | 3.17 | 10.25 | 0.35 |
| tunnel | multipass | 10.61 / 15.59 / 17.7 | 4.26 | 4.20 | 10.22 | 0.28 |

Under 72 Hz pacing every scene holds 13.9 ms in both modes, single-pass at the capital with a 99th percentile of 14.9 to 15.0 ms. With the later GPU savings ([Changes measured toward 120 Hz](#changes-measured-toward-120-hz)) the single-pass median is 9.37 ms at the capital, 8.03 ms at the town, 9.38 ms in the tunnel, 9.11 ms at the shore, and 8.74 ms in the meadow (runs 338 and 339). The town fits 120 Hz. The other scenes are 0.4 to 1.1 ms over, all GPU-bound, with FSR the largest single item.

## What limits the frame

No thread saturates. The busiest single thread, a fragment worker running the draw task, is busy about half the time. Two limits account for every measurement:

- The GPU. The light scenes are GPU-bound in both modes, with under 0.4 ms of GPU idle a frame. With the overlap and the placement in, the capital in single-pass is GPU-bound too.
- The serial CPU chain. The deferred frame tail cannot submit a frame until the draw task has recorded all of it, and in the serial ordering the next frame's render update waits for the runtime lock the tail holds until it submits. The frame is then roughly the draw task, plus the game thread's work between taking the lock and dispatching (the engine's render update, 3.5 ms at the capital), plus `xrWaitFrame`. Multipass adds the mandatory drain between its two dispatches. The overlap removes the render update from the chain, which leaves the draw task as its floor, and the placement halves that.

A GPU span does not by itself say the GPU limits a frame. A span is the GPU's execution window with idle inside it included, so a GPU waiting for commands reads as a long span (see "Reading the GPU lane" in [profiler.md](profiler.md)). The resolution sweep shows it: at the capital the G-buffer's GPU span shrinks as the render size grows, which only a GPU waiting on submission does.

## The resolution sweep

Sweeping `vr.resolution_scale` live at the capital and the town (runs 300 and 301, serial ordering, swapchain fixed at 3584x3561) separates CPU from pixel cost. The draw task is flat across the sweep: it is the CPU cost of the draws and independent of the pixels. The scene passes grow by about 0.25 ms per megapixel per eye and the post chain by about 0.23 ms, of which FSR is 0.17 ms. The GPU therefore takes over where its pixel-proportional work passes the draw task: the town is GPU-bound from about 6 MP per eye in both modes, and the capital was CPU-bound up to the headset's size before the overlap and the placement.

At three times the headset's pixels (6208x6168) frames grow to tens of milliseconds or more, and at four times to seconds: the GPU's memory runs out, and repeated resizes at that size crash in `d3d11.dll`.

The game thread's median busy time is the median of two populations. A frame that runs the 30 Hz simulation tick is busy about twice as long as one that does not, and the tick lands in more than half the frames once the frame takes longer than 16.7 ms. A busy-time median that rises with the render size can be that shift in the mix rather than more work.

## Where the draw task's time goes

The draw task is not mostly the engine's walk of its render blocks. Sampling its thread about once a millisecond with `--sample-draw-task` ([benchmark.md](../testing/benchmark.md#where-the-draw-tasks-time-goes)) at the capital in single-pass, FSR off, 72 Hz pacing, before the placement (runs 321 and 323, 13,450 samples, a median draw task of 10.1 ms), charges each sample to the module its instruction pointer was in, and time in DXVK or Wine to the innermost game or payload function that called it:

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

By engine seam the G-buffer range took 5.6 ms, the prepasses 2.1 ms, the scene range 1.8 ms, and the post effects under 0.1 ms. By render pass the models dominate ([`RP_MODELS_DYNAMIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_DYNAMIC), [`RP_MODELS_STATIC`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_STATIC), and [`RP_MODELS_TRANSPARENT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_MODELS_TRANSPARENT) together 4.7 ms), and by render-block type [`GeneralMkIII`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTypeGeneralMkIII) took 3.8 ms.

Nothing in the draw task waits. Wine's share is the uncontended fast path of the engine's context lock ([`Graphics::AcquireThreadOwnership`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw##func-AcquireThreadOwnership) and its siblings take a critical section around each call into Direct3D). The task is bound by one thread's CPU time, and most of that is DXVK recording the engine's state changes: the vertex constant-buffer binds ([`Graphics::SetVertexProgramConstantBufferNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw##func-SetVertexProgramConstantBufferNoMutex)) alone took 1.1 ms. By the sampled addresses in DXVK's `d3d11.dll`, that time goes to atomic reference-count increments on changed bindings and a pointer chase in a free list. It is memory traffic, not computation.

Removing the payload's locks from the per-draw path (the viewport record is read through a sequence lock and the per-draw counters bumped with a plain load and store; see "The viewport record" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)) cut the payload's own instructions from 1.9 to 1.2 ms but saved only about a quarter of a millisecond of draw task. A locked instruction waits for the stores before it to drain, and after DXVK records a call there are many, so removing one lock moves the wait to the engine's context lock that follows.

## Where the draw task's threads run

A draw task that is memory traffic depends on the cache it runs against. On this processor one die's eight cores share a 96 MB L3 and the other's a 32 MB one, and the scheduler, left to itself, puts the draw task and DXVK's command-stream thread on the smaller-cache die most of the time. Pinning the game process to one die or the other at the capital under 72 Hz pacing (run 325, three cycles with the order rotated):

| Cores | Mode | Draw task | Per 1,000 submissions | GPU | Frame 99th percentile |
|---|---|---|---|---|---|
| free | single-pass, FSR off | 11.45 | 0.86 | 13.44 | 16.43 |
| the 32 MB die | single-pass, FSR off | 11.44 | 0.87 | 13.50 | 16.20 |
| the 96 MB die | single-pass, FSR off | 6.39 | 0.49 | 10.98 | 14.94 |
| free | single-pass | 11.60 | 0.88 | 13.44 | 17.13 |
| the 32 MB die | single-pass | 11.53 | 0.89 | 13.50 | 16.66 |
| the 96 MB die | single-pass | 6.51 | 0.50 | 12.73 | 14.85 |

On the larger cache the draw task takes 56% of the time for the same submissions, and the GPU span falls with it because the span had been the GPU waiting for commands. The game thread's busy time falls by a third too, since the render update and the simulation share the cache. Unpaced, the capital in single-pass costs 10.6 ms a frame on the large-cache cores against 13.8 ms on free ones, and multipass 11.6 ms against 22.9 ms.

`vr.large_cache_cores` (on by default; [`cpu_placement`](../../../crates/jc3vr-core/src/cpu_placement.rs)) narrows the process's affinity to the cores of the largest L3 when the L3 caches differ in size, and leaves a processor with one size of L3 alone. Threads created afterwards inherit the affinity. The threads the graphics driver created natively before it (its shader compilers and its event handler) keep theirs.

With the placement on (run 326, the capital, single-pass, FSR off, unpaced, a median draw task of 6.8 ms), DXVK's share falls from 5.5 to 2.9 ms and the game's own code from 1.5 to 0.9 ms, while the engine's context lock in Wine (1.4 ms) and the payload's own work (1.2 ms, plus 0.5 ms of its calls into DXVK) stay where they were. The locked instructions and the mod's per-draw bookkeeping are the larger shares of the smaller task.

## The per-eye submissions at the capital

The census at the capital ([`coverage_capital_census_headset.toml`](../../../tools/rendering/experiments/coverage_capital_census_headset.toml), run 326, using the accounting in [single-pass-draw-calls.md](../stereo/single-pass-draw-calls.md#accounting-for-the-draw-calls)) counts 14,314 game calls, 14,978 single-pass submissions, and 20,094 multipass submissions. Single-pass submits 664 beyond one per game call:

| Family | How | Extra submissions |
|---|---|---|
| `SSDecal` | block re-issue | 170 |
| `VolumetricTerrainPatch` (tessellated colour, near, and far, the GPU-indirect near basemesh, the detail) | per-eye re-issue | 136 |
| `VegetationFoliage`, `vegetationfoliage*hwinstanced` | hardware-instanced re-issue inside a block drawn once | 124 |
| `NvWaterHighEnd` | block re-issue | 122 |
| `VegetationBark`, `vegetationbark*hwinstanced` | hardware-instanced re-issue inside a block drawn once | 72 |
| `ParticleRB`, tessellated | per-eye re-issue | 16 |
| split fullscreen passes, the occluder, the sun, the moon | second run or re-issue | 24 |

No prepass submits anything per eye: every shadow cascade, the reflection passes, and the vegetation and rain prepasses submit once, as the game calls them (see "Why `PreDraw` is outside the collapse" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)). A submission costs 0.5 µs of draw task on the large-cache cores and about 0.9 µs on free ones, so the 664 cost about 0.3 ms, which the sampled cost of the block re-issues' second eye agrees with. On the GPU they cost tenths of a millisecond. Forcing each family's switch off in turn (the benchmark's `*-reissue-off` and `*-once-off` variants) moves the town within its own drift and the capital's draw task by at most 0.73 ms (`hw_instanced_once` off, 590 extra submissions).

`fsr_per_eye` and `foveation_per_eye` change how FSR and the foveation treat the two halves, not how much they cost.

## The mod's own cost per draw

The stereo draw path's detours (the draw entries, the shader binds, the viewports, and the constant stager) cost multipass a draw task of 0.74 ms at the capital, 0.55 ms at the town, and 0.35 ms at the tunnel on the large-cache cores, measured as multipass before and after the detours are first installed in a session (run 326). Single-pass needs them. Multipass pays them whenever single-pass has run in the session, because skipping them would need the detours to know every probe (the census, the harness's probes, the water and velocity bookkeeping) that wants multipass draws, and each of those costs only a few hundredths of a millisecond.

`--detour-timing` underestimates the detours' own work. A clock-read pair costs 120 ns under Wine and the timing makes one per detour and per forward, so the timers hide most of what they measure. Sampling is the instrument for this question. The forwarding call into DXVK itself is cheap: DXVK records a draw in 75 ns on the draw task and replays it on its command-stream thread.

## Where the GPU's time goes

Run 333 timed every render pass on the GPU, and each post effect and the foveation's two passes, with `--gpu-passes` ([benchmark.md](../testing/benchmark.md)): single-pass at the capital at midday, 72 Hz pacing, three repeats. Each figure is the pass's GPU time per frame. A pass's time is its execution window, idle included, but with the draw task well under the span the GPU leads, so the windows are mostly work. The pass timestamps cost nothing measurable. Engine pass names drop the `RP_` prefix.

| Pass | GPU ms | Share of the 12.4 ms span |
|---|---|---|
| FSR (the mod's resolve, inside the post block) | 2.07 | 17% |
| Models, static (0.83), dynamic (1.02), and transparent (0.12) | 1.97 | 16% |
| SSAO (1.17) and the AO volumes (0.19) | 1.37 | 11% |
| Deferred lights | 1.05 | 8% |
| Shadow cascades, dynamic (cascade 2 0.38), static, and reflective | 0.79 | 6% |
| Vegetation (opaque 0.43) | 0.57 | 5% |
| Between the seams (the VFX depth copy, now skipped) | 0.50 | 4% |
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

The town has the same order with less of the models (0.40 ms) and more of the terrain (0.87 ms); FSR is 1.89 ms of its 9.9 ms span.

"Between the seams" is GPU time inside the dispatch but outside `PreDraw`, the G-buffer, the scene, and the post effects. All of it is one engine copy: the whole-surface copy of the reflection-proxy depth into the VFX depth history slot, which the draw task issues between `DrawGBuffer` and `RenderEngine::Draw` (the EffectInfo slot machine in [rendering.md](../../engine/rendering/rendering.md) §5.1). "Inside the seams but in no pass" is spread thin over the engine's clears and resolves: the holes before `SCREEN_SPACE_SUBSURFACE_SKIN`, before the shadow cascades (the atlas clears), before `COPY_FRAMEBUFFER` (the scene-colour copy, about 0.09 ms, plus the foveation's fill-in when it is on), and before `DOWNSAMPLE_DEPTH`. None is above 0.1 ms on its own.

The mod's post-draw work (the HUD and world-locked overlays, and the eye blits) runs inside the dispatch and takes about 0.15 ms.

### The foveation's net value

At the town the foveation saves about 0.1 ms of GPU span net against no foveation, and 0.33 ms against its earlier form, which forced the stencil test only on the models, the vegetation, and the creatures and dropped single pixels rather than whole quads (run 333, three interleaved repeats each, the benchmark's `foveation-*` variants). The savings, against that earlier form, are in the deferred lights (0.65 to 0.36 ms) and the skybox (0.27 to 0.21 ms), and the fill-in grows from 0.17 to 0.22 ms because it reconstructs from a wider neighbourhood. Each of the three switches alone saves nothing: whole-quad dropping without the extra passes widens the fill-in for no gain, and the extra passes without whole quads skip only the 15% of quads that were already empty. The vegetation, the models, and the terrain's detail pass take the same time in every variant: the first two are geometry-bound, and the detail pass is left out because it writes depth the prepasses do not. Which passes skip the dropped pixels is in [foveation.md](../stereo/foveation.md). The capital is unmeasured.

The SSAO still shades every pixel. Its depth taps would be complete at dropped pixels, but its blur reads its neighbours' occlusion, so it cannot drop pixels before the fill-in without changing kept ones.

### Changes measured toward 120 Hz

Runs 338 and 339 measured each change as a variant without it in the same session: single-pass, unpaced, three seven-second repeats, `--gpu-passes`. The image checks used the fast comparison with each variant also compared directly with the single-pass snapshot, on the town, the tunnel, the meadow, the capital at night, the town in rain, and the shore.

The VFX depth copy has no reader: [`EffectInfo::m_DepthTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::EffectInfo##field-m_DepthTexture) is copied into and never sampled. Under single-pass the [`CopySurfaceToTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture) detour returns early when the destination is one of the slots' textures, and leaves the slots' lifecycle as the engine drives it. That saves 0.42 to 0.53 ms of GPU time in every scene, with the final image at the repeat floor. Multipass makes the copy once per eye and still does.

Under stereo the SSAO pass's temporal filter is cleared while stereo forces its first-pass state, so the block skips the resolve that only copied the blurred occlusion into the history, and the composite reads the blur instead (see the pass's [pyxis doc](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::ssao::SSAOPass)). It saves nothing measurable: the resolve is a half-resolution copy. A generation shader that skipped the 25 depth taps for a 2x2 quad wholly beyond the occlusion range was bit-identical and 0.04 to 0.14 ms slower in all five scenes, so it does not ship.

Each eye is blitted into its swapchain slice straight from its half of the double-wide back buffer in the post-draw, the blit's pixel shader mapping the slice's UV onto the half and clamping it to the half's outermost texel centres. The half is copied into the eye's capture only when something reads it (the mirror, a screenshot, the recorder, the previews, or the capture harness). Against copying both halves out and blitting the captures on the frame tail, that takes 0.1 to 0.3 ms off the frame median. The region arithmetic's unit test checks the direct blit; the harness does not read the swapchain back.

## Overlapping the render update

With `vr.overlap_render_update` on (the default), the next frame's `xrWaitFrame`, view location, and render update run while the previous frame's draw task and deferred tail are still running, and the frame waits for the tail only at its `xrBeginFrame`, just before it dispatches. The per-frame values the draw task reads are kept per frame, so it never sees the next frame's. The frame order, why the engine allows it, and what keeps it safe (including the runtime's swapchain calls under the engine's context mutex) are in [threading.md](../threading.md#who-holds-what-across-a-frame). The benchmark's `multipass-serial` and `single-pass-serial` variants turn it off.

Measured both ways in one session per pacing, before the placement (run 306 at 72 Hz, run 304 unpaced):

| Scene | Mode | Pacing | Frame, serial | Frame, overlapped | Pose age, serial | Pose age, overlapped |
|---|---|---|---|---|---|---|
| capital | single-pass | 72 Hz | 41.6 / 43.0 / 43.4 | 13.9 / 16.1 / 34.2 | 16.0 | 14.6 |
| capital | multipass | 72 Hz | 41.6 / 45.7 / 48.3 | 41.7 / 45.3 / 49.6 | 21.0 | 21.3 |
| capital | single-pass | unpaced | 19.1 / 28.3 / 30.4 | 14.2 / 21.9 / 23.9 | 16.6 | 20.2 |
| town | multipass | unpaced | 14.9 / 19.8 / 24.0 | 9.4 / 15.4 / 21.0 | 10.3 | 11.8 |

Pose age is how old the head pose a submitted frame was rendered from is at its `xrEndFrame`.

Under 72 Hz pacing, missing 13.9 ms costs whole periods, not the overrun: serially the capital's work of 19 to 20 ms dropped it to every third period (24 fps), with the game thread waiting in `xrWaitFrame`. Overlapped, single-pass at the capital holds the period, with the tail lock at zero in every scene: the render update and the compositor's wait cover the whole of the previous frame's tail. Multipass at the capital does not move, because its two dispatches are serial by construction and its draw task alone took 17 ms there (the placement later brings multipass under the period). The other scenes held the period in both orderings.

Under pacing the pose's age is unchanged within 0.3 ms where both orderings held the period, and falls at the capital because the frame no longer waits periods for its slot. Unpaced, three things get worse, all from that pacing: the pose ages by 1 to 3.6 ms because the views are located before the render update and nothing else holds the frame; the light scenes' 99th percentile rises by 3 to 5 ms, spent in `xrWaitFrame`; and multipass at the capital waits 9.4 ms in `xrWaitFrame` for a reason not traced through Monado's estimates. The game thread's busy time also grows unpaced (7.3 to 11.5 ms at the capital in single-pass), because the render update shares the CPU and the engine's locks with the draw task. Under 72 Hz pacing it does not.

## The frame-time tail

The 99th percentile sits 4 to 9 ms above the median in every scene under the null compositor (run 337, single-pass). The GPU's dispatch span has a 99th percentile only 1.1 to 3 ms above its median. The slow frames' extra time is in `xrWaitFrame`: frames more than 3 ms over the median spend about 3.1 ms more in it than the rest, and `xrEndFrame` and the simulation grow by about 0.5 ms each. The slow frames come in clusters a frame or two apart, not on a fixed period, and differ between repeats. They lean on the simulation tick: a frame that runs a tick is slow 2 to 5 times as often as one that does not. The rare slow GPU dispatches grow in `PreDraw` and the G-buffer, which fits the static shadow cascades' refreshes or streaming. Most of the tail is therefore the runtime holding the next frame back after a frame the tick lengthened. A headset's compositor paces differently, so the tail needs measuring there before work on it.

## The motion blur and the depth of field

Both are forced off in VR (see "The motion blur and the depth of field" in [single-pass-collapse-defects.md](../stereo/single-pass-collapse-defects.md)). Forcing them off takes 0.10 to 0.17 ms of post-effect GPU time per frame at the capital and the town in both modes, all of it the motion blur's pass, with no frame-time change outside the repeats' spread (run 302). The depth of field's own passes were already inactive in those views: the town's draw census shows the plain composite (`comp_pg_cc_ef`) and no motion blur, bokeh, or depth-of-field program in either mode.

## Remaining costs

On this machine, at the headset's size, in single-pass with the shipped settings, largest first:

1. FSR's GPU time, 1.9 to 2.1 ms a frame, on the critical path in every GPU-bound scene and growing with the pixels. It runs at the full render size as an anti-aliasing and sharpening pass. At the capital it turns the 1% low from 17.8 into 27 ms under 72 Hz pacing (run 321), and unjittered at 1:1 it renders further from a supersampled reference than the game's SMAA (see "Verdict" in [fsr.md](../rendering/fsr.md)).
2. The SSAO, 0.9 to 1.4 ms, screen-space over the double-wide target, which cannot skip the foveation's dropped pixels.
3. The engine's context lock, 1.3 to 1.4 ms of the draw task in Wine's critical-section fast paths, and the locked instructions generally. The engine takes the lock around nearly every call into Direct3D.
4. The payload's per-draw work, about 1.2 ms of its own instructions and 0.5 ms of its calls into DXVK on the placed draw task, spread over many small sites (the `cb13` pins of the instanced re-issue, the viewport record's reads, the water and velocity probes, and the census). No one site is above 0.15 ms.
5. The per-eye submissions, about 0.3 ms on the placed draw task ([above](#the-per-eye-submissions-at-the-capital)). Converting the decals, the terrain patches, the water, and the vegetation's hardware-instanced draws needs the work "What is still drawn twice" in [single-pass-draw-calls.md](../stereo/single-pass-draw-calls.md) describes.

On a processor with one size of L3 the placement does nothing, and the draw task's length depends on how much of its working set that cache holds. Where it limits the frame, items 3 to 5 are the levers.
