//! The Controllers tab: the scheme, the OpenXR input status, live readouts of the published
//! controller frame, the player's body dimensions, and every controller setting.

use std::time::Duration;

use egui::Ui;
use glam::{Quat, Vec3};

use crate::{
    config,
    controllers::{
        bindings::{self, Idle, TickReport, hold::Phase, inject::Outcome},
        body::{self, BodySource},
        config::{GunReadout, LocomotionDirection, Posture, SchemeSetting, SeatTransition},
        frame::{self, Hand, HandFrame, InteractionProfile, TrackedPose},
        mode::{self, Detection, ModeSnapshot},
        scheme,
    },
    headpose::config::VrTurnMode,
    vr::input::{self as vr_input, ActionsOrigin, HapticPulse},
};

pub fn egui_debug_controllers(ui: &mut Ui) {
    // Everything that locks the config is read before the config is locked for editing below.
    let current = scheme::current();
    let selected = scheme::selected();
    let status = vr_input::status();
    let frame = frame::latest();
    let body = body::player_body();
    let detection = mode::current();
    let snapshot = mode::last_snapshot();
    let report = bindings::last_report();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Status");
        ui.label(format!(
            "Scheme: {current:?} (auto-selection last chose {selected:?})"
        ));
        ui.label(format!("Action set: {:?}", vr_input::active_action_set()));
        ui.label(match (&status.error, status.attached, status.origin) {
            (Some(error), _, _) => format!("OpenXR input: unavailable: {error}"),
            (None, true, Some(origin)) => {
                format!("OpenXR input: attached ({})", origin_label(origin))
            }
            (None, _, _) => "OpenXR input: no session".to_owned(),
        });
        ui.label(format!(
            "Profiles: left {}, right {}",
            profile_label(status.profiles[0]),
            profile_label(status.profiles[1])
        ));
        let ht = status.hand_tracking;
        ui.label(format!(
            "Hand tracking: extension {}, system {}, trackers {}",
            yes_no(ht.extension_enabled),
            yes_no(ht.system_supported),
            yes_no(ht.trackers_created)
        ));

        ui.separator();
        egui::CollapsingHeader::new("Mode and bindings")
            .default_open(true)
            .show(ui, |ui| {
                mode_readout(ui, detection, snapshot.as_ref(), &report)
            });

        egui::CollapsingHeader::new("Live controller frame")
            .default_open(true)
            .show(ui, |ui| live_frame(ui, frame.as_ref()));

        egui::CollapsingHeader::new("Body dimensions").show(ui, |ui| body_readout(ui, &body));

        ui.separator();
        ui.heading("Settings");
        let mut cfg = config::CONFIG.lock();
        settings(ui, &mut cfg);
    });
}

fn live_frame(ui: &mut Ui, frame: Option<&frame::ControllerFrame>) {
    let Some(frame) = frame else {
        ui.label("No controller frame (no session, or the controllers are disabled).");
        return;
    };
    ui.label(format!(
        "Frame {} · display time {:.3} s · age {:.0} ms · focused {} · set {:?}",
        frame.sequence,
        frame.display_time_ns as f64 * 1e-9,
        frame.age().as_secs_f32() * 1000.0,
        yes_no(frame.focused),
        frame.action_set
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            ui.strong(hand.label());
            hand_readout(ui, hand, frame.hand(hand));
        }
    });
}

fn hand_readout(ui: &mut Ui, hand: Hand, h: &HandFrame) {
    ui.label(format!(
        "active {} · {}",
        yes_no(h.active),
        profile_label(h.profile)
    ));
    pose_readout(ui, "grip", &h.grip);
    pose_readout(ui, "aim", &h.aim);
    let i = &h.inputs;
    ui.label(format!(
        "trigger {:.2} · squeeze {:.2} (force {:.2})",
        i.trigger, i.squeeze, i.squeeze_force
    ));
    ui.label(format!(
        "stick ({:+.2}, {:+.2}) · pad ({:+.2}, {:+.2}) force {:.2}",
        i.thumbstick.x, i.thumbstick.y, i.trackpad.x, i.trackpad.y, i.trackpad_force
    ));
    let buttons: Vec<&str> = h.buttons.iter().map(|b| b.label()).collect();
    ui.label(format!(
        "buttons: {}",
        if buttons.is_empty() {
            "none".to_owned()
        } else {
            buttons.join(", ")
        }
    ));
    ui.label(match &h.joints {
        Some(joints) => format!(
            "joints: {} of {} tracked",
            joints.iter().filter(|j| j.tracked).count(),
            joints.len()
        ),
        None => "joints: none".to_owned(),
    });
    if ui.button("Test pulse").clicked() {
        vr_input::request_haptic(
            hand,
            HapticPulse {
                duration: Duration::from_millis(80),
                frequency_hz: None,
                amplitude: 0.6,
            },
        );
    }
}

