//! Corpus tests for the screen-door pixel-shader rewrite, run against the game's extracted shader
//! bundle.

use dxbc_stereo::{
    Dxbc, DxbcError, EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS,
    LocalLights, ShaderStage, TokenStream, localize_eye_camera, localize_screen_door,
    refresh_checksum,
};

mod common;
use common::shader_dir;

/// Every pixel shader that builds the screen-door pattern from its pixel position is rewritten and
/// every other shader is declined; each rewrite re-parses, builds the pattern from the eye-local
/// position at every site the original built it from the pixel position, declares the boundary's
/// buffer once, and carries a self-consistent checksum.
#[test]
fn corpus_rewrite_covers_every_screen_door() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut rewritten = 0;
    let mut pattern_elsewhere = 0;
    for path in shader_paths(&dir) {
        let blob = as_created(&std::fs::read(&path).expect("read a shader"));
        let before = Shader::parse(&blob);
        match localize_screen_door(&blob) {
            Ok(out) => {
                rewritten += 1;
                let after = Shader::parse(&out);
                assert!(before.position_sites > 0, "{}", path.display());
                assert_eq!(after.position_sites, 0, "{}", path.display());
                assert_eq!(
                    after.local_sites,
                    before.position_sites,
                    "{}",
                    path.display()
                );
                assert_eq!(after.temps, before.temps + 1, "{}", path.display());
                assert_eq!(after.buffer_declarations, 1, "{}", path.display());
                assert_eq!(
                    after.instructions,
                    before.instructions + PROLOGUE,
                    "{}",
                    path.display()
                );
                assert_eq!(after.prologue_at, Some(0), "{}", path.display());
                let mut refreshed = out.clone();
                refresh_checksum(&mut refreshed);
                assert_eq!(refreshed, out, "{}: stale checksum", path.display());
            }
            Err(DxbcError::NoScreenDoor) => {
                assert_eq!(before.position_sites, 0, "{}", path.display());
                if before.pattern_constants > 0 {
                    pattern_elsewhere += 1;
                }
            }
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    assert_eq!(rewritten, SCREEN_DOORS, "every screen-door pixel shader");
    // The tree impostors build the same pattern from their own projected position over the screen
    // size, not from `SV_Position`, and are left alone.
    assert_eq!(pattern_elsewhere, IMPOSTOR_PATTERNS);
}

/// Every site in the corpus is the one hash, so the rewrite's structural match and the engine's
/// pattern are the same set.
#[test]
fn every_screen_door_site_is_the_engine_hash() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut sites = 0;
    for path in shader_paths(&dir) {
        let blob = as_created(&std::fs::read(&path).expect("read a shader"));
        let shader = Shader::parse(&blob);
        assert_eq!(
            shader.position_sites,
            shader.position_pattern_sites,
            "{}",
            path.display()
        );
        sites += shader.position_sites;
    }
    assert_eq!(sites, SCREEN_DOORS);
}

