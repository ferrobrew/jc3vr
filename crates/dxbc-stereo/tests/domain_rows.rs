//! Validates the terrain domain shaders' eye-row rewrites ([`rebase_pinned_domain_shader`] and
//! [`rebase_domain_shader`]) against the game's own terrain domain shaders: each reads its camera rows
//! from `cb13` through a selecting prologue and otherwise runs its own instructions unchanged. The
//! game shaders are a git-ignored local extract; see `common`.

use dxbc_stereo::{
    Dxbc, DxbcError, EYE_ROWS_SELECT_ROW, STEREO_SELECT_CB_ROWS, ShaderStage, TokenStream,
    rebase_domain_shader, rebase_pinned_domain_shader, refresh_checksum,
};

mod common;
use common::shader_dir;

/// The terrain patch system's domain shaders, which take the pinned-eye path.
const PATCH_DOMAIN_SHADERS: [&str; 3] = [
    "sh_1514_00a8e340.dxbc", // volumetricterrain4
    "sh_1515_00a8f040.dxbc", // volumetricterrain4near
    "sh_1516_00a92e80.dxbc", // volumetricterrain4silhouette
];

/// The base terrain's domain shaders, whose vertex shaders leave `TEXCOORD3.z` free for the eye.
const LANE_DOMAIN_SHADERS: [&str; 3] = [
    "sh_1513_00a8d600.dxbc", // volumetricterrain2
    "sh_1518_00a94710.dxbc", // volumetricterrainpow2
    "sh_1520_00a971d0.dxbc", // volumetricterrainsilhouette
];

const OPCODE_MOVC: u32 = 0x37;
const OPCODE_DCL_CONSTANT_BUFFER: u32 = 0x59;

/// Every patch domain shader takes the pinned rebase, keeps both signatures and its feature flags,
/// and refuses a second pass.
#[test]
fn pinned_rebase_is_sound_on_the_patch_shaders() {
    let Some(dir) = shader_dir() else {
        return;
    };
    for name in PATCH_DOMAIN_SHADERS {
        let blob = std::fs::read(dir.join(name)).expect("read shader");
        let out = rebase_pinned_domain_shader(&blob).unwrap_or_else(|e| panic!("{name}: {e}"));
        validate_rebase(&blob, &out, Path::Pinned).unwrap_or_else(|e| panic!("{name}: {e}"));
        let before = Dxbc::parse(&blob).unwrap();
        let after = Dxbc::parse(&out).unwrap();
        for tag in [b"ISGN", b"OSGN", b"SFI0"] {
            assert_eq!(
                before.chunk(tag).map(|c| c.body(&blob).to_vec()),
                after.chunk(tag).map(|c| c.body(&out).to_vec()),
                "{name}: {} changed",
                String::from_utf8_lossy(tag)
            );
        }
        assert_eq!(
            rebase_pinned_domain_shader(&out).err(),
            Some(DxbcError::Cb13AlreadyDeclared),
            "{name}: a second pass is refused"
        );
    }
}

/// Every lane domain shader takes the lane rebase: the eye from the lane, the viewport output, and
/// the camera rows of that eye's slot.
#[test]
fn lane_rebase_is_sound_on_the_lane_shaders() {
    let Some(dir) = shader_dir() else {
        return;
    };
    for name in LANE_DOMAIN_SHADERS {
        let blob = std::fs::read(dir.join(name)).expect("read shader");
        let out = rebase_domain_shader(&blob).unwrap_or_else(|e| panic!("{name}: {e}"));
        validate_rebase(&blob, &out, Path::Lane).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            rebase_domain_shader(&out).err(),
            Some(DxbcError::NoEyeLane),
            "{name}: a second pass is refused"
        );
    }
}

/// The lane rebase declines the patch shaders for want of a lane, as the lane reprojection does, so
/// the payload falls through to the pinned path for them.
#[test]
fn lane_rebase_declines_the_patch_shaders() {
    let Some(dir) = shader_dir() else {
        return;
    };
    for name in PATCH_DOMAIN_SHADERS {
        let blob = std::fs::read(dir.join(name)).expect("read shader");
        assert_eq!(
            rebase_domain_shader(&blob).err(),
            Some(DxbcError::NoEyeLane),
            "{name}"
        );
    }
}

