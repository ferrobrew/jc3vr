//! Corpus tests for the planar water reflection rewrite, run against the game's extracted shader
//! bundle.

use dxbc_stereo::{Dxbc, DxbcError, ShaderStage, TokenStream, localize_planar_reflection};

mod common;
use common::shader_dir;

/// Every pixel shader that binds the planar reflection is rewritten for both eyes, the rewrite
/// re-parses with exactly one more instruction, and no other shader is touched.
#[test]
fn corpus_planar_rewrite_covers_every_water_permutation() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let contains = |blob: &[u8], name: &[u8]| blob.windows(name.len()).any(|w| w == name);
    let count = |blob: &[u8]| {
        let dxbc = Dxbc::parse(blob).expect("the shader parses");
        let shex = dxbc.shader_chunk().expect("a shader chunk");
        let stream = TokenStream::new(shex.body(blob)).expect("the shader tokenizes");
        assert_eq!(stream.stage(), ShaderStage::Pixel);
        stream
            .instructions()
            .inspect(|insn| assert!(insn.is_ok(), "every instruction decodes"))
            .count()
    };
    let (mut rewritten, mut declined) = (0, Vec::new());
    for path in paths {
        let blob = std::fs::read(&path).expect("read a shader");
        let binds = contains(&blob, b"g_ReflectionTexture\0");
        for right in [false, true] {
            match localize_planar_reflection(&blob, right) {
                Ok(out) => {
                    assert!(binds, "{}: rewritten without a reflection", path.display());
                    assert_eq!(count(&out), count(&blob) + 1, "{}", path.display());
                    rewritten += 1;
                }
                Err(DxbcError::NoPlanarReflection) if binds => {
                    declined.push(path.file_name().expect("a file name").to_owned());
                }
                Err(_) => assert!(!binds, "{}: failed with a reflection", path.display()),
            }
        }
    }
    assert!(
        declined.is_empty(),
        "reflection shaders declined: {declined:?}"
    );
    assert_eq!(rewritten, 8, "water permutations rewritten, both eyes");
}

/// The lake permutation's reflection division gains the eye-local numerator `2u - eye` and keeps
/// its divisor.
#[test]
fn lake_division_takes_the_eye_local_numerator() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let blob = std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).expect("read the lake shader");
    for (right, bias) in [(false, 0f32), (true, -1f32)] {
        let out = localize_planar_reflection(&blob, right).expect("the lake shader is rewritten");
        let shex = Dxbc::parse(&out)
            .expect("parses")
            .shader_chunk()
            .expect("chunk");
        let body = shex.body(&out);
        let stream = TokenStream::new(body).expect("tokenizes");
        let tokens: Vec<u32> = body
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        let insns: Vec<_> = stream.instructions().map(|i| i.expect("decodes")).collect();
        let at = insns
            .iter()
            .position(|i| i.opcode == 0x0e && tokens[i.end - 2] == 1 && tokens[i.end - 1] == 14)
            .expect("the division by g_UVScale.x");
        let (mad, div) = (&insns[at - 1], &insns[at]);
        // mad r8.x, r5.y, l(2.0), l(bias)
        assert_eq!(
            &tokens[mad.start..mad.end],
            &[
                0x0900_0032,
                0x0010_0012,
                8,
                0x0010_001a,
                5,
                0x4001,
                2f32.to_bits(),
                0x4001,
                bias.to_bits()
            ]
        );
        // div r8.x, r8.x, cb1[14].x
        assert_eq!(
            &tokens[div.start..div.end],
            &[
                0x0800_000e,
                0x0010_0012,
                8,
                0x0010_000a,
                8,
                0x0020_800a,
                1,
                14
            ]
        );
    }
}
