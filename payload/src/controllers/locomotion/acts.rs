//! The act filter: keeps the turning animations off the local player while the stick drives the
//! body on foot. Called from the `Character::QueueAct` detour (`hooks::character::acts`).
//!
//! - The turn-in-place acts (the rotate-on-spot acts, their warp flavors, the continuous rotation
//!   starts, and the quarter and half turns) are dropped. The body still turns: the orientation
//!   executor yaws it toward the torso's heading, which is a rotation of the root and not an
//!   animation that steps the feet around.
//! - The turning run and walk starts are replaced by the straight start, and the turning stops by
//!   the straight stop, so the state machine still passes through its start and stop states (it
//!   needs to know Rico is moving) without the clip swinging the body. The replacement goes through
//!   the game's own pre-flight (`AnimatedModel::TryAct`) first, and the original act is queued when
//!   the state machine would not take the straight one.
//!
//! A filter that removes acts can leave the animation state machine waiting for one it never
//! receives. The [`Watchdog`] watches for that: the filter normally acts in short bursts (a start,
//! a stop), so when it has acted without a break for [`ActFilterConfig::watchdog_s`] the watchdog
//! lets every act through for [`ActFilterConfig::release_s`] and logs a warning.

use std::{
    ffi::CStr,
    sync::OnceLock,
    time::{Duration, Instant},
};

use jc3gi::{animation::symbol_table::EventIdSymbolTable, character::character::Character};
use parking_lot::Mutex;

use crate::{
    controllers::{
        config::LocomotionConfig,
        locomotion::config::ActFilterConfig,
        mode::{Mode, OnFootVariant, current as current_mode},
    },
    headpose::{Source, source},
    hooks::{character::resolve_act_id, in_gameplay},
};

/// What the filter does with an act.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActDecision {
    /// Queue the act as the game asked.
    Pass,
    /// Drop the act.
    Drop,
    /// Queue this act id instead.
    Replace(i32),
}

/// The filter's counters, for the debug UI and the log.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ActFilterStatus {
    pub dropped: u64,
    pub replaced: u64,
    /// Replacements the state machine refused, which queued the original act.
    pub refused: u64,
    /// How often the watchdog released the filter.
    pub releases: u64,
    /// Whether the watchdog is letting every act through right now.
    pub released: bool,
}

/// The filter's counters.
pub fn status() -> ActFilterStatus {
    STATE.lock().status
}

/// Decide what to do with the local player's act `id`. `locomotion` is the locomotion settings and
/// `controllers_enabled` the controllers' master switch, read by the caller with the config lock it
/// already takes.
///
/// # Safety
///
/// `character` must be the live local character, on the thread queuing its act.
pub(crate) unsafe fn filter(
    character: &mut Character,
    id: i32,
    locomotion: &LocomotionConfig,
    controllers_enabled: bool,
) -> ActDecision {
    let config = &locomotion.acts;
    if !config.enabled || !locomotion.stick.enabled || !applies(controllers_enabled) {
        return ActDecision::Pass;
    }
    let Some(table) = table() else {
        return ActDecision::Pass;
    };
    let wanted = table.decide(id);
    if wanted == ActDecision::Pass {
        return ActDecision::Pass;
    }
    let now = Instant::now();
    let mut state = STATE.lock();
    if !state.watchdog.admit(now) {
        state.status.released = true;
        return ActDecision::Pass;
    }
    state.status.released = false;
    if let Some(release) = state.watchdog.record(now, config) {
        state.status.releases += 1;
        state.status.released = true;
        tracing::warn!(
            target: "controllers",
            acted_for_s = release.acted_for.as_secs_f32(),
            release_s = config.release_s,
            "locomotion: the act filter acted without a break; letting every act through",
        );
        return ActDecision::Pass;
    }
    match wanted {
        ActDecision::Replace(replacement) => {
            let act = replacement as u32;
            // SAFETY: the act pre-flight only reads the animation state machine, on the thread
            // that is queuing the act.
            if unsafe { character.m_AnimatedModel.TryAct(&act) } {
                state.status.replaced += 1;
                wanted
            } else {
                state.status.refused += 1;
                ActDecision::Pass
            }
        }
        ActDecision::Drop => {
            state.status.dropped += 1;
            wanted
        }
        ActDecision::Pass => ActDecision::Pass,
    }
}

/// Times the filter's bursts of activity and releases it when one runs too long.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Watchdog {
    /// When the current burst began.
    burst_start: Option<Instant>,
    /// When the filter last acted.
    last: Option<Instant>,
    /// When the current release ends.
    released_until: Option<Instant>,
}

