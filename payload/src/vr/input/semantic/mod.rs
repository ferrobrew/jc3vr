//! The semantic actions: OpenXR actions named for what they do in the game (`jump`, `reload`,
//! `accelerate`) rather than for the physical input they read, so that the runtime's binding
//! interface shows the player game meanings.
//!
//! There is one action set per control context plus `global`. `global` is always active; of the
//! others exactly one is, chosen from the mode ([`ActionSetKind`]): `on_foot` (on foot, swimming,
//! falling, and the parachute), `wingsuit`, `land_vehicle` (cars, motorcycles, boats, and
//! unresolved vehicles), `helicopter`, `plane`, and `menu` (the game's menus and the native map).
//! Every set has priority 0, the same as the always-active `hands` set that holds the raw inputs,
//! because a runtime ignores a lower-priority set's binding on an input a higher-priority set also
//! binds, and the raw and semantic actions share their inputs.
//!
//! Every action is created with both hands as subaction paths and read per hand, so the hand a
//! press came from stays known after the player remaps it.
//!
//! - [`vocabulary`] declares each set's actions, their types, and their localized names.
//! - [`defaults`] holds the suggested bindings on the Index and Touch profiles, written in terms
//!   of the dominant hand and the off-hand, and the left-handed table mirrored from the
//!   right-handed one.
//! - [`frame`] publishes each frame's per-hand state of the active set's and `global`'s actions.
//!
//! The binding tables read the semantic state ([`crate::controllers::bindings::intents`]); the
//! gestures, grabs, chords, and pointers read the raw inputs of the `hands` set.
//!
//! - [`bound`] reduces the runtime's bound sources to the physical inputs each action reads on
//!   each hand, which the bindings' masks act on.

pub(crate) mod bound;
pub(crate) mod defaults;
pub(crate) mod frame;
pub(crate) mod vocabulary;

#[cfg(test)]
mod tests;

use crate::{
    controllers::frame::ActionSetKind,
    vr::input::semantic::vocabulary::{
        GlobalAction, HelicopterAction, LandVehicleAction, MenuAction, OnFootAction, PlaneAction,
        WingsuitAction,
    },
};

/// The version of the action schema: the sets, their actions, and the actions' types. It is the
/// OpenXR application version, so a runtime that keys saved bindings on it can tell the schemas
/// apart. A change to the schema bumps it and adds an entry to the changelog in
/// `docs/mod/input/controllers.md` ("Action schema").
pub(crate) const SCHEMA_VERSION: u32 = 3;

/// An action's OpenXR type.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ActionKind {
    Bool,
    /// A scalar. Actions that need the mod's own press thresholds are floats, since the runtime
    /// thresholds a boolean action bound to a scalar input with its own hysteresis.
    Float,
    Vec2,
}

impl ActionKind {
    /// A short tag for logs, the debug UI, and the schema snapshot.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            ActionKind::Bool => "bool",
            ActionKind::Float => "float",
            ActionKind::Vec2 => "vec2",
        }
    }
}

/// One action's definition.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct ActionDef {
    /// The OpenXR action name, unique within its set.
    pub(crate) name: &'static str,
    pub(crate) kind: ActionKind,
    /// The name the runtime's binding interface shows.
    pub(crate) localized_name: &'static str,
}

/// A semantic action set.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SemanticSet {
    Global,
    OnFoot,
    Wingsuit,
    LandVehicle,
    Helicopter,
    Plane,
    Menu,
}

impl SemanticSet {
    /// Every set, in creation and stash order.
    pub(crate) const ALL: [SemanticSet; 7] = [
        SemanticSet::Global,
        SemanticSet::OnFoot,
        SemanticSet::Wingsuit,
        SemanticSet::LandVehicle,
        SemanticSet::Helicopter,
        SemanticSet::Plane,
        SemanticSet::Menu,
    ];

    /// The set a mode selects.
    pub(crate) const fn of(kind: ActionSetKind) -> SemanticSet {
        match kind {
            ActionSetKind::OnFoot => SemanticSet::OnFoot,
            ActionSetKind::Wingsuit => SemanticSet::Wingsuit,
            ActionSetKind::LandVehicle => SemanticSet::LandVehicle,
            ActionSetKind::Helicopter => SemanticSet::Helicopter,
            ActionSetKind::Plane => SemanticSet::Plane,
            ActionSetKind::Ui => SemanticSet::Menu,
        }
    }

    /// The index of this set in [`ALL`](Self::ALL).
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// The OpenXR action set name.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            SemanticSet::Global => "global",
            SemanticSet::OnFoot => "on_foot",
            SemanticSet::Wingsuit => "wingsuit",
            SemanticSet::LandVehicle => "land_vehicle",
            SemanticSet::Helicopter => "helicopter",
            SemanticSet::Plane => "plane",
            SemanticSet::Menu => "menu",
        }
    }

    /// The localized set name the runtime's binding interface shows.
    pub(crate) const fn localized_name(self) -> &'static str {
        match self {
            SemanticSet::Global => "Always",
            SemanticSet::OnFoot => "On foot, falling, and parachute",
            SemanticSet::Wingsuit => "Wingsuit",
            SemanticSet::LandVehicle => "Cars, bikes, and boats",
            SemanticSet::Helicopter => "Helicopters",
            SemanticSet::Plane => "Planes",
            SemanticSet::Menu => "Menus and map",
        }
    }

    /// The set's actions, in creation and stash order.
    pub(crate) const fn actions(self) -> &'static [ActionDef] {
        match self {
            SemanticSet::Global => GlobalAction::DEFS,
            SemanticSet::OnFoot => OnFootAction::DEFS,
            SemanticSet::Wingsuit => WingsuitAction::DEFS,
            SemanticSet::LandVehicle => LandVehicleAction::DEFS,
            SemanticSet::Helicopter => HelicopterAction::DEFS,
            SemanticSet::Plane => PlaneAction::DEFS,
            SemanticSet::Menu => MenuAction::DEFS,
        }
    }
}
