//! When the summary shows, and where: the panel's pose on the off-hand wrist, the glance at it, and
//! the fade. Pure.
//!
//! The panel lies on the back of the off-hand's wrist, like a watch face, read with the forearm
//! across the body. It shows while the player glances at it: the panel sits within a cone around
//! the head's forward and faces the head, for a short dwell (so that a hand sweeping past the view
//! in a fight does not show it), and it lingers briefly after the look ends. Both angles widen once
//! it shows, so that it does not flicker at the edge.

use glam::{Mat3, Quat, Vec3};

use crate::controllers::{
    frame::{Hand, Pose},
    summary::config::{GlanceConfig, WristPlacement},
};

/// The panel's pose in the world: its centre, and the rotation whose +X runs along its text, +Y up
/// its page, and +Z out of its face toward the reader.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PanelPose {
    pub center: Vec3,
    pub rotation: Quat,
}

impl PanelPose {
    /// The direction the panel's face looks.
    pub fn normal(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    /// The corners of a `width` by `height` panel: top left, top right, bottom left, bottom right,
    /// the order the quad pass takes them in.
    pub fn corners(&self, width: f32, height: f32) -> [Vec3; 4] {
        let right = self.rotation * Vec3::X * (width * 0.5);
        let up = self.rotation * Vec3::Y * (height * 0.5);
        [
            self.center - right + up,
            self.center + right + up,
            self.center - right - up,
            self.center + right - up,
        ]
    }
}

/// The panel's pose on `hand`'s wrist, from the controller's grip pose (OpenXR's grip axes: -Z
/// along the closed fist, +X out of the palm of the left hand and into the palm of the right, so
/// that -X is the back of either hand).
pub fn wrist_pose(grip: Pose, hand: Hand, placement: &WristPlacement) -> PanelPose {
    PanelPose {
        center: grip.position + grip.orientation * placement.offset_m,
        rotation: (grip.orientation * face(hand)).normalize(),
    }
}

/// The panel's rotation in the grip frame. The face looks out of the back of the hand. With the
/// forearm across the body, the text runs toward the left hand's fingers and toward the right
/// hand's elbow, which is left to right for the reader either way.
fn face(hand: Hand) -> Quat {
    let (right, up) = match hand {
        Hand::Left => (Vec3::NEG_Z, Vec3::NEG_Y),
        Hand::Right => (Vec3::Z, Vec3::Y),
    };
    Quat::from_mat3(&Mat3::from_cols(right, up, Vec3::NEG_X))
}

/// Whether the head at `head` is looking at `panel`: the panel within the view cone and its face
/// turned to the head. `widen` adds the hysteresis to both angles.
pub fn looking(head: Pose, panel: &PanelPose, config: &GlanceConfig, widen: bool) -> bool {
    let to_panel = panel.center - head.position;
    let Some(to_panel) = to_panel.try_normalize() else {
        return false;
    };
    let extra = if widen { config.hysteresis_deg } else { 0.0 };
    let forward = head.orientation * Vec3::NEG_Z;
    let in_view = forward.angle_between(to_panel).to_degrees() <= config.view_cone_deg + extra;
    let facing = panel.normal().angle_between(-to_panel).to_degrees() <= config.facing_deg + extra;
    in_view && facing
}

/// The glance's state across frames.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Glance {
    shown: bool,
    /// How long the look has held (while hidden) or been gone (while shown), seconds.
    timer: f32,
}

impl Glance {
    pub const fn new() -> Self {
        Self {
            shown: false,
            timer: 0.0,
        }
    }

    /// Whether the glance shows the panel: a look must hold for the dwell to show it, and be gone
    /// for the linger to hide it.
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Advance by `dt` seconds with the look as given, and return whether the panel is shown.
    pub fn step(&mut self, looking: bool, dt: f32, config: &GlanceConfig) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if looking != self.shown {
            self.timer += dt;
            let wait = if self.shown {
                config.linger_s
            } else {
                config.dwell_s
            };
            if self.timer >= wait {
                self.shown = looking;
                self.timer = 0.0;
            }
        } else {
            self.timer = 0.0;
        }
        self.shown
    }
}

