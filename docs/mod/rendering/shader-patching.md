# Shader patching

The mod edits some of the game's shaders as the engine creates them. The edits apply in memory, to the bytecode passed to shader creation, and the `.shader_bundle` files on disk stay unmodified. The engine side (the creation path, the reload condition of `LoadShaderBundle`, and the DXBC checksum requirement) is in [`shaders.md`](../../engine/rendering/shaders.md).

## The create hooks

`crates/jc3vr-render/src/hooks/graphics_engine/shader.rs` detours the four `Graphics::Create*Program` functions. Each hook reads the DXBC from the params (`m_Code`, `m_Size`), builds a patched copy when a patch applies, points the params at the copy for the call to the original, and restores the caller's pointer and size afterwards. The D3D `Create*Shader` call copies the bytecode ([shaders.md](../../engine/rendering/shaders.md#how-the-engine-creates-shaders)), so the copy only has to outlive that call. `m_Size` moves with `m_Code` because some rewrites grow the blob.

- `CreateFragmentProgram` applies the sun-shadow PCF hash patch (below) and the single-pass pixel-shader rewrites ([single-pass-per-eye-inputs.md](../stereo/single-pass-per-eye-inputs.md)). Later rewrites read buffers that earlier ones declare, so they run in a fixed order, which the `shader.rs` module docs list.
- `CreateVertexProgram` applies the single-pass vertex-shader transform ([single-pass-stereo.md](../stereo/single-pass-stereo.md#the-vertex-shader-transform)).
- `CreateHullProgram` and `CreateDomainProgram` apply the single-pass terrain rewrites ([single-pass-stereo.md](../stereo/single-pass-stereo.md#the-terrain-tessellation-path)).

The PCF patch is a byte patch found by scanning for a byte pattern. The single-pass rewrites are structural and live in the `dxbc-stereo` crate. A structural rewrite either declines (another stage, an unrecognised shape, a slot already taken), and the hook leaves the shader alone or tries the next rewrite, or finds the bytecode malformed, which no runtime-compiled shader is, and the hook logs an error naming the rewrite and leaves the shader as compiled (`DxbcError::kind` tells the two apart). A corpus test checks that every shipped rewrite only declines the game's own shaders.

Every patch recomputes the container checksum (`dxbc_stereo::refresh_checksum`) before the create call, because the D3D stack under Proton rejects a blob whose checksum does not match ([shaders.md](../../engine/rendering/shaders.md#the-dxbc-container-checksum)). A PCF patch without it fails to create the shadow shaders, and the scene renders near-black with speckle.

## Reloading the shaders

The hooks see a shader only when the engine creates it: with launch-time injection that is every shader, and with mid-session injection the patches apply only after a reload.

`bounce_shader_bundle` reads `GraphicsEngine::m_CurrentBundleName`, waits for the draw thread with `WaitForCPUDrawToFinish`, and calls `LoadShaderBundle` with the other shadow-quality variant of the bundle (`toggle_bundle`), then again with the original name, since `LoadShaderBundle` reloads only when the name changes. Between the two calls the single-pass census and the patched-vertex-shader set are reset, so both reflect the final pass. A reload is requested by the debug UI's "Reload shaders" button (serviced by `process_reload_request` at the top of the next frame, on the game thread), by `reconcile_single_pass` when single-pass turns on or off, and by a change of shadow quality in the game's graphics menu. The Stereo feature shows the number of patched PCF sites; zero means no matching shader has been created since injection.

On eject, `restore_original_shaders_on_eject` bounces the bundle once more if anything was patched. `jc3vr_core::is_shutting_down()` is already set, so every hook passes the bytecode through and the game keeps only its original shaders. It runs on the game thread in `shutdown_from_game`, before the hooks are uninstalled.

## The sun-shadow PCF hash

The PCF disk rotation in the sun-shadow resolve is seeded from the screen pixel `v0.xy` ([shaders.md](../../engine/rendering/shaders.md#the-screen-pixel-pcf-rotation-hash-in-the-sun-shadow-resolve)). The same world point lands on a different pixel in each eye, so each eye gets a different tap noise, and sun shadows and alpha-tested foliage shimmer differently in each eye. The shadow's position and mean coverage already match, since the lookup uses the interpolated world position.

The pixel-shader hook zeroes the seed constants `12.9898` and `78.233`, the first eight bytes of the `dp2` immediate `39 d6 4f 41 4c 77 9c 42 00 00 00 00 00 00 00 00`, so the rotation is constant and both eyes sample the same unrotated 38-tap disk, with a negligible visual change. The constant occurs only in this instruction (159 sites per bundle), so the scan matches nothing else.

It does not address shading that is uniformly stronger in one eye. That is the view dependence of screen-space AO, computed from each eye's own depth, not an accumulation bug: the shadow cascades are computed once per frame from the shared camera, the SSAO history is handled per eye ([lighting-shadow-vr-interactions.md](../stereo/lighting-shadow-vr-interactions.md#save-and-restore-ledger)), and a per-eye render-target hash comparison with both eyes on one camera found no cross-eye accumulator.
