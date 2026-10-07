//! The pause menu's VR settings pages: `VR: CONTROLS` ([`controls`]) for the controller and turning
//! settings, `VR: COMFORT` ([`comfort`]), and `VR: DISPLAY` ([`display`]) for the rendering and the
//! floating panel, and the glue they and the body page share. See `docs/mod/pause-menu.md` for the
//! menu itself.
//!
//! The pages edit the live configuration, so every change applies at once, and each change goes
//! through [`crate::config::edit`], so the settings file saves it like any other change the user
//! makes. The macros below build an item from a place expression in [`Config`]
//! (`|c| c.controllers.aim.gun`): the item reads the place, writes it, and takes its default from
//! [`Config::new`], so the menu's defaults are the configuration's.

pub(crate) mod comfort;
pub(crate) mod controls;
pub(crate) mod display;

use smol_str::SmolStr;

use crate::{
    config::{self, Config, SaveStatus},
    controllers::config::ControllersConfig,
    grapple::GrappleComfortConfig,
    headpose::config::VrTurnConfig,
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

/// Change the configuration as the user meant to, so that the change is saved.
pub(crate) fn edit_config(f: impl FnOnce(&mut Config)) {
    edit_with(f);
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

/// The settings file's state, for a readout.
pub(crate) fn settings_state() -> SmolStr {
    save_status_label(config::status()).into()
}

/// Reset every setting the controller pages cover (the controller settings, the VR turn settings,
/// and the grapple's comfort settings) to its default, keeping the per-weapon sight calibration,
/// which is measured rather than chosen. The display settings are not among them: they are the
/// renderer's and the HUD's, and `VR: DISPLAY`'s own reset covers them.
pub(crate) fn reset_to_defaults(config: &mut Config) {
    let sights = config.controllers.aim.sights;
    config.controllers = ControllersConfig::new();
    config.controllers.aim.sights = sights;
    config.headpose.vr_turn = VrTurnConfig::new();
    config.headpose.grapple = GrappleComfortConfig::new();
}

/// Reset the live settings to their defaults ([`reset_to_defaults`]).
pub(crate) fn reset_live_to_defaults() {
    edit_config(reset_to_defaults);
}

#[cfg(not(test))]
fn with_config<R>(f: impl FnOnce(&mut Config) -> R) -> R {
    f(&mut config::CONFIG.lock())
}

#[cfg(not(test))]
fn edit_with(f: impl FnOnce(&mut Config)) {
    f(&mut config::edit());
}

// Tests run each item against a configuration of their own thread, so they can change settings
// without racing other tests over the global one.
#[cfg(test)]
fn with_config<R>(f: impl FnOnce(&mut Config) -> R) -> R {
    tests::TEST_CONFIG.with_borrow_mut(f)
}

#[cfg(test)]
fn edit_with(f: impl FnOnce(&mut Config)) {
    tests::TEST_CONFIG.with_borrow_mut(f);
}

fn save_status_label(status: SaveStatus) -> &'static str {
    match status {
        SaveStatus::UpToDate => "UP TO DATE",
        SaveStatus::Pending => "SAVING",
        SaveStatus::Failed => "FAILED, SEE LOG",
        SaveStatus::Unavailable => "NOT SAVED",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;

    use glam::{Vec2, Vec3};

    use super::*;
    use crate::{
        config::validate,
        controllers::{
            config::{SchemeSetting, SightOverride},
            frame::Hand,
        },
        grapple::GrappleComfortMode,
        pause_menu::model::page::{Item, Page},
    };

    thread_local! {
        pub(crate) static TEST_CONFIG: RefCell<Config> = const { RefCell::new(Config::new()) };
    }

    /// Put every value item of `page` through every value it can take, checking that each reads
    /// back what was written and that the settings stay valid, then reset the page and check that
    /// it is back at the configuration's defaults. Runs against this thread's configuration.
    pub(crate) fn exercise_page(page: &Page) {
        let valid = || {
            read_config(|c| {
                validate::controllers(&c.controllers)?;
                validate::sights(&c.controllers.aim.sights)?;
                validate::vr_turn(&c.headpose.vr_turn)
            })
            .map_err(|e| e.to_string())?;
            read_config(|c| c.foveation.validate()).map_err(|e| e.to_string())
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
        // Only this page's items changed, so its reset brings the whole configuration back.
        page.reset_to_defaults();
        let saved = |c: &Config| serde_json::to_value(c).unwrap();
        assert_eq!(read_config(saved), saved(&Config::new()));
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
        config.headpose.grapple.mode = GrappleComfortMode::Off;
        config.headpose.mouse_sensitivity = 3.0;
        config.hud.distance = 2.0;
        reset_to_defaults(&mut config);
        assert_eq!(config.controllers.aim.sights.get(7), Some(&sight));
        assert_eq!(config.controllers.dominant_hand, Hand::Right);
        assert_eq!(config.controllers.scheme, SchemeSetting::Controllers);
        assert_eq!(config.headpose.vr_turn.snap_angle_deg, 30.0);
        assert_eq!(config.headpose.grapple.mode, GrappleComfortMode::HoldView);
        // Settings outside the controller pages are not the reset's, the display ones included.
        assert_eq!(config.headpose.mouse_sensitivity, 3.0);
        assert_eq!(config.hud.distance, 2.0);
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
}
