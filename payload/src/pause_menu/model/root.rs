//! Where the VR entry goes: the root menu the pause menu opens on, directly below its options
//! entry.
//!
//! The movie's `InitMenu` builds each valid menu's fixed entries, and the engine then adds its own
//! entries around them, the in-game menu's above (resume and the rest) and a few below (go
//! online). The movie can only append to or prepend to a list from outside, so the VR entry is
//! appended while `InitMenu` runs, right after the movie has added the options entry. In the
//! in-game menu the movie adds the main-menu entry after the options entry in the same call, and
//! only when the main-menu list is initialised first; so the runtime runs `InitMenu` without the
//! main-menu list, appends the VR entry, runs it again for the main-menu list alone, and appends
//! the main-menu entry the movie would have added. See `docs/mod/pause-menu.md`.

use jc3gi::ui::pause_ui::PauseMenuType;

/// How to add the VR entry to a freshly loaded movie, decided from the valid menus.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct RootPlan {
    /// The root menu that gets the VR entry: the menu the movie opens on.
    pub(crate) menu: PauseMenuType,
    /// Whether the movie would add its main-menu entry after the options entry, so `InitMenu` runs
    /// in two parts with the VR entry between them.
    pub(crate) split_main_menu: bool,
}

impl RootPlan {
    /// The plan for the valid menus `valid` (indexed by [`PauseMenuType`]), or `None` when the
    /// movie opens on neither the in-game nor the front-end menu. The in-game menu wins, as it
    /// does when the engine picks the menu the movie opens on.
    pub(crate) fn new(valid: [bool; MENU_COUNT]) -> Option<Self> {
        let is_valid = |menu: PauseMenuType| valid[menu as usize];
        let menu = if is_valid(PauseMenuType::PAUSE_SHIP) {
            PauseMenuType::PAUSE_SHIP
        } else if is_valid(PauseMenuType::PAUSE_FRONTEND) {
            PauseMenuType::PAUSE_FRONTEND
        } else {
            return None;
        };
        Some(Self {
            menu,
            split_main_menu: menu == PauseMenuType::PAUSE_SHIP
                && is_valid(PauseMenuType::PAUSE_MAIN_MENU),
        })
    }
}

/// The number of menus, the length of
/// [`PauseUI::m_IsValidMenu`](jc3gi::ui::pause_ui::PauseUI::m_IsValidMenu).
pub(crate) const MENU_COUNT: usize = 7;

#[cfg(test)]
mod tests {
    use PauseMenuType::{
        PAUSE_CONTROLS, PAUSE_DEV, PAUSE_FRONTEND, PAUSE_MAIN_MENU, PAUSE_OPTIONS, PAUSE_SHIP,
    };

    use super::*;

    fn valid(menus: &[PauseMenuType]) -> [bool; MENU_COUNT] {
        let mut valid = [false; MENU_COUNT];
        for &menu in menus {
            valid[menu as usize] = true;
        }
        valid
    }

    #[test]
    fn in_game_splits_around_the_main_menu_entry() {
        let plan = RootPlan::new(valid(&[
            PAUSE_SHIP,
            PAUSE_OPTIONS,
            PAUSE_MAIN_MENU,
            PAUSE_CONTROLS,
        ]));
        assert_eq!(
            plan,
            Some(RootPlan {
                menu: PAUSE_SHIP,
                split_main_menu: true
            })
        );
    }

    #[test]
    fn in_game_without_a_main_menu_appends_once() {
        // Boom Island has no main-menu submenu.
        let plan = RootPlan::new(valid(&[PAUSE_SHIP, PAUSE_OPTIONS, PAUSE_CONTROLS]));
        assert_eq!(
            plan,
            Some(RootPlan {
                menu: PAUSE_SHIP,
                split_main_menu: false
            })
        );
    }

    #[test]
    fn the_title_screen_uses_the_front_end_menu() {
        let plan = RootPlan::new(valid(&[PAUSE_FRONTEND, PAUSE_OPTIONS, PAUSE_CONTROLS]));
        assert_eq!(
            plan,
            Some(RootPlan {
                menu: PAUSE_FRONTEND,
                split_main_menu: false
            })
        );
        // The main-menu entry belongs to the in-game menu only.
        let plan = RootPlan::new(valid(&[PAUSE_FRONTEND, PAUSE_MAIN_MENU]));
        assert_eq!(plan.map(|plan| plan.split_main_menu), Some(false));
    }

    #[test]
    fn the_in_game_menu_wins_over_the_front_end() {
        let plan = RootPlan::new(valid(&[PAUSE_FRONTEND, PAUSE_SHIP]));
        assert_eq!(plan.map(|plan| plan.menu), Some(PAUSE_SHIP));
    }

    #[test]
    fn no_root_menu_no_entry() {
        assert_eq!(RootPlan::new(valid(&[])), None);
        assert_eq!(RootPlan::new(valid(&[PAUSE_DEV, PAUSE_OPTIONS])), None);
    }
}
