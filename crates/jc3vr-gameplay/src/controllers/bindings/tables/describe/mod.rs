//! The describing reader: what each controller input does in a mode, read from the same table
//! declarations the injection evaluates ([`crate::controllers::bindings::tables::declare`]), plus
//! the inputs the mod handles outside the tables (the pause button, the body turn, the crouch
//! flick, the sight chord, the recenter and debug panel chords, the weapon gestures, and the
//! parachute handles). Pure; the controls summary ([`crate::controllers::summary`]) labels and draws
//! the rows.
//!
//! The tables name semantic actions; each is described on the physical inputs the runtime reports
//! it bound to ([`jc3vr_render::vr::input::semantic::bound::SourceTable`]), or, for an action whose
//! sources are not known, on those its default binding
//! ([`jc3vr_render::vr::input::semantic::defaults`]) puts it on, less the inputs the table context masks
//! ([`crate::controllers::bindings::tables::context_masks`]). A source the mod does not recognise
//! is described by the runtime's name for it ([`Control::Named`]), and an action bound to nothing
//! as [`Control::Unbound`].

use jc3gi::input::input_action_map::Action;
use jc3vr_input::{
    frame::Hand,
    semantic::vocabulary::{ActionId, GlobalAction, MenuAction, OnFootAction},
};
use jc3vr_render::vr::input::semantic::{
    bound::{Layout, PhysicalSet, SourceName, SourceTable},
    defaults::{Component, default_binding},
};

pub use crate::controllers::intent::rows::{Control, Effect, Gesture, ModAction, Row, StickDir};
use crate::controllers::{
    bindings::{
        intents::{Mask, YANK_ACTIONS},
        tables::{
            body_turn_action, context_masks,
            declare::{TableCtx, declare, vehicle_weapon_actions},
            grapple_press,
            sink::{Axis, Direction, Gate, Input, PadLayout, PadZone, Press, Sink, pad_of},
        },
    },
    mode::{AirborneVariant, Mode, OnFootVariant, VehicleClass},
    vehicles::wingsuit_gestures::GestureAvailability,
};

/// Physical rows supplied only by the flight recognizer's current availability.
pub(crate) fn flight_mechanics(availability: GestureAvailability) -> Vec<Row> {
    let actions: &[ModAction] = match availability {
        GestureAvailability::Unavailable => &[],
        GestureAvailability::Deploy => &[ModAction::GestureDeploy],
        GestureAvailability::CloseAndBrake => &[ModAction::GestureClose, ModAction::GestureBrake],
    };
    actions
        .iter()
        .map(|&action| Row {
            control: Control::EitherMotion,
            gesture: Gesture::Press,
            effect: Effect::Mod(action),
        })
        .collect()
}

/// The mod's own controller features that add inputs outside the tables, as configured.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct Features {
    /// The yank gesture.
    pub yank: bool,
    pub holsters: bool,
    pub foregrip: bool,
    /// The true reload's pouch and well.
    pub magazine: bool,
    /// The true reload's belt reload.
    pub belt_reload: bool,
    /// The true reload's flick eject.
    pub flick_eject: bool,
    /// The true reload's chamber, worked by hand.
    pub work_action: bool,
    /// The physical grenade throw.
    pub grenade_throw: bool,
    /// The C4 throw from the chest pouch.
    pub c4_throw: bool,
    /// The sight tuning capture chord is armed.
    pub sight_chord: bool,
    /// The weapon-select trackpad press opens the weapon wheel.
    pub wheel_trackpad: bool,
    /// The `weapon_wheel` action opens the weapon wheel.
    pub wheel_stick: bool,
    /// The parachute's handles.
    pub parachute_handles: bool,
    /// The ripcord at the hip.
    pub ripcord: bool,
    /// The wingsuit's arm steering.
    pub arm_steering: bool,
    /// The physical vehicle controls offered to a driver, by kind.
    pub physical: PhysicalFeatures,
    /// The developer chords are withheld in the vehicle sets
    /// (`controllers.bindings.suppress_developer_chords_in_vehicles`).
    pub suppress_developer_chords_in_vehicles: bool,
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

