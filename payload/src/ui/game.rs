//! The on-foot swim settings, the movement hooks' live readouts, and the engine experiment
//! controls.

use std::sync::atomic::Ordering;

use jc3gi::{
    character::character::{AimState, Character},
    spawn_system::SpawnSystem,
    types::math::Matrix4,
};

use crate::{
    config::{self, Config},
    hooks::{
        self,
        input::{
            locomotion::{
                AIM_RELATIVE_TASK_CALLS, FACE_CAMERA_CALLS, INSTANT_SPEED_FLOORS, MOVE_TASK_CALLS,
                SHIMMED_CALLS, SKIPPED_STARTS, SLIDE_CALLS,
            },
            parachute::PARACHUTE_LOOK_STEER_SUPPRESSIONS,
            swim::{
                SWIM_BACKWARD_REFUSALS, SWIM_DIRECTION_OVERRIDES, SWIM_EVAL_CALLS,
                SWIM_ROTATE_OVERRIDES, SWIM_TURN_SUPPRESSIONS, SWIM_UNTARGETED,
            },
            wingsuit::{barrel_rolls_dropped, suppressed as stall_flip_suppressed},
        },
    },
    sim_rate::{self, SimRate},
    ui::{catalog::FeatureId, dispatch::FeatureBody, util::patchbox},
};

pub(crate) fn controls(ui: &mut egui::Ui, id: FeatureId) {
    match id {
        FeatureId::OnFoot => swimming_controls(ui, &mut config::edit()),
        FeatureId::Engine => egui_debug_game(ui),
        _ => {}
    }
}

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
        FeatureId::Engine => |ui, _| game_readout(ui),
        _ => return None,
    };
    Some(body)
}

pub fn egui_debug_game(ui: &mut egui::Ui) {
    sim_rate_controls(ui);
    unsafe {
        if let Some(game) = jc3gi::game::Game::get() {
            ui.collapsing("Game", |ui| {
                let mut interpolation_override = game.m_InterpolationOverride;
                let before = interpolation_override;
                egui::ComboBox::from_label("Interpolation override")
                    .selected_text(interpolation_override.to_string())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut interpolation_override, -1, "Really None");
                        ui.selectable_value(&mut interpolation_override, 0, "None");
                        ui.selectable_value(&mut interpolation_override, 1, "1");
                        ui.selectable_value(&mut interpolation_override, 2, "2");
                        ui.selectable_value(&mut interpolation_override, 3, "3");
                    });

                if before != interpolation_override
                    && let Some(mut patcher) = hooks::patcher()
                {
                    patcher.patch(
                        &mut game.m_InterpolationOverride as *mut _ as usize,
                        &interpolation_override.to_le_bytes(),
                    );
                }
                patchbox(ui, "Decouple enabled", &mut game.m_DecoupleEnabled);
            });

            ui.collapsing("Spawn", |ui| {
                if ui
                    .button("Spawn exploding barrel")
                    .on_hover_text(
                        "Queue a spawn request for the shipped \"Exploding Barrel\" prop definition \
                         a few metres along the current view direction. The spawn is asynchronous; \
                         the barrel appears once its entity has streamed in.",
                    )
                    .clicked()
                    && let Some(spawn_system) = SpawnSystem::get()
                {
                    let pos = &spawn_system.m_ViewPos;
                    let dir = &spawn_system.m_ViewDir;
                    let mut transform = Matrix4::IDENTITY;
                    for axis in 0..3 {
                        transform.data[12 + axis] =
                            pos.data[axis] + dir.data[axis] * BARREL_SPAWN_DISTANCE;
                    }
                    spawn_system.Spawn(
                        // The only prop definition tagged "barrel" is the exploding fuel barrel.
                        c"barrel".as_ptr().cast(),
                        &transform,
                        // Skip the LOS/proximity/spawn-block checks: the point is to spawn where
                        // the player is looking, which those checks exist to prevent.
                        SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                    );
                }
            });
        } else {
            ui.label("Unavailable: the game state is not initialized.");
        }
    }
}

fn swimming_controls(ui: &mut egui::Ui, cfg: &mut Config) {
    ui.checkbox(
        &mut cfg.movement.smooth_swim_yaw,
        "Smooth swim yaw (right-stick heading in water)",
    )
    .on_hover_text(
        "Aim both the swim move direction and the body's heading rotation at the \
         heading you own with the right stick: the camera has no influence, the left stick \
         swims forward along that heading (backward is refused -- there is no backstroke), \
         and the game's act-quantized turn steps never trigger. Inert on flatscreen or \
         before the right-stick yaw target seeds.",
    );
    ui.add(
        egui::Slider::new(&mut cfg.movement.swim_turn_rate_deg_s, 180.0..=540.0)
            .text("Swim body turn rate (deg/s)"),
    )
    .on_hover_text(
        "How fast the body is allowed to rotate toward the heading you own with the \
         right stick. Below the look input's own rate the body lags the target and the turn \
         keeps going after you let go.",
    );
}

