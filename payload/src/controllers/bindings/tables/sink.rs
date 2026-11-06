//! The vocabulary the binding tables are declared in, and the evaluating reader.
//!
//! A table names its inputs as semantic actions ([`Press`], [`Axis`]) and hands them to a [`Sink`]
//! with the game actions they drive. Two sinks read the same declarations: [`EvalSink`] resolves
//! each input against the tick's [`Intents`] and collects the wanted actions, and
//! [`DescribeSink`](crate::controllers::bindings::tables::describe::DescribeSink) records what each
//! input drives, for the controls summary. One declaration feeding both is what keeps the summary
//! from drifting from the bindings.
//!
//! A table never names a hand for a press: the player's binding decides which hand an action is
//! on, and a press counts on either hand. The per-hand state the tables need besides (each hand's
//! menu modifier and its pad press) is latched per hand and read on the hand of the press.

use glam::Vec2;
use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::{
        bindings::{
            controls::{ButtonControl, rotate_stick},
            hold::{Desired, MapKind, Target, Want},
            intents::Intents,
        },
        frame::Hand,
    },
    vr::input::semantic::{
        SemanticSet,
        vocabulary::{ActionId, MenuAction, OnFootAction},
    },
};

/// A digital input of the tables.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Input {
    /// A semantic action held: a boolean action, or a float action past its press thresholds.
    Action(ActionId),
    /// A pad press action ([`pad_of`]) held, with the zone of the pad it latched to at its press
    /// edge ([`PadLatch`]).
    Zone(ActionId, PadZone),
    /// The menu stick pushed one way: held while the push latched to that direction lasts
    /// ([`MenuLatch::stick`]).
    MenuStick(Direction),
    /// A short press of a semantic action: held for the one tick at which a press released before
    /// the long-press time ends ([`PressTimer`]).
    Short(ActionId),
    /// A long press of a semantic action: held for the one tick at which the press reaches the
    /// long-press time, a tap however long the press goes on ([`PressTimer`]).
    Long(ActionId),
}

impl Input {
    /// The semantic action the input reads, for the inputs that read one.
    pub const fn action(self) -> Option<ActionId> {
        match self {
            Input::Action(a) | Input::Zone(a, _) | Input::Short(a) | Input::Long(a) => Some(a),
            Input::MenuStick(_) => None,
        }
    }
}

/// A menu stick push's direction.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// A zone of a trackpad, as a pad press latches to it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum PadZone {
    Left,
    Right,
    Up,
    Down,
}

/// How a pad press divides its pad.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum PadLayout {
    /// The halves left and right of the vertical centre line.
    Halves,
    /// The four quadrants of a d-pad, by the position's larger axis.
    Quadrants,
}

impl PadLayout {
    /// The zone of the pad at `position` (+X right, +Y up), or `None` within
    /// [`PAD_ZONE_MARGIN`] of the lines between the zones, where a press waits for the thumb to
    /// move off.
    pub fn zone(self, position: Vec2) -> Option<PadZone> {
        if !position.is_finite() {
            return None;
        }
        let (x, y) = (position.x, position.y);
        match self {
            PadLayout::Halves if x <= -PAD_ZONE_MARGIN => Some(PadZone::Left),
            PadLayout::Halves if x >= PAD_ZONE_MARGIN => Some(PadZone::Right),
            PadLayout::Halves => None,
            PadLayout::Quadrants => {
                if x.abs().max(y.abs()) < PAD_ZONE_MARGIN || (x.abs() - y.abs()).abs() < 1e-6 {
                    None
                } else if y.abs() > x.abs() {
                    Some(if y > 0.0 { PadZone::Up } else { PadZone::Down })
                } else {
                    Some(if x > 0.0 {
                        PadZone::Right
                    } else {
                        PadZone::Left
                    })
                }
            }
        }
    }
}

