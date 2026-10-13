//! The act filter: keeps the turning animations off the local player while the stick drives the
//! body on foot, and while the player stands in a vehicle's stunt area. Called from the
//! `Character::QueueAct` detour (`hooks::character::acts`).
//!
//! - The turn-in-place acts (the rotate-on-spot acts, their warp flavors, the continuous rotation
//!   starts, the quarter and half turns, and the heavy weapon's aim-focus rotations) are dropped.
//!   The body still turns: the orientation executor yaws it toward the torso's heading, which is a
//!   rotation of the root and not an animation that steps the feet around.
//! - The turning run and walk starts are replaced by the straight start, the turning stops by the
//!   straight stop, and the turning flicks (running and aiming) by the straight flick, so the
//!   state machine still passes through its start, stop, and flick states (it needs to know Rico
//!   is moving) without the clip swinging the body. The replacement goes through the game's own
//!   pre-flight (`AnimatedModel::TryAct`) first, and the original act is queued when the state
//!   machine would not take the straight one.
//! - The stunt turns, which turn the body toward the stick while it stands on a vehicle's roof,
//!   are replaced by the return to the idle (`ACT_TO_IDLE`), which is what the game queues in their
//!   place when the stick points nearly straight ahead, and dropped when the state machine would
//!   not take it. The game queues them only on its input tasks' stunt branch, while the character
//!   is attached to a vehicle, where the mode detection may read a vehicle rather than on foot, so
//!   they have their own scope ([`ActScope::StuntArea`]): it needs neither the on-foot mode nor
//!   the stick locomotion, which does not run there. The stunt area's view rides the body, so a
//!   stunt turn would turn the view.
//!
//! A filter that removes acts can leave the animation state machine waiting for one it never
//! receives. The [`Watchdog`] watches for that on foot (the stunt area's acts are queued every tick
//! the stick is held toward a side with no stunt area, and their replacement is the act the game
//! itself queues at the same point when no turn is needed, so the watchdog never releases them): the filter normally acts in short bursts (a start,
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

/// Where an act the filter knows is filtered.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActScope {
    /// On foot on the ground, while the stick drives the body.
    OnFoot,
    /// Standing in a vehicle's stunt area.
    StuntArea,
}

impl ActScope {
    /// Whether the [`Watchdog`] times this scope's acts. The stunt area's replacement is the act
    /// the game itself queues when no turn is needed, so it never leaves the state machine waiting,
    /// and a stick held toward a side with no stunt area must never let a stunt turn through.
    pub const fn watched(self) -> bool {
        match self {
            ActScope::OnFoot => true,
            ActScope::StuntArea => false,
        }
    }

    /// What a replacement the state machine refused becomes. On foot the original act is queued,
    /// so the state machine still moves on; a stunt turn is dropped, which leaves the state as the
    /// game would have left it with no turn to make.
    pub const fn on_refusal(self) -> ActDecision {
        match self {
            ActScope::OnFoot => ActDecision::Pass,
            ActScope::StuntArea => ActDecision::Drop,
        }
    }
}

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
    /// Replacements the state machine refused: on foot the original act was queued, and in the
    /// stunt area the act was dropped ([`ActScope::on_refusal`]).
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
    if !config.enabled {
        return ActDecision::Pass;
    }
    let Some(table) = table() else {
        return ActDecision::Pass;
    };
    let Some((wanted, scope)) = table.decide(id) else {
        return ActDecision::Pass;
    };
    let applies = match scope {
        ActScope::OnFoot => locomotion.stick.enabled && applies_on_foot(controllers_enabled),
        // SAFETY: the caller's contract: the local character, on the thread queuing its act.
        ActScope::StuntArea => unsafe { applies_in_stunt_area(character, controllers_enabled) },
    };
    if !applies {
        return ActDecision::Pass;
    }
    let mut state = STATE.lock();
    if scope.watched() {
        let now = Instant::now();
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
                scope.on_refusal()
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
    stunt_replace: Vec<(i32, i32)>,
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
        let idle = resolve(EventIdSymbolTable::ACT_TO_IDLE)?;
        let stunt_replace = STUNT_TURNS
            .iter()
            .map(|name| Some((resolve(name)?, idle)))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            drop,
            replace,
            stunt_replace,
        })
    }

    /// What the filter would do with `id`, and where, before the watchdog and the pre-flight, or
    /// `None` for an act it lets through everywhere.
    pub fn decide(&self, id: i32) -> Option<(ActDecision, ActScope)> {
        if self.drop.contains(&id) {
            return Some((ActDecision::Drop, ActScope::OnFoot));
        }
        if let Some(&(_, to)) = self.stunt_replace.iter().find(|(from, _)| *from == id) {
            return Some((ActDecision::Replace(to), ActScope::StuntArea));
        }
        self.replace
            .iter()
            .find(|(from, _)| *from == id)
            .map(|&(_, to)| (ActDecision::Replace(to), ActScope::OnFoot))
    }
}