/// A release the watchdog decided on.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Release {
    /// How long the burst that tripped it lasted.
    pub acted_for: Duration,
}

impl Watchdog {
    pub const fn new() -> Self {
        Self {
            burst_start: None,
            last: None,
            released_until: None,
        }
    }

    /// Whether the filter may act at `now`, or is released.
    pub fn admit(&mut self, now: Instant) -> bool {
        match self.released_until {
            Some(until) if now < until => false,
            Some(_) => {
                self.released_until = None;
                self.burst_start = None;
                self.last = None;
                true
            }
            None => true,
        }
    }

    /// Record that the filter is about to act at `now`, and decide whether this is a burst that has
    /// gone on too long.
    pub fn record(&mut self, now: Instant, config: &ActFilterConfig) -> Option<Release> {
        let continues = self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) <= BURST_GAP);
        if !continues {
            self.burst_start = Some(now);
        }
        self.last = Some(now);
        let start = self.burst_start.unwrap_or(now);
        let acted_for = now.saturating_duration_since(start);
        if acted_for.as_secs_f32() >= config.watchdog_s {
            self.released_until =
                Some(now + Duration::from_secs_f32(config.release_s.clamp(0.0, 60.0)));
            self.burst_start = None;
            self.last = None;
            Some(Release { acted_for })
        } else {
            None
        }
    }
}

/// The act ids the filter acts on, resolved once the event-id symbol table is live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActTable {
    drop: Vec<i32>,
    replace: Vec<(i32, i32)>,
}

impl ActTable {
    /// Resolve the table's names through `resolve`, or `None` when any of them does not resolve.
    pub fn resolve(mut resolve: impl FnMut(&CStr) -> Option<i32>) -> Option<Self> {
        let drop = DROPPED
            .iter()
            .map(|name| resolve(name))
            .collect::<Option<Vec<_>>>()?;
        let replace = REPLACED
            .iter()
            .map(|(from, to)| Some((resolve(from)?, resolve(to)?)))
            .collect::<Option<Vec<_>>>()?;
        Some(Self { drop, replace })
    }

    /// What the filter would do with `id`, before the watchdog and the pre-flight.
    pub fn decide(&self, id: i32) -> ActDecision {
        if self.drop.contains(&id) {
            return ActDecision::Drop;
        }
        self.replace
            .iter()
            .find(|(from, _)| *from == id)
            .map_or(ActDecision::Pass, |&(_, to)| ActDecision::Replace(to))
    }
}

/// The turn-in-place acts.
const DROPPED: [&CStr; 16] = [
    EventIdSymbolTable::ACT_ROTATE_45L,
    EventIdSymbolTable::ACT_ROTATE_45R,
    EventIdSymbolTable::ACT_ROTATE_90L,
    EventIdSymbolTable::ACT_ROTATE_90R,
    EventIdSymbolTable::ACT_ROTATE_180L,
    EventIdSymbolTable::ACT_ROTATE_180R,
    EventIdSymbolTable::ACT_ROTATE_90L_WARP,
    EventIdSymbolTable::ACT_ROTATE_90R_WARP,
    EventIdSymbolTable::ACT_ROTATE_180L_WARP,
    EventIdSymbolTable::ACT_ROTATE_180R_WARP,
    EventIdSymbolTable::ACT_ROTATE_LEFT_START,
    EventIdSymbolTable::ACT_ROTATE_RIGHT_START,
    EventIdSymbolTable::ACT_TURN_90L,
    EventIdSymbolTable::ACT_TURN_90R,
    EventIdSymbolTable::ACT_TURN_180L,
    EventIdSymbolTable::ACT_TURN_180R,
];

/// The turning starts and stops, each with its straight variant.
const REPLACED: [(&CStr, &CStr); 22] = [
    (
        EventIdSymbolTable::ACT_IDLE_TO_START_90R,
        EventIdSymbolTable::ACT_IDLE_TO_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_START_90L,
        EventIdSymbolTable::ACT_IDLE_TO_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_START_120R,
        EventIdSymbolTable::ACT_IDLE_TO_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_START_120L,
        EventIdSymbolTable::ACT_IDLE_TO_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_START_180R,
        EventIdSymbolTable::ACT_IDLE_TO_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_START_180L,
        EventIdSymbolTable::ACT_IDLE_TO_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_90R,
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_90L,
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_120R,
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_120L,
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_180R,
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0,
    ),
    (
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_180L,
        EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_90R,
        EventIdSymbolTable::ACT_STOP_TURN_0,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_90L,
        EventIdSymbolTable::ACT_STOP_TURN_0,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_120R,
        EventIdSymbolTable::ACT_STOP_TURN_0,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_120L,
        EventIdSymbolTable::ACT_STOP_TURN_0,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_180R,
        EventIdSymbolTable::ACT_STOP_TURN_0,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_180L,
        EventIdSymbolTable::ACT_STOP_TURN_0,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_90R_WARP,
        EventIdSymbolTable::ACT_STOP_TURN_0_WARP,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_90L_WARP,
        EventIdSymbolTable::ACT_STOP_TURN_0_WARP,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_180R_WARP,
        EventIdSymbolTable::ACT_STOP_TURN_0_WARP,
    ),
    (
        EventIdSymbolTable::ACT_STOP_TURN_180L_WARP,
        EventIdSymbolTable::ACT_STOP_TURN_0_WARP,
    ),
];

