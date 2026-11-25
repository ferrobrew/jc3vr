//! The aim-state pin and the mental-state cuts.

use egui::Ui;

use crate::{
    controllers::aim_state::{AimStateStatus, IkTask},
    ui::controllers::{vec3, yes_no},
};

pub(super) fn aim_state_readout(ui: &mut Ui, status: &AimStateStatus) {
    let pin = &status.pin;
    ui.label(if pin.active {
        format!(
            "Weapon pin: holding · flags {:08b} → {:08b} · aiming bit {} · timer positive {}",
            pin.flags_before,
            pin.flags_after,
            yes_no(pin.aiming_bit_after),
            yes_no(pin.timer_positive)
        )
    } else {
        "Weapon pin: inactive".to_owned()
    });
    ui.label(format!(
        "Pinned updates {} · held against the game's aim {}",
        pin.pinned_updates, pin.held_against_aim
    ));
    ui.label(format!(
        "Grapple pin: {} · cleared {} times",
        if status.grapple_pin_active {
            "holding"
        } else {
            "inactive"
        },
        status.grapple_cleared
    ));
    ui.label(format!(
        "Aim IK skipped: {}",
        IkTask::ALL
            .iter()
            .zip(status.ik_skipped)
            .map(|(task, count)| format!("{} {count}", task.label()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    ui.label(match status.neutral_point {
        Some(point) => format!("Neutral aim point: {}", vec3(point)),
        None => "Neutral aim point: inactive".to_owned(),
    });
    ui.label(format!(
        "Wall-penetration blocks cleared: {}",
        status.wall_blocks_cleared
    ));
    let m = &status.mental;
    ui.label(format!(
        "Mental state {} · combat draw {} · deliberate holster pending {}",
        m.mental_state
            .map_or("unknown".to_owned(), |state| state.to_string()),
        if m.draw_blocked { "blocked" } else { "allowed" },
        yes_no(m.deliberate_pending)
    ));
    ui.label(format!(
        "Automatic holsters swallowed: slow unequip {}, relaxed holster {} · deliberate holsters \
         passed {}",
        m.unequip_slow_swallowed, m.relaxed_holster_swallowed, m.deliberate_holsters_passed
    ));
}
