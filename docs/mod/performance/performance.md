# Performance

The mod's frame is measured with three tools, each answering a different question:

- The always-on frame breakdown ([frame-timing.md](frame-timing.md)) runs in every build and logs every ten seconds whether a slow session is the mod, the engine, the OpenXR runtime, or a wait. Read it first, so the next measurement can be aimed.
- The in-game profiler ([profiler.md](profiler.md)) captures a few seconds of CPU scopes and per-pass GPU timestamps on demand (F9), for what is inside the part of the frame that grew.
- The frame-time benchmark ([benchmark.md](../testing/benchmark.md)) plays a scene forward in each render mode under the capture harness and records every frame, for comparisons between modes, settings, and render sizes. Its results at the headset's size are in [frame-budget.md](frame-budget.md).

## What limits the frame

The geometry passes are bound by draw submission, not by shading: the draw task's time is flat across render sizes, while the scene and post passes grow with the pixels. Most of the draw task is DXVK recording the engine's state changes, which is memory traffic, so it depends heavily on the CPU cache it runs against. Single-pass stereo cuts the submissions, and the mod runs the game on the cores of the largest L3 where the L3 sizes differ. With both, and with the render update overlapped with the draw, every benchmark scene is GPU-bound at the headset's size on the benchmark machine, and FSR is the largest single GPU cost. The numbers, and the ranked remaining costs, are in [frame-budget.md](frame-budget.md).

`PreDraw` (the shadow, reflection, and vegetation prepasses) runs once per dispatch, so single-pass already issues it once per frame, and nothing in it is drawn per eye (see "Why `PreDraw` is outside the collapse" in [single-pass-stereo.md](../stereo/single-pass-stereo.md)).

## Measuring

- Measure the release payload (`scripts/proton_run.sh --release`). A debug payload's numbers describe the debug build.
- Leave the profiler's per-draw render-block scopes off unless the per-type timeline is the question: they put hundreds of scopes a frame on the draw-submission path they would measure.
- Read busy as an upper bound on GPU work and starved as a lower bound on GPU idle ([profiler.md](profiler.md#reading-the-gpu-lane-work-not-span)). A GPU span that tracks the CPU submit span is submission-bound. A resolution sweep settles which limit applies better than any single capture.
- Under a compositor that paces, a frame that misses the period costs whole periods, and the compositor's wait can move between sessions. Compare the work inside the frame, not only the frame interval ([frame-budget.md](frame-budget.md#reading-the-frame-interval)).
- The engine renders at the runtime's recommended size scaled by the current `vr.resolution_scale`, while the OpenXR swapchain keeps the size it was created at, and the eye-to-swapchain step is a scaling blit. A live change to the scale makes the two disagree, and then every frame pays a resample that looks like softness. Both sizes are in the log (`created the stereo swapchain` against the resize target).
