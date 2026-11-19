# JC3 shaders: extraction, disassembly, and interpretation

How to get at Just Cause 3's compiled shaders, turn them back into readable assembly, and find a
specific one. The tooling referenced here lives in
[`tools/shaders/`](../../../tools/shaders/).

## Where the shaders live

The game ships four shader bundles in its install root (next to `JustCause3.exe`):

| Bundle | Selected when |
|---|---|
| `Shaders_F.shader_bundle` | normal shadow quality |
| `ShadersLowShadows_F.shader_bundle` | low shadow quality |
| `ShadersConstMath_F.shader_bundle` | Intel GPU (`VendorId 0x8086`), normal shadows |
| `ShadersConstMathLowShadows_F.shader_bundle` | Intel GPU, low shadows |

Each is an **ADF container** (Avalanche Data Format — magic `' FDA'` / `41 44 46 20` little-endian)
that packs one **DXBC** blob per shader permutation (~1550 in `Shaders_F`). `CSettingsManager::UpdateSettings`
picks the bundle name (`"Shaders"`, `"ShadersLowShadows"`, `"ShadersConstMath"`,
`"ShadersConstMathLowShadows"`; the `_F.shader_bundle` suffix is added internally) by shadow quality and
GPU vendor, then calls `GraphicsEngine::LoadShaderBundle`.

## Extracting the DXBC

`extract_dxbc.py` scans for the `DXBC` container magic and uses each container's embedded total-size
field (a u32 at `+0x18`), so it doesn't need to parse the ADF structure:

```sh
python3 tools/shaders/extract_dxbc.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
# -> Shaders_F.shaders/sh_0000_0003dac0.dxbc ...   (sh_<index>_<byte-offset-in-bundle>.dxbc)
```

