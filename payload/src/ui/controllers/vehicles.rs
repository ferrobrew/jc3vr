//! Vehicles and flight: the seat and the physical controls it offers, what each hand holds, the
//! values the controls write, where the wheel and handlebars were placed from and the rendered
//! wheel's turn, the driving view's correction, the mounted gun's aim, the arm steering's pose and
//! output, the parachute handles' pulls and output, and the vehicle, driving-view, mounted-gun,
//! arm-steering, and handle settings.

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        vehicles::{
            self, VehiclesStatus,
            arm_steering::PitchInput,
            config::{
                ArmPitch, ArmSteeringConfig, CockpitButtonsConfig, DrivingViewConfig,
                FlightControlsConfig, HandlebarsSpec, LeverAxis, LeverSpec, MountedConfig,
                ParachuteHandlesConfig, PhysicalControlsConfig, StickSpec, VehiclesConfig,
                WheelSpec, WheelTracking,
            },
            driver_aim,
            driving_view::{self, DrivingViewStatus},
            fit::FitReadout,
            look_steer,
            physical::{ControlEvent, ControlKind},
            wheel::WheelFrame,
            wheel_render,
        },
    },
    ui::controllers::{vec3, vec3_drag},
};

/// Everything the section shows, read before the config is locked for editing.
pub(super) struct VehiclesReadout {
    status: VehiclesStatus,
    driving_view: DrivingViewStatus,
    driver_substitutions: u64,
    look_steer_suppressions: u64,
    wheel_turns: u64,
}

impl VehiclesReadout {
    pub(super) fn read() -> Self {
        Self {
            status: vehicles::status(),
            driving_view: driving_view::status(),
            driver_substitutions: driver_aim::substitutions(),
            look_steer_suppressions: look_steer::suppressions(),
            wheel_turns: wheel_render::turned(),
        }
    }
}

