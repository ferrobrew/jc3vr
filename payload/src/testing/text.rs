//! Bounded admission for diagnostic TOML input on the game thread.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub(super) const REQUEST_BYTES: usize = 8 * 1024;
pub(super) const SCENARIO_BYTES: usize = 64 * 1024;
pub(super) const STRING_BYTES: usize = 4096;
pub(super) const EXPECTATIONS: usize = 128;

#[derive(Debug)]
pub(crate) enum TextError {
    Io(io::Error),
    Utf8(std::string::FromUtf8Error),
    Limit {
        field: &'static str,
        actual: usize,
        max: usize,
    },
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "diagnostic input: ")?;
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Utf8(error) => write!(f, "invalid UTF-8: {error}"),
            Self::Limit { field, actual, max } => write!(
                f,
                "{field} has {actual} bytes or entries; the limit is {max}"
            ),
        }
    }
}

impl std::error::Error for TextError {}

pub(super) fn read(path: &Path, max: usize) -> Result<String, TextError> {
    read_from(File::open(path).map_err(TextError::Io)?, max)
}

pub(super) fn limit(field: &'static str, actual: usize, max: usize) -> Result<(), TextError> {
    if actual > max {
        Err(TextError::Limit { field, actual, max })
    } else {
        Ok(())
    }
}

pub(super) fn string(field: &'static str, value: &str) -> Result<(), TextError> {
    limit(field, value.len(), STRING_BYTES)
}

fn read_from(reader: impl Read, max: usize) -> Result<String, TextError> {
    let mut reader = reader.take((max + 1) as u64);
    let mut bytes = Vec::with_capacity(max + 1);
    let mut buffer = [0; 4096];
    // Even one-byte reads and interrupted calls have a finite game-thread work bound.
    for _ in 0..max + 17 {
        match reader.read(&mut buffer) {
            Ok(0) => return String::from_utf8(bytes).map_err(TextError::Utf8),
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                limit("file", bytes.len(), max)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(TextError::Io(error)),
        }
    }
    Err(TextError::Io(io::Error::new(
        io::ErrorKind::Interrupted,
        "diagnostic read attempt limit exceeded",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_limit_sentinel_growth_and_interruptions_are_bounded() {
        for max in [0, 1, 4096, 4097] {
            assert_eq!(read_from(&vec![b'a'; max][..], max).unwrap().len(), max);
            assert!(
                matches!(read_from(&vec![b'a'; max + 20][..], max), Err(TextError::Limit { actual, .. }) if actual == max + 1)
            );
        }
        struct Growing {
            position: usize,
        }
        impl Read for Growing {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                bytes[0] = b'a';
                self.position += 1;
                Ok(1)
            }
        }
        assert!(matches!(
            read_from(Growing { position: 0 }, 4),
            Err(TextError::Limit { actual: 5, .. })
        ));
        struct Interrupted;
        impl Read for Interrupted {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::Interrupted.into())
            }
        }
        assert!(matches!(read_from(Interrupted, 4), Err(TextError::Io(_))));
        assert!(matches!(read_from(&[255][..], 4), Err(TextError::Utf8(_))));
    }
}
