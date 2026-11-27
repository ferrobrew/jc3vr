# Frame-time benchmark

The capture harness's comparisons say whether single-pass renders the same image as multipass; the benchmark says what each costs. It plays the game forward in a scene for a few seconds in each render mode and records every frame's CPU time per thread, GPU time per dispatch, and draw submissions, at the session's render size or a sweep of them. What its results at the headset's size say is in [performance.md](../performance/performance.md#what-limits-the-frame). The payload side is `crates/jc3vr-capture/src/testing/benchmark/`, the harness side `tools/rendering/benchmark/`, and the scenes and mode variants `tools/rendering/suites/benchmark.toml`.

## Running it

Launch a session at the headset's size with the compositor's pace well above what the game reaches, so the frame interval is the frame's cost rather than the pace:

```sh
python3 -m tools.rendering.session.matched_run N --gpu <vendor:device> --prior M --unmatched --build-only --resolution-scale 1.0 --eye-size 3584x3561 --compositor-fps 1000
```

The numbers must come from the release payload, but the session launches the debug one. A re-injected payload loads the OpenXR loader beside its own file, and the runtime allows one instance per client, so a release payload re-injected into a debug-launched session cannot reuse the instance the debug loader created. Point the session at the debug loader first, by setting `loader_path` in the `[vr]` table of `target/harness-probe/persistent-N/config/settings.toml` to the debug build's `openxr_loader.dll`, then build and re-inject the release payload with the first run:

```sh
python3 -m tools.rendering benchmark --session N --build --release                  # the whole suite, multipass and single-pass, three repeats
python3 -m tools.rendering benchmark --session N --only capital town --scales 0.5 0.7071 1 1.4142 2
python3 -m tools.rendering benchmark --session N --only capital --modes single_pass fsr-off taa
python3 -m tools.rendering benchmark summary target/harness-probe/benchmark/<stamp>
python3 -m tools.rendering benchmark charts target/harness-probe/benchmark/<stamp>
```

`--modes` takes `multipass`, `single_pass`, and the suite's variant names: a variant forces single-pass switches on the single-pass mode, or FSR, its hybrid decode, its jitter, the TAA, the foveation, or `vr.overlap_render_update` on either mode. `--scales` sweeps `vr.resolution_scale` (each side, so 1.4142 is twice the pixels). `python3 -m tools.rendering benchmark --help` lists the rest (`tools/rendering/benchmark/cli.py`), among them the repeats and the record and warm-up times, and the optional measurements below. The run stops before the disk falls below `--min-free-gib`. The results land under `target/harness-probe/benchmark/<stamp>/`, and the command prints the summary table.

## What a run does

Each scene is staged as the fast comparison stages it, reusing the stage when the session staged it last ([fast-comparison.md](fast-comparison.md#reusing-the-stage)). Then, for each render size, each repeat, and each mode, the harness sends one `benchmark` request (`tools/rendering/protocol/benchmark.py`) and samples the game's threads while it waits. The modes rotate from repeat to repeat, so a drift over the scene does not always land on the same mode. On the payload, a run:

1. Applies its mode as a player would: multipass is the shipped configuration, and single-pass is the whole feature switched on together, as the debug UI's one button does (`stereo::config::set_single_pass_feature`), with a variant's switches forced on top. FSR and the foveation are set to their shipped settings whatever the session configured (the harness launches sessions with the foveation off), then to the variant's where it names them. The resolution scale is applied live: the scene's targets resize at once, while the OpenXR swapchain keeps the size it was created at.
2. Holds the camera with the full-camera freeze (`vr::pose_control`) at the pose the first run of the scene captured, so every mode renders the same view, or moves it along the scene's `camera` path (a sine in yaw and along the heading). Only the camera is held: the clock, the simulation, the AI, the traffic, and the streaming run on, unlike the capture profile, which also pauses the clock.
3. Requests the shader reload when the live shaders were built under another state, and waits until it is done, the stereo mode and target are as asked, the engine's size matches the render target, and the camera is held.
4. Warms up (FSR's history is reset at its start), then records every frame and writes the acknowledgement, `captures/<id>.json`, with every frame.

The scene's last run puts the configuration back and releases the camera.

## What is measured

Each frame row is the difference between two readings of never-reset counters taken a frame apart at the top of the game's update (`crates/jc3vr-capture/src/testing/benchmark/sample.rs`):

- From the always-on frame breakdown ([performance.md](../performance/performance.md#the-frame-breakdown)): the frame interval, each thread's self time by kind, and the slots that say where the time went, among them the draw task (`draw_task`, the engine's walk of the render passes that records a dispatch's draws, with the stereo draw path's detours inside it), the drains that wait on it, the OpenXR calls, the runtime lock the deferred tail holds, and the simulation.
- The engine's draw calls and compute dispatches through its `Graphics::` wrappers (`hooks::draw_count`), which count a block re-issued per eye once per eye but miss blocks that call Direct3D directly, and the stereo draw path's submissions (`probes::submission_total`), counted while the draw path's detours are installed. The harness installs them in every run, so every mode pays for them alike; `--no-draw-detours` in a fresh session measures multipass without them.
- From the GPU timestamp layer ([performance.md](../performance/performance.md#the-gpu-lane)): each dispatch's GPU span (its whole window, idle inside it included), the idle gap before it, the CPU time spent recording it, and the time of each seam, from the prepasses through the G-buffer, the scene, the post effects (with FSR inside them), and the mod's own work after `PostDraw`. The frame tail's swapchain blit is outside every dispatch and not timed. The read-back trails the CPU by a few frames.
- With `--gpu-passes`, each render pass's GPU time and that of the hooked post effects and the foveation's passes, totalled per dispatch lane and label over the recording, with the holes between timed intervals totalled as gaps named by where they lie (the engine's clears, copies, and resolves outside a pass's walk land there). The extra queries add a little GPU time, so compare against a run without them before reading small differences.
- From `/proc/<game>/task/*/stat`, sampled by the harness every quarter second (`tools/rendering/benchmark/threads.py`): every thread's busy fraction over the recording window. The payload names the draw task's thread `jc3-draw-task` during a run, and DXVK names its command-stream, submission, and queue threads. This is the only measure of DXVK's threads, which execute the Direct3D calls the draw task records; a thread busy for nearly the whole window is what limits the frame.

The summary (`tools/rendering/benchmark/stats.py`) reports per run, and then per scene, mode, and size over the repeats:

| Metric | What it is |
|---|---|
| `frame_ms` | The interval between successive frame starts. With the compositor pacing below what the game reaches, it is the pace; well above it, the frame's cost. |
| `game_busy_ms` | The game thread's own work, waits excluded. |
| `draw_ms` | The draw task's CPU time plus the post-draw work, over the frame's dispatches. |
| `gpu_ms` | The summed GPU spans: an upper bound on the GPU's work, since a dispatch the CPU feeds too slowly idles inside its span. The GPU limits the frame when this approaches `frame_ms` and `gpu_idle_ms` is near zero. |
| `gpu_idle_ms` | The GPU's measured idle between dispatches. |
| `gpu_<seam>_ms` | Each seam's GPU time. |
| `wait_compositor_ms`, `wait_draw_ms`, `wait_tail_lock_ms` | The game thread blocked in `xrWaitFrame`, on the draw task, and on the frame tail's hold of the runtime. |
| `mod_ms` | The mod's scoped work on every thread, except the draw detours, which are inside `draw_ms`. |
| `pose_age_ms` | How old a submitted frame's head pose is at its `xrEndFrame` (`vr::pose_age_totals`). |
| `d3d_submissions`, `engine_draws` | The submissions and the engine's draw calls per frame. |

Each metric's median, 99th percentile, and worst frame are taken per run; across the repeats the summary gives the median of the medians and of the 99th percentiles, the worst frame of any repeat, and the spread of the medians.

### The draw detours' own cost

`--detour-timing` arms timers in the draw entry detours (`stereo::single_pass::draw_detours`) and around the forwarding call in the submission funnel (`stereo::single_pass::detour_timing`). The difference is what the detours spend around the draws, the state changes they make for a draw included. The payload measures a clock-read pair's cost at the end of the run and the summary takes it out. The timing inflates the frame it measures, so its frame times are not comparable with other runs'.

### Where the draw task's time goes

`--sample-draw-task` profiles the draw task by sampling (`testing::benchmark::sampler`). A payload thread wakes about once a millisecond, suspends the thread running the draw task, reads its registers and copies the top of its stack, and resumes it; each sample also carries the render pass, block type, and per-eye re-issue the draw path had published. The copy is unwound afterwards through the modules' unwind tables. `benchmark samples <run.json>...` (`tools/rendering/benchmark/samples.py`) pools the samples and prints their share of the draw task by module, seam, pass, block type, re-issue, and the innermost caller outside DXVK, Wine, and the C runtime, so time in DXVK is charged to the game or payload function that called it. Game addresses are printed raw (`--addresses` writes them for a lookup in the IDB); payload addresses are named from the PDB with `llvm-symbolizer`. A sample suspends the draw thread for tens of microseconds, so a sampled run is slower by a few percent and the summary marks it, and a sample's instruction pointer tends to land on the instruction after a slow one.

## Result files

Every run writes `runs/<scene>/<label>.json` (`jc3vr-benchmark-run` version 1): the run's mode and forced settings, the session and its compositor pace, the machine, the payload's acknowledgement (the environment it ran in, the configuration timing, one row per frame and per GPU dispatch in microseconds, the per-pass totals, every slot's totals, and any draw-task samples), the thread samples, and the run's summary. `summary.json` (`jc3vr-benchmark-summary` version 1) and `summary.md` aggregate them per scene, mode, scale, eye size, pace, and measurement option. A field keeps its meaning within a version; one that changes meaning or goes away bumps it. `tools/rendering/benchmark/run.py` and `report.py` write them.

`benchmark charts` writes standalone SVGs into `charts/`: each scene and mode's frame-time distribution over the repeats, and each mode's median frame and GPU time against the megapixels per eye, both marked with the frame budgets of the common headset rates. A run whose median frame exceeds 100 ms is left out: the GPU ran out of memory and the frame measures paging.

## What it can and cannot say

The harness runs the game under Proton and DXVK, displayed by gamescope's headless backend, with Monado's simulated headset and null compositor: no real compositor, no reprojection, no lens distortion pass, no second process competing for the GPU, and pacing that is not a headset's. What the benchmark is for is the comparison between modes on the same machine, in the same scene, from the same camera; the absolute numbers are that machine's, and a weaker GPU reaches the point where the GPU limits the frame at a smaller render size. The scenes are a few views of the world whose cost varies with what streams in and what the simulation does, which the repeats' spread shows; the GPU spans are an upper bound; the per-thread CPU times have the host scheduler's resolution; and the resolution sweep resizes the scene's targets but not the swapchain.
