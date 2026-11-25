//! Persisting the action handles across inject cycles.
//!
//! The mod keeps the OpenXR instance and session alive across an uninject and reinject
//! ([`crate::vr::VrConfig::persist_instance`]). Action sets can be attached to a session only once,
//! so the reinjected payload cannot create and attach a fresh set to the persisted session: it must
//! re-wrap the handles of the set it attached before. Like the instance and session handles, they
//! are stashed in the game process's environment, which outlives the payload DLL.
//!
//! One variable holds everything, as `version;instance;session;layout;handle,handle,...` in hex.
//! The instance and session handles say which objects the actions belong to and are attached to,
//! and the layout fingerprint ([`crate::vr::input::layout::fingerprint`]) says whether the handles
//! are in the order this payload expects.

use windows::{
    Win32::System::Environment::{GetEnvironmentVariableW, SetEnvironmentVariableW},
    core::{PCWSTR, w},
};

/// The persisted action handles and what they belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputStash {
    /// The instance the action sets and actions were created on.
    pub(crate) instance: u64,
    /// The session the action sets were attached to.
    pub(crate) session: u64,
    /// The layout fingerprint of the payload that created them.
    pub(crate) layout: u64,
    /// The action set and action handles, in [`crate::vr::input::layout::stash_order`].
    pub(crate) handles: Vec<u64>,
}

impl InputStash {
    pub(crate) fn encode(&self) -> String {
        let handles: Vec<String> = self.handles.iter().map(|h| format!("{h:x}")).collect();
        format!(
            "{VERSION};{:x};{:x};{:x};{}",
            self.instance,
            self.session,
            self.layout,
            handles.join(",")
        )
    }

    /// Parse an encoded stash. `None` for any other version or a malformed value.
    pub(crate) fn decode(text: &str) -> Option<InputStash> {
        let mut fields = text.trim().split(';');
        if fields.next()? != VERSION {
            return None;
        }
        let hex = |s: &str| u64::from_str_radix(s, 16).ok();
        let instance = hex(fields.next()?)?;
        let session = hex(fields.next()?)?;
        let layout = hex(fields.next()?)?;
        let handles = fields
            .next()?
            .split(',')
            .map(hex)
            .collect::<Option<Vec<_>>>()?;
        if fields.next().is_some() {
            return None;
        }
        Some(InputStash {
            instance,
            session,
            layout,
            handles,
        })
    }
}

/// Stash the handles in the game process's environment.
pub(crate) fn store(stash: &InputStash) {
    let value: Vec<u16> = stash
        .encode()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both strings are valid and null-terminated for the duration of the call.
    if let Err(e) = unsafe { SetEnvironmentVariableW(STASH_VAR, PCWSTR(value.as_ptr())) } {
        tracing::warn!(target: "vr", "input: could not stash the action handles: {e}");
    }
}

/// Read and clear the stash. Clearing on read means a stash is used at most once: whatever the
/// caller does with the handles, the next teardown stashes the current ones again.
pub(crate) fn take() -> Option<InputStash> {
    // SAFETY: a null buffer asks for the required size, including the terminator.
    let needed = unsafe { GetEnvironmentVariableW(STASH_VAR, None) };
    if needed == 0 {
        return None;
    }
    let mut buffer = vec![0u16; needed as usize];
    // SAFETY: `buffer` is a valid out-parameter of the length passed.
    let len = unsafe { GetEnvironmentVariableW(STASH_VAR, Some(&mut buffer)) } as usize;
    clear();
    if len == 0 || len >= buffer.len() {
        return None;
    }
    let text = String::from_utf16_lossy(&buffer[..len]);
    let stash = InputStash::decode(&text);
    if stash.is_none() {
        tracing::warn!(target: "vr", "input: ignoring a malformed action-handle stash");
    }
    stash
}

/// Clear the stash: the runtime is genuinely stopping, so the handles die with it.
pub(crate) fn clear() {
    // SAFETY: a null value deletes the variable.
    let _ = unsafe { SetEnvironmentVariableW(STASH_VAR, PCWSTR::null()) };
}

/// The stash format version. A payload that changes the format bumps it, so it never misreads an
/// older payload's stash.
const VERSION: &str = "1";

const STASH_VAR: PCWSTR = w!("jc3vr_xr_input");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stash_round_trips() {
        let stash = InputStash {
            instance: 0x1234_5678_9abc,
            session: 0x42,
            layout: u64::MAX,
            handles: vec![1, 0xdead_beef, 0],
        };
        assert_eq!(InputStash::decode(&stash.encode()), Some(stash));
    }

    #[test]
    fn a_foreign_or_malformed_stash_is_rejected() {
        assert_eq!(InputStash::decode("2;1;2;3;4"), None);
        assert_eq!(InputStash::decode("1;1;2;3"), None);
        assert_eq!(InputStash::decode("1;1;2;3;4,zz"), None);
        assert_eq!(InputStash::decode("1;1;2;3;4;5"), None);
        assert_eq!(InputStash::decode(""), None);
    }
}
