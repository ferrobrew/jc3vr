//! What the bindings stop when they do not inject, one set of steps per reason.
//!
//! A full stand-down (the controllers disabled, an idle tick, the debug panel's capture cancelling
//! the routes) undoes the bindings' writes, forgets their holds and latches, and stands down every
//! subsystem whose acts the tick drives. A partial one keeps the holds: an action set that is still
//! switching pauses the writes until the next OpenXR sample, and a game menu or the map pauses the
//! acts so that what the hands hold is still held when gameplay resumes. Each set runs in the
//! order it lists.

use jc3vr_body::controllers::{body::firm_ragdoll, grab, intent::hold::DevicePoll};
use jc3vr_core::game_tick::GameTick;
use jc3vr_grapple::{controllers::snatch, grapple};
use jc3vr_physical::controllers::physical_hands;
use jc3vr_vehicles::controllers::vehicles;
use jc3vr_view::vr::input::semantic::{frame as semantic_frame, history};
use jc3vr_weapons::controllers::{weapons, weapons::StandDown};

use crate::controllers::bindings::{
    INPUT_POLICY, Idle, PANEL_RECOVERY, State,
    pointer::{clear_controller, update_egui_controller},
    policy_a::runtime::TickInput,
    rail,
    settings::Settings,
    stage::Segment,
};

/// Why the bindings do not inject on a tick, as far as what they stop depends on it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Reason {
    /// The controllers are disabled in the settings.
    Disabled,
    /// The bindings are idle ([`Idle`]).
    Idle(Idle),
    /// The debug panel's capture of the controls cancelled the tick's routes.
    CaptureCancel,
    /// The controller frame or the semantic frame is still in another action set than the mode's.
    ActionSetPending,
    /// A game menu or the map is open: the finish half's acts pause.
    Menu,
}

/// One thing a stand-down stops.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// The policy cancels the gameplay publications taken since the last tick.
    CancelPolicy,
    /// The policy steps with no frame and no gameplay, consuming the publications.
    IdlePolicy,
    /// The panel pointer's recovery waits for the controls to rest again.
    SuspendPanelRecovery,
    /// The semantic frame and its edges are taken and dropped, so they are not replayed later.
    DropSemanticFrame,
    /// The game pointer lets go.
    ClearPointer,
    /// The panel pointer lets go.
    ClearPanelPointer,
    /// The bindings forget every hold and latch and undo the last tick's writes
    /// ([`State::stand_down`]).
    ReleaseWrites,
    /// The bindings pause their holds: the poll releases the effectors, and only the clicks are
    /// undone.
    PauseWrites,
    /// Each hand's recovery waits for the frame to come back.
    SuspendRecovery,
    /// Each hand's recovery waits for its controls to rest.
    DisarmRecovery,
    /// Each hand's recovery is published, and a long wait logged.
    PublishRecovery,
    /// The wingsuit gestures forget their pose history.
    ResetFlight,
    /// The helicopter rail's release forgets its lateral movement.
    ResetRail,
    /// The held guns' fire input is cleared.
    ClearFire,
    /// The weapon handling stands down, as far as its reason's scope says
    /// ([`StandDown::scope`]).
    Weapons(StandDown),
    /// The loadout's maintenance runs without the hands.
    MaintainInventory,
    /// The weapon wheel pauses.
    PauseWheel,
    /// The physical hands' pickups pause.
    PausePickup,
    Vehicles,
    /// The mounted gun lets the player go as its user.
    Mounted,
    /// The physical grapple lets go of its line.
    PhysicalGrapple,
    /// The seat grapple ends its hook.
    SeatGrapple,
    /// The firm ragdoll lets go.
    FirmRagdoll,
    /// The snatch ends its line and lets go of its target.
    Snatch,
    /// The grip model forgets the tick's targets.
    Grab,
    /// The physical hands stop publishing their held bodies.
    HeldBodies,
}

