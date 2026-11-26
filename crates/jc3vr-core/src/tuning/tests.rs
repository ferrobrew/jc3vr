use std::{fs, path::Path};

use pretty_assertions::assert_eq;
use serde::Deserialize;

use super::*;
use crate::config::engine::{Invalid, positive};

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    radius_m: f32,
    inner: Inner,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inner {
    gain: f32,
}

impl Tuning for Sample {
    fn validate(&self) -> Result<(), Invalid> {
        positive("radius_m", self.radius_m, 10.0)
    }
}

const LOCATION: Location = Location {
    crate_name: "sample-crate",
    file_name: "sample.toml",
    manifest_dir: "/repo/crates/sample-crate",
};

const VALID: &str = "radius_m = 0.5\n[inner]\ngain = 2.0\n";

fn file() -> &'static TuningFile<Sample> {
    Box::leak(Box::new(TuningFile::new(LOCATION)))
}

fn stage_error(text: &str) -> FileCause {
    file().stage(text).unwrap_err()
}

#[test]
fn a_staged_file_is_published_only_by_the_commit() {
    let file = file();
    file.stage(VALID).unwrap();
    assert!(file.commit());
    file.stage("radius_m = 1.5\n[inner]\ngain = 2.0\n").unwrap();
    assert_eq!(file.get().radius_m, 0.5);
    assert!(file.commit());
    assert_eq!(file.get().radius_m, 1.5);
    assert!(!file.commit(), "a second commit has nothing to publish");
}

#[test]
fn a_reference_from_before_a_reload_keeps_its_values() {
    let file = file();
    file.stage(VALID).unwrap();
    file.commit();
    let before = file.get();
    file.stage("radius_m = 1.5\n[inner]\ngain = 2.0\n").unwrap();
    file.commit();
    assert_eq!((before.radius_m, file.get().radius_m), (0.5, 1.5));
}

#[test]
fn the_latest_staged_text_wins() {
    let file = file();
    file.stage(VALID).unwrap();
    file.stage("radius_m = 2.5\n[inner]\ngain = 2.0\n").unwrap();
    file.commit();
    assert_eq!(file.get().radius_m, 2.5);
}

#[test]
fn a_file_must_name_every_key_and_nothing_else() {
    for text in [
        "radius_m = 0.5\n",
        "radius_m = 0.5\nextra = 1\n[inner]\ngain = 2.0\n",
        "radius_m = 0.5\n[inner]\ngain = 2.0\nextra = 1\n",
        "radius_m = \"wide\"\n[inner]\ngain = 2.0\n",
        "radius_m = ",
    ] {
        assert!(
            matches!(stage_error(text), FileCause::Syntax(_)),
            "{text:?} loaded"
        );
    }
}

#[test]
fn a_value_out_of_range_fails_validation_and_names_the_value() {
    let cause = stage_error("radius_m = -1.0\n[inner]\ngain = 2.0\n");
    let message = cause
        .at(Path::new("C:\\mod\\tuning\\sample-crate\\sample.toml"))
        .to_string();
    assert!(
        message.starts_with(
            "tuning: C:\\mod\\tuning\\sample-crate\\sample.toml: invalid value: radius_m"
        ),
        "{message}"
    );
}

#[test]
fn a_rejected_edit_leaves_the_published_values() {
    let file = file();
    file.stage(VALID).unwrap();
    file.commit();
    assert!(file.stage("radius_m = 0.0\n[inner]\ngain = 2.0\n").is_err());
    assert!(!file.commit());
    assert_eq!(file.get().radius_m, 0.5);
}

#[test]
fn the_packaged_and_source_tree_paths_follow_the_owning_crate() {
    assert_eq!(
        LOCATION.packaged(Path::new("C:\\mod\\tuning")),
        Path::new("C:\\mod\\tuning\\sample-crate\\sample.toml")
    );
    assert_eq!(
        LOCATION.source_tree(),
        "/repo/crates/sample-crate/tuning/sample.toml"
    );
    let windows = Location {
        manifest_dir: "D:\\src\\jc3vr\\crates\\sample-crate",
        ..LOCATION
    };
    assert_eq!(
        windows.source_tree(),
        "D:\\src\\jc3vr\\crates\\sample-crate\\tuning\\sample.toml"
    );
    assert_eq!(LOCATION.to_string(), "sample-crate/sample.toml");
}

#[test]
fn a_missing_file_reads_as_missing() {
    let path =
        std::env::temp_dir().join(format!("jc3vr-tuning-missing-{}.toml", std::process::id()));
    let error = fs::read_to_string(&path).unwrap_err();
    assert!(matches!(FileCause::read(error), FileCause::Missing));
}

#[test]
fn the_watcher_stages_a_changed_file_and_skips_a_rejected_one() {
    let dir = std::env::temp_dir().join(format!("jc3vr-tuning-watch-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.toml");
    fs::write(&path, VALID).unwrap();
    let file = file();
    let mut watched = runtime::tests::watched(file, path.clone());
    file.commit();

    fs::write(&path, "radius_m = 3.0\n[inner]\ngain = 2.0\n").unwrap();
    runtime::tests::force_poll(&mut watched);
    assert!(file.commit());
    assert_eq!(file.get().radius_m, 3.0);

    fs::write(&path, "radius_m = 3.0\n[inner]\n").unwrap();
    runtime::tests::force_poll(&mut watched);
    assert!(!file.commit(), "a rejected edit stages nothing");
    assert_eq!(file.get().radius_m, 3.0);

    fs::remove_dir_all(&dir).unwrap();
}
