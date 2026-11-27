//! The arguments used by both native armed-weapon update phases.

use glam::Vec3;
use jc3gi::{equipment::weapon::WeaponInputs, types::math::Vector3};
use jc3vr_core::published::{Stamp, Window};

use crate::controllers::weapons::hold::tuning::HOLD;

#[derive(Copy, Clone)]
pub(super) struct HandTarget {
    pub owner: usize,
    pub weapon: usize,
    pub point: Vec3,
    pub at: Stamp,
}

pub(super) struct UpdateFacts {
    pub owner: usize,
    pub weapon: usize,
    pub hand_target: Option<HandTarget>,
    pub(crate) native_target: Option<Vec3>,
    pub now: Stamp,
    pub fire: bool,
    pub(crate) reload_held: bool,
    pub(crate) animation_allows: bool,
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
    let window = Window::new(HOLD.get().hold.target_max_age_ms.duration());
    let target = facts
        .hand_target
        .filter(|target| {
            target.owner == facts.owner
                && target.weapon == facts.weapon
                && target.point.is_finite()
                && window.holds(target.at, facts.now)
        })
        .map(|target| target.point)
        .or(facts.native_target.filter(|target| target.is_finite()));
    let Some(target) = target else {
        return UpdateArguments::Stop;
    };
    let fire = facts.fire && !facts.reload_held && facts.animation_allows;
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