fn pose_readout(ui: &mut Ui, name: &str, p: &TrackedPose) {
    let flags = format!(
        "{}{}{}{}",
        if p.position_valid { "P" } else { "-" },
        if p.position_tracked { "p" } else { "-" },
        if p.orientation_valid { "O" } else { "-" },
        if p.orientation_tracked { "o" } else { "-" },
    );
    let (yaw, pitch, roll) = euler_deg(p.pose.orientation);
    ui.label(format!(
        "{name} [{flags}] {} · ypr ({yaw:+.0}, {pitch:+.0}, {roll:+.0})°",
        vec3(p.pose.position)
    ));
    let speed = |v: Option<Vec3>| v.map_or("n/a".to_owned(), |v| format!("{:.2}", v.length()));
    ui.label(format!(
        "  |v| {} m/s · |ω| {} rad/s",
        speed(p.linear_velocity),
        speed(p.angular_velocity)
    ));
}

fn body_readout(ui: &mut Ui, body: &body::PlayerBody) {
    ui.label(match &body.source {
        BodySource::File { path } => format!("From {}", path.display()),
        BodySource::Defaults { path: Some(path) } => {
            format!("Generic adult defaults (no {})", path.display())
        }
        BodySource::Defaults { path: None } => "Generic adult defaults".to_owned(),
        BodySource::Invalid { error } => format!("Generic adult defaults ({error})"),
    });
    let d = body.dimensions;
    egui::Grid::new("body_dimensions").show(ui, |ui| {
        for (name, value) in [
            ("Height", d.height),
            ("Eye height", d.eye_height),
            ("Shoulder height", d.shoulder_height),
            ("Shoulder width", d.shoulder_width),
            ("Arm span", d.arm_span),
            ("Upper arm", d.upper_arm),
            ("Forearm", d.forearm),
            ("Hand", d.hand),
        ] {
            ui.label(name);
            ui.label(format!("{value:.2} m"));
            ui.end_row();
        }
    });
    if ui
        .button("Reload body file")
        .on_hover_text("Re-read body.local.json beside the payload DLL.")
        .clicked()
    {
        body::reload();
    }
}

