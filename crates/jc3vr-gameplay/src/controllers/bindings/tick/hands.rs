//! The hands' stages: every subsystem's grip targets and the resolve among them, the logical
//! controls, the triggers' ownership, and the grapple button's owners.

use body_rig::math::angle::{heading, yaw_of};
use glam::Vec3;
use jc3gi::state::StateBitFlag;
use jc3vr_body::controllers::{
    aim::sights,
    body_frame, grab,
    grab::Provided,
    hands,
    hands::ActiveHands,
    haptics, intent,
    intent::{
        config::{long_press_s, trigger_deadzone},
        controls::{ButtonControl, Controls},
        grapple_button::GrappleContext,
        yank::{HandLine, grapple_yank},
    },
    locomotion::config::LocomotionDirection,
    mode,
    mode::{Mode, VehicleClass},
    physics::local_player::LocalPlayer,
};
use jc3vr_grapple::{
    controllers::snatch,
    grapple,
    grapple::physical::{TickInput as GrappleTickInput, TickOutput as GrappleTickOutput},
};
use jc3vr_input::{
    frame::{self, Button, ControllerFrame, GripPress, Hand},
    haptics::cues::Event,
    semantic::SemanticSet,
};
use jc3vr_physical::controllers::{
    physical_hands, physical_hands::pickup::diagnostics::scan::Scan,
};
use jc3vr_vehicles::controllers::vehicles;
use jc3vr_view::{headpose, tuning::VIEW, vr::input::config::grip_press};
use jc3vr_weapons::controllers::{weapons, weapons::StandDown};

use crate::controllers::{
    bindings::{
        bail,
        bail::{BailTick, bails_by_parachute},
        buttons::{grapple_button, grapple_button_held},
        intents::SemanticInput,
        pointer::owns_menu_trigger,
        summary::yank_armed_in,
        tables,
        tick::{
            Live,
            input::{Capture, Filtered, Routed},
        },
        trigger_ownership,
        trigger_ownership::{Owner as TriggerOwner, RawTrigger},
    },
    tuning::GAMEPLAY,
};

/// Every subsystem's grip targets, resolved, and the tick's semantic input.
pub(super) struct Offers {
    /// The physical hands reserving a trigger for a held gun, from the weapon handling's tick
    /// start.
    pub(super) held: Option<[bool; 2]>,
    pub(super) active: Option<ActiveHands>,
    /// What each hand's interaction scan saw, for the pickup's diagnostics.
    pub(super) interaction_scans: [Option<Scan>; 2],
    pub(super) semantic_input: SemanticInput,
    /// The grapple button as the semantic input held it before the policy's routes.
    pub(super) raw_grapple_button: ButtonControl,
    /// The grapple button is held, as far as the grapple hand's pulls are concerned.
    pub(super) grapple_held: bool,
    pub(super) targets: grab::tick::Tick,
    /// How far the move stick turns, radians: zero for head-relative locomotion.
    pub(super) move_rotation: f32,
}

