//! The game's context button hints, relabelled for the controllers: the hints the HUD has queued
//! (release the tethers, the gunner seat, a hang jump, a mission's prompt), each shown on the wrist
//! with the controller input that drives its action in the current mode, rather than the gamepad
//! glyph the hidden HUD hint would show ([`crate::controllers::summary::game_hints`]).
//!
//! The input tick reads the HUD's queue ([`read`]) without waiting on its lock, and [`lines`] turns
//! each hint into a row of the summary: the hint's own localised description, with the glyph of the
//! summary row whose actions include the hint's action, and the "HOLD" prefix for a hint the game
//! asks to be held. A hint whose action no row drives is left out, since the wrist cannot say how to
//! do it.

use jc3gi::{
    input::{button_mapping::ButtonInteraction, input_action_map::Action},
    ui::hud_ui::{ButtonHintType, HUDUI},
};
use windows::Win32::System::Threading::{LeaveCriticalSection, TryEnterCriticalSection};

use crate::controllers::{
    bindings::tables::describe::{Control, Effect, Gesture, ModAction, Row},
    mode::Mode,
    summary::{
        content::Line,
        labels::{ActionLabel, Text, action_label, gesture_prefix},
    },
};

/// One queued hint, as the summary needs it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ContextHint {
    /// The HUD's id for the hint.
    pub id: u32,
    /// The action the hint names.
    pub action: Action,
    /// Whether the game asks for the action to be held.
    pub hold: bool,
    /// The hash of the hint's `buttonhint_*` description key.
    pub description: u32,
}

/// The hints the HUD has queued, in its order: the first three context hints and every big hint,
/// the ones the HUD shows. `None` when the queue cannot be read without waiting (a source is adding
/// one), so the caller keeps the last read.
///
/// # Safety
///
/// The game thread, past startup.
pub unsafe fn read() -> Option<Vec<ContextHint>> {
    // SAFETY: the HUD is a singleton that lives for the game's lifetime once created.
    let hud = unsafe { HUDUI::get() }?;
    let lock = hud.m_ButtonHintsLock;
    if lock.is_null() {
        return None;
    }
    // SAFETY: the lock is the HUD's own critical section, initialised with the HUD; a try never
    // waits, so this cannot deadlock against a source holding it.
    if !unsafe { TryEnterCriticalSection(lock) }.as_bool() {
        return None;
    }
    let mut out = Vec::new();
    let mut normal = 0;
    let (begin, end) = (hud.m_ButtonHints.begin, hud.m_ButtonHints.end);
    if !begin.is_null() && end >= begin {
        // SAFETY: the vector's live range, read under its lock.
        let count = unsafe { end.offset_from(begin) } as usize;
        // SAFETY: as above.
        let hints = unsafe { std::slice::from_raw_parts(begin, count) };
        for hint in hints {
            let request = &hint.m_Request;
            let shown = match request.m_Type {
                ButtonHintType::BUTTON_HINT_NORMAL => {
                    normal += 1;
                    normal <= 3
                }
                ButtonHintType::BUTTON_HINT_BIG => true,
                // The use prompt names its item, not the button; the summary has the use row.
                _ => false,
            };
            if shown {
                out.push(ContextHint {
                    id: hint.m_Id,
                    action: request.m_Mapping.m_Action1,
                    hold: request.m_Mapping.m_Interaction
                        == ButtonInteraction::BINT_SINGLE_BUTTON_HOLD,
                    description: request.m_Mapping.m_DescriptionKey,
                });
            }
        }
    }
    // SAFETY: entered above.
    unsafe { LeaveCriticalSection(lock) };
    Some(out)
}

