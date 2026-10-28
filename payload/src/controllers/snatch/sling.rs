//! A compliant rope: tension changes only radial velocity relative to the moving forearm anchor.

use glam::Vec3;

/// The owner facts required before continuing any snatch phase.
pub(super) struct Continuation {
    pub enabled: bool,
    pub on_foot: bool,
    pub tracked: bool,
    pub same_hand: bool,
    pub empty_hand: bool,
    pub grip_held: bool,
}

impl Continuation {
    pub fn cancellation(&self) -> Option<&'static str> {
        if !self.tracked {
            Some("the grapple hand lost tracking")
        } else if !self.same_hand || !self.empty_hand {
            Some("the grapple hand is no longer available")
        } else if !self.enabled || !self.on_foot {
            Some("the snatch is off or the player left their feet")
        } else if !self.grip_held {
            Some("the grip opened")
        } else {
            None
        }
    }
}

/// The attached target's next operation, before any engine access.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum AttachedMode {
    Swing,
    Resume,
    Pull,
    Cancel,
}

pub(super) fn attached_mode(enabled: bool, button_held: bool, swinging: bool) -> AttachedMode {
    match (enabled, button_held, swinging) {
        (false, _, true) => AttachedMode::Cancel,
        (true, true, _) => AttachedMode::Swing,
        (_, false, true) => AttachedMode::Resume,
        _ => AttachedMode::Pull,
    }
}

/// The rope's immutable length and previous tracked anchor.
#[derive(Debug, Copy, Clone)]
pub(super) struct Sling {
    pub length_m: f32,
    anchor: Vec3,
}

/// One pre-simulation radial correction; gravity and collisions remain the engine's work.
pub(super) struct RopeStep {
    pub centre: Vec3,
    pub velocity: Vec3,
    pub anchor: Vec3,
    pub dt: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub max_accel: f32,
}

impl Sling {
    pub fn new(centre: Vec3, anchor: Vec3) -> Option<Self> {
        let length_m = centre.distance(anchor);
        (centre.is_finite() && anchor.is_finite() && length_m.is_finite() && length_m > 0.01)
            .then_some(Self { length_m, anchor })
    }

