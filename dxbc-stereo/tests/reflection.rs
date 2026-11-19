//! Corpus tests for the screen-space reflection ray-march and blur rewrites, run against the game's
//! extracted shader bundle.

use dxbc_stereo::{
    Dxbc, DxbcError, LocalLights, SCREEN_SPACE_REFLECTION_CB_REGISTER,
    SCREEN_SPACE_REFLECTION_ROWS_PER_EYE, ShaderStage, TokenStream, localize_eye_camera,
    localize_reflection_blur, localize_screen_space_reflection,
};

mod common;
use common::shader_dir;

/// Every shader in the corpus `rewrite` accepts, re-parsed.
fn accepted(rewrite: fn(&[u8]) -> Result<Vec<u8>, DxbcError>, decline: DxbcError) -> Vec<String> {
    let Some(dir) = shader_dir() else {
        return Vec::new();
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let mut rewritten = Vec::new();
    for path in paths {
        let blob = std::fs::read(&path).expect("read a shader");
        match rewrite(&blob) {
            Ok(out) => {
                instructions(&out);
                rewritten.push(
                    path.file_name()
                        .expect("a file name")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            Err(error) if error == decline => {}
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    rewritten
}

/// The ray march and its simpler permutation are rewritten, and every other shader is declined.
#[test]
fn corpus_rewrite_covers_exactly_the_ray_march() {
    let rewritten = accepted(
        localize_screen_space_reflection,
        DxbcError::NoScreenSpaceReflection,
    );
    if shader_dir().is_none() {
        return;
    }
    assert_eq!(
        rewritten,
        ["sh_0644_00261350.dxbc", "sh_0645_002632d0.dxbc"],
        "the rewritten shaders are `screenspaceraytracereflection` and `screenspacereflection`"
    );
}

/// Both blur programs are rewritten, and every other shader is declined.
#[test]
fn corpus_blur_rewrite_covers_exactly_the_reflection_blur() {
    let rewritten = accepted(localize_reflection_blur, DxbcError::NoReflectionBlur);
    if shader_dir().is_none() {
        return;
    }
    assert_eq!(
        rewritten,
        ["sh_0646_002648b0.dxbc", "sh_0647_002651d0.dxbc"],
        "the rewritten shaders are the two `screenspacereflectionblur` programs"
    );
}

/// The ray march reads no interpolated view ray, no shared matrix row, and no shared target size
/// after the rewrite: each comes from the eye's block or the eye's halved size, and every depth
/// sample away from the screen UV is moved into the eye's half.
#[test]
fn ray_march_reads_each_eye_from_its_own_block() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0644_00261350.dxbc")).expect("read the shader");
    let before = instructions(&blob);
    let rewritten = localize_screen_space_reflection(&blob).expect("rewritten");
    let after = instructions(&rewritten);

    let ray_input = 2;
    let reads = |insns: &[Vec<u32>], f: &dyn Fn(&[u32]) -> bool| -> usize {
        insns
            .iter()
            .filter(|insn| !is_declaration(insn[0] & 0x7ff))
            .map(|insn| operands(insn).iter().filter(|o| f(o)).count())
            .sum()
    };
    let is_ray = |o: &[u32]| (o[0] >> 12) & 0xff == 1 && o.last() == Some(&ray_input);
    let shared_row = |o: &[u32]| {
        (o[0] >> 12) & 0xff == 8
            && (o[0] >> 22) & 0x1ff == 0
            && matches!(o[1..], [1, 0..=7] | [2, 0..=12])
    };
    let eye_row = |o: &[u32]| (o[0] >> 12) & 0xff == 8 && o.get(1) == Some(&13);
    assert!(reads(&before, &is_ray) > 0 && reads(&before, &shared_row) > 0);
    assert_eq!(reads(&after, &is_ray), 0, "no read of the interpolated ray");
    // The one shared read left is the prologue's halving of the target size.
    assert_eq!(reads(&after, &shared_row), 1, "no shared matrix row");
    assert!(reads(&after, &eye_row) >= reads(&before, &shared_row) - 4);

    let dcl = after
        .iter()
        .find(|insn| insn[0] & 0x7ff == 0x59 && insn[2] == SCREEN_SPACE_REFLECTION_CB_REGISTER)
        .expect("the eye block's declaration");
    assert_eq!(dcl[3], 2 * SCREEN_SPACE_REFLECTION_ROWS_PER_EYE);

    // The march's depth sample reads a moved coordinate; the centre sample keeps the screen UV.
    let depth_samples: Vec<_> = after
        .iter()
        .filter(|insn| insn[0] & 0x7ff == 0x48 && operands(insn)[2][1] == 2)
        .collect();
    assert_eq!(depth_samples.len(), 2);
    assert_eq!((operands(depth_samples[0])[1][0] >> 12) & 0xff, 1);
    assert_eq!(operands(depth_samples[1])[1], [0x0010_0046, 15]);
}

/// The simpler permutation keeps its tuning row and its depth samples: it reads no target size, so it
/// marches over the double-wide target's UV through the remapped view-projection.
#[test]
fn simple_permutation_keeps_its_tuning_and_samples() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0645_002632d0.dxbc")).expect("read the shader");
    let before = instructions(&blob);
    let after = instructions(&localize_screen_space_reflection(&blob).expect("rewritten"));
    let tuning = |insns: &[Vec<u32>]| {
        insns
            .iter()
            .filter(|insn| !is_declaration(insn[0] & 0x7ff))
            .flat_map(|insn| operands(insn))
            .filter(|o| (o[0] >> 12) & 0xff == 8 && (o[0] >> 22) & 0x1ff == 0 && o[1] == 2)
            .count()
    };
    assert!(tuning(&before) > 0);
    assert_eq!(tuning(&after), tuning(&before));
    let depth_coordinates = |insns: &[Vec<u32>]| -> Vec<Vec<u32>> {
        insns
            .iter()
            .filter(|insn| insn[0] & 0x7ff == 0x48 && operands(insn)[2][1] == 2)
            .map(|insn| operands(insn)[1].clone())
            .collect()
    };
    assert_eq!(depth_coordinates(&after), depth_coordinates(&before));
}

/// The eye-camera rewrite, which the payload applies over every rewrite, still takes the ray march.
#[test]
fn eye_camera_composes_with_the_ray_march() {
    let Some(dir) = shader_dir() else {
        return;
    };
    for name in ["sh_0644_00261350.dxbc", "sh_0645_002632d0.dxbc"] {
        let blob = std::fs::read(dir.join(name)).expect("read the shader");
        let rewritten = localize_screen_space_reflection(&blob).expect("rewritten");
        let both = localize_eye_camera(&rewritten, LocalLights::PerEye)
            .expect("the camera rewrite composes");
        instructions(&both);
    }
}

/// Every blur sample reads a coordinate clamped into the eye's half.
#[test]
fn blur_clamps_every_sample() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0647_002651d0.dxbc")).expect("read the shader");
    let before = instructions(&blob);
    let after = instructions(&localize_reflection_blur(&blob).expect("rewritten"));
    let samples = |insns: &[Vec<u32>]| {
        insns
            .iter()
            .filter(|insn| matches!(insn[0] & 0x7ff, 0x45 | 0x48))
            .count()
    };
    assert_eq!(samples(&before), 5);
    assert_eq!(samples(&after), 5);
    // Each sample gains the seven-instruction clamp in front of it.
    let prologue = 2;
    assert_eq!(after.len() - before.len(), prologue + 8 * samples(&before));
}

fn is_declaration(opcode: u32) -> bool {
    matches!(opcode, 0x35 | 0x58..=0x6A | 0x91..=0xA2)
}

/// The instructions of a pixel shader's token stream, each as its tokens.
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
    assert_eq!(
        tokens[1] as usize,
        tokens.len(),
        "the declared length is the stream length"
    );
    stream
        .instructions()
        .map(|insn| {
            let insn = insn.expect("every instruction decodes");
            tokens[insn.start..insn.end].to_vec()
        })
        .collect()
}

/// The operands of one instruction, each as its tokens.
fn operands(insn: &[u32]) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    let mut at = 1;
    let mut extended = insn[0] >> 31 != 0;
    while extended {
        extended = insn[at] >> 31 != 0;
        at += 1;
    }
    while at < insn.len() {
        let start = at;
        let token = insn[at];
        at += 1;
        if token >> 31 != 0 {
            at += 1;
        }
        let dimensions = (token >> 20) & 0x3;
        if (token >> 12) & 0xff == 4 {
            at += match token & 0x3 {
                1 => 1,
                2 => 4,
                _ => 0,
            };
        }
        for dimension in 0..dimensions {
            let representation = (token >> (22 + 3 * dimension)) & 0x7;
            at += match representation {
                0 => 1,
                3 => 1 + relative_length(insn, at + 1),
                _ => panic!("unexpected index representation {representation}"),
            };
        }
        out.push(insn[start..at].to_vec());
    }
    out
}

/// The length of a relative index operand starting at `at`.
fn relative_length(insn: &[u32], at: usize) -> usize {
    2 + usize::from(insn[at] >> 31 != 0)
}