/// How far from the line between two pad zones, in the pad's `-1..=1` axes, a press must land to
/// count for a zone.
pub const PAD_ZONE_MARGIN: f32 = 0.2;

/// The position action that goes with a pad press action, and how the press divides the pad, for
/// the pad press actions.
pub fn pad_of(press: ActionId) -> Option<(ActionId, PadLayout)> {
    if press == MenuAction::MenuTabPress.id() {
        Some((MenuAction::MenuTabPad.id(), PadLayout::Halves))
    } else if press == OnFootAction::WeaponSelectPress.id() {
        Some((OnFootAction::WeaponSelectPad.id(), PadLayout::Quadrants))
    } else {
        None
    }
}

/// The pad press action of `set`, if it has one.
pub fn pad_press_of(set: SemanticSet) -> Option<ActionId> {
    match set {
        SemanticSet::Menu => Some(MenuAction::MenuTabPress.id()),
        SemanticSet::OnFoot => Some(OnFootAction::WeaponSelectPress.id()),
        SemanticSet::Global
        | SemanticSet::Wingsuit
        | SemanticSet::LandVehicle
        | SemanticSet::Helicopter
        | SemanticSet::Plane => None,
    }
}

/// A per-tick condition an input must also meet, beyond being held.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Gate {
    Always,
    /// Not while both grips arm the sight tuning chord.
    UnlessChordArmed,
    /// Not while the dominant grip is held with the sight tuning chord armed.
    UnlessChordGrip,
    /// Only when the press began with its own hand's menu modifier held ([`MenuLatch`]).
    Gripped,
    /// Only when the press began without its own hand's menu modifier held ([`MenuLatch`]).
    Ungripped,
    /// Only while the character is a ragdoll.
    Ragdolling,
    /// Only on a hand holding a gun ([`Latches::armed`]).
    HandArmed,
    /// Only on a hand holding no gun ([`Latches::armed`]).
    HandUnarmed,
}

/// A digital input, with its gate.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Press {
    pub input: Input,
    pub gate: Gate,
}

impl Press {
    pub const fn new(input: Input) -> Self {
        Self {
            input,
            gate: Gate::Always,
        }
    }

    pub const fn gated(self, gate: Gate) -> Self {
        Self { gate, ..self }
    }
}

/// An analog input.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Axis {
    /// A vector action's horizontal axis, +X right, rotated by the move rotation
    /// ([`Controls::move_rotation`](crate::controllers::bindings::controls::Controls::move_rotation)).
    MoveX(ActionId),
    /// A vector action's vertical axis, +Y forward, rotated by the move rotation.
    MoveY(ActionId),
    /// A rotated vector action's deflection, capped at one.
    MoveLength(ActionId),
    /// A vector action's horizontal axis, +X right.
    X(ActionId),
    /// A vector action's vertical axis, +Y forward.
    Y(ActionId),
    /// A vector action's deflection, capped at one.
    Length(ActionId),
    /// A float action's value past its deadzone.
    Value(ActionId),
}

impl Axis {
    /// The action the axis reads.
    pub const fn action(self) -> ActionId {
        match self {
            Axis::MoveX(a)
            | Axis::MoveY(a)
            | Axis::MoveLength(a)
            | Axis::X(a)
            | Axis::Y(a)
            | Axis::Length(a)
            | Axis::Value(a) => a,
        }
    }
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

/// The per-tick state the gates and the latched inputs read, decided by
/// [`Bindings::step`](crate::controllers::bindings::tables::Bindings::step) before the tables run.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Latches {
    /// The sight tuning chord's dominant grip is held.
    pub chord_grip: bool,
    /// Both of the sight tuning chord's grips are held.
    pub chord_armed: bool,
    /// The character is a ragdoll.
    pub ragdolling: bool,
    /// Each hand holds a gun, by [`Hand::index`].
    pub armed: [bool; 2],
    /// Each hand's short and long presses of the timed actions, by [`Hand::index`].
    pub timed: [TimedLatch; 2],
    /// Each hand's menu presses, by [`Hand::index`].
    pub menu: [MenuLatch; 2],
    /// Each hand's pad press, by [`Hand::index`].
    pub pads: [PadLatch; 2],
}

