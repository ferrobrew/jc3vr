//! The tables' evaluation and the subsystems' acts on it.

use std::time::Instant;

use jc3gi::input::input_action_map::Action;
use jc3vr_body::controllers::{
    aim::sights,
    body::{firm_ragdoll, rig::grounded::add_step_walk},
    body_frame, facts, grab,
    grab::{Consumer, GrabVolume, grabs_for, markers::MarkerFrame},
    hands::ActiveHands,
    intent::{
        controls::ButtonControl,
        finish::FinishInputs,
        hold::{Desired, MapKind, Target, Want},
    },
    locomotion,
    mode::{AirborneVariant, Detection, Mode, Traversal},
    roomscale,
};
use jc3vr_grapple::{
    controllers::{snatch, snatch::config::SnatchSettings},
    grapple,
    grapple::physical::lock::LockTickInput,
};
use jc3vr_input::{frame::Hand, semantic::vocabulary::OnFootAction};
use jc3vr_physical::controllers::{
    physical_hands, physical_hands::pickup::diagnostics::InputContext,
};
use jc3vr_vehicles::{
    vehicles,
    vehicles::wingsuit_gestures::{GestureCommands, SuitTransition},
};
use jc3vr_view::{
    controllers::cockpit::CockpitTransform,
    vr::input::{flight_pose, sources},
};
use jc3vr_weapons::controllers::{loadout::contents, weapons, weapons::reload::BeltEligibility};

use crate::controllers::bindings::{
    buttons::reload_button,
    flight, gun_fire,
    intents::SemanticInput,
    rail,
    settings::Settings,
    stand_down::Reason,
    summary,
    tables::Step,
    tick::{Hands, Live, input::Routed},
    trigger_ownership,
    trigger_ownership::Owner as TriggerOwner,
};

/// The tables' evaluation, and what the acts and the summary read of it.
pub(super) struct Evaluated {
    pub(super) step: Step,
    flight_commands: GestureCommands,
    flight: FlightRead,
    interaction_context: InputContext,
    /// The summary's reads from before the acts.
    pub(super) summary_before: summary::BeforeFinish,
}

