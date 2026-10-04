//! The describing reader: what each controller input does in a mode, read from the same table
//! declarations the injection evaluates ([`crate::controllers::bindings::tables::declare`]), plus
//! the inputs the mod handles outside the tables (the pause button, the body turn, the crouch
//! flick, the sight chord, the recenter and debug panel chords, the weapon gestures, and the
//! parachute handles). Pure; the controls summary ([`crate::controllers::summary`]) labels and draws
//! the rows.
//!
//! The tables name semantic actions; each is described on the physical inputs its default binding
//! ([`crate::vr::input::semantic::defaults`]) puts it on, less the inputs the table context masks
//! ([`crate::controllers::bindings::tables::context_masks`]).

use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::{
        bindings::{
            intents::{Mask, YANK_ACTIONS},
            tables::{
                body_turn_action, context_masks,
                declare::{TableCtx, declare},
                sink::{Axis, Direction, Gate, Input, PadLayout, PadZone, Press, Sink, pad_of},
            },
        },
        frame::Hand,
        mode::{AirborneVariant, Mode, VehicleClass},
    },
    vr::input::semantic::{
        bound::{Layout, PhysicalSet},
        defaults::{Component, default_binding},
        vocabulary::{ActionId, GlobalAction, MenuAction, OnFootAction},
    },
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
    /// Pressed on the top quadrant.
    PressUp,
    /// Pressed on the bottom quadrant.
    PressDown,
    /// Pressed on the left quadrant.
    PressLeft,
    /// Pressed on the right quadrant.
    PressRight,
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
    /// Take a C4 charge from its chest pouch, and throw it by letting go.
    C4,
    /// Push the dominant fingertip into the wrist button to detonate the placed charges.
    WristDetonator,
    /// Take a parachute handle, and pull it down to steer, or both to brake.
    ParachuteHandles,
    /// Spread the arms in the wingsuit and lower or raise them to bank, dive, and climb.
    ArmSteering,
    /// Hold the car's or the boat's wheel to steer.
    Wheel,
    /// Hold the handlebars to steer.
    Handlebars,
    /// Hold the helicopter's cyclic and collective to fly.
    HelicopterControls,
    /// Hold the plane's flight stick and throttle to fly.
    PlaneControls,
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
    /// The C4 throw from the chest pouch.
    pub c4_throw: bool,
    /// The wrist detonator.
    pub wrist_detonator: bool,
    /// Physical melee strikes.
    pub melee: bool,
    /// The sight tuning capture chord is armed.
    pub sight_chord: bool,
    /// The trackpad touch shows the summary.
    pub trackpad_shows_summary: bool,
    /// The parachute's handles.
    pub parachute_handles: bool,
    /// The wingsuit's arm steering.
    pub arm_steering: bool,
    /// The physical vehicle controls offered to a driver, by kind.
    pub physical: PhysicalFeatures,
}

/// The physical vehicle controls that are on, by kind.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct PhysicalFeatures {
    pub wheel: bool,
    pub handlebars: bool,
    /// The helicopter's cyclic or collective.
    pub helicopter: bool,
    /// The plane's flight stick or throttle.
    pub plane: bool,
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
    describe_with(ctx, default_binding)
}

/// [`describe`], with the actions on the inputs `layout` binds them to.
pub(crate) fn describe_with(ctx: &DescribeCtx, layout: Layout) -> Vec<Row> {
    let mut sink = DescribeSink {
        dominant: ctx.dominant,
        yank: ctx.features.yank,
        layout,
        masks: context_masks(ctx.mode, &ctx.table, ctx.dominant),
        rows: Vec::new(),
    };
    declare(&mut sink, ctx.mode, &ctx.table);
    let mut rows = sink.rows;
    rows.extend(outside_rows(ctx, layout));
    fold_hands(merge(rows))
}

/// The describing [`Sink`]: records a row per input instead of resolving it.
pub struct DescribeSink {
    dominant: Hand,
    /// Whether the yank gesture is on; off, its rows are left out.
    yank: bool,
    layout: Layout,
    masks: Vec<Mask>,
    pub rows: Vec<Row>,
}

impl DescribeSink {
    /// The hands and components `action` is bound to, in [`Hand::BOTH`] order, less the masked.
    fn bound(&self, action: ActionId) -> Vec<(Hand, Component)> {
        bound_inputs(self.layout, action, self.dominant)
            .into_iter()
            .filter(|&(hand, component)| {
                !self.masks.iter().any(|m| {
                    m.hand == hand && m.covers(action, PhysicalSet::of(component.physical()))
                })
            })
            .collect()
    }

