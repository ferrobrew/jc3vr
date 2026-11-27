//! Vehicles, mounted guns, the wingsuit, and the parachute under the controller scheme: the weapons
//! in `docs/mod/input/vehicle-weapons.md`, the binding tables in `docs/mod/input/bindings.md`, and
//! the physical controls and arm steering in `docs/mod/input/cockpit-controls.md` and
//! `docs/mod/input/flight.md`.
//!
//! - [`config`]: the settings, all at `controllers.vehicles` ([`VEHICLES_CONFIG`]), and
//!   [`tuning`]: the fixed placement and tuning of everything here;
//! - [`driving`]: the driver's seat: its physical controls and the cockpit's buttons, the wheel and
//!   the handlebars fitted to the vehicle, the driver's view, the rendered wheel, and the driver's
//!   hand-aimed weapons;
//! - [`mounted`]: mounted guns and gunner seats, whose handles hold the hands and aim the gun,
//!   with the handles' grip and recoil and the manned vehicle gun's turn toward the hands' aim;
//! - [`passenger`]: a passenger's seated body turned toward the gun hand's aim;
//! - [`wingsuit`]: the wingsuit's arm steering, the evade and the gestures the arms throw, the head
//!   look-steer removed, and the take-off from the ground;
//! - [`parachute`]: the parachute's handles where the game's rig puts them, and the ripcord;
//! - [`rail`]: standing on a helicopter's skid after a reel onto its door side;
//! - [`grip_sources`]: the grip sources of the handles the hands hold besides the physical controls.
//!
//! The stick bindings of every class are the tables' (`controllers::bindings::tables`).
//! This module runs on the input tick in two halves around them, like the weapon handling:
//!
//! 1. [`begin_tick`] reads the mounted gun, fits the wheel and the handlebars to the vehicle, and
//!    provides the vehicle handling's grip targets ([`jc3vr_body::controllers::grab`]): the
//!    driver's controls, the cockpit's buttons, the parachute's handles with a reach margin around
//!    each, the ripcord, and the mounted gun's handles. What a hand holds is offered to it as held,
//!    so that a press made while it still holds (a release and a press between two ticks) takes it
//!    again.
//! 2. [`finish_tick`] acts on the tick's grabs, moves the held controls, replaces the tables'
//!    analog writes for each axis a control drives, and publishes the wheel angle for the rendered
//!    wheel; in the wingsuit it adds the arm pose to the move stick, and under the parachute the
//!    held handles.
//!
//! A control or a handle is held only while its hand's grip is: releasing the grip lets go.
//!
//! The hands take the anchored poses in [`apply`], which every hand subsystem's hands pass through
//! (the body's `hands::active_hands`). Everything acts only while the bindings inject, which is
//! only under the controller scheme with a controller active.
//!
//! A driven vehicle's turrets are left to the game: a turret turns toward its weapon's aim point at
//! its own limited rate, so a turret aimed by hand trails the hand exactly as it trails the gaze.
//! The motorcycle's and jetski's personal weapon is a wielded weapon on the hand's attach bone, so
//! the per-hand gun aim already aims it by hand ([`jc3vr_body::controllers::aim::gun`]).
//!
//! [`VEHICLES_CONFIG`]: crate::vehicles::config::VEHICLES_CONFIG

pub(crate) mod axis_output;
mod begin;
pub mod config;
pub mod driving;
mod entry;
mod finish;
pub mod grip_sources;
pub mod mounted;
pub mod parachute;
pub mod passenger;
pub mod rail;
mod speed;
mod state;
pub mod tuning;
mod types;
pub mod wingsuit;

use re_utilities::hook_library::HookLibrary;

pub use self::{
    axis_output::write_outputs,
    begin::begin_tick,
    finish::finish_tick,
    parachute::ripcord_tick::{finish as ripcord_finish, holder as ripcord_holder},
    speed::seat_vehicle_speed_mps,
    state::{
        apply, is_mounted_weapon, mounted_aim_point, mounted_target, parachute_holds, stand_down,
        status, wrist_tilt_now_deg,
    },
    types::{HandlesReadout, TickVolumes, VehiclesStatus},
};

/// The vehicles' hooks, enabled by the payload's composition.
pub fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(driving::aim::hook_library())
        .with_hook_library(wingsuit::look_steer::hook_library())
        .with_hook_library(driving::wheel_render::hook_library())
        .with_hook_library(parachute::rig::hook_library())
        .with_hook_library(mounted::turret::hook_library())
        .with_hook_library(rail::hook_library())
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec2, Vec3};
    use jc3gi::input::input_action_map::Action::{self, *};
    use jc3vr_body::controllers::{
        grab::cockpit::ControlKind,
        hands::CockpitHand,
        intent::hold::{Desired, MapKind, Target, Want},
        mode::{VehicleClass, VehicleSeat},
    };
    use jc3vr_input::frame::Pose;

    use super::*;
    use crate::vehicles::{
        driving::physical::{ControlState, Hold, Outputs},
        finish::rendered_angle,
        mounted::input_tick::offered,
        state::move_grip,
        tuning::PhysicalControlsTuning,
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
    fn only_the_driver_is_offered_the_controls() {
        let seat = |class, driver| Some(VehicleSeat { class, driver });
        assert_eq!(
            offered(seat(VehicleClass::Helicopter, true), false, false),
            &[ControlKind::Cyclic, ControlKind::Collective]
        );
        assert!(offered(seat(VehicleClass::Helicopter, false), false, false).is_empty());
        assert!(offered(None, false, false).is_empty());
        assert_eq!(
            offered(seat(VehicleClass::Sea, true), true, false),
            &[ControlKind::Handlebars]
        );
        assert_eq!(
            offered(seat(VehicleClass::Plane, true), false, true),
            &[ControlKind::Yoke, ControlKind::Throttle]
        );
    }

    #[test]
    fn the_rendered_wheel_follows_the_held_wheel_and_otherwise_the_steering() {
        let config = PhysicalControlsTuning::shipped();
        let lock = config.wheel.lock_deg.to_radians();
        let mut controls = ControlState::new();
        let half = rendered_angle(&controls, &config, Some(0.5));
        assert!((driving::steering::steer_for_angle(half, &config.wheel) - 0.5).abs() < 1e-4);
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
