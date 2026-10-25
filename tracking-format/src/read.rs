//! Budgeted legacy and chunked decoding. Only incomplete outer records permit recovery.

use std::mem::size_of;

use crate::{
    DecodeError, DecodeErrorKind, Frame, MAGIC, RECORD_FRAMING, Recording, TAG_FRAME, TAG_INPUT,
    VERSION,
    chunk::{
        CHUNK_HEADER_BYTES, Chunk, MAX_CHUNK_BYTES, OPTIONAL_TAG_BIT, TAG_CHUNK, bounded, invalid,
    },
    codec,
    input::{CapturedInput, MAX_INPUT_PAYLOAD},
    input_codec,
    wire::Decoder,
};

/// Limits for a whole-file decode. These are resource limits, not duration or sample-rate limits.
/// Callers processing larger recordings can supply larger budgets with [`decode_with_limits`].
#[derive(Debug, Copy, Clone)]
pub struct DecodeLimits {
    pub max_file_bytes: u64,
    pub max_expanded_bytes: u64,
    /// A conservative allocation projection, including vector growth and dynamic record fields.
    pub max_projected_bytes: u64,
    pub max_records: u64,
    pub max_chunks: u64,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 8 * 1024 * 1024 * 1024,
            max_expanded_bytes: 16 * 1024 * 1024 * 1024,
            max_projected_bytes: 64 * 1024 * 1024 * 1024,
            max_records: 8_000_000,
            max_chunks: 1_000_000,
        }
    }
}

/// Read a recording within explicit byte, allocation-projection, record, and chunk budgets.
/// A complete malformed chunk is an error; only a cut final outer record is recoverable.
pub fn decode_with_limits(bytes: &[u8], limits: DecodeLimits) -> Result<Recording, DecodeError> {
    limit(bytes.len() as u64, limits.max_file_bytes, "file bytes", 0)?;
    let mut reader = Decoder::new(bytes);
    let magic = reader.take(MAGIC.len()).map_err(|error| DecodeError {
        kind: DecodeErrorKind::BadMagic,
        ..error
    })?;
    if magic != MAGIC {
        return Err(DecodeError {
            offset: 0,
            kind: DecodeErrorKind::BadMagic,
        });
    }
    let version_offset = reader.offset();
    let version = reader.u16()?;
    if version == 0 || version > VERSION {
        return Err(DecodeError {
            offset: version_offset,
            kind: DecodeErrorKind::UnsupportedVersion(version),
        });
    }
    let header_len = reader.u32()? as usize;
    bounded(
        header_len,
        MAX_HEADER_BYTES,
        "header bytes",
        reader.offset(),
    )?;
    let mut budget = Budget::new(limits);
    budget.add(header_len, 0, reader.offset())?;
    let mut header_reader = reader.sub(header_len)?;
    let header = codec::read_header(&mut header_reader)?;
    header_reader.finish("header")?;
    let mut recording = Recording {
        header,
        frames: Vec::new(),
        input: Vec::new(),
        version,
        truncated: false,
        skipped_records: 0,
    };
    while !reader.is_empty() {
        if reader.remaining() < RECORD_FRAMING {
            recording.truncated = true;
            break;
        }
        let tag = reader.u8()?;
        let len = reader.u32()? as usize;
        if version == 3 {
            if tag != TAG_CHUNK && tag & OPTIONAL_TAG_BIT == 0 {
                return Err(invalid(
                    "required outer tag",
                    u64::from(tag),
                    reader.offset() - RECORD_FRAMING,
                ));
            }
            bounded(
                len,
                MAX_CHUNK_BYTES + CHUNK_HEADER_BYTES,
                "outer payload bytes",
                reader.offset(),
            )?;
        } else {
            record_bound(tag, len, reader.offset())?;
        }
        if reader.remaining() < len {
            recording.truncated = true;
            break;
        }
        let mut payload = reader.sub(len)?;
        if version == 3 && tag == TAG_CHUNK {
            let offset = payload.offset();
            let chunk = Chunk::read(&mut payload)?;
            budget.chunk(offset)?;
            budget.add(chunk.expanded, chunk.count, offset)?;
            let expanded = chunk.expand()?;
            let result = read_chunk_records(&expanded, chunk.count, &mut recording);
            result.map_err(|error| DecodeError { offset, ..error })?;
        } else {
            budget.add(len, 1, payload.offset())?;
            append(tag, &mut payload, &mut recording)?;
        }
    }
    Ok(recording)
}

