//! Validates the eye-pinned reprojection ([`reproject_pinned_eye`]) against the terrain patch
//! pipelines that cannot carry an eye index: the patch domain shaders whose vertex shaders fill the
//! whole `TEXCOORD3` interpolant, and the GPU-indirect basemesh vertex shaders that already consume
//! `SV_InstanceID`. The game shaders are a git-ignored local extract; see `common`.

use dxbc_stereo::{
    Dxbc, DxbcError, STEREO_REPROJ_CB_ROWS, ShaderStage, TokenStream, patch_vertex_shader,
    rebase_domain_shader, refresh_checksum, reproject_pinned_eye, reproject_vertex_shader,
};

mod common;
use common::shader_dir;

/// The terrain patch system's domain shaders: they build clip from the domain `cb1` offset
/// view-projection, and their vertex shaders write all four `TEXCOORD3` components, so the eye-lane
/// reprojection declines them.
const PATCH_DOMAIN_SHADERS: [&str; 3] = [
    "sh_1514_00a8e340.dxbc", // volumetricterrain4
    "sh_1515_00a8f040.dxbc", // volumetricterrain4near
    "sh_1516_00a92e80.dxbc", // volumetricterrain4silhouette
];

/// The terrain domain shaders whose vertex shaders leave `TEXCOORD3.z` free, which the eye-lane
/// reprojection takes.
const LANE_DOMAIN_SHADERS: [&str; 3] = [
    "sh_1513_00a8d600.dxbc", // volumetricterrain2
    "sh_1518_00a94710.dxbc", // volumetricterrainpow2
    "sh_1520_00a971d0.dxbc", // volumetricterrainsilhouette
];

/// The GPU-indirect basemesh vertex shaders: they write `SV_Position` from the full view-projection
/// and already read `SV_InstanceID`, so neither the remap nor the instance-parity reprojection applies.
const INSTANCED_BASEMESH_SHADERS: [&str; 2] = [
    "sh_0195_00073a20.dxbc", // volumetricterrain4notessellationblendinstanced
    "sh_0196_00075060.dxbc", // volumetricterrain4notessellationinstanced
];

/// The shaders the pinned rewrite exists for are exactly the ones the existing transforms decline:
/// the eye-lane domain rewrite refuses the patch domain shaders for want of a lane (and takes
/// the others), and both vertex transforms refuse the instanced basemesh shaders on their instance
/// input.
#[test]
fn pinned_targets_are_the_shaders_the_other_transforms_decline() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let read = |name: &str| std::fs::read(dir.join(name)).expect("read shader");
    for name in PATCH_DOMAIN_SHADERS {
        assert_eq!(
            rebase_domain_shader(&read(name)).err(),
            Some(DxbcError::NoEyeLane),
            "{name}: the eye-lane rewrite declines it"
        );
    }
    for name in LANE_DOMAIN_SHADERS {
        assert!(
            rebase_domain_shader(&read(name)).is_ok(),
            "{name}: the eye-lane rewrite takes it"
        );
    }
    for name in INSTANCED_BASEMESH_SHADERS {
        let blob = read(name);
        assert_eq!(
            patch_vertex_shader(&blob).err(),
            Some(DxbcError::InstanceIdAlreadyDeclared),
            "{name}: the remap declines it"
        );
        assert_eq!(
            reproject_vertex_shader(&blob).err(),
            Some(DxbcError::InstanceIdAlreadyDeclared),
            "{name}: the instance-parity reprojection declines it"
        );
    }
}

/// Every target rewrites into a sound blob: it re-parses to the exact declared length, declares
/// `cb13` at the reprojection size, keeps both signatures byte for byte, adds no viewport capability,
/// writes `SV_Position` only through the four-`dp4` epilogue before each `ret`, reads the first eye
/// slot's `M_eye` rows, and carries a valid checksum. A second pass is refused.
#[test]
fn pinned_rewrite_is_sound_on_every_target() {
    let Some(dir) = shader_dir() else {
        return;
    };
    for name in PATCH_DOMAIN_SHADERS
        .iter()
        .chain(INSTANCED_BASEMESH_SHADERS.iter())
    {
        let blob = std::fs::read(dir.join(name)).expect("read shader");
        let out = reproject_pinned_eye(&blob).unwrap_or_else(|e| panic!("{name}: {e}"));
        validate_pinned(&blob, &out).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            reproject_pinned_eye(&out).err(),
            Some(DxbcError::Cb13AlreadyDeclared),
            "{name}: a second pass is refused"
        );
    }
}

/// Across the whole corpus the rewrite either produces a sound blob or declines with a reason that
/// names the shader's shape: no position output (hull, pixel, and control-point shaders), a
/// conditional return, or an existing `cb13`. Nothing errors for an unexpected reason.
#[test]
fn corpus_pinned_rewrite_is_sound_or_declines_cleanly() {
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
        match reproject_pinned_eye(&blob) {
            Ok(out) => {
                validate_pinned(&blob, &out).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                rewritten += 1;
            }
            Err(
                DxbcError::NoPositionOutput
                | DxbcError::UnsupportedControlFlow
                | DxbcError::Cb13AlreadyDeclared
                | DxbcError::UnsupportedShaderModel,
            ) => {}
            Err(e) => panic!("{}: unexpected decline: {e}", path.display()),
        }
    }
    assert!(rewritten > 0, "the corpus has position-writing shaders");
}

