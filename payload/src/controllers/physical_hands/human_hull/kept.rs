//! The shape pairs an eject left in place, recorded in the game process's environment.
//!
//! An eject that cannot restore every narrowed body in time leaves the replacements in the settings
//! tables and on live bodies, and keeps a reference to each original and each replacement for the
//! life of the process ([`NativeState::commit_in_place`]). Those shapes are never freed, so their
//! addresses name them for as long as the game runs. The next payload reads the pairs back and
//! recognises a slot that holds one of them, instead of reading a replacement as authored geometry
//! and narrowing it again. The environment outlives the payload DLL, as it does for the persisted
//! OpenXR handles.
//!
//! One variable holds every pair, as `version;index:original:replacement,...` in hex.
//!
//! [`NativeState::commit_in_place`]: crate::controllers::physical_hands::human_hull::native::NativeState::commit_in_place

use windows::{
    Win32::System::Environment::{GetEnvironmentVariableW, SetEnvironmentVariableW},
    core::{PCWSTR, w},
};

/// A settings slot's authored shape and the replacement installed for it, both kept alive by the
/// references an earlier payload left behind.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct KeptPair {
    /// The settings slot, a [`jc3gi::physics::ProxyState`] index.
    pub(super) index: usize,
    pub(super) original: usize,
    pub(super) replacement: usize,
}

/// The kept pair for slot `index` whose original or replacement is `shape`.
pub(super) fn find(pairs: &[KeptPair], index: usize, shape: usize) -> Option<KeptPair> {
    pairs
        .iter()
        .copied()
        .find(|pair| pair.index == index && (pair.replacement == shape || pair.original == shape))
}

/// Add `pairs` to `kept`, skipping any already recorded. Returns whether anything was added.
pub(super) fn merge(kept: &mut Vec<KeptPair>, pairs: impl IntoIterator<Item = KeptPair>) -> bool {
    let before = kept.len();
    for pair in pairs {
        if !kept.contains(&pair) {
            kept.push(pair);
        }
    }
    kept.len() != before
}

/// The pairs recorded by earlier payloads in this process, empty when there are none.
pub(super) fn load() -> Vec<KeptPair> {
    // SAFETY: a null buffer asks for the required size, including the terminator.
    let needed = unsafe { GetEnvironmentVariableW(KEPT_VAR, None) };
    if needed == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; needed as usize];
    // SAFETY: `buffer` is a valid out-parameter of the length passed.
    let len = unsafe { GetEnvironmentVariableW(KEPT_VAR, Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() {
        return Vec::new();
    }
    decode(&String::from_utf16_lossy(&buffer[..len])).unwrap_or_else(|| {
        tracing::warn!("human hull: ignoring a malformed record of the shapes left in place");
        Vec::new()
    })
}

/// Record `pairs` for the next payload in this process.
pub(super) fn store(pairs: &[KeptPair]) {
    let value: Vec<u16> = encode(pairs)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both strings are valid and null-terminated for the duration of the call.
    if let Err(error) = unsafe { SetEnvironmentVariableW(KEPT_VAR, PCWSTR(value.as_ptr())) } {
        tracing::warn!("human hull: could not record the shapes left in place: {error}");
    }
}

/// The format version. A payload that changes the format bumps it, so it never misreads an older
/// payload's record; an unreadable record is ignored, and its slots read as authored.
const VERSION: &str = "1";

const KEPT_VAR: PCWSTR = w!("jc3vr_human_hull_kept");

fn encode(pairs: &[KeptPair]) -> String {
    let pairs: Vec<String> = pairs
        .iter()
        .map(|pair| {
            format!(
                "{:x}:{:x}:{:x}",
                pair.index, pair.original, pair.replacement
            )
        })
        .collect();
    format!("{VERSION};{}", pairs.join(","))
}

fn decode(text: &str) -> Option<Vec<KeptPair>> {
    let (version, pairs) = text.trim().split_once(';')?;
    if version != VERSION {
        return None;
    }
    if pairs.is_empty() {
        return Some(Vec::new());
    }
    let hex = |s: &str| usize::from_str_radix(s, 16).ok();
    pairs
        .split(',')
        .map(|pair| {
            let mut fields = pair.split(':');
            let pair = KeptPair {
                index: hex(fields.next()?)?,
                original: hex(fields.next()?).filter(|&shape| shape != 0)?,
                replacement: hex(fields.next()?).filter(|&shape| shape != 0)?,
            };
            fields.next().is_none().then_some(pair)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: KeptPair = KeptPair {
        index: 0,
        original: 0x1_0000,
        replacement: 0x2_0000,
    };
    const B: KeptPair = KeptPair {
        index: 9,
        original: 0xdead_beef_0010,
        replacement: 0xdead_beef_0020,
    };

    #[test]
    fn the_record_reads_back_as_written() {
        assert_eq!(decode(&encode(&[A, B])), Some(vec![A, B]));
        assert_eq!(decode(&encode(&[])), Some(vec![]));
    }

    #[test]
    fn a_malformed_or_foreign_record_is_rejected_whole() {
        assert_eq!(decode("2;0:1:2"), None);
        assert_eq!(decode("1;0:1"), None);
        assert_eq!(decode("1;0:1:2:3"), None);
        assert_eq!(decode("1;0:0:2"), None);
        assert_eq!(decode("1;0:1:2,zz:1:2"), None);
        assert_eq!(decode("garbage"), None);
    }

    #[test]
    fn a_slot_matches_either_shape_of_its_own_pair_only() {
        let pairs = [A, B];
        assert_eq!(find(&pairs, 0, A.replacement), Some(A));
        assert_eq!(find(&pairs, 0, A.original), Some(A));
        assert_eq!(find(&pairs, 1, A.replacement), None);
        assert_eq!(find(&pairs, 0, 0x3_0000), None);
    }

    #[test]
    fn merging_records_each_pair_once() {
        let mut kept = vec![A];
        assert!(merge(&mut kept, [A, B, B]));
        assert_eq!(kept, [A, B]);
        assert!(!merge(&mut kept, [B]));
    }
}
