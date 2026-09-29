//! Vehicles and flight: the seat and the physical controls it offers, what each hand holds, the
//! values the controls write, the mounted gun's aim, the arm steering's pose and output, and the
//! vehicle, mounted-gun, and arm-steering settings.

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        vehicles::{
            self, VehiclesStatus,
            config::{
                ArmSteeringConfig, HandlebarsSpec, LeverAxis, LeverSpec, MountedConfig,
                PhysicalControlsConfig, StickSpec, VehiclesConfig, WheelSpec,
            },
            driver_aim, look_steer,
            physical::{ControlEvent, ControlKind},
        },
    },
    ui::controllers::{vec3, vec3_drag},
};

/// Everything the section shows, read before the config is locked for editing.
pub(super) struct VehiclesReadout {
    status: VehiclesStatus,
    driver_substitutions: u64,
    look_steer_suppressions: u64,
}

impl VehiclesReadout {
    pub(super) fn read() -> Self {
        Self {
            status: vehicles::status(),
            driver_substitutions: driver_aim::substitutions(),
            look_steer_suppressions: look_steer::suppressions(),
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
        "Written: steer {} · lean {} · cyclic {} · collective {} · flight stick {} · throttle {}",
        axis(o.steer),
        axis(o.lean),
        pair(o.cyclic),
        axis(o.collective),
        pair(o.flight_stick),
        axis(o.throttle),
    ));
    for event in status.events.iter().rev() {
        ui.label(match event {
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
                "Arms: bank {:+.2} m · height {:+.2} m · spread {:.0}%",
                a.pose.bank_m,
                a.pose.height_m,
                a.pose.spread * 100.0
            ));
            ui.label(format!(
                "Arm steer {} · written ({:+.2}, {:+.2})",
                a.arms.map_or("not spread".to_owned(), |v| format!(
                    "({:+.2}, {:+.2})",
                    v.x, v.y
                )),
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

pub(super) fn vehicles_settings(ui: &mut Ui, v: &mut VehiclesConfig) {
    ui.checkbox(&mut v.driver_weapons_by_hand, "Driver weapons aim by hand");
    ui.label("The wingsuit's head look-steer removal follows the parachute's movement setting.");
    egui::CollapsingHeader::new("Mounted guns").show(ui, |ui| mounted_settings(ui, &mut v.mounted));
    egui::CollapsingHeader::new("Physical controls")
        .show(ui, |ui| physical_settings(ui, &mut v.physical));
    egui::CollapsingHeader::new("Arm steering")
        .show(ui, |ui| arm_settings(ui, &mut v.arm_steering));
}

fn mounted_settings(ui: &mut Ui, m: &mut MountedConfig) {
    ui.checkbox(&mut m.aim_by_hands, "Aim by the hands on the handles");
    ui.checkbox(&mut m.anchor_hands, "Hands on the handles");
    ui.add(
        egui::Slider::new(&mut m.min_lever_m, 0.1..=1.5)
            .text("minimum lever (m)")
            .fixed_decimals(2),
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
    egui::CollapsingHeader::new("Wheel (car, boat)").show(ui, |ui| wheel(ui, &mut p.wheel));
    egui::CollapsingHeader::new("Handlebars (motorcycle)")
        .show(ui, |ui| handlebars(ui, &mut p.handlebars));
    egui::CollapsingHeader::new("Cyclic (helicopter)").show(ui, |ui| stick(ui, &mut p.cyclic));
    egui::CollapsingHeader::new("Collective (helicopter)")
        .show(ui, |ui| lever(ui, &mut p.collective));
    egui::CollapsingHeader::new("Flight stick (plane)")
        .show(ui, |ui| stick(ui, &mut p.flight_stick));
    egui::CollapsingHeader::new("Throttle (plane)").show(ui, |ui| lever(ui, &mut p.throttle));
}

fn wheel(ui: &mut Ui, w: &mut WheelSpec) {
    ui.checkbox(&mut w.enabled, "Enabled");
    vec3_drag(ui, "hub (m)", &mut w.centre_m, 0.005);
    ui.add(egui::Slider::new(&mut w.radius_m, 0.1..=0.3).text("radius (m)"));
    ui.add(egui::Slider::new(&mut w.tilt_deg, 0.0..=80.0).text("tilt (°)"));
    ui.add(egui::Slider::new(&mut w.max_angle_deg, 30.0..=540.0).text("full steer (°)"));
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
    ui.checkbox(&mut a.parachute, "Parachute");
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
    ui.add(
        egui::Slider::new(&mut a.full_pitch_m, 0.1..=1.0)
            .text("full pitch (m)")
            .fixed_decimals(2),
    );
    ui.checkbox(&mut a.invert_pitch, "Raised hands dive");
}