/// Evaluate the tables, and run what acts on the evaluation before the subsystems' finish halves:
/// the hang under a helicopter, the grapple lock, the rail, the firm ragdoll, the seat grapple,
/// the controls summary's hold, the wingsuit gestures' evaluation, and the grip model's finish.
pub(super) fn evaluate(live: &mut Live<'_, '_>, routed: &Routed, hands: &Hands) -> Evaluated {
    let game = live.game;
    let detection = live.detection;
    let grapple_hand = live.grapple_hand;
    let controls = &hands.controls;
    let semantic_input = &hands.offers.semantic_input;
    let grapple_held = hands.offers.grapple_held;
    let snatch_button_owned = hands.grapple.snatch_button_owned;
    let mut step = live
        .state
        .bindings
        .step(detection.mode, controls, semantic_input, live.real_dt);
    let enter_from_hang = live.state.heli_hang.step(&mut step.desired);
    grapple::physical::lock::lock_tick(
        game,
        &LockTickInput {
            button: step.grapple_lock,
            carries: detection.traversal == Some(Traversal::ReelingIn),
            yank: hands.grapple.physical.lock_yank,
            jump: jump_pressed(semantic_input),
            enter: enter_from_hang,
            line: hands.grapple.grapple_line,
            free_hand: grapple::line_grab::lock_hand(
                grapple_hand,
                live.state.bindings.grip_latches()[grapple_hand.other().index()],
            ),
            deployed: matches!(
                detection.mode,
                Mode::Airborne(AirborneVariant::Wingsuit | AirborneVariant::Parachute)
            ),
            real_dt: live.real_dt,
            hand: grapple_hand,
        },
    );
    // The helicopter's skid: standing on it, and the jump off it.
    vehicles::rail::input_tick(
        game,
        jump_pressed(semantic_input),
        matches!(detection.mode, Mode::OnFoot(_))
            && detection.traversal != Some(Traversal::ReelingIn),
    );
    // The firm ragdoll for the line the lock holds and for the canopy.
    let firm = if facts::lock::holds() {
        firm_ragdoll::Want::Line
    } else if detection.mode == Mode::Airborne(AirborneVariant::Parachute) {
        firm_ragdoll::Want::Canopy
    } else {
        firm_ragdoll::Want::Off
    };
    firm_ragdoll::input_tick(game, firm);
    grapple::seat::tick(
        game,
        &grapple::seat::SeatTickInput {
            seated: controls.grapple.seated,
            fire: step.grapple_seat_fire,
            held: grapple_held && !snatch_button_owned,
            release_all: step.desired.iter().any(|d| {
                matches!(
                    d.target.action,
                    Action::RELEASE_GRAPPLE | Action::VEHICLE_RELEASE_GRAPPLE
                ) && d.want == Want::Click
            }),
            target: body_frame::aim()
                .grapple
                .filter(|g| g.seat && g.valid)
                .and_then(|g| grapple::seat::Target::capture(game, g.point)),
            dt: live.dt,
            hand: grapple_hand,
        },
    );
    let summary_masks =
        live.state
            .bindings
            .effective_masks(detection.mode, controls, semantic_input);
    let (_, summary_sources) = sources::table();
    let (summary_hold, summary_cancel) = crate::controllers::summary::hold::accepted(
        semantic_input,
        &routed.policy,
        &summary_masks,
        &hands.filtered.frame,
        &summary_sources,
    );
    crate::controllers::summary::hold::publish(summary_hold, summary_cancel);
    let targets = &hands.offers.targets;
    let flight_read = FlightRead {
        airborne: locomotion::anchor::status().airborne,
        reaches: targets.resolved.each_ref().map(|resolved| {
            resolved
                .reaches
                .iter()
                .map(|r| r.volume)
                .collect::<Vec<_>>()
        }),
    };
    let settings = live.settings;
    let flight_commands = flight::evaluate(
        live.state.flight.get_or_insert_with(Default::default),
        &flight_tick(
            settings,
            detection,
            live.dt,
            hands,
            step.grabs,
            &flight_read,
        ),
        &step.desired,
    );
    let in_menu = live.in_menu;
    let active = hands.offers.active.as_ref();
    grab::tick::finish(&grab::tick::Finish {
        tick: targets,
        latches: live.state.bindings.grip_latches(),
        grips: controls.hands.map(|h| h.grip),
        grabs: step.grabs,
        markers: active
            .filter(|_| settings.overlay.markers && !in_menu)
            .map(marker_frame),
    });
    let view_frame = active.filter(|_| !in_menu).map(marker_frame);
    physical_hands::grab_view::submit(targets, view_frame.as_ref());
    if step.crouch_toggle {
        roomscale::toggle_seated_crouch();
    }
    if step.sight_capture {
        sights::tuning::request_capture();
    }
    let interaction_context = physical_hands::pickup::diagnostics::input(
        hands.filtered.frame.sequence,
        targets,
        physical_hands::pickup::diagnostics::InputState {
            grips: controls.hands.map(|h| h.grip),
            latches: live.state.bindings.grip_latches(),
            grabs: step.grabs,
        },
        hands.offers.interaction_scans,
    );
    Evaluated {
        step,
        flight_commands,
        flight: flight_read,
        interaction_context,
        summary_before: summary::BeforeFinish::capture(vehicles::parachute_holds()),
    }
}

/// What the acts produced for the stages after them.
pub(super) struct Acted {
    /// The belt reload the weapon handling's finish half offers.
    pub(super) belt_reload: Option<BeltEligibility>,
    /// Each hand's fire input as the weapon handling last published it this tick: the finish
    /// half's decision, then each retain.
    pub(super) fire: [bool; 2],
    /// Each hand's trigger changed owner in the acts.
    pub(super) trigger_changed: [bool; 2],
}

/// One of the subsystems' acts on the tick's evaluation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Act {
    /// The weapon handling's finish half.
    Weapons,
    /// A trigger whose gun changed in the weapons' finish half changes owner, and keeps its fire.
    TriggerHandoff,
    /// The weapon wheel.
    Wheel,
    /// The physical hands' pickups.
    Pickup,
    /// The mounted gun.
    MountedGun,
    /// The bodies the physical hands hold, published for their lifetime.
    HeldBodies,
    /// The physical grapple's retract.
    Retract,
    /// The snatch's finish half.
    Snatch,
    /// The other hand's catch of a snatched target, held by its grip.
    OtherCatch,
    /// The physical hands' climb.
    Climb,
    /// The physical hands' stunt hang.
    Stunt,
    /// The helicopter rail's release by lateral movement.
    Rail,
    /// The parachute's ripcord.
    Ripcord,
    /// The wingsuit gestures' commit.
    Flight,
    /// A committed suit deploy spends the grapple's press.
    SpendGrapple,
    /// The vehicle handling's finish half.
    Vehicles,
    /// The grounded rig's committed step.
    StepWalk,
    /// The arm strokes of locomotion.
    Strokes,
}

