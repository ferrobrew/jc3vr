//! The controller frame and its edges, reduced to the logical controls the binding tables read:
//! per-hand buttons with their edges, deadzoned sticks and triggers, and the grab volume each hand
//! is in. Pure.

use glam::{Quat, Vec2, Vec3};

use crate::{
    controllers::{
        bindings::grab::GrabVolume,
        config::BindingsConfig,
        frame::{Button, ControllerFrame, Edges, Hand, HandFrame},
    },
    vr::input::{
        layout::{Input, Vec2Input, provides},
        semantic::bound::PhysicalSet,
    },
};

/// One digital input over the tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ButtonControl {
    /// Down at the latest frame.
    pub down: bool,
    /// Pressed at some frame since the last tick.
    pub pressed: bool,
    /// Released at some frame since the last tick.
    pub released: bool,
}

impl ButtonControl {
    /// Whether the button counts as held this tick. A press and release between two ticks still
    /// holds for one tick, so that a quick tap reaches the game.
    pub const fn held(self) -> bool {
        self.down || self.pressed
    }

    fn of(hand: &HandFrame, edges: &Edges, which: Hand, button: Button) -> Self {
        Self {
            down: hand.buttons.contains(button),
            pressed: edges.pressed(which, button),
            released: edges.released(which, button),
        }
    }
}

/// One controller's logical controls.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandControls {
    /// The trigger's pull past the trigger deadzone, rescaled to `0..=1`.
    pub trigger: f32,
    /// The trigger pulled past the press threshold (with hysteresis).
    pub trigger_press: ButtonControl,
    /// The grip squeezed past the press threshold (with hysteresis).
    pub grip: ButtonControl,
    /// The lower face button (A; X on the left Touch controller).
    pub a: ButtonControl,
    /// The upper face button (B; Y on the left Touch controller).
    pub b: ButtonControl,
    pub stick_click: ButtonControl,
    /// The system (or menu) button.
    pub system: ButtonControl,
    /// The trackpad pressed on its left half.
    pub trackpad_left: ButtonControl,
    /// The trackpad pressed on its right half.
    pub trackpad_right: ButtonControl,
    /// The thumbstick past the radial deadzone, rescaled, +Y forward.
    pub stick: Vec2,
    /// The thumbstick as reported, for consumers with their own deadzone (the body turn).
    pub stick_raw: Vec2,
    /// The thumb's position on the trackpad, each axis `-1..=1`, zero on a controller without one.
    pub trackpad: Vec2,
    /// The grab volume the hand is in.
    pub grab_volume: Option<GrabVolume>,
}

/// Both controllers' logical controls, and the layout's handedness.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Controls {
    /// Indexed by [`Hand::index`].
    pub hands: [HandControls; 2],
    /// The hand in the layout's "right" role: the guns, the fire trigger, and the body turn. The
    /// other hand takes the "left" role: the grapple, the move stick, and the tethers.
    pub dominant: Hand,
    /// The angle (radians, counter-clockwise from above) to rotate the move stick by on foot, so
    /// that its forward is the configured locomotion reference rather than the head.
    pub move_rotation: f32,
    /// Whether the move stick's click cycles to the next weapon on foot: the fallback for drawing
    /// weapons, on whenever the holsters are off.
    pub weapon_cycle: bool,
    /// Whether a pistol is in the off-hand with the dual-wield split acting, in a mode whose
    /// triggers fire the hand weapons: the off-hand's trigger fires its pistol, and where it would
    /// retract the tethers the off-hand's stick click does instead.
    pub dual_wield: bool,
    /// Whether the yank gesture fired this tick: a grapple tap.
    pub yank: bool,
    /// Whether the sight tuning capture chord is armed: the tuning tool is on and its marker is
    /// placed.
    pub sight_chord: bool,
    /// The hand whose trigger belongs to the native UI pointer.
    pub pointer_hand: Option<Hand>,
    /// Whether a controller has a trackpad, so the menus page with the trackpads rather than the
    /// grips.
    pub trackpad_paging: bool,
    /// Each hand masked whole from the semantic actions by input recovery, by [`Hand::index`].
    pub masked_hands: [bool; 2],
    /// The inputs of each hand claimed by mechanisms outside the tables (the debug panel chord),
    /// masked from the semantic actions, by [`Hand::index`].
    pub masked_inputs: [PhysicalSet; 2],
}

