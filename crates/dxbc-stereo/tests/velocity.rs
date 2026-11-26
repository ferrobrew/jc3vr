//! Corpus tests for the velocity pixel-shader rewrite, run against the game's extracted shader
//! bundle.

use dxbc_stereo::{
    Dxbc, DxbcError, EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS,
    LocalLights, ShaderStage, TokenStream, VELOCITY_ORIGIN_ROW, VELOCITY_PIXEL_ROW,
    VELOCITY_REPROJECTION_ROW, VELOCITY_REPROJECTION_ROWS_PER_EYE, localize_eye_camera,
    localize_projected_screen_door, localize_screen_door, localize_velocity, refresh_checksum,
};

mod common;
use common::shader_dir;

/// Every pixel shader that encodes a motion vector is rewritten and every other shader is declined;
/// each rewrite re-parses, corrects the positions once per encoding site right after the divide,
/// declares the buffer once and the position's `.xy`, marks the position read in its signature, and
/// carries a self-consistent checksum.
#[test]
fn corpus_rewrite_covers_every_velocity_shader() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut rewritten = Vec::new();
    for path in shader_paths(&dir) {
        let blob = std::fs::read(&path).expect("read a shader");
        let before = Shader::parse(&blob);
        match localize_velocity(&blob) {
            Ok(out) => {
                rewritten.push(file_index(&path));
                let after = Shader::parse(&out);
                assert_eq!(before.sites, 1, "{}", path.display());
                assert_eq!(after.sites, 1, "{}", path.display());
                assert_eq!(
                    after.temps,
                    before.temps + SCRATCH_TEMPS,
                    "{}",
                    path.display()
                );
                assert_eq!(after.buffer_declarations, 1, "{}", path.display());
                assert!(after.position_reads_xy, "{}", path.display());
                assert!(after.previous_reads_xyzw, "{}", path.display());
                assert!(after.position_signature_reads_xy, "{}", path.display());
                assert_eq!(
                    after.instructions,
                    before.instructions + CORRECTION,
                    "{}",
                    path.display()
                );
                assert_eq!(
                    after.correction_at,
                    after.divide_at.map(|at| at + 1),
                    "{}",
                    path.display()
                );
                let mut refreshed = out.clone();
                refresh_checksum(&mut refreshed);
                assert_eq!(refreshed, out, "{}: stale checksum", path.display());
            }
            Err(DxbcError::NoVelocity) => {
                assert_eq!(before.sites, 0, "{}", path.display());
            }
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    assert_eq!(rewritten, VELOCITY_SHADERS, "every velocity pixel shader");
}

/// In the order the payload applies the pixel rewrites (the eye camera, the screen door, the
/// projected screen door, then the velocity), the velocity rewrite takes the same shaders it takes
/// alone and reads the buffer an earlier rewrite declared rather than declaring it again.
#[test]
fn composes_after_the_other_eye_buffer_rewrites() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut rewritten = Vec::new();
    let mut over_screen_door = 0;
    for path in shader_paths(&dir) {
        let blob = std::fs::read(&path).expect("read a shader");
        let camera = localize_eye_camera(&blob, LocalLights::PerEye).ok();
        let current = camera.as_deref().unwrap_or(&blob);
        let screen_door = localize_screen_door(current).ok();
        let current = screen_door.as_deref().unwrap_or(current);
        let projected = localize_projected_screen_door(current).ok();
        let earlier = camera.is_some() || screen_door.is_some() || projected.is_some();
        let current = projected.as_deref().unwrap_or(current);
        match localize_velocity(current) {
            Ok(out) => {
                rewritten.push(file_index(&path));
                over_screen_door += usize::from(earlier);
                let before = Shader::parse(current);
                let after = Shader::parse(&out);
                assert_eq!(after.buffer_declarations, 1, "{}", path.display());
                assert_eq!(
                    after.temps,
                    before.temps + SCRATCH_TEMPS,
                    "{}",
                    path.display()
                );
                assert_eq!(
                    after.instructions,
                    before.instructions + CORRECTION,
                    "{}",
                    path.display()
                );
            }
            Err(DxbcError::NoVelocity) => {}
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    assert_eq!(rewritten, VELOCITY_SHADERS);
    assert_eq!(over_screen_door, OVER_SCREEN_DOOR);
}

/// The model families' velocity shader gains the correction of the module docs right after its
/// divide, in its own spare temps: the eye from the boundary, the eye's three matrix rows and left
/// edge selected from the buffer, the previous position taken through them and blended in by the
/// weight before its divide, and the pixel's own NDC blended in for the current one.
#[test]
fn model_velocity_takes_the_correction_after_its_divide() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let out = localize_velocity(&std::fs::read(dir.join(MODEL_VELOCITY)).expect("read"))
        .expect("rewritten");
    let instructions = executable(&out);
    let correction = &instructions[5..5 + CORRECTION];
    let opcodes: Vec<u32> = correction.iter().map(|insn| insn[0] & 0x7ff).collect();
    assert_eq!(
        opcodes,
        [
            0x1d, 0x37, 0x37, 0x37, 0x37, 0x11, 0x11, 0x11, 0x00, 0x32, 0x0e, 0x32, 0x32, 0x00,
            0x32
        ]
    );
    let reads = |insn: &[u32], row: u32| {
        insn.windows(3).any(|t| {
            t[1] == EYE_CAMERA_CB_REGISTER && t[2] == row && t[0] & 0x000f_f000 == 0x0000_8000
        })
    };
    assert!(reads(&correction[0], EYE_CAMERA_BOUNDARY_ROW));
    for (insn, row) in correction[1..4].iter().zip([0, 1, 3]) {
        assert!(reads(insn, VELOCITY_REPROJECTION_ROW + row));
        assert!(reads(
            insn,
            VELOCITY_REPROJECTION_ROW + VELOCITY_REPROJECTION_ROWS_PER_EYE + row
        ));
    }
    assert!(reads(&correction[4], VELOCITY_ORIGIN_ROW));
    for insn in [
        &correction[9],
        &correction[11],
        &correction[12],
        &correction[14],
    ] {
        assert!(reads(insn, VELOCITY_PIXEL_ROW));
    }
    // The previous position's divide writes the positions temp's `.xy`, and the last blend its `.zw`.
    assert_eq!(&correction[10][1..3], &[0x0010_0032, 0]);
    assert_eq!(&correction[14][1..3], &[0x0010_00c2, 0]);
    // Every matrix row is dotted with the interpolated previous position, `v2`.
    for insn in &correction[5..8] {
        assert_eq!(&insn[5..7], &[0x0010_1e46, 2]);
    }
}

/// `generalmkiiiprezvelocity`: no texture, no discard, the shortest velocity shader.
const MODEL_VELOCITY: &str = "sh_0927_005578a0.dxbc";
/// The bundle indices of every pixel shader that encodes a motion vector: the `rendervelocitybuffer`
/// family and `renderfadingvelocitybuffer`, `tex0velocity`, the bark and foliage velocity shaders,
/// and the two `generalmkiiiprezvelocity` permutations.
const VELOCITY_SHADERS: [u32; 12] = [610, 613, 614, 616, 617, 618, 619, 665, 793, 808, 927, 928];
/// How many of them an earlier eye-buffer rewrite takes first (the screen-door fades of the bark,
/// foliage, stippled, and alpha-tested model shaders).
const OVER_SCREEN_DOOR: usize = 4;
/// The temps the correction adds.
const SCRATCH_TEMPS: u32 = 4;
/// The fifteen instructions the correction adds.
const CORRECTION: usize = 15;

fn shader_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    paths
}

