//! The driving view: in the driver's seat, the view moved so that the eyes sit at one standard
//! offset from the steering wheel's hub in every vehicle.
//!
//! Where the hub sits from the seated character's eyes is a property of the seat: the driver
//! animation and the wheel part decide it, and it differs by a tenth of a metre or more from car
//! to car. The fit measures it on the input tick from the character's eyes: the head pose's anchor
//! (the animated head, before the head pose or this correction moves it) with the camera's eye arm
//! hung off it, so neither the player's posture nor a correction already applied enters the
//! measurement. Once the driver has settled into the seat,
//! [`Entry`] averages it over a short window and keeps the mean for the rest of the entry, so the
//! driving animation's sway does not move the view. [`derive`] turns the mean into the correction:
//! the hub's offset from the standard place, limited in length, or a [`SEAT_OVERRIDES`] entry's.
//!
//! The input tick publishes the correction for the vehicle ([`publish`]). The frame half of the
//! pose composition ([`jc3vr_body::controllers::roomscale::compose_cockpit`]) eases toward it, and
//! back to nothing through a seat transition ([`facts::driving::advance`]), and adds it to the
//! composed cockpit position after the posture, the seated reference, and the crouch have been
//! resolved, so it neither feeds them nor is undone by them. The view, the hands, and the grab
//! volumes are all composed from the eye point the head pose places, so they move together, and the
//! correction is in the body frame, which rides the seat and so the vehicle.
//!
//! Only a wheel fitted from the vehicle's own part, or the part's override, is measured: the
//! fallback wheel says nothing about the seat. Only cars are corrected, since some boats are driven
//! standing at the helm and the seated standard does not fit them.
//!
//! A vehicle's gunner seat is corrected the same way from the manned gun's handles: the turret's
//! animation puts the head right over them, so on the 14 November take the view looked down at the
//! handles from a few centimetres and the player stepped back to reach them. Their midpoint is put
//! at the tuning's `driving_view.handles_from_eyes_m` ([`derive_handles`]).

use std::time::Duration;

use glam::Vec3;
use jc3vr_body::controllers::facts;
use parking_lot::Mutex;

use crate::vehicles::tuning::VEHICLES;

/// A seat whose derived correction is replaced, keyed by the wheel part's name (see
/// [`SEAT_OVERRIDES`]).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SeatOverride {
    /// The steering wheel part's name.
    pub part: &'static str,
    /// The correction to apply instead of the derived one, cockpit frame, metres, still limited to
    /// the tuning's `driving_view.max_correction_m`; `None` leaves the seat's view where it is.
    pub correction: Option<Vec3>,
}

/// The seats whose derived correction is replaced, for tuning in the headset: the logged `driving
/// position` line gives each vehicle's wheel part and measurement.
pub(crate) const SEAT_OVERRIDES: &[SeatOverride] = &[
    // A ship's wheel at a standing helm, where the seated standard does not apply.
    SeatOverride {
        part: "transport_steering_wheel",
        correction: None,
    },
];

/// The override for the wheel part named `part`, if any.
pub(crate) fn find_override(part: &str) -> Option<&'static SeatOverride> {
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
pub fn derive(hub_from_eyes: Vec3, seat_override: Option<&SeatOverride>) -> Correction {
    derive_from(
        hub_from_eyes,
        VEHICLES.get().driving_view.hub_from_eyes_m,
        seat_override,
    )
}

/// The correction for a gunner seat whose gun's handles' midpoint sits at `handles_from_eyes`
/// from the character's eyes, as [`derive`] for a wheel's hub.
pub(crate) fn derive_handles(handles_from_eyes: Vec3) -> Correction {
    derive_from(
        handles_from_eyes,
        VEHICLES.get().driving_view.handles_from_eyes_m,
        None,
    )
}

