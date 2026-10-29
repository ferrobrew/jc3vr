//! Every controller setting except those with their own section's file.

use egui::Ui;

use crate::{
    config,
    controllers::{
        bindings::{Blocker, HandRecovery},
        config::{InputConfig, SchemeSetting},
        frame::{self, Button, GripPress, Hand, PressThresholds},
    },
    ui::controllers::{locomotion, unarmed},
    vr::input::{self as vr_input, restart},
};

pub(super) fn devices(
    ui: &mut Ui,
    cfg: &mut config::Config,
    status: &vr_input::InputStatus,
    recovery: [HandRecovery; 2],
) {
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
    })
    .response
    .on_hover_text(
        "The button layout follows the dominant hand from the next game start: OpenXR fixes the \
         suggested bindings once the session takes them.",
    );
    // The config lock is held here, so the reason is decided from the hand being edited rather
    // than through `restart::current`, which would take the lock again.
    if let Some(reason) = restart::reason(status, c.dominant_hand) {
        ui.colored_label(ui.visuals().warn_fg_color, reason.headline())
            .on_hover_text(reason.detail());
    }
    for hand in Hand::BOTH {
        ui.label(format!(
            "Input {}: {}",
            hand.label(),
            recovery_label(recovery[hand.index()])
        ))
        .on_hover_text(
            "After a loss of tracking or focus, a hand's input stays masked until its trigger, \
             grip, buttons, and stick rest.",
        );
    }
}

pub(super) fn thresholds(ui: &mut Ui, cfg: &mut config::Config) {
    let c = &mut cfg.controllers;
    egui::CollapsingHeader::new("Input")
        .default_open(true)
        .show(ui, |ui| {
            let before_trigger = c.input.trigger_press;
            ui.add(
                egui::Slider::new(&mut c.input.trigger_press.press, 0.05..=1.0)
                    .text("Trigger press"),
            );
            settle_threshold(&mut c.input.trigger_press, before_trigger, true);
            let before_trigger = c.input.trigger_press;
            ui.add(
                egui::Slider::new(&mut c.input.trigger_press.release, 0.0..=1.0)
                    .text("Trigger release"),
            );
            settle_threshold(&mut c.input.trigger_press, before_trigger, false);
            let before_squeeze = c.input.squeeze_press;
            ui.add(
                egui::Slider::new(&mut c.input.squeeze_press.press, 0.05..=1.0)
                    .text("Squeeze press"),
            )
            .on_hover_text("The grip press on a controller without grip force (Touch).");
            settle_threshold(&mut c.input.squeeze_press, before_squeeze, true);
            let before_squeeze = c.input.squeeze_press;
            ui.add(
                egui::Slider::new(&mut c.input.squeeze_press.release, 0.0..=1.0)
                    .text("Squeeze release"),
            );
            settle_threshold(&mut c.input.squeeze_press, before_squeeze, false);
            let before_force = c.input.squeeze_force_press;
            ui.add(
                egui::Slider::new(&mut c.input.squeeze_force_press.press, 0.05..=1.0)
                    .text("Grip force press"),
            )
            .on_hover_text(
                "The grip press on a controller with grip force (Index), whose capacitive squeeze \
             value is full whenever the hand rests round the handle.",
            );
            settle_threshold(&mut c.input.squeeze_force_press, before_force, true);
            let before_force = c.input.squeeze_force_press;
            ui.add(
                egui::Slider::new(&mut c.input.squeeze_force_press.release, 0.0..=1.0)
                    .text("Grip force release"),
            );
            settle_threshold(&mut c.input.squeeze_force_press, before_force, false);
            let before_trackpad = c.input.trackpad_press;
            ui.add(
                egui::Slider::new(&mut c.input.trackpad_press.press, 0.05..=1.0)
                    .text("Trackpad press"),
            )
            .on_hover_text("The trackpad force that presses a half, which pages the menu tabs.");
            settle_threshold(&mut c.input.trackpad_press, before_trackpad, true);
            let before_trackpad = c.input.trackpad_press;
            ui.add(
                egui::Slider::new(&mut c.input.trackpad_press.release, 0.0..=1.0)
                    .text("Trackpad release"),
            );
            settle_threshold(&mut c.input.trackpad_press, before_trackpad, false);
            grip_readout(ui, &c.input);
            ui.add(
                egui::Slider::new(&mut c.input.stick_deadzone, 0.0..=0.9).text("Stick deadzone"),
            );
            ui.checkbox(&mut c.input.hand_tracking, "Hand tracking")
                .on_hover_text(
                    "Locate the hand skeleton through XR_EXT_hand_tracking when offered.",
                );
        });
}

pub(super) fn bindings_settings(ui: &mut Ui, cfg: &mut config::Config) {
    let c = &mut cfg.controllers;
    egui::CollapsingHeader::new("Bindings")
        .default_open(true)
        .show(ui, |ui| {
            let b = &mut c.bindings;
            ui.checkbox(&mut b.enabled, "Inject bindings").on_hover_text(
            "Map the controllers to the game's actions while the controller scheme is in effect.",
        );
            ui.add(egui::Slider::new(&mut b.move_deadzone, 0.0..=0.6).text("Move stick deadzone"));
            ui.add(egui::Slider::new(&mut b.trigger_deadzone, 0.0..=0.5).text("Trigger deadzone"));
            ui.add(
                egui::Slider::new(&mut c.mode.debounce_ticks, 1..=6).text("Mode debounce (ticks)"),
            );
        });
}

