//! The controller-to-game bindings: on every engine input tick, map the latest controller frame
//! and its edges to the game's actions for the current mode, and inject them.
//!
//! - [`controls`] reduces the frame to logical controls (deadzones, edges, what each grip would
//!   take);
//! - [`intents`] reads the tick's semantic actions and masks what the mod claims of each hand;
//! - [`tables`] maps the semantic actions to the wanted game actions per mode;
//! - [`hold`] turns the wanted actions into effector writes with the game's hold timing;
//! - the grip model ([`crate::controllers::grab`]) gathers the targets the weapon handling, the
//!   vehicle handling, and the physical hands provide, resolves each hand among them, and hands
//!   each grab to the subsystem that owns its target;
//! - [`grapple_button`] reads the grapple button's gestures while tethers or a line are attached:
//!   a tap, a double tap that releases the tethers, a hold that tethers or tightens, and a sharp
//!   pull that zips;
//! - [`yank`] detects the yank gesture on the grapple hand, a sharp pull while the grapple button is
//!   held, and the snatch's own yank while the grip holds a remote target
//!   ([`crate::controllers::snatch`]);
//! - `recovery` masks a hand after a loss of its pose or of injection until its controls rest
//!   ([`HandRecovery`]): its raw controls, and every semantic action on it;
//! - [`inject`] applies the writes to the player and UI action maps;
//! - [`stage`] splits the tick between the input job and the game thread, and hands the writes and
//!   the body turn between them;
//! - [`policy_a`] arbitrates the shared system chords before raw, semantic, and pointer consumers,
//!   retaining ownership while gameplay stands down.
//!
//! The tick runs in two stages ([`stage`]). [`game_tick`], the decisions and the subsystems' acts,
//! runs in the game stage, on the game thread at the top of the world's pre-simulation queue in
//! play (inline in the input update in the load state and the frontend's loading). Its writes land
//! in the next poll stage, in the `InputDeviceManager::Update` post-call detour
//! ([`crate::hooks::input::look`]), after the device poll and before the game reads its actions.
//! It injects only while the bindings are enabled, the controller scheme is in effect, a controller
//! is active, the game's device poll runs, and the mod's panel is down; otherwise it writes nothing,
//! so a real gamepad under the gamepad scheme is never fought. The right stick's body turn is not an
//! action: it goes to the look detour's next poll stage, which feeds it to the headpose's VR turn
//! scheme.
//!
//! Whenever injection stops, the bindings undo what they wrote rather than leave it to the device
//! poll ([`HoldTracker::release_all`]): the poll does not run while the device manager is disabled,
//! and an effector left clicked there reads as a fresh click on every frame.

mod buttons;
pub mod controls;
mod flight;
pub mod grapple_button;
mod gun_fire;
pub mod hold;
pub mod inject;
pub mod intents;
mod loss;
pub(crate) mod policy_a;
mod recovery;
mod report;
pub(crate) mod stage;
mod summary;
pub mod tables;
mod trigger_ownership;
pub mod yank;

use std::{sync::LazyLock, time::Instant};

use body_rig::angle::{head_heading, yaw_of};
use glam::Vec3;
use jc3gi::{input::input_action_map::Action, state::StateBitFlag};
use parking_lot::Mutex;

pub use crate::controllers::bindings::{
    recovery::{Blocker, HandRecovery},
    report::{Idle, ReportedWrite, TickReport, recovery_status},
};
use crate::{
    config::Config,
    controllers::{
        aim::{self, sights},
        bindings::{
            buttons::{grapple_button, grapple_button_held, reload_button},
            controls::{ButtonControl, Controls},
            grapple_button::GrappleContext,
            hold::{ActionWrite, Desired, DevicePoll, HoldTracker, MapKind, Target, Want},
            intents::SemanticInput,
            policy_a::{
                SystemCommand,
                runtime::{Runtime, TickInput},
            },
            recovery::{MaskEvent, MaskLog, NeutralPolicy, Recovery},
            report::{publish_recovery_status, report_writes},
            stage::{Delivery, GameStageOutput, LandedSegment, PollRecord, Segment},
            summary::yank_armed_in,
            tables::Bindings,
            trigger_ownership::{Owner as TriggerOwner, RawTrigger, TriggerOwnership},
            yank::{HandLine, LineYank, Yank},
        },
        body::rig::grounded::add_step_walk,
        clock::TimeScale,
        config::LocomotionDirection,
        frame::{self, Button, ControllerFrame, Hand},
        grab::{self, Consumer, Provided, grabs_for, markers::MarkerFrame},
        hands::{self, tiers},
        haptics::{self, Event},
        locomotion,
        mode::{self, Detection, Mode},
        physical_hands, roomscale, scheme, snatch,
        vehicles::{self, wheel::CockpitTransform},
        weapons,
    },
    grapple, headpose,
    hud::{
        egui_panel,
        pointer::{clear_controller, owns_menu_trigger, update_controller, update_egui_controller},
    },
    vr::{
        self,
        input::semantic::{SemanticSet, frame as semantic_frame, history, vocabulary::MenuAction},
    },
};