/// The turn-in-place acts.
const DROPPED: [&CStr; 18] = [
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
    EventIdSymbolTable::ACT_AIM_FOCUS_ROTATE_90L,
    EventIdSymbolTable::ACT_AIM_FOCUS_ROTATE_90R,
];

/// The turning starts, stops, and flicks, each with its straight variant.
const REPLACED: [(&CStr, &CStr); 27] = [
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
    (
        EventIdSymbolTable::ACT_FLICK_120R,
        EventIdSymbolTable::ACT_FLICK_0,
    ),
    (
        EventIdSymbolTable::ACT_FLICK_120L,
        EventIdSymbolTable::ACT_FLICK_0,
    ),
    (
        EventIdSymbolTable::ACT_FLICK_90R_AIM,
        EventIdSymbolTable::ACT_FLICK_0_AIM,
    ),
    (
        EventIdSymbolTable::ACT_FLICK_90L_AIM,
        EventIdSymbolTable::ACT_FLICK_0_AIM,
    ),
    (
        EventIdSymbolTable::ACT_FLICK_180_AIM,
        EventIdSymbolTable::ACT_FLICK_0_AIM,
    ),
];

/// The stunt turns, each replaced by `ACT_TO_IDLE`. The straight one turns the body too, by up to
/// a sixth of a turn through its angle correction, so all three are replaced.
const STUNT_TURNS: [&CStr; 3] = [
    EventIdSymbolTable::ACT_STUNT_TURN_0,
    EventIdSymbolTable::ACT_STUNT_TURN_120R,
    EventIdSymbolTable::ACT_STUNT_TURN_120L,
];

/// Acts closer together than this belong to one burst.
const BURST_GAP: Duration = Duration::from_millis(400);

/// Whether the on-foot scope applies: the headset drives the view, gameplay owns the character,
/// and the player is on foot on the ground.
fn applies_on_foot(controllers_enabled: bool) -> bool {
    let detection = current_mode();
    source() == Source::Vr
        && in_gameplay()
        && controllers_enabled
        && detection.mode == Mode::OnFoot(OnFootVariant::Ground)
        && detection.traversal.is_none()
}

