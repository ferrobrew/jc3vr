//! The summary's content and layout model: the described rows labelled, grouped by hand, and
//! sized to the panel. Pure.

use crate::controllers::{
    bindings::tables::describe::{Control, Effect, Row},
    frame::Hand,
    mode::Mode,
    summary::labels::{
        ActionLabel, HOLD_KEY, Text, action_label, gesture_prefix, heading, mod_action,
    },
};

/// The labelled summary of one mode.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    pub heading: String,
    pub groups: Vec<Group>,
}

/// Which hand a group's rows are on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Side {
    Hand(Hand),
    /// Either hand, or no hand in particular (the system button).
    Either,
}

/// The rows of one side.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub side: Side,
    pub lines: Vec<Line>,
}

/// One input and what it does, labelled.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub control: Control,
    /// The gesture's prefix ("HOLD", "FLICK", ...).
    pub prefix: Option<String>,
    pub text: String,
}

/// Label `rows` for `mode` and group them: the off-hand first (the panel is on its wrist), then
/// the dominant hand, then the inputs of either hand. Within a group, the rows are ordered by
/// control ([`Control::order`]). A row whose actions all go unlabelled in this mode is left out.
pub fn build(
    rows: &[Row],
    mode: Mode,
    dominant: Hand,
    resolve: &dyn Fn(Text) -> String,
) -> Summary {
    let hold = resolve(Text::Game(HOLD_KEY));
    let mut groups: Vec<Group> = [
        Side::Hand(dominant.other()),
        Side::Hand(dominant),
        Side::Either,
    ]
    .into_iter()
    .map(|side| Group {
        side,
        lines: Vec::new(),
    })
    .collect();
    let mut placed: Vec<(u8, usize, Line)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let Some(text) = row_text(row, mode, &hold, resolve) else {
            continue;
        };
        placed.push((
            row.control.order(),
            index,
            Line {
                control: row.control,
                prefix: gesture_prefix(row.gesture).map(resolve),
                text,
            },
        ));
    }
    placed.sort_by_key(|(order, index, _)| (*order, *index));
    for (_, _, line) in placed {
        let side = line.control.hand().map_or(Side::Either, Side::Hand);
        if let Some(group) = groups.iter_mut().find(|g| g.side == side) {
            group.lines.push(line);
        }
    }
    groups.retain(|g| !g.lines.is_empty());
    Summary {
        heading: resolve(heading(mode)),
        groups,
    }
}

/// A row's label: its actions' labels in order, each once (ignoring case, since the game spells
/// some the same in different cases), joined; `None` when none is shown.
fn row_text(row: &Row, mode: Mode, hold: &str, resolve: &dyn Fn(Text) -> String) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut add = |part: String| {
        if !parts
            .iter()
            .any(|p| p.to_lowercase() == part.to_lowercase())
        {
            parts.push(part);
        }
    };
    match &row.effect {
        Effect::Mod(action) => add(resolve(mod_action(*action))),
        Effect::Actions(actions) => {
            for &action in actions {
                if let Some(ActionLabel::Shown { tap, hold: held }) = action_label(action, mode) {
                    if let Some(key) = tap {
                        add(resolve(Text::Game(key)));
                    }
                    if let Some(key) = held {
                        add(format!("{hold} {}", resolve(Text::Game(key))));
                    }
                }
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join(" / "))
}

/// The panel's type and row sizes for a summary, pixels.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Metrics {
    /// The margin around the content.
    pub margin: f32,
    pub heading_px: f32,
    /// A group header's height (the controller glyph).
    pub group_px: f32,
    /// A row's height before its text wraps.
    pub row_px: f32,
    pub glyph_px: f32,
    pub font_px: f32,
}

/// Size the rows so that `summary` fills a panel `height` pixels tall, within readable bounds:
/// a short summary keeps large rows, and a long one shrinks them down to the smallest readable
/// size, below which it would rather run off the bottom.
pub fn metrics(summary: &Summary, height: f32) -> Metrics {
    let margin = (height * 0.03).max(4.0);
    let rows = summary.groups.iter().map(|g| g.lines.len()).sum::<usize>() as f32;
    let groups = summary.groups.len() as f32;
    // In row heights: the heading takes one and a half, each group header three quarters.
    let units = 1.5 + groups * 0.75 + rows;
    let available = (height - 2.0 * margin).max(0.0);
    let row_px = (available / units.max(1.0)).clamp(MIN_ROW_PX, MAX_ROW_PX);
    Metrics {
        margin,
        heading_px: row_px * 1.5,
        group_px: row_px * 0.75,
        row_px,
        glyph_px: row_px * 0.85,
        font_px: row_px * 0.5,
    }
}

/// The smallest row the panel stays readable at, pixels.
const MIN_ROW_PX: f32 = 24.0;
/// The largest row worth drawing, pixels.
const MAX_ROW_PX: f32 = 56.0;

#[cfg(test)]
mod tests {
    use jc3gi::input::input_action_map::Action::*;

