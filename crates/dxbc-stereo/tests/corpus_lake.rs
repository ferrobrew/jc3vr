use dxbc_stereo::{
    Dxbc, DxbcError, TokenStream, isolate_lake_reflection, refresh_checksum,
    reproject_lake_reflection, scale_lake_refraction,
};

mod common;
use common::shader_dir;

#[test]
fn impostor_fade_changes_only_the_horizontal_hash_coefficient() {
    let Some(dir) = shader_dir() else { return };
    let original = std::fs::read(dir.join("sh_0723_002d1b10.dxbc")).unwrap();
    let mut expected = original.clone();
    let offsets: Vec<_> = original
        .windows(4)
        .enumerate()
        .filter_map(|(i, w)| (w == 0x3eef965bu32.to_le_bytes()).then_some(i))
        .collect();
    let &[offset] = offsets.as_slice() else {
        panic!("expected unique hash coefficient")
    };
    expected[offset..offset + 4].copy_from_slice(&0x3e6f965bu32.to_le_bytes());
    refresh_checksum(&mut expected);
    let changed = dxbc_stereo::physical_impostor_fade(&original).unwrap();
    assert_eq!(changed, expected);
    assert!(dxbc_stereo::physical_impostor_fade(&changed).is_err());
}

#[test]
fn lake_refraction_only_halves_the_horizontal_displacement() {
    let Some(dir) = shader_dir() else { return };
    let original = std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).unwrap();
    let changed = scale_lake_refraction(&original).unwrap();
    assert_eq!(original.len(), changed.len());
    let chunk = Dxbc::parse(&original).unwrap().shader_chunk().unwrap();
    let edits: Vec<_> = chunk
        .body(&original)
        .as_chunks::<4>()
        .0
        .iter()
        .zip(chunk.body(&changed).as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        edits,
        [(&(-5.0_f32).to_le_bytes(), &(-2.5_f32).to_le_bytes())]
    );
    assert_eq!(
        scale_lake_refraction(&changed),
        Err(DxbcError::NoLakeRefraction)
    );
    let mut expected = original.clone();
    expected[chunk.body_offset..chunk.body_offset + chunk.body_len]
        .copy_from_slice(chunk.body(&changed));
    refresh_checksum(&mut expected);
    assert_eq!(expected, changed);
}

#[test]
fn projected_planar_scales_only_x_before_the_lake_reflection_sample() {
    let Some(dir) = shader_dir() else { return };
    let original = std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).unwrap();
    let packed = scale_lake_refraction(&original).unwrap();
    let dxbc = Dxbc::parse(&packed).unwrap();
    let baseline = instructions(&packed);
    let sites: Vec<_> = baseline
        .iter()
        .enumerate()
        .filter(|(_, op)| op[0] & 0x7ff == 0x45 && op.ends_with(&[0x00107e46, 2, 0x00106000, 2]))
        .map(|(index, _)| index)
        .collect();
    let [sample] = sites[..] else {
        panic!("expected one t2/s2 sample")
    };
    assert_eq!(
        baseline[sample - 1],
        [0x07000000, 0x00100092, 5, 0x00100c06, 6, 0x00100c06, 5]
    );
    for (right, shift) in [(false, 0.25_f32), (true, -0.25_f32)] {
        let projected = reproject_lake_reflection(&packed, right).unwrap();
        let mut expected = baseline.clone();
        expected.insert(
            sample,
            vec![
                0x07000000,
                0x00100012,
                5,
                0x0010000a,
                5,
                0x00004001,
                shift.to_bits(),
            ],
        );
        expected.insert(
            sample - 1,
            vec![
                0x07000038,
                0x00100012,
                6,
                0x0010000a,
                6,
                0x00004001,
                0.5_f32.to_bits(),
            ],
        );
        assert_eq!(instructions(&projected), expected);
        let container = Dxbc::parse(&projected).unwrap();
        assert_eq!(dxbc.chunks().len(), container.chunks().len());
        for (a, b) in dxbc.chunks().iter().zip(container.chunks()) {
            assert_eq!(a.tag, b.tag);
            if a.tag != *b"SHEX" {
                assert_eq!(a.body(&packed), b.body(&projected));
            }
        }
        let mut checked = projected.clone();
        refresh_checksum(&mut checked);
        assert_eq!(checked, projected);
        assert_eq!(
            reproject_lake_reflection(&projected, right),
            Err(DxbcError::NoLakeReflection)
        );
    }
}

fn instructions(shader: &[u8]) -> Vec<Vec<u32>> {
    let body = Dxbc::parse(shader)
        .unwrap()
        .shader_chunk()
        .unwrap()
        .body(shader);
    let words: Vec<_> = body
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(u32::from_le_bytes)
        .collect();
    assert_eq!(words[1] as usize, words.len());
    TokenStream::new(body)
        .unwrap()
        .instructions()
        .map(|op| {
            let op = op.unwrap();
            words[op.start..op.end].to_vec()
        })
        .collect::<Vec<_>>()
}

