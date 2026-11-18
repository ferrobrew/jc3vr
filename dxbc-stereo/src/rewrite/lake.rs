use crate::{Dxbc, DxbcError, ShaderStage, TokenStream, rewrite::common::reassemble};

pub fn offset_lake_reflection(blob: &[u8], right: bool) -> Result<Vec<u8>, DxbcError> {
    const SAMPLE: [u32; 11] = [
        0x8b000045, 0x800000c2, 0x00155543, 0x00100072, 6, 0x001000c6, 5, 0x00107e46, 2,
        0x00106000, 2,
    ];
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
    for instruction in stream.instructions() {
        let instruction = instruction?;
        let i = instruction.start;
        if tokens[i..instruction.end] == SAMPLE
            && (i < ADD.len() || tokens[i - ADD.len()..i] != ADD || site.replace(i).is_some())
        {
            return Err(DxbcError::NoLakeReflection);
        }
    }
    let i = site.ok_or(DxbcError::NoLakeReflection)?;
    let mut out = tokens.to_vec();
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
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    let dep = |k| {
        dxbc.chunk(k)
            .map(|chunk| chunk.body(blob).to_vec())
            .unwrap_or_default()
    };
    reassemble(
        &dxbc,
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