/// The trackpad touch's pin across frames. A touch held for [`TrackpadPin::DELAY_S`] pins the
/// panel until the thumb lifts; a press during the touch (which pages the menu tabs) cancels the
/// pin for the rest of that touch, and the delay keeps a touch that is about to press from
/// flashing the panel first.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TrackpadPin {
    /// How long the current touch has held, seconds, or `None` without a touch.
    touched_s: Option<f32>,
    /// Whether the current touch has pressed.
    pressed: bool,
}

impl TrackpadPin {
    /// How long a touch holds before it pins, seconds.
    pub const DELAY_S: f32 = 0.2;

    pub const fn new() -> Self {
        Self {
            touched_s: None,
            pressed: false,
        }
    }

    /// Advance by `dt` seconds with the trackpad's touch and press as given, and return whether
    /// the panel is pinned.
    pub fn step(&mut self, touched: bool, pressed: bool, dt: f32) -> bool {
        if !touched && !pressed {
            *self = Self::new();
            return false;
        }
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let held = self.touched_s.map_or(0.0, |s| s + dt);
        self.touched_s = Some(held);
        self.pressed |= pressed;
        !self.pressed && held >= Self::DELAY_S
    }
}

/// Move the panel's opacity toward fully shown or hidden over `fade_s` seconds.
pub fn fade(alpha: f32, shown: bool, dt: f32, fade_s: f32) -> f32 {
    let target = if shown { 1.0 } else { 0.0 };
    if fade_s <= 0.0 || !dt.is_finite() {
        return target;
    }
    let step = dt.max(0.0) / fade_s;
    if alpha < target {
        (alpha + step).min(target)
    } else {
        (alpha - step).max(target)
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    const PLACEMENT: WristPlacement = WristPlacement::new();
    const GLANCE: GlanceConfig = GlanceConfig::new();

    /// A head `distance` metres in front of the panel's face, looking at it.
    fn head_facing(panel: &PanelPose, distance: f32) -> Pose {
        let position = panel.center + panel.normal() * distance;
        let forward = -panel.normal();
        Pose {
            position,
            orientation: Quat::from_rotation_arc(Vec3::NEG_Z, forward),
        }
    }

    #[test]
    fn the_panel_faces_out_of_the_back_of_either_hand() {
        for hand in Hand::BOTH {
            let panel = wrist_pose(Pose::IDENTITY, hand, &PLACEMENT);
            assert!(panel.normal().abs_diff_eq(Vec3::NEG_X, 1e-5), "{hand:?}");
            assert!(panel.center.abs_diff_eq(PLACEMENT.offset_m, 1e-5));
            // A proper rotation: the text, the page, and the face form a right-handed frame.
            let (x, y) = (panel.rotation * Vec3::X, panel.rotation * Vec3::Y);
            assert!(x.cross(y).abs_diff_eq(panel.normal(), 1e-5));
        }
        // The text runs toward the left hand's fingers and the right hand's elbow.
        let left = wrist_pose(Pose::IDENTITY, Hand::Left, &PLACEMENT);
        let right = wrist_pose(Pose::IDENTITY, Hand::Right, &PLACEMENT);
        assert!((left.rotation * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-5));
        assert!((right.rotation * Vec3::X).abs_diff_eq(Vec3::Z, 1e-5));
    }

    #[test]
    fn the_panel_follows_the_grip() {
        let grip = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::from_rotation_y(FRAC_PI_2),
        };
        let panel = wrist_pose(grip, Hand::Left, &PLACEMENT);
        let expected = grip.position + grip.orientation * PLACEMENT.offset_m;
        assert!(panel.center.abs_diff_eq(expected, 1e-5));
        assert!(
            panel
                .normal()
                .abs_diff_eq(grip.orientation * Vec3::NEG_X, 1e-5)
        );
    }

    #[test]
    fn a_look_needs_the_panel_in_view_and_facing_the_head() {
        let panel = wrist_pose(Pose::IDENTITY, Hand::Left, &PLACEMENT);
        let head = head_facing(&panel, 0.4);
        assert!(looking(head, &panel, &GLANCE, false));
        // The head turned well away.
        let away = Pose {
            orientation: head.orientation * Quat::from_rotation_y(1.2),
            ..head
        };
        assert!(!looking(away, &panel, &GLANCE, false));
        // The head behind the panel, looking at its back.
        let behind = head_facing(
            &PanelPose {
                rotation: panel.rotation * Quat::from_rotation_y(std::f32::consts::PI),
                ..panel
            },
            0.4,
        );
        assert!(!looking(behind, &panel, &GLANCE, false));
    }

    #[test]
    fn the_hysteresis_widens_the_look_once_shown() {
        let panel = wrist_pose(Pose::IDENTITY, Hand::Left, &PLACEMENT);
        let head = head_facing(&panel, 0.4);
        let edge = (GLANCE.view_cone_deg + GLANCE.hysteresis_deg * 0.5).to_radians();
        let off = Pose {
            orientation: head.orientation * Quat::from_rotation_x(edge),
            ..head
        };
        assert!(!looking(off, &panel, &GLANCE, false));
        assert!(looking(off, &panel, &GLANCE, true));
    }

    #[test]
    fn a_glance_dwells_before_showing_and_lingers_before_hiding() {
        let dt = 0.01;
        // How long the glance takes to flip to `to`, seconds, holding the look at `to`.
        let flip = |glance: &mut Glance, to: bool| {
            let mut elapsed = 0.0;
            while glance.step(to, dt, &GLANCE) != to {
                elapsed += dt;
                assert!(elapsed < 10.0, "never flipped");
            }
            elapsed + dt
        };
        let mut glance = Glance::default();
        // A look broken before the dwell starts over.
        glance.step(true, GLANCE.dwell_s * 0.8, &GLANCE);
        assert!(!glance.step(false, dt, &GLANCE));
        let shown_after = flip(&mut glance, true);
        assert!(
            (shown_after - GLANCE.dwell_s).abs() <= dt * 1.5,
            "{shown_after}"
        );
        assert!(glance.shown());
        let hidden_after = flip(&mut glance, false);
        assert!(
            (hidden_after - GLANCE.linger_s).abs() <= dt * 1.5,
            "{hidden_after}"
        );
        assert!(!glance.shown());
    }

    #[test]
    fn the_fade_moves_toward_its_target_and_stops_there() {
        assert_eq!(fade(0.0, true, 0.05, 0.1), 0.5);
        assert_eq!(fade(0.9, true, 0.05, 0.1), 1.0);
        assert_eq!(fade(0.1, false, 0.05, 0.1), 0.0);
        assert_eq!(fade(0.3, true, 0.05, 0.0), 1.0);
    }

    #[test]
    fn the_corners_span_the_panel() {
        let panel = PanelPose {
            center: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        let [tl, tr, bl, br] = panel.corners(2.0, 4.0);
        assert_eq!(tl, Vec3::new(-1.0, 2.0, 0.0));
        assert_eq!(tr, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(bl, Vec3::new(-1.0, -2.0, 0.0));
        assert_eq!(br, Vec3::new(1.0, -2.0, 0.0));
    }

    #[test]
    fn a_held_touch_pins_after_the_delay_until_lifted() {
        let mut pin = TrackpadPin::new();
        assert!(!pin.step(true, false, 0.0));
        assert!(!pin.step(true, false, TrackpadPin::DELAY_S * 0.5));
        assert!(pin.step(true, false, TrackpadPin::DELAY_S));
        assert!(!pin.step(false, false, 0.01));
        assert!(!pin.step(true, false, 0.01));
    }

    #[test]
    fn a_press_cancels_the_pin_for_the_rest_of_the_touch() {
        let mut pin = TrackpadPin::new();
        assert!(!pin.step(true, false, 0.05));
        assert!(!pin.step(true, true, 0.05));
        assert!(!pin.step(true, false, 1.0));
        assert!(!pin.step(false, false, 0.01));
        // The next touch pins again.
        pin.step(true, false, 0.0);
        assert!(pin.step(true, false, TrackpadPin::DELAY_S));
    }
}
