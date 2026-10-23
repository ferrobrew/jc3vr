//! The save sidecar: the weapons the player carries beyond the slots, kept beside the game's save
//! in a file of the mod's own, since the save stores only the slots. Pure: the format, its
//! version, the fingerprint that ties a sidecar to the save it was written with, and what a load
//! restores.
//!
//! The game keeps one save per user and writes it often (autosaves, checkpoints), and the mod
//! cannot name which save a load reads. So the sidecar is written each time the game stores the
//! player's data, with a fingerprint of what the store writes (each slot's weapon type and every
//! reserve), and a load accepts it only when the same values come back out of the save. A sidecar
//! from another save, an older one, or a later one does not match, and nothing extra is carried.
//!
//! The file is TOML:
//!
//! ```toml
//! version = 1
//!
//! [fingerprint]
//! slots = [0, 1234, 0, 0, 0, 0, 0, 0]
//! reserves = [30, 0, ...]
//!
//! [[carried]]
//! tag_hash = 5678
//! magazine = 12
//! socket = "back_first"
//!
//! [[carried]]
//! tag_hash = 9012
//! magazine = 7
//! socket = "hip_left"
//! single = true
//! ```
//!
//! With the pistols made separate, `slot_pistol_single = true` says the pistol slot's weapon is one
//! pistol rather than the game's pair, so that a load brings it back as one pistol instead of
//! splitting it again, and `single = true` says the same of a carried pistol. A file without them
//! (written before the pistols were separate) leaves the slot's pistol to be split as a pair the
//! game handed over, and its carried pistols pairs.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::controllers::{loadout::Socket, weapons::inventory::sockets};

/// The format's version.
pub const VERSION: u32 = 1;

/// How many inventory slots the save stores a weapon type for.
pub const SLOTS: usize = 8;
/// How many reserves the save stores.
pub const RESERVES: usize = 17;

/// The values that tie a sidecar to a save: each slot's weapon type as its spawn tag hash (zero for
/// an empty slot, and for the heavy slot, which the save never stores), and every reserve.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub slots: [u32; SLOTS],
    pub reserves: [i32; RESERVES],
}

/// How a loaded fingerprint differs from the sidecar's.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Mismatch {
    Slot {
        index: usize,
        saved: u32,
        loaded: u32,
    },
    Reserve {
        index: usize,
        saved: i32,
        loaded: i32,
    },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::Slot {
                index,
                saved,
                loaded,
            } => write!(
                f,
                "slot {index} holds {loaded:#010x} but the sidecar has {saved:#010x}"
            ),
            Mismatch::Reserve {
                index,
                saved,
                loaded,
            } => write!(f, "reserve {index} is {loaded} but the sidecar has {saved}"),
        }
    }
}

impl Fingerprint {
    /// Whether the `loaded` fingerprint is this one's: every slot and reserve equal. A slot whose
    /// type the game does not know now (`known` says no, as for a weapon of content that is not
    /// installed) loads empty, and counts as equal.
    pub fn matches(
        &self,
        loaded: &Fingerprint,
        known: impl Fn(u32) -> bool,
    ) -> Result<(), Mismatch> {
        for (index, (&saved, &loaded)) in self.slots.iter().zip(&loaded.slots).enumerate() {
            let unknown_and_empty = loaded == 0 && saved != 0 && !known(saved);
            if saved != loaded && !unknown_and_empty {
                return Err(Mismatch::Slot {
                    index,
                    saved,
                    loaded,
                });
            }
        }
        for (index, (&saved, &loaded)) in self.reserves.iter().zip(&loaded.reserves).enumerate() {
            if saved != loaded {
                return Err(Mismatch::Reserve {
                    index,
                    saved,
                    loaded,
                });
            }
        }
        Ok(())
    }
}

/// One carried weapon: its type, its magazine, its socket, and whether it is a single pistol.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Record {
    pub tag_hash: u32,
    pub magazine: i32,
    pub socket: Socket,
    /// Whether the weapon is one pistol of the pistol slot rather than the game's pair
    /// ([`crate::controllers::weapons::dual_wield::single`]).
    pub single: bool,
}

/// A sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    pub fingerprint: Fingerprint,
    pub carried: Vec<Record>,
    /// Whether the pistol slot's weapon is one pistol, when the file says.
    pub slot_pistol_single: Option<bool>,
}

/// Why a sidecar could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarError {
    /// The text is not TOML, or not the format's shape.
    Malformed(String),
    /// The file is of another version.
    Version(u32),
    /// A carried weapon names a socket this build does not know.
    UnknownSocket(String),
}