/// The acts, in the order they run.
///
/// The trigger handoff reads the guns the weapons' finish half left; the other hand's catch is
/// what the snatch's finish half handed over; the ripcord's canopy request goes in before the
/// gestures commit, so it wins over a suit request on the same tick; the grapple's press is spent
/// on the deploy the gestures committed; and the vehicles' finish half reads the actions all of
/// them added.
pub(super) const ACTS: [Act; 18] = [
    Act::Weapons,
    Act::TriggerHandoff,
    Act::Wheel,
    Act::Pickup,
    Act::MountedGun,
    Act::HeldBodies,
    Act::Retract,
    Act::Snatch,
    Act::OtherCatch,
    Act::Climb,
    Act::Stunt,
    Act::Rail,
    Act::Ripcord,
    Act::Flight,
    Act::SpendGrapple,
    Act::Vehicles,
    Act::StepWalk,
    Act::Strokes,
];

/// Run the subsystems' acts on the evaluation, or in a game menu pause them ([`Reason::Menu`]).
pub(super) fn finish(
    live: &mut Live<'_, '_>,
    hands: &mut Hands,
    evaluated: &mut Evaluated,
) -> Acted {
    let acted = Acted {
        belt_reload: None,
        fire: [false; 2],
        trigger_changed: [false; 2],
    };
    if live.in_menu {
        live.stand_down(Reason::Menu);
        return acted;
    }
    let controls = &hands.controls;
    let finish = FinishInputs {
        grabs: grabs_for(evaluated.step.grabs, Consumer::Weapons),
        // Every hold the hands keep (a gun, a part of one, a pickup, a climb, the snatch's catch)
        // lets go when the hand opens, not when a force-sensing grip's press sags.
        grips: controls.hands.map(|h| h.hold),
        triggers: controls.hands.map(|h| h.trigger_press),
        fire_presses: gun_fire::presses(
            live.detection.mode,
            &hands.offers.semantic_input,
            controls.hands.map(|h| h.trigger_press),
            controls.masked_hands,
        ),
        unavailable: controls.masked_hands,
        reload_button: reload_button(&hands.offers.semantic_input),
        dt: live.dt,
        real_dt: live.real_dt,
    };
    let mut cx = Acts {
        live,
        hands,
        evaluated,
        finish,
        acted,
    };
    for act in ACTS {
        cx.run(act);
    }
    cx.acted
}

/// What the acts read and write.
struct Acts<'l, 'a, 'g> {
    live: &'l mut Live<'a, 'g>,
    hands: &'l mut Hands,
    evaluated: &'l mut Evaluated,
    finish: FinishInputs,
    acted: Acted,
}

