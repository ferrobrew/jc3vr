//! The grip model's part of the input tick: resolve the hands among the targets the providers
//! placed, then, after the tables have latched the grips, publish the result for the readouts,
//! submit the markers, and log each grip press.
//!
//! The readout is a leaf slot that any thread may read ([`status`]), as the status rows ([`crate::status`])
//! do in the middle of a frame (`docs/mod/threading.md`).

use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    bindings::controls::ButtonControl,
    frame::Hand,
    grab::{
        GrabVolume, GripTarget, Reach, Resolved,
        latch::GripLatch,
        markers::{self, MarkerFrame},
        resolve::{self, losers, nearest_miss},
    },
    hands::ActiveHands,
    physical_hands,
};

/// One input tick's targets and how each hand stands among them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tick {
    pub targets: Vec<GripTarget>,
    pub resolved: [Resolved; 2],
    /// Each hand's grip as it is drawn, by [`Hand::index`].
    pub grips: [Option<Vec3>; 2],
    /// Whether each hand moves slowly enough for a fist to take what it comes to.
    pub slow: [bool; 2],
    /// What the providers noted about each hand's reach this tick, logged with its grip press.
    pub notes: [Vec<String>; 2],
}

impl Tick {
    /// What each hand's grip would take, by [`Hand::index`].
    pub fn chosen(&self) -> [Option<GrabVolume>; 2] {
        self.resolved.clone().map(|r| r.chosen.map(|c| c.volume))
    }

    /// The actual selected provider targets for summary-only eligibility, before losing identity.
    pub(crate) fn selected_for_summary(&self) -> [Option<GripTarget>; 2] {
        Hand::BOTH.map(|hand| {
            self.resolved[hand.index()]
                .chosen
                .and_then(|chosen| self.targets.get(chosen.index).copied())
        })
    }
}

/// Resolve the hands of `active` among `targets`, with the providers' `notes`.
pub(crate) fn resolve(
    targets: Vec<GripTarget>,
    notes: [Vec<String>; 2],
    active: Option<&ActiveHands>,
) -> Tick {
    let grips = active.map_or([None; 2], hand_grips);
    let resolved = resolve::resolve(&targets, grips);
    let slow = step_speeds(grips, Instant::now());
    Tick {
        targets,
        resolved,
        grips,
        slow,
        notes,
    }
}

/// Each hand's grip as it is drawn, where every target is reached.
pub(crate) fn hand_grips(active: &ActiveHands) -> [Option<Vec3>; 2] {
    active.hands.map(|hand| hand.map(|h| h.grip.position))
}

/// What the input tick hands the grip model after the tables.
pub(crate) struct Finish<'a> {
    pub tick: &'a Tick,
    pub latches: [GripLatch; 2],
    pub grips: [ButtonControl; 2],
    /// What each hand's grip took this tick.
    pub grabs: [Option<GrabVolume>; 2],
    /// Where the markers are placed, or `None` while the markers are off.
    pub markers: Option<MarkerFrame>,
}

/// Publish the tick, submit its markers, and log each grip press and what a fist took.
pub(crate) fn finish(finish: &Finish) {
    let Finish {
        tick,
        latches,
        grips,
        grabs,
        markers,
    } = *finish;
    for hand in Hand::BOTH {
        let i = hand.index();
        let pressed = grips[i].pressed;
        let acquired = !pressed && grabs[i].is_some();
        if pressed || acquired {
            log_press(hand, tick, latches[i], acquired);
        }
    }
    match markers {
        Some(frame) => markers::submit(&tick.targets, &tick.resolved, &frame),
        None => markers::clear(),
    }
    let mut published = STATUS.lock();
    published.1 = Some(Instant::now());
    let status = &mut published.0;
    status.live = true;
    status.targets = tick.targets.len();
    for hand in Hand::BOTH {
        let i = hand.index();
        status.hands[i] = HandStatus {
            latch: latches[i],
            chosen: tick.resolved[i].chosen.map(|c| (c.volume, c.distance)),
            in_reach: tick.resolved[i].reaches.len(),
        };
        status.grabs += u64::from(grabs[i].is_some());
    }
}

/// Forget the hands' speeds and clear the markers: the bindings stand down.
pub(crate) fn stand_down() {
    *SPEEDS.lock() = [None; 2];
    markers::clear();
    let mut published = STATUS.lock();
    published.0.live = false;
    published.0.hands = [HandStatus::default(); 2];
}

/// The grip model's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GrabStatus {
    /// Whether the input tick resolved the hands recently.
    pub live: bool,
    /// The targets placed on the last tick.
    pub targets: usize,
    pub hands: [HandStatus; 2],
    /// Grabs taken since the payload loaded.
    pub grabs: u64,
}

/// One hand's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandStatus {
    pub latch: GripLatch,
    /// What a squeeze would take, and how far its grab point is.
    pub chosen: Option<(GrabVolume, f32)>,
    /// How many targets the hand is within reach of.
    pub in_reach: usize,
}

