//! The tactical knock: a fresh magazine from the pouch, held in the free hand, struck against the
//! magazine still in the gun knocks that one out of the well, as a tactical reload is done in the
//! films. Pure, but for the publication of the zone for the grab view.
//!
//! The pouch magazine in the hand has no body in the physics world (it is drawn from the gun's own
//! magazine part, or not at all), so the strike is a test of the held magazine's tip against the
//! inserted magazine: a capsule down the well's axis over the magazine's visible length, in the
//! well's moving frame, so that the gun's own motion does not count. The tip must come into the
//! capsule from outside, between two input ticks or along the line between them, closing on it
//! faster than [`KnockConfig::min_speed_mps`] relative to the gun. A tip that rests against the
//! magazine or slides along it does nothing.
//!
//! The knocked magazine leaves through the eject's own drop, with the eject's round accounting and
//! its unlimited-ammunition rule. Its flight is the physics world's: the drop gives the prop the
//! gun's velocity, and the strike adds an impulse at the struck point as a collision with a hand
//! driven body would, so a base strike sends it out along the well and a strike on its side near
//! the well turns it as it goes ([`knocked`]).
//!
//! [`KnockConfig::min_speed_mps`]: crate::controllers::weapons::reload::config::KnockConfig::min_speed_mps

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    physical_hands::rigid::{RigidBody, Velocities},
    weapons::reload::{
        loose::Loose,
        plan::{LoadKind, WellFrame},
        wells::WellEntry,
    },
};

/// The inserted magazine a held one can knock out, in the cockpit frame at the player's scale.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct KnockZone {
    /// The well's mouth.
    pub mouth: Vec3,
    /// Into the gun along the well, the way a magazine seats: a unit vector.
    pub axis: Vec3,
    /// How far the magazine hangs below the mouth: its base is this far out along the axis.
    pub below: f32,
    /// How far into the well above the mouth the zone reaches.
    pub lip: f32,
    /// How far from the axis a tip touches the magazine.
    pub radius: f32,
}

impl KnockZone {
    /// The zone of a magazine hanging `below_m` (model metres) out of a well with its mouth at
    /// `mouth` and its insertion axis `axis`, at `world_scale`. `None` for a degenerate axis, scale,
    /// or point.
    pub(super) fn of(mouth: Vec3, axis: Vec3, below_m: f32, world_scale: f32) -> Option<Self> {
        let axis = axis.try_normalize()?;
        let scale = Some(world_scale).filter(|s| s.is_finite() && *s > 0.0)?;
        (mouth.is_finite() && below_m.is_finite()).then_some(Self {
            mouth,
            axis,
            below: below_m.max(0.0) / scale,
            lip: KNOCK.lip_m / scale,
            radius: KNOCK.radius_m / scale,
        })
    }

    /// The ends of the zone's axis: the top (inside the mouth) and the magazine's base.
    pub(crate) fn ends(&self) -> (Vec3, Vec3) {
        (
            self.mouth + self.axis * self.lip,
            self.mouth - self.axis * self.below,
        )
    }

    /// The magazine's centre: halfway down its visible length.
    fn centre_local(&self) -> Vec3 {
        Vec3::new(0.0, -self.below * 0.5, 0.0)
    }

    /// The well's frame: +Y along the axis into the gun, the mouth at the origin.
    fn rotation(&self) -> Quat {
        Quat::from_rotation_arc(self.axis, Vec3::Y)
    }

    fn local(&self, point: Vec3) -> Vec3 {
        self.rotation() * (point - self.mouth)
    }

    /// The point on the zone's axis nearest `local`, in the well's frame.
    fn closest_local(&self, local: Vec3) -> Vec3 {
        Vec3::new(0.0, local.y.clamp(-self.below, self.lip), 0.0)
    }

    fn distance_local(&self, local: Vec3) -> f32 {
        local.distance(self.closest_local(local))
    }
}

/// A strike of the held magazine's tip on the inserted magazine.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Strike {
    /// Where the tip met the magazine, from the magazine's centre, cockpit orientation.
    pub contact: Vec3,
    /// The tip's velocity relative to the gun, cockpit frame, metres per second at the player's
    /// scale.
    pub velocity: Vec3,
    /// How fast the tip closed on the magazine's surface.
    pub closing_mps: f32,
}

