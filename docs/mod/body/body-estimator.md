# The learned body estimator

The `body-estimator` crate runs EgoPoser (Jiang et al., ECCV 2024), a learned full-body estimator that reads a window of head and hand poses and estimates an SMPL body: the pelvis's global orientation and the 21 other joints' rotations. It tells the body rig where the pelvis and chest face when the head and hands alone leave it ambiguous. This document covers the estimator itself, its two implementations, how they are tested, what each costs, how the payload runs it in the game, and which other parts of the body could take its estimate. How the rig's yaw uses the estimate is in [rig.md, "The yaw"](rig.md#the-yaw).

The crate has no game dependencies, so the offline tools (`tools/tracking-synth`) run it on Linux and the payload runs it in the game.

## The weights

The checkpoint is not in the repository and must never be committed: it is trained on AMASS, whose licence is non-commercial, and `.gitignore` keeps `.pth` and `.safetensors` files out (the golden fixture, which holds inputs and outputs only, excepted). The crate loads it at run time from a safetensors file holding the original `.pth` state dict under PyTorch's own parameter names, every tensor 32-bit. Tests and tools take the path from `JC3VR_EGOPOSER_WEIGHTS`, and the tests that need the weights skip with a message when it is unset. The released checkpoints have no shape head; the code supports one where a checkpoint has it.

To set the weights up:

