//! The spawner: queue a spawn request for any tag set the spawn system knows, placed relative to
//! the view.
//!
//! Requests go through `CSpawnSystem::Spawn` with the high-priority flags, which skip the
//! line-of-sight, proximity, and spawn-block checks; those exist to keep spawns out of the player's
//! view, and putting something in view is the point. The spawn is asynchronous: the object appears
//! once its entity streams in. The request state records submission, not entity completion.
//!
//! The picker's definitions come from the game's own spawn definition files, extracted once into
//! `spawn_defs.json` (see `tools/spawn_defs/extract_spawn_defs.py`) and embedded, so what it
//! offers is exactly what the spawn system can match: a definition's full tag set selects it
//! outright, where a single broad tag (`vehicle`) picks a weighted random member of the pool.

use std::sync::OnceLock;

use jc3gi::{spawn_system::SpawnSystem, types::math::Matrix4};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// What the spawner spawns and where.
#[derive(Clone, Serialize, Deserialize)]
pub struct SpawnConfig {
    /// The space-separated tag string the spawn system matches definitions against.
    pub tags: String,
    /// How far along the view direction to place the object, in metres.
    pub distance: f32,
    /// Turn the spawned object to face the player rather than share the view direction.
    pub face_player: bool,
}
impl SpawnConfig {
    pub const fn new() -> Self {
        Self {
            tags: String::new(),
            distance: 8.0,
            face_player: false,
        }
    }
}

/// A named tag set for the debug tab's preset buttons.
pub struct SpawnPreset {
    pub label: &'static str,
    pub tags: &'static str,
}

/// Quick presets: the full tag set of one shipped definition each, so the spawn system's
/// tag match lands on that definition (or an identically tagged sibling) rather than on a random
/// member of a broad pool. Taken from [`definitions`].
pub const PRESETS: &[SpawnPreset] = &[
    SpawnPreset {
        label: "Exploding barrel",
        tags: "prop,barrel,explosive,timed",
    },
    SpawnPreset {
        label: "Vintage sports car",
        tags: "vehicle,ground,car,civilian,vintage,mugello,sports,unarmed,unarmored,ambient,liberated,highend",
    },
    SpawnPreset {
        label: "Sport bike",
        tags: "vehicle,ground,bike,civilian,modern,sport,mugello,unarmed,unarmored,ambient,liberated",
    },
    SpawnPreset {
        label: "Rebel patrol boat",
        tags: "vehicle,water,boat,heavy,patrol,rebel,capstone,mounted,machinegun,armored,r02_outpost,r03_outpost",
    },
    SpawnPreset {
        label: "Rebel attack helicopter",
        tags: "vehicle,air,helicopter,rebel,light,assault,machinegun,unarmored,urga,heat3,heat4,r01_outpost,r02_outpost,r03_outpost",
    },
    SpawnPreset {
        label: "Rebel fighter jet",
        tags: "vehicle,air,plane,jet,capstone,rebel,fighter,machinegun,missiles,armored",
    },
    SpawnPreset {
        label: "Rebel tank",
        tags: "vehicle,ground,tank,rebel,armored,capstone,cannon,machinegun,not_shielded,r03_outpost",
    },
    SpawnPreset {
        label: "Enemy soldier",
        tags: "character,military,enemy,machinegunner,heat4,heat5",
    },
];

/// One shipped spawn definition, from the game's `settings/spawn_*_defs.bin` files as extracted
/// by `tools/spawn_defs/extract_spawn_defs.py`.
#[derive(Clone, Deserialize)]
pub struct SpawnDefinition {
    /// The definition's display name (or its entity stem when it has none).
    pub name: String,
    /// The definition's debug-menu category (`vehicle|ground|civilian`), possibly empty.
    pub category: String,
    /// The comma-separated tag set the spawn system matches against.
    pub tags: String,
    /// The budget pool the definition file belongs to (`vehicle`, `aircraft`, `combatant`, ...).
    pub pool: String,
}

/// Every shipped definition the picker offers, parsed once from the embedded table.
pub fn definitions() -> &'static [SpawnDefinition] {
    static TABLE: OnceLock<Vec<SpawnDefinition>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(SPAWN_DEFS_JSON).unwrap_or_else(|e| {
            tracing::error!("director: the embedded spawn definition table does not parse: {e}");
            Vec::new()
        })
    })
}

