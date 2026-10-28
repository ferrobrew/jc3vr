use super::*;

fn facts(now: Instant) -> UpdateFacts {
    UpdateFacts {
        owner: 11,
        weapon: 22,
        hand_target: None,
        native_target: Some(Vec3::new(3.0, 4.0, 5.0)),
        now,
        loaded: true,
        fire: true,
        reload_held: false,
        animation_allows: true,
    }
}

fn tick(arguments: UpdateArguments) -> (f32, Vec3) {
    match arguments {
        UpdateArguments::Tick { inputs, aim } => {
            assert_eq!(inputs.m_SecondaryFireInput, 0.0);
            (inputs.m_PrimaryFireInput, Vec3::from_array(aim.data))
        }
        UpdateArguments::Stop => panic!("expected a native state update"),
    }
}

#[test]
fn disabled_aim_ticks_both_phases_and_delivers_trigger_release() {
    let now = Instant::now();
    for _phase in 0..2 {
        let mut input = facts(now);
        assert_eq!(
            tick(update_arguments(facts(now))),
            (1.0, input.native_target.unwrap())
        );
        input.fire = false;
        assert_eq!(tick(update_arguments(input)).0, 0.0);
    }
}

#[test]
fn recovered_aim_uses_the_hand_point_and_expiry_returns_to_native() {
    let now = Instant::now();
    let mut input = facts(now);
    let point = Vec3::new(8.0, 9.0, 10.0);
    let target = HandTarget {
        owner: 11,
        weapon: 22,
        point,
        at: now,
    };
    input.hand_target = Some(target);
    assert_eq!(tick(update_arguments(input)), (1.0, point));
    let mut expired = facts(now + MAX_TARGET_AGE + Duration::from_nanos(1));
    expired.hand_target = Some(target);
    let native = expired.native_target.unwrap();
    assert_eq!(tick(update_arguments(expired)), (1.0, native));
}

#[test]
fn replacement_never_reuses_a_previous_characters_or_weapons_target() {
    let now = Instant::now();
    for (owner, weapon) in [(12, 22), (11, 23)] {
        let mut input = facts(now);
        input.hand_target = Some(HandTarget {
            owner,
            weapon,
            point: Vec3::ONE,
            at: now,
        });
        let native = input.native_target.unwrap();
        assert_eq!(tick(update_arguments(input)), (1.0, native));
    }
}

#[test]
fn traversal_reload_and_ammunition_withhold_fire_without_withholding_ticks() {
    let now = Instant::now();
    for gate in 0..3 {
        let mut input = facts(now);
        match gate {
            0 => input.animation_allows = false,
            1 => input.reload_held = true,
            _ => input.loaded = false,
        }
        assert_eq!(tick(update_arguments(input)).0, 0.0);
    }
}

#[test]
fn unavailable_or_invalid_targets_request_native_stop_even_with_fire_held() {
    let now = Instant::now();
    for native in [None, Some(Vec3::NAN)] {
        let mut input = facts(now);
        input.native_target = native;
        assert!(matches!(update_arguments(input), UpdateArguments::Stop));
    }
    let mut input = facts(now);
    input.native_target = None;
    input.hand_target = Some(HandTarget {
        owner: 11,
        weapon: 22,
        point: Vec3::NAN,
        at: now,
    });
    assert!(matches!(update_arguments(input), UpdateArguments::Stop));
}
