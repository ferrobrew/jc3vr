//! The pause menu's VR pages for the settings `controllers.local.json` holds: `VR: CONTROLS`
//! ([`controls`]) and `VR: COMFORT` ([`comfort`]), and the glue they and the body page share. See
//! `docs/mod/pause-menu.md` for the menu itself.
//!
//! The pages edit the live configuration, so every change applies at once. The macros below build
//! an item from a place expression in [`Config`] (`|c| c.controllers.aim.gun`): the item reads the
//! place, writes it and marks the settings changed, and takes its default from [`Config::new`], so
//! the menu's defaults are the configuration's. Leaving a page saves the settings through the
//! persistence worker when anything changed ([`save_if_changed`]); nothing is saved otherwise, so
//! browsing the menu never creates the file.

pub(crate) mod comfort;
pub(crate) mod controls;

use std::sync::atomic::{AtomicBool, Ordering};

use smol_str::SmolStr;

use crate::{
    config::Config,
    controllers::{
        config::ControllersConfig,
        persistence::{self, Request, Status},
    },
    headpose::config::VrTurnConfig,
    pause_menu::model::page::ButtonOutcome,
};

/// A [`Toggle`](crate::pause_menu::model::page::Toggle) over the `bool` at a place in [`Config`].
macro_rules! config_toggle {
    ($label:expr, |$c:ident| $place:expr $(,)?) => {
        $crate::pause_menu::model::page::Toggle {
            label: $label.into(),
            get: || $crate::controllers::menu::read_config(|$c| $place),
            set: |value| $crate::controllers::menu::edit_config(|$c| $place = value),
            default: {
                let $c = &$crate::config::Config::new();
                $place
            },
        }
    };
}

/// A [`Slider`](crate::pause_menu::model::page::Slider) over the `f32` at a place in [`Config`].
macro_rules! config_slider {
    ($label:expr, $range:expr, $format:expr, |$c:ident| $place:expr $(,)?) => {
        $crate::pause_menu::model::page::Slider {
            label: $label.into(),
            range: $range,
            format: $format,
            get: || $crate::controllers::menu::read_config(|$c| $place),
            set: |value| $crate::controllers::menu::edit_config(|$c| $place = value),
            default: {
                let $c = &$crate::config::Config::new();
                $place
            },
        }
    };
}

/// A [`Choice`](crate::pause_menu::model::page::Choice) over the value at a place in [`Config`],
/// among the constant `$values`, labelled by the constant `$labels` (the same length, in the same
/// order). A value not in the list shows as the first option.
macro_rules! config_choice {
    ($label:expr, $values:expr, $labels:expr, |$c:ident| $place:expr $(,)?) => {
        $crate::pause_menu::model::page::Choice {
            label: $label.into(),
            options: $labels,
            get: || {
                $crate::controllers::menu::index_of(
                    $values,
                    &$crate::controllers::menu::read_config(|$c| $place),
                )
            },
            set: |index| {
                if let Some(&value) = $values.get(index) {
                    $crate::controllers::menu::edit_config(|$c| $place = value);
                }
            },
            default: $crate::controllers::menu::index_of($values, &{
                let $c = &$crate::config::Config::new();
                $place
            }),
        }
    };
}

pub(crate) use config_choice;
pub(crate) use config_slider;
pub(crate) use config_toggle;

/// Read the configuration.
pub(crate) fn read_config<R>(f: impl FnOnce(&Config) -> R) -> R {
    with_config(|config| f(config))
}

/// Change the configuration and mark the settings changed, so leaving the page saves them.
pub(crate) fn edit_config(f: impl FnOnce(&mut Config)) {
    with_config(f);
    set_dirty(true);
}

/// The index of `value` in `values`, or `0` when it is not there.
pub(crate) fn index_of<T: PartialEq>(values: &[T], value: &T) -> usize {
    values.iter().position(|v| v == value).unwrap_or(0)
}

/// The index of the entry of `values` nearest `value`, or `0` for an empty list or a value that is
/// not a number.
pub(crate) fn nearest_index(values: &[f32], value: f32) -> usize {
    values
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - value).abs().total_cmp(&(*b - value).abs()))
        .map_or(0, |(index, _)| index)
}