    fn press_rows(&self, press: Press, actions: &[Action]) -> Vec<Row> {
        let gate = |gesture| match press.gate {
            Gate::EvadeFlick => Gesture::AfterFlick,
            Gate::Gripped => Gesture::WithGrip,
            Gate::Always
            | Gate::UnlessChordArmed
            | Gate::UnlessChordGrip
            | Gate::NoEvadeFlick
            | Gate::Ungripped => gesture,
        };
        let row = |control, gesture| Row {
            control,
            gesture: gate(gesture),
            effect: Effect::Actions(actions.to_vec()),
        };
        let mut rows = Vec::new();
        match press.input {
            Input::Action(action) => {
                for (hand, component) in self.bound(action) {
                    rows.push(row(
                        press_control(component, hand),
                        press_gesture(component),
                    ));
                }
                if self.yank && YANK_ACTIONS.contains(&action) {
                    rows.push(row(Control::Motion(self.dominant.other()), Gesture::Yank));
                }
            }
            Input::Zone(action, zone) => {
                let layout = pad_of(action).map_or(PadLayout::Halves, |(_, layout)| layout);
                for (hand, _) in self.bound(action) {
                    rows.push(row(Control::Trackpad(hand), zone_gesture(layout, zone)));
                }
            }
            Input::MenuStick(direction) => {
                let dir = match direction {
                    Direction::Up | Direction::Down => StickDir::Vertical,
                    Direction::Left | Direction::Right => StickDir::Horizontal,
                };
                for (hand, _) in self.bound(MenuAction::MenuNavigate.id()) {
                    rows.push(row(Control::Stick(hand, dir), Gesture::Press));
                }
            }
        }
        rows
    }

    fn axis_controls(&self, axis: Axis) -> Vec<Control> {
        let dir = match axis {
            Axis::MoveX(_) | Axis::X(_) => StickDir::Horizontal,
            Axis::MoveY(_) | Axis::Y(_) => StickDir::Vertical,
            Axis::MoveLength(_) | Axis::Length(_) => StickDir::Any,
            Axis::Value(_) => StickDir::Any,
        };
        self.bound(axis.action())
            .into_iter()
            .map(|(hand, component)| match (axis, component) {
                (Axis::Value(_), component) => press_control(component, hand),
                (_, Component::Trackpad) => Control::Trackpad(hand),
                _ => Control::Stick(hand, dir),
            })
            .collect()
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
            let rows = self.press_rows(press, actions);
            self.rows.extend(rows);
        }
    }

    fn click(&mut self, press: Press, action: Action) {
        self.hold(&[press], &[action]);
    }

    fn analog(&mut self, axis: Axis, action: Action) {
        for control in self.axis_controls(axis) {
            self.push(control, vec![action]);
        }
    }

    fn axis(&mut self, axis: Axis, negative: Action, positive: Action) {
        // Forward before back, and left before right, as a reader expects them.
        let actions = match axis {
            Axis::MoveY(_) | Axis::Y(_) => vec![positive, negative],
            _ => vec![negative, positive],
        };
        for control in self.axis_controls(axis) {
            self.push(control, actions.clone());
        }
    }
}

/// The hands and components `layout` binds `action` to under `dominant`, in [`Hand::BOTH`] order.
fn bound_inputs(layout: Layout, action: ActionId, dominant: Hand) -> Vec<(Hand, Component)> {
    let Some(binding) = layout(action.set, action.index()) else {
        return Vec::new();
    };
    Hand::BOTH
        .into_iter()
        .filter(|&hand| binding.side.covers(hand, dominant))
        .map(|hand| (hand, binding.component))
        .collect()
}

/// The control a press of `component` on `hand` is.
const fn press_control(component: Component, hand: Hand) -> Control {
    match component {
        Component::TriggerValue => Control::Trigger(hand),
        Component::GripPress => Control::Grip(hand),
        Component::LowerFace => Control::A(hand),
        Component::UpperFace => Control::B(hand),
        Component::ThumbstickClick => Control::StickClick(hand),
        Component::Thumbstick => Control::Stick(hand, StickDir::Any),
        Component::ThumbstickX => Control::Stick(hand, StickDir::Horizontal),
        Component::ThumbstickY => Control::Stick(hand, StickDir::Vertical),
        Component::Trackpad | Component::TrackpadForce | Component::TrackpadTouch => {
            Control::Trackpad(hand)
        }
        Component::System => Control::System,
    }
}