/// Across the corpus the pinned rebase rewrites only domain shaders that read the terrain constants'
/// view-projection rows, and every such rewrite is sound.
#[test]
fn corpus_pinned_rebase_is_sound_or_declines_cleanly() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut rewritten = 0;
    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read shader");
        match rebase_pinned_domain_shader(&blob) {
            Ok(out) => {
                validate_rebase(&blob, &out, Path::Pinned)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                rewritten += 1;
            }
            Err(
                DxbcError::NotDomainShader
                | DxbcError::NoTerrainCameraRows
                | DxbcError::UnsupportedShaderModel,
            ) => {}
            Err(e) => panic!("{}: unexpected decline: {e}", path.display()),
        }
    }
    assert!(rewritten >= PATCH_DOMAIN_SHADERS.len());
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    Pinned,
    Lane,
}

/// The structural checks: the container re-parses with a fresh checksum, the shader is a domain
/// shader ending at its declared length, `cb13` is declared at the selector size, the prologue is
/// five `movc`s that each choose between a `cb13` row and the matching terrain row by the selector,
/// no terrain camera row is read anywhere else, and every other instruction is the original's in
/// order (operands aside).
fn validate_rebase(original: &[u8], out: &[u8], path: Path) -> Result<(), String> {
    let mut refreshed = out.to_vec();
    refresh_checksum(&mut refreshed);
    if refreshed != out {
        return Err("stale checksum".into());
    }
    let (_, rewritten) = body_tokens(out)?;
    let (_, source) = body_tokens(original)?;
    let stream = TokenStream::new(&rewritten.bytes).map_err(|e| format!("stream: {e}"))?;
    if stream.stage() != ShaderStage::Domain {
        return Err("not a domain shader".into());
    }
    let tokens = &rewritten.tokens;

    let mut cb13_rows = None;
    let mut last_end = 2;
    let mut movcs = Vec::new();
    let mut opcodes = Vec::new();
    for insn in stream.instructions() {
        let insn = insn.map_err(|e| format!("insn: {e}"))?;
        last_end = insn.end;
        if insn.is_declaration() {
            if insn.opcode == OPCODE_DCL_CONSTANT_BUFFER && tokens[insn.start + 2] == 13 {
                cb13_rows = Some(tokens[insn.start + 3]);
            }
            continue;
        }
        let mut terrain_rows = Vec::new();
        let mut selector = false;
        for operand in insn.operands() {
            let operand = operand.map_err(|e| format!("operand: {e}"))?;
            match operand.kind {
                dxbc_stereo::OperandKind::ConstantBuffer {
                    register: 1,
                    element: element @ 1..=5,
                } => terrain_rows.push(element),
                dxbc_stereo::OperandKind::ConstantBuffer {
                    register: 13,
                    element,
                } if element == EYE_ROWS_SELECT_ROW => selector = true,
                _ => {}
            }
        }
        if insn.opcode == OPCODE_MOVC && selector {
            movcs.push(terrain_rows);
            continue;
        }
        if !terrain_rows.is_empty() {
            return Err(format!(
                "a terrain camera row is read outside the prologue: {terrain_rows:?}"
            ));
        }
        opcodes.push(insn.opcode);
    }
    if last_end != tokens[1] as usize {
        return Err("instructions do not end at the declared length".into());
    }
    if cb13_rows != Some(STEREO_SELECT_CB_ROWS) {
        return Err(format!("cb13 declared as {cb13_rows:?}"));
    }
    if movcs != (1..=5).map(|row| vec![row]).collect::<Vec<_>>() {
        return Err(format!("the prologue selects {movcs:?}"));
    }

    // The original's instructions, with the lane path's eye prologue (`and`, `mov`, `imul`) ahead.
    let original_stream = TokenStream::new(&source.bytes).map_err(|e| format!("stream: {e}"))?;
    let mut expected: Vec<u32> = match path {
        Path::Pinned => Vec::new(),
        Path::Lane => vec![0x01, 0x36, 0x26],
    };
    for insn in original_stream.instructions() {
        let insn = insn.map_err(|e| format!("insn: {e}"))?;
        if !insn.is_declaration() {
            expected.push(insn.opcode);
        }
    }
    if opcodes != expected {
        return Err("the shader's own instructions changed".into());
    }
    Ok(())
}

struct Body {
    bytes: Vec<u8>,
    tokens: Vec<u32>,
}

fn body_tokens(blob: &[u8]) -> Result<(Dxbc, Body), String> {
    let dxbc = Dxbc::parse(blob).map_err(|e| format!("parse: {e}"))?;
    let bytes = dxbc.shader_chunk().ok_or("no SHEX")?.body(blob).to_vec();
    let tokens = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    Ok((dxbc, Body { bytes, tokens }))
}