/// Save the settings through the persistence worker when a page changed them. Run when the player
/// leaves a page. When the worker is busy, the settings stay marked changed and the next page left
/// tries again.
pub(crate) fn save_if_changed() {
    if !take_dirty() {
        return;
    }
    if LOAD_PENDING.load(Ordering::Relaxed) {
        // The load replaces the settings anyway.
        tracing::info!("pause menu: a settings load is running; the changes are not saved");
        return;
    }
    let file = read_config(persistence::from_config);
    if persistence::queue(Request::Save(Box::new(file))) {
        persistence::start_pending();
        tracing::debug!("pause menu: saving the controller settings");
    } else {
        set_dirty(true);
        tracing::warn!(
            "pause menu: the controller settings were not saved: another settings operation is \
             running or the payload is shutting down; they are saved when a VR page is next left"
        );
    }
}

/// Load the saved settings through the persistence worker, replacing the live ones. The page is
/// rebuilt by [`on_frame`] when the load finishes.
pub(crate) fn reload_saved() -> ButtonOutcome {
    if persistence::queue(Request::Load) {
        persistence::start_pending();
        LOAD_PENDING.store(true, Ordering::Relaxed);
    } else {
        tracing::warn!(
            "pause menu: the saved controller settings were not loaded: another settings operation \
             is running or the payload is shutting down"
        );
    }
    ButtonOutcome::Stay
}

/// The per-frame work of a settings page: once a load it started has finished, rebuild the page so
/// its rows show the loaded values.
pub(crate) fn on_frame() -> ButtonOutcome {
    if !LOAD_PENDING.load(Ordering::Relaxed) {
        return ButtonOutcome::Stay;
    }
    match file_activity(&persistence::status()) {
        FileActivity::Busy => ButtonOutcome::Stay,
        activity => {
            LOAD_PENDING.store(false, Ordering::Relaxed);
            if activity == FileActivity::Succeeded {
                // The live settings are the file's now.
                set_dirty(false);
            }
            ButtonOutcome::Rebuild
        }
    }
}

/// The settings file's state, for a readout.
pub(crate) fn file_state() -> SmolStr {
    file_state_label(is_dirty(), file_activity(&persistence::status())).into()
}

/// Reset every setting `controllers.local.json` holds to its default, keeping the per-weapon sight
/// calibration, which is measured rather than chosen.
pub(crate) fn reset_to_defaults(config: &mut Config) {
    let sights = config.controllers.aim.sights;
    config.controllers = ControllersConfig::new();
    config.controllers.aim.sights = sights;
    config.headpose.vr_turn = VrTurnConfig::new();
}

/// Reset the live settings to their defaults ([`reset_to_defaults`]) and mark them changed.
pub(crate) fn reset_live_to_defaults() {
    edit_config(reset_to_defaults);
}

/// What the persistence worker last did, as the settings pages report it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum FileActivity {
    /// Nothing has run this session.
    Idle,
    /// An operation is queued or running.
    Busy,
    /// The last operation succeeded.
    Succeeded,
    /// The last operation failed; the log says why.
    Failed,
    /// The payload is shutting down.
    Closed,
}

/// Whether a page changed the settings since they were last saved or loaded.
static DIRTY: AtomicBool = AtomicBool::new(false);
/// Whether a load the pages started has not been seen to finish.
static LOAD_PENDING: AtomicBool = AtomicBool::new(false);

#[cfg(not(test))]
fn with_config<R>(f: impl FnOnce(&mut Config) -> R) -> R {
    f(&mut crate::config::CONFIG.lock())
}

// Tests run each item against a configuration of their own thread, so they can change settings
// without racing other tests over the global one.
#[cfg(test)]
fn with_config<R>(f: impl FnOnce(&mut Config) -> R) -> R {
    tests::TEST_CONFIG.with_borrow_mut(f)
}

fn set_dirty(dirty: bool) {
    DIRTY.store(dirty, Ordering::Relaxed);
}

fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

fn is_dirty() -> bool {
    DIRTY.load(Ordering::Relaxed)
}

fn file_activity(status: &Status) -> FileActivity {
    match status {
        Status::Idle => FileActivity::Idle,
        Status::Queued | Status::Running => FileActivity::Busy,
        Status::Finished(Ok(_)) => FileActivity::Succeeded,
        Status::Finished(Err(_)) => FileActivity::Failed,
        Status::ShuttingDown => FileActivity::Closed,
    }
}

