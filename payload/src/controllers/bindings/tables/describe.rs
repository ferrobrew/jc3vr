//! The describing reader: what each controller input does in a mode, read from the same table
//! declarations the injection evaluates ([`crate::controllers::bindings::tables::declare`]), plus
//! the inputs the mod handles outside the tables (the system button, the body turn, the crouch
//! flick, the sight chord, the recenter and debug panel chords, the weapon gestures, and the
//! parachute handles). Pure; the controls summary ([`crate::controllers::summary`]) labels and draws
//! the rows.

use jc3gi::input::input_action_map::Action;

use crate::controllers::{
    bindings::tables::{
        body_turn_bound,
        declare::{TableCtx, declare},
        sink::{Axis, Direction, Gate, Input, Press, Role, Sink},
    },
    frame::Hand,
    mode::{AirborneVariant, Mode},
};

/// Which way a stick is pushed for a row.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StickDir {
    /// Any direction: the stick as a whole.
    Any,
    Vertical,
    Horizontal,
    Down,
}

/// A physical control, on a physical hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Control {
    Trigger(Hand),
    Grip(Hand),
    A(Hand),
    B(Hand),
    Stick(Hand, StickDir),
    StickClick(Hand),
    /// The Index controller's trackpad.
    Trackpad(Hand),
    /// A motion of the hand itself.
    Motion(Hand),
    /// The grip of either hand.
    EitherGrip,
    /// The A of either hand.
    EitherA,
    /// The B of either hand.
    EitherB,
    /// The stick of either hand.
    EitherStick(StickDir),
    /// The trackpad of either hand.
    EitherTrackpad,
    /// A motion of either hand.
    EitherMotion,
    /// Both stick clicks together.
    BothStickClicks,
    /// The system (menu) button, whichever hand reports it.
    System,
    /// The right B pressed while the left stick click is held.
    LeftStickClickAndRightB,
}

impl Control {
    /// The hand the control is on, or `None` for a control of either hand.
    pub const fn hand(self) -> Option<Hand> {
        match self {
            Control::Trigger(h)
            | Control::Grip(h)
            | Control::A(h)
            | Control::B(h)
            | Control::Stick(h, _)
            | Control::StickClick(h)
            | Control::Trackpad(h)
            | Control::Motion(h) => Some(h),
            Control::EitherGrip
            | Control::EitherA
            | Control::EitherB
            | Control::EitherStick(_)
            | Control::EitherTrackpad
            | Control::EitherMotion
            | Control::BothStickClicks
            | Control::System
            | Control::LeftStickClickAndRightB => None,
        }
    }

    /// The control's place in a listing: the triggers first, then the grips, the face buttons, the
    /// stick, the trackpad, the motions, and the system button.
    pub const fn order(self) -> u8 {
        match self {
            Control::Trigger(_) => 0,
            Control::Grip(_) | Control::EitherGrip => 1,
            Control::A(_) | Control::EitherA => 2,
            Control::B(_) | Control::EitherB => 3,
            Control::Stick(_, StickDir::Any) | Control::EitherStick(StickDir::Any) => 4,
            Control::Stick(_, StickDir::Vertical) | Control::EitherStick(StickDir::Vertical) => 5,
            Control::Stick(_, StickDir::Horizontal)
            | Control::EitherStick(StickDir::Horizontal) => 6,
            Control::Stick(_, StickDir::Down) | Control::EitherStick(StickDir::Down) => 7,
            Control::StickClick(_)
            | Control::BothStickClicks
            | Control::LeftStickClickAndRightB => 8,
            Control::Trackpad(_) | Control::EitherTrackpad => 9,
            Control::Motion(_) | Control::EitherMotion => 10,
            Control::System => 11,
        }
    }
}

/// How the control is worked for a row.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Gesture {
    /// Pressed, pulled, or pushed.
    Press,
    /// Held down past the long-press time.
    Hold,
    /// Pressed right after a flick of the move stick.
    AfterFlick,
    /// Flicked and let go.
    Flick,
    /// Touched.
    Touch,
    /// Pressed on the left half.
    PressLeftHalf,
    /// Pressed on the right half.
    PressRightHalf,
    /// A sharp pull of the hand back toward the body.
    Yank,
    /// A fast strike of the hand.
    Strike,
    /// Pressed while both grips are held.
    WithBothGrips,
    /// Pressed while the same hand's grip is held.
    WithGrip,
}

/// What the mod, rather than the game, does with an input.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ModAction {
    /// Turn the body.
    Turn,
    /// Toggle the seated crouch.
    SeatedCrouch,
    /// Capture the held weapon's sight offset.
    SightCapture,
    /// Draw or holster a weapon at a body holster.
    Holster,
    /// Steady a two-handed weapon by its foregrip.
    Foregrip,
    /// Reload by bringing a magazine from the belt pouch to the weapon.
    Magazine,
    /// Take a grenade from its pouch, and throw it by letting go.
    Grenade,
    /// Take a parachute handle, and pull it down to steer, or both to brake.
    ParachuteHandles,
    /// Show the controls summary.
    ShowSummary,
    /// Recenter the VR view.
    Recenter,
    /// Show or hide the mod's debug panel.
    DebugPanel,
}

