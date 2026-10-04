//! Command parsing and routing: which of the pause movie's commands the VR menu handles itself,
//! which it lets the game handle, and which it observes on the way through.
//!
//! The VR rows talk to native code through the game's own settings commands, so the command detour
//! decides per command. A command carrying a VR row id is always swallowed: the settings manager
//! would ignore it, but the key capture would start listening for a key binding. So is a tab
//! command ([`TabCommands`]): the game does not know it, and so is the VR entry's own event
//! ([`ENTRY_EVENT`]), which the root menu's VR entry sends when the player selects it.

use jc3gi::ui::{pause_ui::PauseUI, ui_base::UIBase};

use crate::pause_menu::model::{ids::RowId, state::MenuState, tabs::TabCommands};

/// One argument of a command, as the runtime decoded it from the movie's value.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) enum Arg {
    Int(i32),
    Number(f64),
    Bool(bool),
    /// A string, classified as one the menu knows or not.
    Text(Text),
    /// Any other type (an object, undefined).
    Other,
}

/// The event the VR entry in the root menu posts when the player selects it. The game has no
/// handler for it; the command detour takes it and opens the VR screen.
pub(crate) const ENTRY_EVENT: &str = "jc3vr.pause_menu.open";

/// The sound the pause document plays after handling any main-menu selection.
pub(crate) const SELECT_SOUND: &str = "jc3_gui_msg_box_show_hide";

/// A string argument the menu recognises.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Text {
    /// [`ENTRY_EVENT`].
    EntryEvent,
    /// [`SELECT_SOUND`].
    SelectSound,
    /// Any other string.
    Other,
}

impl Text {
    /// Classify the string `bytes`.
    pub(crate) fn of(bytes: &[u8]) -> Self {
        if bytes == ENTRY_EVENT.as_bytes() {
            Text::EntryEvent
        } else if bytes == SELECT_SOUND.as_bytes() {
            Text::SelectSound
        } else {
            Text::Other
        }
    }
}

impl Arg {
    /// The argument as an integer: an int, or a number with an exact integer value (AS3 may widen
    /// an int to a number on the way out).
    pub(crate) fn as_int(self) -> Option<i32> {
        match self {
            Arg::Int(value) => Some(value),
            Arg::Number(value)
                if value.fract() == 0.0
                    && value >= f64::from(i32::MIN)
                    && value <= f64::from(i32::MAX) =>
            {
                Some(value as i32)
            }
            _ => None,
        }
    }
}

/// A pause-movie command the VR menu cares about.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Command {
    /// `populate_settings`: the movie is opening the settings screen.
    PopulateSettings,
    /// `set_setting_value(id, value)`.
    SetSettingValue { id: i32, value: i32 },
    /// `get_key_for_action(action)`.
    GetKeyForAction { action: i32 },
    /// `restore_default_keymap`.
    RestoreDefaultKeymap,
    /// `persist_changes`: the settings or controls screen is closing.
    PersistChanges,
    /// The base `inactive` command: the movie was unloaded.
    Inactive,
    /// The tab bar's callback chose tab `tab`.
    SelectTab { tab: usize },
    /// The `event` command with [`ENTRY_EVENT`]: the player selected the VR entry.
    OpenEntry,
    /// The `event` command with [`SELECT_SOUND`].
    SelectSound,
    /// Anything else, including a known command with arguments of the wrong shape.
    Other,
}

