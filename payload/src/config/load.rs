//! The tolerant load of this configuration, for the tests: the engine's load
//! ([`jc3vr_core::config::load`]) for a [`Config`]. The checks it runs on the settings whose types admit unusable values are the
//! ones the groups' slots carry ([`SlotOptions::validated`](jc3vr_core::config::SlotOptions::validated)):
//! the controller settings, the VR turn settings, the HUD, and the body dimensions.

use jc3vr_core::config::{
    self,
    load::{LoadError, LoadReport},
};

use crate::config::{Config, schema::SCHEMA};

/// A loaded configuration and what the load had to change to get it.
pub(crate) struct Loaded {
    pub config: Config,
    pub report: LoadReport,
}

/// Load a settings file's text: parse it, merge it over the defaults, deserialize it resetting the
/// values that cannot be read, and validate it. Fails only when the text is not TOML.
pub(crate) fn parse(text: &str) -> Result<Loaded, LoadError> {
    let loaded = config::load::parse(&SCHEMA, text)?;
    let config = loaded
        .root
        .downcast::<Config>()
        .expect("config: the schema loads a Config");
    Ok(Loaded {
        config: *config,
        report: loaded.report,
    })
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
