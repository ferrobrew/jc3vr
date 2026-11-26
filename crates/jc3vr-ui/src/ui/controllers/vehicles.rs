//! Vehicles and flight: the vehicle rows, the seat and the physical controls it offers, what each
//! hand holds, the values the controls write, where the wheel and handlebars were placed from and
//! the rendered wheel's turn, the driving view's correction, the mounted gun's aim, the arm
//! steering's pose and output, the parachute handles' pulls and output, and the vehicle and air
//! settings.

use egui::Ui;
use jc3vr_gameplay::controllers::{
    vehicles,
    vehicles::{
        VehiclesStatus,
        arm_steering::PitchInput,
        config::VehiclesConfig,
        driver_aim, driving_view,
        driving_view::DrivingViewStatus,
        fit::FitReadout,
        look_steer, parachute_rig,
        physical::{ControlEvent, ControlKind, calibration},
        rail,
        ripcord::MIN_PULL_M,
        turret,
        wheel::WheelFrame,
        wheel_render, wingsuit_evade,
        wingsuit_evade::EvadeStatus,
    },
};
use jc3vr_input::frame::Hand;

use crate::{
    status::vehicles::{COCKPIT_BUTTONS, FLIGHT_CONTROLS, MOUNTED_GUN, PASSENGER_AIM},
    ui::{catalog::FeatureId, controllers::vec3, status_row::headlines},
};

/// Everything the section shows, read before the config is locked for editing.
pub(super) struct VehiclesReadout {
    status: VehiclesStatus,
    driving_view: DrivingViewStatus,
    driver_substitutions: u64,
    look_steer_suppressions: u64,
    hand_aimed_turret_ticks: u64,
    wheel_turns: u64,
    handles_unplaced: u64,
    evade: EvadeStatus,
}

impl VehiclesReadout {
    pub(super) fn read() -> Self {
        Self {
            status: vehicles::status(),
            driving_view: driving_view::status(),
            driver_substitutions: driver_aim::substitutions(),
            look_steer_suppressions: look_steer::suppressions(),
            hand_aimed_turret_ticks: turret::hand_aimed_ticks(),
            wheel_turns: wheel_render::turned(),
            handles_unplaced: parachute_rig::handles_unplaced(),
            evade: wingsuit_evade::status(),
        }
    }
}

pub(super) fn feature_readout(ui: &mut Ui, readout: &VehiclesReadout, id: FeatureId) {
    if !readout.status.active {
        ui.label("Inactive: the bindings are not injecting.");
    }
    match id {
        FeatureId::Vehicles => {
            headlines(
                ui,
                &[
                    &FLIGHT_CONTROLS,
                    &COCKPIT_BUTTONS,
                    &PASSENGER_AIM,
                    &MOUNTED_GUN,
                ],
            );
            seat_readout(ui, &readout.status);
            driving_view_readout(ui, &readout.driving_view);
            ui.label(format!(
                "Driver aim points replaced by the hand: {}",
                readout.driver_substitutions
            ));
            physical_readout(ui, readout);
            mounted_readout(ui, readout);
        }
        FeatureId::Air => {
            arms_readout(ui, readout);
            evade_readout(ui, &readout.evade);
            handles_readout(ui, readout);
            ui.label(match vehicles::ripcord_holder() {
                Some(hand) => format!("Ripcord: held by the {} hand", hand.label()),
                None => "Ripcord: free".to_owned(),
            });
        }
        _ => {}
    }
}

fn seat_readout(ui: &mut Ui, status: &VehiclesStatus) {
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
}

fn physical_readout(ui: &mut Ui, readout: &VehiclesReadout) {
    let status = &readout.status;
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
        "Wheel {:+.0}° · bars turn {:+.0}° · stick ({:+.2}, {:+.2}) m · collective {:+.2} · throttle {:+.2}",
        c.wheel_angle.to_degrees(),
        c.bars.to_degrees(),
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
    let grip = |v: Option<f32>| v.map_or("not held".to_owned(), |v| format!("{v:.2}"));
    ui.label(format!(
        "Bars' grips: throttle {} · brake {}",
        grip(o.bike_throttle),
        grip(o.bike_brake),
    ));
    ui.label(format!(
        "Stick and lever cues {} (last {}) · afterburner gate passed {}",
        status.cues,
        status.last_cue.map_or("none", |c| c.label()),
        status.afterburner_lights,
    ));
    wheel_readout(ui, status, readout.wheel_turns);
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
}

fn mounted_readout(ui: &mut Ui, readout: &VehiclesReadout) {
    if let Some(m) = &readout.status.mounted {
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
    }
}