/// Every input of `ctx.mode` and what it does, on the default bindings, with the rows that share
/// an input and a gesture merged, in declaration order, and a face button, stick, or trackpad row
/// that both hands share folded into one. The summary describes the runtime's sources instead
/// ([`describe_bound`]).
#[cfg(test)]
pub fn describe(ctx: &DescribeCtx) -> Vec<Row> {
    describe_with(
        ctx,
        Bindings {
            layout: default_binding,
            runtime: None,
            routes: None,
        },
    )
}

/// [`describe`], on the sources the runtime reported in `sources`, with the default bindings
/// standing in for the actions whose sources are not known.
pub(crate) fn describe_bound(ctx: &DescribeCtx, sources: &SourceTable) -> Vec<Row> {
    describe_with(
        ctx,
        Bindings {
            layout: default_binding,
            runtime: Some(sources),
            routes: None,
        },
    )
}

/// Runtime sources without default fallbacks or cross-hand folding. Each physical source remains
/// identifiable until the wrist applies per-hand eligibility.
#[cfg(test)]
pub(crate) fn describe_sources(ctx: &DescribeCtx, sources: &SourceTable) -> Vec<Row> {
    describe_current(ctx, sources, None)
}

/// Current semantic routes are filtered while action identity is still available. Physical
/// mechanics remain subject to their owning subsystem's separate eligibility.
pub(crate) fn describe_current(
    ctx: &DescribeCtx,
    sources: &SourceTable,
    routes: Option<&[(ActionId, Hand)]>,
) -> Vec<Row> {
    describe_unfolded(
        ctx,
        Bindings {
            layout: |_, _| None,
            runtime: Some(sources),
            routes,
        },
    )
}

/// Where the description finds the inputs each action is bound to.
#[derive(Debug, Copy, Clone)]
pub(crate) struct Bindings<'a> {
    /// The default bindings, for the actions whose sources are not known.
    pub(crate) layout: Layout,
    /// The sources the runtime reported.
    pub(crate) runtime: Option<&'a SourceTable>,
    pub(crate) routes: Option<&'a [(ActionId, Hand)]>,
}

/// [`describe`], with the actions on the inputs `bindings` puts them on.
pub(crate) fn describe_with(ctx: &DescribeCtx, bindings: Bindings) -> Vec<Row> {
    fold_hands(describe_unfolded(ctx, bindings))
}

fn describe_unfolded(ctx: &DescribeCtx, bindings: Bindings) -> Vec<Row> {
    let mut sink = DescribeSink {
        dominant: ctx.dominant,
        yank: ctx.features.yank,
        bindings,
        masks: if bindings.routes.is_some() {
            Vec::new()
        } else {
            context_masks(ctx.mode, &ctx.table, ctx.dominant)
        },
        rows: Vec::new(),
    };
    declare(&mut sink, ctx.mode, &ctx.table);
    let mut rows = sink.rows;
    rows.extend(outside_rows(ctx, bindings));
    merge(rows)
}

/// The describing [`Sink`]: records a row per input instead of resolving it.
pub struct DescribeSink<'a> {
    dominant: Hand,
    /// Whether the yank gesture is on; off, its rows are left out.
    yank: bool,
    bindings: Bindings<'a>,
    masks: Vec<Mask>,
    pub rows: Vec<Row>,
}