/// Acts closer together than this belong to one burst.
const BURST_GAP: Duration = Duration::from_millis(400);

/// Whether the filter applies: the headset drives the view, gameplay owns the character, and the
/// player is on foot on the ground.
fn applies(controllers_enabled: bool) -> bool {
    let detection = current_mode();
    source() == Source::Vr
        && in_gameplay()
        && controllers_enabled
        && detection.mode == Mode::OnFoot(OnFootVariant::Ground)
        && detection.traversal.is_none()
}

/// The resolved table, once every name in it resolves. The names are the `NCharacter` act ids the
/// game registers at startup, so resolving them never adds a name to the table.
fn table() -> Option<&'static ActTable> {
    if let Some(table) = TABLE.get() {
        return Some(table);
    }
    let table = ActTable::resolve(resolve_act_id)?;
    Some(TABLE.get_or_init(|| table))
}

static TABLE: OnceLock<ActTable> = OnceLock::new();

struct State {
    watchdog: Watchdog,
    status: ActFilterStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    watchdog: Watchdog::new(),
    status: ActFilterStatus {
        dropped: 0,
        replaced: 0,
        refused: 0,
        releases: 0,
        released: false,
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ActFilterConfig {
        ActFilterConfig::new()
    }

    #[test]
    fn short_bursts_never_release() {
        let mut dog = Watchdog::new();
        let start = Instant::now();
        let config = config();
        // A start's worth of filtering every two seconds, with quiet between.
        for burst in 0..10u64 {
            for tick in 0..10u64 {
                let now = start + Duration::from_millis(burst * 2_000 + tick * 33);
                assert!(dog.admit(now));
                assert_eq!(dog.record(now, &config), None);
            }
        }
    }

    #[test]
    fn an_unbroken_burst_releases_then_rearms() {
        let mut dog = Watchdog::new();
        let start = Instant::now();
        let config = config();
        let mut released_at = None;
        for tick in 0..200u64 {
            let now = start + Duration::from_millis(tick * 33);
            if !dog.admit(now) {
                continue;
            }
            if dog.record(now, &config).is_some() {
                released_at = Some(now);
                break;
            }
        }
        let released_at = released_at.expect("the watchdog never released");
        let acted = released_at.duration_since(start).as_secs_f32();
        assert!(acted >= config.watchdog_s && acted < config.watchdog_s + 0.1);
        let during = released_at + Duration::from_secs_f32(config.release_s / 2.0);
        assert!(!dog.admit(during));
        let after = released_at + Duration::from_secs_f32(config.release_s + 0.01);
        assert!(dog.admit(after));
        assert_eq!(dog.record(after, &config), None);
    }

    #[test]
    fn the_table_maps_turns_and_straightens_starts() {
        let names: Vec<&CStr> = DROPPED
            .iter()
            .copied()
            .chain(REPLACED.iter().flat_map(|(a, b)| [*a, *b]))
            .collect();
        let id_of = |name: &CStr| {
            names
                .iter()
                .position(|n| *n == name)
                .map(|i| i as i32 + 100)
        };
        let table = ActTable::resolve(id_of).unwrap();
        let rotate = id_of(EventIdSymbolTable::ACT_ROTATE_90L).unwrap();
        assert_eq!(table.decide(rotate), ActDecision::Drop);
        let start = id_of(EventIdSymbolTable::ACT_IDLE_TO_START_180L).unwrap();
        let straight = id_of(EventIdSymbolTable::ACT_IDLE_TO_START_0).unwrap();
        assert_eq!(table.decide(start), ActDecision::Replace(straight));
        assert_eq!(table.decide(straight), ActDecision::Pass);
        assert_eq!(table.decide(-1), ActDecision::Pass);
    }

    #[test]
    fn an_unresolved_name_leaves_no_table() {
        assert_eq!(ActTable::resolve(|_| None), None);
    }
}
