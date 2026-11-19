//! Corpus tests for the eye-camera pixel-shader rewrite, run against the game's extracted shader
//! bundle.

use dxbc_stereo::{
    Dxbc, DxbcError, EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS,
    EYE_CAMERA_ROWS_PER_EYE, EYE_CAMERA_SOURCE_ROWS, LocalLights, OperandKind, ShaderStage,
    TokenStream, localize_eye_camera, localize_post_filter, refresh_checksum,
    trace_deferred_samples,
};

mod common;
use common::shader_dir;

/// Every pixel shader that reads one of the camera rows is rewritten and every other shader is
/// declined; each rewrite re-parses, reads none of the camera rows from `cb0` any more, reads the
/// eye's copy wherever the original read the row, declares the buffer once, and carries a
/// self-consistent checksum.
#[test]
fn corpus_rewrite_covers_every_camera_reader() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut rewritten = 0;
    let mut declared_position = 0;
    let mut widened_position = 0;
    let mut added_position = 0;
    for path in shader_paths(&dir) {
        let blob = std::fs::read(&path).expect("read a shader");
        let reads = camera_reads(&blob);
        match localize_eye_camera(&blob, LocalLights::Unchanged) {
            Ok(out) => {
                assert!(
                    reads > 0,
                    "{}: rewritten without a camera read",
                    path.display()
                );
                rewritten += 1;
                let before = Shader::parse(&blob);
                let after = Shader::parse(&out);
                assert_eq!(camera_reads(&out), 0, "{}", path.display());
                assert_eq!(after.eye_reads, reads, "{}", path.display());
                assert_eq!(after.eye_buffer_declarations, 1, "{}", path.display());
                assert_eq!(after.temps, before.temps + 1, "{}", path.display());
                let mask = after.position_mask.expect("the position is declared");
                assert_ne!(mask & 1, 0, "{}: no .x in the position", path.display());
                match before.position_mask {
                    Some(mask) if mask & 1 != 0 => declared_position += 1,
                    Some(_) => widened_position += 1,
                    None => added_position += 1,
                }
                assert_eq!(
                    after.instructions,
                    before.instructions + PROLOGUE,
                    "{}",
                    path.display()
                );
                assert_ne!(
                    position_read_mask(&out) & 1,
                    0,
                    "{}: the signature does not read the position's .x",
                    path.display()
                );
                let mut refreshed = out.clone();
                refresh_checksum(&mut refreshed);
                assert_eq!(refreshed, out, "{}: stale checksum", path.display());
            }
            Err(DxbcError::NoCameraPosition) => {
                assert_eq!(reads, 0, "{}: a camera reader was declined", path.display());
            }
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    assert_eq!(rewritten, 432, "every camera-reading pixel shader");
    assert_eq!(
        (declared_position, widened_position, added_position),
        (POSITION_DECLARED, POSITION_WIDENED, POSITION_ADDED),
        "how the rewrites came by the position they select the eye with"
    );
}

/// The eye selection compares the pixel position against the boundary row and keeps the eye's first
/// row, and every camera read in the cloth-sheen term of the character material becomes a read of
/// the eye's copy of the same row.
#[test]
fn character_material_reads_its_view_vector_per_eye() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join(CHARACTER)).expect("read the shader");
    let out = localize_eye_camera(&blob, LocalLights::Unchanged).expect("rewritten");
    let shader = Shader::parse(&out);
    let eye = shader.temps - 1;
    let [select, mask] = &shader.prologue[..] else {
        panic!("a two-instruction prologue");
    };
    assert_eq!(
        select,
        &[
            (8 << 24) | 0x1d,
            0x0010_0012,
            eye,
            0x0010_100A,
            0,
            0x0020_800A,
            EYE_CAMERA_CB_REGISTER,
            EYE_CAMERA_BOUNDARY_ROW
        ]
    );
    assert_eq!(
        mask,
        &[
            (7 << 24) | 0x01,
            0x0010_0012,
            eye,
            0x0010_000A,
            eye,
            0x0000_4001,
            EYE_CAMERA_ROWS_PER_EYE
        ]
    );
    // The material reads the second camera row once, for its view vector.
    assert_eq!(shader.eye_rows, vec![1]);
}

/// The glare chain's filters read no camera row, so the eye-half variants built from the bytecode the
/// engine creates are the same whether or not the camera rewrite ran first.
#[test]
fn post_filters_read_no_camera_row() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut filters = 0;
    for path in shader_paths(&dir) {
        let blob = std::fs::read(&path).expect("read a shader");
        if localize_post_filter(&blob).is_ok() {
            filters += 1;
            assert_eq!(camera_reads(&blob), 0, "{}", path.display());
        }
    }
    assert!(filters > 0);
}

