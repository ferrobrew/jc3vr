//! Every layer crate's tuning file, which the startup loads before anything reads the tuning
//! (`docs/mod/tuning.md`). A file added to a crate's `tuning/` directory is listed here; the test
//! below fails until it is.

use jc3vr_body::controllers::fingers::tuning::FINGERS_TUNING;
use jc3vr_body::{
    controllers::body::{ik_body::tuning::IK_BODY, tuning::BODY},
    tuning::FRESHNESS,
};
pub(crate) use jc3vr_core::tuning::{Source, shutdown, tick};
use jc3vr_core::{
    core_tuning::CORE,
    tuning::{AnyTuningFile, StartupError},
};
use jc3vr_gameplay::controllers::tuning::GAMEPLAY;
use jc3vr_grapple::{controllers::snatch::config::SNATCH, grapple::tuning::GRAPPLE};
use jc3vr_hud::hud::tuning::HUD;
use jc3vr_render::tuning::RENDER;
use jc3vr_tooling::tuning::TOOLING;
use jc3vr_ui::tuning::UI;
use jc3vr_vehicles::controllers::vehicles::tuning::VEHICLES;
use jc3vr_view::tuning::HAPTICS;

/// Loads every tuning file from `source` and starts watching them.
pub(crate) fn init(source: Source) -> Result<(), StartupError> {
    jc3vr_core::tuning::init(FILES, source)
}

/// The tuning files, in no particular order: each loads on its own.
static FILES: &[&dyn AnyTuningFile] = &[
    &VEHICLES,
    &GAMEPLAY,
    &UI,
    &TOOLING,
    &FRESHNESS,
    &CORE,
    &RENDER,
    &GRAPPLE,
    &SNATCH,
    &HAPTICS,
    &BODY,
    &IK_BODY,
    &HUD,
    &FINGERS_TUNING,
];

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use pretty_assertions::assert_eq;

    use super::*;

    /// The build stages each crate's files under the crate's directory name, and the payload reads
    /// them under the crate's package name, so the two must agree.
    #[test]
    fn every_shipped_file_is_listed_under_its_crates_directory() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
        let mut on_disk = BTreeSet::new();
        for entry in fs::read_dir(&crates).unwrap() {
            let dir = entry.unwrap().path();
            let Ok(files) = fs::read_dir(dir.join("tuning")) else {
                continue;
            };
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            for file in files {
                let file = file.unwrap().path();
                if file
                    .extension()
                    .is_some_and(|extension| extension == "toml")
                {
                    let file_name = file.file_name().unwrap().to_string_lossy().into_owned();
                    on_disk.insert(format!("{name}/{file_name}"));
                }
            }
        }

        let listed: BTreeSet<_> = FILES
            .iter()
            .map(|file| {
                let location = file.location();
                let dir = Path::new(location.manifest_dir).file_name().unwrap();
                assert_eq!(dir.to_string_lossy(), location.crate_name);
                location.to_string()
            })
            .collect();
        assert_eq!(listed.len(), FILES.len(), "a file is listed twice");
        assert_eq!(listed, on_disk);
    }
}
