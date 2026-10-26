//! Local access to the runtime weight response for physical-target regression tests.

use glam::Vec3;

use crate::controllers::{
    hands::CockpitHand,
    weapons::{
        config::HeftConfig,
        heft::{HandHeft, HandInputs, HeftClass, pose::Weight},
        recoil::Motion,
    },
};

#[derive(Default)]
pub(crate) struct TestWeight {
    hand: Option<HandHeft>,
}

impl TestWeight {
    pub(crate) fn apply(
        &mut self,
        hand: CockpitHand,
        class: HeftClass,
        unbraced: f32,
        config: &HeftConfig,
        time_s: f32,
    ) -> CockpitHand {
        let state = self.hand.get_or_insert(HandHeft::IDLE);
        let (rotation, _) = state.weigh(&HandInputs {
            hand: &hand,
            head: Vec3::ZERO,
            weight: &Weight::of(class, config, unbraced),
            config,
            dt: 1.0 / 90.0,
            t: time_s,
        });
        Motion {
            pivot: hand.grip.position,
            rotation,
            translation: Vec3::ZERO,
        }
        .hand(hand)
    }
}
