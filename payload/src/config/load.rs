//! The tolerant load of this configuration: the engine's load ([`jc3vr_core::config::engine::load`])
//! for a [`Config`], and the checks it runs on the settings whose types admit unusable values.

use jc3vr_body::controllers::aim::sights::config::SightTable;
#[cfg(test)]
use jc3vr_core::config::engine;
#[cfg(test)]
use jc3vr_core::config::engine::load::{LoadError, LoadReport};
use jc3vr_core::config::engine::{
    Invalid,
    load::{Reset, reset_named},
};
use jc3vr_gameplay::controllers::config::{ControllersConfig, validate};
use jc3vr_hud::hud::HudConfig;
use jc3vr_view::headpose::config::VrTurnConfig;

use crate::config::Config;
#[cfg(test)]
use crate::config::schema::SCHEMA;

/// A loaded configuration and what the load had to change to get it.
#[cfg(test)]
pub(crate) struct Loaded {
    pub config: Config,
    pub report: LoadReport,
}

/// Load a settings file's text: parse it, merge it over the defaults, deserialize it resetting the
/// values that cannot be read, and validate it. Fails only when the text is not TOML.
#[cfg(test)]
pub(crate) fn parse(text: &str) -> Result<Loaded, LoadError> {
    let loaded = engine::load::parse(&SCHEMA, text)?;
    let config = loaded
        .root
        .downcast::<Config>()
        .expect("config: the schema loads a Config");
    Ok(Loaded {
        config: *config,
        report: loaded.report,
    })
}

/// Reset the settings that fail their checks. The controller and VR turn checks name the setting
/// that fails, so that setting is reset alone (or the group around it, when its default does not
/// pass either), and the subtree is reset whole only when that does not make it pass. The sight
/// corrections are measured rather than chosen, so a failure elsewhere in the controller settings
/// keeps them.
pub(crate) fn validate_subtrees(config: &mut Config, resets: &mut Vec<Reset>) {
    let fail = |resets: &mut Vec<Reset>, path: &str, error: Invalid| {
        resets.push(Reset {
            path: path.to_owned(),
            reason: error.to_string(),
        });
    };
    if let Err(error) = validate::sights(&config.controllers.aim.sights) {
        config.controllers.aim.sights = SightTable::EMPTY;
        fail(resets, "controllers.aim.sights", error);
    }
    if let Err(error) = reset_named(
        &mut config.controllers,
        &ControllersConfig::new(),
        "controllers",
        validate::controllers,
        resets,
    ) {
        let sights = config.controllers.aim.sights;
        config.controllers = ControllersConfig::new();
        config.controllers.aim.sights = sights;
        fail(resets, "controllers", error);
    }
    if let Err(error) = reset_named(
        &mut config.headpose.vr_turn,
        &VrTurnConfig::new(),
        "headpose.vr_turn",
        validate::vr_turn,
        resets,
    ) {
        config.headpose.vr_turn = VrTurnConfig::new();
        fail(resets, "headpose.vr_turn", error);
    }
    if let Err(error) = reset_named(
        &mut config.hud,
        &Config::new().hud,
        "hud",
        HudConfig::validate,
        resets,
    ) {
        config.hud = Config::new().hud;
        fail(resets, "hud", error);
    }
    if let Err(error) = config.body.validate() {
        config.body = Config::new().body;
        fail(resets, "body", Invalid(error.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn malformed_hud_values_reset_only_the_bad_settings() {
        let loaded = parse(
            r#"
[hud]
distance = nan
panel_scale = 0.7
[hud.egui_panel]
resolution = [0, 4294967295]
aspect = inf
[vr]
mirror = false
"#,
        )
        .unwrap();
        loaded.config.hud.validate().unwrap();
        assert_eq!(loaded.config.hud.panel_scale, 0.7);
        assert!(!loaded.config.vr.mirror);
        assert!(
            loaded
                .report
                .reset
                .iter()
                .all(|reset| reset.path.starts_with("hud."))
        );
        assert!(
            loaded
                .report
                .reset
                .iter()
                .any(|reset| reset.path == "hud.distance")
        );
        let loaded =
            parse("[hud.egui_panel]\nresolution = [\"wrong\", 960]\n[hud]\npanel_scale = 0.6")
                .unwrap();
        assert_eq!(loaded.config.hud.panel_scale, 0.6);
        loaded.config.hud.validate().unwrap();
        assert!(matches!(parse("[hud"), Err(LoadError::Syntax(_))));
    }
}