/// Parse a command from its hash and arguments, recognising the tab commands `tabs`.
pub(crate) fn parse(hash: u32, args: &[Arg], tabs: &TabCommands) -> Command {
    let int = |index: usize| args.get(index).and_then(|arg| arg.as_int());
    if let Some(tab) = tabs.tab_of(hash) {
        return Command::SelectTab { tab };
    }
    match hash {
        PauseUI::COMMAND_POPULATE_SETTINGS => Command::PopulateSettings,
        PauseUI::COMMAND_SET_SETTING_VALUE => match (int(0), int(1)) {
            (Some(id), Some(value)) => Command::SetSettingValue { id, value },
            _ => Command::Other,
        },
        PauseUI::COMMAND_GET_KEY_FOR_ACTION => match int(0) {
            Some(action) => Command::GetKeyForAction { action },
            None => Command::Other,
        },
        PauseUI::COMMAND_RESTORE_DEFAULT_KEYMAP => Command::RestoreDefaultKeymap,
        PauseUI::COMMAND_PERSIST_CHANGES => Command::PersistChanges,
        UIBase::COMMAND_INACTIVE => Command::Inactive,
        UIBase::COMMAND_EVENT => match args.first() {
            Some(Arg::Text(Text::EntryEvent)) => Command::OpenEntry,
            Some(Arg::Text(Text::SelectSound)) => Command::SelectSound,
            _ => Command::Other,
        },
        _ => Command::Other,
    }
}

/// What the command detour does with a command.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Route {
    /// Call the game's handler and nothing else.
    Pass,
    /// Call the game's handler with the populate flag raised, so the populate hook knows the
    /// screen is being opened rather than refreshed.
    Populate,
    /// Handle the command instead of the game.
    Swallow(Action),
    /// Call the game's handler, then act.
    PassThen(Action),
}

/// A VR-menu action a command leads to.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    /// Apply `value` to the row `row`.
    SetValue { row: RowId, value: i32 },
    /// Run the button on row `row`.
    Press { row: RowId },
    /// A command carried a VR id the current state cannot use (no page shown, or another page's
    /// row); drop it.
    Drop,
    /// Reset the shown page's values to their defaults.
    ResetPage,
    /// The player chose tab `tab` while the VR tabs are shown.
    SelectTab { tab: usize },
    /// The player selected the VR entry: open the settings screen on the VR tabs.
    OpenEntry,
    /// The player left the shown page.
    LeavePage,
    /// The movie was unloaded.
    Close,
}

