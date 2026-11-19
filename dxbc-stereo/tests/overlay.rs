//! Corpus tests for the composite overlay rewrite, run against the game's extracted shader bundle.

use dxbc_stereo::{Dxbc, DxbcError, ShaderStage, TokenStream, localize_screen_overlays};

mod common;
use common::shader_dir;

/// Every pixel shader that binds the vignette or the lens dirt is rewritten, the rewrite re-parses,
/// and no other shader is touched.
#[test]
fn corpus_overlay_rewrite_covers_every_composite_permutation() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let contains = |blob: &[u8], name: &[u8]| blob.windows(name.len()).any(|w| w == name);
    let (mut rewritten, mut declined_with_overlay) = (0, Vec::new());
    for path in paths {
        let blob = std::fs::read(&path).expect("read a shader");
        let binds = contains(&blob, b"EdgeFadeTexture\0") || contains(&blob, b"LensDirtTexture\0");
        match localize_screen_overlays(&blob) {
            Ok(out) => {
                assert!(binds, "{}: rewritten without an overlay", path.display());
                let dxbc = Dxbc::parse(&out).expect("the rewrite re-parses");
                let shex = dxbc
                    .shader_chunk()
                    .expect("the rewrite keeps its shader chunk");
                let stream = TokenStream::new(shex.body(&out)).expect("the rewrite re-tokenizes");
                assert_eq!(stream.stage(), ShaderStage::Pixel);
                for insn in stream.instructions() {
                    insn.expect("every rewritten instruction decodes");
                }
                rewritten += 1;
            }
            Err(DxbcError::NoScreenOverlay) if binds => {
                declined_with_overlay.push(path.file_name().expect("a file name").to_owned());
            }
            Err(_) => assert!(!binds, "{}: failed with an overlay", path.display()),
        }
    }
    assert!(
        declined_with_overlay.is_empty(),
        "overlay shaders the rewrite declined: {declined_with_overlay:?}",
    );
    assert_eq!(rewritten, 61, "composite permutations rewritten");
}
