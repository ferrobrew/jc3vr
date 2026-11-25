//! Corpus tests for the remap's per-eye clip origin, run against the game's extracted shader bundle.

use dxbc_stereo::{
    CLIP_ORIGIN_CB0_ROW, CLIP_ORIGIN_ROW_BASE, ClipOrigin, Dxbc, OperandKind,
    STEREO_ORIGIN_CB_ROWS, TokenStream, patch_vertex_shader, patch_vertex_shader_with,
};

mod common;
use common::shader_dir;

/// The per-eye origin changes exactly the remapped shaders that subtract the render camera's position
/// before the view-projection rows without reading `cb0[4]` (the road surfaces and the `roadbias`
/// model permutations): in each of them every `cb0[9]` read becomes the eye's copy and the buffer is
/// declared through eye 1's copy. Every other shader the remap takes is rewritten exactly as without
/// it.
#[test]
fn corpus_origin_covers_the_shaders_without_a_camera_row() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let mut changed = 0;
    for path in paths {
        let blob = std::fs::read(&path).expect("read a shader");
        let Ok(unchanged) = patch_vertex_shader(&blob) else {
            assert!(
                patch_vertex_shader_with(&blob, ClipOrigin::PerEye).is_err(),
                "{}",
                path.display()
            );
            continue;
        };
        assert_eq!(
            patch_vertex_shader_with(&blob, ClipOrigin::Unchanged).expect("remapped"),
            unchanged
        );
        let per_eye = patch_vertex_shader_with(&blob, ClipOrigin::PerEye).expect("remapped");
        let rows = cb0_rows(&blob);
        let expected = rows.contains(&CLIP_ORIGIN_CB0_ROW)
            && !rows.contains(&4)
            && rows.iter().any(|row| (29..=32).contains(row));
        if !expected {
            assert_eq!(per_eye, unchanged, "{}", path.display());
            continue;
        }
        changed += 1;
        assert!(
            !cb0_rows(&per_eye).contains(&CLIP_ORIGIN_CB0_ROW),
            "{}",
            path.display()
        );
        let (declared, origin_reads) = stereo_buffer(&per_eye);
        assert_eq!(declared, Some(STEREO_ORIGIN_CB_ROWS), "{}", path.display());
        assert_eq!(
            origin_reads,
            cb0_rows(&blob)
                .iter()
                .filter(|row| **row == CLIP_ORIGIN_CB0_ROW)
                .count(),
            "{}",
            path.display()
        );
    }
    assert_eq!(changed, ORIGIN_SHADERS);
}

/// `layeredroad` and the 31 `generalmkiii*roadbias` permutations.
const ORIGIN_SHADERS: usize = 32;

/// Every immediately indexed `cb0` row a shader's instructions read, with repeats.
fn cb0_rows(blob: &[u8]) -> Vec<u32> {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
    let stream = TokenStream::new(body).expect("the shader tokenizes");
    let mut rows = Vec::new();
    for insn in stream.instructions() {
        let insn = insn.expect("every instruction decodes");
        if insn.is_declaration() {
            continue;
        }
        for operand in insn.operands() {
            if let OperandKind::ConstantBuffer {
                register: 0,
                element,
            } = operand.expect("every operand decodes").kind
            {
                rows.push(element);
            }
        }
    }
    rows
}

/// The rows the shader declares `cb13` with, and how many of its reads address the origin row
/// relative to the eye's row base.
fn stereo_buffer(blob: &[u8]) -> (Option<u32>, usize) {
    let dxbc = Dxbc::parse(blob).expect("the shader parses");
    let body = dxbc.shader_chunk().expect("a shader chunk").body(blob);
    let tokens: Vec<u32> = body
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    let stream = TokenStream::new(body).expect("the shader tokenizes");
    let mut declared = None;
    let mut reads = 0;
    for insn in stream.instructions() {
        let insn = insn.expect("every instruction decodes");
        let tokens = &tokens[insn.start..insn.end];
        if insn.opcode == DCL_CONSTANT_BUFFER && tokens[2] == 13 {
            declared = Some(tokens[3]);
            continue;
        }
        // A relatively indexed `cb13` operand token, then its extended token when it carries a
        // modifier, then the slot and the row.
        let relative = |token: u32| token >> 12 & 0xff == 8 && token >> 25 & 7 == 3;
        reads += (2..tokens.len().saturating_sub(1))
            .filter(|&at| tokens[at] == 13 && tokens[at + 1] == CLIP_ORIGIN_ROW_BASE)
            .filter(|&at| {
                relative(tokens[at - 1]) || (relative(tokens[at - 2]) && tokens[at - 2] >> 31 == 1)
            })
            .count();
    }
    (declared, reads)
}

const DCL_CONSTANT_BUFFER: u32 = 0x59;