The tooling does not repack bundles. The engine's shader creation path, where bytecode can be
substituted in memory, is described in [how the engine creates shaders](#how-the-engine-creates-shaders).

## Naming the blobs

The bundle carries a name table, so nothing has to be guessed. `shader_names.py` reads it and prints
one row per blob, in the same order and with the same index and offset as the carved files:

```sh
python3 tools/shaders/shader_names.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
# 1551/1551 named via the name-table            (on stderr)
# 0000 00004810 vertex   2dtex1
# ...
# 1512 00a8d600 domain   nvwaterhighend
```

`--json` emits the same records (plus the name and data hashes) for scripting. Measured on the 2026
Steam build: 1551 blobs in `Shaders_F` and `ShadersLowShadows_F`, 1552 in the two `ConstMath` bundles,
all named.

### The name table

The ADF header lists a single instance, `ShaderLibrary`, at file offset `0x50`. **Every pointer inside
an ADF instance is relative to that instance's own offset**, not to the start of the file — the single
most common way to get lost here. The file's own typedef table spells out both structs:

```
ShaderLibrary (112 bytes)          Shader (40 bytes)
  +0   Name*                         +0   NameHash   u32 (8-byte slot)
  +8   BuildTime*                    +8   Name*
  +16  VertexShaders    {ptr,count}  +16  DataHash   u32 (8-byte slot)
  +32  FragmentShaders  {ptr,count}  +24  BinaryData {ptr,count}   -> the DXBC container
  +48  ComputeShaders   {ptr,count}
  +64  GeometryShaders  {ptr,count}
  +80  HullShaders      {ptr,count}
  +96  DomainShaders    {ptr,count}
```

Because a shader lives in the array for its stage, the table also hands you each blob's **program
type** for free (it agrees with the type in the blob's own `SHEX` chunk for all 1551). In `Shaders_F`:
455 vertex, 961 fragment, 65 compute, 10 geometry, 21 hull, 39 domain.

Names are **not** unique on their own — 1551 shaders share 1326 distinct names, e.g. `nvwaterhighend`
appears as a vertex, fragment, hull, and domain shader. The `(type, name)` pair is unique; key on that.

### Fallback: the backwards scan

Each blob's name usually sits as null-padded ASCII immediately *before* its `DXBC` magic, so walking
backwards from the magic recovers most names without touching the ADF structure. `shader_names.py
--scan` does this, and it runs automatically (with a warning naming the mode) when the ADF header does
not parse.

Treat it as a last resort. On `Shaders_F` it names **1330 of 1551**: it leaves 221 blank and reports a
stray printable byte as the name for 4 more (`lightglow` comes out as `B`, `nvwaterhighend` as `2`).
Its gaps are not random — whole runs of hull and domain shaders fall in them — so read from the scan, the
WaveWorks hull and domain shaders look "unnamed". They are named; the scan simply cannot see it. If a shader looks nameless, check the name table before
believing it.

## Disassembling a blob

`dxbc.sh disasm` cross-builds the `dxbc-tool` crate (a `D3DDisassemble` wrapper over the `windows`
crate) with cargo-xwin and runs it under wine against the `d3dcompiler_47.dll` that `shadergen`
provisions:

```sh
./scripts/dxbc.sh disasm Shaders_F.shaders/sh_0467_0016b270.dxbc | less
```

See [`tools/shaders/README.md`](../../../tools/shaders/README.md) for the one-time prerequisites. The output
is standard FXC-style SM5 assembly: a commented reflection header (cbuffers, resource bindings, the
input/output signatures) followed by the instruction stream.

Caveat: the tool runs under wine, so the disassembly text carries **CRLF** line endings; `grep`/`awk`
on the saved output want `grep -a` or a `sed 's/\r$//'` first.

## Reading the disassembly

The header is the fast way in. For example, a sun-lit opaque material shader declares:

```
// Resource Bindings:
// DiffuseMap / NormalMap / PropertiesMap      texture     t0..t2     // material
// ShadowMapTexture            texture  2darray   t14                 // the cascaded shadow map
// ShadowComparisonFilter      sampler_c          s15                 // PCF compare sampler
// CloudShadowMap / HorizonMap0 / HorizonMap1                         // other occlusion terms
// GlobalConstants / LightingFrameConsts        cbuffer  cb0/cb3
//
// Input signature:
// SV_Position  0  POS  ...   TEXCOORD 0..4 ...                       // world pos / TBN carried in TEXCOORDs
```

Then in the body, `dcl_*` declares the registers (`v0` = `SV_Position`, etc.), and the work follows.
Useful reflexes:

- **Identify the shader from its resources.** A fullscreen pass has `SV_Position`-only input and binds
  GBuffers + depth; a forward material shader has `TEXCOORD` inputs and material textures. Shadow
  resolves bind `ShadowMapTexture` (`t14`) + `ShadowComparisonFilter` (`s15`) and emit
  `sample_c_lz_indexable ... t14 ... s15` taps.
- **`v0.xy` is the screen pixel.** Any math seeded off `v0.xy` is screen-space. Its result depends
  on the pixel a surface point projects to, so it changes with the view, not with the surface.
- **Match the assembly to the engine pass** via the resource names: e.g. SSAO binds
  `DeinterleavedDepthTexture`; motion blur binds `VelocityTexture`/`NeighborMaxTexture`.

## Finding a specific shader

Resource and cbuffer names live in the DXBC `RDEF` chunk as ASCII, so you can grep the raw blobs
without disassembling everything:

```sh
# which blobs sample the cascaded shadow map?
grep -la ShadowMapTexture Shaders_F.shaders/*.dxbc | wc -l        # ~167

# distinct occlusion/AO/noise resource names across the bundle
cat Shaders_F.shaders/*.dxbc | strings -n6 | grep -iE 'shadow|ssao|ambient|noise|jitter' | sort | uniq -c
```

Then disassemble the candidates and confirm by their instructions. The engine-side names (what each
pass/effect is called, what reads what) are in [`rendering.md`](rendering.md); follow a resource name
there to the pass that binds it.

## Pixel shaders that read the camera position

The camera world position sits in rows 4 and 5 of the fragment `GlobalConstants` (`cb0`), staged with the rest of the block in [`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData) (see [`lighting-shadow-pipeline.md`](lighting-shadow-pipeline.md) §5.3). In `Shaders_F`, 432 of the 961 pixel shaders read one or both rows: 189 read row 4 and 358 read row 5. Row 6 is read by 16, all of which also read row 4: the underwater composite permutations and a terrain reflection prototype.

What they use it for, by family:

- **The view vector, from the interpolated world position.** The material shaders carry the world position in a `TEXCOORD` and subtract the camera from it for their specular, Fresnel, and environment terms. Some fold a view-dependent term into what they write to the G-buffer rather than leaving it to the lighting: the `character` material darkens or brightens its albedo by a cloth-sheen term, `pow(1 - |N·V|, k)` faded in by camera distance, so its G-buffer albedo depends on where it is seen from.
- **The sun-shadow cascade lookup** of the forward-lit families (the `charactergear*`, `characterskin*`, `particleeffect*`, `generalmkiii*`, and water families, among others): 149 shaders form `M · (world − camera) + T`, with `M` in rows 45 to 47 and the translation `T` in row 48. `T` is baked relative to the camera position, so the two only make sense together.
- **Camera distances and heights**, for distance fades and for the camera-below-water tests.
- **World reconstruction** in the fullscreen passes: the deferred resolve rebuilds a pixel's world position as the camera position plus the interpolated view ray scaled by depth.

None of the pixel shaders reads row 48 without reading the camera rows. 113 index `cb0` dynamically, every one of them from an array base past row 48: the per-cascade parameters at row 51 and the shadow matrices at rows 63 to 66.

Not every view-dependent look comes from a pixel shader's camera read. The vehicle paint material (`carpaintmm`) reads nothing from `cb0` and writes a view-independent G-buffer; its reflections and specular come from the deferred resolve and the screen-space reflections.

The pixel stage's constant-buffer slots the game's pixel shaders declare are 0 to 4, 6 to 9, and 12; slots 5, 10, 11, and 13 are declared by none. Of the 432 camera readers, 183 declare `SV_Position` with its `.x`, 8 declare it without `.x`, and 241 carry it in the input signature only, undeclared.

## How the engine creates shaders

[`Graphics::CreateFragmentProgram(device, params)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CreateFragmentProgram) is the only caller of
`ID3D11Device::CreatePixelShader`. `params` ([`CreateFragmentProgramParams`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw::CreateFragmentProgramParams)) is
just the DXBC bytecode and its length. The returned `HFragmentProgram_t` holds only the
`ID3D11PixelShader*`; **no DXBC is retained**. `CreatePixelShader` copies the bytecode, so the buffer
`params` points at only has to outlive the call.

[`GraphicsEngine::LoadShaderBundle(name)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-LoadShaderBundle) reloads a bundle and re-creates every shader holder through
`CreateFragmentProgram`, but only if `name` differs from the current bundle name
([`GraphicsEngine::m_CurrentBundleName`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CurrentBundleName)). A change of shadow quality in the game's graphics
menu selects the other bundle of the pair (see [where the shaders live](#where-the-shaders-live)), so
it re-creates every shader through this path.

### The DXBC container checksum

A DXBC container stores a checksum at offset `0x4`: a modified MD5 over everything past the 20-byte
header. The D3D stack under Proton validates it at shader creation and rejects a blob whose stored hash
does not match, so the shader is not created. `D3DDisassemble` also rejects such a blob
(`hr=0x80004005`). Any edit to the bytecode, in memory or on disk, requires recomputing the checksum
and writing it back.

## The screen-pixel PCF rotation hash in the sun-shadow resolve

The opaque sun-shadow resolve (the ~159 material shaders that bind `ShadowMapTexture`) rotates its
38-tap Poisson PCF disk by a hash of the screen pixel:

```
dp2     r#.w, v0.xyxx, l(12.989800, 78.233002, 0, 0)   ; v0 = SV_Position (screen pixel)
sincos  r#.w, null, r#.w
mul     r#.w, r#.w, l(43758.546875)
frc     r#.w, r#.w                                       ; fract(sin(dot(pixel, k)) * 43758.5)
sincos  rot_sin, rot_cos, r#.w                           ; -> per-pixel PCF disk rotation
```

The shadow lookup itself uses the interpolated world position. The rotation therefore changes only the
per-pixel tap noise, not the shadow's position or mean coverage, and the noise pattern is fixed to the
screen rather than to the surface. The constant `12.9898` occurs **only** in this instruction (159 sites
per bundle), so it is an exact fingerprint. The `dp2` immediate encodes as the 16 bytes
`39 d6 4f 41 4c 77 9c 42 00 00 00 00 00 00 00 00`.
