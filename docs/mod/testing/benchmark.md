# Frame-time benchmark

The capture harness's comparisons say whether single-pass renders the same image as multipass; the benchmark says what each costs. It plays the game forward in a scene for a few seconds in each render mode and records every frame's CPU time per thread, GPU time per dispatch, and draw submissions, at the session's render size or a sweep of them. The results at the headset's size, and what they say, are in [frame-budget.md](../performance/frame-budget.md). The payload side is `payload/src/testing/benchmark/`, the harness side `tools/rendering/benchmark/`, and the scene list `tools/rendering/suites/benchmark.toml`.

## Running it

Launch a session at the headset's size with the compositor's pace well above what the game reaches, so the frame interval is the frame's cost rather than the pace:

```sh
python3 -m tools.rendering.session.matched_run N --gpu 10de:2b85 --prior M --unmatched --build-only --resolution-scale 1.0 --eye-size 3584x3561 --compositor-fps 1000
```

The numbers must come from the release payload. The session launches the debug one, and a re-injected payload loads the OpenXR loader beside its own file, so a release payload re-injected into a debug-launched session cannot reuse the session's OpenXR instance (the debug payload's loader created it, and the runtime allows one instance per client). Point the session at the debug loader first, by adding `loader_path = "Z:\\...\\target\\x86_64-pc-windows-msvc\\debug\\openxr_loader.dll"` to the `[vr]` table of `target/harness-probe/persistent-N/config/settings.toml`, then build and re-inject the release payload as part of the first run:

```sh
python3 -m tools.rendering benchmark --session N --build --release                  # the whole suite, multipass and single-pass, three repeats
python3 -m tools.rendering benchmark --session N --only capital town --scales 0.5 0.7071 1 1.2247 1.4142 1.7321 2
python3 -m tools.rendering benchmark --session N --only capital --modes single_pass fsr-per-eye-off vegetation-once-off
python3 -m tools.rendering benchmark --session N --only capital town --modes single_pass fsr-off fsr-hybrid-off --scales 1 0.85
python3 -m tools.rendering benchmark --session N --modes multipass multipass-serial single_pass single-pass-serial
python3 -m tools.rendering benchmark --session N --only capital town --detour-timing
python3 -m tools.rendering benchmark summary target/harness-probe/benchmark/<stamp>
python3 -m tools.rendering benchmark charts target/harness-probe/benchmark/<stamp>
```

Options: `--modes` takes `multipass`, `single_pass`, and the suite's variant names (a variant forces single-pass switches on the single-pass mode, or `vr.overlap_render_update`, FSR, or FSR's hybrid decode on either mode: `multipass-serial` and `single-pass-serial` run the serial frame ordering, against the plain modes' overlapped one, and `fsr-off` and `fsr-hybrid-off` run without FSR and without its hybrid decode); `--scales` sweeps `vr.resolution_scale` (each side, so 1.4142 is twice the pixels); `--repeats` (3), `--record-seconds` (10), and `--warmup-seconds` (3) shape each run; `--no-gpu` skips the timestamp queries; `--detour-timing` times the draw detours (see below); `--no-draw-detours` leaves the stereo draw path's detours uninstalled; `--restage` stages afresh. The run stops before the disk would fall below 50 GiB. The results land under `target/harness-probe/benchmark/<stamp>/` or `--output`, and the command prints the summary table.

## What a run does

Each scene is staged as the fast comparison stages it (the stage is reused when the session staged it last), and then, for each render size, each repeat, and each mode, the harness sends one `benchmark` request (`tools/rendering/protocol/benchmark.py`) and samples the game's threads while it waits. The modes rotate from repeat to repeat, so a drift over the scene does not always land on the same mode. On the payload, a run:

