use crate::{
    Dxbc, DxbcError, ShaderStage, TokenStream,
    isa::{OPERAND_IMM32_SCALAR, OPERAND_IMM32_VECTOR},
    rewrite::common::{
        SignatureElement, append_signature_element, max_signature_register, reassemble,
    },
};

pub fn trace_deferred_samples(blob: &[u8], shadow: bool) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoResolveInputs)
    };
    let stream = TokenStream::new(chunk(b"SHEX")?)?;
    let mut signature = chunk(b"OSGN")?.to_vec();
    if stream.stage() != ShaderStage::Pixel || max_signature_register(&signature) != Some(0) {
        return Err(DxbcError::NoResolveInputs);
    }
    for register in 1..=if shadow { 6 } else { 4 } {
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
    let mut shadow_sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        let words = &tokens[op.start..op.end];
        out.extend_from_slice(words);
        if shadow {
            match words {
                [0x7000038, 0x100082, 7, 0x10003a, 7, 0x10003a, 9] => {
                    out.extend([0x5000036, 0x102012, 5, 0x10003a, 7]);
                    out.extend([
                        0x8000036,
                        0x1020e2,
                        5,
                        OPERAND_IMM32_VECTOR,
                        0,
                        0,
                        0,
                        0,
                        0x8000036,
                        0x1020f2,
                        6,
                        OPERAND_IMM32_VECTOR,
                        0,
                        0,
                        0,
                        0,
                    ]);
                    shadow_sites.push(0);
                }
                [
                    0x8d000047,
                    0x80000202,
                    0x155543,
                    0x100022,
                    11,
                    0x100246,
                    18,
                    0x107006,
                    14,
                    0x106000,
                    15,
                    0x10000a,
                    20,
                ] => {
                    out.extend([
                        0x5000036, 0x102022, 5, 0x10001a, 11, 0x5000036, 0x102072, 6, 0x100246, 18,
                        0x5000036, 0x102082, 6, 0x10000a, 20,
                    ]);
                    shadow_sites.push(1);
                }
                [
                    0x7000038,
                    0x100042,
                    10,
                    0x10001a,
                    10,
                    OPERAND_IMM32_SCALAR,
                    0x3d800000,
                ] => {
                    out.extend([0x5000036, 0x102042, 5, 0x10002a, 10]);
                    shadow_sites.push(2);
                }
                [
                    0xb002032,
                    0x100082,
                    9,
                    0x10002a,
                    10,
                    0x20800a,
                    0,
                    9,
                    0x20801a,
                    0,
                    9,
                ] => {
                    out.extend([0x5000036, 0x102082, 5, 0x10003a, 9]);
                    shadow_sites.push(3);
                }
                _ => {}
            }
        }
        match words {
            [0x3000065, 0x1020f2, 0] => {
                for register in 1..=if shadow { 6 } else { 4 } {
                    out.extend([0x3000065, 0x1020f2, register]);
                }
                sites.push(0);
            }
            [
                0x8900002d,
                0x800000c2,
                0x155543,
                0x1000e2,
                12,
                0x100e46,
                0,
                0x107936,
                25,
            ] => {
                for (target, xyz, source, w, scalar) in [
                    (1, 0x100396, 11, 0x10003a, 9),
                    (2, 0x100396, 12, 0x10003a, 7),
                    (3, 0x100246, 9, OPERAND_IMM32_SCALAR, 1f32.to_bits()),
                    (4, 0x100246, 6, OPERAND_IMM32_SCALAR, 1f32.to_bits()),
                ] {
                    out.extend([
                        0x5000036, 0x102072, target, xyz, source, 0x5000036, 0x102082, target, w,
                        scalar,
                    ]);
                }
                sites.push(1);
            }
            _ => {}
        }
    }
    if sites != [0, 1] || (shadow && shadow_sites != [0, 1, 2, 3]) {
        return Err(DxbcError::NoResolveInputs);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        chunk(b"ISGN")?.to_vec(),
        signature,
        out.iter().flat_map(|w| w.to_le_bytes()).collect(),
        false,
    )
}

