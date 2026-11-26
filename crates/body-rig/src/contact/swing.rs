//! A step's swing: its planned landing, probed for the ground, and the arc the foot follows to
//! it. Pure.
//!
//! The swing is held in the frame of the support the foot left, as a planted foot is, so a moving
//! deck carries a swinging foot along with the planted one and the body standing on it.

use glam::{Vec2, Vec3};

use crate::{
    contact::{
        ContactFrame, ContactTuning, FootPose, Ground, Probe, Release, Surroundings, stance_spot,
        support::{SupportFrame, SupportId},
    },
    math::{Side, angle::wrap, smooth::smoothstep},
};

/// A foot on its way to a landing.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Swing {
    /// Where the foot left, in the frame of [`Swing::support`].
    pub from: FootPose,
    /// Where it lands, in the frame of [`Swing::support`]; the height is the ground's once the
    /// probe answers.
    pub to: FootPose,
    /// The support the foot left, whose frame the swing is held in.
    pub support: SupportId,
    pub(crate) landing: Landing,
    /// Seconds since the foot left.
    pub elapsed: f32,
    /// How long the swing takes, seconds.
    pub duration: f32,
    /// How high the arc lifts the foot at its middle, metres.
    pub arc_m: f32,
    pub reason: Release,
    /// How long the swing has waited at its landing for the probe's answer, seconds.
    pub(crate) waited: f32,
}

impl Swing {
    /// How far along the swing is, `0..=1`.
    pub fn progress(&self) -> f32 {
        if self.duration > 0.0 && self.duration.is_finite() {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Whether the swing has reached its landing.
    pub fn arrived(&self) -> bool {
        self.progress() >= 1.0
    }

    /// Whether the landing's ground is known.
    pub fn landing_known(&self) -> bool {
        matches!(self.landing, Landing::Known(_))
    }

    /// Advance by `dt` seconds.
    pub(crate) fn advance(&mut self, dt: f32) {
        let before = self.elapsed;
        self.elapsed += dt;
        if self.duration.is_finite() && self.elapsed > self.duration {
            self.waited += (self.elapsed - self.duration.max(before)).max(0.0);
        }
    }

    /// Take the probe's answer for the landing, with the swing's support at `frame` now: a hit
    /// puts the landing's ankle `ankle_height_m` over the ground the probe found.
    pub(crate) fn answer(&mut self, probe: Probe, frame: &SupportFrame, ankle_height_m: f32) {
        self.landing = match probe {
            Probe::Hit(ground) => {
                let mut to = frame.to_model(self.to);
                to.ankle.y = ground.height + ankle_height_m;
                self.to = frame.to_local(to);
                Landing::Known(ground)
            }
            Probe::Miss => Landing::Missed,
            Probe::Pending => Landing::Pending,
        };
    }

    /// The foot's pose on the arc now, in the frame of [`Swing::support`].
    pub(crate) fn pose(&self) -> FootPose {
        let t = self.progress();
        let ease = smoothstep(t);
        let flat = self.from.flat().lerp(self.to.flat(), ease);
        let lift = self.arc_m.max(0.0) * 4.0 * t * (1.0 - t);
        FootPose {
            ankle: Vec3::new(
                flat.x,
                self.from.ankle.y + (self.to.ankle.y - self.from.ankle.y) * ease + lift,
                flat.y,
            ),
            yaw: wrap(self.from.yaw + wrap(self.to.yaw - self.from.yaw) * ease),
        }
    }
}

/// What the probe said of a swing's landing.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Landing {
    Known(Ground),
    Pending,
    Missed,
}

/// A step to plan.
pub(crate) struct Plan<'a> {
    pub frame: &'a ContactFrame,
    pub side: Side,
    /// Where the foot leaves, in the model's frame.
    pub from: FootPose,
    /// The support it leaves, and that support's frame now.
    pub support: SupportId,
    pub support_frame: SupportFrame,
    /// The pelvis's own velocity, relative to the support, `(x, z)`.
    pub velocity: Vec2,
    pub reason: Release,
}

/// Plan the step `request` asks for: to the foot's stance spot under the pelvis, led along the
/// pelvis's travel over the swing, probed for the ground there.
pub(crate) fn plan(
    request: &Plan<'_>,
    world: &mut impl Surroundings,
    tuning: &ContactTuning,
) -> Swing {
    let Plan {
        frame,
        side,
        from,
        support,
        support_frame,
        velocity,
        reason,
    } = *request;
    let (spot, yaw) = stance_spot(frame, side);
    let lead = if velocity.is_finite() {
        velocity * tuning.step_s.max(0.0) * tuning.lead.max(0.0)
    } else {
        Vec2::ZERO
    };
    let spot = spot + lead;
    let to = FootPose {
        ankle: Vec3::new(spot.x, from.ankle.y, spot.y),
        yaw,
    };
    let mut swing = Swing {
        from: support_frame.to_local(from),
        to: support_frame.to_local(to),
        support,
        landing: Landing::Pending,
        elapsed: 0.0,
        duration: tuning.step_s.max(0.0),
        arc_m: tuning.arc_m,
        reason,
        waited: 0.0,
    };
    let probe = world.probe(side, spot, from.ankle.y);
    swing.answer(probe, &support_frame, frame.ankle_height_m);
    swing
}
