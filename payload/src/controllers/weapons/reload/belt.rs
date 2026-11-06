//! The one-handed belt reload: the gun pushed down onto the belt pouch. Pure.
//!
//! The point pushed is the base of the magazine still in the gun, or the well's mouth with the
//! magazine out ([`push_point`]). A push counts when that point comes within the push sphere around
//! the pouch with the gun upright enough that its insertion axis (which points up into the gun from
//! the mouth) leans no further than the tilt limit from straight up, and then goes the push depth
//! into the sphere. How fast it comes does not matter: a hand bringing a gun to its hip slows as it
//! arrives. An entry that misses the tilt is a brush past the belt and does nothing until the point
//! leaves the sphere. After a reload the push stays latched until the point leaves the sphere, so a
//! gun held on the belt reloads once, and the fire stays held until then.

use glam::Vec3;

use crate::controllers::weapons::reload::tuning::BeltTuning;

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

/// One tick's inputs for one gun, in the frame the pouch is placed in.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PushInputs {
    /// The point pushed onto the pouch ([`push_point`]).
    pub point: Vec3,
    /// The direction a round travels into the gun from the mouth, unit length.
    pub axis: Vec3,
    pub pouch_centre: Vec3,
    /// The push sphere's radius around the pouch's centre.
    pub pouch_radius: f32,
    /// Whether the gun can be reloaded from the belt now.
    pub eligible: bool,
}

/// The point of a gun that is pushed onto the pouch: the base of the magazine hanging `below` out
/// of the well along its axis, from the mouth at `mouth`, or the mouth itself for a magazine that
/// is out or modelled into the gun (`below` zero).
pub fn push_point(mouth: Vec3, axis: Vec3, below: f32) -> Vec3 {
    let below = if below.is_finite() {
        below.max(0.0)
    } else {
        0.0
    };
    mouth - axis * below
}

/// Whether a gun whose point is at `point`, with its insertion axis `axis`, is upright enough for
/// a push and near enough the pouch at `centre` for the push to be offered: within `approach` of
/// the push sphere's surface of radius `radius`.
pub fn approaching(
    point: Vec3,
    axis: Vec3,
    centre: Vec3,
    radius: f32,
    approach: f32,
    config: &BeltTuning,
) -> bool {
    upright(axis, config) && point.distance(centre) <= radius + approach.max(0.0)
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
    pub fn step(&mut self, inputs: Option<PushInputs>, config: &BeltTuning) -> bool {
        let Some(i) = inputs.filter(|i| i.point.is_finite() && i.axis.is_finite()) else {
            self.stage = PushStage::Outside;
            return false;
        };
        let distance = i.point.distance(i.pouch_centre);
        if distance > i.pouch_radius {
            self.stage = PushStage::Outside;
            return false;
        }
        if !i.eligible {
            self.stage = if self.stage == PushStage::Pushing {
                PushStage::Outside
            } else {
                PushStage::Spent
            };
            return false;
        }
        match self.stage {
            PushStage::Outside => {
                self.stage = if upright(i.axis, config) {
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

    fn reach(&mut self, distance: f32, radius: f32, config: &BeltTuning) -> bool {
        if self.stage == PushStage::Pushing && radius - distance >= config.depth_m {
            self.stage = PushStage::Spent;
            return true;
        }
        false
    }
}

/// Whether the insertion axis `axis` leans no further than the tilt limit from straight up.
fn upright(axis: Vec3, config: &BeltTuning) -> bool {
    axis.dot(Vec3::Y) >= config.max_tilt_deg.to_radians().cos()
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::weapons::reload::tuning::RELOAD;

    const CONFIG: BeltTuning = RELOAD.belt;
    const POUCH: Vec3 = Vec3::new(0.2, -0.6, -0.1);

    fn at(height: f32) -> Option<PushInputs> {
        Some(PushInputs {
            point: POUCH + Vec3::Y * height,
            axis: Vec3::Y,
            pouch_centre: POUCH,
            pouch_radius: CONFIG.radius_m,
            eligible: true,
        })
    }

    #[test]
    fn a_gun_brought_down_onto_the_pouch_reloads_once_however_slowly() {
        let mut b = BeltPush::new();
        assert!(!b.step(at(0.2), &CONFIG));
        assert!(!b.step(at(0.115), &CONFIG));
        assert_eq!(b.stage, PushStage::Pushing);
        assert!(b.step(at(0.09), &CONFIG));
        assert!(b.holds_fire());
        assert!(!b.step(at(0.0), &CONFIG));
        assert!(!b.step(at(0.2), &CONFIG));
        assert!(!b.holds_fire());
    }

    #[test]
    fn a_gun_tilted_past_sixty_degrees_does_nothing_until_it_leaves() {
        let tilted = |angle: f32| {
            at(0.115).map(|i| PushInputs {
                axis: Quat::from_rotation_x(angle.to_radians()) * Vec3::Y,
                ..i
            })
        };
        let mut b = BeltPush::new();
        b.step(tilted(55.0), &CONFIG);
        assert!(b.step(at(0.0), &CONFIG));

        let mut b = BeltPush::new();
        b.step(tilted(65.0), &CONFIG);
        assert!(!b.step(at(0.0), &CONFIG));
        assert_eq!(b.stage, PushStage::Spent);
    }

    #[test]
    fn an_ineligible_gun_or_no_gun_never_reloads() {
        let mut b = BeltPush::new();
        let ineligible = |h| {
            at(h).map(|i| PushInputs {
                eligible: false,
                ..i
            })
        };
        b.step(ineligible(0.1), &CONFIG);
        assert!(!b.step(at(0.0), &CONFIG));
        assert!(!b.step(None, &CONFIG));
        assert_eq!(b.stage, PushStage::Outside);
    }

    #[test]
    fn the_magazine_base_is_pushed_while_it_is_in_and_the_mouth_once_it_is_out() {
        let mouth = Vec3::new(0.1, 1.0, -0.2);
        assert_eq!(push_point(mouth, Vec3::Y, 0.15), mouth - Vec3::Y * 0.15);
        assert_eq!(push_point(mouth, Vec3::Y, 0.0), mouth);
        assert_eq!(push_point(mouth, Vec3::Y, f32::NAN), mouth);
        assert_eq!(push_point(mouth, Vec3::Y, -1.0), mouth);
    }

    #[test]
    fn the_push_is_offered_to_an_upright_gun_near_the_pouch() {
        let near = POUCH + Vec3::Y * 0.3;
        assert!(approaching(near, Vec3::Y, POUCH, 0.12, 0.2, &CONFIG));
        assert!(!approaching(near, Vec3::Y, POUCH, 0.12, 0.1, &CONFIG));
        assert!(!approaching(POUCH, Vec3::Z, POUCH, 0.12, 0.2, &CONFIG));
    }
}
