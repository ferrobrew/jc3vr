//! The semantic action schema, read from the payload's golden snapshot of it, so that the
//! recordings' semantic states index the actions the payload defines.
//!
//! The snapshot is a line `version: <n>`, then a line per set (`<set>: set`) and per action
//! (`<set>.<action>: <type>`), in stash order. The payload's tests hold it equal to the actions the
//! payload creates.

use std::fmt;

use tracking_format::model::{ActionState, ActionValue, SemanticSet, SetState};

/// The payload's snapshot of its semantic action schema.
pub const GOLDEN: &str = include_str!("../../../payload/src/vr/input/semantic/schema.golden");

/// The schema: its version and each semantic set's actions.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub version: u32,
    /// Indexed by [`SemanticSet::index`].
    sets: Vec<Vec<Action>>,
}

/// One action of a set.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub name: String,
    pub kind: ActionKind,
}

/// An action's value type.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActionKind {
    Bool,
    Float,
    Vec2,
}

impl Schema {
    /// Read a snapshot.
    pub fn parse(text: &str) -> Result<Schema, SchemaError> {
        let mut lines = text.lines().enumerate();
        let version = lines
            .next()
            .and_then(|(_, line)| line.strip_prefix("version: "))
            .and_then(|version| version.trim().parse().ok())
            .ok_or(SchemaError::NoVersion)?;
        let mut sets: Vec<Vec<Action>> = vec![Vec::new(); SemanticSet::ALL.len()];
        let mut current: Option<SemanticSet> = None;
        let mut current_name = String::new();
        for (index, line) in lines {
            let number = index + 1;
            let (name, tag) = line
                .split_once(": ")
                .ok_or(SchemaError::Malformed { line: number })?;
            if tag == "set" {
                current = set_named(name);
                current_name = name.to_owned();
                continue;
            }
            let (set, action) = name
                .split_once('.')
                .ok_or(SchemaError::Malformed { line: number })?;
            if set != current_name {
                return Err(SchemaError::OutsideItsSet { line: number });
            }
            // The raw `hands` set is not a semantic set, and its poses and haptics are not actions
            // a semantic state holds.
            let Some(current) = current else {
                continue;
            };
            let kind = match tag {
                "bool" => ActionKind::Bool,
                "float" => ActionKind::Float,
                "vec2" => ActionKind::Vec2,
                _ => {
                    return Err(SchemaError::UnknownType {
                        line: number,
                        tag: tag.to_owned(),
                    });
                }
            };
            sets[current.index()].push(Action {
                name: action.to_owned(),
                kind,
            });
        }
        if let Some(set) = SemanticSet::ALL
            .iter()
            .find(|set| sets[set.index()].is_empty())
        {
            return Err(SchemaError::MissingSet(*set));
        }
        Ok(Schema { version, sets })
    }

    /// The payload's schema.
    pub fn payload() -> Result<Schema, SchemaError> {
        Schema::parse(GOLDEN)
    }

    /// The actions of `set`, in index order.
    pub fn actions(&self, set: SemanticSet) -> &[Action] {
        &self.sets[set.index()]
    }

    /// The index of the action of `set` named `name`, and its type.
    pub fn find(&self, set: SemanticSet, name: &str) -> Result<(usize, ActionKind), SchemaError> {
        self.actions(set)
            .iter()
            .position(|action| action.name == name)
            .map(|index| (index, self.actions(set)[index].kind))
            .ok_or_else(|| SchemaError::NoSuchAction {
                set,
                name: name.to_owned(),
            })
    }

    /// Every action of `set` at rest on both hands.
    pub fn rest(&self, set: SemanticSet) -> SetState {
        SetState {
            set,
            actions: self
                .actions(set)
                .iter()
                .map(|action| [rest(action.kind); 2])
                .collect(),
        }
    }
}