/// The bindings' game stage ([`stage::game_stage`]): take in where the last writes landed, detect
/// the mode (which selects the action set), evaluate the bindings, run the subsystems' acts, and
/// produce this tick's writes. Under [`Delivery::Inline`] the writes land before this returns;
/// under [`Delivery::Deferred`] they are returned for the next poll stage to land.
pub(crate) fn game_tick(
    record: PollRecord,
    landed: Vec<LandedSegment>,
    delivery: Delivery,
) -> GameStageOutput {
    let mut reported = absorb_landed(landed);
    let mut sink = Vec::new();
    let body_turn = tick(record.dt, record.poll, &mut sink);
    let deferred = match delivery {
        Delivery::Inline => {
            // SAFETY: an inline game stage runs inside the input update right after the device
            // poll, outside the input job.
            reported.extend(absorb_landed(unsafe { stage::land(sink) }));
            Vec::new()
        }
        Delivery::Deferred => sink,
    };
    STATE.lock().report.writes = reported;
    GameStageOutput {
        deferred,
        body_turn,
    }
}

/// Tell the hold tracker where its tracked writes landed, and report every write's outcome.
fn absorb_landed(landed: Vec<LandedSegment>) -> Vec<ReportedWrite> {
    let mut state = STATE.lock();
    let mut reported = Vec::new();
    for LandedSegment { segment, landings } in landed {
        if segment.tracked {
            let effectors: Vec<_> = landings.iter().map(|l| l.effector).collect();
            state.tracker.landed(&segment.writes, &effectors);
        }
        reported.extend(report_writes(&segment.writes, &landings));
    }
    reported
}

