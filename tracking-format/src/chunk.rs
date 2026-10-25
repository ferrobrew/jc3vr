//! Independent, bounded storage of complete serialized capture batches.

use lz4_flex::block::{compress, decompress_into};

use crate::{
    DecodeError, DecodeErrorKind, RECORD_FRAMING, TAG_FRAME, TAG_INPUT,
    wire::{Decoder, Encoder},
};

/// The version 3 atomic capture chunk tag. Its payload cannot contain another chunk.
pub const TAG_CHUNK: u8 = 3;
/// Unknown version 3 tags with this bit set are optional and can be skipped.
pub const OPTIONAL_TAG_BIT: u8 = 0x80;
/// Maximum expanded bytes in one independently recoverable chunk.
pub const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// Maximum complete records in one chunk.
pub const MAX_CHUNK_RECORDS: usize = 4096;

/// Store complete frame/input records without changing any serialized byte. LZ4 is used only
/// when its block is smaller than the original bytes. This function belongs on the writer thread.
pub fn encode_chunk(records: &[u8]) -> Result<Vec<u8>, DecodeError> {
    bounded(records.len(), MAX_CHUNK_BYTES, "chunk expanded bytes", 0)?;
    let count = validate_records(records)?;
    let compressed = compress(records);
    let (storage, stored) = if compressed.len() < records.len() {
        (1, compressed.as_slice())
    } else {
        (0, records)
    };
    let mut metadata = Encoder::default();
    metadata.u8(storage);
    metadata.u8(0);
    metadata.u16(0);
    metadata.u32(records.len() as u32);
    metadata.u32(stored.len() as u32);
    metadata.u32(count as u32);
    let checksum = checksum(&metadata.bytes, stored);
    let mut out = Encoder::default();
    out.u8(TAG_CHUNK);
    out.u32((CHUNK_HEADER_BYTES + stored.len()) as u32);
    out.bytes.extend_from_slice(&metadata.bytes);
    out.u32(checksum);
    out.bytes.extend_from_slice(stored);
    Ok(out.bytes)
}

pub(crate) const CHUNK_HEADER_BYTES: usize = 20;

pub(crate) struct Chunk<'a> {
    storage: u8,
    pub(crate) expanded: usize,
    pub(crate) count: usize,
    stored: &'a [u8],
    offset: usize,
}

impl<'a> Chunk<'a> {
    pub(crate) fn read(reader: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        let offset = reader.offset();
        let metadata = reader.take(16)?;
        let mut fields = Decoder::new(metadata);
        let storage = fields.u8()?;
        bounded(usize::from(storage), 1, "chunk storage", offset)?;
        let flags = fields.u8()?;
        bounded(usize::from(flags), 0, "chunk flags", offset)?;
        let reserved = fields.u16()?;
        bounded(usize::from(reserved), 0, "chunk reserved", offset)?;
        let expanded = fields.u32()? as usize;
        let stored_len = fields.u32()? as usize;
        let count = fields.u32()? as usize;
        bounded(expanded, MAX_CHUNK_BYTES, "chunk expanded bytes", offset)?;
        bounded(stored_len, MAX_CHUNK_BYTES, "chunk stored bytes", offset)?;
        bounded(count, MAX_CHUNK_RECORDS, "chunk records", offset)?;
        if count == 0 || expanded < count.saturating_mul(RECORD_FRAMING) {
            return Err(invalid("chunk record count", count as u64, offset));
        }
        if (storage == 0 && stored_len != expanded)
            || (storage == 1 && (stored_len == 0 || stored_len >= expanded))
        {
            return Err(invalid("chunk storage lengths", stored_len as u64, offset));
        }
        let expected = reader.u32()?;
        let stored = reader.take(stored_len)?;
        reader.finish("chunk")?;
        if checksum(metadata, stored) != expected {
            return Err(DecodeError {
                offset,
                kind: DecodeErrorKind::ChunkChecksum,
            });
        }
        Ok(Self {
            storage,
            expanded,
            count,
            stored,
            offset,
        })
    }