pub(super) fn vehicles_readout(ui: &mut Ui, readout: &VehiclesReadout) {
    let status = &readout.status;
    if !status.active {
        ui.label("Inactive: the bindings are not injecting.");
    }
    ui.label(match status.seat {
        Some(seat) => format!(
            "Seat: {:?}, {}",
            seat.class,
            if seat.driver {
                "driver"
            } else {
                "passenger or gunner"
            }
        ),
        None => "Seat: not in a vehicle".to_owned(),
    });
    ui.label(format!(
        "Physical controls offered: {}",
        if status.offered.is_empty() {
            "none".to_owned()
        } else {
            status
                .offered
                .iter()
                .map(|k| k.label())
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            ui.strong(hand.label());
            ui.label(format!(
                "holds: {}",
                status.holds[hand.index()].map_or("nothing", ControlKind::label)
            ));
            ui.label(format!(
                "in: {}",
                status.volumes[hand.index()].map_or("no volume", |v| v.label())
            ));
        }
    });

    let c = &status.controls;
    let o = &status.outputs;
    ui.label(format!(
        "Wheel {:+.0}° · bars turn {:+.0}° roll {:+.0}° · stick ({:+.2}, {:+.2}) m · collective {:+.2} · throttle {:+.2}",
        c.wheel_angle.to_degrees(),
        c.bars.x.to_degrees(),
        c.bars.y.to_degrees(),
        c.stick.x,
        c.stick.y,
        c.levers[0],
        c.levers[1],
    ));
    let axis = |v: Option<f32>| v.map_or("stick".to_owned(), |v| format!("{v:+.2}"));
    let pair = |v: Option<glam::Vec2>| {
        v.map_or("stick".to_owned(), |v| {
            format!("({:+.2}, {:+.2})", v.x, v.y)
        })
    };
    ui.label(format!(
        "Written: steer {} · lean {} · cyclic {} · collective {} · flight stick {} · rudder {} · throttle {}{}",
        axis(o.steer),
        axis(o.lean),
        pair(o.cyclic),
        axis(o.collective),
        pair(o.flight_stick),
        axis(o.rudder),
        axis(o.throttle),
        if o.afterburner { " (afterburner)" } else { "" },
    ));
    ui.label(format!(
        "Stick and lever cues {} (last {}) · afterburner gate passed {} · horn {} · nitrous {} · buttons worked {}",
        status.cues,
        status.last_cue.map_or("none", |c| c.label()),
        status.afterburner_lights,
        o.horn,
        o.nitrous,
        status.button_presses,
    ));
    wheel_readout(ui, status, readout.wheel_turns);
    driving_view_readout(ui, &readout.driving_view);
    for event in status.events.iter().rev() {
        ui.label(match event {
            ControlEvent::Button(event) => format!(
                "{} {} the {}",
                event.hand().label(),
                if event.engages() {
                    "worked"
                } else {
                    "let go of"
                },
                event.what(),
            ),
            ControlEvent::Grabbed(hand, kind) => {
                format!("{} grabbed the {}", hand.label(), kind.label())
            }
            ControlEvent::Released(hand, kind, why) => format!(
                "{} let go of the {} ({})",
                hand.label(),
                kind.label(),
                why.label()
            ),
        });
    }

    ui.separator();
    match &status.mounted {
        Some(m) => {
            ui.label(format!(
                "Mounted gun at {} · barrel {} · handles {} / {}",
                vec3(m.gun.pivot),
                vec3(m.gun.barrel),
                m.gun.handles[0].map_or("none".to_owned(), vec3),
                m.gun.handles[1].map_or("none".to_owned(), vec3),
            ));
            ui.label(format!(
                "Hands aim along {} at {}",
                m.direction.map_or("nothing".to_owned(), vec3),
                m.target.map_or("nothing".to_owned(), vec3),
            ));
            ui.label(format!(
                "Handles held: left {} · right {} · grabs and releases {} · kicks {}",
                status.mounted_holds[0],
                status.mounted_holds[1],
                status.mounted_grip_events,
                vehicles::mounted_recoil::shots(),
            ));
        }
        None => {
            ui.label("Mounted gun: none");
        }
    }
    ui.label(format!(
        "Driver aim points replaced by the hand: {}",
        readout.driver_substitutions
    ));

    ui.separator();
    match &status.arms {
        Some(a) => {
            ui.label(format!(
                "Arms: spread {:.0}% of the arm span (steers from {:.0}%) · bank {:+.2} m (left \
                 above right) · height {:+.2} m above the shoulders at {:+.2} m · wrist tilt \
                 {:+.0}° (leading edges down) · sweep {:+.2} m ahead of the shoulders",
                a.pose.spread * 100.0,
                a.needed_spread * 100.0,
                a.pose.bank_m,
                a.pose.height_m,
                a.shoulder_y,
                a.pose.tilt.to_degrees(),
                a.pose.sweep_m,
            ));
            ui.label(format!(
                "Pitch input: {}",
                match a.pitch {
                    PitchInput::Height(m) => format!("height {m:+.2} m"),
                    PitchInput::TiltDeg(deg) => format!("tilt {deg:+.0}° from the neutral"),
                    PitchInput::Sweep(m) => format!("sweep {m:+.2} m"),
                }
            ));
            ui.label(
                "Spread both arms out to the sides to fly by them: lower one hand to bank \
                 toward it, and pitch by the configured measure. The stick still adds.",
            );
            ui.label(format!(
                "Arm steer {} · stick ({:+.2}, {:+.2}) · written ({:+.2}, {:+.2})",
                a.arms.map_or("not spread".to_owned(), |v| format!(
                    "({:+.2}, {:+.2})",
                    v.x, v.y
                )),
                a.stick.x,
                a.stick.y,
                a.written.x,
                a.written.y
            ));
        }
        None => {
            ui.label("Arm steering: not acting");
        }
    }
    match &status.handles {
        Some(h) => {
            let side = |i: usize| {
                if h.held[i] {
                    format!("pulled {:.0}%", h.pulls[i] * 100.0)
                } else {
                    "free".to_owned()
                }
            };
            ui.label(format!(
                "Parachute handles: left {} · right {}",
                side(Hand::Left.index()),
                side(Hand::Right.index())
            ));
            ui.label(format!(
                "Handle steer {} · written {}",
                h.handles
                    .map_or("none".to_owned(), |v| format!("({:+.2}, {:+.2})", v.x, v.y)),
                h.written.map_or("the stick's".to_owned(), |v| format!(
                    "({:+.2}, {:+.2})",
                    v.x, v.y
                )),
            ));
        }
        None => {
            ui.label("Parachute handles: not acting");
        }
    }
    ui.label(format!(
        "Wingsuit look-steer removals: {}",
        readout.look_steer_suppressions
    ));
}

