//! Corpus tests for the eye-half sample clamp, run against the game's extracted shader bundle.

use dxbc_stereo::{
    Dxbc, DxbcError, ShaderStage, TokenStream, localize_composite_bloom, localize_post_filter,
    localize_screen_overlays,
};

mod common;
use common::shader_dir;

/// The post-filter rewrite takes every program the glare chain draws with, and the handful of other
/// programs that bind only the same generic source names, and every result re-parses.
#[test]
fn corpus_post_filter_covers_the_glare_chain() {
    let rewritten = rewritten_by(localize_post_filter, DxbcError::NoPostFilter);
    assert_eq!(
        rewritten,
        [
            "sh_0455_00167e90.dxbc", // 2dtex1
            "sh_0456_00168200.dxbc", // 2dtex2
            "sh_0457_00168630.dxbc", // 2dtexsimple: the chain's copy and upsample
            "sh_0458_001688e0.dxbc", // 2dtexsimpledepth
            "sh_0459_00168b90.dxbc", // 2dtexsimplenoalpha
            "sh_0460_00168e70.dxbc", // 2dtexsimpleoffset
            "sh_0469_00173e50.dxbc", // bloomdilation_3x3
            "sh_0470_00174440.dxbc", // bloomerosion_3x3
            "sh_0471_00174c50.dxbc", // blur
            "sh_0473_001757f0.dxbc", // blur_1dgaussian_17sample
            "sh_0474_00175f50.dxbc", // blur_1dsaw_17sample
            "sh_0497_0019df70.dxbc", // compositebloom
            "sh_0526_001c6590.dxbc", // downscale2x2
            "sh_0527_001c6a80.dxbc", // downscale4x4
            "sh_0561_001f03b0.dxbc", // highpass
            "sh_0637_0025d7c0.dxbc", // ssreflectiondilation_3x3
            "sh_0639_0025e790.dxbc", // samplelumfinal
            "sh_0640_0025f9f0.dxbc", // sampleluminitial
            "sh_0652_00269750.dxbc", // simpleblend
            "sh_0675_002875e0.dxbc", // waterbumpcomposite
            "sh_0684_002a8630.dxbc", // quickui
            "sh_0854_0041a1e0.dxbc", // highpass_pg
            "sh_0855_0041a680.dxbc", // highpass_rh
            "sh_0856_0041ab20.dxbc", // highpass_u2
        ]
    );
}

/// The composite-bloom rewrite takes every composite permutation that binds a bloom texture, and
/// nothing else.
#[test]
fn corpus_composite_bloom_covers_every_composite_permutation() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let rewritten = rewritten_by(localize_composite_bloom, DxbcError::NoCompositeBloom);
    let binders: Vec<String> = sorted_paths(&dir)
        .into_iter()
        .filter(|path| {
            let blob = std::fs::read(path).expect("read a shader");
            [&b"BloomTexture\0"[..], b"SecondaryBloomTexture\0"]
                .iter()
                .any(|name| blob.windows(name.len()).any(|w| w == *name))
        })
        .map(|path| file_name(&path))
        .collect();
    assert_eq!(rewritten, binders);
    assert_eq!(rewritten.len(), 61, "composite permutations rewritten");
    // The payload applies it on top of the overlay rewrite, which every one of them also takes.
    for name in &rewritten {
        let blob = std::fs::read(dir.join(name)).expect("read a shader");
        let overlays = localize_screen_overlays(&blob).expect("the overlay rewrite applies");
        instructions(&localize_composite_bloom(&overlays).expect("the bloom rewrite stacks"));
    }
}