/// One tick of the bindings, pushing its writes to `sink` in order. `dt` is the engine's tick
/// delta, seconds, and `poll` is whether the device poll that preceded the tick ran. Returns the
/// controllers' body turn, or `None` when they are not injecting.
fn tick(dt: f32, poll: DevicePoll, sink: &mut Vec<Segment>) -> Option<f32> {
    // What the player's body does is timed in real time, and `dt` is the game's step, which its
    // slow motion shortens.
    // SAFETY: the bindings tick runs while the game runs.
    let real_dt = unsafe { TimeScale::current() }.real_dt(dt);
    let (config, neutral_policy, flight_dimensions) = Config::lock_query(|c| {
        (
            c.controllers,
            NeutralPolicy::new(
                &c.controllers.input,
                &c.controllers.bindings,
                &c.headpose.vr_turn,
                c.controllers.acting_hand(),
            ),
            c.body,
        )
    });
    let desktop = crate::egui_impl::desktop_input_observation();
    let raw_frame = frame::latest();
    let source_neutral = raw_frame
        .as_ref()
        .and_then(|frame| history::original_group(frame.sequence))
        .is_some_and(|group| {
            policy_a::runtime::group_neutral(&group, &config.bindings, config.acting_hand())
        });
    let controller_neutral = !config.enabled
        || !scheme::is_controllers()
        || raw_frame
            .as_ref()
            .is_some_and(|frame| neutral_policy.frame_neutral(frame) && source_neutral);
    let capture_input = mode::DebugUiInput {
        focused: desktop.focused() == Some(true)
            && raw_frame.as_ref().is_none_or(|frame| frame.focused),
        tracking_ready: !config.enabled
            || !scheme::is_controllers()
            || raw_frame.as_ref().is_some_and(|frame| {
                frame.any_active()
                    && frame
                        .hands
                        .iter()
                        .filter(|hand| hand.active)
                        .all(|hand| hand.grip.is_valid())
            }),
        // An unavailable desktop sample (the window unfocused, or an inline tick off the window's
        // thread) cannot show a held key, and it cannot block gameplay in the headset:
        // the editing owner is gated on desktop focus, under which the sample is observed.
        controls_neutral: controller_neutral && desktop.controls_neutral() != Some(false),
    };
    let mut capture_cancelled =
        mode::update_debug_ui_capture(capture_input).is_some_and(|update| {
            update.effects.consume_until_neutral
                || update.effects.clear_pending_events
                || update.effects.cancel_pointer_and_drag
        });
    if capture_cancelled {
        cancel_capture_routes(poll, sink);
    }
    if !config.enabled {
        clear_controller();
        update_egui_controller(None, config.acting_hand(), Default::default());
        let mut state = STATE.lock();
        state.observe_poll(poll);
        state.stand_down(poll, sink);
        state.recovery.suspend();
        state.publish_recovery();
        frame::take_edges();
        semantic_frame::take();
        let (replay, publications) = history::take_gameplay();
        INPUT_POLICY.lock().step(TickInput {
            publications,
            replay,
            frame: None,
            context: 0,
            gameplay: false,
            usable: false,
            config: &config.bindings,
            dominant: config.acting_hand(),
        });
        weapons::stand_down();
        // SAFETY: the game stage runs on the game thread before the world update.
        unsafe { weapons::inventory::stand_down() };
        vehicles::stand_down();
        grapple::physical::stand_down();
        snatch::stand_down();
        grab::tick::stand_down();
        return None;
    }
    let mut detection = mode::tick();
    tiers::tick(dt);
    // SAFETY: the game stage runs on the game thread before the world update.
    unsafe { hands::anchor::input_tick(&config) };

    let frame = frame::latest()
        .filter(ControllerFrame::any_active)
        .filter(|frame| frame.focused);
    let mut idle = if !config.bindings.enabled {
        Some(Idle::Disabled)
    } else if !scheme::is_controllers() {
        Some(Idle::GamepadScheme)
    } else if frame.is_none() {
        Some(Idle::NoControllers)
    } else if poll == DevicePoll::Disabled {
        Some(Idle::PollDisabled)
    } else if mode::debug_ui_capture().is_some_and(|snapshot| !snapshot.gameplay_effective) {
        Some(Idle::ModPanel)
    } else {
        None
    };
    // The edges are taken even while idle, so that a press from before injection resumed is not
    // replayed then.
    let mut edges = frame::take_edges();
    let (semantic, semantic_edges) = semantic_frame::take();
    let (replay, publications) = history::take_gameplay();
    let mut routed = INPUT_POLICY.lock().step(TickInput {
        publications,
        replay,
        frame: frame.as_ref(),
        context: policy_context(detection.mode),
        gameplay: idle.is_none(),
        usable: config.bindings.enabled && scheme::is_controllers(),
        config: &config.bindings,
        dominant: config.acting_hand(),
    });
    crate::controllers::summary::hold::publish(false, false);
    let mut panel_toggled = false;
    for &command in &routed.commands {
        system_command(command);
        if command == SystemCommand::TogglePanel {
            panel_toggled = true;
            let effects = mode::update_debug_ui_capture(mode::DebugUiInput {
                controls_neutral: false,
                ..capture_input
            });
            if effects.is_some_and(|update| {
                update.effects.consume_until_neutral
                    || update.effects.clear_pending_events
                    || update.effects.cancel_pointer_and_drag
            }) {
                capture_cancelled = true;
                cancel_capture_routes(poll, sink);
            }
            detection = mode::tick();
            if mode::debug_ui_capture().is_some_and(|snapshot| !snapshot.gameplay_effective) {
                idle = Some(Idle::ModPanel);
            }
            break;
        }
    }
    capture_cancelled |= panel_toggled;
    if capture_cancelled {
        routed = Default::default();
    }
    if frame.is_none() {
        PANEL_RECOVERY.lock().suspend();
    }
    let panel_input = frame.clone().map(|mut consumer| {
        let mut edges = edges;
        routed.filter_raw(&mut consumer, &mut edges);
        let mut recovery = PANEL_RECOVERY.lock();
        if let Some(original) = frame.as_ref() {
            recovery.observe(original);
            recovery.filter_observed(original, &mut consumer, &mut edges, &neutral_policy);
        }
        (consumer, edges)
    });
    update_egui_controller(
        panel_input
            .as_ref()
            .filter(|_| {
                mode::debug_ui_capture().is_some_and(|snapshot| snapshot.edit_capture.pointer)
            })
            .map(|(frame, edges)| (frame, edges)),
        config.acting_hand(),
        Hand::BOTH.map(|hand| {
            if panel_input.as_ref().is_some_and(|(frame, _)| {
                frame.hand(hand).active && frame.hand(hand).grip.is_tracked()
            }) && PANEL_RECOVERY.lock().ready(hand)
            {
                routed.button(MenuAction::PointerSelect.id(), hand)
            } else {
                ButtonControl::default()
            }
        }),
    );

    let mut state = STATE.lock();
    state.observe_poll(poll);
    let (Some(mut frame), None) = (frame, idle) else {
        clear_controller();
        state.stand_down(poll, sink);
        state.recovery.suspend();
        state.publish_recovery();
        weapons::stand_down();
        // Leaving the controllers puts the loadout table back into the game's slots; a pause of
        // the controllers (the debug panel, a lost frame) keeps it.
        if matches!(idle, Some(Idle::Disabled | Idle::GamepadScheme)) {
            // SAFETY: the game stage runs on the game thread before the world update.
            unsafe { weapons::inventory::stand_down() };
        }
        vehicles::stand_down();
        grapple::physical::stand_down();
        snatch::stand_down();
        grab::tick::stand_down();
        state.report = TickReport {
            detection,
            idle,
            ..Default::default()
        };
        return None;
    };

    state.recovery.observe(&frame);
    loss::cancel_throws(Hand::BOTH.map(|hand| !state.recovery.ready(hand)));
    // Mode selection precedes the next OpenXR sample. Preserve paused holds while waiting for it;
    // the poll runs here, so it releases their effectors, and only the clicks need undoing.
    let set_pending = frame.action_set != detection.mode.action_set()
        || semantic
            .as_ref()
            .is_some_and(|s| s.action_set != detection.mode.action_set());
    if set_pending {
        snatch::stand_down();
        if let Some(flight) = &mut state.flight {
            flight.reset();
        }
        clear_controller();
        weapons::hold::tick::publish_fire([false; 2]);
        sink.push(Segment {
            writes: state.tracker.pause(),
            tracked: false,
        });
        state.publish_recovery();
        state.report = TickReport {
            detection,
            ..Default::default()
        };
        return None;
    }
    let original = frame.clone();
    let original_edges = edges;
    routed.filter_raw(&mut frame, &mut edges);
    state
        .recovery
        .filter_observed(&original, &mut frame, &mut edges, &neutral_policy);
    state.publish_recovery();
    update_controller(
        Some(&frame),
        if state.recovery.ready(config.acting_hand()) {
            routed.button(MenuAction::PointerSelect.id(), config.acting_hand())
        } else {
            ButtonControl::default()
        },
        detection.mode == Mode::Ui,
        config.acting_hand(),
        state.recovery.ready(config.acting_hand()),
    );

    let dominant = config.acting_hand();
    let move_rotation = match config.locomotion.direction {
        LocomotionDirection::Head => 0.0,
        LocomotionDirection::OffHand => off_hand_rotation(&frame, dominant.other()),
    };
    // A game menu or the map pauses the weapon and vehicle handling rather than standing it down,
    // so that what the hands hold (a foregrip, a primed grenade, a physical control) is still held
    // when gameplay resumes.
    let in_menu = detection.mode == Mode::Ui;
    let (start, vehicle) = if in_menu {
        weapons::reload::cancel_extraction_on_standdown();
        (
            weapons::TickStart::default(),
            vehicles::TickVolumes::default(),
        )
    } else {
        (
            weapons::begin_tick(detection.mode),
            vehicles::begin_tick(detection.mode),
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
    if !in_menu {
        provided.extend(physical_hands::targets(
            active.as_ref().map_or(1.0, |a| a.world_scale),
            &mut interaction_scans,
        ));
    }
    let profiles = Hand::BOTH.map(|hand| frame.hand(hand).profile);
    let mut semantic_input = match &semantic {
        Some(semantic) => SemanticInput::from_frame(
            semantic,
            &semantic_edges,
            profiles,
            dominant,
            &config.bindings,
        ),
        None => SemanticInput::rest(
            SemanticSet::of(detection.mode.action_set()),
            profiles,
            dominant,
        ),
    };
    let raw_grapple_button = grapple_button(&semantic_input, detection.mode);
    routed.filter_semantic(&mut semantic_input);
    state.bindings.cancel_routed_gestures(&routed);
    let grapple_hand = dominant.other();
    // The hand's pulls act only while the grapple button is held.
    let mut grapple_held = grapple_button_held(&semantic_input, detection.mode);
    if !in_menu {
        // SAFETY: the game stage runs on the game thread before the world update.
        provided.extend(unsafe {
            snatch::targets(&snatch::OfferInput {
                hand: grapple_hand,
                detection: &detection,
                button_held: grapple_held,
                grip: ButtonControl {
                    down: frame
                        .hand(grapple_hand)
                        .buttons
                        .contains(Button::SqueezePress),
                    pressed: edges.pressed(grapple_hand, Button::SqueezePress),
                    released: edges.released(grapple_hand, Button::SqueezePress),
                },
                dt: real_dt,
                config: &config,
            })
        });
    }
    let targets = grab::tick::resolve(provided.targets, provided.notes, active.as_ref());
    let mut controls = Controls::from_frame(
        &frame,
        &edges,
        &config.bindings,
        dominant,
        move_rotation,
        targets.chosen(),
    );
    let catch_reserved = if in_menu {
        [false; 2]
    } else {
        snatch::reserved_hands()
    };
    for hand in Hand::BOTH {
        controls.hands[hand.index()].grab_slow = targets.slow[hand.index()];
        controls.hands[hand.index()].catch_reserved = catch_reserved[hand.index()];
    }
    controls.masked_hands = Hand::BOTH.map(|hand| !state.recovery.ready(hand));
    let gun_triggers = config.weapons.hold.either_hand;
    let gun_owners = weapons::capabilities::gun_owners_now();
    controls.armed = gun_owners.map(|gun| gun.is_some() && !in_menu);
    let trigger_owners = gun_owners.map(|gun| TriggerOwner {
        mode: detection.mode,
        gun: gun.filter(|_| gun_triggers && !in_menu),
    });
    let raw_triggers = Hand::BOTH.map(|hand| {
        let source = original.hand(hand);
        RawTrigger {
            button: ButtonControl {
                down: source.buttons.contains(Button::TriggerPress),
                pressed: original_edges.pressed(hand, Button::TriggerPress),
                released: original_edges.released(hand, Button::TriggerPress),
            },
            value: source.inputs.trigger,
            usable: source.active
                && source.grip.is_tracked()
                && !controls.masked_hands[hand.index()],
        }
    });
    let trigger_blocked = state.trigger_ownership.observe(
        trigger_owners,
        raw_triggers,
        config.bindings.trigger_deadzone,
    );
    let trigger_routes = semantic_input.clone();
    state.bindings.cancel_trigger_gestures(trigger_blocked);
    trigger_ownership::mask_routes(
        &mut semantic_input,
        detection.mode,
        trigger_ownership::masks(trigger_owners, trigger_blocked),
    );
    grapple_held = grapple_button_held(&semantic_input, detection.mode);
    for hand in Hand::BOTH {
        if trigger_blocked[hand.index()] {
            controls.hands[hand.index()].trigger_press = ButtonControl::default();
            controls.hands[hand.index()].trigger = 0.0;
        }
    }
    controls.weapon_cycle = config.weapons.holsters.stick_click_cycles;
    controls.pointer_hand = owns_menu_trigger();
    // The off-hand's trigger fires the gun it holds, in a mode whose triggers fire the hand weapons.
    let fires_guns = tables::triggers_fire_hand_weapons(detection.mode);
    let held = start.held.filter(|_| fires_guns).unwrap_or([false; 2]);
    controls.dual_wield = held[dominant.other().index()];
    controls.sight_chord = config.aim.sight_tuning.enabled && sights::tuning::lining_up();
    controls.ragdolling = mode::last_snapshot()
        .and_then(|snapshot| snapshot.character)
        .is_some_and(|character| character.has(StateBitFlag::SB_IN_RAGDOLL));
    let snatch_button_owned = state.bindings.claim_snatch(
        raw_grapple_button,
        snatch::can_claim_button() && config.weapons.snatch.sling,
        semantic.is_some()
            && !controls.masked_hands.contains(&true)
            && matches!(detection.mode, Mode::OnFoot(_)),
        neutral_policy.frame_neutral(&original) && source_neutral,
    );
    let line = line_attached() && !snatch_button_owned;
    let grapple_line = grapple_hand_line(grapple_hand);
    let yank = state.yank.step(
        real_dt,
        grapple_line,
        yank_armed_in(line, &detection) && grapple_held,
        &config.bindings.yank.detector(),
    );
    if let Some(yank) = yank {
        controls.yank = true;
        tracing::debug!(target: "controllers", ?yank, "yank");
        haptics::play(grapple_hand, Event::Yank);
    }
    // SAFETY: the game stage runs on the game thread before the world update.
    let physical_grapple = unsafe {
        grapple::physical::input_tick(&grapple::physical::TickInput {
            dt,
            real_dt,
            line: grapple_line,
            button_held: grapple_held && !snatch_button_owned,
            detection: &detection,
            config: &config,
        })
    };
    controls.grapple = GrappleContext {
        tethers: physical_grapple.tethers > 0,
        line: yank_armed_in(line, &detection),
        aim_target: aim::status().grapple.is_some_and(|g| g.valid),
        reeling: physical_grapple.lockable && !snatch_button_owned,
    };
    controls.lock_yank = physical_grapple.lock_yank;
    let mut step = state.bindings.step(
        detection.mode,
        &controls,
        &semantic_input,
        real_dt,
        &config.bindings,
    );
    // SAFETY: the game stage runs on the game thread before the world update.
    unsafe {
        grapple::physical::lock::lock_tick(&grapple::physical::lock::LockTickInput {
            button: step.grapple_lock,
            yank: physical_grapple.lock_yank,
            line: grapple_line,
            real_dt,
            hand: grapple_hand,
            config: &config.grapple.lock,
        })
    };
    let summary_masks = state
        .bindings
        .effective_masks(detection.mode, &controls, &semantic_input);
    let (_, summary_sources) = crate::vr::input::sources::table();
    let (summary_hold, summary_cancel) = crate::controllers::summary::hold::accepted(
        &semantic_input,
        &routed,
        &summary_masks,
        &frame,
        &summary_sources,
    );
    crate::controllers::summary::hold::publish(summary_hold, summary_cancel);
    let reach_volumes = targets.resolved.each_ref().map(|resolved| {
        resolved
            .reaches
            .iter()
            .map(|r| r.volume)
            .collect::<Vec<_>>()
    });
    let flight_tick = flight::Tick {
        frame: &original,
        config: &config,
        dimensions: &flight_dimensions,
        detection,
        ready: !controls.masked_hands.contains(&true),
        ragdoll: controls.ragdolling,
        // Tethers left in the world do not occupy a hand; only a held button or a live line does.
        grapple: grapple_held || line,
        grabs: step.grabs,
        reaches: reach_volumes.each_ref().map(Vec::as_slice),
        dt,
    };
    let flight_commands = flight::evaluate(
        state.flight.get_or_insert_with(Default::default),
        &flight_tick,
        &step.desired,
    );
    grab::tick::finish(&grab::tick::Finish {
        tick: &targets,
        latches: state.bindings.grip_latches(),
        grips: controls.hands.map(|h| h.grip),
        grabs: step.grabs,
        markers: active
            .as_ref()
            .filter(|_| config.overlay.enabled && config.overlay.markers && !in_menu)
            .map(marker_frame),
    });
    let view_frame = active
        .as_ref()
        .filter(|_| config.overlay.enabled && !in_menu)
        .map(marker_frame);
    physical_hands::grab_view::submit(&targets, view_frame.as_ref());
    if step.crouch_toggle {
        crate::controllers::roomscale::toggle_seated_crouch();
    }
    if step.sight_capture {
        sights::tuning::request_capture();
    }
    let interaction_context = physical_hands::pickup::diagnostics::input(
        frame.sequence,
        &targets,
        physical_hands::pickup::diagnostics::InputState {
            grips: controls.hands.map(|h| h.grip),
            latches: state.bindings.grip_latches(),
            grabs: step.grabs,
        },
        interaction_scans,
    );
    let summary_before = summary::BeforeFinish::capture(vehicles::parachute_holds());
    let mut trigger_changed = [false; 2];
    let mut belt_reload = None;
    if !in_menu {
        let finish = weapons::FinishInputs {
            grabs: grabs_for(step.grabs, Consumer::Weapons),
            grips: controls.hands.map(|h| h.grip),
            triggers: controls.hands.map(|h| h.trigger_press),
            fire_presses: gun_fire::presses(
                detection.mode,
                &semantic_input,
                controls.hands.map(|h| h.trigger_press),
                controls.masked_hands,
            ),
            unavailable: controls.masked_hands,
            reload_button: reload_button(&semantic_input),
            dt,
            real_dt,
        };
        belt_reload = weapons::finish_tick(&finish, &mut step.desired);
        let after = weapons::capabilities::gun_owners_now().map(|gun| TriggerOwner {
            mode: detection.mode,
            gun: gun.filter(|_| gun_triggers),
        });
        let changed = state.trigger_ownership.after_finish(after);
        trigger_changed = changed;
        if changed.contains(&true) {
            let fire = weapons::hold::tick::status().fire;
            weapons::hold::tick::publish_fire(trigger_ownership::retain_fire(fire, changed));
            for hand in Hand::BOTH {
                if changed[hand.index()] {
                    controls.hands[hand.index()].trigger_press = ButtonControl::default();
                    controls.hands[hand.index()].trigger = 0.0;
                }
            }
        }
        let wheel = weapons::wheel::input_tick(
            &weapons::wheel::TickInputs {
                mode: detection.mode,
                presses: step.wheel,
            },
            &mut step.desired,
        );
        // The stick that holds the wheel open is the turn stick: its sideways drift must not turn.
        if wheel.by_stick {
            step.body_turn = 0.0;
        }
        physical_hands::pickup::finish_tick(
            &weapons::FinishInputs {
                grabs: grabs_for(step.grabs, Consumer::Pickup),
                ..finish
            },
            &mut step.desired,
            &interaction_context,
        );
        if let Some(value) = physical_grapple.retract {
            step.desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: Action::RETRACT_GRAPPLE,
                },
                want: Want::Analog(value),
            });
        }
        let button_holds = snatch::ButtonHolds::routed(step.grapple_held, snatch_button_owned);
        // SAFETY: the game stage runs on the game thread before the world update.
        unsafe {
            snatch::finish_tick(
                &snatch::FinishInput {
                    hand: grapple_hand,
                    grabs: grabs_for(step.grabs, Consumer::Snatch),
                    grips: finish.grips,
                    latches: state.bindings.grip_latches(),
                    line: grapple_line,
                    ordinary_button_held: button_holds.ordinary,
                    modifier_held: button_holds.modifier,
                    detection: &detection,
                    dt,
                    real_dt,
                    config: &config,
                },
                &mut step.desired,
            )
        };
        if let Some((hand, volume)) = snatch::take_other_catch() {
            state.bindings.hold_catch(hand, volume);
        }
        physical_hands::climb::finish_tick(
            finish.grips.map(|g| g.held()),
            grabs_for(step.grabs, Consumer::Climb),
        );
        physical_hands::stunt::finish_tick(finish.grips.map(|g| g.held()), &mut step.desired);
        // The ripcord's canopy request goes in before the gestures commit, so it wins over a suit
        // request on the same tick.
        vehicles::ripcord_finish(
            detection.mode,
            grabs_for(step.grabs, Consumer::Vehicles),
            &controls,
            &mut step.desired,
        );
        let reserve_pitch = flight::commit(
            state
                .flight
                .as_mut()
                .expect("the gesture state was initialized"),
            flight_commands,
            &flight_tick,
            &mut step.desired,
        );
        vehicles::finish_tick(
            detection.mode,
            grabs_for(step.grabs, Consumer::Vehicles),
            &controls,
            &mut step.desired,
            dt,
            step.grapple_held,
            reserve_pitch,
        );
        // A committed step walks Rico through the game's animation; the drive sets his speed.
        if config.body.rig.steps.enabled {
            add_step_walk(&mut step.desired, config.body.rig.steps.walk_stick);
        }
        locomotion::strokes::tick(
            active.as_ref(),
            controls.armed,
            &config.locomotion.strokes,
            &mut step.desired,
            real_dt,
        );
    } else {
        weapons::hold::tick::publish_fire([false; 2]);
        weapons::wheel::pause();
        physical_hands::pickup::pause();
        snatch::stand_down();
    }
    let final_owners = weapons::capabilities::gun_owners_now().map(|gun| TriggerOwner {
        mode: detection.mode,
        gun: gun.filter(|_| gun_triggers && !in_menu),
    });
    let late_changed = state.trigger_ownership.after_finish(final_owners);
    trigger_changed =
        Hand::BOTH.map(|hand| trigger_changed[hand.index()] || late_changed[hand.index()]);
    state.bindings.cancel_trigger_gestures(trigger_changed);
    if trigger_changed.contains(&true) {
        let fire = weapons::hold::tick::status().fire;
        weapons::hold::tick::publish_fire(trigger_ownership::retain_fire(fire, trigger_changed));
    }
    trigger_ownership::suppress_changed_routes(
        &mut step.desired,
        detection.mode,
        &controls,
        &trigger_routes,
        trigger_changed,
    );
    let routes = summary::permitted_routes(
        &state.bindings,
        detection.mode,
        &controls,
        &semantic_input,
        &routed,
        trigger_ownership::masks(
            final_owners,
            Hand::BOTH.map(|hand| trigger_blocked[hand.index()] || trigger_changed[hand.index()]),
        ),
    );
    let mut eligibility = summary::capture(summary::Refresh {
        sequence: frame.sequence,
        belt_reload,
        parachute: vehicles::parachute_holds(),
        config: &config,
        owners: final_owners,
        ready: Hand::BOTH.map(|hand| !in_menu && state.recovery.ready(hand)),
        grapple_routes: summary::grapple_routes(
            &state.bindings,
            detection.mode,
            &controls,
            &semantic_input,
            &routed,
            trigger_ownership::masks(
                final_owners,
                Hand::BOTH
                    .map(|hand| trigger_blocked[hand.index()] || trigger_changed[hand.index()]),
            ),
        ),
        trigger_blocked: Hand::BOTH
            .map(|hand| trigger_blocked[hand.index()] || trigger_changed[hand.index()]),
        chosen: Hand::BOTH.map(|hand| {
            targets.chosen()[hand.index()].filter(|_| !controls.hands[hand.index()].grip.released)
        }),
        selected: Hand::BOTH.map(|hand| {
            targets.selected_for_summary()[hand.index()]
                .filter(|_| !controls.hands[hand.index()].grip.released)
        }),
        grabs: step.grabs,
        before: &summary_before,
        grapple: state.bindings.grapple_permissions(controls.grapple),
        yank: (yank_armed_in(line, &detection) && grapple_held).then_some(grapple_hand),
        flight: state
            .flight
            .as_ref()
            .map_or(Default::default(), flight::FlightState::current),
    });
    eligibility.routes = routes;
    crate::controllers::summary::refresh(detection.mode, &controls, &config, &eligibility);
    let writes = state.tracker.resolve(&step.desired, dt);
    sink.push(Segment {
        writes,
        tracked: true,
    });
    state.report = TickReport {
        detection,
        idle: None,
        body_turn: step.body_turn,
        writes: Vec::new(),
    };
    Some(step.body_turn)
}

