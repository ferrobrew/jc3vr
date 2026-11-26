//! The hand composition's stage table: the poses applied, in order, to the active hands after the
//! controllers place them, each one contributed by the system that owns it.
//!
//! [`crate::controllers::hands::active_hands`] and the render-time late update compose the hands
//! the same way: the controllers' poses with the remap and the reach limit, then every
//! [`HandStage`] in the enum's order over one [`HandComposition`]. The order is load-bearing: a
//! physical vehicle control or a mounted gun's handle takes its hand first; the shared support and
//! weight targets pose a two-handed or heavy gun; aiming down the sights lifts it to the eye; the
//! hands as the sights leave them are recorded ([`HandComposition::sighted`]); the synthetic recoil
//! and the wall probe displace a kinematic gun hand; a hand whose gun is a body gets its sighted
//! pose back, since the poses between are the gun body's to give; the hand bodies hold a pressed
//! palm at its surface and draw a body's hand at its body; and an anchored grapple hand holds the
//! line, last, over all of them.
//!
//! Each stage is registered once at install ([`register_hand_stage`]) by the layer that owns it,
//! so the hands name none of the systems above them. The registrations' order does not matter.
//! A stage nobody registered is skipped, and the install refuses to finish with one missing
//! ([`missing_hand_stages`]): the hands do real work in every stage, every frame.

use glam::{Quat, Vec3};
use jc3vr_input::frame::Pose;

use crate::controllers::hands::{CockpitHand, slots::Slots};

/// When the hands are composed for: the simulation tick, or a frame rendered between ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DrawTime {
    /// The simulation tick: the hand bodies' offsets as the last step left them.
    Tick,
    /// A rendered frame: the hand bodies' offsets blended between the last two steps, so the
    /// drawn hand does not step at the physics rate.
    Render,
}

/// One stage of the hand composition, in the order the stages run.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandStage {
    /// A hand on a physical vehicle control or a mounted gun's handle holds it.
    Vehicles,
    /// The shared support and weight targets of a held gun: the two-handed pose while the off-hand
    /// holds a foregrip, and the sag, trail, and sway of a long gun in one hand. Records which
    /// hands hold a gun body ([`HandComposition::physical`]) before posing them.
    GunTargets,
    /// Aiming down the sights. [`HandComposition::shoulders`] is filled before it and
    /// [`HandComposition::sighted`] after it.
    Ads,
    /// A kinematic gun hand's synthetic recoil offset.
    Recoil,
    /// The wall probe's displacement of a gun hand.
    Collide,
    /// A hand whose gun is a body gets its sighted pose back.
    KeepPhysical,
    /// The hand bodies: a palm pressed into a surface is held at it, and a hand body's hand is
    /// drawn at its body.
    PhysicalHands,
    /// An anchored grapple hand holds the line.
    Anchor,
}

impl HandStage {
    /// Every stage, in the order they run.
    pub const ALL: [HandStage; Self::COUNT] = [
        HandStage::Vehicles,
        HandStage::GunTargets,
        HandStage::Ads,
        HandStage::Recoil,
        HandStage::Collide,
        HandStage::KeepPhysical,
        HandStage::PhysicalHands,
        HandStage::Anchor,
    ];

    /// The number of stages; the last variant closes the count.
    const COUNT: usize = Self::Anchor as usize + 1;
}

/// What the stages read and pose: the hands, and what the composition knows of the head, the
/// torso, and the shoulders.
#[derive(Debug, Clone)]
pub struct HandComposition {
    /// The hands being posed, in the cockpit frame, by [`Hand::index`](jc3vr_input::frame::Hand::index).
    pub hands: [Option<CockpitHand>; 2],
    /// The head (the eye midpoint) in the cockpit frame.
    pub head: Pose,
    /// The VR world scale the head pose composes with.
    pub world_scale: f32,
    /// The torso's heading in the cockpit frame.
    pub torso: Quat,
    /// When the hands are composed for.
    pub at: DrawTime,
    /// Each shoulder joint of the character the hands are drawn on, in the cockpit frame; filled
    /// just before [`HandStage::Ads`].
    pub shoulders: [Option<Vec3>; 2],
    /// The hands as [`HandStage::Ads`] left them, recorded just after it.
    pub sighted: [Option<CockpitHand>; 2],
    /// The hands whose gun is a body, as [`HandStage::GunTargets`] found them.
    pub physical: [bool; 2],
}

impl HandComposition {
    pub(crate) fn new(
        hands: [Option<CockpitHand>; 2],
        head: Pose,
        world_scale: f32,
        torso: Quat,
        at: DrawTime,
    ) -> Self {
        Self {
            hands,
            head,
            world_scale,
            torso,
            at,
            shoulders: [None; 2],
            sighted: hands,
            physical: [false; 2],
        }
    }
}

/// One stage's pose of the hands.
pub type HandStageFn = fn(&mut HandComposition);

