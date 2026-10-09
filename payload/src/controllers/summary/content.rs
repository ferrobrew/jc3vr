//! The summary's content and layout model: the described rows labelled, grouped by hand, and the
//! sizes the panel lays them out at. Pure.

use crate::{
    controllers::{
        bindings::tables::describe::{Control, Effect, Row},
        frame::Hand,
        mode::Mode,
        summary::labels::{
            ActionLabel, HOLD_KEY, ModText, Text, action_label, gesture_prefix, heading, mod_action,
        },
    },
    vr::input::semantic::bound::SourceName,
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
    /// The game's context hints on screen now, relabelled ([`crate::controllers::summary::context`]),
    /// under a title rather than a controller.
    Context,
}

/// The rows of one side.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub side: Side,
    /// The group's title, for a group headed by text rather than a controller glyph.
    pub title: Option<String>,
    pub lines: Vec<Line>,
}

/// One input and what it does, labelled.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub control: Control,
    /// The control's name, for a control no glyph shows: the runtime's name for a source the mod
    /// does not recognise, or "unbound".
    pub source: Option<String>,
    /// The gesture's prefix ("HOLD", "FLICK", ...).
    pub prefix: Option<String>,
    pub text: String,
}

/// Label `rows` for `mode` and group them: the off-hand first (the panel is on its wrist), then
/// the dominant hand, then the inputs of either hand. Within a group, the rows are ordered by
/// control ([`Control::order`]). A row whose actions all go unlabelled in this mode is left out.
/// `name` gives the text of a source the mod has no glyph for.
pub fn build(
    rows: &[Row],
    mode: Mode,
    dominant: Hand,
    resolve: &dyn Fn(Text) -> String,
    name: &dyn Fn(SourceName) -> String,
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
        title: None,
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
                source: match row.control {
                    Control::Named(_, source) => Some(name(source)),
                    Control::Unbound => Some(resolve(Text::Mod(ModText::Unbound))),
                    _ => None,
                },
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

/// Put the game's context hints on screen now, `lines`, first in `summary`, under `title`.
pub fn with_context(mut summary: Summary, lines: Vec<Line>, title: String) -> Summary {
    if !lines.is_empty() {
        summary.groups.insert(
            0,
            Group {
                side: Side::Context,
                title: Some(title),
                lines,
            },
        );
    }
    summary
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
                    if let Some(text) = tap {
                        add(resolve(text));
                    }
                    if let Some(text) = held {
                        add(format!("{hold} {}", resolve(text)));
                    }
                }
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join(" / "))
}

/// The panel's type and row sizes, pixels.
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

/// The sizes for rows `row_px` tall, within readable bounds.
pub fn metrics(row_px: f32) -> Metrics {
    let row_px = if row_px.is_finite() {
        row_px.clamp(MIN_ROW_PX, MAX_ROW_PX)
    } else {
        MIN_ROW_PX
    };
    Metrics {
        margin: row_px * 0.4,
        heading_px: row_px * 1.5,
        group_px: row_px * 0.75,
        row_px,
        glyph_px: row_px * 0.85,
        font_px: row_px * 0.5,
    }
}

/// The row height that fits a summary measured `height` pixels tall at rows `row_px` tall into
/// `max_height` pixels: `row_px` when it already fits, and otherwise shrunk in proportion, down to
/// the smallest readable row, below which the summary would rather run off the bottom.
pub fn fit_row_px(height: f32, row_px: f32, max_height: f32) -> f32 {
    let row_px = metrics(row_px).row_px;
    if !(height.is_finite() && max_height.is_finite()) || height <= max_height || height <= 0.0 {
        return row_px;
    }
    (row_px * max_height / height).clamp(MIN_ROW_PX, row_px)
}

/// The smallest row the panel stays readable at, pixels.
pub const MIN_ROW_PX: f32 = 20.0;
/// The largest row worth drawing, pixels.
pub const MAX_ROW_PX: f32 = 64.0;

#[cfg(test)]
mod tests {
    use jc3gi::input::input_action_map::Action::*;

    use super::*;
    use crate::{
        controllers::{
            bindings::tables::describe::{Gesture, ModAction, StickDir},
            mode::OnFootVariant,
            summary::localise::humanise,
        },
        vr::input::semantic::{
            SemanticSet,
            bound::{EnumeratedSource, SourceTable},
        },
    };