/// The chain's separable blur reads every tap through the eye's clamp: each of its samples (nine
/// bilinear taps standing in for seventeen texels) is preceded by the measure-and-clamp sequence and
/// reads the clamped coordinate.
#[test]
fn blur_reads_every_tap_inside_the_eye() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0473_001757f0.dxbc")).expect("read the shader");
    let before = instructions(&blob);
    let after = instructions(&localize_post_filter(&blob).expect("rewritten"));
    let samples = |insns: &[Vec<u32>]| insns.iter().filter(|i| opcode(i) == SAMPLE).count();
    assert_eq!(samples(&before), 9);
    assert_eq!(samples(&after), 9);
    // The prologue is two instructions, and each sample adds the eight of its clamp.
    assert_eq!(after.len() - before.len(), 2 + 8 * 9);
    // `dcl_temps 3`, so the coordinate temporary is the third of the four added.
    let clamped = 5;
    for (index, insn) in after.iter().enumerate() {
        if opcode(insn) != SAMPLE {
            continue;
        }
        let ops: Vec<u32> = after[index - 8..index].iter().map(|i| opcode(i)).collect();
        assert_eq!(ops, [RESINFO, DIV, ADD, ADD, ADD, MAX, MIN, MOV]);
        let coordinate = 1 + extended_tokens(insn) + 2;
        assert_eq!(&insn[coordinate..coordinate + 2], &[0x0010_0046, clamped]);
    }
}

/// The erosion's texel loads are masked to the eye's half: every load is followed by an `and` of its
/// result with the in-half mask, and the plain copy, which declares no temporaries, gains them.
#[test]
fn loads_are_zeroed_outside_the_eye_and_temps_are_declared() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0470_00174440.dxbc")).expect("read the erosion");
    let after = instructions(&localize_post_filter(&blob).expect("rewritten"));
    let loads: Vec<usize> = (0..after.len())
        .filter(|&i| opcode(&after[i]) == LD)
        .collect();
    assert_eq!(loads.len(), 9);
    for index in loads {
        assert_eq!(opcode(&after[index + 1]), AND);
        let ops: Vec<u32> = after[index - 7..index].iter().map(|i| opcode(i)).collect();
        assert_eq!(ops, [RESINFO, USHR, AND, IADD, IGE, ILT, AND]);
    }

    let copy = std::fs::read(dir.join("sh_0457_00168630.dxbc")).expect("read the copy");
    assert!(!instructions(&copy).iter().any(|i| opcode(i) == DCL_TEMPS));
    let after = instructions(&localize_post_filter(&copy).expect("rewritten"));
    let temps = after
        .iter()
        .find(|i| opcode(i) == DCL_TEMPS)
        .expect("temporaries declared");
    assert_eq!(temps[1], 4);
}

const DCL_TEMPS: u32 = 0x68;
const ADD: u32 = 0x00;
const AND: u32 = 0x01;
const DIV: u32 = 0x0e;
const IADD: u32 = 0x1e;
const IGE: u32 = 0x21;
const ILT: u32 = 0x22;
const LD: u32 = 0x2d;
const MIN: u32 = 0x33;
const MAX: u32 = 0x34;
const MOV: u32 = 0x36;
const RESINFO: u32 = 0x3d;
const SAMPLE: u32 = 0x45;
const USHR: u32 = 0x55;

fn opcode(insn: &[u32]) -> u32 {
    insn[0] & 0x7ff
}

/// The number of extended opcode tokens after the opcode token: each carries a continuation bit
/// for the one after it.
fn extended_tokens(insn: &[u32]) -> usize {
    let mut count = 0;
    let mut token = insn[0];
    while token & 1 << 31 != 0 {
        count += 1;
        token = insn[count];
    }
    count
}

/// The corpus shaders `rewrite` accepts, failing on any error other than its `decline`.
fn rewritten_by(
    rewrite: fn(&[u8]) -> Result<Vec<u8>, DxbcError>,
    decline: DxbcError,
) -> Vec<String> {
    let Some(dir) = shader_dir() else {
        return Vec::new();
    };
    let mut rewritten = Vec::new();
    for path in sorted_paths(&dir) {
        let blob = std::fs::read(&path).expect("read a shader");
        match rewrite(&blob) {
            Ok(out) => {
                instructions(&out);
                rewritten.push(file_name(&path));
            }
            Err(error) if error == decline => {}
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    rewritten
}

fn sorted_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    paths
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .expect("a file name")
        .to_string_lossy()
        .into_owned()
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
