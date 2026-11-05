//! The controller state's world groups: where the cockpit and the camera sit in the world, the
//! grapple's line, the vehicle, and the frame's place in the engine's sequence. Each is read on the
//! game thread after the render update, beside the rest of the controller state.

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    camera::camera_manager::CameraManager, character::character::Character,
    equipment::wire_end::WireEnd, graphics_engine::graphics_engine::get_render_frame_counters,
    types::math::Matrix4, vehicle::vehicle::Vehicle,
};
use tracking_format::{
    event::{
        CameraState, FrameState, GrappleState, Label, SlingState, VehicleState, ViewState,
        WireEndSample,
    },
    model::Pose,
};

use crate::{
    config::Config,
    controllers::{frame::Hand, haptics, snatch, vehicles},
    frame_timing, headpose,
    hooks::camera::last_dtf,
};

/// The cockpit's placement in the world and the active camera, while the cockpit is placed.
pub(super) fn view_state() -> Option<ViewState> {
    let cockpit = headpose::xr::cockpit_pose()?;
    let head = headpose::query();
    let world_scale = Config::lock_query(|c| c.vr.world_scale);
    // SAFETY: the game thread after the render update, where the camera manager and its active
    // camera are stable for the read.
    let camera = unsafe { CameraManager::get() }
        .and_then(|manager| unsafe { manager.m_ActiveCamera.as_ref() })
        .map(|camera| CameraState {
            previous: pose_of(&camera.m_TransformT0),
            current: pose_of(&camera.m_TransformT1),
            fraction: last_dtf(),
            rendered: pose_of(&camera.m_TransformF),
        });
    Some(ViewState {
        head_world: Pose {
            position: head.position,
            orientation: head.orientation,
        },
        head_cockpit: cockpit.position,
        view: headpose::xr::body_rotation(),
        world_scale,
        camera,
    })
}

/// The local player's grapple, while its hook has a wire out.
pub(super) fn grapple_state(character: &Character) -> Option<GrappleState> {
    // SAFETY: the game thread after the render update; the hook and its wire are the local
    // player's, alive for the read.
    let hook = unsafe { character.m_Inventory.m_GrapplingHook.px.as_ref() }?;
    let wire = unsafe { hook.m_ActiveWire.px.as_ref() }
        .or(unsafe { hook.m_AttachmentWire.px.as_ref() })?;
    let debug = |value: &dyn std::fmt::Debug| Label::Owned(format!("{value:?}"));
    let hook_end = &wire.m_WireEnds[0];
    Some(GrappleState {
        hook_state: debug(&hook.m_State),
        wire_state: debug(&wire.m_WireState),
        max_length_m: wire.m_MaxLength,
        hook: wire_end(hook_end),
        device: wire_end(&wire.m_WireEnds[1]),
        target_object: std::num::NonZeroUsize::new(
            hook_end.m_TargetInfo.m_TargetGameObject.px as usize,
        )
        .map(|object| object.get() as u64),
        sling: snatch::status().swing.map(|swing| SlingState {
            length_m: swing.length_m,
            tension_n: swing.tension_n,
            max_tension_n: swing.max_tension_n,
            mass_kg: swing.mass_kg,
            braced: swing.braced,
            tethered: swing.tethered,
            anchor: swing.anchor,
            centre: swing.centre,
        }),
    })
}

/// The vehicle the local player rides, while in one.
pub(super) fn vehicle_state(character: &Character) -> Option<VehicleState> {
    let summary = vehicles::summary();
    // SAFETY: the game thread after the render update; the local player's vehicle is alive while
    // he is attached to it.
    let vehicle: Option<&Vehicle> = unsafe { character.GetVehiclePtr().as_ref() };
    if vehicle.is_none() && summary.seat.is_none() {
        return None;
    }
    let debug = |value: &dyn std::fmt::Debug| Label::Owned(format!("{value:?}"));
    let transform = vehicle.map(|v| pose_of(&v.m_WorldMatrixT1));
    Some(VehicleState {
        class: summary
            .seat
            .map_or(Label::Borrowed("unknown"), |seat| debug(&seat.class)),
        driver: summary.seat.is_some_and(|seat| seat.driver),
        object: vehicle.map(|v| std::ptr::from_ref(v) as u64),
        transform,
        rendered: vehicle.map(|v| pose_of(&v.m_InterpolatedWorldMatrix)),
        step_m: vehicle.map(|v| {
            Mat4::from(v.m_WorldMatrixT1).w_axis.truncate()
                - Mat4::from(v.m_WorldMatrixT0).w_axis.truncate()
        }),
        steering: vehicle.map(|v| {
            Vec3::new(
                v.m_SteeringAcceleration,
                v.m_SteeringBrake,
                v.m_SteeringTurn,
            )
        }),
        handling: summary.active,
        holds: summary.holds.map(|hold| hold.map(|kind| debug(&kind))),
        mounted: summary.mounted,
        mounted_holds: summary.mounted_holds,
        mounted_aim: summary.mounted.then(vehicles::mounted_aim_point).flatten(),
    })
}

/// The engine's render frame index, the last game frame's interval and ticks, and the haptics the
/// hands last played.
pub(super) fn frame_state() -> FrameState {
    // SAFETY: a static the engine owns, read on the game thread.
    let render_frame = unsafe { get_render_frame_counters() }.m_FrameIndex;
    let (interval, ticks) = frame_timing::last_frame();
    let played = haptics::status().last_output;
    FrameState {
        render_frame: Some(render_frame),
        mirror_frame: crate::vr::latest_present().map(u64::from),
        interval_ms: interval.map(|d| d.as_secs_f32() * 1000.0),
        ticks,
        haptics: Hand::BOTH.map(|hand| played[hand.index()]),
    }
}

fn wire_end(end: &WireEnd) -> WireEndSample {
    WireEndSample {
        state: Label::Owned(format!("{:?}", end.m_WireEndState)),
        position: Vec3::from_array(end.m_WorldPositionT1.data),
        attached: !end.m_PointProxy.is_null(),
    }
}

/// An engine transform as a position and an orientation.
fn pose_of(matrix: &Matrix4) -> Pose {
    let (_, rotation, translation) = Mat4::from(*matrix).to_scale_rotation_translation();
    Pose {
        position: translation,
        orientation: if rotation.is_finite() {
            rotation.normalize()
        } else {
            Quat::IDENTITY
        },
    }
}