1. Download `egoposer.pth`, the base model (not the large, the 30 fps, or the hand-tracking variants), from the Google Drive folder the [EgoPoser repository](https://github.com/eth-siplab/EgoPoser) links under its pre-trained models, by hand or with `uvx gdown --folder <the folder's link>`.
2. Run `scripts/egoposer_weights.sh path/to/egoposer.pth`. It converts the state dict with PyTorch from a throwaway `uv` environment (PyTorch's CPU-only build), writes `.egoposer/egoposer.safetensors`, and copies it next to `jc3vr_payload.dll` in every built `target/x86_64-pc-windows-msvc/{debug,release}` directory. Run it again after a clean build, or copy the cached file.

The payload reads the file from `controllers.body.estimator.weights_path` when that is set, and otherwise from `egoposer.safetensors` next to the payload DLL. The same file is what `JC3VR_EGOPOSER_WEIGHTS` and `tracking-synth --weights` take.

## The network

Each frame of the input is 60 values: the 54-value sparse feature vector EgoPoser's data pipeline builds from the head and both wrists (their global rotations and frame-to-frame rotations in 6D, their positions, and their frame-to-frame steps), with the head's horizontal position taken off every joint, followed by six window-start deltas, each joint's horizontal position less the window's first frame's. A hand the headset could not see has its values zeroed in that frame.

A window is 80 frames at 60 Hz. It splits into a slow stream, every second frame, and a fast stream, the last 40 frames. Both streams go through the same linear embedding from 60 to 256 values and are summed token by token into 40 tokens, with no positional encoding. A three-layer post-norm transformer encoder follows (model width 256, eight heads, a feed-forward of 2048 with ReLU, no dropout at inference). The last token alone goes to two decoders, each a 256-wide hidden layer with ReLU and an output layer: one gives the root orientation (six values, 6D) and one the body pose (126 values, 6D per joint).

## The CPU reference

`EgoPoser::infer` is the network in plain Rust, matrix products through `matrixmultiply`. It is written to be clear rather than fast, and golden tests check it against the original PyTorch model's outputs on real windows from a recorded take (`body-estimator/tests/fixtures/egoposer_golden.safetensors`, inputs and outputs only), every stage to a relative error of about 1e-6. `EgoPoser::infer_traced` also returns the intermediate tensors.

Only the last token reaches the decoders, so the last encoder layer computes its output alone: its keys and values come from all 40 tokens, but the query, the attention's output projection, both residuals, both layer norms, and the feed-forward are the last token's only. After the attention every token's row is computed independently of the others', so this is exact. On one core of the development machine (release build, single-threaded, `taskset` to one CPU) it took an inference from 3.0 ms to 2.56 ms. That is still too much to spend on the game's CPU every frame, which is why the GPU executor exists; the CPU pass stays as the reference the GPU pass is tested against.

## The GPU executor

`egoposer::gpu::GpuEgoPoser` runs the same pass through Direct3D 11 compute shaders, on any device, including the game's own. It is Windows-only and behind the crate's `d3d11` feature, which is on by default and pulls in nothing on other targets, so the Linux builds of the tools are unaffected.

Creating it uploads the weights once. The weight matrices are stored as 32-bit or as 16-bit floats (`WeightPrecision`), converted from the checkpoint's 32-bit values at load; the biases, the layer norms, every activation, and all arithmetic stay 32-bit. A device below feature level 11_0 has no compute shaders and is not a case the executor handles: it panics, since no machine that runs the game is in that state. Buffer, view, and shader creation failures, a checkpoint whose model width the kernels are not written for, and a failed map are reported as `GpuError`.

Each inference never waits on the GPU. `submit` computes the 40 token inputs on the CPU (the slow stream's frame plus the fast stream's; the embedding is linear, so embedding the sum with the bias added twice gives the summed embeddings), uploads them, records the pass on the given device context, and copies the estimate into the next staging buffer of a ring of three. A frame or more later, `poll` maps the oldest staging buffer without waiting and returns its estimate with the number `submit` gave it once the GPU has finished. An estimate still uncollected when its staging buffer comes round again is dropped. `infer_blocking` waits instead, for tests and tools. The executor saves and restores the compute state it changes, so it can record onto a context the engine shares.

The pass is four kernels in `body-estimator/src/shaders/`, compiled by `shadergen` into committed `.dxbc` beside their sources:

- `egoposer_linear.hlsl`, a linear layer with an optional ReLU, compiled once for 32-bit and once for 16-bit weights. A thread group computes one output for eight rows; each row's sixteen threads split the inputs four values at a time, interleaved so the weight row and the input rows are read coalesced, and their partial sums meet in groupshared memory. Each thread keeps a single running sum, so nothing spills to local memory.
- `egoposer_layer_norm.hlsl`, the residual add and the layer norm, a thread group per row with tree reductions for the mean and the variance.
- `egoposer_attention.hlsl`, the multi-head attention before its output projection, a thread group per head and query: a thread per key scores it, the softmax's weights go through groupshared memory, and a thread per column sums the weighted values.

The plan lays the network out as 25 dispatches: the embedding, seven per encoder layer (the fused query, key, and value projection, the attention, the output projection, the first norm, the two feed-forward layers, and the second norm), the decoders' hidden layers stacked into one, and one output layer per decoder. The last encoder layer is trimmed as on the CPU. Every activation has a buffer of its own, which costs under 2 MB and lets the tests read back any stage.

## Testing

The GPU tests compare against the CPU reference on the golden fixture's windows, in three ways. Each kernel is checked in isolation, the CPU op fed the GPU's own input to that kernel, so one kernel's error cannot hide behind another's. The whole pass is checked stage by stage against the CPU's trace. The final estimate is checked on every fixture window and on one with the hands hidden. With 16-bit weights, the kernel and stage checks run against the CPU on the same rounded weights, which isolates the kernels, and the final estimates are compared with the CPU on the checkpoint's own weights, which measures what the rounding costs. A last test checks that `poll` returns estimates in order without waiting and drops the one the ring overwrote.

The tests need a Direct3D 11 device. `./scripts/xwin_test.sh -p body-estimator` runs them under wine on wine's own Direct3D (wined3d), with WARP as the fallback where there is no hardware device. To run them on the Direct3D 11 the game itself runs on under Proton, use `scripts/dxvk_run.sh` as the cargo runner. It copies the binary next to Proton's DXVK DLLs in a scratch directory under `target/` and runs it in the shared prefix with overrides that select them, leaving the prefix itself on wine's Direct3D:

```sh
JC3VR_EGOPOSER_WEIGHTS=/path/to/egoposer.safetensors \
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER="$PWD/scripts/dxvk_run.sh" \
    ./scripts/xwin_test.sh -p body-estimator -- gpu --nocapture
```

The tests print the adapter they run on and every comparison's maximum error. On either Direct3D, with 32-bit weights every kernel and every stage matches the CPU to a maximum absolute error of about 1e-6 (relative to the tensor's largest value, below 1e-6), the same as the CPU's own agreement with PyTorch. With 16-bit weights the kernels match the CPU on the rounded weights as closely, and the rounding moves the final estimate by at most 2e-4 in the root orientation's 6D values and 1.1e-3 in the body pose's, which is at most 0.016 degrees of pelvis or chest yaw on the fixture's windows. The tests hold 16-bit weights to 5e-3 and 0.1 degrees.

## Costs

`body-estimator/examples/egoposer_bench.rs` times the CPU reference and, on Windows, the GPU executor. For the real GPU, run it through DXVK:

```sh
cargo xwin build --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc -p body-estimator --release --example egoposer_bench
JC3VR_EGOPOSER_WEIGHTS=/path/to/egoposer.safetensors \
    ./scripts/dxvk_run.sh target/x86_64-pc-windows-msvc/release/examples/egoposer_bench.exe
```

It measures the GPU time of a pass with timestamp queries inside a disjoint query, both from idle and with the GPU kept busy by unmeasured passes after a two-second warm-up, the CPU time `submit` takes to record a pass, and, in a 90 Hz frame loop that polls and then submits each frame, how many frames after its submission each estimate is read back. On an RTX 5090 through DXVK:

| | 32-bit weights | 16-bit weights |
|---|---|---|
| GPU time per inference | 123 µs | 115 µs |
| CPU time to record (`submit`) | 7 µs | 7 µs |
| Readback | the next frame | the next frame |

The GPU time is the same from idle and under load, and the 16-bit weights save little, because the pass is bound by its 25 dispatches rather than by arithmetic or bandwidth: each dispatch depends on the previous one's output, and under DXVK each one costs a few microseconds of launch and barrier however little it computes. The two feed-forward layers of the full-width encoder layers are the largest single costs, at about 13 to 16 µs each. Fusing kernels (the residual norms into their neighbouring layers, the decoders' output layers into one) would cut the dispatch count if the cost ever matters. The 16-bit weights halve the weights' memory, to about 8 MB, at no measurable cost in accuracy, so they are the precision to choose.

