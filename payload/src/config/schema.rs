//! The configuration's schema: how the engine defaults, migrates, validates, and reports a
//! [`Config`], registered with the store before anything reads the configuration
//! ([`crate::config::register`]).

use toml::Table;

use crate::config::{
    Config,
    engine::{ConfigRoot, Schema},
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