/// The knock's state across input ticks.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Knocks {
    /// The held tip on the last tick, in the well's frame, while a zone was in play.
    pub previous: Option<Vec3>,
    /// Each gun hand's strike, by [`Hand::index`], while its drop is queued.
    pub strikes: [Option<Strike>; 2],
    /// The knocked magazine's impulse, while it waits on a readable body.
    pub impulse: Option<PendingImpulse>,
    /// When a magazine was last knocked out.
    pub knocked_at: Option<Instant>,
}

impl Knocks {
    pub(super) const fn new() -> Self {
        Self {
            previous: None,
            strikes: [None; 2],
            impulse: None,
            knocked_at: None,
        }
    }

    /// Forget the tip, so that the next zone starts without an approach.
    pub(super) fn lose_tip(&mut self) {
        self.previous = None;
    }

    pub(super) fn cancel(&mut self) {
        self.previous = None;
        self.strikes = [None; 2];
    }
}

/// A strike's impulse for the magazine it knocked into the world, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct PendingImpulse {
    pub magazine: Loose,
    /// The struck point from the magazine's centre, game metres.
    pub contact: Vec3,
    /// The tip's velocity relative to the gun, game metres per second.
    pub velocity: Vec3,
    /// The ticks left to find the body readable.
    pub ticks_left: u32,
}

/// The knock's fixed tuning, in the gun model's metres.
pub(super) struct KnockTuning {
    /// How far from the magazine's axis the held tip touches it: a magazine's half-width plus the
    /// held tip's own, and a little of the hand's tracking error.
    pub radius_m: f32,
    /// How far into the well above the mouth the zone reaches, so a strike at the joint of the
    /// magazine and the gun counts.
    pub lip_m: f32,
    /// How much of the closing speed the struck point keeps the other way, as a collision with a
    /// hand-driven body: zero sends it on at the tip's speed, one at twice that.
    pub restitution: f32,
    /// The most a knock adds to the magazine's speed, metres per second.
    pub max_knock_mps: f32,
    /// The most a knock adds to the magazine's spin, radians per second.
    pub max_spin_radps: f32,
    /// How many input ticks a knocked magazine's impulse waits on its body becoming readable.
    pub impulse_ticks: u32,
    /// How long the grab view shows a knock.
    pub flash: Duration,
}

pub(super) const KNOCK: KnockTuning = KnockTuning {
    radius_m: 0.035,
    lip_m: 0.015,
    restitution: 0.3,
    max_knock_mps: 6.0,
    max_spin_radps: 25.0,
    impulse_ticks: 10,
    flash: Duration::from_millis(400),
};

/// The visible length of an inserted magazine below its well, model metres: the magazine part's
/// length, or zero for a magazine modelled into the gun, whose mouth is its base.
pub(super) fn below_m(entry: Option<&WellEntry>) -> f32 {
    entry
        .filter(|e| e.magazine_part)
        .and_then(|e| e.part_length_m)
        .filter(|l| l.is_finite())
        .map_or(0.0, |l| l.max(0.0))
}

/// The knock zone of a gun loading `load` through `well`, with its magazine `out` or not, the
/// well's mouth and axis as the tick placed them. Only a magazine in a wells-table well can be
/// knocked out; a cylinder, or a well the table does not place, is ejected or pulled.
pub(super) fn zone_for(
    load: LoadKind,
    well: WellFrame,
    out: bool,
    placed: Option<(Vec3, Vec3)>,
    below_m: f32,
    world_scale: f32,
) -> Option<KnockZone> {
    if load != LoadKind::Magazine || out || !matches!(well, WellFrame::Gun { .. }) {
        return None;
    }
    let (mouth, axis) = placed?;
    KnockZone::of(mouth, axis, below_m, world_scale)
}

