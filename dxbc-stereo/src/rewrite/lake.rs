use crate::{
    Dxbc, DxbcError, ShaderStage, TokenStream,
    rewrite::common::{
        SignatureElement, append_signature_element, max_signature_register, reassemble,
    },
};

pub fn offset_lake_reflection(blob: &[u8], right: bool) -> Result<Vec<u8>, DxbcError> {
    edit_planar(blob, right, PlanarLookup::Shift)
}

pub fn reproject_lake_reflection(blob: &[u8], right: bool) -> Result<Vec<u8>, DxbcError> {
    edit_planar(blob, right, PlanarLookup::Project)
}

pub fn mirror_lake_reflection(blob: &[u8], right: bool) -> Result<Vec<u8>, DxbcError> {
    edit_planar(blob, right, PlanarLookup::Mirror)
}

pub fn isolate_lake_reflection(blob: &[u8], right: bool) -> Result<Vec<u8>, DxbcError> {
    edit_planar(blob, right, PlanarLookup::Isolated)
}

enum PlanarLookup {
    Isolated,
    Shift,
    Project,
    Mirror,
}

fn edit_planar(blob: &[u8], right: bool, lookup: PlanarLookup) -> Result<Vec<u8>, DxbcError> {
    const ADD: [u32; 7] = [0x07000000, 0x00100092, 5, 0x00100c06, 6, 0x00100c06, 5];
    let dxbc = Dxbc::parse(blob)?;
    let stream = TokenStream::new(
        dxbc.chunk(b"SHEX")
            .ok_or(DxbcError::NoShaderChunk)?
            .body(blob),
    )?;
    let tokens = stream.tokens();
    let mut site = None;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoLakeReflection);
    }
    let mut previous_is_add = false;
    for instruction in stream.instructions() {
        let instruction = instruction?;
        let words = &tokens[instruction.start..instruction.end];
        if words == PLANAR_SAMPLE && (!previous_is_add || site.replace(instruction.start).is_some())
        {
            return Err(DxbcError::NoLakeReflection);
        }
        previous_is_add = words == ADD;
    }
    let i = site.ok_or(DxbcError::NoLakeReflection)?;
    let mut out = tokens.to_vec();
    if matches!(lookup, PlanarLookup::Isolated) {
        const BASE: [u32; 7] = [0x07000000, 0x00100092, 5, 0x001006a6, 6, 0x00100406, 8];
        let base_end = i - ADD.len();
        if !tokens[..base_end].ends_with(&BASE) {
            return Err(DxbcError::NoLakeReflection);
        }
        out.splice(
            base_end..base_end,
            [
                0x09000032,
                0x00100012,
                5,
                0x0010000a,
                5,
                0x00004001,
                2.0_f32.to_bits(),
                0x00004001,
                if right { (-1.0_f32).to_bits() } else { 0 },
            ],
        );
        return assemble_planar(&dxbc, blob, out);
    }
    let shift = if right { -0.25_f32 } else { 0.25_f32 };
    out.splice(
        i..i,
        [
            0x07000000,
            0x00100012,
            5,
            0x0010000a,
            5,
            0x00004001,
            shift.to_bits(),
        ],
    );
    if matches!(lookup, PlanarLookup::Mirror) {
        let after_shift = i + 7;
        out.splice(
            after_shift..after_shift,
            [
                0x07000000,
                0x00100012,
                5,
                0x0010000a,
                5,
                0x00004001,
                (-0.25_f32).to_bits(),
                0x0500001a,
                0x00100012,
                5,
                0x0010000a,
                5,
                0x07000000,
                0x00100012,
                5,
                0x0010000a,
                5,
                0x00004001,
                (-0.5_f32).to_bits(),
                0x08000000,
                0x00100012,
                5,
                0x8010000a,
                0xc1,
                5,
                0x00004001,
                0.75_f32.to_bits(),
            ],
        );
    }
    if !matches!(lookup, PlanarLookup::Shift) {
        out.splice(
            i - ADD.len()..i - ADD.len(),
            [
                0x07000038,
                0x00100012,
                6,
                0x0010000a,
                6,
                0x00004001,
                0.5_f32.to_bits(),
            ],
        );
    }
    assemble_planar(&dxbc, blob, out)
}

