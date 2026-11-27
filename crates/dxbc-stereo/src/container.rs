//! The DXBC container: magic, checksum, and the chunk table.

use std::fmt;

/// A parsed DXBC container -- its chunk table indexed by four-character tag.
pub struct Dxbc {
    chunks: Vec<Chunk>,
}

/// One chunk in a DXBC container.
#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    /// The four-character chunk tag (`SHEX`, `ISGN`, `OSG5`, ...).
    pub tag: [u8; 4],
    /// The byte offset of the chunk body (past the 8-byte tag+size header) within the container.
    pub body_offset: usize,
    /// The chunk body length in bytes.
    pub body_len: usize,
}

impl Chunk {
    /// The chunk body as a slice of the container.
    pub fn body<'a>(&self, blob: &'a [u8]) -> &'a [u8] {
        &blob[self.body_offset..self.body_offset + self.body_len]
    }
}

impl Dxbc {
    /// Parses a DXBC container's header and chunk table. Does not parse chunk contents.
    pub fn parse(blob: &[u8]) -> Result<Dxbc, DxbcError> {
        if blob.len() < 0x20 || &blob[..4] != b"DXBC" {
            return Err(DxbcError::NotDxbc);
        }
        let chunk_count = read_u32(blob, 0x1C)? as usize;
        let table_end = 0x20 + chunk_count * 4;
        if blob.len() < table_end {
            return Err(DxbcError::Truncated);
        }
        let mut chunks = Vec::with_capacity(chunk_count);
        for i in 0..chunk_count {
            let off = read_u32(blob, 0x20 + i * 4)? as usize;
            if off + 8 > blob.len() {
                return Err(DxbcError::Truncated);
            }
            let tag = [blob[off], blob[off + 1], blob[off + 2], blob[off + 3]];
            let body_len = read_u32(blob, off + 4)? as usize;
            let body_offset = off + 8;
            if body_offset + body_len > blob.len() {
                return Err(DxbcError::Truncated);
            }
            chunks.push(Chunk {
                tag,
                body_offset,
                body_len,
            });
        }
        Ok(Dxbc { chunks })
    }

    /// The chunks in container order.
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// The chunk with the given tag, if present.
    pub fn chunk(&self, tag: &[u8; 4]) -> Option<Chunk> {
        self.chunks.iter().copied().find(|c| &c.tag == tag)
    }

    /// The shader bytecode chunk (`SHEX` for SM5, `SHDR` for older), if present.
    pub fn shader_chunk(&self) -> Option<Chunk> {
        self.chunk(b"SHEX").or_else(|| self.chunk(b"SHDR"))
    }
}