impl fmt::Display for SidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SidecarError::Malformed(why) => write!(f, "sidecar: malformed: {why}"),
            SidecarError::Version(found) => write!(
                f,
                "sidecar: version {found} is not this build's ({VERSION})"
            ),
            SidecarError::UnknownSocket(socket) => {
                write!(f, "sidecar: unknown socket `{socket}`")
            }
        }
    }
}

impl std::error::Error for SidecarError {}

impl Sidecar {
    /// The sidecar as its file's text.
    pub fn encode(&self) -> String {
        let file = File {
            version: VERSION,
            fingerprint: FileFingerprint {
                slots: self.fingerprint.slots.to_vec(),
                reserves: self.fingerprint.reserves.to_vec(),
            },
            carried: self
                .carried
                .iter()
                .map(|r| FileRecord {
                    tag_hash: r.tag_hash,
                    magazine: r.magazine,
                    socket: sockets::key(r.socket).to_owned(),
                    single: r.single,
                })
                .collect(),
            slot_pistol_single: self.slot_pistol_single,
        };
        // The file's types are all plain numbers, strings, arrays, and tables, which serialize.
        toml::to_string(&file).unwrap_or_default()
    }

    /// Read a sidecar from its file's text.
    pub fn decode(text: &str) -> Result<Sidecar, SidecarError> {
        let version: Versioned =
            toml::from_str(text).map_err(|e| SidecarError::Malformed(e.message().to_owned()))?;
        if version.version != VERSION {
            return Err(SidecarError::Version(version.version));
        }
        let file: File =
            toml::from_str(text).map_err(|e| SidecarError::Malformed(e.message().to_owned()))?;
        let carried = file
            .carried
            .into_iter()
            .map(|r| {
                sockets::from_key(&r.socket)
                    .map(|socket| Record {
                        tag_hash: r.tag_hash,
                        magazine: r.magazine,
                        socket,
                        single: r.single,
                    })
                    .ok_or(SidecarError::UnknownSocket(r.socket))
            })
            .collect::<Result<_, _>>()?;
        let slots = <[u32; SLOTS]>::try_from(file.fingerprint.slots.as_slice()).map_err(|_| {
            SidecarError::Malformed(format!(
                "the fingerprint has {} slots, not {SLOTS}",
                file.fingerprint.slots.len()
            ))
        })?;
        let reserves =
            <[i32; RESERVES]>::try_from(file.fingerprint.reserves.as_slice()).map_err(|_| {
                SidecarError::Malformed(format!(
                    "the fingerprint has {} reserves, not {RESERVES}",
                    file.fingerprint.reserves.len()
                ))
            })?;
        Ok(Sidecar {
            fingerprint: Fingerprint { slots, reserves },
            carried,
            slot_pistol_single: file.slot_pistol_single,
        })
    }

    /// The carried weapons a load restores: those whose type the game knows now.
    pub fn restorable(&self, known: impl Fn(u32) -> bool) -> (Vec<Record>, usize) {
        let (known, unknown): (Vec<Record>, Vec<Record>) =
            self.carried.iter().partition(|r| known(r.tag_hash));
        (known, unknown.len())
    }
}

/// The file's version alone, read first so a file of another version is told apart from a broken
/// one.
#[derive(Deserialize)]
struct Versioned {
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    fingerprint: FileFingerprint,
    #[serde(default)]
    carried: Vec<FileRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slot_pistol_single: Option<bool>,
}

/// The fingerprint as the file has it, its lengths checked on reading.
#[derive(Serialize, Deserialize)]
struct FileFingerprint {
    slots: Vec<u32>,
    reserves: Vec<i32>,
}

#[derive(Serialize, Deserialize)]
struct FileRecord {
    tag_hash: u32,
    magazine: i32,
    socket: String,
    #[serde(default, skip_serializing_if = "is_false")]
    single: bool,
}

