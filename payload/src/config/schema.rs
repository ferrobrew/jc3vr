//! The configuration's schema: how the engine defaults, migrates, and reports a [`Config`] (the
//! checks a load runs are the ones the groups' slots carry), registered with the store before anything reads the configuration
//! ([`crate::config::register`]).

use jc3vr_core::config::{
    ConfigRoot, Parts, Schema,
    format::{Format, Migrated},
    store::Root,
};
use toml::Table;

use crate::config::{Config, format};

/// The schema the payload registers.
pub(crate) static SCHEMA: Schema = Schema::of::<Config>();

impl ConfigRoot for Config {
    fn defaults() -> Self {
        Self::new()
    }

    fn migrate(file: &mut Table, format: Format) -> Vec<Migrated> {
        format::migrate(file, format)
    }

    /// Note the settings for a tracking recording.
    fn note_saved(text: &str) {
        jc3vr_view::recorder::tracking::note_settings(text);
    }
}

/// A configuration value is itself the parts of one root, so code that changes several groups
/// together ([`set_single_pass_feature`](jc3vr_view::stereo::config::set_single_pass_feature)) runs on a
/// `Config` as it does on the live configuration.
impl Parts for Config {
    fn root(&self) -> &Root {
        self
    }

    fn root_mut(&mut self) -> &mut Root {
        self
    }
}
