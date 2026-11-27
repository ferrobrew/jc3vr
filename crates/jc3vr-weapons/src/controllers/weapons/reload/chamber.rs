//! The chamber and the action that loads it: the slide, the charging handle, the bolt, or the
//! pump. Pure.
//!
//! The game fires from a native loaded-round total. The reload partitions that total into a
//! weapon-owned chamber and magazine-owned rounds in `ammo`. Feeding transfers a magazine round
//! to the chamber without changing the native total; a shot or live-round extraction consumes it.
//! Removing the magazine leaves a chambered round with the weapon, able to fire once without a
//! magazine. A dry gun, or a manually cycled bolt or pump after a shot, needs the action worked.
//!
//! The action worked by hand is a part that follows the hand through its travel
//! ([`crate::controllers::weapons::reload::travel`]). Every plan the reload makes is cocked
//! ([`ReloadPlan::cocked`](crate::controllers::weapons::reload::plan::ReloadPlan::cocked)), so no
//! live gun's action is worked by hand, unless the hand-worked action switch
//! ([`ReloadConfig::hand_worked_action`](crate::controllers::weapons::reload::config::ReloadConfig::hand_worked_action))
//! is on.

use glam::Vec3;
use jc3vr_body::controllers::grab::{GrabVolume, GripTarget, target::TargetReach};
use jc3vr_input::frame::Hand;

/// The mod's chamber.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum Chamber {
    #[default]
    Chambered,
    Empty,
}

/// How a gun's action is worked.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// A pistol's slide.
    Slide,
    /// A charging handle on the receiver.
    ChargingHandle,
    /// A bolt-action rifle's bolt.
    Bolt,
    /// A pump-action shotgun's pump, on the foregrip.
    Pump,
    /// Nothing to work: a revolver, a launcher, or a break-action.
    None,
}

impl ActionKind {
    pub const fn label(self) -> &'static str {
        match self {
            ActionKind::Slide => "slide",
            ActionKind::ChargingHandle => "charging handle",
            ActionKind::Bolt => "bolt",
            ActionKind::Pump => "pump",
            ActionKind::None => "none",
        }
    }

    /// Whether a ready action supports deliberate physical extraction and feeding.
    pub(crate) const fn ready_rack(self) -> bool {
        matches!(
            self,
            ActionKind::Slide | ActionKind::Bolt | ActionKind::Pump
        )
    }

    /// Whether the action cycles by hand after every shot.
    pub const fn manual(self) -> bool {
        matches!(self, ActionKind::Bolt | ActionKind::Pump)
    }
}

/// The chamber after a shot that left `magazine` rounds, for a gun worked by `action`: empty once
/// the gun runs dry, and after every shot of a bolt or a pump, which are worked by hand in place of
/// the game's own cycling.
pub(crate) fn after_shot(magazine: i32, action: ActionKind) -> Chamber {
    if action == ActionKind::None {
        return Chamber::Chambered;
    }
    if magazine <= 0 || action.manual() {
        Chamber::Empty
    } else {
        Chamber::Chambered
    }
}

/// The chamber after the action is worked with `magazine` rounds in a magazine that is in.
pub(crate) fn after_work(magazine: i32, magazine_in: bool) -> Chamber {
    if magazine_in && magazine > 0 {
        Chamber::Chambered
    } else {
        Chamber::Empty
    }
}

