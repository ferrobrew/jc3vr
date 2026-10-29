# Shader patching

The mod edits some of the game's shaders as the engine creates them. The edits apply in memory, to the bytecode passed to shader creation, and the `.shader_bundle` files on disk stay unmodified, so repacking a bundle is not needed and not implemented. The engine side is in [`shaders.md`](../../engine/rendering/shaders.md): the creation path, the reload condition of `LoadShaderBundle`, and the DXBC checksum requirement.

## The create hooks

`payload/src/hooks/graphics_engine/shader.rs` detours the four `Graphics::Create*Program` functions. Each hook reads the DXBC from the params (`m_Code`, `m_Size`), builds a patched copy when a patch applies, points the params at the copy for the call to the original, and restores the caller's pointer and size afterwards. The D3D `Create*Shader` call copies the bytecode ([shaders.md, "How the engine creates shaders"](../../engine/rendering/shaders.md#how-the-engine-creates-shaders)), so the copy only has to outlive that call. `m_Size` moves with `m_Code` because some rewrites grow the blob. A stale size would hand D3D a container whose chunk table runs past the declared length.

- `CreateFragmentProgram` applies the sun-shadow PCF hash patch and the LOD dissolve patch (both below), and the screen-space decal depth-UV bias of single-pass stereo ([single-pass-stereo.md, "2. One eye-0 reconstruction basis over the whole double-wide target"](../stereo/single-pass-stereo.md#2-one-eye-0-reconstruction-basis-over-the-whole-double-wide-target)).
- `CreateVertexProgram` applies the single-pass vertex-shader transform ([single-pass-stereo.md, "The vertex-shader transform"](../stereo/single-pass-stereo.md#the-vertex-shader-transform)).
- `CreateHullProgram` and `CreateDomainProgram` apply the single-pass terrain rewrites ([single-pass-stereo.md, "The terrain tessellation path"](../stereo/single-pass-stereo.md#the-terrain-tessellation-path)).

The two fragment patches are byte patches at fixed immediates, found by scanning the bytecode for a byte pattern. The single-pass rewrites are structural and live in the `dxbc-stereo` crate.

## The DXBC checksum

Every patch recomputes the container checksum before the create call, with `dxbc_stereo::refresh_checksum` (`dxbc-stereo/src/checksum.rs`). The D3D stack under Proton rejects a blob whose stored checksum does not match ([shaders.md, "The DXBC container checksum"](../../engine/rendering/shaders.md#the-dxbc-container-checksum)), so a patch that skips this step fails to create the shader. For the PCF patch the symptom is that the shadow shaders fail to create and the scene renders near-black with speckle. The structural rewrites reassemble the container and refresh the checksum themselves.

## Reloading the shaders

The hooks see a shader only when the engine creates it. With launch-time injection that is every shader. With mid-session injection the game has already built its shaders, so the patches apply only after a reload.

A reload bounces the active shader bundle. `bounce_shader_bundle` reads `GraphicsEngine::m_CurrentBundleName`, waits for the draw thread with `WaitForCPUDrawToFinish`, calls `LoadShaderBundle` with the other shadow-quality variant of the bundle, and then calls it again with the original name. `LoadShaderBundle` reloads only when the name differs from the current bundle, so a single call with the current name does nothing. The bounce re-creates every shader twice through the hooks. Between the two calls the single-pass census and the patched-vertex-shader set are reset, so both reflect the final pass only. `toggle_bundle` pairs `Shaders` with `ShadersLowShadows`, and `ShadersConstMath` with `ShadersConstMathLowShadows`.

Three triggers cause a reload:

- the "Reload shaders" button in the debug UI's Rendering page, in the Stereo feature (beside the count of patched shadow PCF sites) and in the single-pass section;
- F11, which also toggles `stereo.patch_shadow_pcf_hash`, for an in-headset comparison without the debug overlay;
- a change of shadow quality in the game's own graphics menu, which runs the same `LoadShaderBundle` path.

The button and F11 set a request flag. `process_reload_request` services it at the top of the next frame on the game thread, before the frame's draws.

The debug UI shows the number of patched PCF and dissolve sites next to each toggle. A count of zero means that no matching shader has been created since injection, and a reload is needed.

## Restoring the originals on eject

On eject, `restore_original_shaders_on_eject` bounces the bundle once more if any patch or substitution was made. `crate::is_shutting_down()` is already set, so every create hook passes the bytecode through unchanged, and the game keeps only its original shaders after the payload unloads. This runs on the game thread in `shutdown_from_game`, before the hooks are uninstalled.

## The sun-shadow PCF hash (issue #10)

In stereo, sun shadows and alpha-tested foliage shimmered and grained differently in each eye. The cause is the PCF disk rotation in the sun-shadow resolve, which is seeded from the screen pixel `v0.xy` ([shaders.md, "The screen-pixel PCF rotation hash in the sun-shadow resolve"](../../engine/rendering/shaders.md#the-screen-pixel-pcf-rotation-hash-in-the-sun-shadow-resolve)). The same world point lands on a different pixel in each eye, so each eye gets a different rotation and a different tap noise pattern. The shadow lookup uses the interpolated world position, so the shadow's position and mean coverage already match between the eyes. Only the noise differs. The same applies to any shader math seeded from `v0.xy`: it is screen-space, and it differs between the eyes.

`stereo.patch_shadow_pcf_hash` (on by default) zeroes the two seed constants `12.9898` and `78.233`, which are the first eight bytes of the `dp2` immediate `39 d6 4f 41 4c 77 9c 42 00 00 00 00 00 00 00 00`. The dot product becomes zero, the rotation becomes a constant, and both eyes sample the same unrotated 38-tap disk. With 38 taps the visual change is negligible. The constant occurs only in this instruction (159 sites per bundle), so the pattern scan matches nothing else.

The patch does not address shadows that look uniformly stronger or longer in one eye, because a rotated disk changes the noise and not the mean. That residual is view-dependence inherent to screen-space AO: each eye computes SSAO from its own depth, so near an occluder one eye sees more of it. It is not an accumulation bug. The sun-shadow map and its cascades are computed once per frame from the shared camera and are identical between the eyes, and the SSAO temporal history is handled per eye (`FORCE_SSAO_FIRST_PASS` and `RESTORE_SSAO_HISTORY` in [lighting-shadow-vr-interactions.md](../stereo/lighting-shadow-vr-interactions.md#saverestore-ledger--default-on-modifications)). The per-eye render-target hash diagnostic (`stereo.diagnose_rt_hashes`, `payload/src/debug/rt_hash.rs`) confirms that there is no cross-eye accumulator. AO shared between the eyes would require reprojecting one eye's result into the other.

## The jitter-unstable LOD dissolve

`PATCH_LOD_DISSOLVE` (off, in `payload/src/hooks/graphics_engine/shader.rs`) targets the material LOD-dissolve screen-door pattern, whose `dp2` immediate starts with `l(0.467944, -0.703648, ...)` (119 shaders per bundle). Most of these shaders key the pattern to `SV_Position`, which is stable from frame to frame. The vegetation family keys it to the interpolated clip-space position, which carries the FSR camera jitter, so the pattern slides by a sub-pixel amount every frame and mid-fade regions flip coverage together. The patch identifies that family by the operand type of the `dp2` source (a temp register rather than an input), and replaces the `1.0` immediate of the fade `add`, 64 bytes after the seed, with `-3.0e38`. The dissolve's discard then never fires, and LOD transitions pop instead of dissolving. The defect appears only with FSR jitter on, and it was not the cause of the issue #10 flicker, so the patch is off.