fn settings(ui: &mut Ui, cfg: &mut config::Config) {
    let c = &mut cfg.controllers;
    ui.checkbox(&mut c.enabled, "Enabled").on_hover_text(
        "Read controller input and publish the controller frame. Off, every controller subsystem \
         is inert and the gamepad scheme is in effect.",
    );
    ui.horizontal(|ui| {
        ui.label("Scheme");
        ui.selectable_value(&mut c.scheme, SchemeSetting::Auto, "Auto")
            .on_hover_text("Switch on the most recent input.");
        ui.selectable_value(&mut c.scheme, SchemeSetting::Controllers, "Controllers");
        ui.selectable_value(&mut c.scheme, SchemeSetting::Gamepad, "Gamepad");
    });
    ui.horizontal(|ui| {
        ui.label("Dominant hand");
        ui.selectable_value(&mut c.dominant_hand, Hand::Right, "Right");
        ui.selectable_value(&mut c.dominant_hand, Hand::Left, "Left");
    });

    egui::CollapsingHeader::new("Input").show(ui, |ui| {
        ui.add(
            egui::Slider::new(&mut c.input.trigger_press.press, 0.05..=1.0).text("Trigger press"),
        );
        ui.add(
            egui::Slider::new(&mut c.input.trigger_press.release, 0.0..=1.0)
                .text("Trigger release"),
        );
        ui.add(
            egui::Slider::new(&mut c.input.squeeze_press.press, 0.05..=1.0).text("Squeeze press"),
        );
        ui.add(
            egui::Slider::new(&mut c.input.squeeze_press.release, 0.0..=1.0)
                .text("Squeeze release"),
        );
        ui.add(egui::Slider::new(&mut c.input.stick_deadzone, 0.0..=0.9).text("Stick deadzone"));
        ui.checkbox(&mut c.input.hand_tracking, "Hand tracking")
            .on_hover_text("Locate the hand skeleton through XR_EXT_hand_tracking when offered.");
    });

    egui::CollapsingHeader::new("Bindings").show(ui, |ui| {
        let b = &mut c.bindings;
        ui.checkbox(&mut b.enabled, "Inject bindings").on_hover_text(
            "Map the controllers to the game's actions while the controller scheme is in effect.",
        );
        ui.add(egui::Slider::new(&mut b.move_deadzone, 0.0..=0.6).text("Move stick deadzone"));
        ui.add(egui::Slider::new(&mut b.trigger_deadzone, 0.0..=0.5).text("Trigger deadzone"));
        ui.add(egui::Slider::new(&mut b.long_press_s, 0.2..=1.5).text("Commlink long press (s)"));
        ui.add(egui::Slider::new(&mut b.evade_flick, 0.3..=1.0).text("Wingsuit evade flick"));
        ui.add(egui::Slider::new(&mut c.mode.debounce_ticks, 1..=6).text("Mode debounce (ticks)"));
    });

    egui::CollapsingHeader::new("Locomotion and turning").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Move relative to");
            ui.selectable_value(
                &mut c.locomotion.direction,
                LocomotionDirection::Head,
                "Head",
            );
            ui.selectable_value(
                &mut c.locomotion.direction,
                LocomotionDirection::OffHand,
                "Off-hand",
            );
        });
        // Turning is shared with the gamepad's look input (`headpose.vr_turn`).
        let turn = &mut cfg.headpose.vr_turn;
        ui.horizontal(|ui| {
            ui.label("Turn");
            ui.selectable_value(&mut turn.mode, VrTurnMode::Smooth, "Smooth");
            ui.selectable_value(&mut turn.mode, VrTurnMode::Snap, "Snap");
        });
        ui.add(egui::Slider::new(&mut turn.smooth_scale, 0.1..=3.0).text("Smooth turn scale"));
        ui.add(egui::Slider::new(&mut turn.snap_angle_deg, 10.0..=90.0).text("Snap angle (°)"));
    });

    let c = &mut cfg.controllers;
    egui::CollapsingHeader::new("Body and aiming").show(ui, |ui| {
        ui.checkbox(&mut c.aim_state.pin, "Aim-state pin");
        ui.checkbox(&mut c.aim_state.mental_state_cuts, "Mental-state cuts");
        ui.checkbox(&mut c.hands.ik, "Hand IK");
        ui.checkbox(&mut c.hands.late_update, "Render-time late update");
        ui.checkbox(&mut c.aim.gun, "Gun aim from the dominant hand");
        ui.checkbox(&mut c.aim.grapple, "Grapple aim from the off-hand");
        ui.add(egui::Slider::new(&mut c.aim.assist_strength, 0.0..=1.0).text("Aim assist"));
        ui.checkbox(&mut c.body.remap_head, "Remap head height")
            .on_hover_text("Moves the view for every player, controllers or not.");
        ui.checkbox(&mut c.body.remap_hands, "Remap hand reach");
        ui.checkbox(&mut c.fingers.enabled, "Finger curls");
    });

    egui::CollapsingHeader::new("Overlay").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Gun readout");
            ui.selectable_value(&mut c.overlay.gun_readout, GunReadout::None, "None");
            ui.selectable_value(&mut c.overlay.gun_readout, GunReadout::Dot, "Dot");
            ui.selectable_value(&mut c.overlay.gun_readout, GunReadout::Laser, "Laser");
        });
        ui.checkbox(&mut c.overlay.grapple_highlight, "Grapple target highlight");
        ui.checkbox(&mut c.overlay.grapple_beam, "Grapple beam");
        ui.checkbox(&mut c.overlay.debug_gizmos, "Controller gizmos");
    });

    egui::CollapsingHeader::new("Weapons").show(ui, |ui| {
        let w = &mut c.weapons;
        ui.checkbox(&mut w.holsters, "Holsters");
        ui.checkbox(&mut w.foregrip, "Foregrip");
        ui.checkbox(&mut w.dual_wield_split, "Dual-wield split");
        ui.checkbox(&mut w.reload_gesture, "Reload gesture");
        ui.checkbox(&mut w.grenade_throw, "Physical grenade throw");
        ui.checkbox(&mut w.melee, "Physical melee");
        ui.checkbox(&mut w.scopes, "Scope lenses");
        ui.checkbox(&mut w.recoil.enabled, "Recoil");
        ui.add_enabled_ui(w.recoil.enabled, |ui| {
            ui.indent("recoil", |ui| {
                ui.add(egui::Slider::new(&mut w.recoil.kick_scale, 0.0..=3.0).text("Kick"));
                ui.add(
                    egui::Slider::new(&mut w.recoil.spring_stiffness, 10.0..=2000.0)
                        .logarithmic(true)
                        .text("Spring stiffness"),
                );
                ui.add(
                    egui::Slider::new(&mut w.recoil.spring_damping_ratio, 0.1..=2.0)
                        .text("Damping ratio"),
                );
                ui.add(
                    egui::Slider::new(&mut w.recoil.two_hand_stiffness_scale, 1.0..=5.0)
                        .text("Two-hand stiffness"),
                );
                ui.add(
                    egui::Slider::new(&mut w.recoil.two_hand_kick_scale, 0.0..=1.0)
                        .text("Two-hand kick"),
                );
            });
        });
        ui.checkbox(&mut w.haptics.enabled, "Haptics");
        ui.add_enabled(
            w.haptics.enabled,
            egui::Slider::new(&mut w.haptics.intensity, 0.0..=1.0).text("Haptic intensity"),
        );
    });

    egui::CollapsingHeader::new("Posture and roomscale").show(ui, |ui| {
        let r = &mut c.roomscale;
        ui.horizontal(|ui| {
            ui.label("Posture");
            ui.selectable_value(&mut r.posture, Posture::Auto, "Auto");
            ui.selectable_value(&mut r.posture, Posture::Seated, "Seated");
            ui.selectable_value(&mut r.posture, Posture::Standing, "Standing");
            ui.selectable_value(&mut r.posture, Posture::Roomscale, "Roomscale");
        });
        ui.checkbox(&mut r.chase.enabled, "Roomscale chase");
        ui.add_enabled_ui(r.chase.enabled, |ui| {
            ui.indent("chase", |ui| {
                ui.add(egui::Slider::new(&mut r.chase.deadzone_m, 0.0..=0.5).text("Deadzone (m)"));
                ui.add(
                    egui::Slider::new(&mut r.chase.max_speed_mps, 0.5..=8.0)
                        .text("Max speed (m/s)"),
                );
            });
        });
        ui.checkbox(&mut r.crouch.visual, "Visual crouch");
        ui.add_enabled(
            r.crouch.visual,
            egui::Checkbox::new(&mut r.crouch.capsule, "Crouch capsule"),
        );
        ui.add(
            egui::Slider::new(&mut r.crouch.seated_drop_m, 0.1..=1.0)
                .text("Seated crouch drop (m)"),
        );
        ui.checkbox(&mut r.head_fade, "Head-through-geometry fade");
        ui.horizontal(|ui| {
            ui.label("Seat transition");
            ui.selectable_value(&mut r.seat_transition, SeatTransition::Instant, "Instant");
            ui.selectable_value(&mut r.seat_transition, SeatTransition::Fade, "Fade");
            ui.selectable_value(&mut r.seat_transition, SeatTransition::Ride, "Ride");
        });
    });

    egui::CollapsingHeader::new("Vehicles and flight").show(ui, |ui| {
        let v = &mut c.vehicles;
        ui.checkbox(&mut v.physical_controls, "Physical vehicle controls");
        ui.checkbox(&mut v.driver_weapons_by_hand, "Driver weapons aim by hand");
        ui.checkbox(&mut v.wingsuit_arm_steering, "Wingsuit arm steering");
        ui.checkbox(&mut v.parachute_arm_steering, "Parachute arm steering");
    });
}