pub fn capture_lake_planar_uv(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let stream = TokenStream::new(
        dxbc.chunk(b"SHEX")
            .ok_or(DxbcError::NoShaderChunk)?
            .body(blob),
    )?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoLakeReflection);
    }
    let tokens = stream.tokens();
    let mut site = None;
    for op in stream.instructions() {
        let op = op?;
        if tokens[op.start..op.end] == PLANAR_SAMPLE && site.replace(op.start).is_some() {
            return Err(DxbcError::NoLakeReflection);
        }
    }
    let mut out = tokens[..site.ok_or(DxbcError::NoLakeReflection)?].to_vec();
    out.extend_from_slice(&[
        0x06000036,
        0x00102032,
        0,
        0x801000c6,
        0x81,
        5,
        0x07000031,
        0x00100032,
        6,
        0x001000c6,
        5,
        0x00004001,
        0,
        0x0a000001,
        0x00100032,
        6,
        0x00100046,
        6,
        0x00004002,
        1f32.to_bits(),
        2f32.to_bits(),
        0,
        0,
        0x07000000,
        0x00100042,
        6,
        0x0010000a,
        6,
        0x0010001a,
        6,
        0x07000000,
        0x00102042,
        0,
        0x0010002a,
        6,
        0x00004001,
        16f32.to_bits(),
        0x05000036,
        0x00102082,
        0,
        0x00004001,
        1f32.to_bits(),
        0x0100003e,
    ]);
    assemble_planar(&dxbc, blob, out)
}

const PLANAR_SAMPLE: [u32; 11] = [
    0x8b000045, 0x800000c2, 0x00155543, 0x00100072, 6, 0x001000c6, 5, 0x00107e46, 2, 0x00106000, 2,
];

pub fn trace_lake_samples(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    trace_lake(blob, false, LakeTrace::Samples)
}

pub fn trace_lake_lighting(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    trace_lake(blob, false, LakeTrace::Lighting)
}

pub fn trace_lake_response(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    trace_lake(blob, false, LakeTrace::Response)
}

pub fn trace_lake_inner_edge(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    trace_lake(blob, true, LakeTrace::Response)
}

pub fn trace_lake_displacement(response: &[u8]) -> Result<Vec<u8>, DxbcError> {
    trace_lake_operands(response, false)
}

pub fn bound_lake_candidate_depth(displacement: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(displacement)?;
    let stream = TokenStream::new(
        dxbc.chunk(b"SHEX")
            .ok_or(DxbcError::NoShaderChunk)?
            .body(displacement),
    )?;
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        let words = &tokens[op.start..op.end];
        match words {
            [0x02000068, 16] => {
                sites.push(0);
                out.extend([0x02000068, 17]);
            }
            [
                0x8b000045,
                0x800000c2,
                0x155543,
                0x100082,
                2,
                0x1000c6,
                10,
                0x107396,
                0,
                0x106000,
                0,
            ] => {
                sites.push(1);
                out.extend([
                    0x07000033,
                    0x100012,
                    16,
                    0x10000a,
                    10,
                    0x4001,
                    (319.5_f32 / 640.).to_bits(),
                    0x05000036,
                    0x100082,
                    16,
                    0x10003a,
                    10,
                ]);
                out.extend_from_slice(&words[..6]);
                out.push(16);
                out.extend_from_slice(&words[7..]);
            }
            [0x5000036, 0x102032, 6, 0x1000c6, 10] => {
                sites.push(2);
                out.extend_from_slice(words);
            }
            _ => out.extend_from_slice(words),
        }
    }
    if stream.stage() != ShaderStage::Pixel || sites != [0, 1, 2] {
        return Err(DxbcError::NoLakeRefraction);
    }
    assemble_planar(&dxbc, displacement, out)
}