    /// Preserve tangent, never push a slack rope, and bound tension over a bounded time step.
    pub fn step(&mut self, step: RopeStep) -> Option<Vec3> {
        let RopeStep {
            centre,
            velocity,
            anchor,
            dt,
            stiffness,
            damping,
            max_accel,
        } = step;
        if !centre.is_finite()
            || !velocity.is_finite()
            || !anchor.is_finite()
            || !dt.is_finite()
            || dt <= 0.0
            || dt > 0.1
            || !stiffness.is_finite()
            || !damping.is_finite()
            || !max_accel.is_finite()
        {
            return None;
        }
        let anchor_velocity = (anchor - self.anchor) / dt;
        self.anchor = anchor;
        if anchor_velocity.length() > 30.0 {
            return None;
        }
        let radial = centre - anchor;
        let distance = radial.length();
        let Some(direction) = radial.try_normalize() else {
            return Some(velocity);
        };
        if distance > self.length_m + 3.0 {
            return None;
        }
        let extension = (distance - self.length_m).max(0.0);
        let outward = (velocity - anchor_velocity).dot(direction);
        let predicted_extension = (distance + outward * dt - self.length_m).max(0.0);
        if predicted_extension <= 0.0 {
            return Some(velocity);
        }
        let accel = (stiffness.max(0.0) * extension + damping.max(0.0) * outward)
            .clamp(0.0, max_accel.max(0.0));
        Some(velocity - direction * accel * dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grip_tracking_context_and_hand_ownership_loss_cancel_before_driving() {
        for lost in 0..6 {
            let mut input = Continuation {
                enabled: true,
                on_foot: true,
                tracked: true,
                same_hand: true,
                empty_hand: true,
                grip_held: true,
            };
            assert_eq!(input.cancellation(), None);
            match lost {
                0 => input.enabled = false,
                1 => input.on_foot = false,
                2 => input.tracked = false,
                3 => input.same_hand = false,
                4 => input.empty_hand = false,
                _ => input.grip_held = false,
            }
            assert!(input.cancellation().is_some());
        }
    }

    #[test]
    fn hold_bypasses_arrival_and_stall_until_resume_and_disable_cancels() {
        assert_eq!(attached_mode(true, true, false), AttachedMode::Swing);
        assert_eq!(attached_mode(true, true, true), AttachedMode::Swing);
        assert_eq!(attached_mode(true, false, true), AttachedMode::Resume);
        assert_eq!(attached_mode(true, false, false), AttachedMode::Pull);
        assert_eq!(attached_mode(false, true, true), AttachedMode::Cancel);
        assert_eq!(attached_mode(false, false, false), AttachedMode::Pull);
    }

    fn step(centre: Vec3, velocity: Vec3, anchor: Vec3) -> RopeStep {
        RopeStep {
            centre,
            velocity,
            anchor,
            dt: 0.01,
            stiffness: 400.0,
            damping: 30.0,
            max_accel: 120.0,
        }
    }

    #[test]
    fn tension_is_radial_and_bounded_and_release_needs_no_velocity_write() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let before = Vec3::new(5.0, 7.0, -9.0);
        let after = rope.step(step(Vec3::X * 2.1, before, Vec3::ZERO)).unwrap();
        assert_eq!(after.y, before.y);
        assert_eq!(after.z, before.z);
        assert!((before.x - after.x - 1.2).abs() < 1e-5);
        let released = Continuation {
            enabled: true,
            on_foot: true,
            tracked: true,
            same_hand: true,
            empty_hand: true,
            grip_held: false,
        };
        assert_eq!(released.cancellation(), Some("the grip opened"));
        assert_eq!(
            attached_mode(true, false, true),
            AttachedMode::Resume,
            "B release with grip retained resumes pull instead of detaching"
        );
    }

    #[test]
    fn slack_never_pushes_or_drags_the_tangent() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let velocity = Vec3::new(-2.0, 3.0, 4.0);
        assert_eq!(
            rope.step(step(Vec3::X, velocity, Vec3::ZERO)),
            Some(velocity)
        );
    }

    #[test]
    fn moving_anchor_uses_relative_radial_speed() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let velocity = Vec3::new(2.0, 8.0, 0.0);
        assert_eq!(
            rope.step(step(Vec3::X * 2.02, velocity, Vec3::X * 0.02)),
            Some(velocity)
        );
    }

    #[test]
    fn repeated_tension_keeps_a_free_swing_bounded_without_dragging_the_tangent() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let mut centre = Vec3::X * 2.0;
        let mut velocity = Vec3::Y * 6.0;
        for _ in 0..2000 {
            velocity = rope.step(step(centre, velocity, Vec3::ZERO)).unwrap();
            centre += velocity * 0.01;
            assert!(centre.length() < 2.3);
        }
        assert!(velocity.length() > 4.0);
        assert_eq!(rope.length_m, 2.0);
    }

    #[test]
    fn coincident_invalid_and_discontinuous_samples_are_safe() {
        assert!(Sling::new(Vec3::ZERO, Vec3::ZERO).is_none());
        let mut rope = Sling::new(Vec3::X, Vec3::ZERO).unwrap();
        assert_eq!(
            rope.step(step(Vec3::ZERO, Vec3::Y, Vec3::ZERO)),
            Some(Vec3::Y)
        );
        assert!(rope.step(step(Vec3::NAN, Vec3::ZERO, Vec3::ZERO)).is_none());
        assert!(rope.step(step(Vec3::X, Vec3::ZERO, Vec3::Y)).is_none());
        let mut invalid = step(Vec3::X, Vec3::ZERO, Vec3::ZERO);
        invalid.dt = 0.0;
        assert!(rope.step(invalid).is_none());
    }
}