## In the game

The payload runs the GPU executor on the game's own Direct3D 11 device (`payload/src/controllers/body/estimator/`). Producing the estimate is separate from consuming it: the producer runs whenever the controller scheme is in effect and the head is tracked, in every mode, on foot or not, and publishes the latest estimate through one accessor (`estimator::latest`) that any part of the mod can read. Under the gamepad scheme nothing runs and nothing is published, so head-tracked gamepad play is as it was. Only the grounded rig consumes the estimate for now, standing and seated, while Rico is on the ground and the player owns his root; everywhere else the estimate is published and drawn but unused.

The game thread builds the window. Once per rendered frame, the pose composition (`roomscale::compose_cockpit`) hands the producer the eyes, the head's orientation, and the tracked grips in the rig's room frame: the cockpit frame with its floor at zero, in the player's metres, the grips lifted with the eyes. Seated, the eyes are at the standing eye height plus the head's travel from the seated reference, the remap the rig itself reads ([rig.md, "Posture"](rig.md#posture)), so a seated player's window looks like a standing one; the network's legs are not used. `body_estimator::window::WindowBuilder` keeps the window, mirroring what `tracking-synth estimate-body` does to a whole recording:

- The poses go into a room frame of the window's own that no recenter moves. A recenter or a follow bank turns the cockpit frame; its turn reaches the window from `roomscale::on_recenter`, the window folds it into its map from the cockpit frame, and the first frame after it takes out the cockpit frame's shift by keeping the eyes' horizontal velocity continuous. Snap and smooth turning do not move the cockpit frame, so they need nothing.
- Frames arrive at the headset's rate and are resampled onto a 60 Hz grid, positions linearly and orientations by slerp.
- A controller that has lost tracking marks its hand not visible in the frames it covers, which the network masks; its pose holds the last one seen.
- A gap of more than a quarter of a second, a change of posture (which moves the eyes' height by the remap's difference), or the estimator being turned off, the gamepad scheme, or a lost head starts the window again; a full window takes 1.35 s to refill.

At the submit rate (`controllers.body.estimator.submit_hz`, 30 Hz by default) the producer posts the full window to a mailbox, a mutex around one value that a newer window replaces. The render thread's post-draw hook, once per frame on the frame's last near dispatch with the engine's context lock held, takes the window, records the pass with `GpuEgoPoser::submit`, and polls the staging ring without waiting; every finished estimate is matched to its submission and published to a second mailbox the same way. The executor saves and restores the compute state it touches. A readback usually arrives the frame after its submission, so an estimate's age, from the window's last pose to now, is a frame or two plus up to one 60 Hz grid step.

The weights load on a thread of their own when the first window arrives, so a gamepad player never reads them, and the render thread uploads them with 16-bit weight matrices (`WeightPrecision::F16`). The log says when the estimator starts running, with the weights' path and precision. A missing or unreadable weights file while the estimator is on is a panic whose message names the path it was expected at and how to produce it (`scripts/egoposer_weights.sh`, or set the path, or turn the estimator off): the weights are part of the installation, so their absence is a broken install rather than a state to run in. Turning the estimator off drops the executor and its GPU resources and withdraws the estimate.

`estimator::latest` gives the estimate in the cockpit frame as it stands when it is read, so a recenter between the submission and the read changes nothing: the pelvis's and the upper chest's (SMPL spine3's) headings in `body-rig`'s yaw convention, every SMPL body joint's global rotation, its age, the posture whose remap the window was in, and how long the readback took. An estimate older than half a second is stale for any consumer. The grounded rig takes it as a `body_rig::BodyEstimate` whose confidence falls from one to none between `yaw.estimate.fresh_s` and `yaw.estimate.stale_s` of age (`EstimateTuning::confidence`), so a stalled estimator eases out of the yaw rather than holding it on an old reading.

### Settings, readouts, and the floor arrows

The debug UI's Body fit feature has a `Learned body estimate` section: the switch (`controllers.body.estimator.enabled`, on), the submit rate, the weights' path (empty for the default), the floor arrows' switch, and the estimate's tuning in the yaw (`controllers.body.rig.yaw.estimate.*`, described in [rig.md](rig.md#the-yaw)). Nothing is on the pause menu. The readout shows the executor's stage (`off`, `waiting` for the first window, `loading`, `running`, or `failed` with the error), the windows submitted and read and the estimates the ring dropped, the window's pose count, the CPU time of the latest submission, the weights' path, and the latest estimate's pelvis and chest headings against the head, its age (marked stale past half a second), its readback time, and its posture; while the grounded rig runs, also the rig's legs and chest against the head, the estimate's say over them, and the smoothed pelvis heading the legs follow. The GPU time of a pass is not measured in the game; the benchmark above measures it.

The floor arrows (`controllers.overlay.body_estimate`, off by default since they crowd the view in play, and `Body estimate` in the visualisation drawer) are drawn under the player's eyes in every mode the estimator runs in: a long green arrow for the estimate's pelvis and a short light green one for its chest, dimmed while stale, and while the grounded rig runs, a long magenta arrow for the rig's legs and a short light magenta one for its chest beside them. They hang from the head in the player's own metres, like the rig's own diagnostics.

The recordings' settings event carries the whole configuration, so a take records whether the estimator was on and its tuning. The estimate itself is not recorded: `tracking-synth estimate-body`, and `rig-eval --weights`, recompute it from the recorded head and grip poses.

## Other consumers

Only the grounded rig reads the estimate. These are the other parts of the body that could, and what each would take. Each should be checked offline on recordings of the state first (`tracking-synth estimate-body` over the stretch, against the video and Rico as drawn), since EgoPoser was trained on AMASS: people standing, walking, and moving about on the ground, with the hands free.

- The in-air torso heading. Off the ground, every estimate of the player's shoulders (the hands' shoulders, the arm remap, the wingsuit's arms, and the ripcord) takes the torso's heading from one follower (`rig::torso_heading`, reading `procedural::wings::WingHeading`; [procedural-states.md](procedural-states.md#the-wingsuit)): the line between the wrists while they are spread, and otherwise a slow follower of the head. The estimate's chest heading could stand in for the slow follower. The wingsuit's spread arms, held out level for long stretches, are far from anything in AMASS, and the parachute's raised hands on the handles are not much closer, so the estimate there is most likely to be wrong; the wrist line already reads the spread arms directly.
- Vehicle seats. Seated in a vehicle, the head's heading stands in for the torso's. The estimate's chest could replace it for the hands' shoulders and the arm remap. A seated player in a real chair is closer to AMASS than the air states, but the hands on a wheel or a stick and the window seeing no walking are less so; the seated remap would apply as on foot.
- Swimming. The swim's yaw ([swim-yaw.md](swim-yaw.md)) decides where Rico swims from the head and the hands. The player stands or sits while Rico swims, so the estimate would read the player's real body, which is what the swim yaw approximates, but the arm strokes are far from AMASS's motions and would need checking.
- Grapple hangs. Hanging from the grapple, the game owns the root and the mod poses the torso; the estimate's chest could turn it. The player usually holds one arm up toward the grapple, which AMASS has little of.

Each consumer would read `estimator::latest`, judge the estimate's age for itself (the rig's confidence ramp is one way), and blend it in and out, as the rig does, so that the estimate appearing and disappearing never snaps the body.