impl Reason {
    /// What a stand-down for this reason stops, in order.
    pub(super) const fn steps(self) -> &'static [Step] {
        use Step::*;
        match self {
            Self::Disabled => &[
                ClearPointer,
                ClearPanelPointer,
                ReleaseWrites,
                SuspendRecovery,
                PublishRecovery,
                DropSemanticFrame,
                IdlePolicy,
                Weapons(StandDown::Disabled),
                Vehicles,
                Mounted,
                PhysicalGrapple,
                SeatGrapple,
                Snatch,
                Grab,
            ],
            Self::Idle(Idle::ModPanel) => &IDLE_MOD_PANEL,
            Self::Idle(Idle::PollDisabled) => &IDLE_POLL_DISABLED,
            Self::Idle(Idle::NoControllers) => &IDLE_NO_CONTROLLERS,
            // Leaving the controllers puts the loadout table back into the game's slots, and ends a
            // hook flying from the seat, which a pause keeps for the next tick to tether.
            Self::Idle(Idle::GamepadScheme) => &[
                ClearPointer,
                ReleaseWrites,
                SuspendRecovery,
                PublishRecovery,
                Weapons(StandDown::GamepadScheme),
                SeatGrapple,
                Vehicles,
                Mounted,
                PhysicalGrapple,
                FirmRagdoll,
                Snatch,
                Grab,
            ],
            // The panel opening pauses the weapons rather than standing them down, and keeps a
            // hook flying from the seat for the next tick to tether.
            Self::CaptureCancel => &[
                CancelPolicy,
                SuspendPanelRecovery,
                DropSemanticFrame,
                ClearPointer,
                ClearPanelPointer,
                ReleaseWrites,
                DisarmRecovery,
                PublishRecovery,
                Weapons(StandDown::CaptureCancel),
                Vehicles,
                Mounted,
                PhysicalGrapple,
                Snatch,
                Grab,
            ],
            Self::ActionSetPending => &[
                Snatch,
                ResetFlight,
                ClearPointer,
                ClearFire,
                PauseWrites,
                PublishRecovery,
            ],
            Self::Menu => &[
                ResetRail,
                ClearFire,
                PauseWheel,
                PausePickup,
                Snatch,
                Mounted,
                HeldBodies,
            ],
        }
    }
}

/// What a stand-down acts on.
pub(super) struct Context<'a, 'g> {
    pub(super) game: &'a GameTick<'g>,
    /// The device poll the released writes are undone for.
    pub(super) poll: DevicePoll,
    pub(super) settings: &'a Settings,
    pub(super) state: &'a mut State,
    pub(super) sink: &'a mut Vec<Segment>,
}

/// Stand down for `reason`.
pub(super) fn run(reason: Reason, cx: &mut Context<'_, '_>) {
    for &step in reason.steps() {
        apply(step, cx);
    }
}

const IDLE_MOD_PANEL: [Step; 12] = idle(StandDown::ModPanel);
const IDLE_POLL_DISABLED: [Step; 12] = idle(StandDown::PollDisabled);
const IDLE_NO_CONTROLLERS: [Step; 12] = idle(StandDown::NoControllers);

/// An idle tick's set: the debug panel and a disabled poll pause the weapons, as a game menu does,
/// so what the hands hold stays held; a lost controller stands them down. Either way the loadout
/// table stays in place and keeps its maintenance.
const fn idle(weapons: StandDown) -> [Step; 12] {
    use Step::*;
    [
        ClearPointer,
        ReleaseWrites,
        SuspendRecovery,
        PublishRecovery,
        Weapons(weapons),
        MaintainInventory,
        Vehicles,
        Mounted,
        PhysicalGrapple,
        FirmRagdoll,
        Snatch,
        Grab,
    ]
}

fn apply(step: Step, cx: &mut Context<'_, '_>) {
    let game = cx.game;
    match step {
        Step::CancelPolicy => {
            let publications = history::take_gameplay();
            INPUT_POLICY.lock().cancel_publications(publications);
        }
        Step::IdlePolicy => {
            let publications = history::take_gameplay();
            INPUT_POLICY.lock().step(TickInput {
                publications,
                frame: None,
                context: 0,
                gameplay: false,
                usable: false,
                config: &cx.settings.bindings,
                dominant: cx.settings.acting,
            });
        }
        Step::SuspendPanelRecovery => PANEL_RECOVERY.lock().suspend(),
        Step::DropSemanticFrame => {
            semantic_frame::take();
        }
        Step::ClearPointer => clear_controller(),
        Step::ClearPanelPointer => {
            update_egui_controller(None, cx.settings.acting, Default::default());
        }
        Step::ReleaseWrites => cx.state.stand_down(cx.poll, cx.sink),
        Step::PauseWrites => cx.sink.push(Segment {
            writes: cx.state.tracker.pause(),
            tracked: false,
        }),
        Step::SuspendRecovery => cx.state.recovery.suspend(),
        Step::DisarmRecovery => cx.state.recovery.disarm(),
        Step::PublishRecovery => cx.state.publish_recovery(),
        Step::ResetFlight => {
            if let Some(flight) = &mut cx.state.flight {
                flight.reset();
            }
        }
        Step::ResetRail => cx.state.rail = rail::RailRelease::new(),
        Step::ClearFire => weapons::hold::tick::publish_fire([false; 2]),
        Step::Weapons(reason) => weapons::stand_down(game, reason),
        Step::MaintainInventory => weapons::inventory::maintenance_tick(game, cx.settings.acting),
        Step::PauseWheel => weapons::wheel::pause(game),
        Step::PausePickup => physical_hands::pickup::pause(),
        Step::Vehicles => vehicles::stand_down(),
        // SAFETY: the bindings stand down in the game stage, on the game thread before the world
        // update.
        Step::Mounted => unsafe { vehicles::mounted::stand_down() },
        Step::PhysicalGrapple => grapple::physical::stand_down(),
        Step::SeatGrapple => grapple::seat::stand_down(game),
        Step::FirmRagdoll => firm_ragdoll::input_tick(game, firm_ragdoll::Want::Off),
        Step::Snatch => snatch::stand_down(),
        Step::Grab => grab::tick::stand_down(),
        Step::HeldBodies => physical_hands::lifetime::clear(),
    }
}