/// The most semantic actions whose presses are told apart by length in one mode.
pub const MAX_TIMED: usize = 4;

/// One hand's presses of the timed actions as of this tick: for each, whether a short press ended
/// this tick and whether a press reached the long-press time this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TimedLatch {
    pub presses: [Option<(ActionId, TimedPress)>; MAX_TIMED],
}

impl TimedLatch {
    /// The press state of `action`, or the idle state for an action that is not timed.
    pub fn of(&self, action: ActionId) -> TimedPress {
        self.presses
            .iter()
            .flatten()
            .find(|(a, _)| *a == action)
            .map_or(TimedPress::default(), |&(_, press)| press)
    }
}

/// A timed action's press on one hand, this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TimedPress {
    /// A press released before the long-press time ended this tick.
    pub short: bool,
    /// The press reached the long-press time this tick.
    pub long: bool,
}

/// Times one hand's presses of one action, to tell a short press from a long one.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PressTimer {
    /// How long the current press has been held, seconds.
    held: Option<f32>,
}

impl PressTimer {
    /// Advance by `dt` seconds with the action's `button` this tick. A press and release between
    /// two ticks counts as held for one tick, so a quick tap ends as a short press on the next.
    pub fn step(&mut self, button: ButtonControl, dt: f32, long_press_s: f32) -> TimedPress {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if button.held() {
            let before = self.held;
            let seconds = before.map_or(0.0, |s| s + dt);
            self.held = Some(seconds);
            TimedPress {
                short: false,
                long: seconds >= long_press_s && before.is_none_or(|s| s < long_press_s),
            }
        } else {
            let ended = self.held.take();
            TimedPress {
                short: ended.is_some_and(|s| s < long_press_s),
                long: false,
            }
        }
    }
}

/// One hand's menu presses, each latched at its press edge with whether that hand's menu modifier
/// was held then, so that squeezing or letting go of the grip mid-press never turns one action
/// into another.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct MenuLatch {
    /// Whether the confirm press began with the modifier held, while confirm is held.
    pub confirm: Option<bool>,
    /// Whether the back press began with the modifier held, while back is held.
    pub back: Option<bool>,
    /// The direction the stick push latched to, and whether it began with the modifier held.
    pub stick: Option<(Direction, bool)>,
    /// Whether the previous page press began with the modifier held, while it is held.
    pub page_prev: Option<bool>,
    /// Whether the next page press began with the modifier held, while it is held.
    pub page_next: Option<bool>,
}

/// One hand's pad press: the press action and the zone it latched to at its press edge, held
/// until the press is released. A press on a line between zones waits for the thumb to move off.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct PadLatch {
    pub press: Option<(ActionId, Option<PadZone>)>,
}

impl PadLatch {
    /// The zone `press` latched to, while it is held.
    pub fn zone(&self, press: ActionId) -> Option<PadZone> {
        match self.press {
            Some((latched, zone)) if latched == press => zone,
            _ => None,
        }
    }
}

/// The evaluating reader: resolves the tables' inputs against one tick's intents and collects the
/// wanted actions, all on one map.
pub struct EvalSink<'a> {
    intents: &'a Intents,
    latches: Latches,
    move_rotation: f32,
    /// The menu actions whose presses latch the modifier: confirm, back, and the previous and next
    /// page.
    menu_presses: [ActionId; 4],
    pub map: MapKind,
    pub desired: Vec<Desired>,
}

impl<'a> EvalSink<'a> {
    pub fn new(intents: &'a Intents, latches: Latches, move_rotation: f32, map: MapKind) -> Self {
        Self {
            intents,
            latches,
            move_rotation,
            menu_presses: [
                MenuAction::MenuConfirm.id(),
                MenuAction::MenuBack.id(),
                MenuAction::MenuPagePrev.id(),
                MenuAction::MenuPageNext.id(),
            ],
            map,
            desired: Vec::with_capacity(32),
        }
    }