fn mode_readout(
    ui: &mut Ui,
    detection: Detection,
    snapshot: Option<&ModeSnapshot>,
    report: &TickReport,
) {
    ui.label(format!(
        "Mode: {:?}{}{}",
        detection.mode,
        detection
            .traversal
            .map_or(String::new(), |t| format!(" · traversal {t:?}")),
        detection
            .held
            .map_or(String::new(), |h| format!(" · held ({h:?})"))
    ));
    if let Some(s) = snapshot {
        ui.label(format!(
            "UI signals: clock stopped {}, screen focus {}, mod panel {} · teleporting {}",
            yes_no(s.ui.clock_stopped),
            yes_no(s.ui.screen_focus),
            yes_no(s.ui.mod_panel),
            yes_no(s.teleporting)
        ));
        ui.label(match &s.character {
            Some(c) => format!(
                "State words {:016x} {:016x} {:016x} · vehicle {:?} · wire {} · reeled {}",
                c.state_bits[0],
                c.state_bits[1],
                c.state_bits[2],
                c.vehicle,
                yes_no(c.grapple_wire),
                yes_no(c.reeled_in)
            ),
            None => "Character: not readable".to_owned(),
        });
    }
    ui.separator();
    match report.idle {
        Some(idle) => {
            ui.label(format!("Injection idle: {}", idle_label(idle)));
        }
        None => {
            ui.label(format!(
                "Injecting · body turn {:+.2} · {} writes last tick",
                report.body_turn,
                report.writes.len()
            ));
        }
    }
    egui::Grid::new("controller_writes")
        .striped(true)
        .show(ui, |ui| {
            for w in &report.writes {
                ui.label(format!("{:?}", w.map));
                ui.label(format!("{:?}", w.action));
                ui.label(format!("{:.2}", w.value));
                ui.label(match w.phase {
                    Phase::Value => "value".to_owned(),
                    Phase::Clicked => "clicked".to_owned(),
                    Phase::Held { seconds } => format!("held {seconds:.2} s"),
                    Phase::Released { seconds } => format!("released after {seconds:.2} s"),
                });
                ui.label(match w.outcome {
                    Outcome::Applied => "applied",
                    Outcome::KeptPhysicalPress => "kept a physical press",
                    Outcome::Unresolved => "unresolved",
                });
                ui.end_row();
            }
        });
}