/// The structural checks shared by the tests above.
fn validate_pinned(original: &[u8], out: &[u8]) -> Result<(), String> {
    let before = Dxbc::parse(original).map_err(|e| format!("parse original: {e}"))?;
    let dxbc = Dxbc::parse(out).map_err(|e| format!("parse: {e}"))?;
    for tag in [b"ISGN", b"OSGN"] {
        let a = before.chunk(tag).ok_or("original lacks a signature")?;
        let b = dxbc.chunk(tag).ok_or("rewrite lacks a signature")?;
        if a.body(original) != b.body(out) {
            return Err(format!("{} changed", String::from_utf8_lossy(tag)));
        }
    }
    let sfi0 = |d: &Dxbc, blob: &[u8]| {
        d.chunk(b"SFI0")
            .map(|c| c.body(blob).to_vec())
            .unwrap_or_default()
    };
    if sfi0(&before, original) != sfi0(&dxbc, out) {
        return Err("the feature flags changed".into());
    }
    let mut refreshed = out.to_vec();
    refresh_checksum(&mut refreshed);
    if refreshed != out {
        return Err("stale checksum".into());
    }

    let shex = dxbc.shader_chunk().ok_or("no SHEX")?;
    let body = shex.body(out);
    let stream = TokenStream::new(body).map_err(|e| format!("stream: {e}"))?;
    if !matches!(stream.stage(), ShaderStage::Vertex | ShaderStage::Domain) {
        return Err("rewrote a stage with no position output".into());
    }
    let tokens: Vec<u32> = body
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let declared_len = tokens[1] as usize;
    let pos_reg = position_output_register(out).ok_or("no SV_Position output")?;

    let mut last_end = 2;
    let mut cb13_rows = None;
    let mut writers = Vec::new();
    let mut rets = 0;
    let mut meye_rows = Vec::new();
    for insn in stream.instructions() {
        let insn = insn.map_err(|e| format!("insn: {e}"))?;
        for operand in insn.operands() {
            operand.map_err(|e| format!("operand: {e}"))?;
        }
        last_end = insn.end;
        if insn.is_declaration() {
            // dcl_constantbuffer: operand token, register, size.
            if insn.opcode == 0x59 && tokens[insn.start + 2] == 13 {
                cb13_rows = Some(tokens[insn.start + 3]);
            }
            continue;
        }
        if insn.opcode == 0x3E {
            rets += 1;
        }
        for operand in insn.operands() {
            let operand = operand.map_err(|e| format!("operand: {e}"))?;
            if writes_output_register(&tokens, operand.token_offset, pos_reg) {
                writers.push(insn.opcode);
                // dp4 o, cb13[rBase + row], rClip: opcode, destination, register, then the cb operand
                // token, its register, and its row.
                meye_rows.push(tokens[insn.start + 5]);
            }
        }
    }
    if last_end != declared_len {
        return Err("instructions do not end at the declared length".into());
    }
    if cb13_rows != Some(STEREO_REPROJ_CB_ROWS) {
        return Err(format!("cb13 declared as {cb13_rows:?}"));
    }
    if rets == 0 || writers != vec![0x11u32; 4 * rets] {
        return Err(format!(
            "SV_Position writers = {writers:?}, want four dp4 per ret ({rets})"
        ));
    }
    if meye_rows.chunks(4).any(|rows| rows != [10, 11, 12, 13]) {
        return Err(format!("epilogue reads rows {meye_rows:?}"));
    }
    // The row base is held at zero: `mov rBase.x, l(0)` with rBase the first injected temp.
    let zeroed = stream.instructions().filter_map(Result::ok).any(|insn| {
        insn.opcode == 0x36 && tokens[insn.start + 3] == 0x4001 && tokens[insn.start + 4] == 0
    });
    if !zeroed {
        return Err("no zero row base".into());
    }
    Ok(())
}

/// Whether the operand at `token_offset` writes output register `reg`.
fn writes_output_register(tokens: &[u32], token_offset: usize, reg: u32) -> bool {
    let tok = tokens[token_offset];
    let extended = ((tok >> 31) & 1) as usize;
    (tok >> 12) & 0xFF == 2
        && (tok >> 20) & 0x3 == 1
        && (tok >> 22) & 0x7 == 0
        && tokens.get(token_offset + 1 + extended) == Some(&reg)
}

/// The `SV_Position` output register, read from the `OSGN` (system value 1).
fn position_output_register(blob: &[u8]) -> Option<u32> {
    let dxbc = Dxbc::parse(blob).ok()?;
    let body = dxbc.chunk(b"OSGN")?.body(blob);
    let count = u32::from_le_bytes(body[0..4].try_into().ok()?) as usize;
    let table = u32::from_le_bytes(body[4..8].try_into().ok()?) as usize;
    (0..count).find_map(|i| {
        let record = body.get(table + i * 24..table + i * 24 + 24)?;
        let system_value = u32::from_le_bytes(record[8..12].try_into().ok()?);
        (system_value == 1).then(|| u32::from_le_bytes(record[16..20].try_into().unwrap()))
    })
}
