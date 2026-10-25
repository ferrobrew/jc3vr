use std::time::Duration;

use glam::{Quat, Vec3};

use super::*;
use crate::controllers::{
    body::mirror::{PouchRenderContext, PouchRenderStatus, pouch_render_markers},
    overlay::{
        primitive::{Anchor, Primitive, Source},
        store::Store,
    },
    weapons::pouch_models::{render_character, render_matrix},
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

fn id(sequence: u64) -> SampleId {
    SampleId {
        owner: 7,
        epoch: 3,
        sequence,
    }
}

fn sample(attachment: Attachment) -> Sample {
    Sample {
        id: id(11),
        at: Instant::now(),
        character: frames().character,
        attachments: vec![attachment],
    }
}

fn sprite_position(primitive: Primitive) -> Vec3 {
    let Primitive::Sprite {
        at: Anchor::World(position),
        ..
    } = primitive
    else {
        panic!("expected a world-anchored sprite, got {primitive:?}");
    };
    position
}

#[test]
fn all_props_compare_the_applied_render_matrix_with_the_unchanged_input_target() {
    let frames = frames();
    let config = ModelPlacement {
        offset_m: Vec3::new(0.03, -0.02, 0.04),
        rotation_deg: Vec3::new(10.0, -15.0, 20.0),
    };
    let character = [
        Mat4::from_rotation_translation(Quat::from_rotation_y(-0.4), Vec3::new(20.0, 4.0, -8.0)),
        Mat4::from_rotation_translation(
            Quat::from_rotation_y(1.1) * Quat::from_rotation_z(0.3),
            Vec3::new(24.0, 6.0, -9.0),
        ),
    ];
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
                let pouch = attachment.pouch.unwrap();
                assert_eq!(attachment.model, Some(17));
                assert_eq!(attachment.weapon, wanted.weapon);
                assert_eq!(attachment.side, Some(side));
                assert!(
                    frames
                        .model_to_cockpit(pouch.position)
                        .abs_diff_eq(wanted.pouch.position, 1e-5)
                );
                let expected_local =
                    Mat4::from_rotation_translation(pouch.orientation, pouch.position)
                        * on_pouch(&config, side);
                assert!(local.abs_diff_eq(expected_local, 1e-5));
                let input = sample(attachment);
                let local = [
                    Mat4::from_rotation_translation(
                        Quat::from_rotation_z(0.6),
                        Vec3::new(-0.4, 0.7, 0.2),
                    ),
                    local,
                ];
                for t in [0.0, 0.5, 1.0] {
                    let applied = render_matrix(character, local, t).unwrap();
                    let world = render_character(character, t).unwrap();
                    let mut render = RenderSample::new(id(19), &input, world, input.at).unwrap();
                    render.applied(prop, 17, applied);
                    assert_eq!(render.models[prop.index()].unwrap().matrix, applied);
                    let context = PouchRenderContext::new(&render, 1.5).unwrap();
                    let mut primitives = Vec::new();
                    let status = pouch_render_markers(&render, context, &mut primitives);
                    assert_eq!(status.render, Some(id(19)));
                    assert_eq!(status.input, Some(id(11)));
                    assert_eq!(
                        (status.targets, status.displayed, status.missing),
                        (1, 1, 0)
                    );
                    assert_eq!(primitives.len(), 4);
                    let target = input.character.transform_point3(pouch.position);
                    assert!(
                        sprite_position(primitives[0])
                            .abs_diff_eq(context.plane.reflect(target), 1e-5)
                    );
                    assert!(
                        sprite_position(primitives[1])
                            .abs_diff_eq(context.plane.reflect(applied.w_axis.truncate()), 1e-5,)
                    );
                    let Primitive::Line {
                        from: Anchor::World(from),
                        to: Anchor::World(to),
                        ..
                    } = primitives[2]
                    else {
                        panic!("expected the applied model orientation line");
                    };
                    assert!(
                        from.abs_diff_eq(context.plane.reflect(applied.w_axis.truncate()), 1e-5)
                    );
                    assert!(
                        to.abs_diff_eq(
                            context
                                .plane
                                .reflect(applied.transform_point3(Vec3::NEG_Z * 0.08)),
                            1e-5,
                        )
                    );
                }
            }
        }
    }
}