/// Where the markers are placed for the hands of `active` at this tick.
fn marker_frame(active: &hands::ActiveHands) -> MarkerFrame {
    MarkerFrame {
        xf: CockpitTransform {
            head_world: headpose::query().position,
            head_cockpit: active.head.position,
            body: headpose::xr::body_rotation(),
            world_scale: active.world_scale,
        },
        drawn: active.hands,
    }
}

/// The last input tick's report.
pub fn last_report() -> TickReport {
    STATE.lock().report.clone()
}

/// The yank gesture's readout: how many yanks were detected, and the last one.
pub fn yank_status() -> (u64, Option<Yank>) {
    let state = STATE.lock();
    (state.yank.detector.count, state.yank.detector.last)
}

/// Dispatch accepted system commands even while gameplay injection is stopped.
fn system_command(command: SystemCommand) {
    match command {
        SystemCommand::Recenter => {
            let kind = roomscale::manual_recenter_kind();
            vr::request_recenter(kind);
            tracing::info!(target: "controllers", ?kind, "controller input: recentering the view");
            haptics::play_hands(Hand::BOTH, Event::Chord);
        }
        SystemCommand::TogglePanel => {
            let enabled = egui_panel::toggle();
            tracing::info!(target: "controllers", enabled, "controller input: toggled the debug panel");
            haptics::play(Hand::Right, Event::Chord);
        }
        SystemCommand::Mark => {
            tracing::info!(target: "controllers", "controller input: take marker");
            crate::recorder::session_recording::mark();
            haptics::play_hands(Hand::BOTH, Event::Chord);
        }
    }
}