/// What a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Drives these game actions; the game's context picks the one that applies.
    Actions(Vec<Action>),
    /// Drives the mod.
    Mod(ModAction),
}

/// One input and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub control: Control,
    pub gesture: Gesture,
    pub effect: Effect,
}

/// The mod's own controller features that add inputs outside the tables, as configured.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct Features {
    /// The yank gesture.
    pub yank: bool,
    pub holsters: bool,
    pub foregrip: bool,
    /// The magazine reload gesture.
    pub magazine: bool,
    /// The physical grenade throw.
    pub grenade_throw: bool,
    /// Physical melee strikes.
    pub melee: bool,
    /// The sight tuning capture chord is armed.
    pub sight_chord: bool,
    /// The trackpad touch shows the summary.
    pub trackpad_shows_summary: bool,
    /// The parachute's handles.
    pub parachute_handles: bool,
}

/// What a description is for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DescribeCtx {
    pub mode: Mode,
    pub dominant: Hand,
    pub table: TableCtx,
    pub features: Features,
}

/// Every input of `ctx.mode` and what it does, with the rows that share an input and a gesture
/// merged, in declaration order, and a face button, stick, or trackpad row that both hands share
/// folded into one.
pub fn describe(ctx: &DescribeCtx) -> Vec<Row> {
    let mut sink = DescribeSink {
        dominant: ctx.dominant,
        yank: ctx.features.yank,
        rows: Vec::new(),
    };
    declare(&mut sink, ctx.mode, &ctx.table);
    let mut rows = sink.rows;
    rows.extend(outside_rows(ctx));
    fold_hands(merge(rows))
}

/// The describing [`Sink`]: records a row per input instead of resolving it.
pub struct DescribeSink {
    dominant: Hand,
    /// Whether the yank gesture is on; off, its presses are left out.
    yank: bool,
    pub rows: Vec<Row>,
}

impl DescribeSink {
    fn press_row(&self, press: Press, actions: &[Action]) -> Option<Row> {
        let hand = press.role.hand(self.dominant);
        let (control, gesture) = match press.input {
            Input::Trigger => (Control::Trigger(hand), Gesture::Press),
            Input::GripDefault => (Control::Grip(hand), Gesture::Press),
            Input::A => (Control::A(hand), Gesture::Press),
            Input::B => (Control::B(hand), Gesture::Press),
            Input::StickClick => (Control::StickClick(hand), Gesture::Press),
            Input::TrackpadLeft => (Control::Trackpad(hand), Gesture::PressLeftHalf),
            Input::TrackpadRight => (Control::Trackpad(hand), Gesture::PressRightHalf),
            Input::Yank if self.yank => (Control::Motion(hand), Gesture::Yank),
            Input::Yank => return None,
            Input::Stick(Direction::Up | Direction::Down) => {
                (Control::Stick(hand, StickDir::Vertical), Gesture::Press)
            }
            Input::Stick(Direction::Left | Direction::Right) => {
                (Control::Stick(hand, StickDir::Horizontal), Gesture::Press)
            }
        };
        let gesture = match press.gate {
            Gate::EvadeFlick => Gesture::AfterFlick,
            Gate::Gripped => Gesture::WithGrip,
            Gate::Always
            | Gate::UnlessChordArmed
            | Gate::UnlessChordGrip
            | Gate::NoEvadeFlick
            | Gate::Ungripped => gesture,
        };
        Some(Row {
            control,
            gesture,
            effect: Effect::Actions(actions.to_vec()),
        })
    }

    fn axis_control(&self, axis: Axis) -> Control {
        let hand = |role: Role| role.hand(self.dominant);
        let off = self.dominant.other();
        match axis {
            Axis::MoveX => Control::Stick(off, StickDir::Horizontal),
            Axis::MoveY => Control::Stick(off, StickDir::Vertical),
            Axis::MoveLength => Control::Stick(off, StickDir::Any),
            Axis::StickX(role) => Control::Stick(hand(role), StickDir::Horizontal),
            Axis::StickY(role) => Control::Stick(hand(role), StickDir::Vertical),
            Axis::StickLength(role) => Control::Stick(hand(role), StickDir::Any),
            Axis::Trigger(role) => Control::Trigger(hand(role)),
        }
    }

    fn push(&mut self, control: Control, actions: Vec<Action>) {
        self.rows.push(Row {
            control,
            gesture: Gesture::Press,
            effect: Effect::Actions(actions),
        });
    }
}

impl Sink for DescribeSink {
    fn hold(&mut self, presses: &[Press], actions: &[Action]) {
        for &press in presses {
            if let Some(row) = self.press_row(press, actions) {
                self.rows.push(row);
            }
        }
    }

    fn click(&mut self, press: Press, action: Action) {
        self.hold(&[press], &[action]);
    }

    fn analog(&mut self, axis: Axis, action: Action) {
        self.push(self.axis_control(axis), vec![action]);
    }