#[test]
fn isolated_lookup_expands_only_the_packed_base_before_displacement() {
    let Some(dir) = shader_dir() else { return };
    let packed =
        scale_lake_refraction(&std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).unwrap()).unwrap();
    let original = instructions(&packed);
    let mut sites = original.windows(3).enumerate().filter(|(_, ops)| {
        ops[0] == [0x07000000, 0x00100092, 5, 0x001006a6, 6, 0x00100406, 8]
            && ops[1] == [0x07000000, 0x00100092, 5, 0x00100c06, 6, 0x00100c06, 5]
            && ops[2].ends_with(&[0x00107e46, 2, 0x00106000, 2])
    });
    let (base, _) = sites.next().unwrap();
    assert!(sites.next().is_none());
    for (right, bias) in [(false, 0.0_f32), (true, -1.0_f32)] {
        let isolated = isolate_lake_reflection(&packed, right).unwrap();
        let mut expected = original.clone();
        expected.insert(
            base + 1,
            vec![
                0x09000032,
                0x00100012,
                5,
                0x0010000a,
                5,
                0x00004001,
                2.0_f32.to_bits(),
                0x00004001,
                bias.to_bits(),
            ],
        );
        assert_eq!(instructions(&isolated), expected);
        assert_eq!(
            isolate_lake_reflection(&isolated, right),
            Err(DxbcError::NoLakeReflection)
        );
    }
}

#[test]
fn inner_edge_changes_only_the_color_sample_coordinate() {
    let Some(dir) = shader_dir() else { return };
    let pristine = std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).unwrap();
    let source =
        isolate_lake_reflection(&scale_lake_refraction(&pristine).unwrap(), false).unwrap();
    let reference = dxbc_stereo::trace_lake_response(&source).unwrap();
    let edge = dxbc_stereo::trace_lake_inner_edge(&source).unwrap();
    let instructions = |bytes: &[u8]| {
        let chunk = Dxbc::parse(bytes).unwrap().shader_chunk().unwrap();
        let stream = TokenStream::new(chunk.body(bytes)).unwrap();
        stream
            .instructions()
            .map(|op| {
                let op = op.unwrap();
                chunk.body(bytes)[4 * op.start..4 * op.end]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .copied()
                    .map(u32::from_le_bytes)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let baseline = instructions(&reference);
    let mut modified = instructions(&edge);
    let temp = modified
        .iter()
        .position(|op| op == &[0x02000068, 16])
        .unwrap();
    modified[temp][1] = 15;
    let site = modified
        .iter()
        .position(|op| {
            op == &[
                0x07000031,
                0x100012,
                15,
                0x4001,
                0.5f32.to_bits(),
                0x10000a,
                2,
            ]
        })
        .unwrap();
    assert_eq!(
        modified[site + 1].last(),
        Some(&(319.5f32 / 640.).to_bits())
    );
    assert_eq!(modified[site + 3], [0x5000036, 0x102032, 2, 0x100046, 2]);
    assert_eq!(&modified[site + 4][7..], &[0x107e46, 3, 0x106000, 2]);
    modified.drain(site..site + 3);
    assert_eq!(modified, baseline);
}

#[test]
fn candidate_depth_changes_only_the_lookup_operand() {
    let Some(dir) = shader_dir() else { return };
    let pristine = std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).unwrap();
    let source =
        isolate_lake_reflection(&scale_lake_refraction(&pristine).unwrap(), false).unwrap();
    let baseline =
        dxbc_stereo::trace_lake_displacement(&dxbc_stereo::trace_lake_inner_edge(&source).unwrap())
            .unwrap();
    let bounded = dxbc_stereo::bound_lake_candidate_depth(&baseline).unwrap();
    let mut ops = instructions(&bounded);
    let temp = ops.iter().position(|op| op == &[0x02000068, 17]).unwrap();
    ops[temp][1] = 16;
    let site = ops
        .iter()
        .position(|op| {
            op == &[
                0x07000033,
                0x100012,
                16,
                0x10000a,
                10,
                0x4001,
                (319.5_f32 / 640.).to_bits(),
            ]
        })
        .unwrap();
    assert_eq!(ops[site + 1], [0x05000036, 0x100082, 16, 0x10003a, 10]);
    assert_eq!(ops[site + 2][6], 16);
    ops[site + 2][6] = 10;
    ops.drain(site..site + 2);
    assert_eq!(ops, instructions(&baseline));
    assert!(dxbc_stereo::bound_lake_candidate_depth(&bounded).is_err());
}

#[test]
fn displacement_trace_only_appends_two_write_only_targets() {
    let Some(dir) = shader_dir() else { return };
    let pristine = std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).unwrap();
    let packed = scale_lake_refraction(&pristine).unwrap();
    for source in [
        dxbc_stereo::trace_lake_response(&pristine).unwrap(),
        dxbc_stereo::trace_lake_inner_edge(&isolate_lake_reflection(&packed, false).unwrap())
            .unwrap(),
        dxbc_stereo::trace_lake_response(&isolate_lake_reflection(&packed, true).unwrap()).unwrap(),
    ] {
        let bytes = dxbc_stereo::trace_lake_displacement(&source).unwrap();
        let mut traced = instructions(&bytes);
        let mut additions = Vec::new();
        traced.retain(|w| {
            if matches!(
                w.as_slice(),
                [0x3000065, 0x1020f2, 6 | 7]
                    | [0x5000036, 0x102032 | 0x102042 | 0x102082, 6 | 7, _, _]
            ) {
                additions.push(w.clone());
                false
            } else {
                true
            }
        });
        assert_eq!(traced, instructions(&source));
        assert_eq!(additions.len(), 8);
        assert!(dxbc_stereo::trace_lake_displacement(&bytes).is_err());
    }
}