fn policy_context(mode: Mode) -> u64 {
    let detail = match mode {
        Mode::OnFoot(variant) => variant as u64,
        Mode::Airborne(variant) => 2 + variant as u64,
        Mode::Vehicle(seat) => 5 + (seat.class as u64) * 2 + u64::from(seat.driver),
        Mode::Ui => 32,
    };
    (detail << 8) | mode.action_set() as u64
}

fn cancel_capture_routes(poll: DevicePoll, sink: &mut Vec<Segment>) {
    let (replay, publications) = history::take_gameplay();
    INPUT_POLICY
        .lock()
        .cancel_publications(replay, publications);
    PANEL_RECOVERY.lock().suspend();
    frame::take_edges();
    semantic_frame::take();
    clear_controller();
    update_egui_controller(
        None,
        Config::lock_query(|config| config.controllers.acting_hand()),
        Default::default(),
    );
    let mut state = STATE.lock();
    state.stand_down(poll, sink);
    state.recovery.disarm();
    state.publish_recovery();
    drop(state);
    weapons::stand_down();
    weapons::reload::cancel_extraction_on_standdown();
    vehicles::stand_down();
    grapple::physical::stand_down();
    snatch::stand_down();
    grab::tick::stand_down();
}

/// System ownership survives gameplay stand-down and editor open/close transitions.
static INPUT_POLICY: LazyLock<Mutex<Runtime>> = LazyLock::new(|| Mutex::new(Runtime::default()));
static PANEL_RECOVERY: Mutex<Recovery> = Mutex::new(Recovery::new());