fn arms_readout(ui: &mut Ui, readout: &VehiclesReadout) {
    let status = &readout.status;
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
    ui.label(format!(
        "Wingsuit look-steer removals: {}",
        readout.look_steer_suppressions
    ));
    ui.label(format!(
        "Vehicle gun ticks aimed by the hands: {}",
        readout.hand_aimed_turret_ticks
    ));
    let r = rail::status();
    ui.label(format!(
        "Helicopter skid: {}, {} entries, {} drops into the hang refused",
        if r.active { "standing" } else { "off" },
        r.entries,
        r.drops_refused
    ));
    if r.active {
        ui.label(format!(
            "Skid: {:.2} m off the line, walking {:.1} m/s, the update {} the proxy",
            r.off_line_m,
            r.walk_mps,
            if r.update_carried {
                "carried"
            } else {
                "did not carry"
            }
        ));
    }
    if let Some(reason) = r.last_release {
        ui.label(format!("Skid last let go: {reason}"));
    }
    let c = calibration::status();
    ui.horizontal(|ui| {
        if ui
            .button("Set the cockpit controls from the hands")
            .on_hover_text(
                "Press, close the panel, and sit at an aircraft's controls: a three-second \
                 countdown ticks on both hands. Hold the left hand where the collective or \
                 throttle should be and the right where the stick (or the yoke's right horn) \
                 should be. Both hands buzz when they are captured; the controls move there at \
                 once and are saved for this cockpit and seat.",
            )
            .clicked()
        {
            calibration::request();
        }
        if ui
            .add_enabled(
                c.current_calibrated,
                egui::Button::new("Clear this cockpit's calibration"),
            )
            .clicked()
        {
            calibration::clear_current();
        }
    });
    if c.waiting {
        ui.label(
            "Cockpit calibration: close the panel and sit at the controls to start the countdown",
        );
    }
    if let Some(countdown) = c.countdown_s {
        ui.label(format!(
            "Capturing the hands in {countdown:.1} s: left on the lever, right on the stick"
        ));
    }
    ui.label(format!(
        "Cockpit calibration: {} ({}), {} saved",
        c.current.as_deref().unwrap_or("no cockpit"),
        if c.current_calibrated {
            "calibrated"
        } else {
            "the table's"
        },
        c.entries
    ));
    if let Some(last) = &c.last {
        ui.label(format!("Cockpit calibration last: {last}"));
    }
    if let Some(path) = &c.path {
        ui.label(format!("Cockpit calibration file: {}", path.display()));
    }
}

fn evade_readout(ui: &mut Ui, evade: &EvadeStatus) {
    ui.label(format!(
        "Evade: arm bank {} · throws {} · turned away {}{}",
        evade
            .bank_deg
            .map_or("not read".to_owned(), |bank| format!("{bank:+.0}°")),
        evade.throws,
        evade.refusals,
        if evade.holding {
            " · writing the throw's direction"
        } else {
            ""
        },
    ));
    if let Some(throw) = evade.last_throw {
        ui.label(format!(
            "Last evade: {}, banked {:+.0}° after a {:.0}° swing",
            throw.direction, throw.bank_deg, throw.sweep_deg
        ));
    }
    if let Some((throw, blocked)) = evade.last_refusal {
        ui.label(format!(
            "Last turned away: {}, banked {:+.0}° ({blocked})",
            throw.direction, throw.bank_deg
        ));
    }
}

fn handles_readout(ui: &mut Ui, readout: &VehiclesReadout) {
    let status = &readout.status;
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
            ui.label(format!(
                "Rig handles put back on their cords: {}",
                readout.handles_unplaced
            ));
        }
        None => {
            ui.label("Parachute handles: not acting");
        }
    }
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
        ui.label("The fallback wheel is held in the vehicle's space.");
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
            "the fallback handlebars"
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

pub(super) fn feature_settings(ui: &mut Ui, v: &mut VehiclesConfig, id: FeatureId) {
    match id {
        FeatureId::Vehicles => vehicle_settings(ui, v),
        FeatureId::Air => air_settings(ui, v),
        _ => {}
    }
}

fn vehicle_settings(ui: &mut Ui, v: &mut VehiclesConfig) {
    ui.checkbox(&mut v.driver_weapons_by_hand, "Driver weapons aim by hand");
    ui.checkbox(&mut v.passenger_body_by_hand, "Passenger aims by hand")
        .on_hover_text(
            "In a passenger or stunt seat, turn the seated body toward the gun hand's aim point \
             instead of the gaze's.",
        );
    let p = &mut v.physical;
    ui.checkbox(
        &mut p.wheel.enabled,
        "Wheel (car, boat), with the horn on its hub",
    );
    ui.add_enabled(
        p.wheel.enabled,
        egui::Slider::new(&mut p.wheel.response, 0.5..=3.0)
            .text("Wheel response (1 is linear, higher is gentler at centre)")
            .fixed_decimals(2),
    );
    ui.checkbox(&mut p.handlebars, "Handlebars (motorcycle, ATV, jetski)");
    ui.add_enabled(
        p.handlebars,
        egui::Slider::new(&mut p.bars_steering.deadzone_deg, 0.0..=10.0)
            .text("Handlebars deadzone (°)")
            .fixed_decimals(1),
    );
    ui.add_enabled(
        p.handlebars,
        egui::Slider::new(&mut p.bars_steering.response, 0.5..=3.0)
            .text("Handlebars response (1 is linear, higher is gentler at centre)")
            .fixed_decimals(2),
    );
    ui.checkbox(
        &mut p.sticks,
        "Stick (helicopter cyclic, plane flight stick)",
    );
    ui.checkbox(
        &mut p.levers,
        "Lever (helicopter collective, plane throttle with its afterburner gate)",
    );
    ui.checkbox(&mut p.nitrous_button, "Nitrous button beside the wheel");
    ui.checkbox(
        &mut p.bars_twist,
        "Twist the bars' grips back: the right for the throttle, the left for the brake",
    );
    ui.checkbox(
        &mut p.twist_rudder,
        "Twist the flight stick or cyclic for the rudder and yaw",
    );
    ui.checkbox(
        &mut v.mounted.aim_by_hands,
        "Mounted guns aim by the hands on the handles",
    );
    ui.checkbox(
        &mut v.mounted.grip_handles,
        "Take the mounted gun's handles with the grip",
    );
}

