# JC3 shaders: extraction, disassembly, and interpretation

How to get at Just Cause 3's compiled shaders, turn them into readable assembly, and find a specific one, and what some of them do. The tooling is in [`tools/shaders/`](../../../tools/shaders/).

## Where the shaders live

The install root (next to `JustCause3.exe`) holds four shader bundles:

| Bundle | Selected when |
|---|---|
| `Shaders_F.shader_bundle` | normal shadow quality |
| `ShadersLowShadows_F.shader_bundle` | low shadow quality |
| `ShadersConstMath_F.shader_bundle` | Intel GPU (`VendorId 0x8086`), normal shadows |
| `ShadersConstMathLowShadows_F.shader_bundle` | Intel GPU, low shadows |

Each is an ADF container (Avalanche Data Format, magic `' FDA'`, `41 44 46 20` little-endian) packing one DXBC blob per shader permutation. `CSettingsManager::UpdateSettings` picks the bundle name (`"Shaders"`, `"ShadersLowShadows"`, `"ShadersConstMath"`, or `"ShadersConstMathLowShadows"`; `_F.shader_bundle` is appended internally) by shadow quality and GPU vendor and calls `GraphicsEngine::LoadShaderBundle`.

## Extracting the DXBC

`extract_dxbc.py` scans for the `DXBC` magic and uses each container's total-size field (a u32 at `+0x18`), so it needs no ADF parsing:

```sh
python3 tools/shaders/extract_dxbc.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
# -> Shaders_F.shaders/sh_0000_0003dac0.dxbc ...   (sh_<index>_<byte-offset-in-bundle>.dxbc)
```

