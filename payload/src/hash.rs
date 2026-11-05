//! The engine's name hash, and tables of animation rule states keyed by it.
//!
//! The game reduces names (file paths, animation states, bones, commands, localisation keys) to Bob
//! Jenkins' lookup3 `hashlittle` with a zero seed: the hash a [`jc3gi::hash::HashString`] stores and
//! the one `jc3gi::hash::hashlittle` calls in the game's code. This is a pure reimplementation of
//! it, so a name can be hashed without the game (from tests, and before the game's code is mapped),
//! and every caller in the mod uses this one.
//!
//! A rule state is identified by the hash of its name, so the mod names the states it reacts to and
//! hashes them once into a [`StateTable`] (or a [`StateSet`]), rather than embedding hash values.

use std::collections::HashMap;

/// Bob Jenkins' lookup3 `hashlittle` of `data` with a zero seed: the game's name hash.
pub(crate) fn hashlittle(data: &[u8]) -> u32 {
    let mut a = 0xdead_beef_u32.wrapping_add(data.len() as u32);
    let (mut b, mut c) = (a, a);
    let mut rest = data;
    while rest.len() > 12 {
        a = a.wrapping_add(word(rest, 0));
        b = b.wrapping_add(word(rest, 4));
        c = c.wrapping_add(word(rest, 8));
        (a, b, c) = mix(a, b, c);
        rest = &rest[12..];
    }
    if rest.is_empty() {
        return c;
    }
    let mut tail = [0u8; 12];
    tail[..rest.len()].copy_from_slice(rest);
    a = a.wrapping_add(word(&tail, 0));
    b = b.wrapping_add(word(&tail, 4));
    c = c.wrapping_add(word(&tail, 8));
    finalize(a, b, c)
}

/// A value for each of a set of rule states, by the hash of their names.
#[derive(Debug, Clone)]
pub(crate) struct StateTable<T> {
    entries: HashMap<u32, T>,
}

impl<T: Copy> StateTable<T> {
    /// Hash each state's name. A name listed twice keeps its last value.
    pub(crate) fn new<'a>(entries: impl IntoIterator<Item = (&'a str, T)>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(name, value)| (hashlittle(name.as_bytes()), value))
                .collect(),
        }
    }

    /// The value of the rule state whose name hash is `state`, if it is one of the table's.
    pub(crate) fn get(&self, state: u32) -> Option<T> {
        self.entries.get(&state).copied()
    }
}

/// A set of rule states, by the hash of their names.
#[derive(Debug, Clone)]
pub(crate) struct StateSet(StateTable<()>);

impl StateSet {
    /// Hash each state's name.
    pub(crate) fn new<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self(StateTable::new(names.into_iter().map(|name| (name, ()))))
    }

    /// Whether the rule state whose name hash is `state` is one of the set's.
    pub(crate) fn contains(&self, state: u32) -> bool {
        self.0.get(state).is_some()
    }
}

/// The little-endian word at `at`.
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn mix(mut a: u32, mut b: u32, mut c: u32) -> (u32, u32, u32) {
    a = a.wrapping_sub(c);
    a ^= c.rotate_left(4);
    c = c.wrapping_add(b);
    b = b.wrapping_sub(a);
    b ^= a.rotate_left(6);
    a = a.wrapping_add(c);
    c = c.wrapping_sub(b);
    c ^= b.rotate_left(8);
    b = b.wrapping_add(a);
    a = a.wrapping_sub(c);
    a ^= c.rotate_left(16);
    c = c.wrapping_add(b);
    b = b.wrapping_sub(a);
    b ^= a.rotate_left(19);
    a = a.wrapping_add(c);
    c = c.wrapping_sub(b);
    c ^= b.rotate_left(4);
    b = b.wrapping_add(a);
    (a, b, c)
}

fn finalize(mut a: u32, mut b: u32, mut c: u32) -> u32 {
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(14));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(11));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(25));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(16));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(4));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(14));
    c ^= b;
    c.wrapping_sub(b.rotate_left(24))
}

#[cfg(test)]
mod tests {
    use jc3gi::ui::ui_base::UIBase;

    use super::*;

    #[test]
    fn hashlittle_matches_the_reference_values() {
        // The reference implementation's published test vectors (seed 0).
        assert_eq!(hashlittle(b""), 0xdead_beef);
        assert_eq!(hashlittle(b"Four score and seven years ago"), 0x1777_0551);
        // A path the archives index, as the offline tooling hashes it.
        assert_eq!(
            hashlittle(b"textures/ui/weapon_icons/301.ddsc"),
            0xad9e_c3bf
        );
    }

    #[test]
    fn hashlittle_matches_the_games_command_hashes() {
        assert_eq!(hashlittle(b"active"), UIBase::COMMAND_ACTIVE);
        assert_eq!(hashlittle(b"inactive"), UIBase::COMMAND_INACTIVE);
        assert_eq!(hashlittle(b"event"), UIBase::COMMAND_EVENT);
    }

    #[test]
    fn hashlittle_covers_every_tail_length() {
        let text = b"textures/ui/weapon_icons/301.ddsc";
        let hashes: Vec<u32> = (0..=text.len()).map(|n| hashlittle(&text[..n])).collect();
        let mut unique = hashes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), hashes.len());
    }

    #[test]
    fn a_state_table_looks_states_up_by_their_name_hash() {
        let table = StateTable::new([("S_IDLE", 1), ("S_RUN", 2)]);
        assert_eq!(table.get(hashlittle(b"S_IDLE")), Some(1));
        assert_eq!(table.get(hashlittle(b"S_RUN")), Some(2));
        assert_eq!(table.get(hashlittle(b"S_JUMP")), None);
        let set = StateSet::new(["S_IDLE"]);
        assert!(set.contains(hashlittle(b"S_IDLE")));
        assert!(!set.contains(hashlittle(b"S_RUN")));
    }
}