/// Start the weapon and vehicle handling's ticks, gather every subsystem's grip targets, and
/// resolve each hand among them.
pub(super) fn offers(live: &mut Live<'_, '_>, filtered: &Filtered, routed: &Routed) -> Offers {
    let game = live.game;
    let detection = live.detection;
    let frame = &filtered.frame;
    let edges = &filtered.edges;
    let move_rotation = match live.settings.locomotion_direction {
        LocomotionDirection::Head => 0.0,
        LocomotionDirection::OffHand => off_hand_rotation(frame, live.dominant.other()),
    };
    let (start, vehicle) = if live.in_menu {
        weapons::stand_down(game, StandDown::Menu);
        (
            weapons::TickStart::default(),
            vehicles::TickVolumes::default(),
        )
    } else {
        (
            weapons::begin_tick(game, detection.mode),
            vehicles::begin_tick(game, detection.mode),
        )
    };
    // Every subsystem's targets go to the one resolver, which picks what each hand would take.
    let active = hands::active_hands_raw();
    let mut provided = Provided::targets(start.targets);
    provided.extend(Provided::targets(vehicle.targets));
    if let Some(note) = weapons::reload::pouch_note() {
        for notes in &mut provided.notes {
            notes.push(note.to_owned());
        }
    }
    let mut interaction_scans = [None; 2];
    if !live.in_menu {
        provided.extend(physical_hands::targets(
            active.as_ref().map_or(1.0, |a| a.world_scale),
            &mut interaction_scans,
        ));
    }
    let profiles = Hand::BOTH.map(|hand| frame.hand(hand).profile);
    let mut semantic_input = match &routed.semantic {
        Some(semantic) => SemanticInput::from_frame(
            semantic,
            &routed.semantic_edges,
            profiles,
            live.dominant,
            &live.settings.bindings,
        ),
        None => SemanticInput::rest(
            SemanticSet::of(detection.mode.action_set()),
            profiles,
            live.dominant,
        ),
    };
    let raw_grapple_button = grapple_button(&semantic_input, detection.mode);
    routed.policy.filter_semantic(&mut semantic_input);
    live.state.bindings.cancel_routed_gestures(&routed.policy);
    // The hand's pulls act only while the grapple button is held.
    let grapple_held = grapple_button_held(&semantic_input, detection.mode);
    if !live.in_menu {
        let grapple_hand = live.grapple_hand;
        provided.extend(snatch::targets(
            game,
            &snatch::OfferInput {
                hand: grapple_hand,
                detection: &live.detection,
                button_held: grapple_held,
                grip: ButtonControl {
                    down: frame
                        .hand(grapple_hand)
                        .buttons
                        .contains(Button::SqueezePress),
                    pressed: edges.pressed(grapple_hand, Button::SqueezePress),
                    released: edges.released(grapple_hand, Button::SqueezePress),
                },
                dt: live.real_dt,
            },
        ));
        provided.extend(grapple::line_grab::targets(
            grapple_hand,
            active.as_ref().map_or(1.0, |a| a.world_scale),
        ));
    }
    let targets = grab::tick::resolve(provided.targets, provided.notes, active.as_ref());
    Offers {
        held: start.held,
        active,
        interaction_scans,
        semantic_input,
        raw_grapple_button,
        grapple_held,
        targets,
        move_rotation,
    }
}

/// The tick's logical controls: the frame reduced to the controls, each hand's closed grip, the
/// masked hands, the stunt, and the bail.
pub(super) fn controls(live: &mut Live<'_, '_>, filtered: &Filtered, offers: &Offers) -> Controls {
    let game = live.game;
    let detection = live.detection;
    let frame = &filtered.frame;
    let mut controls = Controls::from_frame(
        frame,
        &filtered.edges,
        &live.settings.bindings,
        live.dominant,
        offers.move_rotation,
        offers.targets.chosen(),
    );
    let catch_reserved = if live.in_menu {
        [false; 2]
    } else {
        snatch::reserved_hands()
    };
    for hand in Hand::BOTH {
        let i = hand.index();
        controls.hands[i].grab_slow = offers.targets.slow[i];
        controls.hands[i].catch_reserved = catch_reserved[i];
        let source = frame.hand(hand);
        let force = matches!(grip_press(source.profile, hand), GripPress::Force(_));
        controls.hands[i].hold = live.state.closed[i].step(
            controls.hands[i].grip,
            (force && source.active).then_some(source.inputs.squeeze),
            VIEW.get().press.squeeze.release,
        );
        intent::record_hand_closed(hand, controls.hands[i].hold.down);
    }
    controls.masked_hands = Hand::BOTH.map(|hand| !live.state.recovery.ready(hand));
    controls.stunt_available = if matches!(detection.mode, Mode::Vehicle(seat) if seat.class != VehicleClass::Mech)
        && detection.held.is_none()
    {
        if bail::stunt_hangs_inverted(detection.mode) {
            // A helicopter seat's stunt act drops Rico into the hang under the airframe, which
            // holds him inverted; the seat's exit takes the press instead.
            Some(false)
        } else {
            bail::seated_stunt_available(game)
        }
    } else {
        None
    };
    controls.bail = match detection.mode {
        Mode::Vehicle(seat) => {
            let mounted_gun = LocalPlayer::get(game).is_some_and(|p| p.in_mounted_gun_state());
            let seat_bails = bails_by_parachute(seat, mounted_gun);
            let press_long = live.state.bindings.exit_press_long(seat, long_press_s());
            live.state.bail.step(BailTick {
                seat_bails,
                speed_mps: seat_bails
                    .then(|| vehicles::seat_vehicle_speed_mps(game))
                    .flatten(),
                press_long,
            })
        }
        _ => {
            live.state.bail.reset();
            false
        }
    };
    controls
}

