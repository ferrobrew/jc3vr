//! The paired editor's transaction and its settings-file boundary.

use super::*;
use crate::config::{
    engine::{Snapshot, load, store},
    write,
};

#[test]
fn threshold_edits_remain_valid_and_survive_the_settings_file() {
    let mut cfg = Snapshot::defaults();
    let input = &mut cfg.part_mut(&CONTROLLERS_CONFIG).input;
    for pair in [
        &mut input.trigger_press,
        &mut input.squeeze_press,
        &mut input.squeeze_force_press,
        &mut input.trackpad_press,
    ] {
        set_threshold(pair, true, 0.2);
        assert!(pair.release <= pair.press);
        set_threshold(pair, false, 0.8);
        assert_eq!(pair.press, 0.8);
        assert_eq!(pair.release, 0.8);
        set_threshold(pair, true, 0.2);
        assert_eq!(pair.release, 0.2);
        let before = *pair;
        for invalid in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            set_threshold(pair, true, invalid);
            assert_eq!(pair.press, before.press);
            assert_eq!(pair.release, before.release);
            set_threshold(pair, false, invalid);
            assert_eq!(pair.press, before.press);
            assert_eq!(pair.release, before.release);
        }
    }
    input.validate().unwrap();
    let text = cfg.to_toml().unwrap();
    let path = std::env::temp_dir().join(format!(
        "jc3vr-threshold-settings-{}.toml",
        std::process::id()
    ));
    write::replace(&path, text.as_bytes()).unwrap();
    let schema = store::schema();
    let loaded = load::parse(schema, &std::fs::read_to_string(&path).unwrap()).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(loaded.report.reset.is_empty(), "{:?}", loaded.report);
    assert_eq!(schema.serialize(loaded.root.as_ref()).unwrap(), text);
}