    /// Whether the press of `input` on `hand` latched with the modifier held, or `None` while it
    /// is not held (or for an input that does not latch).
    fn gripped(&self, input: Input, hand: Hand) -> Option<bool> {
        let latch = &self.latches.menu[hand.index()];
        match input {
            Input::Action(a) if a == self.menu_presses[0] => latch.confirm,
            Input::Action(a) if a == self.menu_presses[1] => latch.back,
            Input::Action(a) if a == self.menu_presses[2] => latch.page_prev,
            Input::Action(a) if a == self.menu_presses[3] => latch.page_next,
            Input::MenuStick(_) => latch.stick.map(|(_, gripped)| gripped),
            _ => None,
        }
    }

    fn gate(&self, press: Press, hand: Hand) -> bool {
        match press.gate {
            Gate::Always => true,
            Gate::UnlessChordArmed => !self.latches.chord_armed,
            Gate::UnlessChordGrip => !self.latches.chord_grip,
            Gate::Gripped => self.gripped(press.input, hand) == Some(true),
            Gate::Ungripped => self.gripped(press.input, hand) == Some(false),
            Gate::Ragdolling => self.latches.ragdolling,
            Gate::HandArmed => self.latches.armed[hand.index()],
            Gate::HandUnarmed => !self.latches.armed[hand.index()],
        }
    }

    fn held_on(&self, input: Input, hand: Hand) -> bool {
        match input {
            Input::Action(action) => self.intents.hand(action, hand).button.held(),
            Input::Zone(action, zone) => {
                self.latches.pads[hand.index()].zone(action) == Some(zone)
                    && self.intents.hand(action, hand).button.held()
            }
            Input::MenuStick(direction) => self.latches.menu[hand.index()]
                .stick
                .is_some_and(|(latched, _)| latched == direction),
            Input::Short(action) => self.latches.timed[hand.index()].of(action).short,
            Input::Long(action) => self.latches.timed[hand.index()].of(action).long,
        }
    }

    fn pressed_on(&self, input: Input, hand: Hand) -> bool {
        match input {
            Input::Action(action) => self.intents.hand(action, hand).button.pressed,
            Input::Zone(action, zone) => {
                self.latches.pads[hand.index()].zone(action) == Some(zone)
                    && self.intents.hand(action, hand).button.pressed
            }
            // A short or a long press is held for one tick, so its tick is its press.
            Input::Short(action) => self.latches.timed[hand.index()].of(action).short,
            Input::Long(action) => self.latches.timed[hand.index()].of(action).long,
            Input::MenuStick(_) => false,
        }
    }

    fn value(&self, axis: Axis) -> f32 {
        let moved = |a| rotate_stick(self.intents.vector(a), self.move_rotation);
        match axis {
            Axis::MoveX(a) => moved(a).x,
            Axis::MoveY(a) => moved(a).y,
            Axis::MoveLength(a) => moved(a).length().min(1.0),
            Axis::X(a) => self.intents.vector(a).x,
            Axis::Y(a) => self.intents.vector(a).y,
            Axis::Length(a) => self.intents.vector(a).length().min(1.0),
            Axis::Value(a) => self.intents.value(a),
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
        let held = presses.iter().any(|&p| {
            Hand::BOTH
                .into_iter()
                .any(|h| self.held_on(p.input, h) && self.gate(p, h))
        });
        if held {
            for &action in actions {
                self.push(action, Want::Hold(1.0));
            }
        }
    }

    fn click(&mut self, press: Press, action: Action) {
        let pressed = Hand::BOTH
            .into_iter()
            .any(|h| self.pressed_on(press.input, h) && self.gate(press, h));
        if pressed {
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