1. Applies its mode as a player would: multipass is the shipped configuration, and single-pass is the whole feature switched on together, as the debug UI's one button does (`stereo::config::set_single_pass_feature`), with a variant's switches, and its `vr.overlap_render_update` when it names one, forced on top. FSR and the foveation are set to their shipped settings whatever the session configured (the harness launches sessions with the foveation off), and then FSR's switch and its hybrid decode's to the variant's, when it names them (`fsr`, `fsr_hybrid_velocity` in the request). The motion blur and the depth of field never run in VR. The resolution scale, when the request names one, is applied live: the scene's targets resize at once, while the OpenXR swapchain keeps the size it was created at, so a run at another scale still composites into a swapchain of the session's size.
2. Holds the camera with the full-camera freeze (`vr::pose_control`) at the pose the first run of the scene captured, so every mode renders the same view, or moves it along the scene's `camera` path: a sine of the time since the warm-up began, swinging the yaw by `yaw_deg` and moving forward and back along the heading by `forward_m` over `period_seconds`. Only the camera is held. The clock, the simulation, the AI, the traffic, and the streaming run on; this is not the capture profile, which also pauses the clock.
3. Requests the shader reload when the live shaders were built under another state, and waits until the reload is done, single-pass and the double-wide target are as asked, the engine's size matches the render target, and the camera is held.
4. Warms up (3 seconds by default; FSR's history is reset at its start), then records every frame for the requested time and writes the acknowledgement, `captures/<id>.json`, with every frame.

The scene's last run puts the configuration back and releases the camera, so the next scene stages with the camera following the player.

## What is measured

Each frame row is the difference between two readings of never-reset counters taken a frame apart at the top of the game's update (`payload/src/testing/benchmark/sample.rs`):

- From the always-on frame breakdown ([frame-timing.md](../performance/frame-timing.md)): the frame interval; the game thread's self time by kind (mod, OpenXR, engine, wait), the frame tail's, and the other threads'; and the slots that answer where the time went: the draw task (`draw_task`, the engine's walk of the render passes that records a dispatch's draws, with the stereo draw path's detours inside it) and its count, the post-draw work, the game thread's dispatch, the drains that wait on the draw task, `xrWaitFrame`, `xrBeginFrame`, `xrEndFrame`, the swapchain wait, the runtime lock the deferred tail holds, the submit, the simulation, the engine's update and render update, and the debug UI.
- The engine's draw calls and compute dispatches through its `Graphics::` wrappers (`hooks::draw_count`), which count a render block re-issued per eye once per eye but miss the blocks that call Direct3D directly, and the stereo draw path's submissions (`draw_census::submission_total`), the census's definition of a submission, counted only while the draw path's detours are installed. The harness installs them in every run by default, as any session that has run single-pass has them, so multipass's submissions are counted and every mode pays for the detours alike; `--no-draw-detours` in a fresh session measures multipass with no detours at all.
- From the GPU timestamp layer ([profiler.md](../performance/profiler.md)), armed for the run without the puffin lane: each dispatch's GPU span (its whole execution window, idle inside it included), the measured idle gap before it, the CPU time the draw thread spent recording it, and the time of each seam: `PreDraw` (the shadow, reflection, and vegetation prepasses), the G-buffer, the scene (lighting, opaque, sky, water, and transparents), the post effects, the post-draw work, and FSR inside the post effects. The harness groups the dispatches into frames by the first eye's dispatch and drops the frames cut short at either end of the recording. The read-back trails the CPU by a few frames, so the GPU records cover the recording window shifted by that much.
- From `/proc/<game>/task/*/stat`, sampled every quarter second by the harness (`tools/rendering/benchmark/threads.py`): every thread's CPU time over the recording window (matched by the wall-clock times the acknowledgement carries), as a busy fraction. The game thread is the process's initial thread; the payload names the draw task's thread `jc3-draw-task` while a run is active, and DXVK names its command-stream (`dxvk-cs`), submission, and queue threads. This is the only measure of DXVK's own threads, which execute the Direct3D calls the draw task records; a thread busy close to the whole window is what limits the frame.

The summary (`tools/rendering/benchmark/stats.py`) reports per run, and then per scene, mode, and size over the repeats:

| Metric | What it is |
|---|---|
| `frame_ms` | The interval between successive frame starts. With the compositor pacing below what the game reaches, it is the pace; at a pace of 1000 it is the frame's cost. |
| `game_busy_ms` | The game thread's own work: the engine, the mod, and the OpenXR calls that do work, waits excluded. |
| `draw_ms` | The draw task's CPU time plus the post-draw work, summed over the frame's dispatches. |
| `gpu_ms` | The summed GPU spans of the frame's dispatches: an upper bound on the GPU's work, since a dispatch the CPU feeds too slowly idles inside its span. The GPU limits the frame when this approaches `frame_ms` and `gpu_idle_ms` is near zero. |
| `gpu_idle_ms` | The GPU's measured idle between dispatches. |
| `gpu_<seam>_ms` | Each seam's GPU time. |
| `wait_compositor_ms`, `wait_draw_ms`, `wait_tail_lock_ms` | The game thread blocked in `xrWaitFrame`, on the draw task, and on the frame tail's hold of the runtime. |
| `mod_ms` | The mod's own scoped work on every thread (the draw detours excepted, which sit inside `draw_ms`). |
| `pose_age_ms` | How old the head pose a submitted frame was rendered from is at its `xrEndFrame`: the time from its view location to its submit (`vr::pose_age_totals`), over the rows in which a frame was submitted. |
| `d3d_submissions`, `engine_draws` | The submissions and the engine's draw calls per frame. |

