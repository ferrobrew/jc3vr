//! Corpus tests for the ambient-occlusion volume rewrite, run against the game's extracted shader
//! bundle.

use dxbc_stereo::{
    AMBIENT_OCCLUSION_VOLUME_CB_REGISTER, AMBIENT_OCCLUSION_VOLUME_CB_ROWS, Dxbc, DxbcError,
    ShaderStage, TokenStream, localize_ambient_occlusion_volume, refresh_checksum,
};

mod common;
use common::shader_dir;

/// The volume pass's two pixel shaders, the box and the sphere, are the only ones rewritten, and each
/// rewrite re-parses with a self-consistent checksum.
#[test]
fn corpus_rewrite_covers_exactly_the_volume_pass() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let mut rewritten = Vec::new();
    for path in paths {
        let blob = std::fs::read(&path).expect("read a shader");
        match localize_ambient_occlusion_volume(&blob) {
            Ok(out) => {
                instructions(&out);
                let mut refreshed = out.clone();
                refresh_checksum(&mut refreshed);
                assert_eq!(refreshed, out, "{}: stale checksum", path.display());
                rewritten.push(path.file_name().expect("a file name").to_owned());
            }
            Err(DxbcError::NoAmbientOcclusionVolume) => {}
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    assert_eq!(
        rewritten,
        ["sh_0462_00169680.dxbc", "sh_0464_0016a450.dxbc"],
        "the rewritten shaders are `aobox` and `aosphere`"
    );
}

/// Both depth samples read the pixel's double-wide UV, both depth products become the eye's mapping
/// through the whole matrix, the buffer is declared once at its full size, and the shader's own
/// instructions are otherwise kept.
#[test]
fn volume_pass_maps_each_depth_through_its_eye() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join(VOLUME)).expect("read the shader");
    let before = instructions(&blob);
    let after = instructions(&localize_ambient_occlusion_volume(&blob).expect("rewritten"));

    let count = |insns: &[Vec<u32>], opcode: u32| {
        insns
            .iter()
            .filter(|insn| insn[0] & 0x7ff == opcode)
            .count()
    };
    assert_eq!(count(&before, SAMPLE_L), 2);
    assert_eq!(count(&after, SAMPLE_L), 2);
    assert_eq!(count(&before, DP4), 0);
    assert_eq!(count(&after, DP4), 8);

    // The buffer and position declarations, the five-instruction prologue, and per depth product the copy of the depth,
    // four `dp4`s, the divide, and three matrix rows in place of the one.
    let products = 2;
    assert_eq!(
        after.len(),
        before.len() + 2 + 5 + products * (1 + 4 + 1 + 3 - 1)
    );

    let uv = 3 + 1; // `dcl_temps 3`, then the eye temporary ahead of the UV.
    for insn in after.iter().filter(|insn| insn[0] & 0x7ff == SAMPLE_L) {
        let extended = insn
            .iter()
            .skip(1)
            .take_while(|t| *t & 1 << 31 != 0)
            .count();
        let coordinate = 2 + extended + 2;
        assert_eq!(&insn[coordinate..coordinate + 2], &[0x0010_0046, uv]);
    }

    let declarations: Vec<_> = after
        .iter()
        .filter(|insn| insn[0] & 0x7ff == DCL_CONSTANT_BUFFER)
        .filter(|insn| insn[2] == AMBIENT_OCCLUSION_VOLUME_CB_REGISTER)
        .collect();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0][3], AMBIENT_OCCLUSION_VOLUME_CB_ROWS);

    // The matrix's depth row is read only by the rebuilt products, which read all four rows' first
    // three through it with the original swizzle.
    let depth_row_reads = |insns: &[Vec<u32>]| {
        insns
            .iter()
            .filter(|insn| !is_declaration(insn[0]))
            .flat_map(|insn| {
                insn.windows(3)
                    .map(|w| [w[0], w[1], w[2]])
                    .collect::<Vec<_>>()
            })
            .filter(|w| w[0] & 0x000f_f000 == 0x0000_8000 && w[0] >> 20 & 3 == 2)
            .filter(|w| w[1] == 1 && w[2] == 3)
            .count()
    };
    assert_eq!(depth_row_reads(&before), 2);
    assert_eq!(depth_row_reads(&after), products);
}

/// The volume pass's pixel shader (`aobox`).
const VOLUME: &str = "sh_0462_00169680.dxbc";

const SAMPLE_L: u32 = 0x48;
const DP4: u32 = 0x11;
const DCL_CONSTANT_BUFFER: u32 = 0x59;

fn is_declaration(token: u32) -> bool {
    matches!(token & 0x7ff, 0x35 | 0x58..=0x6A | 0x91..=0xA2)
}

/// The shader's instructions, each as its tokens.
fn instructions(blob: &[u8]) -> Vec<Vec<u32>> {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let shex = dxbc.shader_chunk().expect("a shader chunk");
    let body = shex.body(blob);
    let stream = TokenStream::new(body).expect("the shader tokenizes");
    assert_eq!(stream.stage(), ShaderStage::Pixel);
    let tokens: Vec<u32> = body
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    stream
        .instructions()
        .map(|insn| {
            let insn = insn.expect("every instruction decodes");
            tokens[insn.start..insn.end].to_vec()
        })
        .collect()
}