static STATE: Mutex<State> = Mutex::new(State {
    poll: DevicePoll::Running,
    recovery: Recovery::new(),
    mask_log: MaskLog::new(),
    bindings: Bindings::new(),
    trigger_ownership: TriggerOwnership::new(),
    tracker: HoldTracker::new(),
    flight: None,
    yank: LineYank::new(),
    report: TickReport {
        detection: Detection {
            mode: mode::Mode::OnFoot(mode::OnFootVariant::Ground),
            traversal: None,
            held: None,
        },
        idle: None,
        body_turn: 0.0,
        writes: Vec::new(),
    },
});

struct State {
    /// The device poll as of the last tick, to log its changes.
    poll: DevicePoll,
    recovery: Recovery,
    mask_log: MaskLog,
    bindings: Bindings,
    trigger_ownership: TriggerOwnership,
    tracker: HoldTracker,
    flight: Option<flight::FlightState>,
    yank: LineYank,
    report: TickReport,
}

impl State {
    /// Stop injecting: forget every hold and latch, and return the writes that undo what the last
    /// tick wrote ([`HoldTracker::release_all`]).
    fn reset(&mut self, poll: DevicePoll) -> Vec<ActionWrite> {
        self.recovery.disarm();
        self.bindings.reset_preserving_snatch();
        self.trigger_ownership.suspend();
        if let Some(flight) = &mut self.flight {
            flight.reset();
        }
        self.yank = LineYank::new();
        self.tracker.release_all(poll)
    }