/// Whether the stunt area's scope applies: the headset drives the view, gameplay owns the
/// character, and the game applies its stunt modification to it
/// ([`Character::ShouldApplyStuntModification`]), as its input tasks test just before they queue a
/// stunt turn.
///
/// # Safety
///
/// `character` must be the live local character, on the thread queuing its act.
unsafe fn applies_in_stunt_area(character: &Character, controllers_enabled: bool) -> bool {
    source() == Source::Vr
        && in_gameplay()
        && controllers_enabled
        // SAFETY: the caller's contract; the game's own input task calls it on this thread for this
        // character just before it queues the act.
        && unsafe { character.ShouldApplyStuntModification() }
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
    use std::collections::BTreeSet;

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

    /// Every name in the tables, resolved to a distinct id.
    fn test_table() -> (ActTable, impl Fn(&CStr) -> i32) {
        let names: Vec<&CStr> = DROPPED
            .iter()
            .copied()
            .chain(REPLACED.iter().flat_map(|(a, b)| [*a, *b]))
            .chain(STUNT_TURNS.iter().copied())
            .chain([EventIdSymbolTable::ACT_TO_IDLE])
            .collect();
        let id_of = move |name: &CStr| {
            names
                .iter()
                .position(|n| *n == name)
                .map(|i| i as i32 + 100)
        };
        let table = ActTable::resolve(&id_of).unwrap();
        (table, move |name| id_of(name).unwrap())
    }

    #[test]
    fn the_table_maps_turns_and_straightens_starts() {
        let (table, id) = test_table();
        assert_eq!(
            table.decide(id(EventIdSymbolTable::ACT_ROTATE_90L)),
            Some((ActDecision::Drop, ActScope::OnFoot))
        );
        let straight = id(EventIdSymbolTable::ACT_IDLE_TO_START_0);
        assert_eq!(
            table.decide(id(EventIdSymbolTable::ACT_IDLE_TO_START_180L)),
            Some((ActDecision::Replace(straight), ActScope::OnFoot))
        );
        assert_eq!(table.decide(straight), None);
        assert_eq!(table.decide(-1), None);
    }

    #[test]
    fn the_aim_focus_rotations_are_dropped_on_foot() {
        let (table, id) = test_table();
        for name in [
            EventIdSymbolTable::ACT_AIM_FOCUS_ROTATE_90L,
            EventIdSymbolTable::ACT_AIM_FOCUS_ROTATE_90R,
        ] {
            assert_eq!(
                table.decide(id(name)),
                Some((ActDecision::Drop, ActScope::OnFoot)),
                "{name:?}"
            );
        }
    }

    #[test]
    fn the_turning_flicks_straighten_and_the_straight_ones_pass() {
        let (table, id) = test_table();
        let running = id(EventIdSymbolTable::ACT_FLICK_0);
        let aiming = id(EventIdSymbolTable::ACT_FLICK_0_AIM);
        for (name, straight) in [
            (EventIdSymbolTable::ACT_FLICK_120R, running),
            (EventIdSymbolTable::ACT_FLICK_120L, running),
            (EventIdSymbolTable::ACT_FLICK_90R_AIM, aiming),
            (EventIdSymbolTable::ACT_FLICK_90L_AIM, aiming),
            (EventIdSymbolTable::ACT_FLICK_180_AIM, aiming),
        ] {
            assert_eq!(
                table.decide(id(name)),
                Some((ActDecision::Replace(straight), ActScope::OnFoot)),
                "{name:?}"
            );
        }
        assert_eq!(table.decide(running), None);
        assert_eq!(table.decide(aiming), None);
    }

    #[test]
    fn every_stunt_turn_returns_to_the_idle_in_the_stunt_area() {
        let (table, id) = test_table();
        let idle = id(EventIdSymbolTable::ACT_TO_IDLE);
        for name in STUNT_TURNS {
            assert_eq!(
                table.decide(id(name)),
                Some((ActDecision::Replace(idle), ActScope::StuntArea)),
                "{name:?}"
            );
        }
        assert_eq!(table.decide(idle), None);
    }

    #[test]
    fn a_refused_stunt_turn_is_dropped_and_a_refused_start_passes() {
        assert_eq!(ActScope::StuntArea.on_refusal(), ActDecision::Drop);
        assert_eq!(ActScope::OnFoot.on_refusal(), ActDecision::Pass);
    }

    #[test]
    fn only_the_on_foot_scope_is_watched() {
        assert!(ActScope::OnFoot.watched());
        assert!(!ActScope::StuntArea.watched());
    }

    #[test]
    fn no_act_is_in_two_tables() {
        let mut seen = BTreeSet::new();
        for name in DROPPED
            .iter()
            .chain(REPLACED.iter().map(|(from, _)| from))
            .chain(STUNT_TURNS.iter())
        {
            assert!(seen.insert(*name), "{name:?} is filtered twice");
        }
        for (_, to) in REPLACED {
            assert!(
                !seen.contains(to),
                "{to:?} is both a replacement and filtered"
            );
        }
    }

    #[test]
    fn an_unresolved_name_leaves_no_table() {
        assert_eq!(ActTable::resolve(|_| None), None);
    }
}
