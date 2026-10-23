//! The aim-down-sights pose. Pure.
//!
//! The gun is a rigid body in the holding hand's wrist frame. Each step below moves it as a whole,
//! and the hands on it move with it:
//!
//! 1. The stock ([`shoulder_move`]): with the butt near the shoulder pocket, the gun turns about
//!    its front hand (the hand on the foregrip, or the holding hand without one) so that the butt
//!    lies on the line to the pocket. The front hand then sets where the gun points and the
//!    shoulder holds its back, as a shouldered rifle is held.
//! 2. The sights ([`eye_move`]): with the eye just behind the gun and close to its sight line,
//!    the gun turns about a point ahead of the holding grip on the line (about where the front
//!    sight is) so that the line passes through the eye. The front sight stays where the hands put
//!    it, and the eye, the rear sight, and the front sight line up: the shot goes where the player
//!    sees the sights point.
//! 3. A scope's cheek weld ([`weld_move`]): a scope is lined up by moving the gun across its axis,
//!    keeping its direction, so the eye sits on the axis and the scope's view centres.
//!
//! Each step fades in as its distance closes from its engage distance to its lock distance, so a
//! gun brought up to the eye settles onto it rather than snapping.

use glam::{Quat, Vec3};

use crate::controllers::{frame::Pose, hands::CockpitHand, weapons::ads::config::AdsConfig};

/// A gun's sight line in its holding wrist's frame, in cockpit metres.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WristSight {
    pub origin: Vec3,
    /// A unit vector.
    pub direction: Vec3,
}

/// What the pose is solved from, all in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AdsInputs {
    pub holder: CockpitHand,
    pub sight: WristSight,
    /// The off-hand's grip while it holds the foregrip.
    pub support: Option<Vec3>,
    /// The aiming eye.
    pub eye: Vec3,
    /// The head's forward.
    pub forward: Vec3,
    /// The shoulder pocket on the holding hand's side, when known.
    pub pocket: Option<Vec3>,
    /// Whether the gun is scoped, which welds rather than turns.
    pub scoped: bool,
}

/// A rigid move: `p' = rotation * p + translation`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigidMove {
    pub rotation: Quat,
    pub translation: Vec3,
}

impl RigidMove {
    pub const IDENTITY: RigidMove = RigidMove {
        rotation: Quat::IDENTITY,
        translation: Vec3::ZERO,
    };

    /// A turn by `rotation` about `pivot`.
    pub fn about(pivot: Vec3, rotation: Quat) -> RigidMove {
        RigidMove {
            rotation,
            translation: pivot - rotation * pivot,
        }
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        self.rotation * p + self.translation
    }

    pub fn direction(&self, d: Vec3) -> Vec3 {
        self.rotation * d
    }

    pub fn pose(&self, p: Pose) -> Pose {
        Pose {
            position: self.point(p.position),
            orientation: (self.rotation * p.orientation).normalize(),
        }
    }

    pub fn hand(&self, h: CockpitHand) -> CockpitHand {
        CockpitHand {
            wrist: self.pose(h.wrist),
            grip: self.pose(h.grip),
            aim: self.pose(h.aim),
        }
    }

    /// This move, then `next`.
    pub fn then(&self, next: RigidMove) -> RigidMove {
        RigidMove {
            rotation: (next.rotation * self.rotation).normalize(),
            translation: next.rotation * self.translation + next.translation,
        }
    }
}

/// The solved pose's move and how far each step took hold.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AdsSolution {
    pub moved: RigidMove,
    /// The share of each step, `0..=1`.
    pub shoulder: f32,
    pub eye: f32,
    pub weld: f32,
    /// How far the eye was off the sight line before the sights were lined up, metres, while the
    /// eye was behind the gun and the head looked along it.
    pub eye_offset_m: Option<f32>,
}

impl AdsSolution {
    /// A pose that takes no hold: the gun where the hands hold it.
    pub const NONE: AdsSolution = AdsSolution {
        moved: RigidMove::IDENTITY,
        shoulder: 0.0,
        eye: 0.0,
        weld: 0.0,
        eye_offset_m: None,
    };
}

