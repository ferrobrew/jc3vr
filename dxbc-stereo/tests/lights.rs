//! Corpus tests for the local-light half of the eye-camera rewrite, run against the game's extracted
//! shader bundle.

use dxbc_stereo::{
    Dxbc, DxbcError, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_LIGHT_OFFSET_ROW, LocalLights, OperandKind,
    TokenStream, localize_eye_camera, refresh_checksum,
};

mod common;
use common::shader_dir;

/// Every pixel shader that lights from the clustered-lighting frame constants and measures its light
/// vectors from the camera row has its two light position reads corrected, and the deferred resolve
/// and the hair also their projection; the vegetation blend, whose light vectors are measured from a
/// position its vertex shader made relative to the camera, is left alone. A shader with corrections
/// declares one more temp than the camera rewrite alone and adds one instruction per position and
/// three per projection, each reading the eye's light offset; every other shader comes out of the two
/// modes identical.
#[test]
fn corpus_corrects_every_light_measured_from_the_eye() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut corrected = Vec::new();
    let mut offset_reads = 0;
    for path in shader_paths(&dir) {
        let blob = std::fs::read(&path).expect("read a shader");
        let (unchanged, per_eye) = match (
            localize_eye_camera(&blob, LocalLights::Unchanged),
            localize_eye_camera(&blob, LocalLights::PerEye),
        ) {
            (Ok(unchanged), Ok(per_eye)) => (unchanged, per_eye),
            (Err(DxbcError::NoCameraPosition), Err(DxbcError::NoCameraPosition)) => continue,
            (a, b) => panic!("{}: {:?} vs {:?}", path.display(), a.err(), b.err()),
        };
        let before = Shader::parse(&unchanged);
        let after = Shader::parse(&per_eye);
        if after.offset_reads == 0 {
            assert_eq!(unchanged, per_eye, "{}", path.display());
            continue;
        }
        let name = path.file_name().expect("a file name").to_string_lossy();
        let added = after.instructions - before.instructions;
        assert_eq!(after.temps, before.temps + 1, "{name}");
        assert_eq!(
            after.offset_reads, added,
            "{name}: every inserted instruction reads the offset"
        );
        let mut refreshed = per_eye.clone();
        refresh_checksum(&mut refreshed);
        assert_eq!(refreshed, per_eye, "{name}: stale checksum");
        offset_reads += after.offset_reads;
        corrected.push((name.into_owned(), added));
    }
    let resolves: Vec<_> = corrected
        .iter()
        .filter(|(name, _)| RESOLVES.iter().any(|resolve| name.starts_with(resolve)))
        .collect();
    assert_eq!(resolves.len(), RESOLVES.len(), "every resolve permutation");
    assert!(
        resolves.iter().all(|(_, added)| *added == 2 + 3),
        "{resolves:?}"
    );
    let hair = corrected
        .iter()
        .find(|(name, _)| name.starts_with(HAIR))
        .expect("the hair is corrected");
    assert_eq!(hair.1, 2 + 3);
    assert!(
        corrected
            .iter()
            .all(|(name, _)| !VEGETATION_BLEND.iter().any(|v| name.starts_with(v))),
        "the vegetation blend's interpolated position is left alone"
    );
    assert_eq!(corrected.len(), CORRECTED_SHADERS);
    assert_eq!(
        offset_reads,
        2 * CORRECTED_SHADERS + 3 * (RESOLVES.len() + 1)
    );
}

/// In the deferred resolve, each light position read becomes a read of a temp the instruction before
/// computes as the original read minus the eye's offset.
#[test]
fn resolve_subtracts_the_eye_offset_from_each_light_position() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join(RESOLVE)).expect("read the resolve");
    let out = localize_eye_camera(&blob, LocalLights::PerEye).expect("rewritten");
    let dxbc = Dxbc::parse(&out).expect("parses");
    let stream = TokenStream::new(dxbc.shader_chunk().expect("a shader chunk").body(&out))
        .expect("tokenizes");
    let mut adds = 0;
    for insn in stream.instructions() {
        let insn = insn.expect("decodes");
        let reads_offset = insn.operands().any(|operand| {
            matches!(
                operand.expect("decodes").kind,
                OperandKind::ConstantBufferDynamic {
                    register: EYE_CAMERA_CB_REGISTER
                }
            )
        });
        let reads_lights = insn.operands().any(|operand| {
            matches!(
                operand.expect("decodes").kind,
                OperandKind::ConstantBufferDynamic { register: 3 }
            )
        });
        if insn.opcode == 0x00 && reads_offset && reads_lights {
            adds += 1;
        }
    }
    assert_eq!(adds, 2, "the point and the spot light position");
}

/// The deferred clustered-lighting resolve permutations, by file name prefix.
const RESOLVES: [&str; 10] = [
    "sh_0516_", "sh_0517_", "sh_0518_", "sh_1178_", "sh_1179_", "sh_1180_", "sh_1181_", "sh_1182_",
    "sh_1183_", "sh_1184_",
];
const RESOLVE: &str = "sh_0516_001a64e0.dxbc";
/// `characterhair_blend`, the one forward family that reads a projection.
const HAIR: &str = "sh_0492_";
/// `vegetationfoliageblend` and `vegetationfoliageblendaoit`.
const VEGETATION_BLEND: [&str; 2] = ["sh_0796_", "sh_0797_"];
/// The forward-lit families measuring from the camera row, and the resolves.
const CORRECTED_SHADERS: usize = 141;

fn shader_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    paths
}

struct Shader {
    instructions: usize,
    temps: u32,
    offset_reads: usize,
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
            offset_reads: 0,
        };
        for insn in stream.instructions() {
            let insn = insn.expect("every instruction decodes");
            if insn.is_declaration() {
                if insn.opcode == 0x68 {
                    shader.temps = tokens[insn.start + 1];
                }
                continue;
            }
            shader.instructions += 1;
            for operand in insn.operands() {
                let operand = operand.expect("every operand decodes");
                if operand.kind
                    == (OperandKind::ConstantBufferDynamic {
                        register: EYE_CAMERA_CB_REGISTER,
                    })
                {
                    let token = tokens[operand.token_offset];
                    let extended = ((token >> 31) & 1) as usize;
                    if tokens[operand.token_offset + 2 + extended] == EYE_CAMERA_LIGHT_OFFSET_ROW {
                        shader.offset_reads += 1;
                    }
                }
            }
        }
        shader
    }
}
