//! The vocabulary the binding tables are declared in, and the evaluating reader.
//!
//! A table names its inputs symbolically ([`Press`], [`Axis`]) and hands them to a [`Sink`] with
//! the actions they drive. Two sinks read the same declarations: [`EvalSink`] resolves each input
//! against the tick's [`Controls`] and collects the wanted actions, and
//! [`DescribeSink`](crate::controllers::bindings::tables::describe::DescribeSink) records what each
//! input drives, for the controls summary. One declaration feeding both is what keeps the summary
//! from drifting from the bindings.

use jc3gi::input::input_action_map::Action;

use crate::controllers::{
    bindings::{
        controls::{ButtonControl, Controls, HandControls, rotate_stick},
        hold::{Desired, MapKind, Target, Want},
    },
    frame::Hand,
};

/// Which hand an input is on: by its role in the layout, or by its physical side. The gameplay
/// tables use the roles, so a left-handed layout mirrors them; the menu table uses the sides.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Role {
    /// The hand in the layout's "right" role ([`Controls::dominant`]).
    Dom,
    /// The hand in the layout's "left" role.
    Off,
    Left,
    Right,
}

impl Role {
    /// The physical hand this role names under `dominant`.
    pub const fn hand(self, dominant: Hand) -> Hand {
        match self {
            Role::Dom => dominant,
            Role::Off => dominant.other(),
            Role::Left => Hand::Left,
            Role::Right => Hand::Right,
        }
    }
}

/// A digital input of one hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Input {
    /// The trigger pulled past its press threshold.
    Trigger,
    /// The grip squeezed with its press latched to the grip's default use rather than to a grab.
    GripDefault,
    A,
    B,
    StickClick,
    /// The trackpad pressed on its left half.
    TrackpadLeft,
    /// The trackpad pressed on its right half.
    TrackpadRight,
    /// The yank gesture fired this tick (a one-tick press, which the game reads as a tap).
    Yank,
    /// The stick pushed one way for menu navigation: held while the push latched to that direction
    /// lasts ([`MenuLatch::stick`]).
    Stick(Direction),
}

/// A menu stick push's direction.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// A per-tick condition an input must also meet, beyond being held.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Gate {
    Always,
    /// Not while both grips arm the sight tuning chord.
    UnlessChordArmed,
    /// Not while the dominant grip is held with the sight tuning chord armed.
    UnlessChordGrip,
    /// Only when the wingsuit A press was latched as an evade flick.
    EvadeFlick,
    /// Only when the wingsuit A press was not latched as an evade flick.
    NoEvadeFlick,
    /// Only when the press began with its own hand's grip held ([`MenuLatch`]).
    Gripped,
    /// Only when the press began without its own hand's grip held ([`MenuLatch`]).
    Ungripped,
}

/// A digital input on a hand, with its gate.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Press {
    pub role: Role,
    pub input: Input,
    pub gate: Gate,
}

impl Press {
    pub const fn new(role: Role, input: Input) -> Self {
        Self {
            role,
            input,
            gate: Gate::Always,
        }
    }

    pub const fn gated(self, gate: Gate) -> Self {
        Self { gate, ..self }
    }

    /// The yank gesture, on the grapple hand.
    pub const YANK: Press = Press::new(Role::Off, Input::Yank);
}

/// An analog input.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Axis {
    /// The move stick's horizontal axis, rotated by [`Controls::move_rotation`].
    MoveX,
    /// The move stick's vertical axis, rotated by [`Controls::move_rotation`].
    MoveY,
    /// The rotated move stick's deflection, capped at one.
    MoveLength,
    /// A stick's horizontal axis, +X right.
    StickX(Role),
    /// A stick's vertical axis, +Y forward.
    StickY(Role),
    /// A stick's deflection, capped at one.
    StickLength(Role),
    /// A trigger's pull, `0..=1`.
    Trigger(Role),
}

/// What the tables report their declarations to.
pub trait Sink {
    /// Hold every action in `actions` while any of `presses` is held.
    fn hold(&mut self, presses: &[Press], actions: &[Action]);
    /// Click `action` on the press edge of `press`.
    fn click(&mut self, press: Press, action: Action);
    /// Drive `action` with an analog input's `0..=1` value.
    fn analog(&mut self, axis: Axis, action: Action);
    /// Split a signed axis into its two non-negative half-axis actions.
    fn axis(&mut self, axis: Axis, negative: Action, positive: Action);
}

/// The per-tick state the gates and the latched grips read, decided by
/// [`Bindings::step`](crate::controllers::bindings::tables::Bindings::step) before the tables run.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Latches {
    /// Whether each hand's grip press is latched to its default use, by [`Hand::index`].
    pub grip_default: [bool; 2],
    /// The sight tuning chord's dominant grip is held.
    pub chord_grip: bool,
    /// Both of the sight tuning chord's grips are held.
    pub chord_armed: bool,
    /// The wingsuit A press was latched as an evade flick.
    pub evade: bool,
    /// Each hand's menu presses, by [`Hand::index`].
    pub menu: [MenuLatch; 2],
}