#[cfg(test)]
mod tests {
    use jc3vr_weapons::controllers::weapons::StandDownScope;
    use test_case::test_case;

    use super::*;

    const FULL: [Reason; 6] = [
        Reason::Disabled,
        Reason::Idle(Idle::GamepadScheme),
        Reason::Idle(Idle::NoControllers),
        Reason::Idle(Idle::PollDisabled),
        Reason::Idle(Idle::ModPanel),
        Reason::CaptureCancel,
    ];

    const ALL: [Reason; 8] = [
        Reason::Disabled,
        Reason::Idle(Idle::GamepadScheme),
        Reason::Idle(Idle::NoControllers),
        Reason::Idle(Idle::PollDisabled),
        Reason::Idle(Idle::ModPanel),
        Reason::CaptureCancel,
        Reason::ActionSetPending,
        Reason::Menu,
    ];

    fn position(reason: Reason, step: Step) -> Option<usize> {
        reason.steps().iter().position(|s| *s == step)
    }

    fn weapons(reason: Reason) -> Option<StandDown> {
        reason.steps().iter().find_map(|step| match step {
            Step::Weapons(weapons) => Some(*weapons),
            _ => None,
        })
    }

    /// A full stand-down undoes the writes and stands down every subsystem the tick drives,
    /// the mounted gun among them, so none keeps acting for a player the bindings let go of.
    #[test_case(Reason::Disabled)]
    #[test_case(Reason::Idle(Idle::GamepadScheme))]
    #[test_case(Reason::Idle(Idle::NoControllers))]
    #[test_case(Reason::Idle(Idle::PollDisabled))]
    #[test_case(Reason::Idle(Idle::ModPanel))]
    #[test_case(Reason::CaptureCancel)]
    fn a_full_stand_down_stops_every_subsystem(reason: Reason) {
        for step in [
            Step::ClearPointer,
            Step::ReleaseWrites,
            Step::PublishRecovery,
            Step::Vehicles,
            Step::Mounted,
            Step::PhysicalGrapple,
            Step::Snatch,
            Step::Grab,
        ] {
            assert!(position(reason, step).is_some(), "{reason:?} {step:?}");
        }
        assert!(weapons(reason).is_some(), "{reason:?}");
        assert_eq!(position(reason, Step::PauseWrites), None, "{reason:?}");
    }

    /// A partial stand-down keeps the holds, and clears the held guns' cached fire.
    #[test]
    fn a_partial_stand_down_keeps_the_holds_and_clears_the_fire() {
        for reason in [Reason::ActionSetPending, Reason::Menu] {
            assert_eq!(position(reason, Step::ReleaseWrites), None, "{reason:?}");
            assert_eq!(weapons(reason), None, "{reason:?}");
            assert!(position(reason, Step::ClearFire).is_some(), "{reason:?}");
            assert!(position(reason, Step::Snatch).is_some(), "{reason:?}");
        }
        assert!(position(Reason::ActionSetPending, Step::PauseWrites).is_some());
        assert!(position(Reason::ActionSetPending, Step::ClearPointer).is_some());
        assert!(position(Reason::Menu, Step::PauseWheel).is_some());
    }