/// Whether `value` is false, so a record of a weapon that is not a single pistol leaves the key
/// out.
fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::frame::Hand;

    fn fingerprint() -> Fingerprint {
        let mut slots = [0; SLOTS];
        slots[1] = 0x1111_2222;
        slots[6] = 0x3333_4444;
        let mut reserves = [0; RESERVES];
        reserves[3] = 240;
        reserves[0] = 60;
        Fingerprint { slots, reserves }
    }

    fn sidecar() -> Sidecar {
        Sidecar {
            fingerprint: fingerprint(),
            carried: vec![
                Record {
                    tag_hash: 0x5555_6666,
                    magazine: 12,
                    socket: Socket::Shoulder(Hand::Right),
                    single: false,
                },
                Record {
                    tag_hash: 0xFFFF_FFFF,
                    magazine: -1,
                    socket: Socket::Hip(Hand::Left),
                    single: true,
                },
            ],
            slot_pistol_single: Some(true),
        }
    }

    #[test]
    fn a_sidecar_round_trips_through_its_file() {
        let original = sidecar();
        let text = original.encode();
        assert!(text.contains("version = 1"));
        assert_eq!(Sidecar::decode(&text), Ok(original));
        let empty = Sidecar {
            fingerprint: fingerprint(),
            carried: Vec::new(),
            slot_pistol_single: None,
        };
        assert_eq!(Sidecar::decode(&empty.encode()), Ok(empty));
    }

    #[test]
    fn a_file_from_before_separate_pistols_says_nothing_of_the_slot_s_pistol() {
        let text = sidecar().encode();
        assert!(text.contains("slot_pistol_single = true"));
        let older = text.replace("slot_pistol_single = true\n", "");
        assert_eq!(Sidecar::decode(&older).unwrap().slot_pistol_single, None);
    }

    #[test]
    fn a_file_without_carried_weapons_reads_as_none() {
        let text = sidecar().encode();
        let without: String = text.split("[[carried]]").next().unwrap().to_owned();
        assert_eq!(Sidecar::decode(&without).unwrap().carried, Vec::new());
    }

    #[test]
    fn bad_files_are_errors_not_panics() {
        assert!(matches!(
            Sidecar::decode("this is not toml ["),
            Err(SidecarError::Malformed(_))
        ));
        assert!(matches!(
            Sidecar::decode(""),
            Err(SidecarError::Malformed(_))
        ));
        let text = sidecar().encode();
        // Truncated anywhere, the file reads or fails cleanly.
        for end in 0..text.len() {
            if text.is_char_boundary(end) {
                let _ = Sidecar::decode(&text[..end]);
            }
        }
        let nine_slots = "version = 1\n[fingerprint]\nslots = [0, 0, 0, 0, 0, 0, 0, 0, 0]\nreserves = \
                          [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]\n";
        assert!(matches!(
            Sidecar::decode(nine_slots),
            Err(SidecarError::Malformed(_))
        ));
        let eight_slots = nine_slots.replacen("0, 0]", "0]", 1);
        assert!(Sidecar::decode(&eight_slots).is_ok(), "{eight_slots}");
        assert_eq!(
            Sidecar::decode(&text.replacen("version = 1", "version = 2", 1)),
            Err(SidecarError::Version(2))
        );
        assert_eq!(
            Sidecar::decode(&text.replacen("back_first", "shoulder", 1)),
            Err(SidecarError::UnknownSocket("shoulder".to_owned()))
        );
        assert!(matches!(
            Sidecar::decode(&text.replacen("magazine = 12", "magazine = \"twelve\"", 1)),
            Err(SidecarError::Malformed(_))
        ));
    }

    #[test]
    fn the_fingerprint_accepts_its_own_save_only() {
        let saved = fingerprint();
        assert_eq!(saved.matches(&saved, |_| true), Ok(()));
        let mut other_slot = saved;
        other_slot.slots[1] = 0x7777_8888;
        assert_eq!(
            saved.matches(&other_slot, |_| true),
            Err(Mismatch::Slot {
                index: 1,
                saved: 0x1111_2222,
                loaded: 0x7777_8888
            })
        );
        let mut other_reserve = saved;
        other_reserve.reserves[3] = 239;
        assert!(matches!(
            saved.matches(&other_reserve, |_| true),
            Err(Mismatch::Reserve { index: 3, .. })
        ));
    }

    #[test]
    fn a_slot_of_content_not_installed_loads_empty_and_still_matches() {
        let saved = fingerprint();
        let mut loaded = saved;
        loaded.slots[6] = 0;
        assert_eq!(saved.matches(&loaded, |hash| hash != 0x3333_4444), Ok(()));
        assert!(
            saved.matches(&loaded, |_| true).is_err(),
            "a known type that loads empty is another save"
        );
    }

    #[test]
    fn a_load_restores_only_the_types_the_game_knows() {
        let (restored, skipped) = sidecar().restorable(|hash| hash != 0xFFFF_FFFF);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].socket, Socket::Shoulder(Hand::Right));
        assert_eq!(skipped, 1);
    }
}
