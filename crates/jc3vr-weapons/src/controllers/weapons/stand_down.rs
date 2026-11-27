//! The weapon handling's stand-down: why it stands down, how much of it stops for each reason, and
//! each subsystem's part, in the order they run.
//!
//! The order matters in two places:
//!
//! - the C4 throw puts the detonator back on the chest before the detonator's model is released;
//! - the hold lets go of the attach bones before the armed weapons are disarmed onto the body, so
//!   that the game's own layout, not the mod's, is the one the disarm restores.
//!
//! No other ordering between the steps is known to matter. The tick's own state, which no step
//! reads, goes last, and the loadout table's stop, for the reasons that leave the controllers,
//! runs after all of them.

use jc3vr_body::controllers::held::guns::{self, GunHolds};
use jc3vr_core::game_tick::GameTick;

use crate::controllers::{
    loadout::contents,
    weapons::{
        ads, c4, collide, dual_wield, foregrip, grenade, grip_watch, heft, hold, hold::armed,
        inventory, pouch_models, pouch_return, recoil, regions, reload, state::STATE, wheel,
    },
};

/// Why the weapon handling stands down, which decides how much of it stops.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StandDown {
    /// The controllers are disabled in the settings.
    Disabled,
    /// The gamepad scheme is in effect.
    GamepadScheme,
    /// No controller frame, or no active controller.
    NoControllers,
    /// The hands are not active (no tracked hands to act with).
    HandsInactive,
    /// The game's device manager is disabled, so its device poll does not run.
    PollDisabled,
    /// The mod's debug UI owns the controls.
    ModPanel,
    /// The debug UI's capture of the controls cancelled the tick's routes.
    CaptureCancel,
    /// A game menu or the map is open.
    Menu,
}

/// How much of the weapon handling a [`StandDown`] stops.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StandDownScope {
    /// Skip the tick halves but keep what the hands hold.
    Pause,
    /// Stop acting and forget the tick's weapons, keeping the loadout table.
    Hands,
    /// Stop acting, and put the loadout table back into the game's slots.
    Loadout,
}

impl StandDown {
    /// How much of the weapon handling this reason stops.
    pub fn scope(self) -> StandDownScope {
        match self {
            Self::Disabled | Self::GamepadScheme => StandDownScope::Loadout,
            Self::NoControllers | Self::HandsInactive => StandDownScope::Hands,
            Self::PollDisabled | Self::ModPanel | Self::CaptureCancel | Self::Menu => {
                StandDownScope::Pause
            }
        }
    }
}

/// Stand the weapon handling down for `reason`, as far as its [`StandDown::scope`] says, running
/// each subsystem's part in order. Called on every input tick the weapon handling's tick halves do
/// not run.
pub fn stand_down(game: &GameTick<'_>, reason: StandDown) {
    for part in parts(reason.scope()) {
        match part {
            Part::Pause => pause(),
            Part::Hands => run(game, &HANDS),
            Part::Loadout => run(game, &LOADOUT),
        }
    }
}

/// A run of steps of the stand-down.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Part {
    Pause,
    Hands,
    Loadout,
}

/// The parts a stand-down of `scope` runs, in order: the loadout table stops after the hands.
const fn parts(scope: StandDownScope) -> &'static [Part] {
    match scope {
        StandDownScope::Pause => &[Part::Pause],
        StandDownScope::Hands => &[Part::Hands],
        StandDownScope::Loadout => &[Part::Hands, Part::Loadout],
    }
}

/// Pause: physical magazine work is cancelled, and the grip release stands down, so that the first
/// tick back ends a gun's hold with the grip as it is then. What the hands hold stays held: a full
/// stand-down here would take a gun out of a hand the moment the debug panel opens.
pub(crate) fn pause() {
    for step in &PAUSE {
        tracing::trace!(target: "controllers", step = step.name, "weapon handling: pause step");
        (step.run)();
    }
}

/// One subsystem's part in a stand-down: `F` is `fn()` for a pause's, which reads nothing of the
/// game, and `fn(&GameTick)` for the others.
struct Step<F> {
    /// The subsystem, for the trace log and the order's test.
    name: &'static str,
    run: F,
}

/// Run `steps` in order.
fn run(game: &GameTick<'_>, steps: &[Step<fn(&GameTick<'_>)>]) {
    for step in steps {
        tracing::trace!(target: "controllers", step = step.name, "weapon handling: stand-down step");
        (step.run)(game);
    }
}

/// The pause's steps.
const PAUSE: [Step<fn()>; 2] = [
    Step {
        name: "reload extraction",
        run: reload::cancel_extraction_on_standdown,
    },
    Step {
        name: "grip release",
        run: hold::act::cancel_releases,
    },
];

