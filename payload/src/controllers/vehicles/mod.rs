//! Vehicles, mounted guns, the wingsuit, and the parachute under the controller scheme: the plan of
//! record's "Vehicle, mounted, and wingsuit weapons", its vehicle and wingsuit binding tables, and
//! its decisions on physical controls and arm steering (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`config`]: the settings, all under [`ControllersConfig::vehicles`];
//! - [`physical`]: the wheel, handlebars, cyclic, collective, flight stick, and throttle of
//!   the driver's seat, grabbed with the grip, which write the vehicle's analog axes and hold the
//!   hands;
//! - [`steering`]: how the held wheel turns and maps to the steering;
//! - [`wheel`] and [`fit`]: the wheel and the handlebars placed where the driven vehicle's own are;
//! - [`driving_view`]: the driver's view moved per vehicle to a standard place behind the wheel;
//! - [`wheel_render`]: the vehicle's rendered steering wheel turned with the steering;
//! - [`mounted`]: mounted guns and gunner seats, whose handles hold the hands and aim the gun;
//!   [`mounted_grip`] takes the handles with the grip, and [`mounted_recoil`] kicks them;
//! - [`arm_steering`]: the arm-pose steering of the wingsuit;
//! - [`parachute`]: the parachute's handles, taken with the grip and pulled to steer and brake;
//! - [`driver_aim`]: the driver's vehicle weapons aimed by the dominant hand, as an option;
//! - [`passenger`]: a passenger's seated body turned toward the gun hand's aim;
//! - [`look_steer`]: the wingsuit's head look-steer removed.
//!
//! The stick bindings of every class are the tables' ([`crate::controllers::bindings::tables`]).
//! This module runs on the input tick in two halves around them, like the weapon handling:
//!
//! 1. [`begin_tick`] reads the mounted gun, fits the wheel and the handlebars to the vehicle, and
//!    places the physical controls' and the parachute handles' grab volumes, and returns the volume
//!    each hand is in, with the parachute handle each free hand is within reach of. A hand holding
//!    a control or a handle is always in it, so that a press made while it still holds (a release
//!    and a press between two ticks) takes the control again rather than firing a weapon or the
//!    grapple.
//!
//! A control or a handle is held only while its hand's grip is: releasing the grip lets go.
//! 2. [`finish_tick`] acts on the tick's grabs, moves the held controls, replaces the tables'
//!    analog writes for each axis a control drives, and publishes the wheel angle for the rendered
//!    wheel; in the wingsuit it adds the arm pose to the move stick, and under the parachute the
//!    held handles.
//!
//! The hands take the anchored poses in [`apply`], which every hand subsystem's hands pass through
//! ([`crate::controllers::hands::active_hands`]). Everything acts only while the bindings inject,
//! which is only under the controller scheme with a controller active.
//!
//! Turrets are left to the game: a turret turns toward its weapon's aim point at its own limited
//! rate, so a turret aimed by hand trails the hand exactly as it trails the gaze. The motorcycle's
//! and jetski's personal weapon is a wielded weapon on the hand's attach bone, so the per-hand gun
//! aim already aims it by hand ([`crate::controllers::aim::gun`]).
//!
//! [`ControllersConfig::vehicles`]: crate::controllers::config::ControllersConfig::vehicles

pub mod arm_steering;
mod arm_tick;
mod axis_output;
mod begin;
pub mod config;
mod control_haptics;
pub mod driver_aim;
pub mod driving_view;
mod finish;
pub mod fit;
mod handle_tick;
mod highlights;
pub mod look_steer;
pub mod mounted;
pub mod mounted_grip;
pub mod mounted_recoil;
mod mounted_tick;
pub mod parachute;
pub mod parachute_rig;
pub mod passenger;
pub mod physical;
mod state;
pub mod steering;
mod types;
pub mod wheel;
pub mod wheel_render;

use re_utilities::hook_library::HookLibrary;

pub use self::{
    axis_output::write_outputs,
    state::{
        apply, arm_panel, is_mounted_weapon, mounted_aim_point, mounted_target, parachute_brake,
        status, summary, wrist_tilt_now_deg,
    },
    types::{ArmReadout, HandlesReadout, TickVolumes, VehicleSummary, VehiclesStatus},
};
pub(crate) use self::{begin::begin_tick, finish::finish_tick, state::stand_down};

