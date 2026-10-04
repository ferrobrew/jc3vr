//! The driving view: in the driver's seat, the view moved so that the eyes sit at one standard
//! offset from the steering wheel's hub in every vehicle.
//!
//! Where the hub sits from the seated character's eyes is a property of the seat: the driver
//! animation and the wheel part decide it, and it differs by a tenth of a metre or more from car
//! to car. The fit measures it on the input tick from the head pose's anchor (the animated head,
//! before the head pose or this correction moves it), so neither the player's posture nor a
//! correction already applied enters the measurement. Once the driver has settled into the seat,
//! [`Entry`] averages it over a short window and keeps the mean for the rest of the entry, so the
//! driving animation's sway does not move the view. [`derive`] turns the mean into the correction:
//! the hub's offset from the standard place, limited in length, or a [`SEAT_OVERRIDES`] entry's.
//!
//! The input tick publishes the correction for the vehicle ([`publish`]). The frame half of the
//! pose composition ([`crate::controllers::roomscale::compose_cockpit`]) eases toward it, and back
//! to nothing through a seat transition ([`advance`]), and adds it to the composed cockpit
//! position after the posture, the seated reference, and the crouch have been resolved, so it
//! neither feeds them nor is undone by them. The view, the hands, and the grab volumes are all
//! composed from the head pose, so they move together, and the correction is in the body frame,
//! which rides the seat and so the vehicle.
//!
//! Only a wheel fitted from the vehicle's own part, or the part's override, is measured: the
//! configured fallback wheel says nothing about the seat. Only cars are corrected by default,
//! since some boats are driven standing at the helm and the seated standard does not fit them.

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::vehicles::{config::DrivingViewConfig, fit::SETTLE};

/// A seat whose derived correction is replaced, keyed by the wheel part's name (see
/// [`SEAT_OVERRIDES`]).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SeatOverride {
    /// The steering wheel part's name.
    pub part: &'static str,
    /// The correction to apply instead of the derived one, cockpit frame, metres, still limited to
    /// [`DrivingViewConfig::max_m`]; `None` leaves the seat's view where it is.
    pub correction: Option<Vec3>,
}

/// The seats whose derived correction is replaced, for tuning in the headset: the logged `driving
/// position` line gives each vehicle's wheel part and measurement.
pub const SEAT_OVERRIDES: &[SeatOverride] = &[
    // A ship's wheel at a standing helm, where the seated standard does not apply.
    SeatOverride {
        part: "transport_steering_wheel",
        correction: None,
    },
];

/// The override for the wheel part named `part`, if any.
pub fn find_override(part: &str) -> Option<&'static SeatOverride> {
    SEAT_OVERRIDES.iter().find(|o| o.part == part)
}

/// A seat's view correction.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Correction {
    /// The hub from the character's eyes, before any correction, cockpit frame, metres.
    pub hub_from_eyes: Vec3,
    /// The correction that would put the hub at the standard place: the view moves by this.
    pub wanted: Vec3,
    /// The correction to apply: `wanted` limited in length, or the override's.
    pub applied: Vec3,
    pub origin: CorrectionOrigin,
}

/// Where a [`Correction`]'s applied value came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CorrectionOrigin {
    /// The hub's offset from the standard place, in full.
    Derived,
    /// The hub's offset from the standard place, shortened to the longest correction.
    Limited,
    /// A [`SeatOverride`] with a correction.
    Override,
    /// A [`SeatOverride`] without one.
    Skipped,
}

impl CorrectionOrigin {
    pub const fn label(self) -> &'static str {
        match self {
            CorrectionOrigin::Derived => "derived",
            CorrectionOrigin::Limited => "derived, limited",
            CorrectionOrigin::Override => "the seat's override",
            CorrectionOrigin::Skipped => "skipped by the seat's override",
        }
    }
}

