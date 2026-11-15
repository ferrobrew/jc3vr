//! A step's swing: its planned landing, probed for the ground, and the arc the foot follows to
//! it. Pure.

use glam::Vec3;

use crate::{
    angle::wrap,
    contact::{
        ContactFrame, ContactTuning, FootPose, Ground, Probe, Release, Side, Surroundings,
        stance_spot,
    },
    smooth::smoothstep,
};

/// A foot on its way to a landing.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Swing {
    /// Where the foot left, in the model's frame.
    pub from: FootPose,
    /// Where it lands across the ground, in the model's frame; the height is the ground's once
    /// the probe answers.
    pub to: FootPose,
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

    /// The foot's pose on the arc now, its ankle `ankle_height_m` over the landing's ground.
    pub(crate) fn pose(&self, ankle_height_m: f32) -> FootPose {
        let t = self.progress();
        let ease = smoothstep(t);
        let land_y = match self.landing {
            Landing::Known(ground) => ground.height + ankle_height_m,
            Landing::Pending | Landing::Missed => self.to.ankle.y,
        };
        let flat = self.from.flat().lerp(self.to.flat(), ease);
        let lift = self.arc_m.max(0.0) * 4.0 * t * (1.0 - t);
        FootPose {
            ankle: Vec3::new(
                flat.x,
                self.from.ankle.y + (land_y - self.from.ankle.y) * ease + lift,
                flat.y,
            ),
            yaw: wrap(self.from.yaw + wrap(self.to.yaw - self.from.yaw) * ease),
        }
    }

    /// The pose the foot is planted in on `ground`.
    pub(crate) fn landed(&self, ground: Ground, ankle_height_m: f32) -> FootPose {
        FootPose {
            ankle: Vec3::new(
                self.to.ankle.x,
                ground.height + ankle_height_m,
                self.to.ankle.z,
            ),
            yaw: self.to.yaw,
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

/// Plan `side`'s step from `from`: to its stance spot under the pelvis, led along the pelvis's
/// travel over the swing, probed for the ground there.
pub(crate) fn plan(
    frame: &ContactFrame,
    side: Side,
    from: FootPose,
    world: &mut impl Surroundings,
    tuning: &ContactTuning,
    reason: Release,
) -> Swing {
    let (spot, yaw) = stance_spot(frame, side);
    let lead = if frame.pelvis_velocity.is_finite() {
        frame.pelvis_velocity * tuning.step_s.max(0.0) * tuning.lead.max(0.0)
    } else {
        glam::Vec2::ZERO
    };
    let spot = spot + lead;
    let landing = match world.probe(side, spot, from.ankle.y) {
        Probe::Hit(ground) => Landing::Known(ground),
        Probe::Miss => Landing::Missed,
        Probe::Pending => Landing::Pending,
    };
    let height = match landing {
        Landing::Known(ground) => ground.height + frame.ankle_height_m,
        Landing::Pending | Landing::Missed => from.ankle.y,
    };
    Swing {
        from,
        to: FootPose {
            ankle: Vec3::new(spot.x, height, spot.y),
            yaw,
        },
        landing,
        elapsed: 0.0,
        duration: tuning.step_s.max(0.0),
        arc_m: tuning.arc_m,
        reason,
        waited: 0.0,
    }
}
