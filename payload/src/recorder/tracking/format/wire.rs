//! The primitive encoding of the tracking file: little-endian integers and floats, length-prefixed
//! UTF-8 strings, and the decode error every reader returns.

use std::fmt;

use glam::{Quat, Vec2, Vec3};

use crate::controllers::frame::Pose;

/// Why a tracking file could not be read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DecodeError {
    /// The byte offset in the file at which the read failed.
    pub(crate) offset: usize,
    pub(crate) kind: DecodeErrorKind,
}

/// What was wrong at a [`DecodeError`]'s offset.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DecodeErrorKind {
    /// The file does not start with the tracking file's magic.
    BadMagic,
    /// The file is of a format version this build cannot read.
    UnsupportedVersion(u16),
    /// The data ends before a value it announces.
    Truncated { needed: usize, available: usize },
    /// A value outside the range its field allows.
    InvalidValue { field: &'static str, value: u64 },
    /// A string that is not UTF-8.
    InvalidUtf8 { field: &'static str },
    /// A record whose payload holds bytes beyond the fields it declares.
    TrailingBytes { record: &'static str, extra: usize },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tracking file: at byte {}: {}", self.offset, self.kind)
    }
}

impl fmt::Display for DecodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeErrorKind::BadMagic => write!(f, "not a jc3vr tracking recording"),
            DecodeErrorKind::UnsupportedVersion(version) => write!(
                f,
                "format version {version} is not supported by this build, which reads version {}",
                crate::recorder::tracking::format::VERSION
            ),
            DecodeErrorKind::Truncated { needed, available } => write!(
                f,
                "the data ends early: {needed} bytes needed, {available} available"
            ),
            DecodeErrorKind::InvalidValue { field, value } => {
                write!(f, "{field} holds the invalid value {value}")
            }
            DecodeErrorKind::InvalidUtf8 { field } => write!(f, "{field} is not UTF-8"),
            DecodeErrorKind::TrailingBytes { record, extra } => {
                write!(f, "a {record} record has {extra} unread bytes")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// An append-only byte buffer with the file's primitive encodings.
#[derive(Default)]
pub(crate) struct Encoder {
    pub(crate) bytes: Vec<u8>,
}

impl Encoder {
    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    /// A string, as its byte length (`u16`, truncated at a character boundary to fit) and UTF-8.
    pub(crate) fn str(&mut self, value: &str) {
        let mut end = value.len().min(usize::from(u16::MAX));
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.u16(end as u16);
        self.bytes.extend_from_slice(&value.as_bytes()[..end]);
    }

    pub(crate) fn vec2(&mut self, value: Vec2) {
        self.f32(value.x);
        self.f32(value.y);
    }

    pub(crate) fn vec3(&mut self, value: Vec3) {
        self.f32(value.x);
        self.f32(value.y);
        self.f32(value.z);
    }

    pub(crate) fn quat(&mut self, value: Quat) {
        self.f32(value.x);
        self.f32(value.y);
        self.f32(value.z);
        self.f32(value.w);
    }

    pub(crate) fn pose(&mut self, value: &Pose) {
        self.vec3(value.position);
        self.quat(value.orientation);
    }
}

/// A cursor over the file's bytes with the primitive decodings. Every read reports its offset in
/// the whole file, so a nested reader over one record is created with [`Decoder::sub`].
pub(crate) struct Decoder<'a> {
    bytes: &'a [u8],
    position: usize,
    /// The offset of `bytes` in the file.
    base: usize,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            base: 0,
        }
    }

    /// The offset in the file of the next read.
    pub(crate) fn offset(&self) -> usize {
        self.base + self.position
    }

    /// How many bytes are left.
    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// An error of `kind` at the next read's offset.
    pub(crate) fn error(&self, kind: DecodeErrorKind) -> DecodeError {
        DecodeError {
            offset: self.offset(),
            kind,
        }
    }

    /// The next `len` bytes.
    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining() < len {
            return Err(self.error(DecodeErrorKind::Truncated {
                needed: len,
                available: self.remaining(),
            }));
        }
        let slice = &self.bytes[self.position..self.position + len];
        self.position += len;
        Ok(slice)
    }

    /// A reader over the next `len` bytes, which this reader skips.
    pub(crate) fn sub(&mut self, len: usize) -> Result<Decoder<'a>, DecodeError> {
        let base = self.offset();
        let bytes = self.take(len)?;
        Ok(Decoder {
            bytes,
            position: 0,
            base,
        })
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub(crate) fn i64(&mut self) -> Result<i64, DecodeError> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub(crate) fn bool(&mut self, field: &'static str) -> Result<bool, DecodeError> {
        let offset = self.offset();
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(DecodeError {
                offset,
                kind: DecodeErrorKind::InvalidValue {
                    field,
                    value: u64::from(value),
                },
            }),
        }
    }

    pub(crate) fn str(&mut self, field: &'static str) -> Result<String, DecodeError> {
        let len = usize::from(self.u16()?);
        let offset = self.offset();
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError {
            offset,
            kind: DecodeErrorKind::InvalidUtf8 { field },
        })
    }

    pub(crate) fn vec2(&mut self) -> Result<Vec2, DecodeError> {
        Ok(Vec2::new(self.f32()?, self.f32()?))
    }

    pub(crate) fn vec3(&mut self) -> Result<Vec3, DecodeError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    pub(crate) fn quat(&mut self) -> Result<Quat, DecodeError> {
        Ok(Quat::from_xyzw(
            self.f32()?,
            self.f32()?,
            self.f32()?,
            self.f32()?,
        ))
    }

    pub(crate) fn pose(&mut self) -> Result<Pose, DecodeError> {
        Ok(Pose {
            position: self.vec3()?,
            orientation: self.quat()?,
        })
    }

    /// Fail unless every byte has been read; `record` names the record for the error.
    pub(crate) fn finish(&self, record: &'static str) -> Result<(), DecodeError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(self.error(DecodeErrorKind::TrailingBytes {
                record,
                extra: self.remaining(),
            }))
        }
    }
}