/// The correction for a seat whose hub sits at `hub_from_eyes` from the character's eyes.
///
/// Moving the view by `c` moves the hub by `-c` relative to it, so the correction that puts the
/// hub at the standard place is the hub's offset from it.
pub fn derive(
    hub_from_eyes: Vec3,
    config: &DrivingViewConfig,
    seat_override: Option<&SeatOverride>,
) -> Correction {
    let wanted = hub_from_eyes - config.hub_from_eyes_m;
    let wanted = if wanted.is_finite() {
        wanted
    } else {
        Vec3::ZERO
    };
    let (unlimited, origin) = match seat_override.map(|o| o.correction) {
        None => (wanted, CorrectionOrigin::Derived),
        Some(Some(fixed)) if fixed.is_finite() => (fixed, CorrectionOrigin::Override),
        Some(_) => (Vec3::ZERO, CorrectionOrigin::Skipped),
    };
    let max = if config.max_m.is_finite() {
        config.max_m.max(0.0)
    } else {
        0.0
    };
    let applied = unlimited.clamp_length_max(max);
    let origin = if origin == CorrectionOrigin::Derived && applied != unlimited {
        CorrectionOrigin::Limited
    } else {
        origin
    };
    Correction {
        hub_from_eyes,
        wanted,
        applied,
        origin,
    }
}

/// One vehicle entry's measurement of the hub: nothing until the driver has settled, then the
/// mean over [`SAMPLE_WINDOW`], kept for the rest of the entry.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Entry {
    sampler: Sampler,
    closed: bool,
    mean: Option<Vec3>,
}

impl Entry {
    pub const fn new() -> Self {
        Self {
            sampler: Sampler::new(),
            closed: false,
            mean: None,
        }
    }

    /// Take this tick's hub from the eyes, `elapsed` after the entry began; `None` while it is not
    /// measured (no anchor yet, or a fallback wheel). Returns whether the window closed on this
    /// tick, which happens once per entry.
    pub fn observe(&mut self, elapsed: Duration, hub_from_eyes: Option<Vec3>) -> bool {
        if self.closed || elapsed < SETTLE {
            return false;
        }
        if let Some(hub) = hub_from_eyes {
            self.sampler.add(hub);
        }
        if elapsed < SETTLE + SAMPLE_WINDOW {
            return false;
        }
        self.closed = true;
        self.mean = self.sampler.mean();
        true
    }

    /// The hub from the eyes measured for the entry, once the window has closed with samples.
    pub fn mean(&self) -> Option<Vec3> {
        self.mean
    }
}

impl Default for Entry {
    fn default() -> Self {
        Self::new()
    }
}

/// How long after the driver has settled the hub is averaged over.
pub const SAMPLE_WINDOW: Duration = Duration::from_millis(500);

/// The correction the input tick asks for in the vehicle the player drives.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Target {
    /// The vehicle's address, which identifies the entry.
    pub vehicle: usize,
    pub correction: Correction,
}

/// Publish the input tick's target: the driven vehicle's correction, or `None` out of a measured
/// driver's seat. A leaf slot, taken briefly.
pub fn publish(target: Option<Target>) {
    let mut shared = SHARED.lock();
    shared.target = target;
    shared.published_at = Some(Instant::now());
}

/// The frame half's inputs to [`advance`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameInputs {
    /// Whether the player is not entering, shuffling between, or leaving a seat.
    pub seated: bool,
    /// Seconds since the previous frame.
    pub dt: f32,
}

/// Ease the correction toward the published target and return it, cockpit frame, metres, to add
/// to the composed cockpit position. Called once per rendered VR frame by the pose composition,
/// under the roomscale frame lock; takes only this module's slot.
pub fn advance(config: &DrivingViewConfig, inputs: FrameInputs, now: Instant) -> Vec3 {
    let mut shared = SHARED.lock();
    let fresh = shared
        .published_at
        .is_some_and(|at| now.saturating_duration_since(at) < STALE_AFTER);
    let goal = match shared.target {
        Some(target) if config.enabled && inputs.seated && fresh => target.correction.applied,
        _ => Vec3::ZERO,
    };
    let offset = shared.ease.advance(goal, inputs.dt, config.blend_s);
    shared.offset = offset;
    shared.goal = goal;
    offset
}

/// The driving view's readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DrivingViewStatus {
    /// The input tick's latest target.
    pub target: Option<Target>,
    /// The correction the view is easing toward.
    pub goal: Vec3,
    /// The correction the view was composed with on the latest frame.
    pub offset: Vec3,
}

