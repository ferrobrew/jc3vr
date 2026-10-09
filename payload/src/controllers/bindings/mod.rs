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
//!   held;
//! - `recovery` masks a hand after a loss of its pose or of injection until its controls rest
//!   ([`HandRecovery`]): its raw controls, and every semantic action on it;
//! - [`inject`] applies the writes to the player and UI action maps;
//! - [`panel_chord`] reads the debug panel chord, which acts even while the bindings stand down.
//!
//! [`input_tick`] runs from the `InputDeviceManager::Update` post-call detour
//! ([`crate::hooks::input::look`]), after the device poll and before the game reads its actions.
//! It injects only while the bindings are enabled, the controller scheme is in effect, a controller
//! is active, the game's device poll runs, and the mod's panel is down; otherwise it writes nothing,
//! so a real gamepad under the gamepad scheme is never fought. The right stick's body turn is not an
//! action: it is returned to the look detour, which feeds it to the headpose's VR turn scheme.
//!
//! Whenever injection stops, the bindings undo what they wrote rather than leave it to the device
//! poll ([`HoldTracker::release_all`]): the poll does not run while the device manager is disabled,
//! and an effector left clicked there reads as a fresh click on every frame.

pub mod controls;
pub mod grapple_button;
pub mod hold;
pub mod inject;
pub mod intents;
pub mod panel_chord;
mod recovery;
pub mod tables;
pub mod yank;

use std::time::Instant;

use jc3gi::input::input_action_map::Action;
use parking_lot::Mutex;

pub use crate::controllers::bindings::recovery::{Blocker, HandRecovery};
use crate::{
    config::Config,
    controllers::{
        aim::{self, sights},
        bindings::{
            controls::{ButtonControl, Controls, yaw_of},
            grapple_button::GrappleContext,
            hold::{ActionWrite, Desired, DevicePoll, HoldTracker, MapKind, Phase, Target, Want},
            inject::{Landing, Outcome},
            intents::SemanticInput,
            panel_chord::{ChordInput, ChordTick, PanelChord},
            recovery::{MaskEvent, MaskLog, NeutralPolicy, Recovery},
            tables::Bindings,
            yank::{Yank, YankConfig, YankDetector},
        },
        config::LocomotionDirection,
        frame::{self, ControllerFrame, Hand},
        grab::{self, Consumer, Provided, grabs_for, markers::MarkerFrame},
        hands::{self, tiers},
        haptics::{self, Event},
        mode::{self, Detection, Mode},
        physical_hands, roomscale, scheme, summary,
        vehicles::{self, wheel::CockpitTransform},
        weapons::{
            self,
            dual_wield::{self, TriggerInput, TriggerTick},
        },
    },
    grapple, headpose,
    hud::{
        egui_panel,
        pointer::{clear_controller, owns_menu_trigger, update_controller, update_egui_controller},
    },
    vr::{
        self,
        input::semantic::{SemanticSet, frame as semantic_frame, vocabulary::OnFootAction},
    },
};

/// What the input tick hands back to the look detour.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct InputTickOutput {
    /// The controllers' body-turn input for the VR turn scheme, `-1..=1`, positive turning right,
    /// or `None` when the controllers are not injecting.
    pub body_turn: Option<f32>,
}

/// Why the bindings did not inject on a tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Idle {
    /// The bindings toggle is off.
    Disabled,
    /// The gamepad scheme is in effect.
    GamepadScheme,
    /// No controller frame, or no active controller.
    NoControllers,
    /// The game's device manager is disabled, so its device poll does not run.
    PollDisabled,
    /// The mod's panel is up.
    ModPanel,
}

/// One injected write, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReportedWrite {
    pub map: MapKind,
    pub action: Action,
    pub value: f32,
    pub phase: Phase,
    pub outcome: Outcome,
}

/// The last input tick, for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickReport {
    pub detection: Detection,
    /// Why nothing was injected, when nothing was.
    pub idle: Option<Idle>,
    pub body_turn: f32,
    pub writes: Vec<ReportedWrite>,
}

