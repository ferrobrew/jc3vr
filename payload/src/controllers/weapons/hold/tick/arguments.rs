//! The arguments used by both native armed-weapon update phases.

use std::time::{Duration, Instant};

use glam::Vec3;
use jc3gi::{equipment::weapon::WeaponInputs, types::math::Vector3};

pub(super) const MAX_TARGET_AGE: Duration = Duration::from_millis(250);

#[derive(Copy, Clone)]
pub(super) struct HandTarget {
    pub owner: usize,
    pub weapon: usize,
    pub point: Vec3,
    pub at: Instant,
}

pub(super) struct UpdateFacts {
    pub owner: usize,
    pub weapon: usize,
    pub hand_target: Option<HandTarget>,
    pub native_target: Option<Vec3>,
    pub now: Instant,
    pub loaded: bool,
    pub fire: bool,
    pub reload_held: bool,
    pub animation_allows: bool,
}

pub(super) enum UpdateArguments {
    Tick {
        inputs: WeaponInputs,
        aim: Vector3,
    },
    /// No current target can be passed safely. Release mapped input state without an aim update.
    Stop,
}

pub(super) fn update_arguments(facts: UpdateFacts) -> UpdateArguments {
    let target = facts
        .hand_target
        .filter(|target| {
            target.owner == facts.owner
                && target.weapon == facts.weapon
                && target.point.is_finite()
                && facts.now.saturating_duration_since(target.at) <= MAX_TARGET_AGE
        })
        .map(|target| target.point)
        .or(facts.native_target.filter(|target| target.is_finite()));
    let Some(target) = target else {
        return UpdateArguments::Stop;
    };
    let fire = facts.loaded && facts.fire && !facts.reload_held && facts.animation_allows;
    UpdateArguments::Tick {
        inputs: WeaponInputs {
            m_PrimaryFireInput: if fire { 1.0 } else { 0.0 },
            m_SecondaryFireInput: 0.0,
        },
        aim: Vector3 {
            data: target.to_array(),
        },
    }
}

#[cfg(test)]
mod tests;