/// Fold the held tip at `tip` into `zone` over `dt`, from its place on the last tick, `previous`,
/// in the well's frame. Returns the strike, if the tip came into the magazine from outside closing
/// on it at `min_speed_mps` or faster, and the tip's place in the well's frame for the next tick.
pub(super) fn strike(
    zone: &KnockZone,
    previous: Option<Vec3>,
    tip: Vec3,
    dt: f32,
    min_speed_mps: f32,
) -> (Option<Strike>, Option<Vec3>) {
    let local = zone.local(tip);
    if !local.is_finite() {
        return (None, None);
    }
    let Some(previous) = previous.filter(|_| dt.is_finite() && dt > MIN_DT) else {
        return (None, Some(local));
    };
    // A tip already touching the magazine rests or slides on it: a knock comes from outside.
    if zone.distance_local(previous) <= zone.radius {
        return (None, Some(local));
    }
    let travel = local - previous;
    let steps = ((travel.length() / (zone.radius * 0.5)).ceil() as u32).clamp(1, MAX_SWEEP_STEPS);
    let Some(entry) = (1..=steps)
        .map(|i| previous.lerp(local, i as f32 / steps as f32))
        .find(|p| zone.distance_local(*p) <= zone.radius)
    else {
        return (None, Some(local));
    };
    let velocity = travel / dt;
    let on_axis = zone.closest_local(entry);
    let normal = (entry - on_axis)
        .try_normalize()
        .or_else(|| (-velocity).try_normalize())
        .unwrap_or(Vec3::NEG_Y);
    let closing_mps = -velocity.dot(normal);
    if closing_mps.is_nan() || closing_mps < min_speed_mps.max(0.0) {
        return (None, Some(local));
    }
    let to_cockpit = zone.rotation().inverse();
    let strike = Strike {
        contact: to_cockpit * (on_axis - zone.centre_local()),
        velocity: to_cockpit * velocity,
        closing_mps,
    };
    (Some(strike), Some(local))
}

/// The velocities of `body` after a strike at the world point `contact` by a tip moving at
/// `strike` relative to it: the impulse a hand-driven body gives the struck point along the
/// strike, with the restitution, which changes its linear velocity by the impulse over the mass and
/// its spin by the impulse's moment about the centre. The change is capped.
pub(super) fn knocked(
    body: RigidBody,
    contact: Vec3,
    strike: Vec3,
    tuning: &KnockTuning,
) -> Velocities {
    let before = body.velocities;
    let (Some(direction), true) = (strike.try_normalize(), contact.is_finite()) else {
        return before;
    };
    let mass = body.mass_along(contact, direction);
    if mass.is_nan() || mass <= 0.0 {
        return before;
    }
    let mut after = body;
    after.apply_impulse(
        direction * ((1.0 + tuning.restitution.max(0.0)) * mass * strike.length()),
        contact,
    );
    Velocities {
        linear: before.linear
            + (after.velocities.linear - before.linear).clamp_length_max(tuning.max_knock_mps),
        angular: before.angular
            + (after.velocities.angular - before.angular).clamp_length_max(tuning.max_spin_radps),
    }
}

/// One held gun's knock zone for the grab view.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct KnockView {
    /// The hand holding the gun; the held magazine is in the other.
    pub holder: Hand,
    pub zone: KnockZone,
    /// The held magazine's tip, cockpit frame.
    pub tip: Option<Vec3>,
    /// Whether the tip's last move would have knocked the magazine out had it met it: its speed
    /// relative to the gun is at least the threshold.
    pub fast: bool,
    /// When the magazine was last knocked out.
    pub knocked_at: Option<Instant>,
}

impl KnockView {
    /// Whether the knock is recent enough to flash.
    pub(crate) fn flashing(&self, now: Instant) -> bool {
        self.knocked_at
            .is_some_and(|at| now.saturating_duration_since(at) <= KNOCK.flash)
    }
}

/// The last input tick's knock zone, while one is in play or its knock still flashes.
pub(crate) fn view() -> Option<KnockView> {
    let now = Instant::now();
    match *VIEW.lock() {
        Some((at, view))
            if now.saturating_duration_since(at) <= VIEW_MAX_AGE || view.flashing(now) =>
        {
            Some(view)
        }
        _ => None,
    }
}

/// Publish the tick's zone. A tick without one keeps the last zone while its knock flashes, since
/// the knocked magazine leaves the well, and the zone with it, within a tick or two.
pub(super) fn publish(view: Option<KnockView>) {
    let now = Instant::now();
    let mut published = VIEW.lock();
    match view {
        Some(view) => *published = Some((now, view)),
        None => {
            if !published.is_some_and(|(_, view)| view.flashing(now)) {
                *published = None;
            }
        }
    }
}

/// The shortest tick the tip's velocity is taken over.
const MIN_DT: f32 = 1e-4;
/// The most points the tip's path between two ticks is tested at.
const MAX_SWEEP_STEPS: u32 = 32;
/// How long a published view is drawn without a newer one.
const VIEW_MAX_AGE: Duration = Duration::from_millis(250);

static VIEW: Mutex<Option<(Instant, KnockView)>> = Mutex::new(None);

#[cfg(test)]
mod tests;