/// Why a parse or a rewrite failed. [`DxbcError::kind`] says whether the bytecode is malformed or the
/// rewrite does not apply to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxbcError {
    /// The blob does not start with the `DXBC` magic.
    NotDxbc,
    /// The blob is shorter than its header/chunk table declare.
    Truncated,
    /// The container has no `SHEX`/`SHDR` shader chunk.
    NoShaderChunk,
    /// An instruction's declared length was zero, which would loop the token walk forever.
    ZeroLengthInstruction,
    /// The token stream ended inside an instruction or operand.
    UnexpectedEndOfTokens,
    /// The shader is not a vertex shader, so the stereo rewrite does not apply.
    NotVertexShader,
    /// The shader is not a domain shader, so the domain reprojection does not apply.
    NotDomainShader,
    /// The shader is not a hull shader, so the hull eye-forward does not apply.
    NotHullShader,
    /// The shader is SM4 (`SHDR`); the rewrite emits SM5-era structures (`SFI0`, the viewport
    /// output) and only supports `SHEX` shaders.
    UnsupportedShaderModel,
    /// The container has no `ISGN` input-signature chunk to extend.
    MissingInputSignature,
    /// The container has no `OSGN` output-signature chunk to extend.
    MissingOutputSignature,
    /// A signature chunk's header or element table does not match the expected `ISGN`/`OSGN`
    /// 24-byte-element layout.
    MalformedSignature,
    /// The shader has no `cb0` per-eye operands, so there is nothing to rewrite; the caller should
    /// leave it double-drawn.
    NoPerEyeReferences,
    /// The shader already declares `cb13`, the slot the stereo constants need.
    Cb13AlreadyDeclared,
    /// The shader already declares an `SV_InstanceID` input, which the rewrite asked for cannot
    /// take: it appends one of its own.
    InstanceIdAlreadyDeclared,
    /// The shader declares no `SV_InstanceID` input, which the instanced reprojection halves.
    NoInstanceId,
    /// A per-eye operand uses an index encoding the rewrite does not support (for instance a 64-bit
    /// or already-relative element index).
    UnsupportedOperandEncoding,
    /// Rewriting an instruction would push its length past the 7-bit instruction-length field.
    InstructionTooLong,
    /// The reprojection rewrite found no `SV_Position` output to reproject (no `dcl_output_siv
    /// position`), so there is no clip position to post-multiply.
    NoPositionOutput,
    /// The reprojection rewrite hit a control-flow shape it does not support -- a conditional early
    /// return (`retc`) would need the per-eye epilogue on that path too.
    UnsupportedControlFlow,
    /// The terrain eye-lane rewrite found no free `TEXCOORD3.z` output lane to carry the eye index
    /// through the VS -> HS -> DS pipeline (the `TEXCOORD3` output is missing, or its `.z` is used).
    NoEyeLane,
    /// The domain shader reads no offset view-projection row of its terrain constants (`cb1`), so
    /// there are no camera rows to take from an eye's slot.
    NoTerrainCameraRows,
    /// The shader reads no row of the full view-projection (`cb0[0..3]`), so there is nothing to take
    /// from an eye's slot.
    NoFullViewProjection,
    /// The pixel shader carries no screen-space depth fetch built from a projective UV, so there is
    /// nothing for the decal depth-UV bias to separate; the caller should leave it untouched.
    NoDepthUvFetch,
    /// The pixel shader is not one the capture harness's resolve, reflection-ray, impostor, bark,
    /// fog, depth-of-field, or anti-aliasing trial was written against.
    #[cfg(feature = "capture-rewrites")]
    NoResolveInputs,
    /// The pixel shader is not the lake permutation the capture harness's lake traces were written
    /// against: it holds no single planar reflection sample in the shape they recognise.
    #[cfg(feature = "capture-rewrites")]
    NoLakeReflection,
    /// The pixel shader holds no single lake refraction displacement scale in the shape the packed
    /// refraction rewrite recognises; the caller should leave it untouched.
    NoLakeRefraction,
    /// The pixel shader lays no screen-space overlay over the frame in a shape the overlay rewrite
    /// recognises; the caller should leave it untouched.
    NoScreenOverlay,
    /// The pixel shader binds no planar water reflection, or forms its coordinate in a shape the
    /// planar-reflection rewrite does not recognise; the caller should leave it untouched.
    NoPlanarReflection,
    /// The pixel shader is not the ambient-occlusion generation pass (it binds no packed linear depth
    /// and normal buffer), or reconstructs and samples in a shape the ambient-occlusion rewrite does
    /// not recognise; the caller should leave it untouched.
    NoAmbientOcclusion,
    /// The pixel shader is not the ambient-occlusion volume pass (it binds neither its depth textures
    /// nor its screen-to-box matrix), or reads them in a shape the volume rewrite does not
    /// recognise; the caller should leave it untouched.
    NoAmbientOcclusionVolume,
    /// The pixel shader is not a screen-space reflection ray march (it binds no linear depth, previous
    /// scene, and reflection constants), or reads its view ray and constants in a shape the reflection
    /// rewrite does not recognise; the caller should leave it untouched.
    NoScreenSpaceReflection,
    /// The pixel shader is not the screen-space reflection blur (it binds no reflection colour and
    /// alpha), or reads them in a shape the blur rewrite does not recognise; the caller should leave
    /// it untouched.
    NoReflectionBlur,
    /// The pixel shader is not one of the post chain's generic filters (it binds a texture under a
    /// name the filters do not use, or binds none), or reads a texture in a shape the post-filter
    /// rewrite does not recognise; the caller should leave it untouched.
    NoPostFilter,
    /// The pixel shader binds neither of the composite's bloom textures, or reads them in a shape the
    /// composite-bloom rewrite does not recognise; the caller should leave it untouched.
    NoCompositeBloom,
    /// The pixel shader reads none of the camera rows of the fragment `GlobalConstants`, so there is
    /// nothing for the eye-camera rewrite to localize; the caller should leave it untouched.
    NoCameraPosition,
    /// The pixel shader has no `SV_Position` input to select its eye by.
    NoPixelPosition,
    /// The pixel shader already declares the constant-buffer slot the eye-camera rewrite needs.
    EyeCameraSlotTaken,
    /// The pixel shader indexes `cb0` dynamically from a base that could reach a camera row.
    DynamicCameraAccess,
    /// The pixel shader builds no screen-door pattern from its pixel position (no `dp2` of the
    /// `SV_Position` input's `.xy` with an immediate vector), so there is nothing for the screen-door
    /// rewrite to localize; the caller should leave it untouched.
    NoScreenDoor,
    /// The pixel shader declares the eye-camera constant-buffer slot smaller than the screen-door
    /// rewrite reads it, so the slot is not the eye-camera buffer.
    ScreenDoorSlotTaken,
    /// The pixel shader writes no motion vector in the engine's encoding (the current and previous
    /// clip positions divided, mapped to UV, and subtracted), so there is nothing for the velocity
    /// rewrite to localize; the caller should leave it untouched.
    NoVelocity,
    /// The pixel shader declares the eye-camera constant-buffer slot smaller than the velocity
    /// rewrite reads it, so the slot is not the eye-camera buffer.
    VelocitySlotTaken,
}

