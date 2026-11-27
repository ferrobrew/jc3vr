# Frame-time benchmark

The capture harness's comparisons say whether single-pass renders the same image as multipass; the benchmark says what each costs. It plays the game forward in a scene for a few seconds in each render mode and records every frame's CPU time per thread, GPU time per dispatch, and draw submissions, at the session's render size or a sweep of them. What the results say is in [performance.md](../performance/performance.md#what-limits-the-frame). The payload side is `crates/jc3vr-capture/src/testing/benchmark/`, the harness side `tools/rendering/benchmark/`, and the scenes and mode variants `tools/rendering/suites/benchmark.toml`.

## Running it

Launch a session at the headset's size with the compositor's pace well above what the game reaches (`--compositor-fps 1000`), so the frame interval is the frame's cost rather than the pace. The numbers must come from the release payload, but the session launches the debug one, and the runtime allows one instance per client, so a re-injected release payload must use the debug loader's instance: set `loader_path` in the `[vr]` table of the session's `settings.toml` to the debug build's `openxr_loader.dll` before the first run, which builds and re-injects the release payload:

```sh
python3 -m tools.rendering benchmark --session N --build --release
python3 -m tools.rendering benchmark --session N --only capital --modes single_pass fsr-off --scales 0.7071 1 1.4142
python3 -m tools.rendering benchmark summary target/harness-probe/benchmark/<stamp>
python3 -m tools.rendering benchmark charts target/harness-probe/benchmark/<stamp>
```

`--modes` takes `multipass`, `single_pass`, and the suite's variant names, and `--scales` sweeps `vr.resolution_scale`; `--help` lists the rest (`tools/rendering/benchmark/cli.py`).

## What a run does

Each scene is staged as the fast comparison stages it ([fast-comparison.md](fast-comparison.md#reusing-the-stage)). For each size, repeat, and mode the harness sends one `benchmark` request (`tools/rendering/protocol/benchmark.py`), rotating the modes between repeats so a drift over the scene does not always land on one mode. The payload applies the mode as a player would (single-pass is the whole feature switched on together, `stereo::config::set_single_pass_feature`, with a variant's switches on top, and FSR and the foveation at their shipped settings unless the variant names them), holds the camera with the full-camera freeze at the scene's first pose or moves it along the scene's path, waits until the shaders and sizes match, warms up, and records every frame. Only the camera is held: the clock, the simulation, the AI, and the streaming run on.

## What is measured

Each frame row is the difference between two readings of never-reset counters a frame apart (`testing/benchmark/sample.rs`):

- The always-on frame breakdown ([performance.md](../performance/performance.md#the-frame-breakdown)): the frame interval, each thread's self time, and the slots for the draw task, its drains, the OpenXR calls, the runtime lock, and the simulation.
- The engine's draw calls and dispatches (`hooks::draw_count`) and the stereo draw path's submissions (`probes::submission_total`). The harness installs the draw detours in every run, so every mode pays for them alike.
- The GPU timestamp layer ([performance.md](../performance/performance.md#the-gpu-lane)): each dispatch's GPU span, the idle before it, and each seam's time; `--gpu-passes` adds per-pass times at a small GPU cost.
- `/proc/<game>/task/*/stat`, sampled by the harness (`benchmark/threads.py`): every thread's busy fraction, the only measure of DXVK's threads, which execute what the draw task records. A thread busy for nearly the whole window is what limits the frame.

The summary (`benchmark/stats.py`) takes each metric's median, 99th percentile, and worst frame per run, and across repeats the median of medians and their spread. `gpu_ms` is an upper bound on the GPU's work, since a dispatch the CPU feeds too slowly idles inside its span: the GPU limits the frame when it approaches `frame_ms` with `gpu_idle_ms` near zero.

### Where the draw task's time goes

`--sample-draw-task` profiles the draw task by sampling (`testing::benchmark::sampler`): a payload thread suspends the draw thread about once a millisecond and copies its registers and stack top with the pass it is in. `benchmark samples` (`benchmark/samples.py`) unwinds the copies and charges each sample to the innermost caller outside DXVK, Wine, and the C runtime, so time in DXVK lands on the function that called it. Sampling slows a run by a few percent.

## Result files and limits

Every run writes `runs/<scene>/<label>.json` and the aggregate `summary.json` and `summary.md`, versioned formats whose fields keep their meaning within a version (`benchmark/run.py`, `report.py`); `benchmark charts` draws them as SVGs.

The harness runs under Proton and DXVK with Monado's simulated headset and null compositor: no real compositor, reprojection, or lens pass, and pacing that is not a headset's. The benchmark is for comparing modes on one machine, scene, and camera; the absolute numbers are that machine's, and the resolution sweep resizes the scene's targets but not the swapchain.