    pub(crate) fn expand(&self) -> Result<Vec<u8>, DecodeError> {
        if self.storage == 1 {
            validate_lz4(self.stored, self.expanded).map_err(|_| DecodeError {
                offset: self.offset,
                kind: DecodeErrorKind::ChunkDecompression,
            })?;
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(self.expanded)
            .map_err(|_| DecodeError {
                offset: self.offset,
                kind: DecodeErrorKind::Allocation,
            })?;
        output.resize(self.expanded, 0);
        if self.storage == 0 {
            output.copy_from_slice(self.stored);
        } else {
            let written = decompress_into(self.stored, &mut output).map_err(|_| DecodeError {
                offset: self.offset,
                kind: DecodeErrorKind::ChunkDecompression,
            })?;
            if written != self.expanded {
                return Err(invalid(
                    "chunk decompressed length",
                    written as u64,
                    self.offset,
                ));
            }
        }
        Ok(output)
    }
}

pub(crate) fn validate_records(bytes: &[u8]) -> Result<usize, DecodeError> {
    let mut reader = Decoder::new(bytes);
    let mut count = 0;
    while !reader.is_empty() {
        let tag = reader.u8()?;
        if tag != TAG_FRAME && tag != TAG_INPUT && tag & OPTIONAL_TAG_BIT == 0 {
            return Err(invalid(
                "chunk inner tag",
                u64::from(tag),
                reader.offset() - 1,
            ));
        }
        let len = reader.u32()? as usize;
        crate::read::record_bound(tag, len, reader.offset())?;
        reader.take(len)?;
        count += 1;
        bounded(count, MAX_CHUNK_RECORDS, "chunk records", reader.offset())?;
    }
    if count == 0 {
        return Err(invalid("chunk record count", 0, 0));
    }
    Ok(count)
}

pub(crate) fn bounded(
    value: usize,
    limit: usize,
    field: &'static str,
    offset: usize,
) -> Result<(), DecodeError> {
    if value > limit {
        Err(invalid(field, value as u64, offset))
    } else {
        Ok(())
    }
}

pub(crate) fn invalid(field: &'static str, value: u64, offset: usize) -> DecodeError {
    DecodeError {
        offset,
        kind: DecodeErrorKind::InvalidValue { field, value },
    }
}

// This checks bounded block structure, not the encoder's final-literal or last-match conventions.
// The terminal token has no match, so its unused low nibble does not constrain the block.
fn validate_lz4(bytes: &[u8], expected: usize) -> Result<(), ()> {
    let mut input = Decoder::new(bytes);
    let mut produced = 0usize;
    loop {
        let token = input.u8().map_err(|_| ())?;
        let literals = lz4_length(&mut input, usize::from(token >> 4))?;
        produced = produced.checked_add(literals).ok_or(())?;
        if produced > expected {
            return Err(());
        }
        input.take(literals).map_err(|_| ())?;
        if input.is_empty() {
            return if produced == expected {
                Ok(())
            } else {
                Err(())
            };
        }
        // A full output cannot be followed by another sequence, even a zero-literal token.
        if produced == expected {
            return Err(());
        }
        let distance = usize::from(input.u16().map_err(|_| ())?);
        if distance == 0 || distance > produced {
            return Err(());
        }
        let matched = lz4_length(&mut input, usize::from(token & 15))?
            .checked_add(4)
            .ok_or(())?;
        produced = produced.checked_add(matched).ok_or(())?;
        if produced >= expected || input.is_empty() {
            return Err(());
        }
    }
}

fn lz4_length(input: &mut Decoder, base: usize) -> Result<usize, ()> {
    let mut length = base;
    if base == 15 {
        loop {
            let extra = input.u8().map_err(|_| ())?;
            length = length.checked_add(usize::from(extra)).ok_or(())?;
            if extra != 255 {
                break;
            }
        }
    }
    Ok(length)
}

fn checksum(metadata: &[u8], stored: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in metadata.iter().chain(stored) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests;