/// Whether a [`DxbcError`] is a defect of the bytecode or a rewrite declining a sound shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxbcErrorKind {
    /// The bytecode is not a well-formed container or token stream. A shader the runtime compiled
    /// never is, so the caller reports it and tries no other rewrite on it.
    Malformed,
    /// The shader is well formed, but the rewrite does not apply to it: another stage, another
    /// shader model, a shape the rewrite does not recognise, or a resource it would need already in
    /// use. The caller leaves the shader as it is or tries the next rewrite.
    NotApplicable,
}

impl DxbcError {
    /// Whether the bytecode is malformed or the rewrite does not apply to it.
    pub fn kind(&self) -> DxbcErrorKind {
        match self {
            DxbcError::NotDxbc
            | DxbcError::Truncated
            | DxbcError::NoShaderChunk
            | DxbcError::ZeroLengthInstruction
            | DxbcError::UnexpectedEndOfTokens
            | DxbcError::MalformedSignature => DxbcErrorKind::Malformed,
            DxbcError::NotVertexShader
            | DxbcError::NotDomainShader
            | DxbcError::NotHullShader
            | DxbcError::UnsupportedShaderModel
            | DxbcError::MissingInputSignature
            | DxbcError::MissingOutputSignature
            | DxbcError::NoPerEyeReferences
            | DxbcError::Cb13AlreadyDeclared
            | DxbcError::InstanceIdAlreadyDeclared
            | DxbcError::NoInstanceId
            | DxbcError::UnsupportedOperandEncoding
            | DxbcError::InstructionTooLong
            | DxbcError::NoPositionOutput
            | DxbcError::UnsupportedControlFlow
            | DxbcError::NoEyeLane
            | DxbcError::NoTerrainCameraRows
            | DxbcError::NoFullViewProjection
            | DxbcError::NoDepthUvFetch
            | DxbcError::NoLakeRefraction
            | DxbcError::NoScreenOverlay
            | DxbcError::NoPlanarReflection
            | DxbcError::NoAmbientOcclusion
            | DxbcError::NoAmbientOcclusionVolume
            | DxbcError::NoScreenSpaceReflection
            | DxbcError::NoReflectionBlur
            | DxbcError::NoPostFilter
            | DxbcError::NoCompositeBloom
            | DxbcError::NoCameraPosition
            | DxbcError::NoPixelPosition
            | DxbcError::EyeCameraSlotTaken
            | DxbcError::DynamicCameraAccess
            | DxbcError::NoScreenDoor
            | DxbcError::ScreenDoorSlotTaken
            | DxbcError::NoVelocity
            | DxbcError::VelocitySlotTaken => DxbcErrorKind::NotApplicable,
            #[cfg(feature = "capture-rewrites")]
            DxbcError::NoResolveInputs | DxbcError::NoLakeReflection => {
                DxbcErrorKind::NotApplicable
            }
        }
    }
}

/// A rewrite's result with its declines separated from its failures.
pub trait Applicable<T> {
    /// `Ok(Some)` for a rewritten shader, `Ok(None)` when the rewrite does not apply, and the error
    /// only when the bytecode is malformed.
    fn applicable(self) -> Result<Option<T>, DxbcError>;
}

impl<T> Applicable<T> for Result<T, DxbcError> {
    fn applicable(self) -> Result<Option<T>, DxbcError> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(error) => match error.kind() {
                DxbcErrorKind::NotApplicable => Ok(None),
                DxbcErrorKind::Malformed => Err(error),
            },
        }
    }
}

