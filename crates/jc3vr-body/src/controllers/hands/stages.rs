//! The hand composition's stage table: the poses applied, in order, to the active hands after the
//! controllers place them, each one contributed by the system that owns it.
//!
//! [`crate::controllers::hands::active_hands`] and the render-time late update compose the hands
//! the same way: the controllers' poses with the remap and the reach limit, then every registered
//! stage in its table's order over one [`HandComposition`]
//! ([`crate::contributions::BodyContributions::target_stages`], then
//! [`hand_stages`](crate::contributions::BodyContributions::hand_stages)). The table that fills
//! them declares the order, once: the target stages pose the hands on what they hold, and the hand
//! bodies' drive applies the same ones ([`apply_targets`]); the rest follow. The body contributes
//! two stages of its own for that table to place: [`record_sighted`], just after the sights, and
//! [`keep_physical`]. The composition's shoulders are filled before the first stage.

use glam::{Quat, Vec3};
use jc3vr_input::frame::Pose;

use crate::{contributions, controllers::hands::CockpitHand};

/// When the hands are composed for: the simulation tick, or a frame rendered between ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DrawTime {
    /// The simulation tick: the hand bodies' offsets as the last step left them.
    Tick,
    /// A rendered frame: the hand bodies' offsets blended between the last two steps, so the
    /// drawn hand does not step at the physics rate.
    Render,
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
    /// before the first stage.
    pub shoulders: [Option<Vec3>; 2],
    /// The hands as the sights left them ([`record_sighted`]).
    pub sighted: [Option<CockpitHand>; 2],
    /// The hands whose gun is a body, as the held guns' target stage found them.
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

/// Fill `composition`'s shoulders with `shoulders`, then run every registered stage over it in
/// order.
pub(crate) fn run(composition: &mut HandComposition, shoulders: [Option<Vec3>; 2]) {
    let (targets, rest) = contributions::get().map_or((&[][..], &[][..]), |contributions| {
        (contributions.target_stages, contributions.hand_stages)
    });
    run_in(targets, rest, composition, shoulders);
}

/// Give `hands` the registered target stages alone, with the head at `head`: the poses the hands
/// take on what they hold, for the hand bodies' drive. The target stages read only the hands and
/// the head's position.
pub fn apply_targets(hands: &mut [Option<CockpitHand>; 2], head: Vec3) {
    let targets = contributions::get().map_or(&[][..], |contributions| contributions.target_stages);
    let mut composition = HandComposition::new(
        *hands,
        Pose {
            position: head,
            orientation: Quat::IDENTITY,
        },
        1.0,
        Quat::IDENTITY,
        DrawTime::Tick,
    );
    for run in targets {
        run(&mut composition);
    }
    *hands = composition.hands;
}

/// Record the hands as the sights left them ([`HandComposition::sighted`]). The stage the table
/// places just after aiming down the sights.
pub fn record_sighted(composition: &mut HandComposition) {
    composition.sighted = composition.hands;
}

/// Put back each hand on a gun whose body the hands hold ([`HandComposition::physical`]) as it was
/// after the sights ([`HandComposition::sighted`]): the kinematic poses between are the gun body's
/// to give.
pub fn keep_physical(composition: &mut HandComposition) {
    for (i, kept) in composition.physical.into_iter().enumerate() {
        if kept {
            composition.hands[i] = composition.sighted[i];
        }
    }
}

fn run_in(
    targets: &[HandStageFn],
    rest: &[HandStageFn],
    composition: &mut HandComposition,
    shoulders: [Option<Vec3>; 2],
) {
    composition.shoulders = shoulders;
    for run in targets.iter().chain(rest) {
        run(composition);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        static RAN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
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
    fn the_target_stages_run_before_the_rest_in_table_order() {
        fn record(stage: usize) {
            RAN.with(|ran| ran.borrow_mut().push(stage));
        }
        let targets: [HandStageFn; 2] = [|_| record(0), |_| record(1)];
        let rest: [HandStageFn; 3] = [|_| record(2), |_| record(3), |_| record(4)];
        run_in(&targets, &rest, &mut composition(), [None; 2]);
        assert_eq!(RAN.with(|ran| ran.take()), [0, 1, 2, 3, 4]);
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
        let targets: [HandStageFn; 1] = [|c| c.physical = [true, false]];
        let rest: [HandStageFn; 4] = [
            |c| {
                assert_eq!(c.shoulders, [Some(Vec3::Y), None]);
                c.hands = [hand_at(1.0), hand_at(1.0)];
            },
            record_sighted,
            |c| c.hands = [hand_at(2.0), hand_at(2.0)],
            keep_physical,
        ];
        let mut c = composition();
        run_in(&targets, &rest, &mut c, [Some(Vec3::Y), None]);
        assert_eq!(c.sighted, [hand_at(1.0), hand_at(1.0)]);
        assert_eq!(c.hands, [hand_at(1.0), hand_at(2.0)]);
    }
}