/// Where the wheel and the handlebars were placed from, the wheel as placed, and the rendered
/// wheel's turn.
fn wheel_readout(ui: &mut Ui, status: &VehiclesStatus, turns: u64) {
    let Some(fit) = &status.fit else {
        return;
    };
    fit_readout(ui, fit);
    if let Some(wheel) = &status.wheel {
        placed_wheel(ui, wheel);
    }
    ui.label(match status.rendered_angle {
        Some(angle) => format!(
            "Rendered wheel turned {:+.0}° ({turns} render updates turned it)",
            angle.to_degrees()
        ),
        None => format!("Rendered wheel: not turned ({turns} render updates turned it)"),
    });
}

fn fit_readout(ui: &mut Ui, fit: &FitReadout) {
    ui.label(format!(
        "Vehicle type hash {:08x} · wheel part {} · fork part {}",
        fit.vehicle_type_hash,
        fit.wheel_part.as_deref().unwrap_or("none"),
        fit.fork_part.as_deref().unwrap_or("none"),
    ));
    ui.label(format!("Wheel from {}", fit.source.label()));
    if fit.fallback_latched {
        ui.label("The configured wheel is held in the vehicle's space.");
    }
    if let Some(w) = &fit.part_wheel {
        ui.label(format!(
            "Part-space hub {} · radius {:.3} m · tilt {:.0}°",
            vec3(w.hub),
            w.radius,
            w.tilt.to_degrees()
        ));
    }
    let seat = &fit.seat;
    ui.label(format!(
        "Driving position: hub from the character's eyes {} · from your head {}",
        seat.hub_from_eyes.map_or("unknown".to_owned(), vec3),
        vec3(seat.hub_from_head),
    ));
    ui.label(match &seat.correction {
        Some(c) => format!(
            "Measured hub from the eyes {} · {:.0} cm off the standard place {} · correction {} \
             ({})",
            vec3(c.hub_from_eyes),
            c.wanted.length() * 100.0,
            vec3(c.wanted),
            vec3(c.applied),
            c.origin.label(),
        ),
        None => "Seat correction: not measured (the fallback wheel, or the driver has not \
                 settled yet)"
            .to_owned(),
    });
    ui.label(format!(
        "Handlebars from {}",
        if fit.bars_from_vehicle {
            "the vehicle's steering fork"
        } else {
            "the configured handlebars"
        }
    ));
}

/// The correction the view is composed with, and the one it eases toward.
fn driving_view_readout(ui: &mut Ui, status: &DrivingViewStatus) {
    if status.target.is_none() && status.offset == glam::Vec3::ZERO {
        return;
    }
    ui.label(format!(
        "Driving view: moved {} ({:.0} cm), easing to {}{}",
        vec3(status.offset),
        status.offset.length() * 100.0,
        vec3(status.goal),
        if status.target.is_some() {
            ""
        } else {
            " · no seat correction published"
        },
    ));
}

fn placed_wheel(ui: &mut Ui, wheel: &WheelFrame) {
    let tilt = wheel.up.z.atan2(wheel.up.y).to_degrees();
    ui.label(format!(
        "Cockpit hub {} · radius {:.3} m · tilt {:.0}° from the cockpit's vertical",
        vec3(wheel.centre),
        wheel.radius,
        -tilt
    ));
}

