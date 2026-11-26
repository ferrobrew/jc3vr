//! Low-pass filtering of synthetic head rotation. Tracking never enters this filter.

use glam::{Quat, Vec3};

use crate::tuning::VIEW;

/// A first-order quaternion low-pass with unit gain at steady state.
pub struct RotationFilter {
    rotation: Option<Quat>,
}

impl Default for RotationFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl RotationFilter {
    pub const fn new() -> Self {
        Self { rotation: None }
    }

    pub fn clear(&mut self) {
        self.rotation = None;
    }

    pub(crate) fn step(&mut self, target: Quat, dt: f32, reset: bool) -> Quat {
        let target = target.normalize();
        let previous = self.rotation.filter(|_| !reset);
        let tuning = &VIEW.get().head_animation;
        let rotation = match previous {
            Some(previous) if dt.is_finite() && dt > 0.0 && dt <= tuning.max_step_s => previous
                .slerp(target, -(-dt / tuning.time_constant_s).exp_m1())
                .normalize(),
            _ => target,
        };
        self.rotation = Some(rotation);
        rotation
    }
}

/// Remove the bone's bind frame before composing it with the world root.
pub fn head_frame(root: Quat, animated: Quat, bind: Quat) -> Quat {
    (root * animated * bind.inverse()).normalize()
}

/// Keep commanded root yaw immediate while filtering animation and root pitch and roll together.
/// Filtering them separately would briefly apply a bank twice when a clip transfers it to the root.
pub fn synthetic_rotation(
    filter: &mut RotationFilter,
    root: Quat,
    head: Quat,
    dt: f32,
    reset: bool,
) -> Quat {
    let heading = body_rig::rotation::twist_about_y(root);
    let local = heading.inverse() * head;
    (root.inverse() * heading * filter.step(local, dt, reset)).normalize()
}

/// Keep the animated eye point exact while the synthetic orientation settles.
pub fn eye_arm(root: Quat, filtered: Quat, neck_to_eyes_world: Vec3) -> Vec3 {
    (root * filtered).inverse() * neck_to_eyes_world
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Quat, b: Quat) {
        assert!(
            body_rig::rotation::angle_between(a, b) < 0.001,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn the_bind_frame_is_not_a_camera_rotation() {
        let root = Quat::from_rotation_z(0.6);
        let bind = Quat::from_euler(glam::EulerRot::YXZ, 0.3, 1.2, -0.4);
        let motion = Quat::from_rotation_x(-0.5);
        close(head_frame(root, motion * bind, bind), root * motion);
    }

    #[test]
    fn a_bank_transferred_between_root_and_animation_is_applied_once() {
        let mut filter = RotationFilter::default();
        let bank = Quat::from_rotation_z(0.7);
        let first = synthetic_rotation(&mut filter, Quat::IDENTITY, bank, 0.02, true);
        close(first, bank);
        let second = synthetic_rotation(&mut filter, bank, bank, 0.02, false);
        close(bank * second, bank);
    }

    #[test]
    fn a_short_pitch_impulse_is_attenuated_and_a_held_roll_is_preserved() {
        let mut filter = RotationFilter::default();
        filter.step(Quat::IDENTITY, 0.02, true);
        let impulse = filter.step(Quat::from_rotation_x(-1.0), 0.02, false);
        assert!(body_rig::rotation::angle(impulse) < 0.2);
        let bank = Quat::from_rotation_z(1.0);
        for _ in 0..100 {
            filter.step(bank, 0.02, false);
        }
        close(filter.rotation.unwrap(), bank);
    }

    #[test]
    fn quaternion_sign_and_tick_subdivision_do_not_change_the_result() {
        let target = Quat::from_rotation_x(0.8);
        let mut whole = RotationFilter::default();
        let mut halves = RotationFilter::default();
        whole.step(Quat::IDENTITY, 0.02, true);
        halves.step(Quat::IDENTITY, 0.02, true);
        let a = whole.step(target, 0.04, false);
        halves.step(-target, 0.02, false);
        let b = halves.step(target, 0.02, false);
        close(a, b);
    }

    #[test]
    fn resets_and_invalid_or_stalled_steps_do_not_retain_a_previous_pose() {
        for dt in [0.0, -0.1, f32::NAN, 0.6] {
            let mut filter = RotationFilter::default();
            filter.step(Quat::IDENTITY, 0.02, true);
            let target = Quat::from_rotation_x(0.7);
            close(filter.step(target, dt, false), target);
            close(filter.step(Quat::IDENTITY, 0.02, true), Quat::IDENTITY);
        }
    }

    #[test]
    fn eye_translation_does_not_lag_the_animated_anatomy() {
        let root = Quat::from_rotation_y(0.4);
        let filtered = Quat::from_rotation_z(0.2);
        let neck = Vec3::new(4.0, 2.0, -1.0);
        let eyes = Vec3::new(4.1, 2.08, -1.12);
        let arm = eye_arm(root, filtered, eyes - neck);
        assert!((neck + root * filtered * arm).abs_diff_eq(eyes, 1.0e-6));
    }

    #[test]
    fn tracking_is_composed_after_the_filter_without_attenuation() {
        let mut filter = RotationFilter::default();
        let synthetic = filter.step(Quat::from_rotation_z(0.4), 0.02, true);
        let hmd = Quat::from_rotation_y(2.8) * Quat::from_rotation_x(-0.8);
        let view = synthetic * hmd;
        close(synthetic.inverse() * view, hmd);
        let travel = Vec3::new(0.15, -0.6, 0.2);
        let anchor = Vec3::new(5.0, 3.0, 2.0);
        let pose = crate::headpose::xr::compose(travel, hmd, synthetic, anchor, 1.0);
        assert!((synthetic.inverse() * (pose.position - anchor)).abs_diff_eq(travel, 1.0e-6));
        close(pose.orientation, view);
    }
}