/// How a press of `component` is worked.
const fn press_gesture(component: Component) -> Gesture {
    match component {
        Component::TrackpadTouch => Gesture::Touch,
        _ => Gesture::Press,
    }
}

/// The gesture of a pad press latched to `zone`.
const fn zone_gesture(layout: PadLayout, zone: PadZone) -> Gesture {
    match (layout, zone) {
        (PadLayout::Halves, PadZone::Left) => Gesture::PressLeftHalf,
        (PadLayout::Halves, PadZone::Right) => Gesture::PressRightHalf,
        (PadLayout::Halves, PadZone::Up) | (PadLayout::Quadrants, PadZone::Up) => Gesture::PressUp,
        (PadLayout::Halves, PadZone::Down) | (PadLayout::Quadrants, PadZone::Down) => {
            Gesture::PressDown
        }
        (PadLayout::Quadrants, PadZone::Left) => Gesture::PressLeft,
        (PadLayout::Quadrants, PadZone::Right) => Gesture::PressRight,
    }
}

/// The inputs the mod handles outside the tables, in `ctx.mode`.
fn outside_rows(ctx: &DescribeCtx, layout: Layout) -> Vec<Row> {
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

    let bound = |action: ActionId| bound_inputs(layout, action, dom);
    if let Some(turn) = body_turn_action(ctx.mode) {
        for (hand, component) in bound(turn) {
            add(
                press_control(component, hand),
                Gesture::Press,
                modded(ModAction::Turn),
            );
        }
    }
    let on_foot = matches!(ctx.mode, Mode::OnFoot(_));
    if on_foot {
        for (hand, component) in bound(OnFootAction::CrouchFlick.id()) {
            let control = match press_control(component, hand) {
                Control::Stick(hand, _) => Control::Stick(hand, StickDir::Down),
                control => control,
            };
            add(control, Gesture::Flick, modded(ModAction::SeatedCrouch));
        }
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
        if features.c4_throw {
            add(Control::EitherGrip, Gesture::Press, modded(ModAction::C4));
        }
        if features.melee {
            add(
                Control::EitherMotion,
                Gesture::Strike,
                Effect::Actions(vec![Action::PUSH_GRAPPLE]),
            );
        }
    }
    if features.wrist_detonator && ctx.mode != Mode::Ui {
        add(
            Control::Motion(dom),
            Gesture::Press,
            modded(ModAction::WristDetonator),
        );
    }
    if features.parachute_handles && ctx.mode == Mode::Airborne(AirborneVariant::Parachute) {
        add(
            Control::EitherGrip,
            Gesture::Press,
            modded(ModAction::ParachuteHandles),
        );
    }
    if features.arm_steering && ctx.mode == Mode::Airborne(AirborneVariant::Wingsuit) {
        add(
            Control::EitherMotion,
            Gesture::Press,
            modded(ModAction::ArmSteering),
        );
    }
    if let Mode::Vehicle(seat) = ctx.mode
        && seat.driver
    {
        let physical = features.physical;
        let held = match seat.class {
            VehicleClass::Car | VehicleClass::Sea if physical.wheel => Some(ModAction::Wheel),
            VehicleClass::Motorcycle if physical.handlebars => Some(ModAction::Handlebars),
            VehicleClass::Helicopter if physical.helicopter => Some(ModAction::HelicopterControls),
            VehicleClass::Plane if physical.plane => Some(ModAction::PlaneControls),
            _ => None,
        };
        if let Some(action) = held {
            add(Control::EitherGrip, Gesture::Press, modded(action));
        }
    }
    if features.trackpad_shows_summary {
        for (hand, component) in bound(GlobalAction::ShowControlsSummary.id()) {
            add(
                press_control(component, hand),
                press_gesture(component),
                modded(ModAction::ShowSummary),
            );
        }
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
    let mut pause: Vec<Control> = Vec::new();
    for (hand, component) in bound(GlobalAction::Pause.id()) {
        let control = press_control(component, hand);
        if !pause.contains(&control) {
            pause.push(control);
        }
    }
    for control in pause {
        add(
            control,
            Gesture::Press,
            Effect::Actions(vec![Action::GUI_PAUSE]),
        );
        add(
            control,
            Gesture::Hold,
            Effect::Actions(vec![Action::GUI_PDA]),
        );
    }
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