/// The distinct pools in [`definitions`], in table order.
pub fn pools() -> Vec<&'static str> {
    let mut pools: Vec<&str> = Vec::new();
    for def in definitions() {
        if !pools.contains(&def.pool.as_str()) {
            pools.push(&def.pool);
        }
    }
    pools
}

/// The shipped definition table; regenerate with `tools/spawn_defs/extract_spawn_defs.py`.
const SPAWN_DEFS_JSON: &str = include_str!("spawn_defs.json");

/// The last request-processing result; submission does not confirm an object has spawned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Submission {
    NoTags,
    TagsContainNul,
    NoSpawnSystem,
    Submitted,
}

/// Owned request state, independent of asynchronous entity streaming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Status {
    pub pending: bool,
    pub last_submission: Option<Submission>,
}

/// Copy the request state without servicing it or querying the engine.
pub(crate) fn status() -> Status {
    with_state(|state| *state)
}

/// Queue a spawn of the config's tags; serviced on the next main-thread update.
pub fn request() {
    with_state(|state| state.pending = true);
}

/// Run the queued spawn, if any. Main thread: the spawn system is game-thread data.
pub(super) fn service_pending(cfg: &SpawnConfig) {
    if !with_state(|state| std::mem::take(&mut state.pending)) {
        return;
    }
    let tags = cfg.tags.trim();
    if tags.is_empty() {
        with_state(|state| state.last_submission = Some(Submission::NoTags));
        tracing::warn!("director: spawn dropped: no tags");
        return;
    }
    let Ok(tags_c) = std::ffi::CString::new(tags) else {
        with_state(|state| state.last_submission = Some(Submission::TagsContainNul));
        tracing::warn!("director: spawn dropped: tags contain a NUL");
        return;
    };
    // SAFETY: the spawn system singleton is main-thread data and this runs from the main-thread
    // update; the tag string outlives the call.
    unsafe {
        let Some(spawn_system) = SpawnSystem::get() else {
            with_state(|state| state.last_submission = Some(Submission::NoSpawnSystem));
            tracing::warn!("director: spawn dropped: no spawn system");
            return;
        };
        let pos = spawn_system.m_ViewPos.data;
        let dir = spawn_system.m_ViewDir.data;
        let transform = placement(pos, dir, cfg.distance, cfg.face_player);
        tracing::info!(
            "director: spawn \"{tags}\" at ({:.1}, {:.1}, {:.1})",
            transform.data[12],
            transform.data[13],
            transform.data[14]
        );
        spawn_system.Spawn(
            tags_c.as_ptr().cast(),
            &transform,
            SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS,
            std::ptr::null(),
            std::ptr::null_mut(),
        );
    }
    with_state(|state| state.last_submission = Some(Submission::Submitted));
}

/// The spawn transform: `distance` along the view direction from the view position, with the
/// object's forward along the ground-plane view direction (or back toward the viewer with
/// `face_player`) and its up along world up. The row-major matrix's rows are right, up, forward,
/// and translation; the game's forward is `+Z` in object space and up is `+Y`.
pub fn placement(
    view_pos: [f32; 3],
    view_dir: [f32; 3],
    distance: f32,
    face_player: bool,
) -> Matrix4 {
    let position = [
        view_pos[0] + view_dir[0] * distance,
        view_pos[1] + view_dir[1] * distance,
        view_pos[2] + view_dir[2] * distance,
    ];
    let flat = glam::Vec3::new(view_dir[0], 0.0, view_dir[2]);
    let forward = if flat.length_squared() > 1e-6 {
        flat.normalize()
    } else {
        glam::Vec3::Z
    };
    let forward = if face_player { -forward } else { forward };
    let up = glam::Vec3::Y;
    let right = up.cross(forward).normalize();
    let mut m = Matrix4::IDENTITY;
    m.data[0..3].copy_from_slice(&right.to_array());
    m.data[4..7].copy_from_slice(&up.to_array());
    m.data[8..11].copy_from_slice(&forward.to_array());
    m.data[12..15].copy_from_slice(&position);
    m
}