/// The bundle index in an extracted shader's file name (`sh_<index>_<offset>.dxbc`).
fn file_index(path: &std::path::Path) -> u32 {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('_').nth(1))
        .and_then(|index| index.parse().ok())
        .expect("an extracted shader's file name")
}

fn words(blob: &[u8]) -> Vec<u32> {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
    body.as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect()
}

/// Every executable instruction of a shader, as its tokens.
fn executable(blob: &[u8]) -> Vec<Vec<u32>> {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
    let stream = TokenStream::new(body).expect("the shader tokenizes");
    let tokens = words(blob);
    stream
        .instructions()
        .map(|insn| insn.expect("every instruction decodes"))
        .filter(|insn| !insn.is_declaration())
        .map(|insn| tokens[insn.start..insn.end].to_vec())
        .collect()
}

/// The register and read mask of the `SV_Position` element of an input signature.
fn position_input(isgn: &[u8]) -> Option<(u32, u8)> {
    let count = u32::from_le_bytes(isgn.get(0..4)?.try_into().ok()?) as usize;
    (0..count)
        .map(|i| isgn.get(8 + i * 24..8 + (i + 1) * 24))
        .find_map(|record| {
            let record = record?;
            (u32::from_le_bytes(record[8..12].try_into().ok()?) == 1).then(|| {
                (
                    u32::from_le_bytes(record[16..20].try_into().expect("4 bytes")),
                    record[21],
                )
            })
        })
}