/// Why the schema snapshot could not be read or used.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaError {
    /// The first line does not name the version.
    NoVersion,
    /// A line is neither a set nor an action.
    Malformed { line: usize },
    /// An action is listed under another set's line.
    OutsideItsSet { line: usize },
    /// An action's type is not one a semantic action has.
    UnknownType { line: usize, tag: String },
    /// A semantic set has no actions.
    MissingSet(SemanticSet),
    /// The schema has no action of that name in that set.
    NoSuchAction { set: SemanticSet, name: String },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "semantic schema: ")?;
        match self {
            SchemaError::NoVersion => write!(f, "the first line does not name the version"),
            SchemaError::Malformed { line } => {
                write!(f, "line {line} is neither a set nor an action")
            }
            SchemaError::OutsideItsSet { line } => {
                write!(f, "line {line} lists an action under another set")
            }
            SchemaError::UnknownType { line, tag } => {
                write!(f, "line {line} has the unknown action type `{tag}`")
            }
            SchemaError::MissingSet(set) => write!(f, "the {set:?} set has no actions"),
            SchemaError::NoSuchAction { set, name } => {
                write!(f, "the {set:?} set has no action named `{name}`")
            }
        }
    }
}

impl std::error::Error for SchemaError {}

/// The rest state of an action of `kind`.
pub fn rest(kind: ActionKind) -> ActionState {
    ActionState {
        value: match kind {
            ActionKind::Bool => ActionValue::Bool(false),
            ActionKind::Float => ActionValue::Float(0.0),
            ActionKind::Vec2 => ActionValue::Vec2(glam::Vec2::ZERO),
        },
        active: false,
        down: false,
    }
}

/// The semantic set of an OpenXR action set name, or `None` for the raw `hands` set.
fn set_named(name: &str) -> Option<SemanticSet> {
    Some(match name {
        "global" => SemanticSet::Global,
        "on_foot" => SemanticSet::OnFoot,
        "wingsuit" => SemanticSet::Wingsuit,
        "land_vehicle" => SemanticSet::LandVehicle,
        "helicopter" => SemanticSet::Helicopter,
        "plane" => SemanticSet::Plane,
        "menu" => SemanticSet::Menu,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_payload_snapshot_reads() {
        let schema = Schema::payload().expect("the snapshot reads");
        assert!(schema.version > 0);
        assert_eq!(
            schema.find(SemanticSet::OnFoot, "move"),
            Ok((0, ActionKind::Vec2))
        );
        assert_eq!(
            schema
                .find(SemanticSet::OnFoot, "turn")
                .map(|(_, kind)| kind),
            Ok(ActionKind::Float)
        );
        let global = schema.rest(SemanticSet::Global);
        assert_eq!(
            global.actions.len(),
            schema.actions(SemanticSet::Global).len()
        );
    }

    #[test]
    fn a_snapshot_is_read_set_by_set() {
        let mut text = String::from("version: 7\nhands: set\nhands.grip_pose: pose\n");
        for set in [
            "global",
            "on_foot",
            "wingsuit",
            "land_vehicle",
            "helicopter",
            "plane",
            "menu",
        ] {
            text.push_str(&format!("{set}: set\n{set}.a: bool\n{set}.b: vec2\n"));
        }
        let schema = Schema::parse(&text).expect("the snapshot reads");
        assert_eq!(schema.version, 7);
        assert_eq!(
            schema.actions(SemanticSet::Menu),
            &[
                Action {
                    name: "a".to_owned(),
                    kind: ActionKind::Bool
                },
                Action {
                    name: "b".to_owned(),
                    kind: ActionKind::Vec2
                },
            ]
        );
        assert_eq!(
            schema.find(SemanticSet::Plane, "c"),
            Err(SchemaError::NoSuchAction {
                set: SemanticSet::Plane,
                name: "c".to_owned()
            })
        );
    }

    #[test]
    fn a_broken_snapshot_is_refused_with_its_line() {
        assert_eq!(Schema::parse("hands: set\n"), Err(SchemaError::NoVersion));
        assert_eq!(
            Schema::parse("version: 1\nglobal: set\nglobal.a: pose\n"),
            Err(SchemaError::UnknownType {
                line: 3,
                tag: "pose".to_owned()
            })
        );
        assert_eq!(
            Schema::parse("version: 1\nglobal: set\non_foot.a: bool\n"),
            Err(SchemaError::OutsideItsSet { line: 3 })
        );
        assert_eq!(
            Schema::parse("version: 1\nglobal: set\nglobal.a: bool\n"),
            Err(SchemaError::MissingSet(SemanticSet::OnFoot))
        );
    }
}