#[test]
fn missing_hidden_unapplied_and_replaced_models_do_not_claim_display_parity() {
    let config = ModelPlacement {
        offset_m: Vec3::ZERO,
        rotation_deg: Vec3::ZERO,
    };
    let wanted = wanted(Prop::Grenade, Hand::Left, Quat::IDENTITY);
    let absent = placement(Prop::Grenade, None, None, &frames(), &config);
    assert_eq!(absent.pouch, Err(Missing::NotOffered));
    let missing = placement(Prop::Grenade, Some(&wanted), None, &frames(), &config);
    assert_eq!(missing.local, Err(Missing::Model));
    let hidden = placement(Prop::Grenade, Some(&wanted), Some(17), &frames(), &config);
    let mut visible = hidden;
    visible.visible = true;
    for (attachment, model, expected_targets, expected_missing) in [
        (absent, None, 0, 0),
        (missing, None, 1, 1),
        (hidden, Some(17), 1, 1),
        (visible, None, 1, 1),
        (visible, Some(23), 1, 1),
    ] {
        let input = sample(attachment);
        let mut render = RenderSample::new(id(19), &input, Mat4::IDENTITY, input.at).unwrap();
        if let Some(model) = model {
            render.applied(Prop::Grenade, model, Mat4::IDENTITY);
        }
        let context = PouchRenderContext::new(&render, 1.5).unwrap();
        let mut primitives = Vec::new();
        let status = pouch_render_markers(&render, context, &mut primitives);
        assert_eq!(status.targets, expected_targets);
        assert_eq!(status.missing, expected_missing);
        assert_eq!(status.displayed, 0);
        assert_eq!(primitives.len(), expected_targets);
    }
}

#[test]
fn foreign_expired_invalid_or_different_phase_samples_are_rejected() {
    let config = ModelPlacement {
        offset_m: Vec3::ZERO,
        rotation_deg: Vec3::ZERO,
    };
    let wanted = wanted(Prop::Charge, Hand::Right, Quat::IDENTITY);
    let mut invalid_frames = frames();
    invalid_frames.character = Mat4::ZERO;
    let invalid = placement(
        Prop::Charge,
        Some(&wanted),
        Some(17),
        &invalid_frames,
        &config,
    );
    assert_eq!(invalid.pouch, Err(Missing::Transform));
    assert_eq!(invalid.local, Err(Missing::Transform));
    let input = sample(placement(
        Prop::Charge,
        Some(&wanted),
        Some(17),
        &frames(),
        &config,
    ));
    assert!(input.current(7, input.at));
    assert!(input.current(7, input.at + MAX_AGE));
    assert!(!input.current(8, input.at));
    assert!(!input.current(7, input.at + MAX_AGE + Duration::from_nanos(1)));
    assert!(!input.current(7, input.at - Duration::from_millis(1)));
    let mut foreign = id(19);
    foreign.owner += 1;
    assert!(RenderSample::new(foreign, &input, Mat4::IDENTITY, input.at).is_none());
    let mut old_epoch = id(19);
    old_epoch.epoch -= 1;
    assert!(RenderSample::new(old_epoch, &input, Mat4::IDENTITY, input.at).is_none());
    assert!(RenderSample::new(id(19), &input, Mat4::ZERO, input.at).is_none());
    assert!(
        RenderSample::new(
            id(19),
            &input,
            Mat4::IDENTITY,
            input.at + MAX_AGE + Duration::from_nanos(1)
        )
        .is_none()
    );
    let render = RenderSample::new(id(19), &input, Mat4::IDENTITY, input.at).unwrap();
    let mut context = PouchRenderContext::new(&render, 1.5).unwrap();
    context.id.sequence -= 1;
    let mut primitives = Vec::new();
    assert_eq!(
        pouch_render_markers(&render, context, &mut primitives),
        PouchRenderStatus::default()
    );
    assert!(primitives.is_empty());
    assert!(PouchRenderContext::new(&render, f32::NAN).is_none());
}

#[test]
fn clearing_or_replacing_the_render_source_preserves_the_skeleton_source() {
    let now = Instant::now();
    let mut store = Store::new();
    let config = ModelPlacement {
        offset_m: Vec3::ZERO,
        rotation_deg: Vec3::ZERO,
    };
    let wanted = wanted(Prop::Charge, Hand::Right, Quat::IDENTITY);
    let input = sample(placement(
        Prop::Charge,
        Some(&wanted),
        None,
        &frames(),
        &config,
    ));
    let render = RenderSample::new(id(19), &input, Mat4::IDENTITY, input.at).unwrap();
    let context = PouchRenderContext::new(&render, 1.5).unwrap();
    let mut pouches = Vec::new();
    pouch_render_markers(&render, context, &mut pouches);
    let skeleton = pouches[0];
    store.submit(Source::BodyMirror, &mut vec![skeleton], MAX_AGE, now);
    store.submit(Source::BodyMirrorPouches, &mut pouches, MAX_AGE, now);
    let mut out = Vec::new();
    let collection = store.collect(now, &mut out);
    assert_eq!(collection.retained[Source::BodyMirror.index()], 1);
    assert_eq!(collection.retained[Source::BodyMirrorPouches.index()], 1);
    store.clear(Source::BodyMirrorPouches);
    let cleared = store.collect(now, &mut out);
    assert_eq!(cleared.retained[Source::BodyMirrorPouches.index()], 0);
    assert_eq!(out, vec![skeleton]);
    store.submit(Source::BodyMirrorPouches, &mut Vec::new(), MAX_AGE, now);
    assert_eq!(
        store.collect(now, &mut out).retained[Source::BodyMirror.index()],
        1
    );
    assert_eq!(out, vec![skeleton]);
    for (index, source) in Source::ALL.into_iter().enumerate() {
        assert_eq!(source.index(), index);
    }
}