/// What the tests read back from a shader.
#[derive(Default)]
struct Shader {
    instructions: usize,
    temps: u32,
    buffer_declarations: usize,
    /// Whether the position's input declaration covers `.xy`.
    position_reads_xy: bool,
    /// Whether the position's signature element is marked read in `.xy`.
    position_signature_reads_xy: bool,
    /// Whether every input the correction takes through a matrix is declared in all four components.
    previous_reads_xyzw: bool,
    /// Encoding sites: a mapping to UV by `(0.5, -0.5, 0.5, -0.5)` and `0.5`.
    sites: usize,
    /// The index of the last executable instruction dividing ahead of the correction.
    divide_at: Option<usize>,
    /// The index of the first executable instruction reading the eye boundary.
    correction_at: Option<usize>,
}

impl Shader {
    fn parse(blob: &[u8]) -> Shader {
        let dxbc = Dxbc::parse(blob).expect("the shader parses");
        let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
        let stream = TokenStream::new(body).expect("the shader tokenizes");
        let mut shader = Shader::default();
        if stream.stage() != ShaderStage::Pixel {
            return shader;
        }
        let tokens = words(blob);
        let position = dxbc
            .chunk(b"ISGN")
            .and_then(|chunk| position_input(chunk.body(blob)));
        shader.position_signature_reads_xy = position.is_some_and(|(_, read)| read & 3 == 3);
        let mut last_divide = None;
        let mut input_masks = Vec::new();
        let mut matrix_inputs = Vec::new();
        for insn in stream.instructions() {
            let insn = insn.expect("every instruction decodes");
            for operand in insn.operands() {
                operand.expect("every operand decodes");
            }
            let w = &tokens[insn.start..insn.end];
            if insn.is_declaration() {
                match insn.opcode {
                    0x68 => shader.temps = w[1],
                    0x59 if w[2] == EYE_CAMERA_CB_REGISTER => {
                        assert!(w[3] >= EYE_CAMERA_CB_ROWS);
                        shader.buffer_declarations += 1;
                    }
                    0x62 => input_masks.push((w[2], w[1] & 0xf0)),
                    0x64 if position.is_some_and(|(register, _)| register == w[2])
                        && w[1] & 0x30 == 0x30 =>
                    {
                        shader.position_reads_xy = true;
                    }
                    _ => {}
                }
                continue;
            }
            let index = shader.instructions;
            shader.instructions += 1;
            if insn.opcode == 0x0e {
                last_divide = Some(index);
            }
            // `dp4` of a temp with an input register.
            if insn.opcode == 0x11 && w.len() == 7 && (w[5] >> 12) & 0xff == 1 {
                matrix_inputs.push(w[6]);
            }
            if w.windows(3)
                .any(|t| t == [0x0020_800A, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_BOUNDARY_ROW])
                && shader.correction_at.is_none()
            {
                shader.correction_at = Some(index);
                shader.divide_at = last_divide;
            }
            if insn.opcode == 0x32 && w.windows(UV_MAPPING.len()).any(|t| t == UV_MAPPING) {
                shader.sites += 1;
                shader
                    .divide_at
                    .get_or_insert(last_divide.unwrap_or(usize::MAX));
            }
        }
        shader.previous_reads_xyzw = !matrix_inputs.is_empty()
            && matrix_inputs.iter().all(|register| {
                input_masks
                    .iter()
                    .any(|&(declared, mask)| declared == *register && mask == 0xf0)
            });
        shader
    }
}

/// The mapping from NDC to UV, as the encoding's `mad` carries its two immediates.
const UV_MAPPING: [u32; 10] = [
    0x0000_4002,
    0x3f00_0000,
    0xbf00_0000,
    0x3f00_0000,
    0xbf00_0000,
    0x0000_4002,
    0x3f00_0000,
    0x3f00_0000,
    0x3f00_0000,
    0x3f00_0000,
];