/// The summary rows for `hints` in `mode`: each hint's description (`description` resolves the
/// hint's key, `None` for a key the game has no string for or whose string is only a placeholder)
/// beside the control of the first of `rows` that drives its action. `hold` is the game's "HOLD"
/// prefix, and `resolve` resolves the labels a hint without a description falls back to.
pub fn lines(
    hints: &[ContextHint],
    rows: &[Row],
    mode: Mode,
    hold: &str,
    description: &dyn Fn(u32) -> Option<String>,
    resolve: &dyn Fn(Text) -> String,
) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for hint in hints {
        let Some(row) = row_for(rows, hint.action) else {
            continue;
        };
        let Some(text) = description(hint.description)
            .filter(|text| !text.contains('{'))
            .or_else(|| fallback_label(hint.action, mode, resolve))
        else {
            continue;
        };
        let prefix = if hint.hold {
            Some(hold.to_owned())
        } else {
            gesture_prefix(row.gesture).map(resolve)
        };
        let line = Line {
            control: row.control,
            source: None,
            prefix,
            text,
        };
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// The row that drives `action`: one that injects it, or for the tethers' release and the reel-in's
/// cancel, the grapple button's double tap that releases them.
fn row_for(rows: &[Row], action: Action) -> Option<&Row> {
    let usable = |row: &&Row| !matches!(row.control, Control::Unbound | Control::Named(..));
    rows.iter()
        .filter(usable)
        .find(|row| matches!(&row.effect, Effect::Actions(actions) if actions.contains(&action)))
        .or_else(|| {
            matches!(
                action,
                Action::RELEASE_GRAPPLE | Action::VEHICLE_RELEASE_GRAPPLE | Action::CANCEL
            )
            .then(|| {
                rows.iter().filter(usable).find(|row| {
                    row.gesture == Gesture::DoubleTap
                        && row.effect == Effect::Mod(ModAction::ReleaseTethers)
                })
            })
            .flatten()
        })
}

/// The summary's own label for `action` in `mode`, for a hint without a description.
fn fallback_label(action: Action, mode: Mode, resolve: &dyn Fn(Text) -> String) -> Option<String> {
    match action_label(action, mode)? {
        ActionLabel::Shown { tap, hold } => tap.or(hold).map(resolve),
        ActionLabel::Hidden => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{
        frame::Hand,
        mode::{OnFootVariant, VehicleClass, VehicleSeat},
        summary::localise::humanise,
    };

    const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

    fn english(text: Text) -> String {
        match text {
            Text::Game(key) => humanise(key),
            Text::Mod(text) => text.english().to_owned(),
        }
    }

    fn hint(action: Action, hold: bool, description: u32) -> ContextHint {
        ContextHint {
            id: description,
            action,
            hold,
            description,
        }
    }

    fn row(control: Control, gesture: Gesture, effect: Effect) -> Row {
        Row {
            control,
            gesture,
            effect,
        }
    }

    fn described(key: u32) -> Option<String> {
        match key {
            1 => Some("Enter gunner seat".to_owned()),
            2 => Some("Release tethers".to_owned()),
            3 => Some("{0x1234}".to_owned()),
            _ => None,
        }
    }

    #[test]
    fn a_hint_takes_the_control_that_drives_its_action() {
        let car = Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver: false,
        });
        let rows = [row(
            Control::B(Hand::Right),
            Gesture::Press,
            Effect::Actions(vec![Action::EXIT_VEHICLE, Action::ENTER_VEHICLE]),
        )];
        let lines = lines(
            &[hint(Action::ENTER_VEHICLE, true, 1)],
            &rows,
            car,
            "HOLD",
            &described,
            &english,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].control, Control::B(Hand::Right));
        assert_eq!(lines[0].prefix.as_deref(), Some("HOLD"));
        assert_eq!(lines[0].text, "Enter gunner seat");
    }

    #[test]
    fn the_tethers_release_is_the_grapples_double_tap() {
        let rows = [
            row(
                Control::Unbound,
                Gesture::Press,
                Effect::Actions(vec![Action::RELEASE_GRAPPLE]),
            ),
            row(
                Control::B(Hand::Left),
                Gesture::DoubleTap,
                Effect::Mod(ModAction::ReleaseTethers),
            ),
        ];
        let lines = lines(
            &[hint(Action::RELEASE_GRAPPLE, false, 2)],
            &rows,
            ON_FOOT,
            "HOLD",
            &described,
            &english,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].control, Control::B(Hand::Left));
        assert!(lines[0].prefix.is_some(), "the double tap's prefix");
    }

    #[test]
    fn a_placeholder_description_falls_back_and_an_unreachable_hint_is_left_out() {
        let rows = [row(
            Control::A(Hand::Right),
            Gesture::Press,
            Effect::Actions(vec![Action::JUMP]),
        )];
        let lines = lines(
            &[
                hint(Action::JUMP, false, 3),
                hint(Action::PRECISION_AIM, false, 1),
            ],
            &rows,
            ON_FOOT,
            "HOLD",
            &described,
            &english,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Jump");
    }
}
