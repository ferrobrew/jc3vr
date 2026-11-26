//! Local access to the runtime weight response for the weight's regression tests.

use glam::Vec3;
use jc3vr_body::controllers::hands::CockpitHand;

use crate::controllers::weapons::{
    heft::{HandHeft, HandInputs, Heft, HeftClass, pose::Weight},
    recoil::Motion,
};

#[derive(Default)]
pub(super) struct TestWeight {
    hand: Option<HandHeft>,
}

impl TestWeight {
    pub(super) fn apply(
        &mut self,
        hand: CockpitHand,
        class: HeftClass,
        unbraced: f32,
        config: &Heft,
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