    /// The recovery is published after the writes are released or paused, which resets it.
    #[test]
    fn the_recovery_is_published_after_the_writes() {
        for reason in ALL {
            let Some(publish) = position(reason, Step::PublishRecovery) else {
                continue;
            };
            let writes = position(reason, Step::ReleaseWrites)
                .or(position(reason, Step::PauseWrites))
                .unwrap();
            assert!(writes < publish, "{reason:?}");
        }
    }

    /// The debug panel, a disabled poll, and the capture pause the weapons; a lost controller
    /// stands them down; leaving the controllers also ends the seat's hook.
    #[test]
    fn the_weapons_stand_down_as_far_as_the_reason_says() {
        assert_eq!(
            FULL.map(|reason| weapons(reason).map(StandDown::scope)),
            [
                Some(StandDownScope::Loadout),
                Some(StandDownScope::Loadout),
                Some(StandDownScope::Hands),
                Some(StandDownScope::Pause),
                Some(StandDownScope::Pause),
                Some(StandDownScope::Pause),
            ]
        );
        for reason in ALL {
            let seat = position(reason, Step::SeatGrapple).is_some();
            assert_eq!(
                seat,
                matches!(reason, Reason::Disabled | Reason::Idle(Idle::GamepadScheme)),
                "{reason:?}"
            );
        }
    }

    /// The full stand-downs stop the subsystems in one order.
    #[test]
    fn the_subsystems_stand_down_in_one_order() {
        let order = [
            Step::Weapons(StandDown::Menu),
            Step::MaintainInventory,
            Step::Vehicles,
            Step::Mounted,
            Step::PhysicalGrapple,
            Step::FirmRagdoll,
            Step::Snatch,
            Step::Grab,
        ];
        let rank = |step: &Step| {
            order.iter().position(|o| match (o, step) {
                (Step::Weapons(_), Step::Weapons(_)) => true,
                (o, step) => o == step,
            })
        };
        for reason in FULL {
            let ranks: Vec<_> = reason.steps().iter().filter_map(rank).collect();
            assert!(ranks.is_sorted(), "{reason:?}: {:?}", reason.steps());
        }
    }

    /// The exact sets, pinned: a change of membership or order is a behaviour change to check in
    /// the game.
    #[test]
    fn the_sets_are_pinned() {
        use Step::*;
        assert_eq!(
            Reason::Disabled.steps(),
            [
                ClearPointer,
                ClearPanelPointer,
                ReleaseWrites,
                SuspendRecovery,
                PublishRecovery,
                DropSemanticFrame,
                IdlePolicy,
                Weapons(StandDown::Disabled),
                Vehicles,
                Mounted,
                PhysicalGrapple,
                SeatGrapple,
                Snatch,
                Grab,
            ]
        );
        assert_eq!(
            Reason::Idle(Idle::NoControllers).steps(),
            [
                ClearPointer,
                ReleaseWrites,
                SuspendRecovery,
                PublishRecovery,
                Weapons(StandDown::NoControllers),
                MaintainInventory,
                Vehicles,
                Mounted,
                PhysicalGrapple,
                FirmRagdoll,
                Snatch,
                Grab,
            ]
        );
        assert_eq!(
            Reason::Idle(Idle::GamepadScheme).steps(),
            [
                ClearPointer,
                ReleaseWrites,
                SuspendRecovery,
                PublishRecovery,
                Weapons(StandDown::GamepadScheme),
                SeatGrapple,
                Vehicles,
                Mounted,
                PhysicalGrapple,
                FirmRagdoll,
                Snatch,
                Grab,
            ]
        );
        assert_eq!(
            Reason::CaptureCancel.steps(),
            [
                CancelPolicy,
                SuspendPanelRecovery,
                DropSemanticFrame,
                ClearPointer,
                ClearPanelPointer,
                ReleaseWrites,
                DisarmRecovery,
                PublishRecovery,
                Weapons(StandDown::CaptureCancel),
                Vehicles,
                Mounted,
                PhysicalGrapple,
                Snatch,
                Grab,
            ]
        );
        assert_eq!(
            Reason::ActionSetPending.steps(),
            [
                Snatch,
                ResetFlight,
                ClearPointer,
                ClearFire,
                PauseWrites,
                PublishRecovery
            ]
        );
        assert_eq!(
            Reason::Menu.steps(),
            [
                ResetRail,
                ClearFire,
                PauseWheel,
                PausePickup,
                Snatch,
                Mounted,
                HeldBodies
            ]
        );
    }
}
