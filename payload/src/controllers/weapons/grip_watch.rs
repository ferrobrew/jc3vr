//! A watch on how each held gun sits in its hand, for diagnosing a gun that turns in the hand.
//!
//! The rigid grip ([`crate::controllers::hands::grip`]) holds a gun in a controller-driven hand at
//! a fixed pose on the solved wrist, and records at each placement how the gun sits on that wrist.
//! On every input tick the watch reads each hand's last record and compares the gun's rotation on
//! the wrist with two references:
//!
//! - the previous tick's: a change past [`FLIP_DEG`] is a snap;
//! - the rotation the gun had when the hand took it (a grab, kept across attach-bone changes): a
//!   departure past [`SWEEP_DEG`] held for [`SWEEP_HOLD`] is a sweep, which catches a turn the
//!   animation blends over several ticks.
//!
//! A move to another attach bone is logged too. Each warning carries what tells the causes apart:
//! the attach bone and the weapon state, whether the bone was pinned, how far the animation had put
//! the attach bone from the canonical grip, the solved wrist's distance and drift from its target,
//! the IK weight, the hand tier, and the character's animation state. With the grip pinned the
//! gun's rotation on the wrist is constant, so a warning means the pin did not act; the fields say
//! why. Warnings are rate-limited per hand.

use std::time::{Duration, Instant};

use glam::Quat;
use jc3gi::{
    character::character::Character,
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        frame::Hand,
        hands::{
            grip::{self, HandGrip},
            tiers,
        },
    },
    hooks::animation,
};

/// The watch's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GripWatchStatus {
    /// The change in each hand's gun-on-wrist rotation over the last tick, degrees, indexed by
    /// [`Hand::index`].
    pub change_deg: [Option<f32>; 2],
    /// Each hand's gun-on-wrist rotation away from its grab, degrees.
    pub from_grab_deg: [Option<f32>; 2],
    /// Snaps and sweeps seen per hand since the payload loaded.
    pub flips: [u64; 2],
    pub sweeps: [u64; 2],
    /// Attach-bone changes seen per hand since the payload loaded.
    pub bone_changes: [u64; 2],
    /// The last flip or sweep: its hand and its size, degrees.
    pub last_flip: Option<(Hand, f32)>,
    /// Each hand's last grip record: whether it was pinned, and how far the animation had put the
    /// attach bone from the canonical grip, degrees.
    pub pinned: [Option<bool>; 2],
    pub animated_deviation_deg: [Option<f32>; 2],
}

pub fn status() -> GripWatchStatus {
    STATE.lock().status
}

/// A tick-to-tick change in the gun's rotation on the wrist larger than this is a snap, degrees.
pub const FLIP_DEG: f32 = 90.0;
/// A departure from the grab's rotation larger than this, held for [`SWEEP_HOLD`], is a sweep,
/// degrees.
pub const SWEEP_DEG: f32 = 60.0;
/// How long a departure past [`SWEEP_DEG`] lasts before it is a sweep.
pub const SWEEP_HOLD: Duration = Duration::from_millis(100);

/// The change between two gun-on-wrist rotations, degrees.
pub fn change_deg(previous: Quat, current: Quat) -> f32 {
    grip::angle_deg(previous, current)
}

/// Measure `weapon`'s hand-held guns from the rigid grip's last records.
///
/// # Safety
///
/// `weapon` must be live, read on the main thread outside the world update.
pub(super) unsafe fn observe(weapon: &WeaponBase) {
    let now = Instant::now();
    let records = grip::status().hands;
    let address = weapon as *const WeaponBase as usize;
    let character = unsafe { weapon.m_User[0].cast::<Character>().as_ref() };
    let animation_state = character.and_then(|c| unsafe { animation::active_state_type_id(c) });
    let mut guard = STATE.lock();
    let state = &mut *guard;
    for hand in Hand::BOTH {
        let record = records[hand.index()]
            .filter(|r| r.weapon == address && now.saturating_duration_since(r.at) <= MAX_AGE);
        let context = Context {
            hand,
            weapon_state: weapon.m_State,
            animation_state,
        };
        let events = state.hands[hand.index()].observe(record, now);
        let status = &mut state.status;
        status.change_deg[hand.index()] = events.change_deg;
        status.from_grab_deg[hand.index()] = events.from_grab_deg;
        status.pinned[hand.index()] = record.map(|r| r.pinned);
        status.animated_deviation_deg[hand.index()] = record.map(|r| r.animated_deviation_deg);
        let Some(record) = record else {
            continue;
        };
        if let Some(from) = events.bone_change {
            status.bone_changes[hand.index()] += 1;
            if state.hands[hand.index()].limiter.allow(now) {
                tracing::info!(
                    target: "controllers",
                    ?hand,
                    from = grip::bone_label(from),
                    to = grip::bone_label(record.bone),
                    "a held gun moved to another attach bone"
                );
                log_context(&context, &record, &events);
            }
        }
        let flip = events.snap.map(|c| (Kind::Snap, c));
        let sweep = events.sweep.map(|c| (Kind::Sweep, c));
        for (kind, size) in flip.into_iter().chain(sweep) {
            let status = &mut state.status;
            match kind {
                Kind::Snap => status.flips[hand.index()] += 1,
                Kind::Sweep => status.sweeps[hand.index()] += 1,
            }
            status.last_flip = Some((hand, size));
            let suppressed = state.hands[hand.index()].limiter.suppressed;
            if state.hands[hand.index()].limiter.allow(now) {
                tracing::warn!(
                    target: "controllers",
                    ?hand,
                    kind = kind.label(),
                    size_deg = size,
                    suppressed,
                    "a held gun turned in its hand"
                );
                log_context(&context, &record, &events);
            }
        }
    }
}