impl DescribeSink<'_> {
    /// The inputs `action` is bound to, less the masked, or `None` when it is bound to nothing.
    fn bound(&self, action: ActionId) -> Option<Vec<Bound>> {
        let inputs = bound_inputs(self.bindings, action, self.dominant);
        if inputs.is_empty() {
            return None;
        }
        Some(
            inputs
                .into_iter()
                .filter(|input| {
                    let (hand, physical) = match *input {
                        Bound::Component(hand, component) => {
                            (hand, PhysicalSet::of(component.physical()))
                        }
                        // A source under no hand is masked by nothing; one the mod does not
                        // recognise only with its whole hand.
                        Bound::Named(None, _) => return true,
                        Bound::Named(Some(hand), _) => (hand, PhysicalSet::EMPTY.with_unknown()),
                    };
                    !self
                        .masks
                        .iter()
                        .any(|m| m.hand == hand && m.covers(action, physical))
                })
                .collect(),
        )
    }

    fn press_rows(&self, press: Press, actions: &[Action]) -> Vec<Row> {
        // A press read only on a hand holding a gun fires that gun, which the gun's own row
        // describes ([`ModAction::FireHandGun`]).
        if press.gate == Gate::HandArmed {
            return Vec::new();
        }
        let long = matches!(press.input, Input::Long(_) | Input::LongHeld(_));
        let gate = |gesture| match press.gate {
            Gate::Gripped => Gesture::WithGrip,
            _ if long => Gesture::Hold,
            Gate::Always
            | Gate::UnlessChordArmed
            | Gate::UnlessChordGrip
            | Gate::Ungripped
            | Gate::Ragdolling
            | Gate::HandArmed
            | Gate::HandUnarmed => gesture,
        };
        let row = |control, gesture| Row {
            control,
            gesture: gate(gesture),
            effect: Effect::Actions(actions.to_vec()),
        };
        let mut rows = Vec::new();
        let unbound = Row {
            control: Control::Unbound,
            gesture: Gesture::Press,
            effect: Effect::Actions(actions.to_vec()),
        };
        match press.input {
            Input::Action(action)
            | Input::Short(action)
            | Input::Long(action)
            | Input::LongHeld(action) => {
                match self.bound(action) {
                    Some(inputs) => {
                        for input in inputs {
                            rows.push(row(input.press_control(), input.press_gesture()));
                        }
                    }
                    None => rows.push(unbound),
                }
                if self.yank && YANK_ACTIONS.contains(&action) {
                    rows.push(row(Control::Motion(self.dominant.other()), Gesture::Yank));
                }
            }
            Input::Zone(action, zone) => {
                let layout = pad_of(action).map_or(PadLayout::Halves, |(_, layout)| layout);
                let Some(inputs) = self.bound(action) else {
                    return vec![unbound];
                };
                for input in inputs {
                    let control = match input {
                        Bound::Component(hand, _) => Control::Trackpad(hand),
                        Bound::Named(hand, name) => Control::Named(hand, name),
                    };
                    rows.push(row(control, zone_gesture(layout, zone)));
                }
            }
            Input::MenuStick(direction) => {
                let dir = match direction {
                    Direction::Up | Direction::Down => StickDir::Vertical,
                    Direction::Left | Direction::Right => StickDir::Horizontal,
                };
                let Some(inputs) = self.bound(MenuAction::MenuNavigate.id()) else {
                    return vec![unbound];
                };
                for input in inputs {
                    let control = match input {
                        Bound::Component(hand, _) => Control::Stick(hand, dir),
                        Bound::Named(hand, name) => Control::Named(hand, name),
                    };
                    rows.push(row(control, Gesture::Press));
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
        let Some(inputs) = self.bound(axis.action()) else {
            return vec![Control::Unbound];
        };
        inputs
            .into_iter()
            .map(|input| match (axis, input) {
                (_, Bound::Named(hand, name)) => Control::Named(hand, name),
                (Axis::Value(_), Bound::Component(hand, component)) => {
                    press_control(component, hand)
                }
                (_, Bound::Component(hand, Component::Trackpad)) => Control::Trackpad(hand),
                (_, Bound::Component(hand, _)) => Control::Stick(hand, dir),
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

impl Sink for DescribeSink<'_> {
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

/// One input an action is bound to.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Bound {
    /// A component the mod knows, on a hand.
    Component(Hand, Component),
    /// A source the mod does not recognise, by the runtime's name for it.
    Named(Option<Hand>, SourceName),
}

impl Bound {
    /// The control a press of the input is.
    const fn press_control(self) -> Control {
        match self {
            Bound::Component(hand, component) => press_control(component, hand),
            Bound::Named(hand, name) => Control::Named(hand, name),
        }
    }

    /// How a press of the input is worked.
    const fn press_gesture(self) -> Gesture {
        match self {
            Bound::Component(_, component) => press_gesture(component),
            Bound::Named(..) => Gesture::Press,
        }
    }
}

/// The inputs `bindings` puts `action` on under `dominant`: the sources the runtime reported where
/// they are known, otherwise the default binding's, in [`Hand::BOTH`] order. Empty for an action
/// bound to nothing.
fn bound_inputs(bindings: Bindings, action: ActionId, dominant: Hand) -> Vec<Bound> {
    if let Some(sources) = bindings
        .runtime
        .and_then(|table| table.sources(action.set, action.index()))
    {
        return sources
            .iter()
            .filter(|source| {
                bindings.routes.is_none_or(|routes| {
                    source
                        .hand
                        .is_some_and(|hand| routes.contains(&(action, hand)))
                })
            })
            .map(|source| match (source.hand, source.component) {
                (Some(hand), Some(component)) => Bound::Component(hand, component),
                (hand, _) => Bound::Named(hand, source.name),
            })
            .collect();
    }
    let Some(binding) = (bindings.layout)(action.set, action.index()) else {
        return Vec::new();
    };
    Hand::BOTH
        .into_iter()
        .filter(|&hand| binding.side.covers(hand, dominant))
        .map(|hand| Bound::Component(hand, binding.component))
        .collect()
}

/// The control a press of `component` on `hand` is.
pub(crate) const fn press_control(component: Component, hand: Hand) -> Control {
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
fn outside_rows(ctx: &DescribeCtx, bindings: Bindings) -> Vec<Row> {
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

    // The controls an action is on, or the unbound row's for an action bound to nothing.
    let controls = |action: ActionId| -> Vec<(Control, Gesture)> {
        let inputs = bound_inputs(bindings, action, dom);
        if inputs.is_empty() {
            return vec![(Control::Unbound, Gesture::Press)];
        }
        inputs
            .into_iter()
            .map(|input| (input.press_control(), input.press_gesture()))
            .collect()
    };
    if let Some(turn) = body_turn_action(ctx.mode) {
        for (control, _) in controls(turn) {
            add(control, Gesture::Press, modded(ModAction::Turn));
        }
    }
    // The grapple button's gestures (`crate::controllers::bindings::grapple_button`).
    if let Some((grapple, _)) = grapple_press(ctx.mode) {
        for &action in grapple {
            for (control, _) in controls(action)
                .into_iter()
                .filter(|(c, _)| *c != Control::Unbound)
            {
                add(
                    control,
                    Gesture::DoubleTap,
                    modded(ModAction::ReleaseTethers),
                );
                add(control, Gesture::Hold, modded(ModAction::TightenTethers));
            }
        }
    }
    // A gun in a hand takes over that hand's vehicle weapon action.
    if let Mode::Vehicle(seat) = ctx.mode {
        for action in vehicle_weapon_actions(seat.class) {
            for (control, gesture) in controls(action)
                .into_iter()
                .filter(|(c, _)| *c != Control::Unbound)
            {
                add(control, gesture, modded(ModAction::FireHandGun));
            }
        }
    }
    let on_foot = matches!(ctx.mode, Mode::OnFoot(_));
    if on_foot {
        for (control, _) in controls(OnFootAction::CrouchFlick.id()) {
            let (control, gesture) = match control {
                Control::Stick(hand, _) => (Control::Stick(hand, StickDir::Down), Gesture::Flick),
                Control::Unbound => (Control::Unbound, Gesture::Press),
                control => (control, Gesture::Flick),
            };
            add(control, gesture, modded(ModAction::SeatedCrouch));
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
            add(
                Control::Motion(off),
                Gesture::Press,
                modded(ModAction::InsertRound),
            );
        }
        if features.work_action {
            add(
                Control::Grip(off),
                Gesture::Press,
                modded(ModAction::WorkAction),
            );
        }
        if features.belt_reload {
            add(
                Control::Motion(dom),
                Gesture::Press,
                modded(ModAction::BeltReload),
            );
        }
        if features.flick_eject {
            add(
                Control::Motion(dom),
                Gesture::Flick,
                modded(ModAction::FlickEject),
            );
        }
        if features.grenade_throw {
            add(
                Control::Grip(dom),
                Gesture::Press,
                modded(ModAction::Grenade),
            );
            add(
                Control::Trigger(dom),
                Gesture::Press,
                modded(ModAction::PullPin),
            );
        }
        if features.c4_throw {
            add(Control::EitherGrip, Gesture::Press, modded(ModAction::C4));
        }
    }
    // The weapon wheel acts where the weapon handling does: not while swimming.
    let wheel = matches!(
        ctx.mode,
        Mode::OnFoot(OnFootVariant::Ground)
            | Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Parachute)
    );
    if wheel && features.wheel_trackpad {
        for (control, _) in controls(OnFootAction::WeaponSelectPress.id()) {
            add(control, Gesture::Hold, modded(ModAction::WeaponWheel));
        }
    }
    if wheel && features.wheel_stick {
        for (control, _) in controls(OnFootAction::WeaponWheel.id()) {
            let gesture = match control {
                Control::Stick(..) | Control::EitherStick(_) => Gesture::HoldUp,
                Control::Unbound => Gesture::Press,
                _ => Gesture::Hold,
            };
            add(control, gesture, modded(ModAction::WeaponWheel));
        }
    }
    // The map reads its zoom itself (`controllers::map`), in the menu set.
    if ctx.mode == Mode::Ui {
        for (control, _) in controls(MenuAction::MapZoom.id()) {
            add(control, Gesture::Press, modded(ModAction::MapZoom));
        }
    }
    if features.parachute_handles && ctx.mode == Mode::Airborne(AirborneVariant::Parachute) {
        add(
            Control::EitherGrip,
            Gesture::Press,
            modded(ModAction::ParachuteHandles),
        );
    }
    if features.ripcord
        && matches!(
            ctx.mode,
            Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Wingsuit)
        )
    {
        add(
            Control::Grip(dom),
            Gesture::Press,
            modded(ModAction::Ripcord),
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
    for (control, gesture) in controls(GlobalAction::ShowControlsSummary.id()) {
        add(control, gesture, modded(ModAction::ShowSummary));
    }
    add(
        Control::BothStickClicks,
        Gesture::Hold,
        modded(ModAction::Recenter),
    );
    if !(features.suppress_developer_chords_in_vehicles && ctx.mode.action_set().is_vehicle()) {
        add(
            Control::LeftStickClickAndRightB,
            Gesture::Press,
            modded(ModAction::DebugPanel),
        );
    }
    if ctx.mode != Mode::Ui {
        let mut skip: Vec<Control> = Vec::new();
        for (control, _) in controls(GlobalAction::SkipCutscene.id()) {
            if !skip.contains(&control) {
                skip.push(control);
            }
        }
        for control in skip {
            add(
                control,
                Gesture::Hold,
                Effect::Actions(vec![Action::SKIP_CUTSCENE]),
            );
        }
    }
    for (control, gesture) in controls(GlobalAction::OpenCommlink.id()) {
        if control != Control::Unbound {
            add(control, gesture, Effect::Actions(vec![Action::GUI_PDA]));
        }
    }
    let mut pause: Vec<Control> = Vec::new();
    for (control, _) in controls(GlobalAction::Pause.id()) {
        if !pause.contains(&control) {
            pause.push(control);
        }
    }
    for control in pause {
        add(
            control,
            Gesture::Press,
            Effect::Actions(vec![Action::GUI_PDA]),
        );
        // An unbound system button has no long press either; its one row says so.
        if control != Control::Unbound {
            add(
                control,
                Gesture::Hold,
                Effect::Actions(vec![Action::GUI_PAUSE]),
            );
        }
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