The tooling does not repack bundles; bytecode can be substituted in memory at creation ([how the engine creates shaders](#how-the-engine-creates-shaders)).

## Naming the blobs

The bundle carries a name table. `shader_names.py` reads it and prints one row per blob, with the same index and offset as the carved files (`--json` adds the name and data hashes):

```sh
python3 tools/shaders/shader_names.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
# 1551/1551 named via the name-table            (on stderr)
# 0000 00004810 vertex   2dtex1
# 1512 00a8d600 domain   nvwaterhighend
```

`Shaders_F` and `ShadersLowShadows_F` hold 1551 blobs and the two `ConstMath` bundles 1552, all named.

### The name table

The ADF header lists one instance, `ShaderLibrary`. Every pointer inside an ADF instance is relative to that instance's own offset, not to the file start. The file's typedef table gives both structs, in field order:

```
ShaderLibrary                      Shader
  Name*                              NameHash   u32
  BuildTime*                         Name*
  VertexShaders    {ptr,count}       DataHash   u32
  FragmentShaders  {ptr,count}       BinaryData {ptr,count}   -> the DXBC container
  ComputeShaders   {ptr,count}
  GeometryShaders  {ptr,count}
  HullShaders      {ptr,count}
  DomainShaders    {ptr,count}
```

A shader's array gives its program type, which agrees with its `SHEX` chunk for all 1551. `Shaders_F` has 455 vertex, 961 fragment, 65 compute, 10 geometry, 21 hull, and 39 domain shaders. Names are not unique (1551 shaders share 1326 names; `nvwaterhighend` is a vertex, fragment, hull, and domain shader), so key on `(type, name)`.

### Fallback: the backwards scan

A blob's name usually sits as null-padded ASCII just before its `DXBC` magic. `shader_names.py --scan` walks back from the magic, and runs automatically, with a warning, when the ADF header does not parse. It is a last resort: on `Shaders_F` it names 1330 of 1551, leaves 221 blank (whole runs of hull and domain shaders, the WaveWorks ones among them), and misreads a stray byte as the name for 4 more (`lightglow` as `B`, `nvwaterhighend` as `2`). A shader that looks nameless is named in the table.

## Disassembling a blob

`dxbc.sh disasm` cross-builds the `dxbc-tool` crate (a `D3DDisassemble` wrapper over the `windows` crate) with cargo-xwin and runs it under wine against the `d3dcompiler_47.dll` that `shadergen` provisions (prerequisites in [`tools/shaders/README.md`](../../../tools/shaders/README.md)):

```sh
./scripts/dxbc.sh disasm Shaders_F.shaders/sh_0467_0016b270.dxbc | less
```

The output is FXC-style SM5 assembly, a commented reflection header (cbuffers, resource bindings, signatures) then the instructions, with CRLF line endings from wine (use `grep -a` or `sed 's/\r$//'`).

## Reading the disassembly

The header is the fast way in. A sun-lit opaque material shader declares:

```
// DiffuseMap / NormalMap / PropertiesMap      texture     t0..t2     // material
// ShadowMapTexture            texture  2darray   t14                 // the cascaded shadow map
// ShadowComparisonFilter      sampler_c          s15                 // PCF compare sampler
// CloudShadowMap / HorizonMap0 / HorizonMap1                         // other occlusion terms
// GlobalConstants / LightingFrameConsts        cbuffer  cb0/cb3
// Input signature: SV_Position, TEXCOORD 0..4                       // world pos and TBN in TEXCOORDs
```

A fullscreen pass has only `SV_Position` as input and binds G-buffers and depth; a forward material has `TEXCOORD` inputs and material textures. Shadow resolves bind `ShadowMapTexture` (`t14`) and `ShadowComparisonFilter` (`s15`) and emit `sample_c_lz_indexable ... t14 ... s15` taps. `v0.xy` is the screen pixel, so math seeded from it changes with the view, not the surface. Resource names match a shader to its pass: SSAO binds `DeinterleavedDepthTexture`, motion blur `VelocityTexture` and `NeighborMaxTexture`.

## Finding a specific shader

Resource and cbuffer names sit in the `RDEF` chunk as ASCII, so the raw blobs can be grepped:

```sh
grep -la ShadowMapTexture Shaders_F.shaders/*.dxbc | wc -l        # the shadow-map readers
cat Shaders_F.shaders/*.dxbc | strings -n6 | grep -iE 'shadow|ssao|ambient|noise|jitter' | sort | uniq -c
```

Then disassemble the candidates and confirm by their instructions. The pass and effect names are in [`rendering.md`](rendering.md).

## Pixel shaders that read the camera position

The camera world position is in rows 4 and 5 of the fragment `GlobalConstants` (`cb0`), staged in [`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData) ([`lighting-shadow-pipeline.md`](lighting-shadow-pipeline.md) §5.3). In `Shaders_F`, 432 of the 961 pixel shaders read one or both rows (189 row 4, 358 row 5). Sixteen read row 6, all also reading row 4: the underwater composite permutations and a terrain reflection prototype. They use it for:

- The view vector, from the interpolated world position, for specular, Fresnel, and environment terms. Some fold a view-dependent term into the G-buffer: the `character` material scales its albedo by a cloth-sheen term, `pow(1 - |N·V|, k)` faded in by camera distance, so its G-buffer albedo depends on the viewpoint.
- The forward-lit families' sun-shadow cascade lookup (`charactergear*`, `characterskin*`, `particleeffect*`, `generalmkiii*`, the water families, and others): 149 shaders form `M · (world − camera) + T`, `M` in rows 45 to 47 and `T` in row 48, which is baked relative to the camera position.
- Camera distances and heights, for distance fades and the camera-below-water tests.
- World reconstruction in the fullscreen passes: the deferred resolve rebuilds a pixel's world position as the camera position plus the interpolated view ray scaled by depth.

No pixel shader reads row 48 without the camera rows. 113 index `cb0` dynamically, all from a base past row 48: the per-cascade parameters at row 51 and the shadow matrices at rows 63 to 66. The vehicle paint (`carpaintmm`) reads nothing from `cb0` and writes a view-independent G-buffer; its reflections and specular come from the deferred resolve and SSR.

The game's pixel shaders declare constant-buffer slots 0 to 4, 6 to 9, and 12; none declares 5, 10, 11, or 13. Of the 432 camera readers, 183 declare `SV_Position` with its `.x`, 8 without `.x`, and 241 carry it in the input signature only.

## How the engine creates shaders

[`Graphics::CreateFragmentProgram(device, params)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CreateFragmentProgram) is the only caller of `ID3D11Device::CreatePixelShader`. `params` ([`CreateFragmentProgramParams`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw::CreateFragmentProgramParams)) is the DXBC bytecode and its length, and the returned `HFragmentProgram_t` holds only the `ID3D11PixelShader*`; no DXBC is retained, and since `CreatePixelShader` copies the bytecode, the buffer only has to outlive the call.

[`GraphicsEngine::LoadShaderBundle(name)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-LoadShaderBundle) reloads a bundle and re-creates every shader holder through `CreateFragmentProgram`, but only if `name` differs from [`GraphicsEngine::m_CurrentBundleName`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CurrentBundleName). Changing the shadow quality in the graphics menu selects the other bundle of the pair, so it re-creates every shader this way.

### The DXBC container checksum

A DXBC container stores a checksum at offset `0x4`, a modified MD5 over everything past the 20-byte header. The D3D stack under Proton validates it at creation and does not create a blob whose hash mismatches, and `D3DDisassemble` rejects one too (`hr=0x80004005`). Any bytecode edit, in memory or on disk, needs the checksum recomputed.

## The screen-pixel PCF rotation hash in the sun-shadow resolve

The opaque sun-shadow resolve (about 159 material shaders binding `ShadowMapTexture`) rotates its 38-tap Poisson PCF disk by a hash of the screen pixel:

```
dp2     r#.w, v0.xyxx, l(12.989800, 78.233002, 0, 0)   ; v0 = SV_Position (screen pixel)
sincos  r#.w, null, r#.w
mul     r#.w, r#.w, l(43758.546875)
frc     r#.w, r#.w                                       ; fract(sin(dot(pixel, k)) * 43758.5)
sincos  rot_sin, rot_cos, r#.w                           ; -> per-pixel PCF disk rotation
```

The lookup itself uses the interpolated world position, so the rotation changes only the per-pixel tap noise, which is fixed to the screen, not the shadow's position or mean coverage. `12.9898` occurs only in this instruction (159 sites per bundle), an exact fingerprint; the `dp2` immediate encodes as `39 d6 4f 41 4c 77 9c 42 00 00 00 00 00 00 00 00`.

## The screen-door hash

The alpha-tested and fading materials discard against a threshold hashed from the screen pixel:

```
dp2         r#.x, v0.xyxx, l(0.467944, -0.703648, 0, 0)   ; v0 = SV_Position (screen pixel)
frc         r#.x, r#.x                                      ; the threshold, in [0, 1)
add         r#.x, -r#.x, v6.w                               ; the coverage: a distance fade, a LOD crossfade
lt          r#.x, r#.x, l(0)
discard_nz  r#.x
```

It is the only other `dp2` of `SV_Position` with an immediate vector in `Shaders_F` (encoded `5b 96 ef 3e 46 22 34 bf`). 101 pixel shaders build it from `SV_Position`: the `general*` families (the `general_lod*` crossfades, the `generalmkiii*` alpha tests and their shadow and reflective-shadow permutations), `vegetationbark*` and `vegetationfoliage*`, `prop`, `propdecal`, `buildingjc3`, `landmark`, the `carpaintmm*_stipple` and `carpaintmmshadow` stipples, `tex0shadowstipple`, `rendervelocitybufferstipple`, the volumetric fog's `fogvolumeapply*` (which dithers its march by it), and the four `ssao*` permutations (the generation pass rotates its spiral by it). The 18 `treeimpostor*` permutations build it from their projected position over twice the inverse screen size instead. Neither multiplier is a whole number of cycles per pixel, so the threshold field does not repeat under any shift of the pixel grid: a surface drawn at another offset in its target keeps a different set of pixels.

## The velocity pass's motion vectors

The velocity buffer (`m_VelocityBufferTexture`) is written only by `RP_Z_AND_VELOCITY_PASS` draws, through the render setup that binds it. Twelve pixel shaders in `Shaders_F` write it, all encoding alike from two clip positions the vertex shader passes: the `rendervelocitybuffer` family (`rendervelocitybuffer`, `…clamp`, `…notex`, `…notexclamp`, `…opacity`, `…stipple`), `renderfadingvelocitybuffer`, `tex0velocity`, `vegetationbarkvelocity`, `vegetationfoliagevelocity`, and `generalmkiiiprezvelocity` with its `alphatest` permutation:

```
mov r0.xy, v2.xyxx                    ; the previous clip position
mov r0.zw, v1.xxxy                    ; the current clip position
mov r1.x, v2.w
mov r1.z, v1.w
div r0.xyzw, r0.xyzw, r1.xxzz         ; both, in NDC
mad r0.xyzw, r0.xyzw, l(0.5, -0.5, 0.5, -0.5), l(0.5, 0.5, 0.5, 0.5)
add r0.xy, -r0.xyxx, r0.zwzz          ; current UV less previous UV
mul r0.xy, r0.xyxx, l(8, 8, 0, 0)
max/min r0.xy, ±1                     ; clamped to an eighth of the screen per frame
mad o0.xy, r0.xyxx, l(0.5, 0.5, 0, 0), l(0.5, 0.5, 0, 0)
mul o0.z, v1.w, l(0.01)               ; the current clip w, scaled
```

The motion is in screen UV and carries no target size. The two `clamp` permutations first clamp the UV motion to a constant; the others differ only in the discard before the encoding.

In every velocity vertex shader (`generalmkiiiprezvelocity*`, `renderprezvelocity*`, `generalprezvelocity`, `carpaintmmprezvelocity*`, `charactervelocity*`, `characterskinvelocity*`, and `vegetation*velocity*`) the current clip position copies the `SV_Position` output's `x`, `y`, and `w` (only `z` takes a depth bias), so its NDC is the pixel's own. The previous position comes one of three ways:

- `generalmkiiiprezvelocity` and its non-instanced permutations: the previous world matrix (`ModelWorldMatrixPrev` in `cb12`) times the previous view-projection in the vertex `GlobalConstants` at `cb0[33..36]`.
- The hardware-instanced `generalmkiiiprezvelocityhwinstanced*`: the previous camera-relative view-projection at `cb0[44..47]` with the previous camera position at `cb0[48]`.
- Every other set: a previous world-view-projection the block bakes on the CPU into its own constant buffer (`PrevWorldViewProj` in the bark and foliage `PerInstanceConstants`, `PreviousWorldViewProjection` in the `renderprezvelocity` and `generalprezvelocity` `$Globals`), from the render context's previous camera-relative view-projection and camera position.

All three trace to the render camera's previous view and projection: [`RenderEngine::SetGlobalShaderProgramCameraConstants`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SetGlobalShaderProgramCameraConstants) stages the render context's previous view-projection and previous camera-relative view-projection into [`m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData), and the render context copies them from the camera's [`m_PreviousViewProjectionF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::Camera##field-m_PreviousViewProjectionF) and previous view and projection. Only what is enqueued on the pass draws in it: in a draw census of the town and the meadow ([single-pass-validation.md](../../mod/stereo/single-pass-validation.md#the-draw-census)), the vegetation's bark and foliage and a few hardware-instanced `GeneralMkIII` draws, and no static models. No terrain block draws in the pass, and pixels no velocity draw covers keep the cleared value.