/// Forget the measurements, for when the weapon handling stands down.
pub(super) fn clear() {
    let mut state = STATE.lock();
    state.hands = [HandWatch::new(), HandWatch::new()];
}

/// What the watch saw for one hand on one tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Events {
    change_deg: Option<f32>,
    from_grab_deg: Option<f32>,
    /// The tick-to-tick change, when it was a snap.
    snap: Option<f32>,
    /// The departure from the grab, when it became a sweep this tick.
    sweep: Option<f32>,
    /// The attach bone the gun left, when it moved.
    bone_change: Option<u32>,
    /// The solved wrist's drift from its target since the grab, degrees.
    wrist_drift_deg: Option<f32>,
    /// How long the gun has been past [`SWEEP_DEG`], when it is.
    away_for: Option<Duration>,
}

/// One hand's watch: the grab it compares against and the previous tick.
#[derive(Debug, Copy, Clone)]
struct HandWatch {
    grab: Option<Grab>,
    previous: Option<HandGrip>,
    /// When the gun went past [`SWEEP_DEG`], and whether that excursion was reported.
    away_since: Option<(Instant, bool)>,
    limiter: Limiter,
}

impl HandWatch {
    const fn new() -> Self {
        HandWatch {
            grab: None,
            previous: None,
            away_since: None,
            limiter: Limiter::new(),
        }
    }

    fn observe(&mut self, record: Option<HandGrip>, now: Instant) -> Events {
        let Some(record) = record else {
            self.grab = None;
            self.previous = None;
            self.away_since = None;
            return Events::default();
        };
        let same_gun = |g: &HandGrip| g.weapon == record.weapon && g.gun == record.gun;
        let grab = match self
            .grab
            .filter(|g| g.weapon == record.weapon && g.gun == record.gun)
        {
            Some(grab) => grab,
            None => {
                self.away_since = None;
                Grab {
                    weapon: record.weapon,
                    gun: record.gun,
                    gun_in_wrist: record.gun_in_wrist,
                    wrist_in_target: record.wrist_in_target,
                }
            }
        };
        self.grab = Some(grab);
        let previous = self.previous.filter(same_gun);
        self.previous = Some(record);

        let tick_change = previous.map(|p| change_deg(p.gun_in_wrist, record.gun_in_wrist));
        let from_grab_deg = change_deg(grab.gun_in_wrist, record.gun_in_wrist);
        let bone_change = previous.filter(|p| p.bone != record.bone).map(|p| p.bone);
        let wrist_drift_deg = grab
            .wrist_in_target
            .zip(record.wrist_in_target)
            .map(|(a, b)| change_deg(a, b));

        let mut sweep = None;
        let mut away_for = None;
        if from_grab_deg > SWEEP_DEG {
            let (since, reported) = *self.away_since.get_or_insert((now, false));
            let held = now.saturating_duration_since(since);
            away_for = Some(held);
            if !reported && held >= SWEEP_HOLD {
                sweep = Some(from_grab_deg);
                self.away_since = Some((since, true));
            }
        } else if from_grab_deg < SWEEP_DEG * 0.5 {
            self.away_since = None;
        }
        Events {
            change_deg: tick_change,
            from_grab_deg: Some(from_grab_deg),
            snap: tick_change.filter(|&c| c > FLIP_DEG),
            sweep,
            bone_change,
            wrist_drift_deg,
            away_for,
        }
    }
}

/// The gun-on-wrist rotation a grab started with.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Grab {
    weapon: usize,
    gun: usize,
    gun_in_wrist: Quat,
    wrist_in_target: Option<Quat>,
}

/// Allows one log line per [`LOG_INTERVAL`], counting the ones it holds back.
#[derive(Debug, Copy, Clone)]
struct Limiter {
    last: Option<Instant>,
    suppressed: u64,
}

