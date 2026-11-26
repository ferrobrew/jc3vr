//! Invalid panel geometry never reaches the draw constants.

use super::*;

#[test]
fn invalid_inputs_and_overflow_never_publish_panel_corners() {
    let mut params = PanelParams {
        pos: Vec3::ZERO,
        rot: Quat::IDENTITY,
        aspect: 1.3,
        distance: 3.0,
        panel_height: 2.0,
    };
    assert!(compute_world_corners(&params).is_some());
    for invalid in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
        params.aspect = invalid;
        assert!(compute_world_corners(&params).is_none());
    }
    params.aspect = f32::MAX;
    params.panel_height = f32::MAX;
    assert!(compute_world_corners(&params).is_none());
    params.aspect = 1.3;
    params.panel_height = 2.0;
    params.pos.x = f32::NAN;
    assert!(compute_world_corners(&params).is_none());
}