    use super::*;
    use crate::controllers::{
        bindings::tables::describe::{Gesture, ModAction, StickDir},
        mode::OnFootVariant,
        summary::localise::humanise,
    };

    const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

    fn english(text: Text) -> String {
        match text {
            Text::Game(key) => humanise(key),
            Text::Mod(text) => text.english().to_owned(),
        }
    }

    fn row(control: Control, gesture: Gesture, effect: Effect) -> Row {
        Row {
            control,
            gesture,
            effect,
        }
    }

    #[test]
    fn groups_run_off_hand_dominant_then_either_and_rows_by_control() {
        let rows = [
            row(
                Control::System,
                Gesture::Press,
                Effect::Actions(vec![GUI_PAUSE]),
            ),
            row(
                Control::A(Hand::Right),
                Gesture::Press,
                Effect::Actions(vec![JUMP, BTN_A]),
            ),
            row(
                Control::Trigger(Hand::Right),
                Gesture::Press,
                Effect::Actions(vec![FIRE_RIGHT, R2]),
            ),
            row(
                Control::Grip(Hand::Left),
                Gesture::Press,
                Effect::Actions(vec![FIRE_GRAPPLE, L1]),
            ),
        ];
        let summary = build(&rows, ON_FOOT, Hand::Right, &english);
        let sides: Vec<_> = summary.groups.iter().map(|g| g.side).collect();
        assert_eq!(
            sides,
            vec![
                Side::Hand(Hand::Left),
                Side::Hand(Hand::Right),
                Side::Either
            ]
        );
        let right: Vec<_> = summary.groups[1].lines.iter().map(|l| l.control).collect();
        assert_eq!(
            right,
            vec![Control::Trigger(Hand::Right), Control::A(Hand::Right)]
        );
        assert_eq!(summary.groups[1].lines[0].text, "Fire weapon");
        assert_eq!(summary.heading, "On foot");
    }

    #[test]
    fn hidden_actions_drop_and_overloads_get_the_hold_prefix() {
        let rows = [
            row(
                Control::B(Hand::Right),
                Gesture::Press,
                Effect::Actions(vec![RELOAD, BTN_X]),
            ),
            row(
                Control::Stick(Hand::Left, StickDir::Any),
                Gesture::Press,
                Effect::Actions(vec![MOVE_ALL]),
            ),
            row(
                Control::System,
                Gesture::Hold,
                Effect::Actions(vec![GUI_PDA]),
            ),
        ];
        let summary = build(&rows, ON_FOOT, Hand::Right, &english);
        let lines: Vec<_> = summary.groups.iter().flat_map(|g| &g.lines).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Reload / Hold Holster weapon");
        assert_eq!(lines[1].prefix.as_deref(), Some("Hold"));
        assert_eq!(lines[1].text, "Commlink");
    }

    #[test]
    fn labels_that_read_the_same_appear_once() {
        let wingsuit = Mode::Airborne(crate::controllers::mode::AirborneVariant::Wingsuit);
        let same = |text: Text| match text {
            Text::Game("buttonhint_air_brake") => "Air Brake".to_owned(),
            Text::Game("buttonhint_wingsuit_air_brake") => "AIR BRAKE".to_owned(),
            other => english(other),
        };
        let rows = [row(
            Control::Trigger(Hand::Left),
            Gesture::Press,
            Effect::Actions(vec![CANCEL, WINGSUIT_AIRBRAKE]),
        )];
        let summary = build(&rows, wingsuit, Hand::Right, &same);
        assert_eq!(summary.groups[0].lines[0].text, "Air Brake");
    }

    #[test]
    fn mod_rows_use_the_mod_strings() {
        let rows = [row(
            Control::Stick(Hand::Right, StickDir::Down),
            Gesture::Flick,
            Effect::Mod(ModAction::SeatedCrouch),
        )];
        let summary = build(&rows, ON_FOOT, Hand::Right, &english);
        let line = &summary.groups[0].lines[0];
        assert_eq!(line.prefix.as_deref(), Some("FLICK"));
        assert_eq!(line.text, "Crouch (seated)");
    }

    #[test]
    fn rows_shrink_to_fit_within_readable_bounds() {
        let summary = |rows: usize| Summary {
            heading: String::new(),
            groups: vec![Group {
                side: Side::Either,
                lines: vec![
                    Line {
                        control: Control::System,
                        prefix: None,
                        text: String::new(),
                    };
                    rows
                ],
            }],
        };
        let few = metrics(&summary(2), 640.0);
        let many = metrics(&summary(14), 640.0);
        let too_many = metrics(&summary(100), 640.0);
        assert_eq!(few.row_px, MAX_ROW_PX);
        assert!(many.row_px < few.row_px && many.row_px > MIN_ROW_PX);
        assert_eq!(too_many.row_px, MIN_ROW_PX);
        let used = 2.0 * many.margin + many.heading_px + many.group_px + 14.0 * many.row_px;
        assert!((used - 640.0).abs() < 1.0, "{used}");
    }
}