/// Stop acting and forget the tick's weapons. Every step runs in the game stage, on the game
/// thread before the world update, which is what the unsafe ones need.
const HANDS: [Step<fn(&GameTick<'_>)>; 23] = [
    Step {
        name: "foregrip",
        run: |_| foregrip::clear(),
    },
    Step {
        name: "contents",
        run: |_| contents::clear(),
    },
    Step {
        name: "gun bodies",
        run: |_| guns::publish(GunHolds::default()),
    },
    // No gun weighs on a hand, and none is held still.
    Step {
        name: "heft",
        run: |_| {
            heft::observe([None; 2]);
            heft::freeze([false; 2]);
        },
    },
    Step {
        name: "aim down sights",
        run: |_| ads::observe([None; 2]),
    },
    Step {
        name: "collision",
        run: |_| collide::observe([None; 2]),
    },
    Step {
        name: "grip watch",
        run: |_| grip_watch::clear(),
    },
    Step {
        name: "recoil",
        run: |_| recoil::clear(),
    },
    // Safe grenades are released inertly; a caught grenade's fuse runs on.
    Step {
        name: "grenades",
        run: |_| grenade::stand_down(),
    },
    Step {
        name: "pouch models",
        // SAFETY: the game stage, on the game thread before the world update.
        run: |_| unsafe { pouch_models::stand_down() },
    },
    // A held charge is taken back, and the detonator goes back on the chest.
    Step {
        name: "C4",
        run: |_| c4::stand_down(),
    },
    Step {
        name: "detonator model",
        // SAFETY: as above.
        run: |_| unsafe { c4::detonator::release_model() },
    },
    Step {
        name: "pouch return",
        run: |_| pouch_return::stand_down(),
    },
    Step {
        name: "single pistols",
        // SAFETY: as above.
        run: |_| unsafe { dual_wield::single::stand_down() },
    },
    // The mod stops owning the attach bones.
    Step {
        name: "hold",
        run: |_| hold::clear(),
    },
    Step {
        name: "hold snaps",
        run: |_| hold::act::clear(),
    },
    // The hands fire nothing.
    Step {
        name: "hand fire",
        run: |_| hold::tick::stand_down(),
    },
    Step {
        name: "take",
        run: |_| hold::take::stand_down(),
    },
    Step {
        name: "regions",
        run: |_| regions::stand_down(),
    },
    Step {
        name: "armed",
        run: armed::stand_down,
    },
    // A round in hand drops, and the weapons go back to the game.
    Step {
        name: "reload",
        run: |_| reload::stand_down(),
    },
    // An open wheel closes as a cancel.
    Step {
        name: "wheel",
        run: wheel::stand_down,
    },
    // The begin half's handoff and the tick's targets.
    Step {
        name: "tick",
        run: |_| STATE.lock().stand_down(),
    },
];

/// Put the loadout table back into the game's slots, after the hands stand down.
const LOADOUT: [Step<fn(&GameTick<'_>)>; 1] = [Step {
    name: "loadout table",
    run: inventory::stand_down,
}];

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;

    fn names(part: Part) -> Vec<&'static str> {
        match part {
            Part::Pause => PAUSE.iter().map(|step| step.name).collect(),
            Part::Hands => HANDS.iter().map(|step| step.name).collect(),
            Part::Loadout => LOADOUT.iter().map(|step| step.name).collect(),
        }
    }

    /// The steps a stand-down for `reason` runs, in order.
    fn steps_for(reason: StandDown) -> Vec<&'static str> {
        parts(reason.scope())
            .iter()
            .flat_map(|&p| names(p))
            .collect()
    }

    #[test_case(StandDown::Disabled, StandDownScope::Loadout)]
    #[test_case(StandDown::GamepadScheme, StandDownScope::Loadout)]
    #[test_case(StandDown::NoControllers, StandDownScope::Hands)]
    #[test_case(StandDown::HandsInactive, StandDownScope::Hands)]
    #[test_case(StandDown::PollDisabled, StandDownScope::Pause)]
    #[test_case(StandDown::ModPanel, StandDownScope::Pause)]
    #[test_case(StandDown::CaptureCancel, StandDownScope::Pause)]
    #[test_case(StandDown::Menu, StandDownScope::Pause)]
    fn each_reason_stops_as_much_as_its_scope(reason: StandDown, scope: StandDownScope) {
        assert_eq!(reason.scope(), scope);
    }

    #[test_case(StandDown::PollDisabled)]
    #[test_case(StandDown::ModPanel)]
    #[test_case(StandDown::CaptureCancel)]
    #[test_case(StandDown::Menu)]
    fn a_pause_cancels_the_magazine_work_and_the_release_and_keeps_what_the_hands_hold(
        reason: StandDown,
    ) {
        assert_eq!(steps_for(reason), ["reload extraction", "grip release"]);
    }

    #[test_case(StandDown::NoControllers)]
    #[test_case(StandDown::HandsInactive)]
    fn losing_the_hands_forgets_the_tick_but_keeps_the_loadout_table(reason: StandDown) {
        let steps = steps_for(reason);
        assert_eq!(steps, names(Part::Hands));
        assert!(!steps.contains(&"loadout table"));
    }

    #[test_case(StandDown::Disabled)]
    #[test_case(StandDown::GamepadScheme)]
    fn leaving_the_controllers_stops_the_loadout_table_after_the_hands(reason: StandDown) {
        let steps = steps_for(reason);
        let (hands, loadout) = steps.split_at(HANDS.len());
        assert_eq!(hands, names(Part::Hands));
        assert_eq!(loadout, ["loadout table"]);
    }

    #[test]
    fn the_steps_are_named_once_and_the_orderings_that_matter_hold() {
        let order = names(Part::Hands);
        let mut unique = order.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), order.len());
        let at = |name| order.iter().position(|n| *n == name).unwrap();
        assert!(at("C4") < at("detonator model"));
        assert!(at("hold") < at("armed"));
        assert_eq!(at("tick"), order.len() - 1);
    }
}