fn air_settings(ui: &mut Ui, v: &mut VehiclesConfig) {
    let a = &mut v.arm_steering;
    ui.add(
        egui::Slider::new(&mut a.deadzone_m, 0.0..=0.2)
            .text("Wingsuit arm deadzone (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut a.full_bank_m, 0.1..=1.0)
            .text("Wingsuit full bank (m)")
            .fixed_decimals(2),
    );
    let e = &mut v.wingsuit_evade;
    ui.checkbox(
        &mut e.enabled,
        "Wingsuit evade: throw the arms into a steep bank to barrel-roll",
    )
    .on_hover_text(
        "With the boost wingsuit, bank the spread arms hard and fast to one side to roll that way; \
         the view rolls with the body. Off, the player's barrel rolls are dropped.",
    );
    ui.add(egui::Slider::new(&mut e.throw_bank_deg, 30.0..=85.0).text("Evade throw bank (°)"));
    ui.add(
        egui::Slider::new(&mut e.throw_sweep_deg, 20.0..=120.0)
            .text("Evade throw swing within the window (°)"),
    );
    ui.add(
        egui::Slider::new(&mut e.throw_window_s, 0.1..=0.8)
            .text("Evade throw window (s)")
            .fixed_decimals(2),
    );
    ui.add(egui::Slider::new(&mut e.rearm_bank_deg, 5.0..=45.0).text("Evade re-arm bank (°)"));
    ui.add(
        egui::Slider::new(&mut e.cooldown_s, 0.0..=5.0)
            .text("Evade cooldown (s)")
            .fixed_decimals(1),
    );
    ui.add(
        egui::Slider::new(&mut e.diagonal_pitch, 0.0..=1.0)
            .text("Evade diagonal from the arms' pitch (of a full pitch)")
            .fixed_decimals(2),
    );
    let h = &mut v.parachute_handles;
    ui.add(
        egui::Slider::new(&mut h.full_pull_m, 0.1..=0.6)
            .text("Parachute full pull (m)")
            .fixed_decimals(2),
    );
    ui.add(
        egui::Slider::new(&mut h.brake_threshold, 0.0..=0.9)
            .text("Parachute brake threshold (of a full pull)")
            .fixed_decimals(2),
    );
    ui.checkbox(
        &mut v.take_off_to_head,
        "Wingsuit take-off from the ground along the head's heading",
    )
    .on_hover_text(
        "The boost suit's ground take-off turns the body to where you look during its wind-up, \
         under the view, so the suit launches where you look.",
    );
    ui.checkbox(
        &mut v.helicopter_rail,
        "Stand on a helicopter's skid by its door",
    )
    .on_hover_text(
        "After a reel onto a helicopter's door side, stand Rico on the skid, free to walk along \
         it, rather than dropping him into the hang; Y enters, a brisk step off or the jump \
         leaves it.",
    );
    ui.add(
        egui::Slider::new(&mut v.rail.half_length_m, 0.3..=2.5).text("Skid reach either way (m)"),
    );
    ui.add(
        egui::Slider::new(&mut v.rail.step_off_m, 0.15..=1.0).text("Skid step-off distance (m)"),
    );
    ui.add(
        egui::Slider::new(&mut v.rail.pull_s, 0.05..=1.0)
            .text("Skid pull time (s), longer is less sticky"),
    );
    let c = &mut v.canopy;
    ui.checkbox(
        &mut c.skip_reel_opening,
        "Open the canopy out of a reel straight into its idle",
    )
    .on_hover_text(
        "Skips the somersault Rico plays when the canopy opens during a grapple reel; the canopy \
         itself still opens.",
    );
    ui.checkbox(
        &mut c.line_turn,
        "Turn the canopy toward a reeling grapple line",
    )
    .on_hover_text(
        "Under the canopy, a line fired to one side turns the canopy toward it, as the game \
             turns it toward where its camera looks.",
    );
    ui.add(
        egui::Slider::new(&mut c.line_turn_scale, 0.0..=2.0)
            .text("Canopy line turn strength")
            .fixed_decimals(2),
    );
    let r = &mut v.ripcord;
    ui.checkbox(
        &mut r.enabled,
        "Ripcord at the hip opens the parachute (falling, wingsuit)",
    );
    ui.add(
        egui::Slider::new(&mut r.pull_m, MIN_PULL_M..=0.4)
            .text("Ripcord pull (m)")
            .fixed_decimals(2),
    );
}
