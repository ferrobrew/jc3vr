//! Dynamic HUD distance (issue #14): the floating panel eases toward a near distance while the
//! player rides a vehicle and back to the base distance otherwise. Full-screen UI (menus, movies)
//! always reads as far. The panel's constant-apparent-size scaling makes the shift a pure depth
//! change.

use crate::hud::HudMode;

/// The dynamic distance's fixed tuning.
pub(crate) const DEPTH_SHIFT: DepthShiftTuning = DepthShiftTuning {
    near_distance: 1.1,
    halflife: 0.35,
};

/// How the panel distance follows the vehicle state (see the module docs).
#[derive(Copy, Clone)]
pub(crate) struct DepthShiftTuning {
    /// The panel distance while the near shift is engaged, in meters.
    pub near_distance: f32,
    /// The easing halflife between distances, in seconds.
    pub halflife: f32,
}

/// Whether the local player is attached to a vehicle, polled on the game thread (the accessor
/// walks game-thread animation state) and read by the render-side policy.
static IN_VEHICLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Poll the local player's vehicle-attach state. Call once per frame on the game update thread.
pub fn poll_vehicle_state() {
    // SAFETY: game update thread; the accessor reads the character's own animation state.
    let in_vehicle = unsafe {
        jc3gi::character::character::Character::GetLocalPlayerCharacter()
            .as_ref()
            .is_some_and(|c| c.IsInVehicleAttachState())
    };
    IN_VEHICLE.store(in_vehicle, std::sync::atomic::Ordering::Relaxed);
}

/// A live snapshot of the dynamic-distance state, for the debug UI.
#[derive(Clone, Copy)]
pub struct DepthShiftStatus {
    /// The smoothed panel distance currently applied.
    pub smoothed: Option<f32>,
    /// Whether the near shift is engaged.
    pub near_engaged: bool,
}

/// The dynamic-distance policy's smoothing state.
#[derive(Default)]
pub struct DepthShift {
    /// Whether the near shift is currently engaged.
    near_engaged: bool,
    /// The smoothed panel distance; starts at the first target it sees.
    smoothed: Option<f32>,
    last_update: Option<std::time::Instant>,
}

impl DepthShift {
    /// The frame's panel distance: the policy target eased with a critically-damped exponential
    /// (`halflife` seconds). Call once per frame (eye 0).
    pub fn distance(&mut self, cfg: &DepthShiftTuning, mode: HudMode, base: f32) -> f32 {
        let target = self.target(cfg, mode, base);
        let dt = self
            .last_update
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.016)
            .min(0.1);
        self.last_update = Some(std::time::Instant::now());
        let alpha = (1.0 - 2.0_f32.powf(-dt / cfg.halflife.max(0.01))).min(1.0);
        let mut smoothed = self.smoothed.unwrap_or(target);
        smoothed += (target - smoothed) * alpha;
        self.smoothed = Some(smoothed);
        smoothed
    }

    /// The policy's target distance for this frame, before smoothing: near while attached to a
    /// vehicle. No hysteresis is needed, since the flag does not flap, and the easing covers the
    /// mount and the dismount.
    fn target(&mut self, cfg: &DepthShiftTuning, mode: HudMode, base: f32) -> f32 {
        // Full-screen UI reads as far, always.
        if mode != HudMode::Hud {
            self.near_engaged = false;
            return base;
        }
        self.near_engaged = IN_VEHICLE.load(std::sync::atomic::Ordering::Relaxed);
        if self.near_engaged {
            cfg.near_distance
        } else {
            base
        }
    }

    /// A snapshot of the live state for the debug UI.
    pub fn status(&self) -> DepthShiftStatus {
        DepthShiftStatus {
            smoothed: self.smoothed,
            near_engaged: self.near_engaged,
        }
    }
}

/// The render camera projection's z-row `(A, B)` with `device_depth = A + B / view_z`. The
/// embedded render camera is used (not the nullable active-camera pointer), matching the camera
/// the depth buffer was rendered with.
pub(crate) fn projection_z_row() -> Option<(f32, f32)> {
    // SAFETY: reads the live camera manager singleton, like render_camera_pose.
    let projection = unsafe {
        let cm = jc3gi::camera::camera_manager::CameraManager::get()?;
        cm.m_RenderCamera.as_ref()?.m_ProjectionF
    };
    let proj = glam::Mat4::from(projection);
    Some((proj.z_axis.z, proj.w_axis.z))
}