/// Solve the pose.
pub fn solve(inputs: &AdsInputs, config: &AdsConfig) -> AdsSolution {
    let mut solution = AdsSolution::NONE;
    if config.shoulder
        && let Some(pocket) = inputs.pocket
        && let Some((step, weight)) = shoulder_move(inputs, pocket, config)
    {
        solution.moved = step;
        solution.shoulder = weight;
    }
    let line = sight_line(inputs, solution.moved);
    let grip = solution.moved.point(inputs.holder.grip.position);
    let Some(view) = eye_view(line, grip, inputs.eye, inputs.forward, config) else {
        return solution;
    };
    solution.eye_offset_m = Some(view.offset.length());
    let step = if inputs.scoped {
        config.cheek_weld.then(|| {
            let weight = fade(
                view.offset.length(),
                config.weld_lock_m,
                config.weld_engage_m,
            );
            solution.weld = weight;
            weld_move(view, weight)
        })
    } else {
        let weight = fade(view.offset.length(), config.lock_m, config.engage_m);
        solution.eye = weight;
        Some(eye_move(
            line,
            grip,
            inputs.eye,
            config.pivot_ahead_m,
            weight,
        ))
    };
    if let Some(step) = step {
        solution.moved = solution.moved.then(step);
    }
    solution
}

/// The gun's sight line in the cockpit frame after `moved`: (origin, direction).
fn sight_line(inputs: &AdsInputs, moved: RigidMove) -> (Vec3, Vec3) {
    let wrist = inputs.holder.wrist;
    let origin = wrist.position + wrist.orientation * inputs.sight.origin;
    let direction = (wrist.orientation * inputs.sight.direction).normalize_or_zero();
    (moved.point(origin), moved.direction(direction))
}

/// The turn that seats the butt in `pocket`, faded by its distance, and its weight.
pub fn shoulder_move(
    inputs: &AdsInputs,
    pocket: Vec3,
    config: &AdsConfig,
) -> Option<(RigidMove, f32)> {
    let (_, direction) = sight_line(inputs, RigidMove::IDENTITY);
    let grip = inputs.holder.grip.position;
    let butt = grip - direction * config.butt_behind_grip_m;
    let weight = fade(
        butt.distance(pocket),
        config.shoulder_lock_m,
        config.shoulder_engage_m,
    );
    if weight <= 0.0 {
        return None;
    }
    let front = inputs.support.unwrap_or(grip);
    let from = (butt - front).try_normalize()?;
    let to = (pocket - front).try_normalize()?;
    let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(from, to), weight);
    Some((RigidMove::about(front, turn), weight))
}

/// Where the eye is relative to the sight line.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EyeView {
    /// The eye's offset from its nearest point on the line, across the line.
    pub offset: Vec3,
}

/// The eye's view of the line `(origin, direction)` held at `grip`: `None` unless the eye is behind
/// the grip by no more than the configured distance and the head looks along the line.
pub fn eye_view(
    (origin, direction): (Vec3, Vec3),
    grip: Vec3,
    eye: Vec3,
    forward: Vec3,
    config: &AdsConfig,
) -> Option<EyeView> {
    let behind = -(eye - grip).dot(direction);
    if !(0.0..=config.max_eye_behind_m).contains(&behind) {
        return None;
    }
    let forward = forward.try_normalize()?;
    if forward.dot(direction) < config.max_view_angle_deg.to_radians().cos() {
        return None;
    }
    let along = (eye - origin).dot(direction);
    let offset = eye - (origin + direction * along);
    offset.is_finite().then_some(EyeView { offset })
}

/// The turn about a point `pivot_ahead_m` ahead of `grip` on the line that brings the line through
/// `eye`, at `weight`.
pub fn eye_move(
    (origin, direction): (Vec3, Vec3),
    grip: Vec3,
    eye: Vec3,
    pivot_ahead_m: f32,
    weight: f32,
) -> RigidMove {
    let pivot = origin + direction * ((grip - origin).dot(direction) + pivot_ahead_m);
    let Some(through_eye) = (pivot - eye).try_normalize() else {
        return RigidMove::IDENTITY;
    };
    let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(direction, through_eye), weight);
    RigidMove::about(pivot, turn)
}

/// The move across the line that puts it through the eye, at `weight`.
pub fn weld_move(view: EyeView, weight: f32) -> RigidMove {
    RigidMove {
        rotation: Quat::IDENTITY,
        translation: view.offset * weight,
    }
}