impl Acts<'_, '_, '_> {
    fn run(&mut self, act: Act) {
        let game = self.live.game;
        let mode = self.live.detection.mode;
        let step = &mut self.evaluated.step;
        let finish = &self.finish;
        match act {
            Act::Weapons => {
                self.acted.belt_reload = weapons::finish_tick(game, finish, &mut step.desired);
            }
            Act::TriggerHandoff => {
                let after =
                    weapons::capabilities::gun_owners_now().map(|gun| TriggerOwner { mode, gun });
                let changed = self.live.state.trigger_ownership.after_finish(after);
                self.acted.trigger_changed = changed;
                // The record's fire is the finish half's, sampled at its end.
                self.acted.fire = contents::hands().map(|hand| hand.fired);
                if changed.contains(&true) {
                    self.acted.fire = trigger_ownership::retain_fire(self.acted.fire, changed);
                    weapons::hold::tick::publish_fire(self.acted.fire);
                    for hand in Hand::BOTH {
                        if changed[hand.index()] {
                            let controls = &mut self.hands.controls.hands[hand.index()];
                            controls.trigger_press = ButtonControl::default();
                            controls.trigger = 0.0;
                        }
                    }
                }
            }
            Act::Wheel => {
                let wheel = weapons::wheel::input_tick(
                    game,
                    &weapons::wheel::TickInputs {
                        mode,
                        presses: step.wheel,
                    },
                    &mut step.desired,
                );
                // The stick that holds the wheel open is the turn stick: its sideways drift must
                // not turn.
                if wheel.by_stick {
                    step.body_turn = 0.0;
                }
            }
            Act::Pickup => physical_hands::pickup::finish_tick(
                game,
                &FinishInputs {
                    grabs: grabs_for(step.grabs, Consumer::Pickup),
                    ..*finish
                },
                &mut step.desired,
                &self.evaluated.interaction_context,
            ),
            // SAFETY: the acts run in the game stage, on the game thread before the world update.
            Act::MountedGun => unsafe { vehicles::mounted::tick(game, finish, &mut step.desired) },
            // SAFETY: as above.
            Act::HeldBodies => unsafe {
                physical_hands::lifetime::publish(
                    physical_hands::held_bodies()
                        .map(|held| held.map(|held| (held.object, held.body))),
                )
            },
            Act::Retract => {
                if let Some(value) = self.hands.grapple.physical.retract {
                    step.desired.push(Desired {
                        target: Target {
                            map: MapKind::Player,
                            action: Action::RETRACT_GRAPPLE,
                        },
                        want: Want::Analog(value),
                    });
                }
            }
            Act::Snatch => {
                let button_holds = snatch::ButtonHolds::routed(
                    step.grapple_held,
                    self.hands.grapple.snatch_button_owned,
                );
                snatch::finish_tick(
                    game,
                    &snatch::FinishInput {
                        hand: self.live.grapple_hand,
                        grabs: grabs_for(step.grabs, Consumer::Snatch),
                        grips: finish.grips,
                        latches: self.live.state.bindings.grip_latches(),
                        line: self.hands.grapple.grapple_line,
                        ordinary_button_held: button_holds.ordinary,
                        modifier_held: button_holds.modifier,
                        detection: &self.live.detection,
                        dt: self.live.dt,
                        real_dt: self.live.real_dt,
                        config: &snatch_settings(self.live.settings),
                    },
                    &mut step.desired,
                );
            }
            Act::OtherCatch => {
                if let Some((hand, volume)) = snatch::take_other_catch() {
                    self.live.state.bindings.hold_catch(hand, volume);
                }
            }
            Act::Climb => physical_hands::climb::finish_tick(
                finish.grips.map(|g| g.held()),
                grabs_for(step.grabs, Consumer::Climb),
            ),
            Act::Stunt => physical_hands::stunt::finish_tick(
                game,
                finish.grips.map(|g| g.held()),
                &mut step.desired,
            ),
            Act::Rail => {
                let controls = &self.hands.controls;
                let rail_packet =
                    flight_pose::for_frame(&self.hands.filtered.original, Instant::now());
                if self.live.state.rail.step(
                    rail::RailInput {
                        hanging: physical_hands::stunt::hanging()
                            && !controls.masked_hands.contains(&true)
                            && self.live.detection.held.is_none()
                            && !controls.ragdolling,
                        epoch: rail_packet
                            .as_ref()
                            .map_or(0, |packet| packet.sample().epoch),
                        head: rail_packet.as_ref().map(|packet| {
                            let head = packet.head_pose();
                            (head.position, head.orientation)
                        }),
                    },
                    &mut step.desired,
                ) {
                    tracing::info!(target: "controllers", "lateral movement releases the helicopter rail");
                }
            }
            Act::Ripcord => vehicles::ripcord_finish(
                mode,
                grabs_for(step.grabs, Consumer::Vehicles),
                &self.hands.controls,
                &mut step.desired,
            ),
            Act::Flight => flight::commit(
                self.live
                    .state
                    .flight
                    .as_mut()
                    .expect("the gesture state was initialized"),
                self.evaluated.flight_commands,
                &flight_tick(
                    self.live.settings,
                    self.live.detection,
                    self.live.dt,
                    self.hands,
                    step.grabs,
                    &self.evaluated.flight,
                ),
                &mut step.desired,
            ),
            Act::SpendGrapple => {
                if self.evaluated.flight_commands.transition == Some(SuitTransition::Deploy)
                    && step.desired.iter().any(|input| {
                        input.target.map == MapKind::Player
                            && input.target.action == Action::OPEN_WINGSUIT
                            && input.want == Want::Click
                    })
                {
                    self.live.state.bindings.spend_grapple();
                }
            }
            Act::Vehicles => vehicles::finish_tick(
                game,
                mode,
                grabs_for(step.grabs, Consumer::Vehicles),
                &self.hands.controls,
                &mut step.desired,
                self.live.dt,
                step.grapple_held,
            ),
            // A committed step walks Rico through the game's animation; the drive sets his speed.
            Act::StepWalk => {
                if self.live.settings.steps {
                    add_step_walk(&mut step.desired);
                }
            }
            Act::Strokes => locomotion::strokes::tick(
                self.hands.offers.active.as_ref(),
                self.hands.controls.armed,
                self.live.real_dt,
            ),
        }
    }
}