pub fn status() -> DrivingViewStatus {
    let shared = SHARED.lock();
    DrivingViewStatus {
        target: shared.target,
        goal: shared.goal,
        offset: shared.offset,
    }
}

/// A target older than this is ignored: the input tick stopped publishing, so the view eases back.
const STALE_AFTER: Duration = Duration::from_millis(500);

/// A running mean of the measured hub.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Sampler {
    sum: Vec3,
    count: u32,
}

impl Sampler {
    const fn new() -> Self {
        Self {
            sum: Vec3::ZERO,
            count: 0,
        }
    }

    fn add(&mut self, sample: Vec3) {
        if sample.is_finite() {
            self.sum += sample;
            self.count += 1;
        }
    }

    fn mean(&self) -> Option<Vec3> {
        (self.count > 0).then(|| self.sum / self.count as f32)
    }
}

/// An eased move from one correction to another, restarted from wherever it is whenever the goal
/// changes, so a new goal never jumps the view.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Ease {
    from: Vec3,
    to: Vec3,
    elapsed: f32,
}

impl Ease {
    const fn new() -> Self {
        Self {
            from: Vec3::ZERO,
            to: Vec3::ZERO,
            elapsed: 0.0,
        }
    }

    /// Step toward `goal` by `dt` seconds, over `duration` seconds, and return the value.
    fn advance(&mut self, goal: Vec3, dt: f32, duration: f32) -> Vec3 {
        if goal != self.to {
            self.from = self.value(duration);
            self.to = goal;
            self.elapsed = 0.0;
        }
        if dt.is_finite() {
            self.elapsed += dt.max(0.0);
        }
        self.value(duration)
    }

    fn value(&self, duration: f32) -> Vec3 {
        if !(duration.is_finite() && duration > 0.0) {
            return self.to;
        }
        let t = (self.elapsed / duration).clamp(0.0, 1.0);
        self.from.lerp(self.to, t * t * (3.0 - 2.0 * t))
    }
}