fn with_state<R>(operation: impl FnOnce(&mut Status) -> R) -> R {
    #[cfg(test)]
    if test_status::is_installed() {
        return test_status::with_state(operation);
    }
    operation(&mut STATE.lock())
}

static STATE: Mutex<Status> = Mutex::new(Status {
    pending: false,
    last_submission: None,
});

#[cfg(test)]
pub(crate) mod test_status {
    use std::cell::RefCell;

    use crate::director::spawn::Status;

    pub(crate) struct Fixture {
        previous: Option<Status>,
    }

    pub(crate) fn install(status: Status) -> Fixture {
        Fixture {
            previous: STATE.replace(Some(status)),
        }
    }

    pub(crate) fn snapshot() -> Option<Status> {
        STATE.with_borrow(|state| *state)
    }

    pub(crate) fn is_installed() -> bool {
        STATE.with_borrow(Option::is_some)
    }

    pub(crate) fn with_state<R>(operation: impl FnOnce(&mut Status) -> R) -> R {
        STATE.with_borrow_mut(|state| operation(state.as_mut().unwrap()))
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            STATE.replace(self.previous.take());
        }
    }

    thread_local! {
        static STATE: RefCell<Option<Status>> = const { RefCell::new(None) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_peeks_preserve_queued_and_submitted_requests() {
        assert_eq!(test_status::snapshot(), None);
        let initial = Status {
            pending: false,
            last_submission: Some(Submission::Submitted),
        };
        {
            let _fixture = test_status::install(initial);
            assert_eq!(status(), initial);
            request();
            let queued = Status {
                pending: true,
                ..initial
            };
            assert_eq!(status(), queued);
            assert_eq!(status(), queued);
            assert_eq!(test_status::snapshot(), Some(queued));
            {
                let nested = Status {
                    pending: false,
                    last_submission: Some(Submission::NoSpawnSystem),
                };
                let _nested = test_status::install(nested);
                assert_eq!(status(), nested);
            }
            assert_eq!(status(), queued);
        }
        assert_eq!(test_status::snapshot(), None);
    }

    #[test]
    fn invalid_requests_record_rejections_without_native_submission() {
        let _fixture = test_status::install(Status {
            pending: false,
            last_submission: Some(Submission::Submitted),
        });
        for (tags, expected) in [
            (" \t ", Submission::NoTags),
            ("vehicle\0ground", Submission::TagsContainNul),
        ] {
            request();
            let cfg = SpawnConfig {
                tags: tags.to_owned(),
                ..SpawnConfig::new()
            };
            service_pending(&cfg);
            let processed = Status {
                pending: false,
                last_submission: Some(expected),
            };
            assert_eq!(status(), processed);
            service_pending(&cfg);
            assert_eq!(status(), processed);
        }
    }

    #[test]
    fn the_embedded_definition_table_parses_and_has_the_barrel() {
        let defs = definitions();
        assert!(defs.len() > 400, "{} definitions", defs.len());
        assert!(defs.iter().any(|d| d.tags == "prop,barrel,explosive,timed"));
        for preset in PRESETS {
            assert!(
                defs.iter().any(|d| d.tags == preset.tags),
                "preset {} has no matching definition",
                preset.label
            );
        }
    }

    #[test]
    fn placement_lands_along_the_view_direction() {
        let m = placement([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], 5.0, false);
        assert_eq!(&m.data[12..15], &[1.0, 2.0, 8.0]);
        assert_eq!(&m.data[8..11], &[0.0, 0.0, 1.0]);
        assert_eq!(&m.data[4..7], &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn placement_flattens_a_pitched_view_and_can_face_back() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let m = placement([0.0, 0.0, 0.0], [0.0, -half, half], 2.0, true);
        assert!((m.data[13] - (-std::f32::consts::SQRT_2)).abs() < 1e-3);
        assert!((m.data[8]).abs() < 1e-6);
        assert!((m.data[9]).abs() < 1e-6);
        assert!((m.data[10] - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn placement_survives_a_vertical_view() {
        let m = placement([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, false);
        assert_eq!(&m.data[8..11], &[0.0, 0.0, 1.0]);
        assert_eq!(&m.data[0..3], &[1.0, 0.0, 0.0]);
    }
}
