//! The paired editor's transaction and its settings-file boundary.

use super::*;

#[test]
fn threshold_edits_remain_valid_and_survive_the_settings_file() {
    let mut cfg = config::Config::new();
    for pair in [
        &mut cfg.controllers.input.trigger_press,
        &mut cfg.controllers.input.squeeze_press,
        &mut cfg.controllers.input.squeeze_force_press,
        &mut cfg.controllers.input.trackpad_press,
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
    cfg.controllers.bindings.stick_chord_window_s = 0.001;
    cfg.controllers.bindings.panel_chord_window_s = 0.001;
    config::validate::controllers(&cfg.controllers).unwrap();
    let text = toml::to_string_pretty(&cfg).unwrap();
    let path = std::env::temp_dir().join(format!(
        "jc3vr-threshold-settings-{}.toml",
        std::process::id()
    ));
    config::write::replace(&path, text.as_bytes()).unwrap();
    let loaded = config::load::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(loaded.report.reset.is_empty(), "{:?}", loaded.report);
    assert_eq!(toml::to_string_pretty(&loaded.config).unwrap(), text);
}