/// One hand's menu presses, each latched at its press edge with whether that hand's grip was held
/// then, so that squeezing or letting go of the grip mid-press never turns one action into another.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct MenuLatch {
    /// Whether the A press began with the grip held, while A is held.
    pub a: Option<bool>,
    /// Whether the B press began with the grip held, while B is held.
    pub b: Option<bool>,
    /// The direction the stick push latched to, and whether it began with the grip held.
    pub stick: Option<(Direction, bool)>,
}

impl MenuLatch {
    /// Whether the press of `input` latched with the grip held, or `None` while it is not held (or
    /// for an input that does not latch).
    pub const fn gripped(&self, input: Input) -> Option<bool> {
        match input {
            Input::A => self.a,
            Input::B => self.b,
            Input::Stick(_) => match self.stick {
                Some((_, gripped)) => Some(gripped),
                None => None,
            },
            _ => None,
        }
    }
}

/// The evaluating reader: resolves the tables' inputs against one tick's controls and collects the
/// wanted actions, all on one map.
pub struct EvalSink<'a> {
    controls: &'a Controls,
    latches: Latches,
    pub map: MapKind,
    pub desired: Vec<Desired>,
}

impl<'a> EvalSink<'a> {
    pub fn new(controls: &'a Controls, latches: Latches, map: MapKind) -> Self {
        Self {
            controls,
            latches,
            map,
            desired: Vec::with_capacity(32),
        }
    }

    fn hand(&self, role: Role) -> &HandControls {
        self.controls.hand(role.hand(self.controls.dominant))
    }

    fn button(&self, role: Role, input: Input) -> Option<ButtonControl> {
        let hand = self.hand(role);
        match input {
            Input::Trigger => Some(hand.trigger_press),
            Input::GripDefault => Some(hand.grip),
            Input::A => Some(hand.a),
            Input::B => Some(hand.b),
            Input::StickClick => Some(hand.stick_click),
            Input::TrackpadLeft => Some(hand.trackpad_left),
            Input::TrackpadRight => Some(hand.trackpad_right),
            Input::Yank | Input::Stick(_) => None,
        }
    }

    fn menu(&self, role: Role) -> &MenuLatch {
        &self.latches.menu[role.hand(self.controls.dominant).index()]
    }

    fn gate(&self, press: Press) -> bool {
        match press.gate {
            Gate::Always => true,
            Gate::UnlessChordArmed => !self.latches.chord_armed,
            Gate::UnlessChordGrip => !self.latches.chord_grip,
            Gate::EvadeFlick => self.latches.evade,
            Gate::NoEvadeFlick => !self.latches.evade,
            Gate::Gripped => self.menu(press.role).gripped(press.input) == Some(true),
            Gate::Ungripped => self.menu(press.role).gripped(press.input) == Some(false),
        }
    }

    fn held(&self, press: Press) -> bool {
        let held = match press.input {
            Input::GripDefault => {
                self.latches.grip_default[press.role.hand(self.controls.dominant).index()]
            }
            Input::Yank => self.controls.yank,
            Input::Stick(direction) => self
                .menu(press.role)
                .stick
                .is_some_and(|(latched, _)| latched == direction),
            input => self.button(press.role, input).is_some_and(|b| b.held()),
        };
        held && self.gate(press)
    }

    fn value(&self, axis: Axis) -> f32 {
        let c = self.controls;
        let moved = || rotate_stick(c.off().stick, c.move_rotation);
        match axis {
            Axis::MoveX => moved().x,
            Axis::MoveY => moved().y,
            Axis::MoveLength => moved().length().min(1.0),
            Axis::StickX(role) => self.hand(role).stick.x,
            Axis::StickY(role) => self.hand(role).stick.y,
            Axis::StickLength(role) => self.hand(role).stick.length().min(1.0),
            Axis::Trigger(role) => self.hand(role).trigger,
        }
    }

    fn push(&mut self, action: Action, want: Want) {
        self.desired.push(Desired {
            target: Target {
                map: self.map,
                action,
            },
            want,
        });
    }

    fn push_analog(&mut self, value: f32, action: Action) {
        if value > 0.0 {
            self.push(action, Want::Analog(value));
        }
    }
}

impl Sink for EvalSink<'_> {
    fn hold(&mut self, presses: &[Press], actions: &[Action]) {
        if presses.iter().any(|&p| self.held(p)) {
            for &action in actions {
                self.push(action, Want::Hold(1.0));
            }
        }
    }

    fn click(&mut self, press: Press, action: Action) {
        let pressed = match press.input {
            Input::Yank => self.controls.yank,
            input => self.button(press.role, input).is_some_and(|b| b.pressed),
        };
        if pressed && self.gate(press) {
            self.push(action, Want::Click);
        }
    }

    fn analog(&mut self, axis: Axis, action: Action) {
        self.push_analog(self.value(axis), action);
    }

    fn axis(&mut self, axis: Axis, negative: Action, positive: Action) {
        let value = self.value(axis);
        self.push_analog(-value, negative);
        self.push_analog(value, positive);
    }
}