impl fmt::Display for DxbcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            DxbcError::NotDxbc => "dxbc: not a DXBC container (bad magic)",
            DxbcError::Truncated => "dxbc: container is truncated",
            DxbcError::NoShaderChunk => "dxbc: no SHEX/SHDR shader chunk",
            DxbcError::ZeroLengthInstruction => "dxbc: zero-length instruction token",
            DxbcError::UnexpectedEndOfTokens => "dxbc: token stream ended mid-instruction",
            DxbcError::NotVertexShader => "dxbc: not a vertex shader",
            DxbcError::NotDomainShader => "dxbc: not a domain shader",
            DxbcError::NotHullShader => "dxbc: not a hull shader",
            DxbcError::UnsupportedShaderModel => "dxbc: SM4 (SHDR) shader; the rewrite needs SHEX",
            DxbcError::MissingInputSignature => "dxbc: no ISGN input-signature chunk",
            DxbcError::MissingOutputSignature => "dxbc: no OSGN output-signature chunk",
            DxbcError::MalformedSignature => "dxbc: signature chunk has an unexpected layout",
            DxbcError::NoPerEyeReferences => "dxbc: no per-eye cb0 operands to rewrite",
            DxbcError::Cb13AlreadyDeclared => "dxbc: the shader already declares cb13",
            DxbcError::InstanceIdAlreadyDeclared => {
                "dxbc: the shader already declares an SV_InstanceID input"
            }
            DxbcError::NoInstanceId => "dxbc: the shader declares no SV_InstanceID input",
            DxbcError::UnsupportedOperandEncoding => {
                "dxbc: a per-eye operand uses an unsupported index encoding"
            }
            DxbcError::InstructionTooLong => {
                "dxbc: a rewritten instruction exceeds the instruction-length field"
            }
            DxbcError::NoPositionOutput => "dxbc: no SV_Position output to reproject",
            DxbcError::UnsupportedControlFlow => {
                "dxbc: reprojection does not support a conditional early return (retc)"
            }
            DxbcError::NoEyeLane => "dxbc: no free TEXCOORD3.z lane to carry the eye index",
            DxbcError::NoFullViewProjection => "dxbc: the shader reads no full view-projection row",
            DxbcError::NoTerrainCameraRows => {
                "dxbc: the domain shader reads no terrain camera rows"
            }
            #[cfg(feature = "capture-rewrites")]
            DxbcError::NoResolveInputs => "dxbc: unsupported deferred input sequence",
            DxbcError::NoScreenOverlay => "dxbc: no recognised screen-space overlay lookup",
            DxbcError::NoPlanarReflection => "dxbc: no recognised planar reflection coordinate",
            DxbcError::NoAmbientOcclusion => {
                "dxbc: no recognised ambient-occlusion generation pass"
            }
            DxbcError::NoAmbientOcclusionVolume => {
                "dxbc: no recognised ambient-occlusion volume pass"
            }
            DxbcError::NoScreenSpaceReflection => {
                "dxbc: no recognised screen-space reflection ray march"
            }
            DxbcError::NoReflectionBlur => "dxbc: no recognised screen-space reflection blur",
            DxbcError::NoPostFilter => "dxbc: no recognised post-chain filter",
            DxbcError::NoCompositeBloom => "dxbc: no recognised composite bloom lookup",
            DxbcError::NoCameraPosition => "dxbc: the pixel shader reads no camera position row",
            DxbcError::NoPixelPosition => "dxbc: the pixel shader has no SV_Position input",
            DxbcError::EyeCameraSlotTaken => {
                "dxbc: the pixel shader already declares the eye-camera constant buffer slot"
            }
            DxbcError::DynamicCameraAccess => {
                "dxbc: a dynamically indexed cb0 read could reach a camera row"
            }
            DxbcError::NoScreenDoor => {
                "dxbc: the pixel shader builds no screen-door pattern from its pixel position"
            }
            DxbcError::ScreenDoorSlotTaken => {
                "dxbc: the pixel shader declares the eye-camera constant buffer slot too small for \
                 the eye boundary"
            }
            DxbcError::NoVelocity => "dxbc: the pixel shader writes no motion vector",
            DxbcError::VelocitySlotTaken => {
                "dxbc: the pixel shader declares the eye-camera constant buffer slot too small for \
                 the velocity rows"
            }
            #[cfg(feature = "capture-rewrites")]
            DxbcError::NoLakeReflection => "dxbc: missing unique planar lake reflection sample",
            DxbcError::NoLakeRefraction => {
                "dxbc: missing unique lake refraction displacement scale"
            }
            DxbcError::NoDepthUvFetch => "dxbc: no projective screen-space depth fetch to bias",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for DxbcError {}

fn read_u32(blob: &[u8], off: usize) -> Result<u32, DxbcError> {
    blob.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(DxbcError::Truncated)
}