pub(super) fn menu_panel(ui: &mut Ui, cfg: &mut config::Config) {
    let b = &mut cfg.controllers.bindings;
    ui.add(egui::Slider::new(&mut b.long_press_s, 0.2..=1.5).text("Commlink long press (s)"));
    ui.add(egui::Slider::new(&mut b.crouch_flick, 0.3..=1.0).text("Seated crouch flick"));
    ui.add(egui::Slider::new(&mut b.menu_stick_push, 0.2..=0.95).text("Menu stick push"));
    ui.add(
        egui::Slider::new(&mut b.stick_chord_window_s, 0.001..=0.3)
            .text("Stick click chord window (s)"),
    )
    .on_hover_text("How long a lone stick click waits for the other before it acts.");
    ui.add(egui::Slider::new(&mut b.recenter_hold_s, 0.3..=3.0).text("Recenter chord hold (s)"))
        .on_hover_text("Hold both stick clicks this long to recenter the VR view.");
    ui.add(
        egui::Slider::new(&mut b.panel_chord_window_s, 0.001..=1.0)
            .text("Debug panel chord window (s)"),
    )
    .on_hover_text(
        "How soon after the left stick click the right B must be pressed to show or hide the \
             debug panel. The click is not held back for it.",
    );
}

pub(super) fn grapple_settings(ui: &mut Ui, cfg: &mut config::Config) {
    let c = &mut cfg.controllers;
    ui.checkbox(&mut c.bindings.yank.enabled, "Yank to zip").on_hover_text(
        "A sharp pull of the grapple hand toward the body taps the grapple while a line is live.",
    );
    ui.checkbox(&mut c.grapple.pull_to_reel, "Pull to reel")
        .on_hover_text(
            "The grapple hand's pull speeds the reel, a yank during it boosts it, and a pull back \
             tightens attached tethers.",
        );
    ui.checkbox(&mut c.grapple.pump, "Pump the hang swing");
}

pub(super) fn locomotion_settings(ui: &mut Ui, cfg: &mut config::Config) {
    let l = &mut cfg.controllers.locomotion;
    locomotion::locomotion_settings(ui, &mut l.stick, &mut l.acts, &mut l.view);
}

pub(super) fn animation_settings(ui: &mut Ui, cfg: &mut config::Config) {
    ui.checkbox(&mut cfg.body_ik.enabled, "Body IK")
        .on_hover_text("Drive the upper body toward the headpose via the engine's HumanIK solver.");
    let c = &mut cfg.controllers;
    ui.checkbox(&mut c.aim_state.skip_arm_ik, "Skip the game's arm aim IK");
    ui.checkbox(&mut c.body.remap_head, "Remap head height")
        .on_hover_text("Moves the view for every player, controllers or not.");
    ui.checkbox(&mut c.body.remap_hands, "Remap hand reach");
    unarmed::unarmed_settings(ui, &mut c.body.unarmed);
}

fn settle_threshold(pair: &mut PressThresholds, before: PressThresholds, press: bool) {
    let value = if press { pair.press } else { pair.release };
    *pair = before;
    set_threshold(pair, press, value);
}

fn set_threshold(pair: &mut PressThresholds, press: bool, value: f32) {
    let candidate = if press {
        PressThresholds {
            press: value,
            release: pair.release.min(value),
        }
    } else {
        PressThresholds {
            press: pair.press.max(value),
            release: value,
        }
    };
    if config::validate::press_thresholds("input", candidate).is_ok() {
        *pair = candidate;
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod widget_tests;

/// Each hand's grip and trackpad readings beside their thresholds, for tuning them.
fn grip_readout(ui: &mut Ui, input: &InputConfig) {
    let Some(hands) =
        frame::with_latest(|f| f.hands.each_ref().map(|h| (h.profile, h.inputs, h.buttons)))
    else {
        ui.label("Grip: no controller frame.");
        return;
    };
    for hand in Hand::BOTH {
        let (profile, inputs, buttons) = hands[hand.index()];
        let source = match input.grip_press(profile, hand) {
            GripPress::Force(_) => "force",
            GripPress::Value(_) => "value",
        };
        let pad = if buttons.contains(Button::TrackpadPressLeft) {
            "pressed left"
        } else if buttons.contains(Button::TrackpadPressRight) {
            "pressed right"
        } else {
            "up"
        };
        ui.label(format!(
            "Grip {}: value {:.2} · force {:.2} · press by {source} · {} · pad force {:.2} {pad}",
            hand.label(),
            inputs.squeeze,
            inputs.squeeze_force,
            if buttons.contains(Button::SqueezePress) {
                "pressed"
            } else {
                "up"
            },
            inputs.trackpad_force,
        ));
    }
}

fn recovery_label(recovery: HandRecovery) -> String {
    match recovery {
        HandRecovery::Unusable => "not injecting".to_owned(),
        HandRecovery::Ready => "ready".to_owned(),
        HandRecovery::Masked(None) => "masked, waiting for a sample".to_owned(),
        HandRecovery::Masked(Some(blocker)) => format!("masked until {}", blocker_label(blocker)),
    }
}

fn blocker_label(blocker: Blocker) -> String {
    match blocker {
        Blocker::Button(button) => format!("{button:?} is released"),
        Blocker::Trigger(value) => format!("the trigger rests (at {value:.2})"),
        Blocker::Squeeze(value) => format!("the grip rests (at {value:.2})"),
        Blocker::Stick { magnitude, neutral } => {
            format!("the stick rests under {neutral:.2} (at {magnitude:.2})")
        }
    }
}
