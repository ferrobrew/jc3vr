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

/// The point `point` of a gun held in a hand drawn with its grip at `drawn_grip`, carried into the
/// frame the body's pouches are reached in, where the same hand's grip is at `body_grip`. On the
/// player's body the pouches are reached by the player's own hand, before the body-scale remap
/// shifts the drawn hand, and the gun rides the drawn hand; the remap shifts without turning, so
/// undoing its shift carries the gun back onto the player's hand. Without a body grip the point
/// stays where it is.
pub fn into_body_frame(point: Vec3, drawn_grip: Vec3, body_grip: Option<Vec3>) -> Vec3 {
    match body_grip {
        Some(body) if body.is_finite() && drawn_grip.is_finite() => point - (drawn_grip - body),
        _ => point,
    }
}

/// One tick's inputs for one gun, in the frame the pouch is placed in.
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

    #[test]
    fn a_remapped_gun_is_carried_back_onto_the_players_hand() {
        // The player's hand is on the belt; the remap draws it 6 cm further out and 2 cm lower.
        let body_grip = Vec3::new(0.15, -0.55, -0.1);
        let drawn_grip = body_grip + Vec3::new(0.06, -0.02, 0.0);
        let mouth = drawn_grip + Vec3::new(0.0, -0.08, -0.03);
        let carried = into_body_frame(mouth, drawn_grip, Some(body_grip));
        assert!(carried.distance(body_grip + Vec3::new(0.0, -0.08, -0.03)) < 1e-6);
        // A push the player makes onto the pouch on their own belt counts, whatever the remap.
        let mut b = BeltPush::new();
        let push = |height: f32| {
            at(height, -0.8).map(|i| PushInputs {
                mouth: into_body_frame(
                    i.mouth + (drawn_grip - body_grip),
                    drawn_grip,
                    Some(body_grip),
                ),
                ..i
            })
        };
        b.step(push(0.08), &CONFIG);
        assert!(b.step(push(0.0), &CONFIG));
        // Without the player's grip, the point stays put.
        assert_eq!(into_body_frame(mouth, drawn_grip, None), mouth);
        assert_eq!(into_body_frame(mouth, drawn_grip, Some(Vec3::NAN)), mouth);
    }
}
