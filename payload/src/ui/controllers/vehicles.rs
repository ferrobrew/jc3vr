//! Vehicles and flight: the seat and the physical controls it offers, what each hand holds, the
//! values the controls write, where the wheel and handlebars were placed from and the rendered
//! wheel's turn, the driving view's correction, the mounted gun's aim, the arm steering's pose and
//! output, the parachute handles' pulls and output, and the vehicle and air settings.

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        vehicles::{
            self, VehiclesStatus,
            arm_steering::PitchInput,
            config::VehiclesConfig,
            driver_aim,
            driving_view::{self, DrivingViewStatus},
            fit::FitReadout,
            look_steer, parachute_rig,
            physical::{ControlEvent, ControlKind},
            ripcord::MIN_PULL_M,
            wheel::WheelFrame,
            wheel_render,
        },
    },
    ui::{catalog::FeatureId, controllers::vec3},
};

/// Everything the section shows, read before the config is locked for editing.
pub(super) struct VehiclesReadout {
    status: VehiclesStatus,
    driving_view: DrivingViewStatus,
    driver_substitutions: u64,
    look_steer_suppressions: u64,
    wheel_turns: u64,
    mounted_shots: u64,
    handles_unplaced: u64,
}

impl VehiclesReadout {
    pub(super) fn read() -> Self {
        Self {
            status: vehicles::status(),
            driving_view: driving_view::status(),
            driver_substitutions: driver_aim::substitutions(),
            look_steer_suppressions: look_steer::suppressions(),
            wheel_turns: wheel_render::turned(),
            mounted_shots: vehicles::mounted_recoil::shots(),
            handles_unplaced: parachute_rig::handles_unplaced(),
        }
    }
}

pub(super) fn feature_readout(ui: &mut Ui, readout: &VehiclesReadout, id: FeatureId) {
    if !readout.status.active {
        ui.label("Inactive: the bindings are not injecting.");
    }
    match id {
        FeatureId::Vehicles => {
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
        "Stick and lever cues {} (last {}) · afterburner gate passed {} · horn {} · nitrous {} · buttons worked {}",
        status.cues,
        status.last_cue.map_or("none", |c| c.label()),
        status.afterburner_lights,
        o.horn,
        o.nitrous,
        status.button_presses,
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
    let status = &readout.status;
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
                readout.mounted_shots,
            ));
        }
        None => {
            ui.label("Mounted gun: none");
        }
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
    ui.checkbox(&mut p.twist_rudder, "Twist the flight stick for the rudder");
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