/// Rank the action's handle first for each hand whose other hand's gun waits on it (`waiting`, by
/// the working hand's [`Hand::index`]: its chamber is empty and its magazine feeds), while that
/// hand's grip (`grips`) is within the handle's reach and the hand holds nothing: the handle is
/// offered to it as held, so it wins over the foregrip, the gun's grip, and the inserted magazine,
/// which a closed hand sliding along the gun would otherwise take first.
pub(super) fn prefer_waiting_action(
    targets: &mut [GripTarget],
    grips: [Option<Vec3>; 2],
    waiting: [bool; 2],
) {
    for hand in Hand::BOTH {
        let Some(grip) = grips[hand.index()].filter(|_| waiting[hand.index()]) else {
            continue;
        };
        if targets.iter().any(|t| t.held_by == Some(hand)) {
            continue;
        }
        for target in targets
            .iter_mut()
            .filter(|t| t.volume == GrabVolume::WeaponAction && t.hand == Some(hand))
        {
            if let TargetReach::Sphere { centre, radius } = target.reach
                && grip.distance(centre) <= radius
            {
                *target = target.held(hand);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::grab::{Mark, MarkAt, resolve::resolve};

    use super::*;
    use crate::controllers::weapons::reload::tuning::{ActionTuning, RELOAD};

    fn config() -> &'static ActionTuning {
        &RELOAD.get().action
    }

    fn sphere(volume: GrabVolume, centre: Vec3, radius: f32) -> GripTarget {
        GripTarget::sphere(
            volume,
            Some(Hand::Left),
            centre,
            radius,
            Mark::ring(MarkAt::Cockpit(centre), 0.02),
        )
    }

    /// The action's handle and, nearer the hand, the foregrip, the gun's grip, and the inserted
    /// magazine, for the left hand at the origin.
    fn gun_parts() -> Vec<GripTarget> {
        vec![
            sphere(GrabVolume::Foregrip, Vec3::X * 0.02, 0.1),
            sphere(GrabVolume::Handle, Vec3::X * 0.03, 0.14),
            sphere(GrabVolume::InsertedMagazine, Vec3::X * 0.04, 0.08),
            sphere(
                GrabVolume::WeaponAction,
                Vec3::X * 0.09,
                config().grab_radius_m,
            ),
        ]
    }

    fn chosen(targets: &[GripTarget]) -> Option<GrabVolume> {
        resolve(targets, [Some(Vec3::ZERO), None])[0]
            .chosen
            .map(|r| r.volume)
    }

    #[test]
    fn the_handle_of_a_gun_waiting_on_its_action_comes_first_within_its_reach() {
        // A closed hand 9 cm from the handle, nearer the other parts, which takes the foregrip
        // unless the handle's reach covers it.
        assert!(config().grab_radius_m >= 0.1);
        let mut waiting = gun_parts();
        prefer_waiting_action(&mut waiting, [Some(Vec3::ZERO), None], [true, false]);
        assert_eq!(chosen(&waiting), Some(GrabVolume::WeaponAction));
        // A chambered gun keeps the nearest part.
        let mut loaded = gun_parts();
        prefer_waiting_action(&mut loaded, [Some(Vec3::ZERO), None], [false, false]);
        assert_eq!(chosen(&loaded), Some(GrabVolume::Foregrip));
        // Out of the handle's reach, nothing changes.
        let mut far = gun_parts();
        let grip = Vec3::X * -0.05;
        prefer_waiting_action(&mut far, [Some(grip), None], [true, false]);
        assert_eq!(far, {
            let mut parts = gun_parts();
            prefer_waiting_action(&mut parts, [Some(grip), None], [false, false]);
            parts
        });
    }

    #[test]
    fn a_hand_holding_something_keeps_it_over_the_waiting_handle() {
        let mut targets = gun_parts();
        targets[0] = targets[0].held(Hand::Left);
        prefer_waiting_action(&mut targets, [Some(Vec3::ZERO), None], [true, false]);
        assert_eq!(chosen(&targets), Some(GrabVolume::Foregrip));
    }

    #[test]
    fn a_dry_gun_empties_its_chamber_and_a_manual_action_after_every_shot() {
        assert_eq!(after_shot(0, ActionKind::Slide), Chamber::Empty);
        assert_eq!(after_shot(5, ActionKind::Slide), Chamber::Chambered);
        assert_eq!(after_shot(5, ActionKind::Bolt), Chamber::Empty);
        assert_eq!(after_shot(0, ActionKind::None), Chamber::Chambered);
        assert_eq!(after_work(5, true), Chamber::Chambered);
        assert_eq!(after_work(0, true), Chamber::Empty);
        assert_eq!(after_work(5, false), Chamber::Empty);
    }
}