impl Controls {
    /// Derive the controls from a published frame, the edges since the last tick, and the grab
    /// volume of each hand.
    pub fn from_frame(
        frame: &ControllerFrame,
        edges: &Edges,
        config: &BindingsConfig,
        dominant: Hand,
        move_rotation: f32,
        grab_volumes: [Option<GrabVolume>; 2],
    ) -> Self {
        let hands = Hand::BOTH.map(|which| {
            let hand = frame.hand(which);
            let button = |b| ButtonControl::of(hand, edges, which, b);
            HandControls {
                trigger: scalar_deadzone(hand.inputs.trigger, config.trigger_deadzone),
                trigger_press: button(Button::TriggerPress),
                grip: button(Button::SqueezePress),
                a: button(Button::AClick),
                b: button(Button::BClick),
                stick_click: button(Button::ThumbstickClick),
                system: button(Button::SystemClick),
                trackpad_left: button(Button::TrackpadPressLeft),
                trackpad_right: button(Button::TrackpadPressRight),
                stick: radial_deadzone(hand.inputs.thumbstick, config.move_deadzone),
                stick_raw: hand.inputs.thumbstick,
                trackpad: hand.inputs.trackpad,
                grab_volume: grab_volumes[which.index()],
            }
        });
        Self {
            hands,
            dominant,
            move_rotation,
            weapon_cycle: false,
            dual_wield: false,
            yank: false,
            sight_chord: false,
            pointer_hand: None,
            masked_hands: [false; 2],
            masked_inputs: [PhysicalSet::EMPTY; 2],
            trackpad_paging: Hand::BOTH.into_iter().any(|which| {
                provides(
                    frame.hand(which).profile,
                    which,
                    Input::Vec2(Vec2Input::Trackpad),
                )
            }),
        }
    }

    pub fn hand(&self, hand: Hand) -> &HandControls {
        &self.hands[hand.index()]
    }

    /// The hand in the layout's "right" role.
    pub fn dom(&self) -> &HandControls {
        self.hand(self.dominant)
    }

    /// The hand in the layout's "left" role.
    pub fn off(&self) -> &HandControls {
        self.hand(self.dominant.other())
    }
}

/// A stick with a radial deadzone: zero inside `deadzone`, and outside it rescaled so that the
/// magnitude runs from zero at the deadzone's edge to one at full deflection, keeping the
/// direction.
pub fn radial_deadzone(stick: Vec2, deadzone: f32) -> Vec2 {
    let magnitude = stick.length();
    let deadzone = deadzone.clamp(0.0, 0.99);
    if magnitude <= deadzone || !magnitude.is_finite() {
        return Vec2::ZERO;
    }
    let scaled = (magnitude.min(1.0) - deadzone) / (1.0 - deadzone);
    stick * (scaled / magnitude)
}