pub fn trace_ssr_ray(
    blob: &[u8],
    march_eye: Option<bool>,
    pack_veto: bool,
    physical_grid: bool,
    history: bool,
    center_history: bool,
) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoResolveInputs)
    };
    let stream = TokenStream::new(chunk(b"SHEX")?)?;
    let mut signature = chunk(b"OSGN")?.to_vec();
    if stream.stage() != ShaderStage::Pixel || max_signature_register(&signature) != Some(1) {
        return Err(DxbcError::NoResolveInputs);
    }
    for register in 2..8 {
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
    let mut grid_sites = 0;
    let mut history_sites = 0;
    for op in stream.instructions() {
        let op = op?;
        let words = &tokens[op.start..op.end];
        if physical_grid
            && let Some(i) = words
                .windows(3)
                .position(|v| matches!(v, [0x208156 | 0x208046 | 0x208406 | 0x208ae6, 2, 12]))
        {
            out.push(words[0] - (1 << 24));
            out.extend_from_slice(&words[1..i]);
            out.extend([0x100000 | (words[i] & 0xfff), 12]);
            out.extend_from_slice(&words[i + 3..]);
            grid_sites += 1;
        } else {
            out.extend_from_slice(words);
        }
        let mov = |out: &mut Vec<u32>, reg: u32, mask: u32, src: u32, index: u32| {
            out.extend([0x5000036, 0x102002 | mask << 4, reg, src, index])
        };
        match words {
            [0x3000065, 0x102012, 1] => {
                for i in 2..8 {
                    out.extend([0x3000065, 0x1020f2, i]);
                }
                sites.push(0);
            }
            [0x2000068, 12] => {
                if physical_grid {
                    *out.last_mut().ok_or(DxbcError::NoResolveInputs)? = 13;
                    out.extend([
                        0xb000038,
                        0x1000f2,
                        12,
                        0x208e46,
                        2,
                        12,
                        OPERAND_IMM32_VECTOR,
                        0.5f32.to_bits(),
                        1f32.to_bits(),
                        2f32.to_bits(),
                        1f32.to_bits(),
                    ]);
                }
                for i in 2..8 {
                    mov(&mut out, i, 15, OPERAND_IMM32_SCALAR, 0);
                }
                sites.push(1);
            }
            [
                0x8d000048,
                _,
                _,
                0x100012,
                0,
                0x101046,
                1,
                0x107e46,
                2,
                0x106000,
                0,
                OPERAND_IMM32_SCALAR,
                0,
            ] => {
                mov(&mut out, 2, 3, 0x101046, 1);
                mov(&mut out, 2, 4, 0x10000a, 0);
                mov(&mut out, 2, 8, OPERAND_IMM32_SCALAR, 1f32.to_bits());
                sites.push(2);
            }
            [
                0x8d000048,
                _,
                _,
                0x1000f2,
                1,
                0x101046,
                1,
                0x107e46,
                1,
                0x106000,
                0,
                OPERAND_IMM32_SCALAR,
                0,
            ] => {
                mov(&mut out, 3, 15, 0x100e46, 1);
                sites.push(3);
            }
            [
                0xa000032,
                0x1000d2,
                0,
                0x100006,
                0,
                0x101906,
                2,
                0x208906,
                0,
                4,
            ] => {
                if !history {
                    mov(&mut out, 5, 7, 0x100386, 0);
                    mov(&mut out, 5, 8, 0x10001a, 0);
                }
                sites.push(4);
            }
            [0x8000038, 0x100032, 6, 0x100046, 3, 0x208046, 2, 12] => {
                if !history {
                    mov(&mut out, 4, 3, 0x100516, 2);
                    mov(&mut out, 4, 12, 0x100406, 3);
                }
                sites.push(5);
            }
            [
                0x8d000048,
                _,
                _,
                0x100082,
                5,
                0x100046,
                10,
                0x107396,
                2,
                0x106000,
                0,
                OPERAND_IMM32_SCALAR,
                0,
            ] => {
                pack_sample(&mut out, words, 10, march_eye);
                out.extend([0x300001f, 0x10002a, 5]);
                mov(&mut out, 6, 3, 0x100046, 10);
                mov(&mut out, 6, 4, 0x10003a, 5);
                out.push(0x1000015);
                sites.push(6);
            }
            [0x1000016] => {
                mov(&mut out, 6, 8, 0x10002a, 5);
                sites.push(7);
            }
            [
                0x8d000048,
                _,
                _,
                0x100082,
                0,
                0x100046,
                1,
                0x107396,
                0,
                0x106000,
                0,
                OPERAND_IMM32_SCALAR,
                0,
            ] => {
                pack_sample(&mut out, words, 1, march_eye.filter(|_| pack_veto));
                mov(&mut out, 7, 3, 0x100046, 1);
                mov(&mut out, 7, 4, 0x10003a, 0);
                mov(&mut out, 7, 8, OPERAND_IMM32_SCALAR, 1f32.to_bits());
                sites.push(8);
            }
            [
                0x8d000048,
                _,
                _,
                0x1000e2,
                0,
                0x100596,
                0,
                0x107936,
                3,
                0x106000,
                0,
                OPERAND_IMM32_SCALAR,
                0,
            ] if history => {
                out.truncate(out.len() - words.len());
                if center_history {
                    let right = march_eye.ok_or(DxbcError::NoResolveInputs)?;
                    out.extend([
                        0x7000000,
                        0x100022,
                        0,
                        0x10001a,
                        0,
                        OPERAND_IMM32_SCALAR,
                        if right { 0.25f32 } else { -0.25f32 }.to_bits(),
                    ]);
                }
                mov(&mut out, 4, 3, 0x100596, 0);
                mov(&mut out, 4, 4, 0x10000a, 0);
                mov(&mut out, 4, 8, OPERAND_IMM32_SCALAR, 1f32.to_bits());
                out.extend_from_slice(words);
                mov(&mut out, 5, 7, 0x100396, 0);
                mov(&mut out, 5, 8, OPERAND_IMM32_SCALAR, 1f32.to_bits());
                history_sites += 1;
            }
            _ => {}
        }
    }
    if sites != [0, 1, 2, 3, 4, 5, 6, 7, 8]
        || (physical_grid && grid_sites != 7)
        || (history && history_sites != 1)
    {
        return Err(DxbcError::NoResolveInputs);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        chunk(b"ISGN")?.to_vec(),
        signature,
        out.iter().flat_map(|w| w.to_le_bytes()).collect(),
        false,
    )
}

