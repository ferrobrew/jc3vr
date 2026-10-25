use std::time::Duration;

use glam::{Quat, Vec3};

use super::*;
use crate::controllers::{
    body::mirror::{Plane, pouch_markers},
    overlay::primitive::{Anchor, Primitive},
    weapons::pouch_models::render_matrix,
};

fn frames() -> Frames {
    Frames {
        head_world: Vec3::new(3.0, 2.0, -4.0),
        head_cockpit: Vec3::new(0.0, 1.7, 0.0),
        view: Quat::from_rotation_y(0.8),
        world_scale: 1.3,
        character: Mat4::from_rotation_translation(
            Quat::from_rotation_y(-0.2),
            Vec3::new(2.0, 0.0, -3.0),
        ),
    }
}

fn wanted(prop: Prop, side: Hand, orientation: Quat) -> ShownPouch {
    ShownPouch {
        prop,
        pouch: Pose {
            position: Vec3::new(0.2, 1.0, -0.1),
            orientation,
        },
        side,
        weapon: (prop == Prop::Magazine).then_some(13),
    }
}

#[test]
fn all_pouch_props_share_actual_instance_and_mirror_placement_on_both_sides() {
    let frames = frames();
    let config = ModelPlacement {
        offset_m: Vec3::new(0.03, -0.02, 0.04),
        rotation_deg: Vec3::new(10.0, -15.0, 20.0),
    };
    for prop in Prop::ALL {
        for side in Hand::BOTH {
            for turn in [
                Quat::IDENTITY,
                Quat::from_rotation_x(0.7),
                Quat::from_rotation_z(-0.5),
            ] {
                let wanted = wanted(prop, side, turn);
                let mut attachment = placement(prop, Some(&wanted), Some(17), &frames, &config);
                attachment.visible = true;
                let local = attachment.local.unwrap();
                assert_eq!(attachment.model, Some(17));
                assert_eq!(attachment.weapon, wanted.weapon);
                assert_eq!(attachment.side, Some(side));
                let pouch = attachment.pouch.unwrap();
                assert!(
                    frames
                        .model_to_cockpit(pouch.position)
                        .abs_diff_eq(wanted.pouch.position, 1e-5)
                );
                let expected_local =
                    Mat4::from_rotation_translation(pouch.orientation, pouch.position)
                        * on_pouch(&config, side);
                assert!(local.abs_diff_eq(expected_local, 1e-5));
                let sample = Sample {
                    owner: 7,
                    at: Instant::now(),
                    attachments: vec![attachment],
                };
                let platform = Mat4::from_rotation_translation(
                    Quat::from_rotation_y(1.1),
                    Vec3::new(24.0, 6.0, -9.0),
                );
                let actual_instance = render_matrix([platform; 2], [local; 2], 1.0).unwrap();
                let plane = Plane::ahead_of(&platform, 1.5).unwrap();
                let head = platform.transform_point3(Vec3::new(0.0, 1.7, 0.0));
                let mut primitives = Vec::new();
                pouch_markers(&sample, platform, plane, head, &mut primitives);
                assert_eq!(primitives.len(), 3);
                let Primitive::Sprite {
                    at: Anchor::Head(offset),
                    ..
                } = primitives[1]
                else {
                    panic!("the second ring must be the visible model origin");
                };
                assert!(
                    (head + offset)
                        .abs_diff_eq(plane.reflect(actual_instance.w_axis.truncate()), 1e-5)
                );
            }
        }
    }
}

#[test]
fn an_unoffered_pouch_and_a_missing_model_are_distinct_in_the_actual_mirror() {
    let frames = frames();
    let config = ModelPlacement {
        offset_m: Vec3::ZERO,
        rotation_deg: Vec3::ZERO,
    };
    let wanted = wanted(Prop::Grenade, Hand::Left, Quat::IDENTITY);
    let absent = placement(Prop::Grenade, None, None, &frames, &config);
    assert_eq!(absent.pouch, Err(Missing::NotOffered));
    assert_eq!(absent.local, Err(Missing::NotOffered));
    let missing = placement(Prop::Grenade, Some(&wanted), None, &frames, &config);
    assert!(missing.pouch.is_ok());
    assert_eq!(missing.local, Err(Missing::Model));
    let plane = Plane::ahead_of(&Mat4::IDENTITY, 1.0).unwrap();
    for (attachment, expected_count) in [(absent, 0), (missing, 1)] {
        let sample = Sample {
            owner: 7,
            at: Instant::now(),
            attachments: vec![attachment],
        };
        let mut primitives = Vec::new();
        pouch_markers(&sample, Mat4::IDENTITY, plane, Vec3::ZERO, &mut primitives);
        assert_eq!(primitives.len(), expected_count);
    }
}

#[test]
fn invalid_placement_or_expired_owner_never_becomes_a_current_pouch_model() {
    let mut frames = frames();
    let config = ModelPlacement {
        offset_m: Vec3::ZERO,
        rotation_deg: Vec3::ZERO,
    };
    let wanted = wanted(Prop::Charge, Hand::Right, Quat::IDENTITY);
    frames.character = Mat4::ZERO;
    let attachment = placement(Prop::Charge, Some(&wanted), Some(17), &frames, &config);
    assert_eq!(attachment.pouch, Err(Missing::Transform));
    assert_eq!(attachment.local, Err(Missing::Transform));
    let sample = Sample {
        owner: 7,
        at: Instant::now(),
        attachments: vec![attachment],
    };
    assert!(sample.current(7, sample.at));
    assert!(sample.current(7, sample.at + MAX_AGE));
    assert!(!sample.current(8, sample.at));
    assert!(!sample.current(7, sample.at + MAX_AGE + Duration::from_nanos(1)));
    assert!(!sample.current(7, sample.at - Duration::from_millis(1)));
}