/// The triggers' ownership before the acts.
pub(super) struct Triggers {
    /// Each hand's trigger kept from its routes because another owner holds it.
    pub(super) blocked: [bool; 2],
    /// The semantic input before the triggers' masks, to suppress the routes of a trigger whose
    /// owner changes in the acts.
    pub(super) routes: SemanticInput,
}

/// Decide who owns each trigger, mask the routes of a trigger another owner holds, and fill in
/// the controls the tables read beside the hands.
pub(super) fn triggers(
    live: &mut Live<'_, '_>,
    filtered: &Filtered,
    offers: &mut Offers,
    controls: &mut Controls,
) -> Triggers {
    let detection = live.detection;
    let in_menu = live.in_menu;
    let gun_owners = weapons::capabilities::gun_owners_now();
    controls.armed = gun_owners.map(|gun| gun.is_some() && !in_menu);
    let trigger_owners = gun_owners.map(|gun| TriggerOwner {
        mode: detection.mode,
        gun: gun.filter(|_| !in_menu),
    });
    let raw_triggers = Hand::BOTH.map(|hand| {
        let source = filtered.original.hand(hand);
        RawTrigger {
            button: ButtonControl {
                down: source.buttons.contains(Button::TriggerPress),
                pressed: filtered.original_edges.pressed(hand, Button::TriggerPress),
                released: filtered.original_edges.released(hand, Button::TriggerPress),
            },
            value: source.inputs.trigger,
            usable: source.active
                && source.grip.is_tracked()
                && !controls.masked_hands[hand.index()],
        }
    });
    let blocked =
        live.state
            .trigger_ownership
            .observe(trigger_owners, raw_triggers, trigger_deadzone());
    let routes = offers.semantic_input.clone();
    live.state.bindings.cancel_trigger_gestures(blocked);
    trigger_ownership::mask_routes(
        &mut offers.semantic_input,
        detection.mode,
        trigger_ownership::masks(trigger_owners, blocked),
    );
    offers.grapple_held = grapple_button_held(&offers.semantic_input, detection.mode);
    for hand in Hand::BOTH {
        if blocked[hand.index()] {
            controls.hands[hand.index()].trigger_press = ButtonControl::default();
            controls.hands[hand.index()].trigger = 0.0;
        }
    }
    controls.weapon_cycle = live.settings.stick_click_cycles;
    controls.pointer_hand = owns_menu_trigger();
    // The off-hand's trigger fires the gun it holds, in a mode whose triggers fire the hand weapons.
    let fires_guns = tables::triggers_fire_hand_weapons(detection.mode);
    let held = offers.held.filter(|_| fires_guns).unwrap_or([false; 2]);
    controls.dual_wield = held[live.dominant.other().index()];
    controls.sight_chord = live.settings.sight_tuning && sights::tuning::lining_up();
    controls.ragdolling = mode::last_snapshot()
        .and_then(|snapshot| snapshot.character)
        .is_some_and(|character| character.has(StateBitFlag::SB_IN_RAGDOLL));
    Triggers { blocked, routes }
}

/// The grapple button's owners and the physical grapple's tick.
pub(super) struct GrappleTick {
    /// The snatch owns the grapple button this tick.
    pub(super) snatch_button_owned: bool,
    /// The grapple has a line of its own attached, and the snatch does not own the button.
    pub(super) line: bool,
    pub(super) grapple_line: Option<HandLine>,
    pub(super) physical: GrappleTickOutput,
}