fn file_state_label(dirty: bool, activity: FileActivity) -> &'static str {
    match (activity, dirty) {
        (FileActivity::Busy, _) => "WORKING",
        (FileActivity::Closed, _) => "UNAVAILABLE",
        (_, true) => "SAVED ON LEAVING",
        (FileActivity::Idle, false) => "NO CHANGES",
        (FileActivity::Succeeded, false) => "UP TO DATE",
        (FileActivity::Failed, false) => "FAILED, SEE LOG",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;

    use glam::{Vec2, Vec3};

    use super::*;
    use crate::{
        controllers::{
            config::{SchemeSetting, SightOverride},
            frame::Hand,
        },
        pause_menu::model::page::{Item, Page},
    };

    thread_local! {
        pub(crate) static TEST_CONFIG: RefCell<Config> = const { RefCell::new(Config::new()) };
    }

    /// Put every value item of `page` through every value it can take, checking that each reads
    /// back what was written and that the settings file stays valid, then reset the page and check
    /// that it is back at the configuration's defaults. Runs against this thread's configuration.
    pub(crate) fn exercise_page(page: &Page) {
        let valid = || {
            let file = read_config(persistence::from_config);
            persistence::apply(&file, &mut Config::new())
        };
        for item in &page.items {
            match item {
                Item::Toggle(toggle) => {
                    for value in [true, false] {
                        (toggle.set)(value);
                        assert_eq!((toggle.get)(), value, "{}", toggle.label);
                        valid().unwrap();
                    }
                }
                Item::Choice(choice) => {
                    assert!(choice.default < choice.options.len(), "{}", choice.label);
                    for index in 0..choice.options.len() {
                        (choice.set)(index);
                        assert_eq!((choice.get)(), index, "{}", choice.label);
                        valid().unwrap();
                    }
                }
                Item::Slider(slider) => {
                    let range = slider.range;
                    assert!(
                        (range.min..=range.max).contains(&slider.default),
                        "{}",
                        slider.label
                    );
                    for notch in [0, range.max_notch() / 2, range.max_notch()] {
                        (slider.set)(range.value_at(notch));
                        assert_eq!(range.notch_of((slider.get)()), notch, "{}", slider.label);
                        valid().unwrap_or_else(|e| panic!("{}: {e}", slider.label));
                    }
                }
                Item::Header(_) | Item::Text(_) | Item::Readout(_) | Item::Button(_) => {}
            }
        }
        page.reset_to_defaults();
        let settings = read_config(|c| serde_json::to_value(persistence::from_config(c)).unwrap());
        let defaults = serde_json::to_value(persistence::from_config(&Config::new())).unwrap();
        assert_eq!(settings, defaults);
    }

    #[test]
    fn resetting_keeps_the_sight_calibration() {
        let mut config = Config::new();
        let sight = SightOverride {
            weapon_hash: 7,
            offset_m: Vec3::new(0.0, 0.02, 0.0),
            angles_deg: Vec2::new(0.5, 0.0),
        };
        config.controllers.aim.sights.set(sight);
        config.controllers.dominant_hand = Hand::Left;
        config.controllers.scheme = SchemeSetting::Gamepad;
        config.headpose.vr_turn.snap_angle_deg = 45.0;
        config.headpose.mouse_sensitivity = 3.0;
        reset_to_defaults(&mut config);
        assert_eq!(config.controllers.aim.sights.get(7), Some(&sight));
        assert_eq!(config.controllers.dominant_hand, Hand::Right);
        assert_eq!(config.controllers.scheme, SchemeSetting::Auto);
        assert_eq!(config.headpose.vr_turn.snap_angle_deg, 30.0);
        // Settings outside the file are not the menu's to reset.
        assert_eq!(config.headpose.mouse_sensitivity, 3.0);
    }

    #[test]
    fn indices_map_values_and_fall_back_to_the_first() {
        let values = [SchemeSetting::Auto, SchemeSetting::Controllers];
        assert_eq!(index_of(&values, &SchemeSetting::Controllers), 1);
        assert_eq!(index_of(&values, &SchemeSetting::Gamepad), 0);
        let angles = [15.0, 30.0, 45.0];
        assert_eq!(nearest_index(&angles, 30.0), 1);
        assert_eq!(nearest_index(&angles, 40.0), 2);
        assert_eq!(nearest_index(&angles, -5.0), 0);
        assert_eq!(nearest_index(&angles, f32::NAN), 0);
        assert_eq!(nearest_index(&[], 1.0), 0);
    }

    #[test]
    fn the_file_state_prefers_activity_then_changes() {
        use FileActivity::*;
        assert_eq!(file_state_label(true, Busy), "WORKING");
        assert_eq!(file_state_label(false, Closed), "UNAVAILABLE");
        assert_eq!(file_state_label(true, Idle), "SAVED ON LEAVING");
        assert_eq!(file_state_label(true, Failed), "SAVED ON LEAVING");
        assert_eq!(file_state_label(false, Idle), "NO CHANGES");
        assert_eq!(file_state_label(false, Succeeded), "UP TO DATE");
        assert_eq!(file_state_label(false, Failed), "FAILED, SEE LOG");
    }
}