impl Limiter {
    const fn new() -> Self {
        Limiter {
            last: None,
            suppressed: 0,
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < LOG_INTERVAL)
        {
            self.suppressed += 1;
            return false;
        }
        self.last = Some(now);
        self.suppressed = 0;
        true
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Kind {
    Snap,
    Sweep,
}

impl Kind {
    const fn label(self) -> &'static str {
        match self {
            Kind::Snap => "snap between two ticks",
            Kind::Sweep => "sweep away from the grab",
        }
    }
}

/// What the log lines add besides the record.
struct Context {
    hand: Hand,
    weapon_state: WeaponState,
    animation_state: Option<u32>,
}

fn log_context(context: &Context, record: &HandGrip, events: &Events) {
    let (axis, angle) = record.animated_in_wrist.to_axis_angle();
    tracing::info!(
        target: "controllers",
        hand = ?context.hand,
        bone = grip::bone_label(record.bone),
        gun = record.gun,
        weapon_state = ?context.weapon_state,
        placed_state = ?record.state,
        pinned = record.pinned,
        animated_deviation_deg = record.animated_deviation_deg,
        animated_in_wrist_axis = ?axis,
        animated_in_wrist_deg = angle.to_degrees(),
        change_deg = ?events.change_deg,
        from_grab_deg = ?events.from_grab_deg,
        away_ms = ?events.away_for.map(|d| d.as_millis()),
        wrist_miss_m = ?record.wrist_miss_m,
        wrist_drift_deg = ?events.wrist_drift_deg,
        ik_weight = ?record.weight,
        tier_state = tiers::status().state.map_or("none", |s| s.label()),
        animation_state = ?context.animation_state.map(|id| format!("{id:#010x}")),
        "held gun context"
    );
}

/// A record older than this is from a placement that no longer runs.
const MAX_AGE: Duration = Duration::from_millis(250);
const LOG_INTERVAL: Duration = Duration::from_secs(1);

struct State {
    hands: [HandWatch; 2],
    status: GripWatchStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    hands: [HandWatch::new(), HandWatch::new()],
    status: GripWatchStatus {
        change_deg: [None; 2],
        from_grab_deg: [None; 2],
        flips: [0; 2],
        sweeps: [0; 2],
        bone_changes: [0; 2],
        last_flip: None,
        pinned: [None; 2],
        animated_deviation_deg: [None; 2],
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    fn record(at: Instant, gun_in_wrist: Quat, bone: u32) -> HandGrip {
        HandGrip {
            at,
            weapon: 1,
            gun: 0,
            bone,
            state: WeaponState::E_WS_DUAL_WIELDING,
            pinned: false,
            animated_deviation_deg: 0.0,
            animated_in_wrist: Quat::IDENTITY,
            gun_in_wrist,
            wrist_in_target: Some(Quat::IDENTITY),
            wrist_miss_m: Some(0.0),
            weight: Some(1.0),
        }
    }

    const TICK: Duration = Duration::from_millis(33);

    #[test]
    fn a_steady_grip_changes_little_and_a_flip_a_lot() {
        let held = Quat::from_rotation_y(0.3);
        assert!(change_deg(held, Quat::from_rotation_y(0.35)) < 5.0);
        let flipped = Quat::from_rotation_z(std::f32::consts::PI) * held;
        assert!(change_deg(held, flipped) > FLIP_DEG);
        assert!(change_deg(held, -held) < 1e-3);
    }

    #[test]
    fn a_blended_sweep_is_caught_once_after_the_hold() {
        let mut watch = HandWatch::new();
        let start = Instant::now();
        // The gun turns about its pitch axis by twenty degrees a tick: no single tick is a snap.
        let mut events = Vec::new();
        for tick in 0..12u32 {
            let angle = (20.0 * tick.min(9) as f32).to_radians();
            let at = start + TICK * tick;
            events.push(watch.observe(Some(record(at, Quat::from_rotation_x(angle), 1)), at));
        }
        assert!(events.iter().all(|e| e.snap.is_none()));
        let sweeps: Vec<_> = events.iter().filter_map(|e| e.sweep).collect();
        assert_eq!(sweeps.len(), 1, "{events:?}");
        assert!(sweeps[0] > SWEEP_DEG);
    }

    #[test]
    fn a_short_excursion_is_not_a_sweep() {
        let mut watch = HandWatch::new();
        let start = Instant::now();
        let angles = [0.0f32, 70.0, 0.0, 0.0];
        let events: Vec<_> = angles
            .iter()
            .enumerate()
            .map(|(tick, &a)| {
                let at = start + TICK * tick as u32;
                watch.observe(
                    Some(record(at, Quat::from_rotation_x(a.to_radians()), 1)),
                    at,
                )
            })
            .collect();
        assert!(events.iter().all(|e| e.sweep.is_none()));
    }

    #[test]
    fn a_snap_and_a_bone_change_are_reported_and_keep_the_grab() {
        let mut watch = HandWatch::new();
        let start = Instant::now();
        watch.observe(Some(record(start, Quat::IDENTITY, 1)), start);
        let at = start + TICK;
        let flipped = Quat::from_rotation_x(170.0f32.to_radians());
        let events = watch.observe(Some(record(at, flipped, 2)), at);
        assert!(events.snap.is_some_and(|c| c > FLIP_DEG));
        assert_eq!(events.bone_change, Some(1));
        assert!(events.from_grab_deg.is_some_and(|d| d > 160.0));
    }

    #[test]
    fn the_limiter_holds_back_and_counts() {
        let mut limiter = Limiter::new();
        let start = Instant::now();
        assert!(limiter.allow(start));
        assert!(!limiter.allow(start + Duration::from_millis(10)));
        assert_eq!(limiter.suppressed, 1);
        assert!(limiter.allow(start + LOG_INTERVAL));
        assert_eq!(limiter.suppressed, 0);
    }
}