pub fn mirror_lake_color_overflow(depth_bounded: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(depth_bounded)?;
    let stream = TokenStream::new(
        dxbc.chunk(b"SHEX")
            .ok_or(DxbcError::NoShaderChunk)?
            .body(depth_bounded),
    )?;
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        let words = &tokens[op.start..op.end];
        if words == [0x02000068, 17] {
            sites.push(0);
        }
        if words
            == [
                0x07000031,
                0x100012,
                15,
                0x4001,
                0.5_f32.to_bits(),
                0x10000a,
                2,
            ]
        {
            sites.push(1);
            out.extend_from_slice(words);
            out.extend([
                0x0500001a,
                0x100012,
                16,
                0x10000a,
                2,
                0x08000000,
                0x100022,
                16,
                0x4001,
                1_f32.to_bits(),
                0x8010000a,
                0x41,
                16,
                0x07000033,
                0x100012,
                16,
                0x10000a,
                16,
                0x10001a,
                16,
                0x09000037,
                0x100012,
                2,
                0x10000a,
                15,
                0x10000a,
                16,
                0x10000a,
                2,
            ]);
        }
        out.extend_from_slice(words);
    }
    if stream.stage() != ShaderStage::Pixel || sites != [0, 1] {
        return Err(DxbcError::NoLakeRefraction);
    }
    assemble_planar(&dxbc, depth_bounded, out)
}

pub fn trace_lake_normals(response: &[u8]) -> Result<Vec<u8>, DxbcError> {
    trace_lake_operands(response, true)
}

pub fn trace_lake_waves(response: &[u8]) -> Result<Vec<u8>, DxbcError> {
    wave_layout(response, false)
}

pub fn trace_lake_gradients(response: &[u8]) -> Result<Vec<u8>, DxbcError> {
    wave_layout(response, true)
}