/// The vehicles' hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(driver_aim::hook_library())
        .with_hook_library(look_steer::hook_library())
        .with_hook_library(wheel_render::hook_library())
        .with_hook_library(parachute_rig::hook_library())
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec2, Vec3};
    use jc3gi::input::input_action_map::Action::{self, *};

    use super::*;
    use crate::controllers::{
        bindings::hold::{Desired, MapKind, Target, Want},
        config::ControllersConfig,
        frame::Pose,
        hands::CockpitHand,
        mode::{VehicleClass, VehicleSeat},
        vehicles::{
            config::PhysicalControlsConfig,
            finish::rendered_angle,
            mounted_tick::offered,
            physical::{ControlKind, ControlState, Hold, Outputs},
            state::move_grip,
        },
    };

    fn analog(desired: &[Desired], action: Action) -> Option<f32> {
        desired.iter().find_map(|d| match d.want {
            Want::Analog(v) if d.target.action == action => Some(v),
            _ => None,
        })
    }

    fn want(action: Action, value: f32) -> Desired {
        Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want: Want::Analog(value),
        }
    }

    #[test]
    fn a_held_control_replaces_the_stick_s_axis_and_leaves_the_rest() {
        let mut desired = vec![
            want(TURN_LEFT, 0.7),
            want(ACCELERATE, 1.0),
            want(PLANE_ROLL_LEFT, 0.7),
        ];
        let out = Outputs {
            steer: Some(0.4),
            ..Outputs::default()
        };
        write_outputs(&out, &mut desired);
        assert_eq!(analog(&desired, TURN_LEFT), None);
        assert_eq!(analog(&desired, TURN_RIGHT), Some(0.4));
        assert_eq!(analog(&desired, ACCELERATE), Some(1.0));
        assert_eq!(analog(&desired, PLANE_ROLL_LEFT), Some(0.7));
    }

    #[test]
    fn the_flight_stick_forward_pitches_down_and_the_collective_splits_its_halves() {
        let mut desired = Vec::new();
        let out = Outputs {
            flight_stick: Some(Vec2::new(0.0, 0.5)),
            collective: Some(-0.25),
            throttle: Some(0.0),
            ..Outputs::default()
        };
        write_outputs(&out, &mut desired);
        assert_eq!(analog(&desired, PLANE_PITCH_DOWN), Some(0.5));
        assert_eq!(analog(&desired, PLANE_PITCH_UP), None);
        assert_eq!(analog(&desired, HELI_DEC_ALTITUDE), Some(0.25));
        assert_eq!(analog(&desired, HELI_INC_ALTITUDE), None);
        assert_eq!(analog(&desired, PLANE_INC_TRUST), None);
    }

    #[test]
    fn only_the_driver_is_offered_the_enabled_controls() {
        // The defaults enable every physical control, so disable them all here and re-enable
        // only the one this test means to exercise.
        let mut config = ControllersConfig::new();
        config.vehicles.physical.wheel.enabled = false;
        config.vehicles.physical.handlebars.enabled = false;
        config.vehicles.physical.cyclic.enabled = false;
        config.vehicles.physical.collective.enabled = false;
        config.vehicles.physical.flight_stick.enabled = false;
        config.vehicles.physical.throttle.enabled = false;
        let seat = |class, driver| Some(VehicleSeat { class, driver });
        assert!(offered(seat(VehicleClass::Car, true), &config, false).is_empty());
        config.vehicles.physical.collective.enabled = true;
        assert_eq!(
            offered(seat(VehicleClass::Helicopter, true), &config, false),
            &[ControlKind::Collective]
        );
        assert!(offered(seat(VehicleClass::Helicopter, false), &config, false).is_empty());
        assert!(offered(None, &config, false).is_empty());
        config.vehicles.physical.handlebars.enabled = true;
        assert_eq!(
            offered(seat(VehicleClass::Sea, true), &config, true),
            &[ControlKind::Handlebars]
        );
    }

    #[test]
    fn the_rendered_wheel_follows_the_held_wheel_and_otherwise_the_steering() {
        let config = PhysicalControlsConfig::new();
        let lock = config.wheel.lock_deg.to_radians();
        let mut controls = ControlState::new();
        let half = rendered_angle(&controls, &config, Some(0.5));
        assert!((steering::steer_for_angle(half, &config.wheel) - 0.5).abs() < 1e-4);
        assert!((rendered_angle(&controls, &config, Some(3.0)) - lock).abs() < 1e-5);
        assert_eq!(rendered_angle(&controls, &config, None), 0.0);
        // A released wheel still on its way back is drawn where it is.
        controls.wheel_angle = 0.4;
        assert_eq!(rendered_angle(&controls, &config, Some(0.0)), 0.4);
        controls.holds[1] = Some(Hold {
            kind: ControlKind::Wheel,
            offset: Vec3::ZERO,
            orientation: None,
        });
        assert_eq!(rendered_angle(&controls, &config, Some(0.5)), 0.4);
    }

    #[test]
    fn a_grip_moves_onto_its_control_by_the_weight() {
        let pose = Pose {
            position: Vec3::ZERO,
            orientation: Quat::from_rotation_y(0.3),
        };
        let hand = CockpitHand {
            wrist: pose,
            grip: pose,
            aim: pose,
        };
        let moved = move_grip(hand, Vec3::X, 0.5);
        assert_eq!(moved.grip.position, Vec3::new(0.5, 0.0, 0.0));
        assert_eq!(moved.wrist.orientation, pose.orientation);
    }
}
