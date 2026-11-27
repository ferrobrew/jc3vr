# Evaluating XeSS against FSR 2

The question is whether the mod should adopt Intel XeSS, either as anti-aliasing in place of FSR 2 ([fsr.md](fsr.md)) or as an upscaler that renders each eye below the headset's resolution, on the premise that XeSS is newer than FSR 2.2.1 and may reconstruct better.

The recommendation is no. XeSS can be reached, and it probably reconstructs somewhat better than FSR 2.2.1 on average, and its API solves a stereo problem FSR 2's cannot. It loses on cost and effort: reaching it from a D3D11 game on non-Intel hardware needs a D3D11on12 bridge with a reported 10 to 15% frametime penalty, XeSS is the most expensive of the three upscalers on its DP4a path, and every piece of upscaling work the FSR integration lacks (the render size decoupled from the swapchain size, a display-size resolve target) would be needed unchanged for XeSS. If a newer upscaler is wanted, FSR 3.1 is the candidate: still plain compute with no ML hardware gate, MIT-licensed, with a native anti-aliasing mode and a community DX11 backend, and no interop layer or proprietary binary.

The question is worth reopening if the mod targets Intel Arc as a first-class platform, or if per-eye sub-rectangle dispatch (below) becomes the binding problem.

## The D3D11 backend is Intel-only

The SDK (`github.com/intel/xess`) has a `xess_d3d11.h`, which notes that the D3D11 version only works on Intel hardware; Intel's developer guide limits it to Arc graphics and later, with context creation failing with `XESS_RESULT_ERROR_UNSUPPORTED_DEVICE` elsewhere. `libxess_dx11.dll` is a shim: it imports neither `d3d11.dll` nor `d3d12.dll`, and calls `INTC_D3D11_XessCreateContext` and `INTC_D3D11_XessExecute` in the Intel driver's `igdext64.dll`. That driver component does not exist in a Wine prefix, so the native D3D11 path would not work under Proton even on an Arc GPU.

The cross-vendor `libxess.dll` delay-imports `d3d12.dll` and carries DXIL kernels that need Shader Model 6.4, which a D3D11 device cannot create. The kernels cannot be recompiled to SM5 the way the community FSR 2 DX11 port recompiled FSR's HLSL, because the SDK is binary-only under the Intel Simplified Software License, which permits binary redistribution but forbids reverse engineering, decompilation, and modification.

## The D3D12 path is reachable

OptiScaler runs XeSS (and other D3D12-only upscalers) for D3D11 games through a background D3D12 device over D3D11on12, documenting a penalty of up to about 10% (15% in its wiki). Proton does not block it: DXVK carries a workaround for XeSS's DP4a path on non-Intel GPUs in `src/dxgi/dxgi_options.cpp`, and GE-Proton and Proton-CachyOS integrate XeSS upgrades and OptiScaler. The jc3vr wine prefix already provisions the native `d3dcompiler_47.dll` that covers the one Linux shader-compilation problem OptiScaler reports.

OptiScaler's penalty comes from GPU-bound flatscreen titles. This frame is largely bound by draw submission ([performance.md](../performance/performance.md)), and part of the interop's cost is CPU-side submission, so the penalty here is unknown in either direction. It is the first number to measure if the question is reopened.

## XeSS's advantage: sub-rectangle inputs

Single-pass stereo renders both eyes into one double-wide target. FSR 2's dispatch (`FfxFsr2DispatchDescription`) takes a `renderSize` and no offsets, so its inputs must start at (0, 0), and the per-eye FSR path copies each eye's colour, depth, and velocity into eye-sized textures and copies the result back ([fsr.md, "Under single-pass stereo"](fsr.md#under-single-pass-stereo)). XeSS's execute parameters, in both the D3D11 and D3D12 headers since version 1.0.0, carry `inputColorBase`, `inputMotionVectorBase`, `inputDepthBase`, `inputResponsiveMaskBase`, and `outputColorBase` offsets, so two contexts can resolve the two halves of a double-wide target in place. PureDark's Skyrim VR upscaler does exactly that on a D3D11 side-by-side target. Two contexts are needed either way, since XeSS keeps one temporal history per context; the SDK is single-thread-affine, which suits the render thread. XeSS frame generation does not support VR, and an OpenXR compositor owns reprojection regardless.

## Quality and cost evidence

Independent per-game comparisons place XeSS between DLSS and FSR 2 on quality, softer than DLSS and more temporally stable than FSR 2, but not uniformly: TechPowerUp's series found XeSS 1.3 DP4a free of ghosting and shimmer in one title, with more vegetation shimmer than the alternatives in another, and the most smearing on small flying objects in a third. Vegetation shimmer and small-mover smearing are this game's content. XeSS 1.3 also remapped its presets to different scale factors, so same-preset comparisons across versions are not like for like. All of this evidence is flatscreen, and none of it isolates what VR punishes: disocclusion under head rotation, per-eye consistency, and convergence after a history reset. The author of the Skyrim VR upscaler reportedly advises against XeSS in VR, for an undocumented reason.

On cost, TechSpot and Hardware Unboxed found XeSS 1.0 on an RTX 3060 about one quality tier more expensive than DLSS and FSR 2, and TechPowerUp's aggregate puts it 10 to 13% behind them on the DP4a kernel; both predate XeSS 1.3. No credible per-pass timing of XeSS DP4a on NVIDIA hardware exists at any resolution, so it would have to be measured here. On non-Intel GPUs XeSS runs its DP4a network, a different, lighter model than the XMX one.

## What adoption would require

XeSS takes FSR 2's input set: colour, velocity, depth, an optional exposure scale, an optional responsive mask, the jitter, and a reset flag. The velocity decode (`fsr/velocity_decode.rs`) would be rebuilt against XeSS's conventions (motion in pixels, `XESS_INIT_FLAG_JITTERED_MV`, `XESS_INIT_FLAG_USE_NDC_VELOCITY`, `XESS_INIT_FLAG_INVERTED_DEPTH`). XeSS needs no near plane, far plane, or field of view, which removes FSR's single-scalar field-of-view approximation for asymmetric eye frusta. Against that:

- XeSS is tuned for HDR colour before the tonemap; `XESS_INIT_FLAG_LDR_INPUT_COLOR` exists with a quality warning. The mod dispatches after the tonemap ([fsr.md, "Where it dispatches"](fsr.md#where-it-dispatches)), so XeSS would want a new dispatch point.
- Upscaling with it needs a negative mip bias on the engine's samplers, which the mod has no plumbing for.
- XeSS supplies no jitter sequence, and the sequence length must grow with the square of the scale factor.
- Its longer history makes each reset (a recenter, a fast travel, a loading screen) more visible.

Shipping it means a large proprietary binary beside the mod, a second graphics device, cross-API resource sharing, and a second submission timeline to synchronize every frame, where FSR 2 is MIT source compiled into `fsr-sys`.