/// Over the eye-camera rewrite, the screen-door rewrite reads the buffer that rewrite declared rather
/// than declaring it again, and takes the same shaders it takes alone.
#[test]
fn composes_after_the_eye_camera_rewrite() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut rewritten = 0;
    let mut over_camera = 0;
    for path in shader_paths(&dir) {
        let blob = as_created(&std::fs::read(&path).expect("read a shader"));
        let (code, camera) = match localize_eye_camera(&blob, LocalLights::PerEye) {
            Ok(out) => (out, true),
            Err(_) => (blob.clone(), false),
        };
        match localize_screen_door(&code) {
            Ok(out) => {
                rewritten += 1;
                over_camera += usize::from(camera);
                let before = Shader::parse(&code);
                let after = Shader::parse(&out);
                assert_eq!(after.buffer_declarations, 1, "{}", path.display());
                assert_eq!(after.temps, before.temps + 1, "{}", path.display());
                assert_eq!(after.position_sites, 0, "{}", path.display());
                assert_eq!(
                    after.local_sites,
                    before.position_sites,
                    "{}",
                    path.display()
                );
                assert!(localize_screen_door(&blob).is_ok(), "{}", path.display());
            }
            Err(DxbcError::NoScreenDoor) => {
                assert!(
                    matches!(localize_screen_door(&blob), Err(DxbcError::NoScreenDoor)),
                    "{}",
                    path.display()
                );
            }
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    assert_eq!(rewritten, SCREEN_DOORS);
    assert_eq!(over_camera, OVER_CAMERA);
}

/// Without the sun-shadow filter's rotation zeroed, its hash of the pixel position is laid out per eye
/// as well.
#[test]
fn the_unpatched_shadow_rotation_is_localized_too() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut rewritten = 0;
    for path in shader_paths(&dir) {
        let blob = std::fs::read(&path).expect("read a shader");
        let before = Shader::parse(&blob);
        match localize_screen_door(&blob) {
            Ok(out) => {
                rewritten += 1;
                assert_eq!(Shader::parse(&out).position_sites, 0, "{}", path.display());
            }
            Err(DxbcError::NoScreenDoor) => {
                assert_eq!(before.position_sites, 0, "{}", path.display());
            }
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    assert_eq!(rewritten, SCREEN_DOORS + SHADOW_ROTATIONS);
}

/// The vehicle paint's shadow stipple, the smallest screen door, gains exactly the eye-local
/// position and reads it in its pattern.
#[test]
fn stipple_builds_its_pattern_from_the_eye_local_position() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = as_created(&std::fs::read(dir.join(STIPPLE)).expect("read the shader"));
    let out = localize_screen_door(&blob).expect("rewritten");
    let instructions = executable(&out);
    let local = 1;
    let boundary = [0x0020_800A, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_BOUNDARY_ROW];
    let expected: [Vec<u32>; 5] = [
        [
            vec![(8 << 24) | 0x1d, 0x0010_0012, local, 0x0010_100A, 0],
            boundary.to_vec(),
        ]
        .concat(),
        [
            vec![(8 << 24) | 0x01, 0x0010_0012, local, 0x0010_000A, local],
            boundary.to_vec(),
        ]
        .concat(),
        vec![
            (8 << 24),
            0x0010_0012,
            local,
            0x0010_100A,
            0,
            0x8010_000A,
            0x41,
            local,
        ],
        vec![(5 << 24) | 0x36, 0x0010_0022, local, 0x0010_101A, 0],
        vec![
            (10 << 24) | 0x0f,
            0x0010_0012,
            0,
            0x0010_0046,
            local,
            0x0000_4002,
            0x3eef_965b,
            0xbf34_2246,
            0,
            0,
        ],
    ];
    assert_eq!(&instructions[..5], &expected[..]);
}

/// The vehicle paint's shadow stipple: the pattern and a discard, nothing else.
const STIPPLE: &str = "sh_0487_0018e4d0.dxbc";
/// How many pixel shaders build the screen-door pattern from their pixel position.
const SCREEN_DOORS: usize = 101;
/// How many pixel shaders build only the sun-shadow filter's rotation from their pixel position.
const SHADOW_ROTATIONS: usize = 143;
/// How many of them the eye-camera rewrite takes first.
const OVER_CAMERA: usize = 6;
/// How many pixel shaders carry the pattern's vector but build it from something else.
const IMPOSTOR_PATTERNS: usize = 18;
/// The four instructions the eye-local position adds.
const PROLOGUE: usize = 4;
/// The pattern's vector, as the engine's shaders carry it.
const PATTERN: [u32; 2] = [0x3eef_965b, 0xbf34_2246];

/// A shader as the payload creates it: the sun-shadow filter's rotation vector zeroed
/// (`patch_shadow_pcf_hash`).
fn as_created(blob: &[u8]) -> Vec<u8> {
    let mut code = blob.to_vec();
    let mut patched = false;
    let mut at = 0;
    while at + SHADOW_ROTATION.len() <= code.len() {
        if code[at..at + SHADOW_ROTATION.len()] == SHADOW_ROTATION {
            code[at..at + 8].fill(0);
            patched = true;
            at += SHADOW_ROTATION.len();
        } else {
            at += 1;
        }
    }
    if patched {
        refresh_checksum(&mut code);
    }
    code
}

/// The sun-shadow filter's rotation vector, `l(12.9898, 78.233, 0, 0)`, as its bytes.
const SHADOW_ROTATION: [u8; 16] = [
    0x39, 0xd6, 0x4f, 0x41, 0x4c, 0x77, 0x9c, 0x42, 0, 0, 0, 0, 0, 0, 0, 0,
];

fn shader_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    paths
}