Each metric's median, 99th percentile (the "1% low" as a frame time), and worst frame are taken per run; across the repeats the summary gives the median of the medians and of the 99th percentiles, the worst frame of any repeat, and the spread of the medians.

### The draw detours' own cost

`--detour-timing` arms timers in the five draw entry detours (`stereo::single_pass::draw_detours`) and around the forwarding call in the submission funnel (`stages::velocity::submit`): the outermost detour on a thread adds its whole length, and each forward its own (`testing::benchmark::detour_timing`). The difference is what the detours spend around the draws, the mod's bookkeeping and the state changes it makes for a draw (viewport splits, pinned constants, bound variants) included. Each timed detour and forward costs a clock-read pair, which the payload measures at the end of the run (`clock_pair_ns`) and the summary takes out. The timing inflates the frame it measures, so it is a run of its own and its frame times are not comparable with the others'.

## Result files

Every run writes `runs/<scene>/<label>.json`; `summary.json` and `summary.md` aggregate them, and `charts/` holds the SVG charts. The run file's format is `jc3vr-benchmark-run` version 1, and its fields keep their meaning within a version (new fields may be added; a field that changes meaning or goes away bumps the version):

- `run`: the scene, the config it was staged from, the mode name (`multipass`, `single_pass`, or a variant), the base mode, the forced switches, the forced `overlap_render_update` (`null` for the session's), the forced `fsr` and `hybrid_velocity` (`null` for the shipped settings), the resolution scale (`null` for the session's), the repeat, the label, the camera path, and the wall time the run took.
- `session`: the session number and the compositor's pace, which the payload cannot see and the harness reads from the session's launch request.
- `machine`: the CPU model and thread count, the GPU, its memory, and driver, the kernel, and the translation, display, and runtime stack.
- `payload`: the acknowledgement less its ID and state: `request` (as the payload understood it), `environment` (the build profile, the profiler feature, whether single-pass, the double-wide target, and the draw detours were active, whether the blurs were forced off, the engine and target sizes, the resolution scale, native resolution, FSR and its hybrid decode, the foveation, the far field, `overlap_render_update`, and `clock_pair_ns`), `timing` (how long the configuration took to apply and whether it reloaded the shaders, the recorded seconds, and the recording's start and end as seconds since the Unix epoch), `columns` and `frames` (one row per frame; times in microseconds), `gpu` (`seams`, `columns`, and one row per dispatch in microseconds, -1 where not measured, and the dispatches whose queries were dropped unread), and `slots` (every frame-breakdown slot's self time and calls over the recording).
- `threads`: the window the thread samples span, every group's summed busy fraction and its busiest thread, the sixteen busiest threads, and the whole process's busy sum. `null` when the samples did not bracket the recording.
- `summary`: the run's distributions, as above, and `detours` when the detour timing ran.

`summary.json` is `jc3vr-benchmark-summary` version 1: the machine and one entry per scene, mode, scale, eye size, pace, detour timing, and draw-detour state, with the aggregated metrics, the median thread-group busy fractions, and the detour summary.

## Charts

`benchmark charts` writes standalone SVG files with no dependencies: `distribution-scale-<scale>-pace-<fps>.svg`, each scene and mode's frame times pooled over the repeats (the 1st to 99th percentile, the interquartile range, and the median), and `resolution-<scene>-pace-<fps>.svg`, the median frame time (solid) and GPU time (dashed) of each mode against the megapixels per eye. Both mark the frame budgets of 45, 60, 72, 90, 120, and 144 fps (22.2, 16.7, 13.9, 11.1, 8.3, and 6.9 ms). A run whose median frame exceeds 100 ms is left out: the GPU has run out of memory and the frame measures paging.

## What it can and cannot say

The harness runs the game under Proton and DXVK, displayed by gamescope's headless backend, with Monado's simulated headset and null compositor. There is no real compositor, no reprojection, no lens distortion pass, and no second process competing for the GPU, and the null compositor's frame pacing is not a headset's. Each run records the machine it ran on. What the benchmark is for is the comparison between modes on the same machine, in the same scene, from the same camera: the absolute numbers are this machine's, and a weaker GPU moves the point where the GPU starts to limit the frame to a smaller render size.

Other limits: the scenes are a few views of the world, and a view's cost varies with what streams in and what the simulation is doing, which the repeats and their spread show; the GPU spans are an upper bound on the GPU's work; the per-thread CPU times have the host scheduler's resolution (a hundredth of a second over a ten-second window); and the resolution sweep resizes the scene's targets but not the swapchain.