    const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

    fn english(text: Text) -> String {
        match text {
            Text::Game(key) => humanise(key),
            Text::Mod(text) => text.english().to_owned(),
        }
    }

    fn no_name(_: SourceName) -> String {
        unreachable!("no row names a source")
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
        let summary = build(&rows, ON_FOOT, Hand::Right, &english, &no_name);
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
        let summary = build(&rows, ON_FOOT, Hand::Right, &english, &no_name);
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
        let summary = build(&rows, wingsuit, Hand::Right, &same, &no_name);
        assert_eq!(summary.groups[0].lines[0].text, "Air Brake");
    }

    #[test]
    fn mod_rows_use_the_mod_strings() {
        let rows = [row(
            Control::Stick(Hand::Right, StickDir::Down),
            Gesture::Flick,
            Effect::Mod(ModAction::SeatedCrouch),
        )];
        let summary = build(&rows, ON_FOOT, Hand::Right, &english, &no_name);
        let line = &summary.groups[0].lines[0];
        assert_eq!(line.prefix.as_deref(), Some("FLICK"));
        assert_eq!(line.text, "Crouch (seated)");
    }

    #[test]
    fn controls_without_a_glyph_are_named() {
        let mut table = SourceTable::default();
        table.insert(
            SemanticSet::OnFoot,
            [Some(vec![EnumeratedSource {
                path: "/user/hand/right/input/pinch_ext/value",
                localized: Some("Right Pinch"),
            }])],
        );
        let pinch = table.sources(SemanticSet::OnFoot, 0).unwrap()[0].name;
        let rows = [
            row(
                Control::Named(Some(Hand::Right), pinch),
                Gesture::Press,
                Effect::Actions(vec![JUMP]),
            ),
            row(
                Control::Unbound,
                Gesture::Press,
                Effect::Actions(vec![RELOAD]),
            ),
            row(
                Control::A(Hand::Right),
                Gesture::Press,
                Effect::Actions(vec![USE_ITEM]),
            ),
        ];
        let name = |name| table.name(name).to_owned();
        let summary = build(&rows, ON_FOOT, Hand::Right, &english, &name);
        let lines: Vec<_> = summary.groups.iter().flat_map(|g| &g.lines).collect();
        let source = |control| {
            lines
                .iter()
                .find(|l| l.control == control)
                .map(|l| l.source.clone())
        };
        assert_eq!(
            source(Control::Named(Some(Hand::Right), pinch)),
            Some(Some("Right Pinch".to_owned()))
        );
        assert_eq!(source(Control::Unbound), Some(Some("UNBOUND".to_owned())));
        assert_eq!(source(Control::A(Hand::Right)), Some(None));
        // The named source stays with its hand, and the unbound row goes with either hand's, last.
        assert_eq!(summary.groups[0].side, Side::Hand(Hand::Right));
        let either = summary.groups.last().unwrap();
        assert_eq!(either.side, Side::Either);
        assert_eq!(either.lines.last().unwrap().control, Control::Unbound);
    }

    #[test]
    fn metrics_scale_with_the_row_within_readable_bounds() {
        let m = metrics(32.0);
        assert_eq!((m.row_px, m.font_px, m.heading_px), (32.0, 16.0, 48.0));
        assert_eq!(metrics(1.0).row_px, MIN_ROW_PX);
        assert_eq!(metrics(1000.0).row_px, MAX_ROW_PX);
        assert_eq!(metrics(f32::NAN).row_px, MIN_ROW_PX);
    }

    #[test]
    fn rows_shrink_only_to_fit_the_tallest_panel() {
        assert_eq!(fit_row_px(800.0, 32.0, 1280.0), 32.0);
        assert_eq!(fit_row_px(2560.0, 32.0, 1280.0), 16.0f32.max(MIN_ROW_PX));
        assert_eq!(fit_row_px(1600.0, 32.0, 1280.0), 32.0 * 0.8);
        assert_eq!(fit_row_px(100_000.0, 32.0, 1280.0), MIN_ROW_PX);
        assert_eq!(fit_row_px(f32::NAN, 32.0, 1280.0), 32.0);
    }
}