fn idle_label(idle: Idle) -> &'static str {
    match idle {
        Idle::Disabled => "the bindings are off",
        Idle::GamepadScheme => "the gamepad scheme is in effect",
        Idle::NoControllers => "no active controller",
        Idle::ModPanel => "the mod panel is up",
    }
}

fn origin_label(origin: ActionsOrigin) -> &'static str {
    match origin {
        ActionsOrigin::Created => "created this session",
        ActionsOrigin::RewrappedAndAttached => "re-wrapped, attached to a new session",
        ActionsOrigin::Rewrapped => "re-wrapped from the previous injection",
    }
}

fn profile_label(profile: InteractionProfile) -> &'static str {
    match profile {
        InteractionProfile::None => "none",
        InteractionProfile::ValveIndex => "Valve Index",
        InteractionProfile::OculusTouch => "Oculus Touch",
        InteractionProfile::Other => "other",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn vec3(v: Vec3) -> String {
    format!("({:+.2}, {:+.2}, {:+.2})", v.x, v.y, v.z)
}

/// Yaw, pitch, and roll in degrees, for OpenXR's axes (+Y up, -Z forward).
fn euler_deg(q: Quat) -> (f32, f32, f32) {
    let (yaw, pitch, roll) = q.to_euler(glam::EulerRot::YXZ);
    (yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees())
}