fn pack_sample(out: &mut Vec<u32>, sample: &[u32], reg: u32, right: Option<bool>) {
    if let Some(right) = right {
        out.truncate(out.len() - sample.len());
        out.extend([
            0x9000032,
            0x100012,
            reg,
            0x10000a,
            reg,
            OPERAND_IMM32_SCALAR,
            0.5f32.to_bits(),
            OPERAND_IMM32_SCALAR,
            if right { 0.5f32.to_bits() } else { 0 },
        ]);
        out.extend_from_slice(sample);
    }
}

pub fn local_bark_dither(blob: &[u8], origin: f32, color: bool) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoResolveInputs)
    };
    let stream = TokenStream::new(chunk(b"SHEX")?)?;
    if stream.stage() != ShaderStage::Pixel || !origin.is_finite() || origin <= 0. {
        return Err(DxbcError::NoResolveInputs);
    }
    let tmp = if color { 7 } else { 2 };
    let dest = if color { 0x100022 } else { 0x100012 };
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        let w = &tokens[op.start..op.end];
        match w {
            [0x2000068, n] if *n == tmp => {
                out.extend([0x2000068, tmp + 1]);
                sites.push(0);
            }
            [
                0xa00000f,
                mask,
                0,
                0x101046,
                0,
                OPERAND_IMM32_VECTOR,
                0x3eef965b,
                0xbf342246,
                0,
                0,
            ] if *mask == dest => {
                out.extend([
                    0xa000000,
                    0x100032,
                    tmp,
                    0x101046,
                    0,
                    OPERAND_IMM32_VECTOR,
                    (-origin).to_bits(),
                    0,
                    0,
                    0,
                ]);
                out.extend([
                    0xa00000f,
                    dest,
                    0,
                    0x100046,
                    tmp,
                    OPERAND_IMM32_VECTOR,
                    0x3eef965b,
                    0xbf342246,
                    0,
                    0,
                ]);
                sites.push(1);
            }
            _ => out.extend_from_slice(w),
        }
    }
    if sites != [0, 1] {
        return Err(DxbcError::NoResolveInputs);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        chunk(b"ISGN")?.to_vec(),
        chunk(b"OSGN")?.to_vec(),
        out.iter().flat_map(|w| w.to_le_bytes()).collect(),
        false,
    )
}

