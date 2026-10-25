//! Current wrist hints, projected from resolved sources and explicit gameplay eligibility.

use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::{
        bindings::tables::describe::{Control, Effect, Gesture, ModAction, Row},
        frame::Hand,
        mode::Mode,
        summary::{
            content::{Line, build},
            context::ContextHint,
            labels::{
                ActionLabel, CurrentAction, HOLD_KEY, Text, action_label, gesture_prefix, heading,
            },
        },
    },
    vr::input::semantic::bound::SourceName,
};

/// The current consumer of a physical trigger, including neutral-transfer suppression.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) enum TriggerUse {
    #[default]
    Free,
    Reserved,
    Gun,
    NativePair,
}

/// The final effective routing supplied by the input owner after configuration, recovery,
/// neutral transfer, and shooting permission. Trigger reservation does not disable the whole hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct CurrentRouting {
    pub ready: [bool; 2],
    pub triggers: [TriggerUse; 2],
}

impl CurrentRouting {
    /// Source-only rows retain their physical hand. Real two-hand chords require both participants.
    pub(crate) fn allows(self, control: Control, gesture: Gesture) -> bool {
        if gesture == Gesture::WithBothGrips
            || matches!(
                control,
                Control::BothStickClicks | Control::LeftStickClickAndRightB
            )
        {
            return self.ready.into_iter().all(|ready| ready);
        }
        control.hand().map_or_else(
            || match control {
                Control::EitherGrip
                | Control::EitherA
                | Control::EitherB
                | Control::EitherStick(_)
                | Control::EitherTrackpad
                | Control::EitherMotion
                | Control::System => self.ready.contains(&true),
                Control::Unbound => false,
                _ => true,
            },
            |hand| self.ready[hand.index()],
        )
    }
}

/// The input owner's current eligibility. An absent route is unavailable, not a default binding.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CurrentContext {
    /// Only the usable action subset of each resolved route, plus actual physical mechanic rows.
    pub usable: Vec<Row>,
    pub routing: CurrentRouting,
    pub native_reload: bool,
    pub held_holster: bool,
}

/// Fixed spatial cells. Hand dominance never changes a cell's column.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CellKind {
    Left,
    Right,
    Available,
    ViewMenu,
}

impl CellKind {
    pub(crate) const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Available, Self::ViewMenu];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Available => 2,
            Self::ViewMenu => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct CurrentSummary {
    pub heading: String,
    pub cells: [Vec<Hint>; 4],
}

/// One action, with all usable sources in this spatial cell retained once.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hint {
    pub sources: Vec<Source>,
    pub prefix: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Eq)]
pub(crate) struct Source {
    pub control: Control,
    pub name: Option<String>,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        // Runtime name indices belong to one source table; only the hand and resolved name are drawn.
        let same_control = match (self.control, other.control) {
            (Control::Named(hand, _), Control::Named(other_hand, _)) => hand == other_hand,
            (control, other_control) => control == other_control,
        };
        same_control && self.name == other.name
    }
}