fn wave_layout(response: &[u8], gradients: bool) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(response)?;
    let body = |tag| {
        dxbc.chunk(tag)
            .map(|chunk| chunk.body(response))
            .ok_or(DxbcError::NoShaderChunk)
    };
    let stream = TokenStream::new(body(b"SHEX")?)?;
    let mut signature = body(b"OSGN")?.to_vec();
    if stream.stage() != ShaderStage::Pixel || max_signature_register(&signature) != Some(5) {
        return Err(DxbcError::NoLakeRefraction);
    }
    for register in [6, 7] {
        signature = append_signature_element(
            &signature,
            &SignatureElement {
                name: "SV_Target",
                semantic_index: register,
                system_value: 0,
                component_type: 3,
                register,
                mask: 15,
                rw_mask: 0,
            },
        )?;
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    let mut replaced = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        let w = &tokens[op.start..op.end];
        if matches!(w, [0x5000036 | 0x6000036, dst, 1 | 2 | 3 | 5, ..] if dst & 0xfff000 == 0x102000)
        {
            replaced.push((w[2], w[1]));
            continue;
        }
        out.extend_from_slice(w);
        match w {
            [0x3000065, 0x1020f2, 5] => {
                out.extend([0x3000065, 0x1020f2, 6, 0x3000065, 0x1020f2, 7]);
                sites.push(0);
            }
            [
                0x8b000045,
                0x800000c2,
                0x155543,
                0x1000f2,
                r,
                uv,
                v,
                0x107e46,
                t,
                0x106000,
                s,
            ] if (7..=10).contains(t)
                && s == t
                && *r == t - 6
                && *v == (t - 7) / 2 + 1
                && *uv == if t % 2 == 1 { 0x101046 } else { 0x101ae6 } =>
            {
                if gradients {
                    if *t == 8 || *t == 9 {
                        let (dst, sample_src, uv_src) = if *t == 8 {
                            (0x102032, 0x100046, 0x101ae6)
                        } else {
                            (0x1020c2, 0x100406, 0x101406)
                        };
                        out.extend([
                            0x5000036, dst, 1, sample_src, *r, 0x5000036, dst, 2, uv_src, *v,
                        ]);
                        for (target, opcode) in [(3, 122), (5, 124), (6, 123), (7, 125)] {
                            out.extend([0x5000000 | opcode, dst, target, uv_src, *v]);
                        }
                    }
                } else {
                    let (dst, src) = if t % 2 == 1 {
                        (0x102032, 0x100046)
                    } else {
                        (0x1020c2, 0x100406)
                    };
                    out.extend([0x5000036, dst, *v, src, *r]);
                    if *t == 10 {
                        out.extend([
                            0x5000036, 0x1020f2, 3, 0x101e46, 1, 0x5000036, 0x1020f2, 5, 0x101e46,
                            2, 0x5000036, 0x102072, 6, 0x101396, 3,
                        ]);
                    }
                }
                sites.push(*t);
            }
            [0x6000036, 0x100042, 5, 0x20800a, 2, 0] => {
                if !gradients {
                    out.extend([
                        0x5000036, 0x102082, 6, 0x10002a, 5, 0x5000036, 0x102032, 7, 0x100046, 5,
                    ]);
                }
                sites.push(11);
            }
            [0x7000038, 0x1000b2, 2, 0x100aa6, 0, 0x100c46, 1] => {
                if !gradients {
                    out.extend([0x5000036, 0x1020c2, 7, 0x100406, 2]);
                }
                sites.push(12);
            }
            _ => {}
        }
    }
    if sites != [0, 7, 8, 9, 10, 11, 12]
        || replaced
            != [
                (2, 0x102032),
                (1, 0x102072),
                (2, 0x102042),
                (3, 0x102072),
                (2, 0x102082),
                (3, 0x102082),
                (5, 0x1020f2),
                (1, 0x102082),
            ]
    {
        return Err(DxbcError::NoLakeRefraction);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        response,
        body(b"ISGN")?.to_vec(),
        signature,
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

fn trace_lake_operands(response: &[u8], normals: bool) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(response)?;
    let body = |tag| {
        dxbc.chunk(tag)
            .map(|chunk| chunk.body(response))
            .ok_or(DxbcError::NoShaderChunk)
    };
    let stream = TokenStream::new(body(b"SHEX")?)?;
    let mut signature = body(b"OSGN")?.to_vec();
    if stream.stage() != ShaderStage::Pixel || max_signature_register(&signature) != Some(5) {
        return Err(DxbcError::NoLakeRefraction);
    }
    for register in [6, 7] {
        signature = append_signature_element(
            &signature,
            &SignatureElement {
                name: "SV_Target",
                semantic_index: register,
                system_value: 0,
                component_type: 3,
                register,
                mask: 15,
                rw_mask: 0,
            },
        )?;
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        let words = &tokens[op.start..op.end];
        if normals {
            out.extend_from_slice(words);
            match words {
                [0x3000065, 0x1020f2, 5] => {
                    out.extend([0x3000065, 0x1020f2, 6, 0x3000065, 0x1020f2, 7]);
                    sites.push(0);
                }
                [0x5000036, 0x100042, 6, 0x10103a, 5] => {
                    out.extend([0x5000036, 0x102072, 6, 0x100246, 6]);
                    sites.push(1);
                }
                [0x7000038, 0x1000b2, 2, 0x100aa6, 0, 0x100c46, 1] => {
                    out.extend([0x5000036, 0x102072, 7, 0x100346, 2]);
                    sites.push(2);
                }
                [0x700000e, 0x100012, 7, 0x4001, 0x43960000, 0x10000a, 7] => {
                    out.extend([0x5000036, 0x102082, 7, 0x10000a, 7]);
                    sites.push(3);
                }
                [0x7000038, 0x100032, 2, 0x100046, 2, 0x100006, 10] => {
                    out.extend([0x5000036, 0x102082, 6, 0x10002a, 9]);
                    sites.push(4);
                }
                _ => {}
            }
            continue;
        }
        if words == [0x7000038, 0x100032, 2, 0x100046, 2, 0x100006, 10] {
            out.extend([
                0x5000036, 0x102032, 7, 0x100046, 2, 0x5000036, 0x102042, 7, 0x10002a, 9,
                0x5000036, 0x102082, 7, 0x10001a, 4,
            ]);
            sites.push(2);
        }
        out.extend_from_slice(words);
        match words {
            [0x3000065, 0x1020f2, 5] => {
                out.extend([0x3000065, 0x1020f2, 6, 0x3000065, 0x1020f2, 7]);
                sites.push(0);
            }
            [
                0x8b000045,
                0x800000c2,
                0x155543,
                0x100082,
                2,
                0x100596,
                5,
                0x107396,
                0,
                0x106000,
                0,
            ] => {
                out.extend([0x5000036, 0x102042, 6, 0x10003a, 2]);
                sites.push(1);
            }
            [
                0x8b000045,
                0x800000c2,
                0x155543,
                0x100082,
                2,
                0x1000c6,
                10,
                0x107396,
                0,
                0x106000,
                0,
            ] => {
                out.extend([
                    0x5000036, 0x102032, 6, 0x1000c6, 10, 0x5000036, 0x102082, 6, 0x10003a, 2,
                ]);
                sites.push(3);
            }
            _ => {}
        }
    }
    if sites.as_slice()
        != if normals {
            &[0, 1, 2, 3, 4][..]
        } else {
            &[0, 1, 2, 3][..]
        }
    {
        return Err(DxbcError::NoLakeRefraction);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        response,
        body(b"ISGN")?.to_vec(),
        signature,
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

enum LakeTrace {
    Samples,
    Lighting,
    Response,
}

fn trace_lake(blob: &[u8], inner_edge: bool, trace: LakeTrace) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let body = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoShaderChunk)
    };
    let stream = TokenStream::new(body(b"SHEX")?)?;
    let mut signature = body(b"OSGN")?.to_vec();
    if stream.stage() != ShaderStage::Pixel || max_signature_register(&signature) != Some(0) {
        return Err(DxbcError::NoLakeRefraction);
    }
    let targets = if matches!(trace, LakeTrace::Response) {
        5
    } else {
        4
    };
    for register in 1..=targets {
        signature = append_signature_element(
            &signature,
            &SignatureElement {
                name: "SV_Target",
                semantic_index: register,
                system_value: 0,
                component_type: 3,
                register,
                mask: 15,
                rw_mask: 0,
            },
        )?;
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    let mut extra_temp = false;
    for op in stream.instructions() {
        let op = op?;
        let words = &tokens[op.start..op.end];
        if words == [0x3000065, 0x1020f2, 0] {
            out.extend_from_slice(words);
            for register in 1..=targets {
                out.extend([0x3000065, 0x1020f2, register]);
            }
            sites.push(0);
            continue;
        }
        if inner_edge && words == [0x02000068, 15] {
            if extra_temp {
                return Err(DxbcError::NoLakeRefraction);
            }
            extra_temp = true;
            out.extend([0x02000068, 16]);
            continue;
        }
        if matches!(trace, LakeTrace::Response) {
            let color = words
                == [
                    0x8b000045, 0x800000c2, 0x155543, 0x100072, 2, 0x100046, 2, 0x107e46, 3,
                    0x106000, 2,
                ];
            if color {
                if inner_edge {
                    if !extra_temp {
                        return Err(DxbcError::NoLakeRefraction);
                    }
                    out.extend([
                        0x07000031,
                        0x100012,
                        15,
                        0x4001,
                        0.5_f32.to_bits(),
                        0x10000a,
                        2,
                        0x07000033,
                        0x100022,
                        15,
                        0x10000a,
                        2,
                        0x4001,
                        (319.5_f32 / 640.).to_bits(),
                        0x09000037,
                        0x100012,
                        2,
                        0x10000a,
                        15,
                        0x10000a,
                        2,
                        0x10001a,
                        15,
                    ]);
                }
                out.extend([0x5000036, 0x102032, 2, 0x100046, 2]);
            }
            out.extend_from_slice(words);
            if color {
                out.extend([0x5000036, 0x102072, 1, 0x100246, 2]);
                sites.push(1);
            } else {
                match words {
                    [
                        0xa000032,
                        0x100042,
                        3,
                        0x8010002a,
                        0x41,
                        3,
                        0x10000a,
                        5,
                        0x4001,
                        0x3f800000,
                    ] => {
                        out.extend([
                            0x5000036, 0x102042, 2, 0x10002a, 3, 0x6000036, 0x102072, 3, 0x208246,
                            1, 9,
                        ]);
                        sites.push(2);
                    }
                    [
                        0xa000032,
                        0x100042,
                        3,
                        0x8010002a,
                        0x41,
                        3,
                        0x10001a,
                        5,
                        0x4001,
                        0x3f800000,
                    ] => {
                        out.extend([0x5000036, 0x102082, 2, 0x10002a, 3]);
                        sites.push(3);
                    }
                    [0x7000038, 0x100012, 1, 0x10003a, 2, 0x10000a, 1] => {
                        out.extend([0x5000036, 0x102082, 3, 0x10000a, 1]);
                        sites.push(4);
                    }
                    [
                        0x9000032,
                        0x102072,
                        0,
                        0x100246,
                        0,
                        0x100246,
                        3,
                        0x100246,
                        1,
                    ] => {
                        let mut copy = words.to_vec();
                        copy[2] = 4;
                        out.extend(copy);
                        out.extend([
                            0x5000036,
                            0x102082,
                            4,
                            0x4001,
                            1_f32.to_bits(),
                            0x5000036,
                            0x1020f2,
                            5,
                            0x100e46,
                            3,
                            0x5000036,
                            0x102082,
                            1,
                            0x4001,
                            1_f32.to_bits(),
                        ]);
                        sites.push(5);
                    }
                    _ => {}
                }
            }
            continue;
        }
        if matches!(trace, LakeTrace::Lighting) {
            out.extend_from_slice(words);
            match words {
                [0x7000038, 0x100012, 1, 0x10003a, 2, 0x10000a, 1] => {
                    out.extend([
                        0x5000036, 0x102072, 1, 0x100346, 0, 0x5000036, 0x102082, 1, 0x10000a, 1,
                    ]);
                    sites.push(1);
                }
                [
                    0x9000032,
                    0x1000b2,
                    0,
                    0x100006,
                    1,
                    0x100846,
                    2,
                    0x100c46,
                    0,
                ] => {
                    out.extend([
                        0x5000036, 0x102072, 2, 0x100346, 0, 0x5000036, 0x102082, 2, 0x10002a, 0,
                    ]);
                    sites.push(2);
                }
                [
                    0x9000032,
                    0x100072,
                    0,
                    0x100386,
                    1,
                    0x100556,
                    1,
                    0x100246,
                    0,
                ] => {
                    out.extend([0x5000036, 0x102072, 3, 0x100246, 0]);
                    sites.push(3);
                }
                [0x7000038, 0x100082, 0, 0x10002a, 10, 0x10002a, 1] => {
                    out.extend([0x5000036, 0x102082, 3, 0x10003a, 0]);
                    sites.push(4);
                }
                [
                    0x9000032,
                    0x102072,
                    0,
                    0x100246,
                    0,
                    0x100246,
                    3,
                    0x100246,
                    1,
                ] => {
                    let mut final_color = words.to_vec();
                    final_color[2] = 4;
                    out.extend(final_color);
                    out.extend([0x5000036, 0x102082, 4, 0x4001, 1_f32.to_bits()]);
                    sites.push(5);
                }
                _ => {}
            }
            continue;
        }
        let sample = |dst, r, uv, s, tex, slot, sampler| {
            words
                == [
                    0x8b000045, 0x800000c2, 0x155543, dst, r, uv, s, tex, slot, 0x106000, sampler,
                ]
        };
        if sample(0x100072, 2, 0x100046, 2, 0x107e46, 3, 2) {
            out.extend([0x5000036, 0x102032, 1, 0x100046, 2]);
        }
        out.extend_from_slice(words);
        if sample(0x100082, 2, 0x100596, 5, 0x107396, 0, 0) {
            out.extend([0x5000036, 0x102042, 2, 0x10003a, 2]);
            sites.push(1);
        } else if sample(0x100082, 2, 0x1000c6, 10, 0x107396, 0, 0) {
            out.extend([
                0x5000036, 0x102032, 2, 0x1000c6, 10, 0x5000036, 0x102082, 2, 0x10003a, 2,
            ]);
            sites.push(2);
        } else if words == PLANAR_SAMPLE {
            out.extend([
                0x5000036, 0x1020c2, 1, 0x100c06, 5, 0x5000036, 0x102072, 4, 0x100e46, 6,
                0x5000036, 0x102082, 4, 0x10001a, 1,
            ]);
            sites.push(3);
        } else if sample(0x100072, 2, 0x100046, 2, 0x107e46, 3, 2) {
            out.extend([
                0x5000036,
                0x102072,
                3,
                0x100e46,
                2,
                0x5000036,
                0x102082,
                3,
                0x4001,
                1_f32.to_bits(),
            ]);
            sites.push(4);
        }
    }
    if sites
        != if matches!(trace, LakeTrace::Response) {
            &[0, 1, 2, 3, 4, 5][..]
        } else if matches!(trace, LakeTrace::Lighting) {
            &[0, 1, 2, 4, 3, 5][..]
        } else {
            &[0, 1, 2, 3, 4][..]
        }
    {
        return Err(DxbcError::NoLakeRefraction);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        body(b"ISGN")?.to_vec(),
        signature,
        out.iter().flat_map(|w| w.to_le_bytes()).collect(),
        false,
    )
}