/// The deferred resolve, as the engine creates it with the shadow rotation patched and each eye's
/// camera rows selected, still carries every site the resolve probe traces.
#[test]
fn resolve_probe_traces_the_eye_camera_resolve() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut code = std::fs::read(dir.join(RESOLVE)).expect("read the resolve");
    let seed = [
        0x39, 0xd6, 0x4f, 0x41, 0x4c, 0x77, 0x9c, 0x42, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let at = code
        .windows(seed.len())
        .position(|window| window == seed)
        .expect("the shadow rotation seed");
    code[at..at + 8].fill(0);
    refresh_checksum(&mut code);
    for (local_lights, length) in [
        (LocalLights::Unchanged, 35_120),
        (LocalLights::PerEye, 35_392),
    ] {
        let out = localize_eye_camera(&code, local_lights).expect("rewritten");
        assert_eq!(out.len(), length, "the length the resolve probe accepts");
        for shadow in [false, true] {
            trace_deferred_samples(&out, shadow).expect("every traced site survives");
        }
    }
}

/// The cloth material, which shades its sheen from the camera position.
const CHARACTER: &str = "sh_0489_0018f550.dxbc";
/// The deferred clustered-lighting resolve.
const RESOLVE: &str = "sh_0516_001a64e0.dxbc";
/// The two instructions the eye selection adds.
const PROLOGUE: usize = 2;
/// How many rewritten shaders already declared the position with `.x`, declared it without `.x`, and
/// had it in the signature only.
const POSITION_DECLARED: usize = 183;
const POSITION_WIDENED: usize = 8;
const POSITION_ADDED: usize = 241;

fn shader_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    paths
}

/// The number of immediate `cb0` reads of a camera row in a pixel shader; zero for any other stage.
fn camera_reads(blob: &[u8]) -> usize {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let shex = dxbc.shader_chunk().expect("a shader chunk");
    let stream = TokenStream::new(shex.body(blob)).expect("the shader tokenizes");
    if stream.stage() != ShaderStage::Pixel {
        return 0;
    }
    stream
        .instructions()
        .map(|insn| insn.expect("every instruction decodes"))
        .filter(|insn| !insn.is_declaration())
        .flat_map(|insn| insn.operands().collect::<Vec<_>>())
        .filter(|operand| {
            matches!(
                operand.as_ref().expect("every operand decodes").kind,
                OperandKind::ConstantBuffer { register: 0, element } if EYE_CAMERA_SOURCE_ROWS.contains(&element)
            )
        })
        .count()
}

/// What the tests read back from a shader's token stream.
struct Shader {
    instructions: usize,
    temps: u32,
    eye_buffer_declarations: usize,
    eye_reads: usize,
    /// The immediate part of every read of the eye buffer outside the prologue, in program order.
    eye_rows: Vec<u32>,
    position_mask: Option<u32>,
    prologue: Vec<Vec<u32>>,
}

impl Shader {
    fn parse(blob: &[u8]) -> Shader {
        let dxbc = Dxbc::parse(blob).expect("the shader parses");
        let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
        let stream = TokenStream::new(body).expect("the shader tokenizes");
        let tokens: Vec<u32> = body
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        let mut shader = Shader {
            instructions: 0,
            temps: 0,
            eye_buffer_declarations: 0,
            eye_reads: 0,
            eye_rows: Vec::new(),
            position_mask: None,
            prologue: Vec::new(),
        };
        for insn in stream.instructions() {
            let insn = insn.expect("every instruction decodes");
            let words = &tokens[insn.start..insn.end];
            if insn.is_declaration() {
                match insn.opcode {
                    0x68 => shader.temps = words[1],
                    0x59 if words[2] == EYE_CAMERA_CB_REGISTER => {
                        assert_eq!(words[0] & (1 << 11), 1 << 11, "dynamically indexed");
                        assert_eq!(words[3], EYE_CAMERA_CB_ROWS);
                        shader.eye_buffer_declarations += 1;
                    }
                    0x64 if words.last() == Some(&1) => {
                        shader.position_mask = Some((words[1] >> 4) & 0xf);
                    }
                    _ => {}
                }
                continue;
            }
            shader.instructions += 1;
            let mut reads_boundary = false;
            for operand in insn.operands() {
                let operand = operand.expect("every operand decodes");
                match operand.kind {
                    OperandKind::ConstantBufferDynamic {
                        register: EYE_CAMERA_CB_REGISTER,
                    } => {
                        shader.eye_reads += 1;
                        let tok = tokens[operand.token_offset];
                        let extended = ((tok >> 31) & 1) as usize;
                        shader
                            .eye_rows
                            .push(tokens[operand.token_offset + 2 + extended]);
                    }
                    OperandKind::ConstantBuffer {
                        register: EYE_CAMERA_CB_REGISTER,
                        element: EYE_CAMERA_BOUNDARY_ROW,
                    } => reads_boundary = true,
                    _ => {}
                }
            }
            if reads_boundary || shader.prologue.len() == 1 {
                shader.prologue.push(words.to_vec());
            }
        }
        shader
    }
}

/// The read mask of the `SV_Position` element of a shader's input signature.
fn position_read_mask(blob: &[u8]) -> u8 {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let isgn = dxbc.chunk(b"ISGN").expect("an input signature").body(blob);
    let count = u32::from_le_bytes(isgn[0..4].try_into().expect("4 bytes")) as usize;
    (0..count)
        .map(|i| &isgn[8 + i * 24..8 + (i + 1) * 24])
        .find(|record| u32::from_le_bytes(record[8..12].try_into().expect("4 bytes")) == 1)
        .map(|record| record[21])
        .expect("an SV_Position element")
}