/// The latest readout. Safe from any thread: it takes only a leaf lock.
pub fn status() -> GrabStatus {
    let (status, at) = *STATUS.lock();
    GrabStatus {
        live: status.live && at.is_some_and(|at| at.elapsed() < STATUS_MAX_AGE),
        ..status
    }
}

/// How long the readout counts as live after the last input tick.
const STATUS_MAX_AGE: Duration = Duration::from_millis(500);

/// The fastest a hand may move for a closed fist to take what it comes to, metres per second at the
/// player's scale: a hand laid on a handle is slower, and a punch is far faster.
pub const SLOW_HAND_MPS: f32 = 1.2;

/// Log a grip press of `hand` (or what its fist took, with `acquired`): what it was within reach
/// of, what it took or why not, and why the others lost.
fn log_press(hand: Hand, tick: &Tick, latch: GripLatch, acquired: bool) {
    let resolved = &tick.resolved[hand.index()];
    let mut considered = String::new();
    match resolved.chosen {
        Some(chosen) => {
            for (reach, lost) in losers(&resolved.reaches, chosen) {
                let _ = write!(
                    considered,
                    "{}{} at {:.0} mm ({})",
                    if considered.is_empty() { "" } else { "; " },
                    reach.volume.label(),
                    reach.distance * 1000.0,
                    lost.describe()
                );
            }
        }
        None => {
            for reach in &resolved.reaches {
                let _ = write!(
                    considered,
                    "{}{} at {:.0} mm (worked by the palm)",
                    if considered.is_empty() { "" } else { "; " },
                    reach.volume.label(),
                    reach.distance * 1000.0,
                );
            }
        }
    }
    let notes = tick.notes[hand.index()].join("; ");
    let chosen = resolved.chosen.map(describe);
    let missed = match resolved.chosen {
        Some(_) => None,
        None => {
            nearest_miss(hand, &tick.targets, tick.grips[hand.index()]).map(|(index, outside)| {
                format!(
                    "{} is {:.0} mm out of reach",
                    tick.targets[index].volume.label(),
                    outside * 1000.0
                )
            })
        }
    };
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        took = acquired,
        grip = latch.label(),
        chosen = chosen.as_deref().unwrap_or("nothing in reach"),
        others = if considered.is_empty() { "none" } else { considered.as_str() },
        nearest_miss = missed.as_deref().unwrap_or("none"),
        notes = if notes.is_empty() { "none" } else { notes.as_str() },
        palms = physical_hands::press_summary(hand),
        slow = tick.slow[hand.index()],
        targets = tick.targets.len(),
        "{}",
        if acquired {
            "grip: the closed hand came to a target and took it"
        } else {
            "grip: pressed"
        }
    );
}

fn describe(reach: Reach) -> String {
    format!(
        "{} ({}) at {:.0} mm",
        reach.volume.label(),
        reach.volume.category().label(),
        reach.distance * 1000.0
    )
}

/// Advance the hands' speeds from their drawn grips at `now`, and return whether each is slow.
fn step_speeds(grips: [Option<Vec3>; 2], now: Instant) -> [bool; 2] {
    let mut speeds = SPEEDS.lock();
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let point = grips[i];
        let last = std::mem::replace(&mut speeds[i], point.map(|p| (p, now)));
        slow_between(last, point.map(|p| (p, now)))
    })
}

/// Whether a hand that moved from `last` to `now` (each a point and when) moves slowly. A hand with
/// no motion to measure counts as slow.
fn slow_between(last: Option<(Vec3, Instant)>, now: Option<(Vec3, Instant)>) -> bool {
    let (Some((from, then)), Some((to, at))) = (last, now) else {
        return true;
    };
    let dt = at.saturating_duration_since(then).as_secs_f32();
    if dt <= f32::EPSILON {
        return true;
    }
    let speed = from.distance(to) / dt;
    !speed.is_finite() || speed <= SLOW_HAND_MPS
}

static SPEEDS: Mutex<[Option<(Vec3, Instant)>; 2]> = Mutex::new([None; 2]);

/// The published readout and when it was published.
static STATUS: Mutex<(GrabStatus, Option<Instant>)> = Mutex::new((
    GrabStatus {
        live: false,
        targets: 0,
        hands: [HandStatus {
            latch: GripLatch::Open,
            chosen: None,
            in_reach: 0,
        }; 2],
        grabs: 0,
    },
    None,
));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_is_slow_below_the_speed_a_fist_takes_things_at() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(100);
        let at = |x: f32, t| Some((Vec3::new(x, 0.0, 0.0), t));
        assert!(slow_between(at(0.0, t0), at(0.05, t1)), "0.5 m/s");
        assert!(!slow_between(at(0.0, t0), at(0.3, t1)), "3 m/s, a punch");
        assert!(slow_between(None, at(0.3, t1)), "nothing to measure");
        assert!(slow_between(at(0.0, t1), at(1.0, t1)), "no time passed");
    }
}