pub(super) fn vehicles_settings(ui: &mut Ui, v: &mut VehiclesConfig) {
    ui.checkbox(&mut v.driver_weapons_by_hand, "Driver weapons aim by hand");
    ui.label("The wingsuit's head look-steer removal follows the parachute's movement setting.");
    egui::CollapsingHeader::new("Driving view")
        .show(ui, |ui| driving_view_settings(ui, &mut v.driving_view));
    egui::CollapsingHeader::new("Mounted guns").show(ui, |ui| mounted_settings(ui, &mut v.mounted));
    egui::CollapsingHeader::new("Physical controls")
        .show(ui, |ui| physical_settings(ui, &mut v.physical));
    egui::CollapsingHeader::new("Arm steering (wingsuit)")
        .show(ui, |ui| arm_settings(ui, &mut v.arm_steering));
    egui::CollapsingHeader::new("Parachute handles")
        .show(ui, |ui| handle_settings(ui, &mut v.parachute_handles));
}

fn driving_view_settings(ui: &mut Ui, d: &mut DrivingViewConfig) {
    ui.checkbox(
        &mut d.enabled,
        "Move the view per vehicle to a standard place behind the wheel",
    );
    vec3_drag(ui, "hub from the eyes (m)", &mut d.hub_from_eyes_m, 0.005);
    ui.add(
        egui::Slider::new(&mut d.max_m, 0.0..=0.5)
            .text("longest correction (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut d.blend_s, 0.0..=3.0)
            .text("ease (s)")
            .fixed_decimals(1),
    );
    ui.checkbox(&mut d.boats, "Boats too");
}

fn mounted_settings(ui: &mut Ui, m: &mut MountedConfig) {
    ui.checkbox(&mut m.aim_by_hands, "Aim by the hands on the handles");
    ui.checkbox(&mut m.anchor_hands, "Hands on the handles");
    ui.add(
        egui::Slider::new(&mut m.min_lever_m, 0.1..=1.5)
            .text("minimum lever (m)")
            .fixed_decimals(2),
    );
    ui.checkbox(&mut m.grip_handles, "Take the handles with the grip");
    ui.add(
        egui::Slider::new(&mut m.grab_radius_m, 0.03..=0.3)
            .text("handle grab radius (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut m.release_distance_m, 0.1..=0.6)
            .text("handle release distance (m)")
            .fixed_decimals(2),
    );
    ui.checkbox(&mut m.recoil, "Kick the handles with each shot");
    ui.add(
        egui::Slider::new(&mut m.kick_back_m, 0.0..=0.08)
            .text("kick back (m)")
            .fixed_decimals(3),
    );
    ui.add(
        egui::Slider::new(&mut m.kick_dip_m, 0.0..=0.04)
            .text("kick down (m)")
            .fixed_decimals(3),
    );
}

fn physical_settings(ui: &mut Ui, p: &mut PhysicalControlsConfig) {
    ui.add(
        egui::Slider::new(&mut p.grab_radius_m, 0.02..=0.25)
            .text("grab radius (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut p.release_distance_m, 0.1..=0.6)
            .text("release distance (m)")
            .fixed_decimals(2),
    );
    ui.checkbox(
        &mut p.from_vehicle,
        "Place the wheel and handlebars where the vehicle's are",
    );
    ui.checkbox(
        &mut p.animate_wheel,
        "Turn the vehicle's wheel with the steering",
    );
    ui.checkbox(&mut p.show_wheel, "Draw the wheel's rim and axis");
    egui::CollapsingHeader::new("Wheel (car, boat)").show(ui, |ui| wheel(ui, &mut p.wheel));
    egui::CollapsingHeader::new("Handlebars (motorcycle, ATV, jetski)")
        .show(ui, |ui| handlebars(ui, &mut p.handlebars));
    egui::CollapsingHeader::new("Cyclic (helicopter)").show(ui, |ui| stick(ui, &mut p.cyclic));
    egui::CollapsingHeader::new("Collective (helicopter)")
        .show(ui, |ui| lever(ui, &mut p.collective));
    egui::CollapsingHeader::new("Flight stick (plane)")
        .show(ui, |ui| stick(ui, &mut p.flight_stick));
    egui::CollapsingHeader::new("Throttle (plane)").show(ui, |ui| lever(ui, &mut p.throttle));
    egui::CollapsingHeader::new("Rudder and afterburner (plane)")
        .show(ui, |ui| flight(ui, &mut p.flight));
    egui::CollapsingHeader::new("Horn and nitrous by hand")
        .show(ui, |ui| buttons(ui, &mut p.buttons));
}

fn buttons(ui: &mut Ui, b: &mut CockpitButtonsConfig) {
    ui.checkbox(&mut b.horn, "Horn on the wheel's hub");
    ui.add(
        egui::Slider::new(&mut b.hub_share, 0.1..=0.6)
            .text("hub's share of the wheel's radius")
            .fixed_decimals(2),
    );
    ui.checkbox(&mut b.nitrous_button, "Nitrous button beside the wheel");
    vec3_drag(
        ui,
        "nitrous button from the rim (right, up, toward the driver; m)",
        &mut b.nitrous_from_rim_m,
        0.005,
    );
    ui.add(
        egui::Slider::new(&mut b.nitrous_radius_m, 0.02..=0.1)
            .text("nitrous button radius (m)")
            .fixed_decimals(3),
    );
    ui.add(
        egui::Slider::new(&mut b.press_depth_m, 0.005..=0.06)
            .text("press depth (m)")
            .fixed_decimals(3),
    );
    ui.checkbox(
        &mut b.twist_nitrous,
        "Twist the bars' right grip or the collective for nitrous",
    );
    ui.add(egui::Slider::new(&mut b.twist_deg, 10.0..=80.0).text("twist to burn (°)"));
}

fn flight(ui: &mut Ui, f: &mut FlightControlsConfig) {
    ui.checkbox(&mut f.twist_rudder, "Twist the stick for the rudder");
    ui.add(egui::Slider::new(&mut f.twist_deadzone_deg, 0.0..=20.0).text("twist deadzone (°)"));
    ui.add(egui::Slider::new(&mut f.twist_full_deg, 10.0..=90.0).text("full rudder twist (°)"));
    ui.checkbox(&mut f.afterburner_gate, "Afterburner gate on the throttle");
    ui.add(
        egui::Slider::new(&mut f.gate_at, 0.5..=0.98)
            .text("gate starts at (share of forward travel)")
            .fixed_decimals(2),
    );
}

fn wheel(ui: &mut Ui, w: &mut WheelSpec) {
    ui.checkbox(&mut w.enabled, "Enabled");
    vec3_drag(ui, "hub (m)", &mut w.centre_m, 0.005);
    ui.add(egui::Slider::new(&mut w.radius_m, 0.1..=0.3).text("radius (m)"));
    ui.add(egui::Slider::new(&mut w.tilt_deg, 0.0..=80.0).text("tilt (°)"));
    ui.add(egui::Slider::new(&mut w.lock_deg, 45.0..=540.0).text("full lock, either way (°)"));
    ui.add(
        egui::Slider::new(&mut w.response, 0.5..=3.0)
            .text("response (1 is linear, higher is gentler at centre)")
            .fixed_decimals(2),
    );
    ui.add(egui::Slider::new(&mut w.deadzone_deg, 0.0..=15.0).text("deadzone (°)"));
    ui.horizontal(|ui| {
        ui.label("turned by");
        for tracking in [WheelTracking::Twist, WheelTracking::Position] {
            ui.radio_value(&mut w.turned_by, tracking, tracking.label());
        }
    });
    ui.add(egui::Slider::new(&mut w.return_deg_per_s, 0.0..=1080.0).text("return (°/s)"));
}

fn handlebars(ui: &mut Ui, h: &mut HandlebarsSpec) {
    ui.checkbox(&mut h.enabled, "Enabled");
    vec3_drag(ui, "middle (m)", &mut h.centre_m, 0.005);
    ui.add(egui::Slider::new(&mut h.half_width_m, 0.15..=0.5).text("half width (m)"));
    ui.add(egui::Slider::new(&mut h.max_steer_deg, 5.0..=60.0).text("full steer (°)"));
    ui.add(egui::Slider::new(&mut h.max_lean_deg, 5.0..=60.0).text("full lean (°)"));
}

fn stick(ui: &mut Ui, s: &mut StickSpec) {
    ui.checkbox(&mut s.enabled, "Enabled");
    vec3_drag(ui, "top at rest (m)", &mut s.top_m, 0.005);
    ui.add(
        egui::Slider::new(&mut s.max_deflection_m, 0.03..=0.3)
            .text("full deflection (m)")
            .fixed_decimals(2),
    );
    ui.add(egui::Slider::new(&mut s.deadzone, 0.0..=0.3).text("deadzone"));
    ui.add(
        egui::Slider::new(&mut s.return_s, 0.0..=1.0)
            .text("centring time constant (s)")
            .fixed_decimals(2),
    );
}

fn lever(ui: &mut Ui, l: &mut LeverSpec) {
    ui.checkbox(&mut l.enabled, "Enabled");
    vec3_drag(ui, "neutral (m)", &mut l.rest_m, 0.005);
    ui.horizontal(|ui| {
        ui.label("travels");
        ui.radio_value(&mut l.axis, LeverAxis::Vertical, "up");
        ui.radio_value(&mut l.axis, LeverAxis::ForeAft, "forward");
    });
    ui.add(
        egui::Slider::new(&mut l.travel_m, 0.05..=0.6)
            .text("travel (m)")
            .fixed_decimals(2),
    );
    ui.add(egui::Slider::new(&mut l.detent, 0.0..=0.5).text("detent"));
    ui.checkbox(&mut l.hold_on_release, "Stays where it is let go");
}

fn arm_settings(ui: &mut Ui, a: &mut ArmSteeringConfig) {
    ui.checkbox(&mut a.wingsuit, "Wingsuit");
    ui.add(egui::Slider::new(&mut a.min_spread, 0.1..=1.0).text("spread to engage (of arm span)"));
    ui.add(
        egui::Slider::new(&mut a.deadzone_m, 0.0..=0.2)
            .text("deadzone (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut a.full_bank_m, 0.1..=1.0)
            .text("full bank (m)")
            .fixed_decimals(2),
    );
    ui.horizontal(|ui| {
        ui.label("pitched by");
        for pitch in ArmPitch::ALL {
            ui.radio_value(&mut a.pitch_by, pitch, pitch.label());
        }
    });
    ui.add(
        egui::Slider::new(&mut a.full_tilt_deg, 5.0..=90.0)
            .text("full pitch by tilt (°)")
            .fixed_decimals(0),
    );
    ui.add(
        egui::Slider::new(&mut a.tilt_deadzone_deg, 0.0..=30.0)
            .text("tilt deadzone (°)")
            .fixed_decimals(0),
    );
    ui.add(
        egui::Slider::new(&mut a.tilt_neutral_deg, -45.0..=45.0)
            .text("neutral tilt (°, leading edges down)")
            .fixed_decimals(0),
    );
    ui.add(
        egui::Slider::new(&mut a.full_pitch_m, 0.1..=1.0)
            .text("full pitch by height (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut a.full_sweep_m, 0.1..=1.0)
            .text("full pitch by sweep (m)")
            .fixed_decimals(2),
    );
    ui.checkbox(&mut a.invert_pitch, "Invert the pitch");
    ui.checkbox(
        &mut a.show_guide,
        "Ring the hands, lit while the arms steer, with a line between them",
    );
    ui.checkbox(
        &mut a.show_readout,
        "Show the arm steering's readout in the headset",
    );
}

fn handle_settings(ui: &mut Ui, h: &mut ParachuteHandlesConfig) {
    ui.checkbox(&mut h.enabled, "Enabled");
    vec3_drag(
        ui,
        "right handle from the shoulder (m)",
        &mut h.from_shoulder_m,
        0.005,
    );
    ui.add(
        egui::Slider::new(&mut h.radius_m, 0.03..=0.3)
            .text("grab radius (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut h.reach_m, 0.03..=0.5)
            .text("reach instead of the grapple (m)")
            .fixed_decimals(2),
    );
    ui.checkbox(&mut h.show_handles, "Mark the handles");
    ui.add(
        egui::Slider::new(&mut h.full_pull_m, 0.1..=0.6)
            .text("full pull (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut h.deadzone_m, 0.0..=0.15)
            .text("deadzone (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut h.brake_threshold, 0.0..=0.9)
            .text("brake threshold (of a full pull)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut h.release_distance_m, 0.1..=0.6)
            .text("release distance (m)")
            .fixed_decimals(2),
    );
}