/// A `0..=1` axis with a deadzone at zero, rescaled so that it still reaches one.
pub fn scalar_deadzone(value: f32, deadzone: f32) -> f32 {
    let deadzone = deadzone.clamp(0.0, 0.99);
    if !value.is_finite() || value <= deadzone {
        return 0.0;
    }
    ((value.min(1.0) - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0)
}

/// Rotate a stick vector (+X right, +Y forward) counter-clockwise, seen from above, by `angle`
/// radians.
pub fn rotate_stick(stick: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(stick.x * cos - stick.y * sin, stick.x * sin + stick.y * cos)
}

/// The yaw of an orientation in the cockpit frame (OpenXR axes: +Y up, -Z forward), radians,
/// counter-clockwise from above, zero looking down -Z. `None` when the orientation's forward is
/// near vertical and has no meaningful heading.
pub fn yaw_of(orientation: Quat) -> Option<f32> {
    let forward = orientation * Vec3::NEG_Z;
    let ground = Vec2::new(forward.x, forward.z);
    (ground.length_squared() > 1e-4).then(|| (-forward.x).atan2(-forward.z))
}

#[cfg(test)]
mod tests {
    use std::{f32::consts::FRAC_PI_2, time::Instant};

    use super::*;
    use crate::controllers::frame::{ActionSetKind, InteractionProfile};

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn radial_deadzone_rescales_from_the_edge() {
        assert_eq!(radial_deadzone(Vec2::new(0.1, 0.0), 0.2), Vec2::ZERO);
        assert!(close(
            radial_deadzone(Vec2::new(0.0, 1.0), 0.2),
            Vec2::new(0.0, 1.0)
        ));
        assert!(close(
            radial_deadzone(Vec2::new(0.6, 0.0), 0.2),
            Vec2::new(0.5, 0.0)
        ));
        // Past full deflection (a square gate's corner) the magnitude is capped at one.
        let corner = radial_deadzone(Vec2::new(1.0, 1.0), 0.2);
        assert!((corner.length() - 1.0).abs() < 1e-5);
        assert_eq!(radial_deadzone(Vec2::new(f32::NAN, 0.0), 0.2), Vec2::ZERO);
    }

    #[test]
    fn scalar_deadzone_reaches_one() {
        assert_eq!(scalar_deadzone(0.04, 0.05), 0.0);
        assert_eq!(scalar_deadzone(1.0, 0.05), 1.0);
        assert!((scalar_deadzone(0.525, 0.05) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn rotating_left_turns_forward_into_left() {
        assert!(close(
            rotate_stick(Vec2::Y, FRAC_PI_2),
            Vec2::new(-1.0, 0.0)
        ));
        assert!(close(rotate_stick(Vec2::X, FRAC_PI_2), Vec2::Y));
        assert!(close(
            rotate_stick(Vec2::new(0.3, 0.4), 0.0),
            Vec2::new(0.3, 0.4)
        ));
    }

    #[test]
    fn yaw_is_counter_clockwise_from_forward() {
        assert!(yaw_of(Quat::IDENTITY).unwrap().abs() < 1e-5);
        let left = Quat::from_rotation_y(FRAC_PI_2);
        assert!((yaw_of(left).unwrap() - FRAC_PI_2).abs() < 1e-5);
        let down = Quat::from_rotation_x(-FRAC_PI_2);
        assert_eq!(yaw_of(down), None);
    }

    #[test]
    fn a_tap_between_ticks_holds_for_one_tick() {
        let tap = ButtonControl {
            down: false,
            pressed: true,
            released: true,
        };
        assert!(tap.held());
        assert!(!ButtonControl::default().held());
    }

    fn frame_with_profiles(profiles: [InteractionProfile; 2]) -> ControllerFrame {
        let mut hands: [HandFrame; 2] = Default::default();
        for (hand, profile) in hands.iter_mut().zip(profiles) {
            hand.active = profile != InteractionProfile::None;
            hand.profile = profile;
        }
        ControllerFrame {
            sequence: 0,
            display_time_ns: 0,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::Ui,
            hands,
        }
    }

    #[test]
    fn the_menus_page_with_the_trackpads_only_on_a_controller_that_has_one() {
        let paging = |profiles| {
            Controls::from_frame(
                &frame_with_profiles(profiles),
                &Edges::default(),
                &BindingsConfig::new(),
                Hand::Right,
                0.0,
                [None; 2],
            )
            .trackpad_paging
        };
        let index = InteractionProfile::ValveIndex;
        let none = InteractionProfile::None;
        assert!(paging([index, index]));
        assert!(paging([none, index]));
        assert!(!paging([InteractionProfile::OculusTouch; 2]));
        assert!(!paging([InteractionProfile::Other; 2]));
        assert!(!paging([none, none]));
    }

    #[test]
    fn the_trackpad_halves_carry_their_edges() {
        let mut frame = frame_with_profiles([InteractionProfile::ValveIndex; 2]);
        frame.hands[0].buttons.set(Button::TrackpadPressLeft, true);
        let mut edges = Edges::default();
        edges.pressed[1].set(Button::TrackpadPressRight, true);
        edges.released[1].set(Button::TrackpadPressRight, true);
        let c = Controls::from_frame(
            &frame,
            &edges,
            &BindingsConfig::new(),
            Hand::Right,
            0.0,
            [None; 2],
        );
        assert!(c.hand(Hand::Left).trackpad_left.down);
        assert!(!c.hand(Hand::Left).trackpad_right.held());
        assert!(c.hand(Hand::Right).trackpad_right.held());
    }
}