    fn axis(&mut self, axis: Axis, negative: Action, positive: Action) {
        // Forward before back, and left before right, as a reader expects them.
        let actions = match axis {
            Axis::MoveY | Axis::StickY(_) => vec![positive, negative],
            _ => vec![negative, positive],
        };
        self.push(self.axis_control(axis), actions);
    }
}

/// The inputs the mod handles outside the tables, in `ctx.mode`.
fn outside_rows(ctx: &DescribeCtx) -> Vec<Row> {
    let dom = ctx.dominant;
    let off = dom.other();
    let features = ctx.features;
    let mut rows = Vec::new();
    let mut add = |control, gesture, effect| {
        rows.push(Row {
            control,
            gesture,
            effect,
        })
    };
    let modded = |action| Effect::Mod(action);

    if body_turn_bound(ctx.mode) {
        add(
            Control::Stick(dom, StickDir::Horizontal),
            Gesture::Press,
            modded(ModAction::Turn),
        );
    }
    let on_foot = matches!(ctx.mode, Mode::OnFoot(_));
    if on_foot {
        add(
            Control::Stick(dom, StickDir::Down),
            Gesture::Flick,
            modded(ModAction::SeatedCrouch),
        );
        if features.sight_chord {
            add(
                Control::A(dom),
                Gesture::WithBothGrips,
                modded(ModAction::SightCapture),
            );
        }
    }
    // The weapon gestures act where the hands hold the hand weapons.
    let hand_weapons = on_foot
        || matches!(
            ctx.mode,
            Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Parachute)
        );
    if hand_weapons {
        if features.holsters {
            add(
                Control::EitherGrip,
                Gesture::Press,
                modded(ModAction::Holster),
            );
        }
        if features.foregrip {
            add(
                Control::Grip(off),
                Gesture::Press,
                modded(ModAction::Foregrip),
            );
        }
        if features.magazine {
            add(
                Control::Grip(off),
                Gesture::Press,
                modded(ModAction::Magazine),
            );
        }
        if features.grenade_throw {
            add(
                Control::Grip(dom),
                Gesture::Press,
                modded(ModAction::Grenade),
            );
        }
        if features.melee {
            add(
                Control::EitherMotion,
                Gesture::Strike,
                Effect::Actions(vec![Action::PUSH_GRAPPLE]),
            );
        }
    }
    if features.parachute_handles && ctx.mode == Mode::Airborne(AirborneVariant::Parachute) {
        add(
            Control::EitherGrip,
            Gesture::Press,
            modded(ModAction::ParachuteHandles),
        );
    }
    if features.trackpad_shows_summary {
        add(
            Control::Trackpad(off),
            Gesture::Touch,
            modded(ModAction::ShowSummary),
        );
    }
    add(
        Control::BothStickClicks,
        Gesture::Hold,
        modded(ModAction::Recenter),
    );
    add(
        Control::LeftStickClickAndRightB,
        Gesture::Press,
        modded(ModAction::DebugPanel),
    );
    add(
        Control::System,
        Gesture::Press,
        Effect::Actions(vec![Action::GUI_PAUSE]),
    );
    add(
        Control::System,
        Gesture::Hold,
        Effect::Actions(vec![Action::GUI_PDA]),
    );
    rows
}

/// Fold each pair of rows that give both hands' A, B, stick, or trackpad the same gesture and
/// effect into one row on the control of either hand, in the first row's place.
fn fold_hands(rows: Vec<Row>) -> Vec<Row> {
    let mut folded: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows {
        let Some((twin, either)) = twin_of(row.control) else {
            folded.push(row);
            continue;
        };
        let twin = folded
            .iter_mut()
            .find(|r| r.control == twin && r.gesture == row.gesture && r.effect == row.effect);
        match twin {
            Some(twin) => twin.control = either,
            None => folded.push(row),
        }
    }
    folded
}

/// The same control on the other hand, and the control of either hand the two fold into, for the
/// controls that fold.
const fn twin_of(control: Control) -> Option<(Control, Control)> {
    Some(match control {
        Control::A(hand) => (Control::A(hand.other()), Control::EitherA),
        Control::B(hand) => (Control::B(hand.other()), Control::EitherB),
        Control::Stick(hand, dir) => (Control::Stick(hand.other(), dir), Control::EitherStick(dir)),
        Control::Trackpad(hand) => (Control::Trackpad(hand.other()), Control::EitherTrackpad),
        _ => return None,
    })
}

/// Merge the rows that share a control and a gesture and both drive game actions, keeping the
/// first row's place and each action once.
fn merge(rows: Vec<Row>) -> Vec<Row> {
    let mut merged: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows {
        let existing = merged.iter_mut().find(|m| {
            m.control == row.control
                && m.gesture == row.gesture
                && matches!(m.effect, Effect::Actions(_))
        });
        match (existing, row.effect) {
            (
                Some(Row {
                    effect: Effect::Actions(into),
                    ..
                }),
                Effect::Actions(actions),
            ) => {
                for action in actions {
                    if !into.contains(&action) {
                        into.push(action);
                    }
                }
            }
            (_, effect) => merged.push(Row { effect, ..row }),
        }
    }
    merged
}

#[cfg(test)]
mod tests;
