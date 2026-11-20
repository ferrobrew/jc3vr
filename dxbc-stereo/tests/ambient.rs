//! Corpus tests for the ambient-occlusion generation rewrite, run against the game's extracted shader
//! bundle.

use dxbc_stereo::{
    AMBIENT_OCCLUSION_CB_REGISTER, AmbientOcclusionSampling, Dxbc, DxbcError, ShaderStage,
    TokenStream, localize_ambient_occlusion, localize_ambient_occlusion_filter,
    snap_ambient_occlusion_uv,
};

mod common;
use common::shader_dir;

/// The generation pass and its NVIDIA twin are rewritten, along with their discontinuity-mask
/// variants (`ssao_discontinuitymask*`, which share the layout and which the block never binds), the
/// rewrite re-parses, and every other shader is declined.
#[test]
fn corpus_rewrite_covers_exactly_the_generation_pass() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    for sampling in [
        AmbientOcclusionSampling::Exact,
        AmbientOcclusionSampling::DoubleWide,
    ] {
        let mut rewritten = Vec::new();
        for path in &paths {
            let blob = std::fs::read(path).expect("read a shader");
            match localize_ambient_occlusion(&blob, sampling) {
                Ok(out) => {
                    instructions(&out);
                    rewritten.push(path.file_name().expect("a file name").to_owned());
                }
                Err(DxbcError::NoAmbientOcclusion) => {}
                Err(error) => panic!("{}: {error}", path.display()),
            }
        }
        assert_eq!(
            rewritten,
            [
                "sh_0620_00240810.dxbc",
                "sh_0627_0024c410.dxbc",
                "sh_0628_00251390.dxbc",
                "sh_0631_00256ab0.dxbc"
            ],
            "the rewritten shaders are the `ssao` family ({sampling:?})"
        );
    }
}

/// Every depth and normal sample reads a clamped coordinate, every tap's offset is halved once, and
/// the basis and rotation rows come from the eye's block of the mod-owned buffer.
#[test]
fn generation_pass_reads_each_eye_from_its_own_half() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0631_00256ab0.dxbc")).expect("read the shader");
    let before = instructions(&blob);
    let after = instructions(
        &localize_ambient_occlusion(&blob, AmbientOcclusionSampling::DoubleWide)
            .expect("rewritten"),
    );

    let samples = |insns: &[Vec<u32>]| {
        insns
            .iter()
            .filter(|insn| insn[0] & 0x7ff == SAMPLE_L)
            .count()
    };
    assert_eq!(samples(&before), samples(&after));
    let taps = samples(&before) - 2;

    // The buffer declaration and the prologue, then per tap `add`, `mad`, and the three-instruction
    // clamp; the centre and normal samples only clamp.
    let added = after.len() - before.len();
    let prologue = 1 + 13;
    assert_eq!(added, prologue + 2 * taps + 3 * samples(&before));

    let clamped_coordinate = 12; // `dcl_temps 9` plus the three ahead of the coordinate temporary.
    for insn in after.iter().filter(|insn| insn[0] & 0x7ff == SAMPLE_L) {
        let extended = insn
            .iter()
            .skip(1)
            .take_while(|t| *t & 1 << 31 != 0)
            .count();
        let coordinate = 2 + extended + 2;
        assert_eq!(
            &insn[coordinate..coordinate + 2],
            &[0x0010_0046, clamped_coordinate]
        );
    }

    let reads_cb = |insns: &[Vec<u32>], register: u32| {
        insns
            .iter()
            .flat_map(|insn| insn.windows(2))
            .filter(|w| w[0] & 0x000f_f000 == 0x0000_8000 && w[1] == register)
            .count()
    };
    assert!(reads_cb(&after, AMBIENT_OCCLUSION_CB_REGISTER) > 4);
    let mentions_v0 = |insns: &[Vec<u32>]| {
        insns
            .iter()
            .filter(|insn| !is_declaration(insn[0]))
            .filter(|insn| {
                insn.windows(2)
                    .any(|w| w[0] & 0x000f_f000 == 0x1000 && w[0] >> 20 & 3 == 1 && w[1] == 0)
            })
            .count()
    };
    assert_eq!(mentions_v0(&before), 2);
    // Only the prologue still reads the pixel position.
    assert_eq!(mentions_v0(&after), 2);
}