    /// [`reset`](Self::reset), pushing the writes that undo the last tick's to `sink`.
    fn stand_down(&mut self, poll: DevicePoll, sink: &mut Vec<Segment>) {
        sink.push(Segment {
            writes: self.reset(poll),
            tracked: false,
        });
    }

    /// Log a change of the device poll: while it is disabled the bindings stand down, which
    /// otherwise looks like broken bindings.
    fn observe_poll(&mut self, poll: DevicePoll) {
        if std::mem::replace(&mut self.poll, poll) == poll {
            return;
        }
        match poll {
            DevicePoll::Disabled => tracing::info!(
                target: "controllers",
                "the game's device poll is disabled; the bindings stand down and release their writes"
            ),
            DevicePoll::Running => tracing::info!(
                target: "controllers",
                "the game's device poll runs again; the bindings resume"
            ),
        }
    }

    /// Publish each hand's recovery for the debug UI, and log a hand that keeps waiting for its
    /// controls to rest and its rearm, rate-limited by [`MaskLog`]: a masked hand otherwise looks
    /// like a broken binding.
    fn publish_recovery(&mut self) {
        let status = self.recovery.status();
        publish_recovery_status(status);
        let events = self.mask_log.step(status, Instant::now());
        for (hand, event) in Hand::BOTH.into_iter().zip(events) {
            match event {
                Some(MaskEvent::StillMasked { waited, blocker }) => tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    ?waited,
                    ?blocker,
                    "hand input masked until its controls rest"
                ),
                Some(MaskEvent::Rearmed { waited }) => tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    ?waited,
                    "hand input rearmed"
                ),
                None => {}
            }
        }
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
    let head = headpose::xr::cockpit_pose().map(|pose| head_heading(pose.orientation));
    match (hand.flatten(), head.flatten()) {
        (Some(hand), Some(head)) => hand - head,
        _ => 0.0,
    }
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

/// Whether the grapple has a line of its own attached, as the mode detector's snapshot reads it.
fn line_attached() -> bool {
    mode::last_snapshot()
        .and_then(|s| s.character)
        .is_some_and(|c| c.grapple_wire)
}