const MAX_HEADER_BYTES: usize = 256 * 1024;
const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

pub(crate) fn record_bound(tag: u8, len: usize, offset: usize) -> Result<(), DecodeError> {
    let (max, field) = match tag {
        TAG_INPUT => (MAX_INPUT_PAYLOAD, "input payload length"),
        TAG_FRAME => (MAX_FRAME_BYTES, "frame payload length"),
        _ => (MAX_CHUNK_BYTES, "record payload bytes"),
    };
    bounded(len, max, field, offset)
}

fn read_chunk_records(
    bytes: &[u8],
    count: usize,
    recording: &mut Recording,
) -> Result<(), DecodeError> {
    let mut inner = Decoder::new(bytes);
    for _ in 0..count {
        let tag = inner.u8()?;
        if tag != TAG_FRAME && tag != TAG_INPUT && tag & OPTIONAL_TAG_BIT == 0 {
            return Err(invalid("chunk inner tag", u64::from(tag), inner.offset()));
        }
        let len = inner.u32()? as usize;
        record_bound(tag, len, inner.offset())?;
        let mut record = inner.sub(len)?;
        append(tag, &mut record, recording)?;
    }
    inner.finish("chunk records")
}

fn append(tag: u8, payload: &mut Decoder, recording: &mut Recording) -> Result<(), DecodeError> {
    match tag {
        TAG_FRAME => {
            recording.frames.push(codec::read_frame(payload)?);
            payload.finish("frame")?;
        }
        TAG_INPUT if recording.version >= 2 => {
            recording.input.push(input_codec::decode(payload)?);
            payload.finish("input")?;
        }
        _ => {
            recording.skipped_records = recording
                .skipped_records
                .checked_add(1)
                .ok_or_else(|| invalid("skipped records", u64::MAX, payload.offset()))?;
        }
    }
    Ok(())
}

struct Budget {
    limits: DecodeLimits,
    expanded: u64,
    projected: u64,
    records: u64,
    chunks: u64,
}

impl Budget {
    fn new(limits: DecodeLimits) -> Self {
        Self {
            limits,
            expanded: 0,
            projected: 0,
            records: 0,
            chunks: 0,
        }
    }

    fn chunk(&mut self, offset: usize) -> Result<(), DecodeError> {
        self.chunks = checked_add(self.chunks, 1, "chunks", offset)?;
        limit(self.chunks, self.limits.max_chunks, "chunks", offset)
    }

    fn add(&mut self, bytes: usize, count: usize, offset: usize) -> Result<(), DecodeError> {
        self.expanded = checked_add(self.expanded, bytes as u64, "expanded bytes", offset)?;
        self.records = checked_add(self.records, count as u64, "records", offset)?;
        // Dynamic fields have at least two wire bytes per allocation. The multiplier also covers
        // their allocation headers and vector growth; the static term covers sparse frames.
        let dynamic = (bytes as u64)
            .checked_mul(32)
            .ok_or_else(|| invalid("allocation projection", u64::MAX, offset))?;
        let fixed = (count as u64)
            .checked_mul((4 * (size_of::<Frame>() + size_of::<CapturedInput>())) as u64)
            .ok_or_else(|| invalid("allocation projection", u64::MAX, offset))?;
        let projected = checked_add(dynamic, fixed, "allocation projection", offset)?;
        self.projected = checked_add(self.projected, projected, "allocation projection", offset)?;
        limit(
            self.expanded,
            self.limits.max_expanded_bytes,
            "expanded bytes",
            offset,
        )?;
        limit(self.records, self.limits.max_records, "records", offset)?;
        limit(
            self.projected,
            self.limits.max_projected_bytes,
            "allocation projection",
            offset,
        )
    }
}

fn checked_add(
    left: u64,
    right: u64,
    field: &'static str,
    offset: usize,
) -> Result<u64, DecodeError> {
    left.checked_add(right)
        .ok_or_else(|| invalid(field, u64::MAX, offset))
}

fn limit(value: u64, max: u64, field: &'static str, offset: usize) -> Result<(), DecodeError> {
    if value > max {
        Err(invalid(field, value, offset))
    } else {
        Ok(())
    }
}