/// Decide who owns the grapple button, detect the yank, and step the physical grapple.
pub(super) fn grapple(
    live: &mut Live<'_, '_>,
    routed: &Routed,
    capture: &Capture,
    filtered: &Filtered,
    offers: &mut Offers,
    controls: &mut Controls,
) -> GrappleTick {
    let game = live.game;
    let detection = live.detection;
    let grapple_hand = live.grapple_hand;
    let neutral =
        capture.neutral_policy.frame_neutral(&filtered.original) && capture.source_neutral;
    live.state.bindings.observe_grapple_neutral(
        offers.raw_grapple_button,
        routed.semantic.is_some() && !controls.masked_hands.contains(&true) && neutral,
    );
    let snatch_button_owned = live.state.bindings.claim_snatch(
        offers.raw_grapple_button,
        snatch::can_claim_button(),
        routed.semantic.is_some()
            && !controls.masked_hands.contains(&true)
            && snatch::available_in(game, &detection),
        neutral,
    );
    offers.grapple_held = live
        .state
        .bindings
        .physical_grapple_held(offers.grapple_held);
    let grapple_held = offers.grapple_held;
    let line = line_attached() && !snatch_button_owned;
    let grapple_line = grapple_hand_line(grapple_hand);
    let yank = live.state.yank.step_from(
        live.real_dt,
        grapple_line,
        headpose::xr::cockpit_pose().map(|pose| pose.position),
        yank_armed_in(line, &detection) && grapple_held,
        grapple_yank(),
    );
    if let Some(yank) = yank {
        controls.yank = true;
        tracing::debug!(target: "controllers", ?yank, "yank");
        haptics::play(grapple_hand, Event::Yank);
    }
    let physical = grapple::physical::input_tick(
        game,
        &GrappleTickInput {
            dt: live.dt,
            real_dt: live.real_dt,
            line: grapple_line,
            button_held: grapple_held && !snatch_button_owned,
            detection: &detection,
            dominant: live.settings.acting,
        },
    );
    controls.grapple = GrappleContext {
        tethers: physical.tethers > 0,
        line: yank_armed_in(line, &detection) || grapple::seat::status().flying,
        aim_target: body_frame::aim().grapple.is_some_and(|g| g.valid),
        reeling: physical.lockable && !snatch_button_owned,
        seated: grapple::seat::stands_in_for_local(game),
    };
    controls.lock_yank = physical.lock_yank;
    let lock_status = grapple::physical::lock::status();
    controls.grapple_retarget = lock_status.locked
        && body_frame::aim().grapple.is_some_and(|aim| {
            aim.valid
                && aim.current
                && aim.point.distance(lock_status.hook) > GAMEPLAY.get().bindings.grapple_retarget_m
        });
    GrappleTick {
        snatch_button_owned,
        line,
        grapple_line,
        physical,
    }
}

/// The angle from the head's heading to the off-hand controller's, both in the cockpit frame, for
/// off-hand-relative locomotion. The game already moves relative to the view, which follows the
/// head, so rotating the stick by the difference makes its forward the controller's. Zero when
/// either heading is unknown.
fn off_hand_rotation(frame: &ControllerFrame, off: Hand) -> f32 {
    let aim = &frame.hand(off).aim;
    let hand = aim
        .orientation_valid
        .then(|| yaw_of(aim.pose.orientation * Vec3::NEG_Z));
    let head = headpose::xr::cockpit_pose().map(|pose| heading(pose.orientation));
    match (hand.flatten(), head) {
        (Some(hand), Some(head)) => hand - head,
        _ => 0.0,
    }
}

/// Whether the grapple has a line of its own attached, as the mode detector's snapshot reads it.
fn line_attached() -> bool {
    mode::last_snapshot()
        .and_then(|s| s.character)
        .is_some_and(|c| c.grapple_wire)
}

/// The grapple hand's grip position and aim direction, in the cockpit frame, or `None` while the
/// hand is untracked or the hands are inert; the zip's and the snatch's yanks are measured along
/// the aim.
fn grapple_hand_line(hand: Hand) -> Option<HandLine> {
    let tracked = frame::with_latest(|frame| {
        frame.focused && frame.hand(hand).active && frame.hand(hand).grip.is_tracked()
    })?;
    if !tracked {
        return None;
    }
    let cockpit = hands::active_hands_raw()?.hands[hand.index()]?;
    Some(HandLine {
        grip: cockpit.grip.position,
        aim: cockpit.aim.orientation * Vec3::NEG_Z,
    })
}
