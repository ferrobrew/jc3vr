//! The configuration's schema: how the engine defaults, migrates, validates, and reports a
//! [`Config`], registered with the store before anything reads the configuration
//! ([`crate::config::register`]).

use toml::Table;

use crate::config::{
    Config,
    engine::{ConfigRoot, Parts, Schema, store::Root},
    format::{self, Format, Migrated},
    load::{self, Reset},
};

/// The schema the payload registers.
pub(crate) static SCHEMA: Schema = Schema::of::<Config>();

impl ConfigRoot for Config {
    fn defaults() -> Self {
        Self::new()
    }

    fn migrate(file: &mut Table, format: Format) -> Vec<Migrated> {
        format::migrate(file, format)
    }

    fn validate(&mut self, resets: &mut Vec<Reset>) {
        load::validate_subtrees(self, resets);
    }

    /// Note the settings for a tracking recording.
    fn note_saved(text: &str) {
        crate::controllers::diagnostics::note_settings(text);
    }
}

/// A configuration value is itself the parts of one root, so code that changes several groups
/// together ([`set_single_pass_feature`](crate::stereo::config::set_single_pass_feature)) runs on a
/// `Config` as it does on the live configuration.
impl Parts for Config {
    fn root(&self) -> &Root {
        self
    }

    fn root_mut(&mut self) -> &mut Root {
        self
    }
}
