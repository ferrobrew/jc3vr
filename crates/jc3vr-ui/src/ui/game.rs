//! The movement hooks' live readouts.

use jc3gi::character::character::{AimState, Character};
use jc3vr_body::{
    hooks,
    hooks::{
        animation::canopy::REEL_OPENINGS_SKIPPED,
        input::{
            air::carried_updates,
            locomotion::LOCOMOTION_COUNTS,
            parachute::{PARACHUTE_LINE_TURNS, PARACHUTE_LOOK_STEER_SUPPRESSIONS},
            swim::SWIM_COUNTS,
            wingsuit::{barrel_rolls_dropped, suppressed as stall_flip_suppressed},
        },
    },
};

use crate::ui::{catalog::FeatureId, dispatch::FeatureBody};

pub(crate) fn status(id: FeatureId) -> Option<FeatureBody> {
    let body: FeatureBody = match id {
        FeatureId::OnFoot => |ui, _| {
            locomotion_readout(ui);
            swimming_readout(ui);
        },
        FeatureId::Air => |ui, _| {
            wingsuit_readout(ui);
            parachute_readout(ui);
        },
        _ => return None,
    };
    Some(body)
}

fn swimming_readout(ui: &mut egui::Ui) {
    let swim = SWIM_COUNTS.snapshot();
    ui.label(format!(
        "Swim: cores {}  dir-overrides {}  rotate-overrides {}  turn-suppressions {}  \
         move-refusals {}  untargeted {}",
        swim.evals, swim.directions, swim.rotates, swim.turns, swim.moves, swim.untargeted,
    ));
}

fn locomotion_readout(ui: &mut egui::Ui) {
    let loco = LOCOMOTION_COUNTS.snapshot();
    ui.label(format!(
        "Loco task calls: move {}  aim-relative {}  face-camera {}  slide {}  \
         starts-skipped {}  speed-floors {}",
        loco.move_tasks,
        loco.aim_relative_tasks,
        loco.face_camera,
        loco.slides,
        loco.skipped_starts,
        loco.instant_speed_floors,
    ));
    match hooks::input::locomotion::debug_blackboard_snapshot() {
        Some(snapshot) => {
            let vec = |v: Option<glam::Vec3>| {
                v.map_or_else(
                    || "<absent>".to_owned(),
                    |v| format!("({:+.2}, {:+.2}, {:+.2})", v.x, v.y, v.z),
                )
            };
            let float =
                |f: Option<f32>| f.map_or_else(|| "<absent>".to_owned(), |f| format!("{f:+.2}"));
            ui.label(format!(
                "Loco (game-thread capture): input {}  move dir {}  speed {}  aux {}",
                float(snapshot.input_magnitude),
                vec(snapshot.move_dir),
                float(snapshot.speed),
                float(snapshot.aux_float),
            ));
        }
        None => {
            ui.label("Loco: <not captured yet>");
        }
    }
    unsafe {
        if jc3gi::game::Game::get().is_some() {
            if let Some(character) = Character::GetLocalPlayerCharacter().as_ref() {
                // The flag values come from the generated bitflags, so the labels cannot drift.
                const FLAG_NAMES: [(AimState, &str); 4] = [
                    (AimState::m_AimingEnabled, "Enabled"),
                    (AimState::m_AimingWeapon, "Weapon"),
                    (AimState::m_AimingGrapple, "Grapple"),
                    (AimState::m_WasAiming, "WasAiming"),
                ];
                let flags = character.m_AimFlags;
                let active: Vec<&str> = FLAG_NAMES
                    .iter()
                    .filter(|(flag, _)| flags.contains(*flag))
                    .map(|(_, name)| *name)
                    .collect();
                ui.label(format!(
                    "Game aim flags {:#04x}: {}  timer: {:.2}s (real state; the shim's force is \
                     scoped to the loco tasks and never visible here)",
                    flags.bits(),
                    active.join(" | "),
                    character.m_AimTimer,
                ));
            }
        } else {
            ui.label("Unavailable: the game state is not initialized.");
        }
    }
}

fn parachute_readout(ui: &mut egui::Ui) {
    ui.label(format!(
        "Parachute: look-steer suppressions {}, line-turn ticks {}, reel openings skipped {}",
        PARACHUTE_LOOK_STEER_SUPPRESSIONS.get(),
        PARACHUTE_LINE_TURNS.get(),
        REEL_OPENINGS_SKIPPED.get(),
    ));
}

fn wingsuit_readout(ui: &mut egui::Ui) {
    ui.label(if stall_flip_suppressed() {
        "Stall flip: removed"
    } else {
        "Stall flip: the game's"
    });
    ui.label(format!("Barrel rolls dropped: {}", barrel_rolls_dropped()));
    ui.label(format!(
        "Air physics updates with a reel's speed carried: {}",
        carried_updates()
    ));
}