fn words(blob: &[u8]) -> (Vec<u32>, Option<u32>, bool) {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
    let stream = TokenStream::new(body).expect("the shader tokenizes");
    let tokens = body
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    let isgn = dxbc.chunk(b"ISGN").map(|chunk| chunk.body(blob));
    let position = isgn.and_then(position_register);
    (tokens, position, stream.stage() == ShaderStage::Pixel)
}

/// Every executable instruction of a shader, as its tokens.
fn executable(blob: &[u8]) -> Vec<Vec<u32>> {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
    let stream = TokenStream::new(body).expect("the shader tokenizes");
    let (tokens, _, _) = words(blob);
    stream
        .instructions()
        .map(|insn| insn.expect("every instruction decodes"))
        .filter(|insn| !insn.is_declaration())
        .map(|insn| tokens[insn.start..insn.end].to_vec())
        .collect()
}

/// The register of the `SV_Position` element of an input signature.
fn position_register(isgn: &[u8]) -> Option<u32> {
    let count = u32::from_le_bytes(isgn.get(0..4)?.try_into().ok()?) as usize;
    (0..count)
        .map(|i| isgn.get(8 + i * 24..8 + (i + 1) * 24))
        .find_map(|record| {
            let record = record?;
            (u32::from_le_bytes(record[8..12].try_into().ok()?) == 1)
                .then(|| u32::from_le_bytes(record[16..20].try_into().expect("4 bytes")))
        })
}

/// What the tests read back from a shader's token stream.
#[derive(Default)]
struct Shader {
    instructions: usize,
    temps: u32,
    buffer_declarations: usize,
    /// `dp2` sites reading the position input against an immediate vector.
    position_sites: usize,
    /// Of those, the ones whose vector is the engine's pattern.
    position_pattern_sites: usize,
    /// `dp2` sites reading the temp the rewrite builds, against an immediate vector.
    local_sites: usize,
    /// Executable instructions carrying the pattern's vector, from any source.
    pattern_constants: usize,
    /// The index of the first executable instruction reading the boundary.
    prologue_at: Option<usize>,
}

impl Shader {
    fn parse(blob: &[u8]) -> Shader {
        let dxbc = Dxbc::parse(blob).expect("the shader parses");
        let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
        let stream = TokenStream::new(body).expect("the shader tokenizes");
        let (tokens, position, pixel) = words(blob);
        let mut shader = Shader::default();
        if !pixel {
            return shader;
        }
        let mut temps_before = None;
        for insn in stream.instructions() {
            let insn = insn.expect("every instruction decodes");
            let w = &tokens[insn.start..insn.end];
            if insn.is_declaration() {
                match insn.opcode {
                    0x68 => {
                        shader.temps = w[1];
                        temps_before.get_or_insert(w[1]);
                    }
                    0x59 if w[2] == EYE_CAMERA_CB_REGISTER => {
                        assert!(w[3] >= EYE_CAMERA_CB_ROWS);
                        shader.buffer_declarations += 1;
                    }
                    _ => {}
                }
                continue;
            }
            let index = shader.instructions;
            shader.instructions += 1;
            if w.windows(2).any(|pair| pair == PATTERN) {
                shader.pattern_constants += 1;
            }
            if w.windows(3)
                .any(|t| t == [0x0020_800A, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_BOUNDARY_ROW])
            {
                shader.prologue_at.get_or_insert(index);
            }
            // `dp2 dst, src, l(a, b, c, d)` with a one-token destination register index.
            if insn.opcode != 0x0f || w.len() != 10 || w[5] != 0x0000_4002 {
                continue;
            }
            let (source, register) = (w[3], w[4]);
            let input = (source >> 12) & 0xff == 1;
            let temp = (source >> 12) & 0xff == 0;
            let constant = w[6..8].iter().all(|&lane| f32::from_bits(lane) == 0.0);
            if input && Some(register) == position && !constant {
                shader.position_sites += 1;
                if w[6..8] == PATTERN {
                    shader.position_pattern_sites += 1;
                }
            } else if temp && Some(register) == temps_before.map(|t| t - 1) {
                shader.local_sites += 1;
            }
        }
        shader
    }
}
