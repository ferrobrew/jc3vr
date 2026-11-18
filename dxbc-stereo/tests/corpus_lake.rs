use dxbc_stereo::{
    Dxbc, DxbcError, TokenStream, isolate_lake_reflection, refresh_checksum,
    reproject_lake_reflection, scale_lake_refraction,
};

mod common;
use common::shader_dir;

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