/// Run the controllers' part of the input tick: detect the mode (which selects the action set),
/// then evaluate and inject the bindings. `dt` is the engine's tick delta, seconds, and `poll` is
/// whether the device poll that preceded the tick ran.
pub(crate) fn input_tick(dt: f32, poll: DevicePoll) -> InputTickOutput {
    let (config, neutral_policy) = Config::lock_query(|c| {
        (
            c.controllers,
            NeutralPolicy::new(
                &c.controllers.input,
                &c.controllers.bindings,
                &c.headpose.vr_turn,
                c.controllers.acting_hand(),
            ),
        )
    });
    if !config.enabled {
        clear_controller();
        update_egui_controller(None, config.acting_hand());
        let mut state = STATE.lock();
        state.observe_poll(poll);
        state.stand_down(poll);
        state.recovery.suspend();
        state.publish_recovery();
        frame::take_edges();
        semantic_frame::take();
        weapons::stand_down();
        vehicles::stand_down();
        grapple::physical::stand_down();
        grab::tick::stand_down();
        return InputTickOutput::default();
    }
    let detection = mode::tick();
    tiers::tick(dt);
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { hands::anchor::input_tick(&config) };

    let frame = frame::latest()
        .filter(ControllerFrame::any_active)
        .filter(|frame| frame.focused);
    let idle = if !config.bindings.enabled {
        Some(Idle::Disabled)
    } else if !scheme::is_controllers() {
        Some(Idle::GamepadScheme)
    } else if frame.is_none() {
        Some(Idle::NoControllers)
    } else if poll == DevicePoll::Disabled {
        Some(Idle::PollDisabled)
    } else if mode::last_snapshot().is_some_and(|s| s.ui.mod_panel) {
        Some(Idle::ModPanel)
    } else {
        None
    };
    // The edges are taken even while idle, so that a press from before injection resumed is not
    // replayed then.
    let mut edges = frame::take_edges();
    let (semantic, mut semantic_edges) = semantic_frame::take();
    let chord = panel_chord_tick(
        frame
            .as_ref()
            .filter(|_| config.bindings.enabled)
            .map(|frame| ChordInput::of(frame, &edges))
            .unwrap_or_default(),
        dt,
        config.bindings.panel_chord_window_s,
    );
    // The debug panel's pointer reads the controllers while the bindings stand down for the
    // panel, so it is stepped here, with the chord's presses masked so that the B press that
    // opens or closes the panel never right-clicks on it.
    let panel_input = frame.clone().map(|mut frame| {
        let mut edges = edges;
        panel_chord::mask(&mut frame, &mut edges, chord);
        (frame, edges)
    });
    update_egui_controller(
        panel_input.as_ref().map(|(frame, edges)| (frame, edges)),
        config.acting_hand(),
    );

    let mut state = STATE.lock();
    state.observe_poll(poll);
    if chord.toggle {
        state.bindings.cancel_stick_clicks();
    }
    let (Some(mut frame), None) = (frame, idle) else {
        clear_controller();
        let writes = state.stand_down(poll);
        state.recovery.suspend();
        state.publish_recovery();
        weapons::stand_down();
        vehicles::stand_down();
        grapple::physical::stand_down();
        grab::tick::stand_down();
        state.report = TickReport {
            detection,
            idle,
            writes,
            ..Default::default()
        };
        return InputTickOutput::default();
    };

    let tracking_lost = state.recovery.observe(&frame);
    let mut reported = Vec::new();
    if tracking_lost {
        clear_controller();
        reported = state.stand_down(poll);
        weapons::stand_down();
        vehicles::stand_down();
        grapple::physical::stand_down();
        grab::tick::stand_down();
        edges = Default::default();
        semantic_edges = Default::default();
    }
    // Mode selection precedes the next OpenXR sample. Preserve paused holds while waiting for it;
    // the poll runs here, so it releases their effectors, and only the clicks need undoing.
    let set_pending = frame.action_set != detection.mode.action_set()
        || semantic
            .as_ref()
            .is_some_and(|s| s.action_set != detection.mode.action_set());
    if set_pending {
        clear_controller();
        let paused = state.tracker.pause();
        // SAFETY: the input tick runs on the main thread after the device poll.
        let landings = unsafe { inject::apply(&paused) };
        reported.extend(report_writes(&paused, &landings));
        state.publish_recovery();
        state.report = TickReport {
            detection,
            writes: reported,
            ..Default::default()
        };
        return InputTickOutput::default();
    }
    state
        .recovery
        .filter(&mut frame, &mut edges, &neutral_policy);
    panel_chord::mask(&mut frame, &mut edges, chord);
    state.publish_recovery();
    if tracking_lost {
        clear_controller();
    } else {
        update_controller(
            Some(&frame),
            &edges,
            detection.mode == Mode::Ui,
            config.acting_hand(),
            state.recovery.ready(config.acting_hand()),
        );
    }

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
    if !in_menu {
        provided.extend(physical_hands::targets(
            active.as_ref().map_or(1.0, |a| a.world_scale),
        ));
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
    for hand in Hand::BOTH {
        controls.hands[hand.index()].grab_slow = targets.slow[hand.index()];
    }
    controls.masked_hands = Hand::BOTH.map(|hand| !state.recovery.ready(hand));
    controls.masked_inputs = panel_chord::masked_inputs(chord);
    let profiles = Hand::BOTH.map(|hand| frame.hand(hand).profile);
    let semantic_input = match &semantic {
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
    controls.weapon_cycle =
        !config.weapons.holsters.enabled || config.weapons.holsters.stick_click_cycles;
    controls.pointer_hand = owns_menu_trigger();
    // The hands whose triggers fire a pistol: those holding one, in a mode whose triggers fire.
    let fires_guns = dual_wield::triggers_fire_pistols(detection.mode);
    let pistols = start
        .dual_wield
        .filter(|_| fires_guns)
        .unwrap_or([false; 2]);
    // The off-hand's trigger fires the gun it holds: a pistol of the pair, or a weapon the hold
    // put in that hand.
    let held = start.held.filter(|_| fires_guns).unwrap_or([false; 2]);
    controls.dual_wield = pistols[dominant.other().index()] || held[dominant.other().index()];
    // A pistol the reload holds (its chamber or its magazine empty) fires nothing, so the split
    // never picks it; its trigger only clicks.
    let reload_held = weapons::reload::fire_held();
    controls.sight_chord = config.aim.sight_tuning.enabled && sights::tuning::lining_up();
    dual_wield::observe_triggers(start.dual_wield.map(|_| TriggerTick {
        dominant,
        triggers: Hand::BOTH.map(|hand| {
            let fires = pistols[hand.index()] && !reload_held[hand.index()];
            let trigger = &controls.hands[hand.index()].trigger_press;
            TriggerInput {
                held: fires && trigger.held(),
                pressed: fires && trigger.pressed,
            }
        }),
    }));
    let grapple_hand = dominant.other();
    let grapple_distance = grapple_hand_distance(grapple_hand, &config.bindings.yank);
    // The hand's pulls act only while the grapple button is held.
    let grapple_held = grapple_button_held(&semantic_input, detection.mode);
    let line = line_attached();
    let yank = state.yank.step(
        dt,
        grapple_distance,
        yank_armed_in(line, &detection) && grapple_held,
        &config.bindings.yank,
    );
    if let Some(yank) = yank {
        controls.yank = true;
        tracing::debug!(target: "controllers", ?yank, "yank");
        haptics::play(grapple_hand, Event::Yank);
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    let physical_grapple = unsafe {
        grapple::physical::input_tick(&grapple::physical::TickInput {
            dt,
            distance: grapple_distance,
            button_held: grapple_held,
            detection: &detection,
            config: &config,
        })
    };
    controls.grapple = GrappleContext {
        tethers: physical_grapple.tethers > 0,
        line: yank_armed_in(line, &detection),
        aim_target: aim::status().grapple.is_some_and(|g| g.valid),
    };
    let mut step = state.bindings.step(
        detection.mode,
        &controls,
        &semantic_input,
        dt,
        &config.bindings,
    );
    summary::refresh(detection.mode, &controls, &config, &frame);
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
    if step.crouch_toggle {
        crate::controllers::roomscale::toggle_seated_crouch();
    }
    if step.sight_capture {
        sights::tuning::request_capture();
    }
    if step.recenter {
        // The deferred recenter: the input tick may run while a frame holds the runtime lock.
        let kind = roomscale::manual_recenter_kind();
        vr::request_recenter(kind);
        tracing::info!(target: "controllers", ?kind, "stick click chord: recentering the view");
        chord_pulse(Hand::BOTH);
    }
    if !in_menu {
        let finish = weapons::FinishInputs {
            grabs: grabs_for(step.grabs, Consumer::Weapons),
            grips: controls.hands.map(|h| h.grip),
            triggers: controls.hands.map(|h| h.trigger_press),
            reload_button: reload_button(&semantic_input),
            dt,
        };
        weapons::finish_tick(&finish, &mut step.desired);
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
        vehicles::finish_tick(
            detection.mode,
            grabs_for(step.grabs, Consumer::Vehicles),
            &controls,
            &mut step.desired,
            dt,
            step.grapple_held,
        );
        physical_hands::pickup::finish_tick(
            &weapons::FinishInputs {
                grabs: grabs_for(step.grabs, Consumer::Pickup),
                ..finish
            },
            &mut step.desired,
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
        physical_hands::climb::finish_tick(
            finish.grips.map(|g| g.held()),
            grabs_for(step.grabs, Consumer::Climb),
        );
        physical_hands::stunt::finish_tick(finish.grips.map(|g| g.held()), &mut step.desired);
    } else {
        weapons::wheel::pause();
        physical_hands::pickup::pause();
    }
    let writes = state.tracker.resolve(&step.desired, dt);
    // SAFETY: the input tick runs on the main thread after the device poll.
    let landings = unsafe { inject::apply(&writes) };
    let effectors: Vec<_> = landings.iter().map(|l| l.effector).collect();
    state.tracker.landed(&writes, &effectors);
    reported.extend(report_writes(&writes, &landings));
    state.report = TickReport {
        detection,
        idle: None,
        body_turn: step.body_turn,
        writes: reported,
    };
    InputTickOutput {
        body_turn: Some(step.body_turn),
    }
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
        player: active.player_grips,
    }
}

/// The last input tick's report.
pub fn last_report() -> TickReport {
    STATE.lock().report.clone()
}

/// Each hand's input recovery at the last input tick, in [`Hand::index`] order. Safe to call with
/// the config locked: it takes only a lock that is never held while taking another.
pub fn recovery_status() -> [HandRecovery; 2] {
    *RECOVERY_STATUS.lock()
}

/// The yank gesture's readout: how many yanks were detected, and the last one.
pub fn yank_status() -> (u64, Option<Yank>) {
    let state = STATE.lock();
    (state.yank.count, state.yank.last)
}

/// Step the debug panel chord ([`panel_chord`]) and act on it. Runs on every input tick that has
/// the controllers enabled, whether or not the bindings inject, and takes no lock the bindings
/// hold.
fn panel_chord_tick(input: ChordInput, dt: f32, window_s: f32) -> ChordTick {
    let tick = PANEL_CHORD.lock().step(input, dt, window_s);
    if tick.toggle {
        let enabled = egui_panel::toggle();
        tracing::info!(
            target: "controllers",
            enabled,
            "debug panel chord: toggled the debug panel"
        );
        chord_pulse([panel_chord::B_HAND]);
    }
    tick
}

/// The short pulse that confirms a chord, on each of `hands`.
fn chord_pulse(hands: impl IntoIterator<Item = Hand>) {
    for hand in hands {
        haptics::play(hand, Event::Chord);
    }
}

/// The debug panel chord's latches, apart from [`STATE`] so that standing the bindings down does
/// not reset them.
static PANEL_CHORD: Mutex<PanelChord> = Mutex::new(PanelChord::new());

static RECOVERY_STATUS: Mutex<[HandRecovery; 2]> = Mutex::new([HandRecovery::Unusable; 2]);

static STATE: Mutex<State> = Mutex::new(State {
    poll: DevicePoll::Running,
    recovery: Recovery::new(),
    mask_log: MaskLog::new(),
    bindings: Bindings::new(),
    tracker: HoldTracker::new(),
    yank: YankDetector::new(),
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
    tracker: HoldTracker,
    yank: YankDetector,
    report: TickReport,
}

impl State {
    /// Stop injecting: forget every hold and latch, and return the writes that undo what the last
    /// tick wrote ([`HoldTracker::release_all`]).
    fn reset(&mut self, poll: DevicePoll) -> Vec<ActionWrite> {
        self.recovery.disarm();
        self.bindings = Bindings::new();
        self.yank = YankDetector::new();
        self.tracker.release_all(poll)
    }

    /// [`reset`](Self::reset), applying the writes that undo the last tick's, which are returned
    /// for the report.
    fn stand_down(&mut self, poll: DevicePoll) -> Vec<ReportedWrite> {
        let writes = self.reset(poll);
        // SAFETY: the input tick runs on the main thread after the device poll.
        let landings = unsafe { inject::apply(&writes) };
        report_writes(&writes, &landings)
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
        *RECOVERY_STATUS.lock() = status;
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

/// Pair each write with what became of it, for the report.
fn report_writes(writes: &[ActionWrite], landings: &[Landing]) -> Vec<ReportedWrite> {
    writes
        .iter()
        .zip(landings)
        .map(|(w, landing)| ReportedWrite {
            map: w.target.map,
            action: w.target.action,
            value: w.value,
            phase: w.phase,
            outcome: landing.outcome,
        })
        .collect()
}

/// The angle from the head's heading to the off-hand controller's, both in the cockpit frame, for
/// off-hand-relative locomotion. The game already moves relative to the view, which follows the
/// head, so rotating the stick by the difference makes its forward the controller's. Zero when
/// either heading is unknown.
fn off_hand_rotation(frame: &ControllerFrame, off: Hand) -> f32 {
    let aim = &frame.hand(off).aim;
    let hand = aim.orientation_valid.then(|| yaw_of(aim.pose.orientation));
    let head = headpose::xr::cockpit_pose().map(|pose| yaw_of(pose.orientation));
    match (hand.flatten(), head.flatten()) {
        (Some(hand), Some(head)) => hand - head,
        _ => 0.0,
    }
}

/// The reload action's press over the tick, on either hand, for the reload's eject tap. The
/// tables drive the game's reload action from the same press, which the game reads as a holster
/// when it is held.
fn reload_button(semantic: &SemanticInput) -> ButtonControl {
    semantic
        .get(OnFootAction::Reload.id())
        .map_or_else(ButtonControl::default, |input| {
            input
                .hands
                .iter()
                .fold(ButtonControl::default(), |acc, intent| ButtonControl {
                    down: acc.down || intent.button.down,
                    pressed: acc.pressed || intent.button.pressed,
                    released: acc.released || intent.button.released,
                })
        })
}

/// The grapple hand's distance from the body point a yank pulls toward, in the cockpit frame, or
/// `None` while the hand is untracked or the hands are inert.
fn grapple_hand_distance(hand: Hand, config: &YankConfig) -> Option<f32> {
    let tracked = frame::with_latest(|frame| {
        frame.focused && frame.hand(hand).active && frame.hand(hand).grip.is_tracked()
    })?;
    if !tracked {
        return None;
    }
    let active = hands::active_hands_raw()?;
    let grip = active.hands[hand.index()]?.grip.position;
    Some(grip.distance(yank::body_point(active.head.position, config)))
}

/// Whether the grapple has a line of its own attached, as the mode detector's snapshot reads it.
fn line_attached() -> bool {
    mode::last_snapshot()
        .and_then(|s| s.character)
        .is_some_and(|c| c.grapple_wire)
}

/// Whether the grapple button is held on either hand in `mode`.
fn grapple_button_held(semantic: &SemanticInput, mode: Mode) -> bool {
    tables::grapple_press(mode).is_some_and(|(actions, _)| {
        actions
            .iter()
            .filter_map(|&action| semantic.get(action))
            .any(|input| input.hands.iter().any(|h| h.button.held()))
    })
}

/// Whether a yank may fire, with `wire` whether a grapple line is live: the character is on foot or
/// airborne, and the line is not already carrying the character. During a reel-in or a hang the
/// grapple hand holds the line and drives the hang swing, and a pull on the line there is a swing,
/// not a yank. The caller also needs the grapple button held.
fn yank_armed_in(wire: bool, detection: &Detection) -> bool {
    wire && detection.traversal.is_none()
        && matches!(detection.mode, Mode::OnFoot(_) | Mode::Airborne(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::mode::{AirborneVariant, OnFootVariant, Traversal};

    #[test]
    fn the_yank_is_armed_only_on_a_free_line() {
        let on_foot = Detection {
            mode: Mode::OnFoot(OnFootVariant::Ground),
            ..Default::default()
        };
        assert!(yank_armed_in(true, &on_foot));
        assert!(!yank_armed_in(false, &on_foot));
        let falling = Detection {
            mode: Mode::Airborne(AirborneVariant::Falling),
            ..Default::default()
        };
        assert!(yank_armed_in(true, &falling));
        for traversal in [
            Traversal::PreReel,
            Traversal::ReelingIn,
            Traversal::PreHang,
            Traversal::ReeledIn,
        ] {
            let carried = Detection {
                traversal: Some(traversal),
                ..on_foot
            };
            assert!(!yank_armed_in(true, &carried), "{traversal:?}");
        }
        assert!(!yank_armed_in(
            true,
            &Detection {
                mode: Mode::Ui,
                ..on_foot
            }
        ));
    }
}