pub fn physical_impostor_fade(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let chunk = Dxbc::parse(blob)?
        .chunk(b"SHEX")
        .ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(chunk.body(blob))?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoResolveInputs);
    }
    let mut site = None;
    for op in stream.instructions() {
        let op = op?;
        if stream.tokens()[op.start..op.end]
            == [
                0xa00000f,
                0x100012,
                0,
                0x100046,
                0,
                OPERAND_IMM32_VECTOR,
                0x3eef965b,
                0xbf342246,
                0,
                0,
            ]
            && site
                .replace(chunk.body_offset + 4 * (op.start + 6))
                .is_some()
        {
            return Err(DxbcError::NoResolveInputs);
        }
    }
    let offset = site.ok_or(DxbcError::NoResolveInputs)?;
    let mut out = blob.to_vec();
    out[offset..offset + 4].copy_from_slice(&(0.5 * f32::from_bits(0x3eef965b)).to_le_bytes());
    crate::refresh_checksum(&mut out);
    Ok(out)
}

pub fn impostor_atlas_scale(
    blob: &[u8],
    right_origin: f32,
    left: bool,
) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoResolveInputs)
    };
    let stream = TokenStream::new(chunk(b"SHEX")?)?;
    if stream.stage() != ShaderStage::Pixel || !right_origin.is_finite() || right_origin <= 0. {
        return Err(DxbcError::NoResolveInputs);
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        let w = &tokens[op.start..op.end];
        match w {
            [0x4002864, 0x101042, 0, 1] => {
                out.extend([0x4002864, 0x101052, 0, 1]);
                sites.push(0);
            }
            [0x2000068, 6] => {
                out.extend([0x2000068, 7]);
                sites.push(1);
            }
            [
                0x9000032,
                0x100032,
                0,
                0x100ae6,
                0,
                0x101ff6,
                5,
                0x100046,
                0,
            ] => {
                out.extend([
                    0x700001d,
                    0x100012,
                    6,
                    0x10100a,
                    0,
                    OPERAND_IMM32_SCALAR,
                    right_origin.to_bits(),
                ]);
                out.extend([
                    0x9000037,
                    0x100012,
                    6,
                    0x10000a,
                    6,
                    OPERAND_IMM32_SCALAR,
                    0.5f32.to_bits(),
                    OPERAND_IMM32_SCALAR,
                    if left { 0.5f32 } else { 1f32 }.to_bits(),
                ]);
                out.extend([0x7000038, 0x100012, 6, 0x10000a, 6, 0x10103a, 5]);
                out.extend([
                    0x9000032, 0x100032, 0, 0x100ae6, 0, 0x100006, 6, 0x100046, 0,
                ]);
                sites.push(2);
            }
            _ => out.extend_from_slice(w),
        }
    }
    if sites != [0, 1, 2] {
        return Err(DxbcError::NoResolveInputs);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        crate::rewrite::common::widen_signature_mask(chunk(b"ISGN")?, "SV_Position", 0, 15, 5)?,
        chunk(b"OSGN")?.to_vec(),
        out.iter().flat_map(|w| w.to_le_bytes()).collect(),
        false,
    )
}

