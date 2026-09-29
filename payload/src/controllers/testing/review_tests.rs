use super::*;

#[test]
fn zero_cooldown_and_instant_blends_are_valid() {
    let mut file = from_config(&Config::new());
    file.controllers.bindings.yank.cooldown_s = 0.0;
    file.controllers.hands.yield_blend_s = 0.0;
    file.controllers.hands.return_blend_s = 0.0;
    file.controllers.hands.anchor.blend_s = 0.0;
    file.controllers.weapons.foregrip.blend_s = 0.0;
    file.controllers.weapons.foregrip.min_span_m = 0.0;
    file.controllers.weapons.grenade.release_peak_span_s = 0.0;
    file.controllers.roomscale.fade.out_s = 0.0;
    file.controllers.roomscale.fade.in_s = 0.0;
    file.controllers.vehicles.physical.wheel.return_deg_per_s = 0.0;
    file.controllers.vehicles.physical.wheel.max_angle_deg = 540.0;
    validate(&file).unwrap();
    parse(&serde_json::to_string(&file).unwrap()).unwrap();
}

#[test]
fn nested_typos_and_missing_fields_are_rejected() {
    let file = from_config(&Config::new());
    let mut value = serde_json::to_value(file).unwrap();
    value["controllers"]["hands"]["yield_blend_typo"] = 0.1.into();
    assert!(matches!(parse(&value.to_string()), Err(Error::Json(_))));
    let mut value = serde_json::to_value(file).unwrap();
    value["controllers"]["hands"]
        .as_object_mut()
        .unwrap()
        .remove("yield_blend_s");
    assert!(parse(&value.to_string()).is_err());
}

#[test]
fn preset_stages_enable_the_required_support() {
    for preset in Preset::ALL {
        let mut c = Config::new().controllers;
        apply_preset(&mut c, preset);
        let hands = !matches!(preset, Preset::InputOnly | Preset::GamepadBaseline);
        assert_eq!(c.bindings.yank.enabled, hands);
        assert_eq!(c.body.remap_hands, hands);
        assert_eq!(c.overlay.enabled, hands);
    }
}

#[test]
fn every_owned_switch_marks_the_selected_preset_modified() {
    let baseline = Config::new().controllers;
    let mut leaves = Vec::new();
    collect_leaves(
        &serde_json::to_value(baseline).unwrap(),
        String::new(),
        &mut leaves,
    );
    let mut owned = 0;
    for preset in Preset::ALL {
        let mut expected = baseline;
        apply_preset(&mut expected, preset);
        let expected_value = serde_json::to_value(expected).unwrap();
        for path in &leaves {
            let mut changed_value = expected_value.clone();
            let leaf = changed_value.pointer_mut(path).unwrap();
            if let Some(value) = leaf.as_bool() {
                *leaf = (!value).into();
            } else if leaf.is_string() {
                // Obtain another valid enum variant from a different preset.
                let alternative = Preset::ALL.into_iter().find_map(|other| {
                    let mut c = baseline;
                    apply_preset(&mut c, other);
                    let value = serde_json::to_value(c).unwrap();
                    let alternative = value.pointer(path).unwrap();
                    (alternative != leaf).then(|| alternative.clone())
                });
                let Some(alternative) = alternative else {
                    continue;
                };
                *leaf = alternative;
            } else {
                continue;
            }
            let changed: ControllersConfig = serde_json::from_value(changed_value.clone()).unwrap();
            let mut reapplied = changed;
            apply_preset(&mut reapplied, preset);
            let reapplied = serde_json::to_value(reapplied).unwrap();
            let is_owned = reapplied.pointer(path) != changed_value.pointer(path);
            assert_eq!(
                preset_modified(&changed, preset),
                is_owned,
                "{preset:?}: {path}"
            );
            owned += usize::from(is_owned);
        }
    }
    assert!(owned > 300);
}

fn collect_leaves(value: &serde_json::Value, path: String, leaves: &mut Vec<String>) {
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            collect_leaves(value, format!("{path}/{key}"), leaves);
        }
    } else {
        leaves.push(path);
    }
}

#[test]
fn nonfinite_nested_tuning_is_rejected_before_mutation() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut config = Config::new();
        let mut file = from_config(&config);
        file.controllers.fingers.finger_bend_deg[0] = value;
        assert!(apply(&file, &mut config).is_err());
        file = from_config(&config);
        file.controllers.weapons.grenade.warning_interval_s[0] = value;
        assert!(apply(&file, &mut config).is_err());
    }
}
