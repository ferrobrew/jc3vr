//! A small configuration root of the engine's own, which shows that the engine names no
//! subsystem's settings, and tests of the engine against it.

use pretty_assertions::assert_eq;
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::config::engine::{
    ConfigRoot, ConfigSlot, Invalid, Schema,
    format::{self, Format, Migrated, VERSION, VERSION_KEY},
    load::{self, LoadReport, Reset, reset_named},
    projection,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Sample {
    pub height: f32,
    pub name: Option<String>,
    pub group: Group,
    pub list: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Group {
    pub limit: f32,
    pub on: bool,
}

impl ConfigRoot for Sample {
    fn defaults() -> Self {
        Self {
            height: 1.8,
            name: None,
            group: Group {
                limit: 3.0,
                on: true,
            },
            list: vec![1, 2],
        }
    }

    fn migrate(file: &mut Table, format: Format) -> Vec<Migrated> {
        if format != Format::Zero {
            return Vec::new();
        }
        format::remove_superseded(
            file,
            vec![("group.limit".to_owned(), vec![Value::Float(2.0)])],
        )
    }

    fn validate(&mut self, resets: &mut Vec<Reset>) {
        let defaults = Self::defaults().group;
        if let Err(error) = reset_named(&mut self.group, &defaults, "group", check_group, resets) {
            self.group = defaults;
            resets.push(Reset {
                path: "group".to_owned(),
                reason: error.to_string(),
            });
        }
    }

    fn note_saved(_text: &str) {}
}

pub(super) static SAMPLE: Schema = Schema::of::<Sample>();

fn check_group(group: &Group) -> Result<(), Invalid> {
    if !(0.0..=10.0).contains(&group.limit) {
        return Err(Invalid(format!(
            "limit must be in [0, 10], got {}",
            group.limit
        )));
    }
    Ok(())
}

#[test]
fn the_defaults_write_only_the_version() {
    assert_eq!(
        format::to_text(&SAMPLE, &Sample::defaults())
            .unwrap()
            .trim(),
        format!("{VERSION_KEY} = {VERSION}")
    );
}

#[test]
fn a_save_writes_the_changed_settings_as_written() {
    let mut sample = Sample::defaults();
    sample.height = 1.849;
    sample.name = Some("a".to_owned());
    sample.list = vec![4];
    assert_eq!(
        format::to_text(&SAMPLE, &sample).unwrap(),
        "height = 1.849\nlist = [4]\nname = \"a\"\nversion = 1\n"
    );
}

#[test]
fn a_saved_file_loads_as_the_settings_it_was_written_from() {
    let mut sample = Sample::defaults();
    sample.group.on = false;
    sample.name = Some("b".to_owned());
    let text = format::to_text(&SAMPLE, &sample).unwrap();
    let loaded = load::parse(&SAMPLE, &text).unwrap();
    assert_eq!(loaded.report, LoadReport::default());
    assert_eq!(loaded.format, Format::Current);
    assert_eq!(loaded.root.downcast_ref::<Sample>(), Some(&sample));
}

#[test]
fn a_format_zero_file_takes_the_schemas_migration() {
    let loaded = load::parse(&SAMPLE, "height = 1.7\n[group]\nlimit = 2.0\non = false\n").unwrap();
    let sample = loaded.root.downcast_ref::<Sample>().unwrap();
    assert_eq!(sample.group.limit, 3.0);
    assert!(!sample.group.on);
    assert_eq!(sample.height, 1.7);
    assert_eq!(
        loaded.report.migrated,
        [Migrated {
            path: "group.limit".to_owned(),
            removed: "2.0".to_owned(),
        }]
    );
    // An optional setting's `None` default is left out of the defaults, as out of the file.
    assert_eq!(loaded.report.defaulted, ["list"]);
}

#[test]
fn bad_values_are_skipped_reset_or_validated_alone() {
    let loaded = load::parse(
        &SAMPLE,
        "version = 1\nheight = \"tall\"\nretired = 1\n[group]\nlimit = 50.0\non = false\n",
    )
    .unwrap();
    let sample = loaded.root.downcast_ref::<Sample>().unwrap();
    assert_eq!(sample.height, 1.8);
    assert_eq!(sample.group.limit, 3.0);
    assert!(!sample.group.on);
    assert_eq!(loaded.report.ignored, ["retired"]);
    let resets: Vec<&str> = loaded
        .report
        .reset
        .iter()
        .map(|reset| reset.path.as_str())
        .collect();
    assert_eq!(resets, ["height", "group.limit"]);
}

#[test]
fn a_projection_finds_its_part() {
    let mut sample = Sample::defaults();
    let limit = projection!(Sample, group.limit);
    let whole = projection!(Sample);
    let slot = ConfigSlot::new("group.limit");
    slot.bind(limit);
    *slot.project_mut(&mut sample) = 7.0;
    assert_eq!(sample.group.limit, 7.0);
    let root = ConfigSlot::<Sample>::new("");
    root.bind(whole);
    assert_eq!(root.project(&sample), &sample);
}