/// The correction that puts a control measured at `from_eyes` at `standard`.
fn derive_from(
    hub_from_eyes: Vec3,
    standard: Vec3,
    seat_override: Option<&SeatOverride>,
) -> Correction {
    let wanted = hub_from_eyes - standard;
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
    let applied = unlimited.clamp_length_max(VEHICLES.get().driving_view.max_correction_m);
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
/// mean over the tuning's `driving_view.sample_window_ms`, kept for the rest of the entry.
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
        let tuning = &VEHICLES.get().driving_view;
        let settle = tuning.settle();
        if self.closed || elapsed < settle {
            return false;
        }
        if let Some(hub) = hub_from_eyes {
            self.sampler.add(hub);
        }
        if elapsed < settle + tuning.sample_window() {
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

/// The correction the input tick asks for in the vehicle the player drives.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Target {
    /// The vehicle's address, which identifies the entry.
    pub vehicle: usize,
    pub correction: Correction,
}

/// Publish the input tick's target: the driven vehicle's correction, or `None` out of a measured
/// driver's seat, for the readout, and its applied correction for the frame half
/// ([`facts::driving::publish`]). Leaf slots, taken briefly one after the other.
pub fn publish(target: Option<Target>) {
    *TARGET.lock() = target;
    facts::driving::publish(target.map(|t| t.correction.applied));
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
    let target = *TARGET.lock();
    let eased = facts::driving::status();
    DrivingViewStatus {
        target,
        goal: eased.goal,
        offset: eased.offset,
    }
}

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

static TARGET: Mutex<Option<Target>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicles::tuning::DrivingViewTuning;

    const EPS: f32 = 1e-5;

    fn view() -> &'static DrivingViewTuning {
        &VEHICLES.get().driving_view
    }

    #[test]
    fn a_seat_at_the_standard_is_left_alone() {
        let correction = derive(view().hub_from_eyes_m, None);
        assert!(correction.applied.length() < EPS);
        assert_eq!(correction.origin, CorrectionOrigin::Derived);
    }

    /// Moving the view by the correction puts the hub at the standard place.
    #[test]
    fn the_correction_puts_the_hub_at_the_standard() {
        let hub = view().hub_from_eyes_m + Vec3::new(0.02, -0.08, -0.06);
        let correction = derive(hub, None);
        assert!((hub - correction.applied - view().hub_from_eyes_m).length() < EPS);
        assert_eq!(correction.origin, CorrectionOrigin::Derived);
    }

    /// A seat further off than the limit is moved the limit's length toward the standard.
    #[test]
    fn a_long_correction_is_limited_along_its_direction() {
        let off = Vec3::new(0.0, 0.3, -0.4);
        let correction = derive(view().hub_from_eyes_m + off, None);
        assert!((correction.applied.length() - view().max_correction_m).abs() < EPS);
        assert!(correction.applied.normalize().dot(off.normalize()) > 1.0 - EPS);
        assert!((correction.wanted - off).length() < EPS);
        assert_eq!(correction.origin, CorrectionOrigin::Limited);
    }

    #[test]
    fn overrides_replace_or_skip_the_correction() {
        let hub = view().hub_from_eyes_m + Vec3::new(0.0, -0.1, 0.0);
        let seat = |correction| SeatOverride {
            part: "a_steeringwheel",
            correction,
        };
        let fixed = derive(hub, Some(&seat(Some(Vec3::new(0.0, 0.05, 0.0)))));
        assert!((fixed.applied - Vec3::new(0.0, 0.05, 0.0)).length() < EPS);
        assert_eq!(fixed.origin, CorrectionOrigin::Override);
        let too_far = derive(hub, Some(&seat(Some(Vec3::new(0.0, 1.0, 0.0)))));
        assert!((too_far.applied.length() - view().max_correction_m).abs() < EPS);
        let skipped = derive(hub, Some(&seat(None)));
        assert_eq!(skipped.applied, Vec3::ZERO);
        assert_eq!(skipped.origin, CorrectionOrigin::Skipped);
    }

    #[test]
    fn a_bad_measurement_corrects_nothing() {
        assert_eq!(derive(Vec3::NAN, None).applied, Vec3::ZERO);
    }

    /// The entry measures nothing before the driver settles, averages over the window, closes
    /// once, and keeps its mean afterwards.
    #[test]
    fn the_entry_averages_over_the_window_once() {
        let mut entry = Entry::new();
        assert!(!entry.observe(Duration::ZERO, Some(Vec3::splat(9.0))));
        assert!(!entry.observe(view().settle(), Some(Vec3::new(0.0, -0.2, -0.4))));
        assert!(!entry.observe(view().settle() + view().sample_window() / 2, None));
        assert!(entry.observe(
            view().settle() + view().sample_window(),
            Some(Vec3::new(0.0, -0.4, -0.6))
        ));
        assert!((entry.mean().unwrap() - Vec3::new(0.0, -0.3, -0.5)).length() < EPS);
        assert!(!entry.observe(view().settle() * 4, Some(Vec3::splat(9.0))));
        assert!((entry.mean().unwrap() - Vec3::new(0.0, -0.3, -0.5)).length() < EPS);
    }

    #[test]
    fn an_entry_without_samples_has_no_mean() {
        let mut entry = Entry::new();
        assert!(entry.observe(view().settle() + view().sample_window(), None));
        assert_eq!(entry.mean(), None);
    }
}