struct Shared {
    target: Option<Target>,
    published_at: Option<Instant>,
    ease: Ease,
    goal: Vec3,
    offset: Vec3,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    target: None,
    published_at: None,
    ease: Ease::new(),
    goal: Vec3::ZERO,
    offset: Vec3::ZERO,
});

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn config() -> DrivingViewConfig {
        DrivingViewConfig::new()
    }

    #[test]
    fn a_seat_at_the_standard_is_left_alone() {
        let c = config();
        let correction = derive(c.hub_from_eyes_m, &c, None);
        assert!(correction.applied.length() < EPS);
        assert_eq!(correction.origin, CorrectionOrigin::Derived);
    }

    /// Moving the view by the correction puts the hub at the standard place.
    #[test]
    fn the_correction_puts_the_hub_at_the_standard() {
        let c = config();
        let hub = c.hub_from_eyes_m + Vec3::new(0.02, -0.08, -0.06);
        let correction = derive(hub, &c, None);
        assert!((hub - correction.applied - c.hub_from_eyes_m).length() < EPS);
        assert_eq!(correction.origin, CorrectionOrigin::Derived);
    }

    /// A seat further off than the limit is moved the limit's length toward the standard.
    #[test]
    fn a_long_correction_is_limited_along_its_direction() {
        let c = config();
        let off = Vec3::new(0.0, 0.3, -0.4);
        let correction = derive(c.hub_from_eyes_m + off, &c, None);
        assert!((correction.applied.length() - c.max_m).abs() < EPS);
        assert!(correction.applied.normalize().dot(off.normalize()) > 1.0 - EPS);
        assert!((correction.wanted - off).length() < EPS);
        assert_eq!(correction.origin, CorrectionOrigin::Limited);
    }

    #[test]
    fn overrides_replace_or_skip_the_correction() {
        let c = config();
        let hub = c.hub_from_eyes_m + Vec3::new(0.0, -0.1, 0.0);
        let seat = |correction| SeatOverride {
            part: "a_steeringwheel",
            correction,
        };
        let fixed = derive(hub, &c, Some(&seat(Some(Vec3::new(0.0, 0.05, 0.0)))));
        assert!((fixed.applied - Vec3::new(0.0, 0.05, 0.0)).length() < EPS);
        assert_eq!(fixed.origin, CorrectionOrigin::Override);
        let too_far = derive(hub, &c, Some(&seat(Some(Vec3::new(0.0, 1.0, 0.0)))));
        assert!((too_far.applied.length() - c.max_m).abs() < EPS);
        let skipped = derive(hub, &c, Some(&seat(None)));
        assert_eq!(skipped.applied, Vec3::ZERO);
        assert_eq!(skipped.origin, CorrectionOrigin::Skipped);
    }

    #[test]
    fn a_bad_measurement_or_limit_corrects_nothing() {
        let c = config();
        assert_eq!(derive(Vec3::NAN, &c, None).applied, Vec3::ZERO);
        let no_limit = DrivingViewConfig {
            max_m: f32::NAN,
            ..c
        };
        assert_eq!(
            derive(Vec3::new(0.0, 0.0, -1.0), &no_limit, None).applied,
            Vec3::ZERO
        );
    }

    /// The entry measures nothing before the driver settles, averages over the window, closes
    /// once, and keeps its mean afterwards.
    #[test]
    fn the_entry_averages_over_the_window_once() {
        let mut entry = Entry::new();
        assert!(!entry.observe(Duration::ZERO, Some(Vec3::splat(9.0))));
        assert!(!entry.observe(SETTLE, Some(Vec3::new(0.0, -0.2, -0.4))));
        assert!(!entry.observe(SETTLE + SAMPLE_WINDOW / 2, None));
        assert!(entry.observe(SETTLE + SAMPLE_WINDOW, Some(Vec3::new(0.0, -0.4, -0.6))));
        assert!((entry.mean().unwrap() - Vec3::new(0.0, -0.3, -0.5)).length() < EPS);
        assert!(!entry.observe(SETTLE * 4, Some(Vec3::splat(9.0))));
        assert!((entry.mean().unwrap() - Vec3::new(0.0, -0.3, -0.5)).length() < EPS);
    }

    #[test]
    fn an_entry_without_samples_has_no_mean() {
        let mut entry = Entry::new();
        assert!(entry.observe(SETTLE + SAMPLE_WINDOW, None));
        assert_eq!(entry.mean(), None);
    }

    /// The ease starts and ends at rest and reaches the goal after its duration.
    #[test]
    fn the_ease_reaches_the_goal_smoothly() {
        let goal = Vec3::new(0.0, 0.1, -0.2);
        let mut ease = Ease::new();
        let first = ease.advance(goal, 0.0, 1.0);
        assert_eq!(first, Vec3::ZERO);
        let mid = ease.advance(goal, 0.5, 1.0);
        assert!((mid - goal * 0.5).length() < EPS);
        let early = Ease {
            from: Vec3::ZERO,
            to: goal,
            elapsed: 0.05,
        }
        .value(1.0);
        assert!(early.length() < goal.length() * 0.05, "slow out of rest");
        assert!((ease.advance(goal, 0.6, 1.0) - goal).length() < EPS);
        assert!((ease.advance(goal, 5.0, 1.0) - goal).length() < EPS);
    }

    /// A goal changed part way restarts from where the view is, so it never jumps.
    #[test]
    fn a_new_goal_starts_from_the_current_value() {
        let mut ease = Ease::new();
        let goal = Vec3::new(0.0, 0.0, -0.2);
        let partway = ease.advance(goal, 0.3, 1.0);
        let restarted = ease.advance(Vec3::ZERO, 0.0, 1.0);
        assert!((restarted - partway).length() < EPS);
        assert!(ease.advance(Vec3::ZERO, 1.0, 1.0).length() < EPS);
    }

    #[test]
    fn a_zero_duration_or_bad_step_is_immediate_and_safe() {
        let goal = Vec3::new(0.1, 0.0, 0.0);
        let mut ease = Ease::new();
        assert_eq!(ease.advance(goal, 0.0, 0.0), goal);
        let mut ease = Ease::new();
        let value = ease.advance(goal, f32::NAN, 1.0);
        assert!(value.is_finite());
    }
}
