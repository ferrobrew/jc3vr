use crate::{Dxbc, DxbcError, ShaderStage, TokenStream, rewrite::common::reassemble};

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
