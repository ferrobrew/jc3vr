use super::*;

fn sweep() -> strike::Sweep {
    strike::Sweep {
        origin: Vec3::ZERO,
        direction: Vec3::X,
        length: 3.0,
    }
}

#[test]
fn a_hidden_near_pane_does_not_hide_an_intact_far_pane() {
    let mut casts = 0;
    let found = visible_hit(&sweep(), |ray, remaining| {
        casts += 1;
        assert!(remaining > 0.0);
        if casts == 1 {
            Ok(Some((Vec3::X, true, None)))
        } else {
            assert!(ray.origin.x > 1.0);
            Ok(Some((Vec3::X * 2.0, false, Some(7))))
        }
    })
    .unwrap();
    assert_eq!(found, Some((Vec3::X * 2.0, 7)));
    assert_eq!(casts, 2);
}

#[test]
fn a_solid_between_hidden_and_intact_glass_blocks_the_strike() {
    let mut casts = 0;
    let found = visible_hit::<()>(&sweep(), |_, _| {
        casts += 1;
        Ok(Some((Vec3::X * casts as f32, casts == 1, None)))
    })
    .unwrap();
    assert_eq!(found, None);
    assert_eq!(casts, 2);
}

#[test]
fn hidden_glass_continuation_has_a_budget_and_stops_at_the_endpoint() {
    let mut casts = 0;
    let found = visible_hit::<()>(&sweep(), |ray, _| {
        casts += 1;
        Ok(Some((ray.origin, true, None)))
    })
    .unwrap();
    assert_eq!(found, None);
    assert_eq!(casts, 16);
    let found =
        visible_hit::<()>(&sweep(), |_, _| Ok(Some((Vec3::X * 2.9995, true, None)))).unwrap();
    assert_eq!(found, None);
}

#[test]
fn a_sweep_through_only_hidden_panes_reaches_open_space() {
    let mut casts = 0;
    let found = visible_hit::<()>(&sweep(), |_, _| {
        casts += 1;
        Ok(match casts {
            1 => Some((Vec3::X, true, None)),
            2 => Some((Vec3::X * 2.0, true, None)),
            _ => None,
        })
    })
    .unwrap();
    assert_eq!(found, None);
    assert_eq!(casts, 3);
}