/// Project only known-source routes that the current snapshot permits. Physical gestures can
/// come directly from the snapshot; they never acquire a fabricated semantic button binding.
pub(crate) fn project(
    rows: &[Row],
    mode: Mode,
    current: &CurrentContext,
    hints: &[ContextHint],
    resolve: &dyn Fn(Text) -> String,
    name: &dyn Fn(SourceName) -> String,
    description: &dyn Fn(u32) -> Option<String>,
) -> CurrentSummary {
    let mut eligible = Vec::new();
    for row in rows {
        if row.control == Control::Unbound || !current.routing.allows(row.control, row.gesture) {
            continue;
        }
        let mut row = row.clone();
        match &mut row.effect {
            Effect::Actions(actions) => {
                actions.retain(|action| current.usable.iter().any(|usable| {
                    usable.control == row.control && usable.gesture == row.gesture
                        && matches!(&usable.effect, Effect::Actions(allowed) if allowed.contains(action))
                }));
                if actions.is_empty() {
                    continue;
                }
            }
            Effect::Mod(action) => {
                if !current.usable.iter().any(|usable| {
                    usable.control == row.control
                        && usable.gesture == row.gesture
                        && usable.effect == Effect::Mod(*action)
                }) {
                    continue;
                }
            }
        }
        if retain_trigger_route(&mut row, current.routing) {
            eligible.push(row);
        }
    }
    for row in &current.usable {
        let mut row = row.clone();
        if matches!(row.effect, Effect::Mod(_))
            && current.routing.allows(row.control, row.gesture)
            && retain_trigger_route(&mut row, current.routing)
            && !eligible.contains(&row)
        {
            eligible.push(row);
        }
    }
    for hand in Hand::BOTH {
        if current.routing.ready[hand.index()]
            && current.routing.triggers[hand.index()] == TriggerUse::Gun
        {
            eligible.push(Row {
                control: Control::Trigger(hand),
                gesture: Gesture::Press,
                effect: Effect::Actions(vec![Action::FIRE_RIGHT]),
            });
        }
    }
    let mut labelled = Vec::new();
    for mut row in eligible {
        let reload = if let Effect::Actions(actions) = &mut row.effect {
            let reload = actions.iter().any(|action| {
                matches!(action, Action::RELOAD | Action::MC_RELOAD)
                    && matches!(action_label(*action, mode), Some(ActionLabel::Shown { .. }))
            });
            actions.retain(|action| !matches!(action, Action::RELOAD | Action::MC_RELOAD));
            reload
        } else {
            false
        };
        if reload {
            for (permitted, action) in [
                (current.native_reload, CurrentAction::NativeReload),
                (current.held_holster, CurrentAction::HeldHolster),
            ] {
                if permitted {
                    labelled.push(CurrentRow::Reload(row.control, action));
                }
            }
        }
        if !matches!(&row.effect, Effect::Actions(actions) if actions.is_empty()) {
            labelled.push(CurrentRow::Route(row));
        }
    }
    labelled.sort_by_key(|row| row.control().order());
    let hold = resolve(Text::Game(HOLD_KEY));
    let mut summary = CurrentSummary {
        heading: resolve(heading(mode)),
        ..Default::default()
    };
    for row in labelled {
        let mut line = match &row {
            CurrentRow::Reload(control, action) => Line {
                control: *control,
                source: match control {
                    Control::Named(_, source) => Some(name(*source)),
                    _ => None,
                },
                prefix: gesture_prefix(action.gesture()).map(resolve),
                text: resolve(action.text()),
            },
            CurrentRow::Route(row) => {
                let labelled = build(std::slice::from_ref(row), mode, Hand::Right, resolve, name);
                let Some(line) = labelled
                    .groups
                    .into_iter()
                    .flat_map(|group| group.lines)
                    .next()
                else {
                    continue;
                };
                line
            }
        };
        if let CurrentRow::Route(row) = &row
            && let Some(hint) = hints.iter().find(|hint| matches_hint(row, hint.action))
            && let Some(text) = description(hint.description)
                .filter(|text| !text.contains('{'))
                .or_else(|| fallback(hint.action, mode, resolve))
        {
            line.text = text;
            line.prefix = if hint.hold {
                Some(hold.clone())
            } else {
                gesture_prefix(row.gesture).map(resolve)
            };
        }
        normalize_hold(&mut line, &hold);
        let source = Source {
            control: line.control,
            name: line.source,
        };
        let cell = &mut summary.cells[cell_for(row.control()).index()];
        if let Some(existing) = cell.iter_mut().find(|hint| {
            hint.prefix.as_ref().map(|p| p.to_lowercase())
                == line.prefix.as_ref().map(|p| p.to_lowercase())
                && hint.text.to_lowercase() == line.text.to_lowercase()
        }) {
            if !existing.sources.contains(&source) {
                existing.sources.push(source);
            }
        } else {
            cell.push(Hint {
                sources: vec![source],
                prefix: line.prefix,
                text: line.text,
            });
        }
    }
    summary
}

enum CurrentRow {
    Route(Row),
    Reload(Control, CurrentAction),
}

impl CurrentRow {
    fn control(&self) -> Control {
        match self {
            Self::Route(row) => row.control,
            Self::Reload(control, _) => *control,
        }
    }
}

/// Match the live router's trigger/unknown-source mask. Native-pair permission is already
/// supplied by that router; only its retained fire targets survive on these sources.
fn retain_trigger_route(row: &mut Row, routing: CurrentRouting) -> bool {
    let hand = match row.control {
        Control::Trigger(hand) | Control::Named(Some(hand), _) => hand,
        _ => return true,
    };
    match routing.triggers[hand.index()] {
        TriggerUse::Free => true,
        TriggerUse::NativePair => {
            let Effect::Actions(actions) = &mut row.effect else {
                return false;
            };
            actions.retain(|action| matches!(action, Action::FIRE_RIGHT | Action::R2));
            !actions.is_empty()
        }
        TriggerUse::Reserved | TriggerUse::Gun => false,
    }
}

fn cell_for(control: Control) -> CellKind {
    match control.hand() {
        Some(Hand::Left) => CellKind::Left,
        Some(Hand::Right) => CellKind::Right,
        None => match control {
            Control::System | Control::BothStickClicks | Control::LeftStickClickAndRightB => {
                CellKind::ViewMenu
            }
            _ => CellKind::Available,
        },
    }
}

fn matches_hint(row: &Row, action: Action) -> bool {
    match &row.effect {
        Effect::Actions(actions) => actions.contains(&action),
        Effect::Mod(ModAction::ReleaseTethers) => matches!(
            action,
            Action::RELEASE_GRAPPLE | Action::VEHICLE_RELEASE_GRAPPLE | Action::CANCEL
        ),
        Effect::Mod(_) => false,
    }
}

fn fallback(action: Action, mode: Mode, resolve: &dyn Fn(Text) -> String) -> Option<String> {
    match action_label(action, mode)? {
        ActionLabel::Shown { tap, hold } => tap.or(hold).map(resolve),
        ActionLabel::Hidden => None,
    }
}

fn normalize_hold(line: &mut Line, hold: &str) {
    if !line
        .prefix
        .as_ref()
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(hold))
    {
        return;
    }
    while let Some((prefix, rest)) = line.text.split_once(char::is_whitespace) {
        if !prefix.eq_ignore_ascii_case(hold) {
            break;
        }
        line.text = rest.trim_start().to_owned();
    }
}

#[cfg(test)]
#[path = "current_tests.rs"]
mod tests;