/// Register `stage`; the first registration of a stage stands.
pub fn register_hand_stage(stage: HandStage, run: HandStageFn) {
    STAGES.set(stage as usize, run, "hand stage");
}

/// The stages nobody registered, in their order.
pub fn missing_hand_stages() -> impl Iterator<Item = HandStage> {
    STAGES.missing().map(|index| HandStage::ALL[index])
}

/// Run every registered stage over `composition` in order, filling its shoulders with
/// `shoulders` just before [`HandStage::Ads`] and recording its sighted hands just after.
pub(crate) fn run(
    composition: &mut HandComposition,
    shoulders: impl FnOnce(&HandComposition) -> [Option<Vec3>; 2],
) {
    run_in(&STAGES, composition, shoulders);
}

/// Put back each hand on a gun whose body the hands hold ([`HandComposition::physical`]) as it was
/// after the sights ([`HandComposition::sighted`]): the kinematic poses between are the gun body's
/// to give. The [`HandStage::KeepPhysical`] stage.
pub fn keep_physical(composition: &mut HandComposition) {
    for (i, kept) in composition.physical.into_iter().enumerate() {
        if kept {
            composition.hands[i] = composition.sighted[i];
        }
    }
}

fn run_in(
    stages: &Slots<HandStageFn, { HandStage::COUNT }>,
    composition: &mut HandComposition,
    shoulders: impl FnOnce(&HandComposition) -> [Option<Vec3>; 2],
) {
    let mut shoulders = Some(shoulders);
    for stage in HandStage::ALL {
        if stage == HandStage::Ads
            && let Some(shoulders) = shoulders.take()
        {
            composition.shoulders = shoulders(composition);
        }
        if let Some(run) = stages.get(stage as usize) {
            run(composition);
        }
        if stage == HandStage::Ads {
            composition.sighted = composition.hands;
        }
    }
}

static STAGES: Slots<HandStageFn, { HandStage::COUNT }> = Slots::new();

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        static RAN: RefCell<Vec<HandStage>> = const { RefCell::new(Vec::new()) };
    }

    fn composition() -> HandComposition {
        HandComposition::new(
            [None; 2],
            Pose {
                position: Vec3::ZERO,
                orientation: Quat::IDENTITY,
            },
            1.0,
            Quat::IDENTITY,
            DrawTime::Tick,
        )
    }

    #[test]
    fn the_stages_run_in_the_enum_order_whatever_the_registration_order() {
        fn record(stage: HandStage) {
            RAN.with(|ran| ran.borrow_mut().push(stage));
        }
        let fns: [HandStageFn; HandStage::COUNT] = [
            |_| record(HandStage::Vehicles),
            |_| record(HandStage::GunTargets),
            |_| record(HandStage::Ads),
            |_| record(HandStage::Recoil),
            |_| record(HandStage::Collide),
            |_| record(HandStage::KeepPhysical),
            |_| record(HandStage::PhysicalHands),
            |_| record(HandStage::Anchor),
        ];
        let stages: Slots<HandStageFn, { HandStage::COUNT }> = Slots::new();
        for stage in HandStage::ALL.into_iter().rev() {
            stages.set(stage as usize, fns[stage as usize], "test");
        }
        run_in(&stages, &mut composition(), |_| [None; 2]);
        assert_eq!(RAN.with(|ran| ran.take()), HandStage::ALL);
    }

    fn hand_at(x: f32) -> Option<CockpitHand> {
        let pose = Pose {
            position: Vec3::X * x,
            orientation: Quat::IDENTITY,
        };
        Some(CockpitHand {
            wrist: pose,
            grip: pose,
            aim: pose,
        })
    }

    #[test]
    fn the_shoulders_precede_the_sights_and_a_gun_body_keeps_the_sighted_pose() {
        let stages: Slots<HandStageFn, { HandStage::COUNT }> = Slots::new();
        stages.set(
            HandStage::GunTargets as usize,
            |c| c.physical = [true, false],
            "test",
        );
        stages.set(
            HandStage::Ads as usize,
            |c| {
                assert_eq!(c.shoulders, [Some(Vec3::Y), None]);
                c.hands = [hand_at(1.0), hand_at(1.0)];
            },
            "test",
        );
        stages.set(
            HandStage::Recoil as usize,
            |c| c.hands = [hand_at(2.0), hand_at(2.0)],
            "test",
        );
        stages.set(HandStage::KeepPhysical as usize, keep_physical, "test");
        let mut c = composition();
        run_in(&stages, &mut c, |c| {
            assert_eq!(c.hands, [None; 2]);
            [Some(Vec3::Y), None]
        });
        assert_eq!(c.sighted, [hand_at(1.0), hand_at(1.0)]);
        assert_eq!(c.hands, [hand_at(1.0), hand_at(2.0)]);
    }

    #[test]
    fn the_stage_list_is_the_enum_in_order() {
        for (index, stage) in HandStage::ALL.into_iter().enumerate() {
            assert_eq!(stage as usize, index);
        }
    }
}
