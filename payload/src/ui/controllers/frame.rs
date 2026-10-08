//! The live controller frame: each hand's poses, inputs, and hand-tracking joints.

use egui::Ui;
use glam::Vec3;

use crate::{
    controllers::{
        frame::{self, Hand, HandFrame, TrackedPose},
        haptics::{self, vocabulary::Primitive},
    },
    ui::controllers::{euler_deg, profile_label, vec3, yes_no},
};

pub(super) fn live_frame(ui: &mut Ui, frame: Option<&frame::ControllerFrame>) {
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
    ui.horizontal(|ui| {
        ui.label("Test:");
        for primitive in Primitive::ALL {
            if ui.button(primitive.label()).clicked() {
                haptics::test(hand, primitive);
            }
        }
    });
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