fn assemble_planar(dxbc: &Dxbc, blob: &[u8], mut out: Vec<u32>) -> Result<Vec<u8>, DxbcError> {
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    let dep = |k| {
        dxbc.chunk(k)
            .map(|chunk| chunk.body(blob).to_vec())
            .unwrap_or_default()
    };
    reassemble(
        dxbc,
        blob,
        dep(b"ISGN"),
        dep(b"OSGN"),
        out.iter().flat_map(|x| x.to_le_bytes()).collect(),
        false,
    )
}

pub fn scale_lake_refraction(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    const MUL: [u32; 10] = [
        0x0a000038,
        0x00100032,
        2,
        0x00100046,
        2,
        0x00004002,
        (-5.0_f32).to_bits(),
        5.0_f32.to_bits(),
        0,
        0,
    ];
    let chunk = Dxbc::parse(blob)?
        .chunk(b"SHEX")
        .ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(chunk.body(blob))?;
    let mut site = None;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoLakeRefraction);
    }
    for op in stream.instructions() {
        let op = op?;
        if stream.tokens()[op.start..op.end] == MUL && site.replace(op.start + 6).is_some() {
            return Err(DxbcError::NoLakeRefraction);
        }
    }
    let offset = chunk.body_offset + 4 * site.ok_or(DxbcError::NoLakeRefraction)?;
    let mut out = blob.to_vec();
    out[offset..offset + 4].copy_from_slice(&(-2.5_f32).to_le_bytes());
    crate::refresh_checksum(&mut out);
    Ok(out)
}
