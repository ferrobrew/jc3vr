//! The primary grip captured with its actual native placement and solved wrist.

use glam::{Mat4, Vec3};
use jc3gi::equipment::weapon::WeaponState;

use crate::controllers::{
    frame::{Hand, Pose},
    weapons::hold::world,
};

/// A placement's identity, checked again before its captured grip is used.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Identity {
    pub weapon: usize,
    pub primary_gun: usize,
    pub holder: Hand,
    pub state: WeaponState,
    pub gun_world: Mat4,
}

pub(crate) struct Sample {
    pub identity: Identity,
    pub wrist_world: Pose,
    pub grip_world: Vec3,
    pub weight: f32,
    pub pinned: bool,
}

pub(crate) struct Records {
    generation: u64,
    hands: [Option<Record>; 2],
}

impl Records {
    pub(crate) const fn new() -> Self {
        Self {
            generation: 0,
            hands: [None; 2],
        }
    }

    /// Invalidate every old sample before a placement or ownership change.
    pub(crate) fn clear(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.hands = [None; 2];
        self.generation
    }

    pub(crate) fn invalidate_hand(&mut self, generation: u64, holder: Hand) {
        if generation == self.generation {
            self.hands[holder.index()] = None;
        }
    }

    pub(crate) fn capture(&mut self, generation: u64, sample: Sample) {
        if generation != self.generation {
            return;
        }
        let hand = sample.identity.holder.index();
        self.hands[hand] = None;
        if !sample.pinned
            || !sample.weight.is_finite()
            || sample.weight < FULL_WEIGHT
            || sample.identity.weapon == 0
            || sample.identity.primary_gun == 0
            || !sample.identity.gun_world.is_finite()
        {
            return;
        }
        let Some(local_world) = world::grip_on_current_wrist(
            sample.grip_world,
            sample.wrist_world,
            Pose::IDENTITY,
            1.0,
        ) else {
            return;
        };
        self.hands[hand] = Some(Record {
            identity: sample.identity,
            local_world,
        });
    }

    pub(crate) fn point(
        &mut self,
        identity: Identity,
        current_wrist: Pose,
        world_scale: f32,
    ) -> Option<Vec3> {
        let entry = &mut self.hands[identity.holder.index()];
        let record = (*entry)?;
        if record.identity != identity {
            *entry = None;
            return None;
        }
        world::grip_on_current_wrist(
            record.local_world,
            Pose::IDENTITY,
            current_wrist,
            world_scale,
        )
    }
}

const FULL_WEIGHT: f32 = 0.999;

#[derive(Debug, Copy, Clone)]
struct Record {
    identity: Identity,
    local_world: Vec3,
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn identity(holder: Hand) -> Identity {
        Identity {
            weapon: 1,
            primary_gun: 2,
            holder,
            state: WeaponState::E_WS_WIELDING,
            gun_world: Mat4::IDENTITY,
        }
    }

    fn sample(holder: Hand) -> Sample {
        Sample {
            identity: identity(holder),
            wrist_world: Pose {
                position: Vec3::new(100.0, 2.0, -40.0),
                orientation: Quat::from_rotation_y(0.5),
            },
            grip_world: Vec3::new(100.0, 2.1, -40.0),
            weight: 1.0,
            pinned: true,
        }
    }

    #[test]
    fn ownership_without_a_placement_does_not_produce_a_measurement() {
        let mut records = Records::new();
        assert_eq!(
            records.point(identity(Hand::Right), Pose::IDENTITY, 1.0),
            None
        );
        let generation = records.clear();
        records.capture(generation, sample(Hand::Right));
        assert!(
            records
                .point(identity(Hand::Right), Pose::IDENTITY, 1.0)
                .is_some()
        );
        assert_eq!(
            records.point(identity(Hand::Left), Pose::IDENTITY, 1.0),
            None
        );
        records.clear();
        assert_eq!(
            records.point(identity(Hand::Right), Pose::IDENTITY, 1.0),
            None
        );
        // An ownership change also rejects a placement that started before that change.
        records.capture(generation, sample(Hand::Left));
        assert_eq!(
            records.point(identity(Hand::Left), Pose::IDENTITY, 1.0),
            None
        );
    }

    #[test]
    fn a_sample_must_match_the_weapon_primary_gun_state_and_native_placement() {
        let base = identity(Hand::Right);
        for changed in [
            Identity { weapon: 3, ..base },
            Identity {
                primary_gun: 4,
                ..base
            },
            Identity {
                state: WeaponState::E_WS_HOLSTERED,
                ..base
            },
            Identity {
                gun_world: Mat4::from_translation(Vec3::X),
                ..base
            },
        ] {
            let mut records = Records::new();
            let generation = records.clear();
            records.capture(generation, sample(Hand::Right));
            assert_eq!(records.point(changed, Pose::IDENTITY, 1.0), None);
            assert_eq!(records.point(base, Pose::IDENTITY, 1.0), None);
        }
    }

    #[test]
    fn capture_uses_its_solved_wrist_instead_of_an_unrelated_new_ik_target() {
        let mut records = Records::new();
        let generation = records.clear();
        let capture = sample(Hand::Right);
        let local = capture.wrist_world.orientation.inverse()
            * (capture.grip_world - capture.wrist_world.position);
        records.capture(generation, capture);
        let current = Pose {
            position: Vec3::new(0.3, 1.2, -0.6),
            orientation: Quat::from_rotation_x(0.7),
        };
        let point = records.point(identity(Hand::Right), current, 2.0).unwrap();
        assert!(point.distance(current.position + current.orientation * (local / 2.0)) < 1e-5);
    }

    #[test]
    fn each_placement_clears_missing_hands_and_invalid_capture_clears_its_hand() {
        let mut records = Records::new();
        let generation = records.clear();
        records.capture(generation, sample(Hand::Right));
        records.capture(
            generation,
            Sample {
                grip_world: Vec3::NAN,
                ..sample(Hand::Right)
            },
        );
        assert_eq!(
            records.point(identity(Hand::Right), Pose::IDENTITY, 1.0),
            None
        );
        records.capture(generation, sample(Hand::Right));
        let generation = records.clear();
        records.capture(generation, sample(Hand::Left));
        assert_eq!(
            records.point(identity(Hand::Right), Pose::IDENTITY, 1.0),
            None
        );
        assert!(
            records
                .point(identity(Hand::Left), Pose::IDENTITY, 1.0)
                .is_some()
        );
        for (weight, pinned) in [(0.5, true), (f32::NAN, true), (1.0, false)] {
            records.capture(
                generation,
                Sample {
                    weight,
                    pinned,
                    ..sample(Hand::Left)
                },
            );
            assert_eq!(
                records.point(identity(Hand::Left), Pose::IDENTITY, 1.0),
                None
            );
        }
    }
}
