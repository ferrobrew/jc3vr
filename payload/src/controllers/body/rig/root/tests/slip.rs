use body_rig::body::EYES_TO_NECK;
use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::rig::root::eye_rig_slip;

/// A neck-to-eyes arm a little right, up, and forward, world metres.
const ARM: Vec3 = Vec3::new(0.03, 0.06, -0.1);

#[test]
fn a_body_facing_the_view_has_no_slip() {
    assert_eq!(eye_rig_slip(ARM, Quat::IDENTITY, 1.0), Vec2::ZERO);
}

#[test]
fn a_body_turned_under_the_view_stands_off_by_the_turned_arm() {
    // The body faces a quarter turn left of the view: the view's frame is the body's turned back.
    let turned = std::f32::consts::FRAC_PI_2;
    let slip = eye_rig_slip(ARM, Quat::from_rotation_y(-turned), 1.0);
    // Rico's eyes sit the arm turned with him, ahead of him to the view's left.
    let rico = Quat::from_rotation_y(turned) * ARM;
    assert!((slip - Vec2::new(rico.x - ARM.x, rico.z - ARM.z)).length() < 1e-5);
    assert!((slip.length() - 0.1476).abs() < 1e-3, "{slip:?}");
    // In room metres: a larger world scale shrinks it.
    let scaled = eye_rig_slip(ARM, Quat::from_rotation_y(-turned), 2.0);
    assert!((scaled * 2.0 - slip).length() < 1e-6);
}

#[test]
fn bad_inputs_have_no_slip() {
    assert_eq!(eye_rig_slip(Vec3::NAN, Quat::IDENTITY, 1.0), Vec2::ZERO);
    assert_eq!(
        eye_rig_slip(ARM, Quat::from_rotation_y(1.0), 0.0),
        Vec2::ZERO
    );
}

/// Under the neck model the camera hangs the eyes `-EYES_TO_NECK` from the neutral head over the
/// root, in the view's frame, and the rig's pelvis is the root's cockpit point plus the slip plus
/// `EYES_TO_NECK` turned by the body's heading. With the neck model's arm, that pelvis is where the
/// root stands, whatever the body's turn under the view.
#[test]
fn the_neck_model_slip_puts_the_rig_pelvis_on_the_root() {
    let arm = -EYES_TO_NECK;
    let shift = Vec2::new(0.4, -0.2);
    for turned in [0.0, 0.5, -1.3, std::f32::consts::PI] {
        let slip = eye_rig_slip(arm, Quat::from_rotation_y(-turned), 1.0);
        let body_arm = Quat::from_rotation_y(turned) * EYES_TO_NECK;
        let pelvis = shift + slip + Vec2::new(body_arm.x, body_arm.z);
        let root = shift - Vec2::new(arm.x, arm.z);
        assert!((pelvis - root).length() < 1e-5, "{turned}: {pelvis} {root}");
    }
}