/// Route `command` given the menu `state`.
pub(crate) fn route(command: Command, state: MenuState) -> Route {
    // `None` for a native id; `Some(None)` for a VR id the state cannot use.
    let resolve = |raw: i32| {
        RowId::decode(raw).map(|row| (state.shown_page() == Some(row.page)).then_some(row))
    };
    match command {
        Command::PopulateSettings => Route::Populate,
        Command::SetSettingValue { id, value } => match resolve(id) {
            None => Route::Pass,
            Some(None) => Route::Swallow(Action::Drop),
            Some(Some(row)) => Route::Swallow(Action::SetValue { row, value }),
        },
        Command::GetKeyForAction { action } => match resolve(action) {
            None => Route::Pass,
            Some(None) => Route::Swallow(Action::Drop),
            Some(Some(row)) => Route::Swallow(Action::Press { row }),
        },
        Command::RestoreDefaultKeymap if state.shown_page().is_some() => {
            Route::Swallow(Action::ResetPage)
        }
        Command::PersistChanges if state.shown_page().is_some() => {
            Route::PassThen(Action::LeavePage)
        }
        Command::Inactive => Route::PassThen(Action::Close),
        Command::SelectTab { tab } => match state.shown_page() {
            Some(_) => Route::Swallow(Action::SelectTab { tab }),
            // The callback outlives the VR tabs until the screen next opens; the game must not
            // see the command.
            None => Route::Swallow(Action::Drop),
        },
        // The game has no handler for the entry's event, and the entry exists only while the
        // session has one; anywhere else (a second selection while the screen opens) it is
        // dropped.
        Command::OpenEntry => match state {
            MenuState::EntryAdded { .. } => Route::Swallow(Action::OpenEntry),
            _ => Route::Swallow(Action::Drop),
        },
        Command::RestoreDefaultKeymap
        | Command::PersistChanges
        | Command::SelectSound
        | Command::Other => Route::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pause_menu::model::tabs::TAB_LIMIT;

    fn id(page: usize, item: usize) -> i32 {
        RowId::new(page, item).encode().unwrap()
    }

    const SHOWN_1: MenuState = MenuState::Shown { pages: 3, page: 1 };
    const ENTRIES: MenuState = MenuState::EntryAdded { pages: 3 };

    /// Stand-in tab command hashes, clear of every command hash the tests use.
    fn tabs() -> TabCommands {
        TabCommands::new(|name| 0x7000_0000 + name.parse::<u32>().unwrap())
    }

    /// [`super::parse`] with the stand-in tab commands.
    fn parse(hash: u32, args: &[Arg]) -> Command {
        super::parse(hash, args, &tabs())
    }

    #[test]
    fn parses_known_commands() {
        assert_eq!(
            parse(PauseUI::COMMAND_POPULATE_SETTINGS, &[]),
            Command::PopulateSettings
        );
        assert_eq!(
            parse(
                PauseUI::COMMAND_SET_SETTING_VALUE,
                &[Arg::Int(5), Arg::Number(2.0)]
            ),
            Command::SetSettingValue { id: 5, value: 2 }
        );
        assert_eq!(
            parse(PauseUI::COMMAND_GET_KEY_FOR_ACTION, &[Arg::Int(9)]),
            Command::GetKeyForAction { action: 9 }
        );
        assert_eq!(
            parse(PauseUI::COMMAND_RESTORE_DEFAULT_KEYMAP, &[]),
            Command::RestoreDefaultKeymap
        );
        assert_eq!(
            parse(PauseUI::COMMAND_PERSIST_CHANGES, &[]),
            Command::PersistChanges
        );
        assert_eq!(parse(UIBase::COMMAND_INACTIVE, &[]), Command::Inactive);
        assert_eq!(parse(UIBase::COMMAND_EVENT, &[Arg::Other]), Command::Other);
        assert_eq!(
            parse(UIBase::COMMAND_EVENT, &[Arg::Text(Text::Other)]),
            Command::Other
        );
        assert_eq!(parse(UIBase::COMMAND_EVENT, &[]), Command::Other);
        assert_eq!(
            parse(UIBase::COMMAND_EVENT, &[Arg::Text(Text::EntryEvent)]),
            Command::OpenEntry
        );
        assert_eq!(
            parse(UIBase::COMMAND_EVENT, &[Arg::Text(Text::SelectSound)]),
            Command::SelectSound
        );
        // Only the event command carries the entry's event.
        assert_eq!(
            parse(
                PauseUI::COMMAND_PERSIST_CHANGES,
                &[Arg::Text(Text::EntryEvent)]
            ),
            Command::PersistChanges
        );
    }

    #[test]
    fn strings_are_classified_exactly() {
        assert_eq!(Text::of(ENTRY_EVENT.as_bytes()), Text::EntryEvent);
        assert_eq!(Text::of(SELECT_SOUND.as_bytes()), Text::SelectSound);
        assert_eq!(Text::of(b""), Text::Other);
        assert_eq!(Text::of(b"pauseui.game.resume"), Text::Other);
        assert_eq!(Text::of(ENTRY_EVENT.to_uppercase().as_bytes()), Text::Other);
    }

    #[test]
    fn the_entry_event_opens_the_screen_only_from_the_root_menu() {
        assert_eq!(
            route(Command::OpenEntry, ENTRIES),
            Route::Swallow(Action::OpenEntry)
        );
        for state in [MenuState::Closed, SHOWN_1] {
            assert_eq!(
                route(Command::OpenEntry, state),
                Route::Swallow(Action::Drop)
            );
        }
        for state in [MenuState::Closed, ENTRIES, SHOWN_1] {
            assert_eq!(route(Command::SelectSound, state), Route::Pass);
        }
    }

    #[test]
    fn malformed_arguments_are_other() {
        assert_eq!(
            parse(PauseUI::COMMAND_SET_SETTING_VALUE, &[Arg::Int(5)]),
            Command::Other
        );
        assert_eq!(
            parse(
                PauseUI::COMMAND_SET_SETTING_VALUE,
                &[Arg::Other, Arg::Int(1)]
            ),
            Command::Other
        );
        assert_eq!(
            parse(PauseUI::COMMAND_GET_KEY_FOR_ACTION, &[Arg::Number(1.5)]),
            Command::Other
        );
        assert_eq!(
            parse(PauseUI::COMMAND_GET_KEY_FOR_ACTION, &[Arg::Bool(true)]),
            Command::Other
        );
    }

    #[test]
    fn numbers_convert_only_when_exact() {
        assert_eq!(Arg::Number(-3.0).as_int(), Some(-3));
        assert_eq!(Arg::Number(0.5).as_int(), None);
        assert_eq!(Arg::Number(1e12).as_int(), None);
        assert_eq!(Arg::Number(f64::NAN).as_int(), None);
    }

    #[test]
    fn native_settings_pass_through() {
        for state in [MenuState::Closed, ENTRIES, SHOWN_1] {
            assert_eq!(
                route(Command::SetSettingValue { id: 3, value: 1 }, state),
                Route::Pass
            );
            assert_eq!(
                route(Command::GetKeyForAction { action: 40 }, state),
                Route::Pass
            );
            assert_eq!(route(Command::Other, state), Route::Pass);
        }
    }

    #[test]
    fn rows_of_the_shown_page_are_handled() {
        let row = RowId::new(1, 4);
        assert_eq!(
            route(
                Command::SetSettingValue {
                    id: id(1, 4),
                    value: 7
                },
                SHOWN_1
            ),
            Route::Swallow(Action::SetValue { row, value: 7 })
        );
        assert_eq!(
            route(Command::GetKeyForAction { action: id(1, 4) }, SHOWN_1),
            Route::Swallow(Action::Press { row })
        );
    }

    #[test]
    fn vr_ids_never_reach_the_game() {
        for state in [MenuState::Closed, ENTRIES, SHOWN_1] {
            for raw in [id(0, 0), id(2, 3)] {
                assert_eq!(
                    route(Command::SetSettingValue { id: raw, value: 0 }, state),
                    Route::Swallow(Action::Drop)
                );
                assert_eq!(
                    route(Command::GetKeyForAction { action: raw }, state),
                    Route::Swallow(Action::Drop)
                );
            }
        }
    }

    #[test]
    fn page_commands_depend_on_a_shown_page() {
        assert_eq!(
            route(Command::RestoreDefaultKeymap, SHOWN_1),
            Route::Swallow(Action::ResetPage)
        );
        assert_eq!(route(Command::RestoreDefaultKeymap, ENTRIES), Route::Pass);
        assert_eq!(
            route(Command::PersistChanges, SHOWN_1),
            Route::PassThen(Action::LeavePage)
        );
        assert_eq!(route(Command::PersistChanges, ENTRIES), Route::Pass);
    }

    #[test]
    fn tab_commands_are_parsed_before_anything_else() {
        assert_eq!(parse(0x7000_0002, &[]), Command::SelectTab { tab: 2 });
        assert_eq!(parse(0x7000_0000 + TAB_LIMIT as u32, &[]), Command::Other);
    }

    #[test]
    fn tab_commands_never_reach_the_game() {
        assert_eq!(
            route(Command::SelectTab { tab: 2 }, SHOWN_1),
            Route::Swallow(Action::SelectTab { tab: 2 })
        );
        for state in [MenuState::Closed, ENTRIES] {
            assert_eq!(
                route(Command::SelectTab { tab: 0 }, state),
                Route::Swallow(Action::Drop)
            );
        }
    }

    #[test]
    fn lifecycle_commands() {
        assert_eq!(
            route(Command::PopulateSettings, MenuState::Closed),
            Route::Populate
        );
        for state in [MenuState::Closed, ENTRIES, SHOWN_1] {
            assert_eq!(
                route(Command::Inactive, state),
                Route::PassThen(Action::Close)
            );
        }
    }
}