/// What the wingsuit gestures read at their evaluation, for their commit to read the same.
struct FlightRead {
    /// Rico is off the ground on foot, as the view anchor read it at the evaluation.
    airborne: bool,
    /// What each hand is within reach of this tick, by [`Hand::index`].
    reaches: [Vec<GrabVolume>; 2],
}

fn jump_pressed(input: &SemanticInput) -> bool {
    input
        .get(OnFootAction::Jump.id())
        .is_some_and(|input| input.hands.iter().any(|hand| hand.button.pressed))
}

/// The wingsuit gestures' view of the tick.
fn flight_tick<'a>(
    settings: &'a Settings,
    detection: Detection,
    dt: f32,
    hands: &'a Hands,
    grabs: [Option<GrabVolume>; 2],
    read: &'a FlightRead,
) -> flight::Tick<'a> {
    flight::Tick {
        frame: &hands.filtered.original,
        grip_offset: settings.grip_offset,
        gestures: &vehicles::tuning::VEHICLES.get().wingsuit_gestures,
        dimensions: &settings.dimensions,
        detection,
        airborne: read.airborne,
        ready: !hands.controls.masked_hands.contains(&true),
        ragdoll: hands.controls.ragdolling,
        // Tethers left in the world do not occupy a hand; only a held button or a live line does.
        grapple: hands.offers.grapple_held || hands.grapple.line,
        grabs,
        reaches: read.reaches.each_ref().map(Vec::as_slice),
        dt,
    }
}

/// Where the markers are placed for the hands of `active` at this tick.
fn marker_frame(active: &ActiveHands) -> MarkerFrame {
    MarkerFrame {
        xf: CockpitTransform::current(active.head.position, active.world_scale),
        drawn: active.hands,
    }
}

/// The settings the snatch reads this tick.
fn snatch_settings(settings: &Settings) -> SnatchSettings {
    SnatchSettings {
        overlay: settings.overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(act: Act) -> usize {
        ACTS.iter().position(|a| *a == act).unwrap()
    }

    /// The order the acts' data depends on.
    #[test]
    fn the_acts_run_after_what_they_read() {
        for (before, after) in [
            (Act::Weapons, Act::TriggerHandoff),
            (Act::Snatch, Act::OtherCatch),
            (Act::Ripcord, Act::Flight),
            (Act::Flight, Act::SpendGrapple),
            (Act::Flight, Act::Vehicles),
            (Act::Ripcord, Act::Vehicles),
        ] {
            assert!(position(before) < position(after), "{before:?} {after:?}");
        }
    }

    /// The whole order, pinned: a change is a behaviour change to check in the game.
    #[test]
    fn the_order_is_pinned() {
        assert_eq!(
            ACTS,
            [
                Act::Weapons,
                Act::TriggerHandoff,
                Act::Wheel,
                Act::Pickup,
                Act::MountedGun,
                Act::HeldBodies,
                Act::Retract,
                Act::Snatch,
                Act::OtherCatch,
                Act::Climb,
                Act::Stunt,
                Act::Rail,
                Act::Ripcord,
                Act::Flight,
                Act::SpendGrapple,
                Act::Vehicles,
                Act::StepWalk,
                Act::Strokes,
            ]
        );
    }
}