pub fn local_fog_reconstruction(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoResolveInputs)
    };
    let stream = TokenStream::new(chunk(b"SHEX")?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoResolveInputs);
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut found = 0;
    for op in stream.instructions() {
        let op = op?;
        let w = &tokens[op.start..op.end];
        out.extend_from_slice(w);
        if w == [
            0x8b000045, 0x800000c2, 0x155543, 0x100042, 0, 0x100046, 0, 0x107c96, 0, 0x106000, 0,
        ] && tokens[op.end..].starts_with(&[
            0x8000038, 0x1000f2, 1, 0x100556, 0, 0x208e46, 1, 1, 0xa000032, 0x1000f2, 1, 0x100006,
            0, 0x208e46, 1, 0, 0x100e46, 1,
        ]) {
            out.extend([
                0x7000038,
                0x100012,
                0,
                0x10000a,
                0,
                OPERAND_IMM32_SCALAR,
                2f32.to_bits(),
                0x500001a,
                0x100012,
                0,
                0x10000a,
                0,
            ]);
            found += 1;
        }
    }
    if found != 1 {
        return Err(DxbcError::NoResolveInputs);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        chunk(b"ISGN")?.to_vec(),
        chunk(b"OSGN")?.to_vec(),
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

pub fn local_dof_edge(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoResolveInputs)
    };
    let stream = TokenStream::new(chunk(b"SHEX")?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoResolveInputs);
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        match &tokens[op.start..op.end] {
            [0x2000068, 4] => {
                out.extend([0x2000068, 5]);
                sites.push(0);
            }
            w @ [
                0x8b000045,
                0x800000c2,
                0x155543,
                0x100072,
                1,
                0x101046,
                1,
                0x107e46,
                9,
                0x106000,
                9,
            ] => {
                out.extend([
                    0x5000036,
                    0x100032,
                    4,
                    0x101046,
                    1,
                    0x7000038,
                    0x100012,
                    4,
                    0x10000a,
                    4,
                    OPERAND_IMM32_SCALAR,
                    2f32.to_bits(),
                    0x500001a,
                    0x100012,
                    4,
                    0x10000a,
                    4,
                ]);
                out.extend_from_slice(&w[..5]);
                out.extend([0x100046, 4]);
                out.extend_from_slice(&w[7..]);
                sites.push(1);
            }
            w => out.extend_from_slice(w),
        }
    }
    if sites != [0, 1] {
        return Err(DxbcError::NoResolveInputs);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        chunk(b"ISGN")?.to_vec(),
        chunk(b"OSGN")?.to_vec(),
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

pub fn local_aa_neighbor(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| {
        dxbc.chunk(tag)
            .map(|c| c.body(blob))
            .ok_or(DxbcError::NoResolveInputs)
    };
    let stream = TokenStream::new(chunk(b"SHEX")?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoResolveInputs);
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut sites = Vec::new();
    for op in stream.instructions() {
        let op = op?;
        match &tokens[op.start..op.end] {
            [0x2000068, 4] => {
                out.extend([0x2000068, 5]);
                sites.push(0);
            }
            w @ [
                0x8d000048,
                0x800000c2,
                0x155543,
                0x100072,
                2,
                0x100ae6,
                0,
                0x107e46,
                0,
                0x106000,
                0,
                OPERAND_IMM32_SCALAR,
                0,
            ] => {
                out.extend([
                    0x05000036,
                    0x100032,
                    4,
                    0x100ae6,
                    0,
                    0x07000031,
                    0x100042,
                    4,
                    0x10100a,
                    1,
                    OPERAND_IMM32_SCALAR,
                    0.5f32.to_bits(),
                    0x07000033,
                    0x100082,
                    4,
                    0x10000a,
                    4,
                    OPERAND_IMM32_SCALAR,
                    (319.5f32 / 640.).to_bits(),
                    0x07000034,
                    0x100012,
                    4,
                    0x10000a,
                    4,
                    OPERAND_IMM32_SCALAR,
                    (320.5f32 / 640.).to_bits(),
                    0x09000037,
                    0x100012,
                    4,
                    0x10002a,
                    4,
                    0x10003a,
                    4,
                    0x10000a,
                    4,
                ]);
                out.extend_from_slice(&w[..5]);
                out.extend([0x100046, 4]);
                out.extend_from_slice(&w[7..]);
                sites.push(1);
            }
            w => out.extend_from_slice(w),
        }
    }
    if sites != [0, 1] {
        return Err(DxbcError::NoResolveInputs);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        chunk(b"ISGN")?.to_vec(),
        chunk(b"OSGN")?.to_vec(),
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}