fn swimming_readout(ui: &mut egui::Ui) {
    ui.label(format!(
        "Swim: cores {}  dir-overrides {}  rotate-overrides {}  turn-suppressions {}  \
         backward-refusals {}  untargeted {}",
        SWIM_EVAL_CALLS.load(Ordering::Relaxed),
        SWIM_DIRECTION_OVERRIDES.load(Ordering::Relaxed),
        SWIM_ROTATE_OVERRIDES.load(Ordering::Relaxed),
        SWIM_TURN_SUPPRESSIONS.load(Ordering::Relaxed),
        SWIM_BACKWARD_REFUSALS.load(Ordering::Relaxed),
        SWIM_UNTARGETED.load(Ordering::Relaxed),
    ));
}

fn locomotion_readout(ui: &mut egui::Ui) {
    ui.label(format!(
        "Loco task calls: move {}  aim-relative {}  shimmed {}  face-camera {}  slide {}  \
         starts-skipped {}  speed-floors {}",
        MOVE_TASK_CALLS.load(Ordering::Relaxed),
        AIM_RELATIVE_TASK_CALLS.load(Ordering::Relaxed),
        SHIMMED_CALLS.load(Ordering::Relaxed),
        FACE_CAMERA_CALLS.load(Ordering::Relaxed),
        SLIDE_CALLS.load(Ordering::Relaxed),
        SKIPPED_STARTS.load(Ordering::Relaxed),
        INSTANT_SPEED_FLOORS.load(Ordering::Relaxed),
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
        "Parachute: look-steer suppressions {}",
        PARACHUTE_LOOK_STEER_SUPPRESSIONS.load(Ordering::Relaxed),
    ));
}

fn wingsuit_readout(ui: &mut egui::Ui) {
    ui.label(if stall_flip_suppressed() {
        "Stall flip: removed"
    } else {
        "Stall flip: the game's"
    });
    ui.label(format!("Barrel rolls dropped: {}", barrel_rolls_dropped()));
}

fn game_readout(ui: &mut egui::Ui) {
    unsafe {
        if let Some(game) = jc3gi::game::Game::get() {
            ui.label(format!("Update frequency: {}Hz", game.m_UpdateFrequency));
            ui.label(format!("Update flags: {:X}", game.m_UpdateFlags));
            ui.label(format!(
                "Interpolation method: {:X}",
                game.m_InterpolationMethod
            ));
        } else {
            ui.label("Unavailable: the game state is not initialized.");
        }
    }
    sim_rate_readout(ui);
}

/// How far along the view direction to place a debug-spawned object. Far enough that the object
/// does not overlap the player, close enough to land in front of them.
const BARREL_SPAWN_DISTANCE: f32 = 4.0;

/// The sim-rate experiment (`crate::sim_rate`, `docs/mod/performance/sim-rate.md`).
fn sim_rate_controls(ui: &mut egui::Ui) {
    ui.separator();
    let configured = Config::lock_query(|c| c.sim_rate.rate);
    let mut chosen = configured;
    egui::ComboBox::from_label("Sim rate (experiment)")
        .selected_text(format!("{} Hz", chosen.hz()))
        .show_ui(ui, |ui| {
            for rate in SimRate::ALL {
                let label = if rate == SimRate::DEFAULT {
                    format!("{} Hz (the game's own)", rate.hz())
                } else {
                    format!("{} Hz", rate.hz())
                };
                ui.selectable_value(&mut chosen, rate, label);
            }
        })
        .response
        .on_hover_text(
            "Run the sim tick at another rate, to measure what it costs and what it breaks. Above \
             30 Hz the game's update frequency is set to the rate, the physics step's pin to \
             30 Hz is removed so the world steps by the tick, and the interpolation override is \
             forced to the accumulated method, replacing any override chosen above. Back at \
             30 Hz all three are restored. The test procedure is in \
             docs/mod/performance/sim-rate.md.",
        );
    if chosen != configured {
        config::edit().sim_rate.rate = chosen;
    }
}

fn sim_rate_readout(ui: &mut egui::Ui) {
    let status = sim_rate::status();
    ui.label(format!(
        "Applied {} Hz, physics pin {}, game update frequency {}, interpolation override {}, at \
         most {} ticks a frame when accumulating",
        status.applied_hz,
        if status.pin_removed {
            "removed"
        } else {
            "in place"
        },
        status
            .game_hz
            .map_or_else(|| "?".to_owned(), |hz| format!("{hz} Hz")),
        status
            .interpolation_override
            .map_or_else(|| "?".to_owned(), |o| o.to_string()),
        status.max_ticks_per_frame,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swimming_editor_changes_only_the_selected_config_target_without_runtime() {
        let context = egui::Context::default();
        let mut cfg = Config::new();
        cfg.movement.smooth_swim_yaw = false;
        let before = serde_json::to_value(&cfg).unwrap();
        let mut first_control = egui::Pos2::ZERO;
        for _ in 0..2 {
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                first_control = ui.cursor().min + egui::vec2(8.0, 8.0);
                swimming_controls(ui, &mut cfg);
            });
        }
        for pressed in [true, false] {
            let input = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(first_control),
                    egui::Event::PointerButton {
                        pos: first_control,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            };
            let _ = context.run_ui(input, |ui| swimming_controls(ui, &mut cfg));
        }
        assert!(cfg.movement.smooth_swim_yaw);
        let mut expected = before;
        expected["movement"]["smooth_swim_yaw"] = true.into();
        assert_eq!(serde_json::to_value(&cfg).unwrap(), expected);
    }
}
