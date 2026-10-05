//! The one-handed belt reload: the gun's well pushed down onto the belt pouch. Pure.
//!
//! A push counts when the well's mouth enters the pouch moving down faster than the push speed,
//! with the gun upright enough that its insertion axis (which points up into the gun from the
//! mouth) leans no further than the tilt limit from straight up, and the mouth then goes the push
//! depth into the pouch. An entry that misses any of these is a brush past the belt and does
//! nothing until the mouth leaves the pouch. After a reload the push stays latched until the mouth
//! leaves the pouch, so a gun held on the belt reloads once, and the fire stays held until then.

use glam::Vec3;

use crate::controllers::weapons::reload::config::BeltConfig;

/// Where one gun's push is.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum PushStage {
    /// The mouth is outside the pouch.
    #[default]
    Outside,
    /// The mouth entered the pouch as a push and is on its way to the push depth.
    Pushing,
    /// The mouth entered the pouch without a push, or a push reloaded; nothing happens until it
    /// leaves.
    Spent,
}

/// One tick's inputs for one gun, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PushInputs {
    pub mouth: Vec3,
    /// The direction a round travels into the gun from the mouth, unit length.
    pub axis: Vec3,
    /// The mouth's velocity, metres per second.
    pub velocity: Vec3,
    pub pouch_centre: Vec3,
    pub pouch_radius: f32,
    /// Whether the gun can be reloaded from the belt now.
    pub eligible: bool,
}

/// One gun's push detector.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct BeltPush {
    pub stage: PushStage,
}

impl BeltPush {
    pub const fn new() -> Self {
        Self {
            stage: PushStage::Outside,
        }
    }

    /// Fold in one tick (`None` without a gun to push); true when the push reloads.
    pub fn step(&mut self, inputs: Option<PushInputs>, config: &BeltConfig) -> bool {
        let Some(i) = inputs.filter(|i| i.mouth.is_finite() && i.velocity.is_finite()) else {
            self.stage = PushStage::Outside;
            return false;
        };
        let distance = i.mouth.distance(i.pouch_centre);
        if distance > i.pouch_radius {
            self.stage = PushStage::Outside;
            return false;
        }
        match self.stage {
            PushStage::Outside => {
                let upright = i.axis.dot(Vec3::Y) >= config.max_tilt_deg.to_radians().cos();
                let pushing = -i.velocity.y >= config.min_push_speed_mps;
                self.stage = if i.eligible && upright && pushing {
                    PushStage::Pushing
                } else {
                    PushStage::Spent
                };
                self.reach(distance, i.pouch_radius, config)
            }
            PushStage::Pushing => self.reach(distance, i.pouch_radius, config),
            PushStage::Spent => false,
        }
    }

    /// Whether the push holds the gun's fire: from the reload until the mouth leaves the pouch.
    pub fn holds_fire(&self) -> bool {
        self.stage != PushStage::Outside
    }

    fn reach(&mut self, distance: f32, radius: f32, config: &BeltConfig) -> bool {
        if self.stage == PushStage::Pushing && radius - distance >= config.depth_m {
            self.stage = PushStage::Spent;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: BeltConfig = BeltConfig::new();
    const POUCH: Vec3 = Vec3::new(0.2, -0.6, -0.1);

    fn at(height: f32, velocity_y: f32) -> Option<PushInputs> {
        Some(PushInputs {
            mouth: POUCH + Vec3::Y * height,
            axis: Vec3::Y,
            velocity: Vec3::Y * velocity_y,
            pouch_centre: POUCH,
            pouch_radius: 0.09,
            eligible: true,
        })
    }

    #[test]
    fn a_push_down_onto_the_pouch_reloads_once() {
        let mut b = BeltPush::new();
        assert!(!b.step(at(0.2, -0.8), &CONFIG));
        assert!(!b.step(at(0.08, -0.8), &CONFIG));
        assert_eq!(b.stage, PushStage::Pushing);
        assert!(b.step(at(0.05, -0.8), &CONFIG));
        assert!(b.holds_fire());
        assert!(!b.step(at(0.0, -0.1), &CONFIG));
        assert!(!b.step(at(0.2, 0.5), &CONFIG));
        assert!(!b.holds_fire());
    }

    #[test]
    fn a_slow_or_tilted_entry_does_nothing_until_it_leaves() {
        let mut b = BeltPush::new();
        b.step(at(0.08, -0.05), &CONFIG);
        assert!(!b.step(at(0.0, -0.8), &CONFIG));
        assert_eq!(b.stage, PushStage::Spent);

        let mut b = BeltPush::new();
        let tilted = at(0.08, -0.8).map(|i| PushInputs { axis: Vec3::Z, ..i });
        b.step(tilted, &CONFIG);
        assert!(!b.step(at(0.0, -0.8), &CONFIG));
    }

    #[test]
    fn an_ineligible_gun_or_no_gun_never_reloads() {
        let mut b = BeltPush::new();
        let ineligible = |h, v| {
            at(h, v).map(|i| PushInputs {
                eligible: false,
                ..i
            })
        };
        b.step(ineligible(0.08, -0.8), &CONFIG);
        assert!(!b.step(at(0.0, -0.8), &CONFIG));
        assert!(!b.step(None, &CONFIG));
        assert_eq!(b.stage, PushStage::Outside);
    }
}