#[test]
fn wave_trace_preserves_shading_and_only_replaces_auxiliary_outputs() {
    let Some(dir) = shader_dir() else { return };
    let pristine = std::fs::read(dir.join("sh_0710_002c71f0.dxbc")).unwrap();
    let packed = scale_lake_refraction(&pristine).unwrap();
    for response in [
        dxbc_stereo::trace_lake_response(&pristine).unwrap(),
        dxbc_stereo::trace_lake_inner_edge(&isolate_lake_reflection(&packed, false).unwrap())
            .unwrap(),
        dxbc_stereo::trace_lake_response(&isolate_lake_reflection(&packed, true).unwrap()).unwrap(),
    ] {
        let traced = dxbc_stereo::trace_lake_waves(&response).unwrap();
        let non_aux = |b: &[u8]| {
            instructions(b).into_iter().filter(|w| !matches!(w.as_slice(), [_, dst, 1 | 2 | 3 | 5 | 6 | 7, ..] if dst & 0xfff000 == 0x102000)).collect::<Vec<_>>()
        };
        assert_eq!(non_aux(&response), non_aux(&traced));
        let gradients = dxbc_stereo::trace_lake_gradients(&response).unwrap();
        assert_eq!(non_aux(&response), non_aux(&gradients));
        assert_eq!(
            instructions(&gradients)
                .iter()
                .filter(|op| matches!(op[0] & 2047, 122..=125))
                .count(),
            8
        );
        assert!(dxbc_stereo::trace_lake_waves(&traced).is_err());
    }
    assert!(dxbc_stereo::trace_lake_waves(&pristine).is_err());
}

#[test]
fn local_dof_edge_preserves_the_other_texture_samples() {
    let Some(dir) = shader_dir() else { return };
    let blob = std::fs::read(dir.join("sh_0811_003e3fe0.dxbc")).unwrap();
    let changed = dxbc_stereo::local_dof_edge(&blob).unwrap();
    let samples = |b: &[u8]| {
        let dxbc = Dxbc::parse(b).unwrap();
        let body = dxbc.shader_chunk().unwrap().body(b);
        let words: Vec<_> = body
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        let stream = TokenStream::new(body).unwrap();
        stream
            .instructions()
            .map(Result::unwrap)
            .map(|op| words[op.start..op.end].to_vec())
            .filter(|op| op[0] & 2047 == 69)
            .collect::<Vec<_>>()
    };
    let mut expected = samples(&blob);
    assert_eq!(expected.len(), 8);
    assert_eq!(expected[5][8], 9);
    expected[5][5..7].copy_from_slice(&[0x100046, 4]);
    assert_eq!(samples(&changed), expected);
    assert!(dxbc_stereo::local_dof_edge(&changed).is_err());
}

#[test]
fn local_aa_neighbor_preserves_the_other_texture_samples() {
    let Some(dir) = shader_dir() else { return };
    let blob = std::fs::read(dir.join("sh_0879_0043c590.dxbc")).unwrap();
    let changed = dxbc_stereo::local_aa_neighbor(&blob).unwrap();
    let samples = |b: &[u8]| {
        let dxbc = Dxbc::parse(b).unwrap();
        let body = dxbc.shader_chunk().unwrap().body(b);
        let words: Vec<_> = body
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        let stream = TokenStream::new(body).unwrap();
        stream
            .instructions()
            .map(Result::unwrap)
            .map(|op| words[op.start..op.end].to_vec())
            .filter(|op| op[0] & 2047 == 72)
            .collect::<Vec<_>>()
    };
    let mut expected = samples(&blob);
    assert_eq!(expected.len(), 9);
    assert_eq!(expected[6][8], 0);
    expected[6][5..7].copy_from_slice(&[0x100046, 4]);
    assert_eq!(samples(&changed), expected);
    assert!(dxbc_stereo::local_aa_neighbor(&changed).is_err());
}