/// Under the exact sampling the shader reads the interpolated screen UV only to tell the eyes apart
/// and to size the seam clamp; every tap, reconstruction, and centre sample reads the UV computed from
/// the pixel position, and every sample reads the clamped coordinate.
#[test]
fn exact_generation_reads_the_interpolated_uv_only_in_its_prologue() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0631_00256ab0.dxbc")).expect("read the shader");
    let before = instructions(&blob);
    let after = instructions(
        &localize_ambient_occlusion(&blob, AmbientOcclusionSampling::Exact).expect("rewritten"),
    );
    let reads_v1 = |insns: &[Vec<u32>]| {
        insns
            .iter()
            .filter(|insn| !is_declaration(insn[0]))
            .filter(|insn| {
                insn.windows(2)
                    .any(|w| w[0] & 0x000f_f000 == 0x1000 && w[0] >> 20 & 3 == 1 && w[1] == 1)
            })
            .count()
    };
    assert!(reads_v1(&before) > 2);
    // The prologue's `ge` (the eye) and `deriv_rtx` (the clamp's inset).
    assert_eq!(reads_v1(&after), 2);
    let clamped_coordinate = 12; // `dcl_temps 9` plus the three ahead of the coordinate temporary.
    let samples: Vec<_> = after
        .iter()
        .filter(|insn| insn[0] & 0x7ff == SAMPLE_L)
        .collect();
    assert_eq!(
        samples.len(),
        before
            .iter()
            .filter(|insn| insn[0] & 0x7ff == SAMPLE_L)
            .count()
    );
    for insn in samples {
        let extended = insn
            .iter()
            .skip(1)
            .take_while(|t| *t & 1 << 31 != 0)
            .count();
        let coordinate = 2 + extended + 2;
        assert_eq!(
            &insn[coordinate..coordinate + 2],
            &[0x0010_0046, clamped_coordinate]
        );
    }
    // The sizes come from the depth texture, not from the UV's derivatives alone.
    assert!(after.iter().any(|insn| insn[0] & 0x7ff == RESINFO));
}

/// The arithmetic the exact sampling relies on for its taps: a tap placed in the eye's own UV and
/// halved onto the left half is the eye-sized target's tap scaled by one half, to the bit, at every
/// pixel centre of a half-resolution eye; the double-wide form, which halves the offset after adding
/// it to the double-wide UV, is not.
#[test]
fn exact_taps_are_the_eye_sized_taps_halved() {
    let width = 1792u32;
    let offsets = [0.013f32, -0.0071, 0.0402, -0.1137, 0.00093];
    let mut double_wide_mismatches = 0;
    for x in 0..width {
        let eye_uv = (x as f32 + 0.5) / width as f32;
        let wide_uv = eye_uv * 0.5;
        for offset in offsets {
            let eye_sized = (eye_uv + offset) * width as f32;
            let exact = (eye_uv + offset).mul_add(0.5, 0.0) * (2 * width) as f32;
            assert_eq!(
                exact.to_bits(),
                eye_sized.to_bits(),
                "x {x} offset {offset}"
            );
            let tap = wide_uv + offset;
            let double_wide = (tap - wide_uv).mul_add(0.5, wide_uv) * (2 * width) as f32;
            if double_wide.to_bits() != eye_sized.to_bits() {
                double_wide_mismatches += 1;
            }
        }
    }
    assert!(double_wide_mismatches > 0);
}

/// The bilateral blur and the composite are the only shaders the filter rewrite takes, and each of
/// their source samples reads a clamped coordinate.
#[test]
fn filter_rewrite_covers_exactly_the_blur_and_composite() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let mut rewritten = Vec::new();
    for path in paths {
        let blob = std::fs::read(&path).expect("read a shader");
        match localize_ambient_occlusion_filter(&blob, false) {
            Ok(out) => {
                let (before, after) = (instructions(&blob), instructions(&out));
                let samples = before
                    .iter()
                    .filter(|insn| insn[0] & 0x7ff == SAMPLE_L)
                    .count();
                // The prologue is seven instructions; each clamped sample adds three.
                let clamped = (after.len() - before.len() - 7) / 3;
                assert!(clamped >= 1 && clamped <= samples, "{}", path.display());
                rewritten.push((path.file_name().expect("a file name").to_owned(), clamped));
            }
            Err(DxbcError::NoAmbientOcclusion) => {}
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    let names: Vec<_> = rewritten.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(
        names,
        ["sh_0622_002461a0.dxbc", "sh_0623_002464f0.dxbc"],
        "the rewritten shaders are `ssao_apply` and `ssao_blur`"
    );
    // The composite clamps its occlusion sample and not the occlusion-volume one; the blur clamps
    // all nine taps.
    assert_eq!(rewritten[0].1, 1);
    assert_eq!(rewritten[1].1, 9);
}

const SAMPLE_L: u32 = 0x48;
const RESINFO: u32 = 0x3d;

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

/// The snap takes exactly the occlusion passes, and each re-parses with the pixel position declared.
#[test]
fn snap_covers_exactly_the_occlusion_passes() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let mut passes = Vec::new();
    for path in paths {
        let blob = std::fs::read(&path).expect("read a shader");
        match snap_ambient_occlusion_uv(&blob) {
            Ok(out) => {
                let after = instructions(&out);
                assert!(
                    after
                        .iter()
                        .any(|insn| insn[0] & 0x7ff == 0x64 && insn.last() == Some(&1)),
                    "{}: the pixel position is declared",
                    path.display()
                );
                passes.push(path.file_name().expect("a file name").to_owned());
            }
            Err(DxbcError::NoAmbientOcclusion) => {}
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    // The generation family, the composite, and the blur.
    assert_eq!(
        passes,
        [
            "sh_0620_00240810.dxbc",
            "sh_0622_002461a0.dxbc",
            "sh_0623_002464f0.dxbc",
            "sh_0627_0024c410.dxbc",
            "sh_0628_00251390.dxbc",
            "sh_0631_00256ab0.dxbc"
        ]
    );
}
