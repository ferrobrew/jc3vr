//! The semantic action vocabulary: the action sets, each set's actions, their OpenXR types, and
//! their localized names. The OpenXR side (the action and set handles, the suggested bindings, the
//! schema version, and the per-frame state) lives in `vr::input::semantic` in the payload.

pub mod vocabulary;

#[cfg(test)]
mod tests;

use crate::{
    frame::ActionSetKind,
    semantic::vocabulary::{
        GlobalAction, HelicopterAction, LandVehicleAction, MenuAction, OnFootAction, PlaneAction,
        WingsuitAction,
    },
};

/// An action's OpenXR type.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ActionKind {
    Bool,
    /// A scalar. Actions that need the mod's own press thresholds are floats, since the runtime
    /// thresholds a boolean action bound to a scalar input with its own hysteresis.
    Float,
    Vec2,
}

impl ActionKind {
    /// A short tag for logs, the debug UI, and the schema snapshot.
    pub const fn label(self) -> &'static str {
        match self {
            ActionKind::Bool => "bool",
            ActionKind::Float => "float",
            ActionKind::Vec2 => "vec2",
        }
    }
}

/// One action's definition.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ActionDef {
    /// The OpenXR action name, unique within its set.
    pub name: &'static str,
    pub kind: ActionKind,
    /// The name the runtime's binding interface shows.
    pub localized_name: &'static str,
}

/// A semantic action set.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum SemanticSet {
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
    pub const ALL: [SemanticSet; 7] = [
        SemanticSet::Global,
        SemanticSet::OnFoot,
        SemanticSet::Wingsuit,
        SemanticSet::LandVehicle,
        SemanticSet::Helicopter,
        SemanticSet::Plane,
        SemanticSet::Menu,
    ];

    /// The set a mode selects.
    pub const fn of(kind: ActionSetKind) -> SemanticSet {
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
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The OpenXR action set name.
    pub const fn name(self) -> &'static str {
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
    pub const fn localized_name(self) -> &'static str {
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
    pub const fn actions(self) -> &'static [ActionDef] {
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
