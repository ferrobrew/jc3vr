# The learned body estimator

The `body-estimator` crate runs EgoPoser (Jiang et al., ECCV 2024), a learned full-body estimator that reads a window of head and hand poses and estimates an SMPL body: the pelvis's global orientation and the 21 other joints' rotations. It is meant to tell the body rig where the pelvis and chest face when the head and hands alone leave it ambiguous. Nothing in the payload or the rig uses it yet; this document covers the estimator itself, its two implementations, how they are tested, and what each costs.

The crate has no game dependencies, so the offline tools (`tools/tracking-synth`) run it on Linux and the payload can later run it in the game.

## The weights

The checkpoint is not in the repository and must never be committed. The crate loads it at run time from a safetensors file holding the original `.pth` state dict under PyTorch's own parameter names. Tests and tools take the path from `JC3VR_EGOPOSER_WEIGHTS`, and the tests that need the weights skip with a message when it is unset. The released checkpoints have no shape head; the code supports one where a checkpoint has it.

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