/// `1` at or under `lock`, `0` at or over `engage`, eased between.
pub fn fade(distance: f32, lock: f32, engage: f32) -> f32 {
    if !distance.is_finite() {
        return 0.0;
    }
    if engage <= lock {
        return if distance <= lock { 1.0 } else { 0.0 };
    }
    let t = ((engage - distance) / (engage - lock)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A right hand holding a rifle pointing down -Z, its sight line 8 cm above the grip, at head
    /// height in front of the right eye.
    fn inputs(eye: Vec3) -> AdsInputs {
        let pose = |position| Pose {
            position,
            orientation: Quat::IDENTITY,
        };
        let grip = Vec3::new(0.03, 1.5, -0.3);
        AdsInputs {
            holder: CockpitHand {
                wrist: pose(grip),
                grip: pose(grip),
                aim: pose(grip),
            },
            sight: WristSight {
                origin: Vec3::new(0.0, 0.08, 0.1),
                direction: Vec3::NEG_Z,
            },
            support: None,
            eye,
            forward: Vec3::NEG_Z,
            pocket: None,
            scoped: false,
        }
    }

    fn line_distance(solution: &AdsSolution, inputs: &AdsInputs, point: Vec3) -> f32 {
        let (o, d) = sight_line(inputs, solution.moved);
        let along = (point - o).dot(d);
        (point - (o + d * along)).length()
    }

    #[test]
    fn an_eye_close_to_the_line_is_brought_onto_it() {
        let config = AdsConfig::new();
        // The line runs at y = 1.58, x = 0.03; the eye is 1 cm off it, behind the grip.
        let i = inputs(Vec3::new(0.04, 1.58, -0.05));
        let s = solve(&i, &config);
        assert!(s.eye > 0.99, "{s:?}");
        assert!(line_distance(&s, &i, i.eye) < 1e-4);
        // The pivot ahead of the grip stays put.
        let pivot = Vec3::new(0.03, 1.58, -0.3 - config.pivot_ahead_m);
        assert!(s.moved.point(pivot).distance(pivot) < 1e-4);
    }

    #[test]
    fn an_eye_far_from_the_line_leaves_the_gun_alone() {
        let config = AdsConfig::new();
        let i = inputs(Vec3::new(0.2, 1.58, -0.05));
        let s = solve(&i, &config);
        assert_eq!(s.eye, 0.0);
        assert_eq!(s.moved.point(Vec3::ONE), Vec3::ONE);
    }

    #[test]
    fn a_gun_the_head_does_not_look_along_is_not_aimed() {
        let config = AdsConfig::new();
        let mut i = inputs(Vec3::new(0.04, 1.58, -0.05));
        i.forward = Vec3::X;
        assert_eq!(solve(&i, &config).eye_offset_m, None);
        // An eye in front of the grip is not behind the sights.
        let i = inputs(Vec3::new(0.04, 1.58, -0.6));
        assert_eq!(solve(&i, &config).eye_offset_m, None);
    }

    #[test]
    fn a_scope_is_welded_across_its_axis_without_turning() {
        let config = AdsConfig::new();
        let mut i = inputs(Vec3::new(0.04, 1.585, -0.1));
        i.scoped = true;
        let s = solve(&i, &config);
        assert!(s.weld > 0.99);
        assert_eq!(s.eye, 0.0);
        assert!(s.moved.rotation.angle_between(Quat::IDENTITY) < 1e-5);
        assert!(line_distance(&s, &i, i.eye) < 1e-4);
    }

    #[test]
    fn the_butt_is_seated_in_the_pocket() {
        let config = AdsConfig::new();
        let mut i = inputs(Vec3::new(0.5, 3.0, 0.0));
        let butt = i.holder.grip.position + Vec3::Z * config.butt_behind_grip_m;
        let pocket = butt + Vec3::new(0.02, -0.02, 0.0);
        i.pocket = Some(pocket);
        let s = solve(&i, &config);
        assert!(s.shoulder > 0.99);
        // The grip stays, and the line from it back to the pocket is the gun's.
        let grip = i.holder.grip.position;
        assert!(s.moved.point(grip).distance(grip) < 1e-5);
        let moved_butt = s.moved.point(butt);
        let back = (moved_butt - grip).normalize();
        assert!(back.dot((pocket - grip).normalize()) > 0.9999);
    }

    #[test]
    fn a_butt_away_from_the_shoulder_is_left_alone() {
        let config = AdsConfig::new();
        let mut i = inputs(Vec3::new(0.5, 3.0, 0.0));
        i.pocket = Some(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(solve(&i, &config).shoulder, 0.0);
    }

    #[test]
    fn the_fade_eases_from_engage_to_lock() {
        assert_eq!(fade(0.1, 0.02, 0.06), 0.0);
        assert_eq!(fade(0.01, 0.02, 0.06), 1.0);
        assert!((fade(0.04, 0.02, 0.06) - 0.5).abs() < 1e-6);
        assert_eq!(fade(f32::NAN, 0.02, 0.06), 0.0);
        assert_eq!(fade(0.03, 0.05, 0.05), 1.0);
    }

    #[test]
    fn moves_compose_in_order() {
        let a = RigidMove::about(Vec3::ZERO, Quat::from_rotation_y(0.5));
        let b = RigidMove {
            rotation: Quat::IDENTITY,
            translation: Vec3::X,
        };
        let p = Vec3::new(0.0, 0.0, 1.0);
        let both = a.then(b);
        assert!(both.point(p).distance(b.point(a.point(p))) < 1e-6);
    }
}
