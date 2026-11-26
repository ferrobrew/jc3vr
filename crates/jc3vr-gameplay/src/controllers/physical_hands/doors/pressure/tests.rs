use glam::Quat;
use jc3gi::physics::GameMaterialId;

use super::*;
use crate::controllers::physical_hands::doors::{geometry::DoorBox, table};

fn fixture() -> (Reach, SurfaceHit, DoorRead) {
    let vehicle = NonZeroUsize::new(64).unwrap();
    let target = DoorTarget {
        vehicle,
        vehicle_body: Some(7),
        part_index: 2,
        slot: DoorSlot::FRONT_LEFT,
        latched: true,
        outward: Vec3::X,
    };
    let hit = SurfaceHit {
        position: Vec3::X,
        normal: Vec3::NEG_X,
        surface: Surface::Solid {
            object: Some(vehicle),
            body: 7,
        },
        material: Some(0),
    };
    let reach = Reach {
        object: vehicle,
        body: 7,
        point: Vec3::new(1.0, 0.5, 0.5),
        distance: 0.0,
        normal: Vec3::X,
        kind: TargetKind::Door(target),
    };
    let door = DoorRead {
        slot: target.slot,
        part_index: target.part_index,
        latched: true,
        door_box: DoorBox::new(
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            Vec3::new(0.98, -1.0, -1.0),
            Vec3::new(1.02, 1.0, 1.0),
        ),
        body: None,
        part_hash: None,
    };
    (reach, hit, door)
}

fn sample(palm: Vec3) -> Option<Sample> {
    let (reach, hit, door) = fixture();
    Sample::new(reach, hit, door, Mat4::IDENTITY, palm)
}

fn tick(press: &mut Press, palm: Vec3) -> Option<Opening> {
    press.intent(sample(palm), 0.01, 1.0)
}

#[test]
fn diagnostic_rejections_survive_without_a_reach() {
    let mut press = Press::new();
    let (_, mut hit, door) = fixture();
    hit.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
    press.missing_contact(None, Some(hit));
    assert_eq!(press.diagnostics.readout.reason, Reason::GlassRejected);
    assert_eq!(press.diagnostics.readout.slot, None);
    assert_eq!(panel_rejection(door, hit), Reason::GlassRejected);
    hit.material = None;
    press.missing_contact(None, Some(hit));
    assert_eq!(press.diagnostics.readout.reason, Reason::NoPanel);
    assert_eq!(panel_rejection(door, hit), Reason::NoPanel);
    press.missing_contact(None, None);
    assert_eq!(press.diagnostics.readout.reason, Reason::NoContact);
}

#[test]
fn diagnostic_movement_and_contact_loss_follow_the_latched_contact() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    assert_eq!(
        press.diagnostics.readout.reason,
        Reason::InsufficientMovement
    );
    assert_eq!(press.diagnostics.readout.slot, Some(DoorSlot::FRONT_LEFT));
    tick(&mut press, Vec3::X * 1.02);
    assert!((press.diagnostics.readout.displacement_m - 0.02).abs() < 1e-6);
    press.missing_contact(None, None);
    assert_eq!(press.diagnostics.readout.reason, Reason::ContactLost);
    assert_eq!(press.diagnostics.readout.slot, Some(DoorSlot::FRONT_LEFT));
    press.missing_contact(None, None);
    assert_eq!(press.diagnostics.readout.reason, Reason::NoContact);
    let mut exterior = sample(Vec3::X).unwrap();
    exterior.hit.normal = Vec3::X;
    press.intent(Some(exterior), 0.01, 1.0);
    assert_eq!(press.diagnostics.readout.reason, Reason::ExteriorFace);
}

#[test]
fn an_inside_outward_press_opens_once_at_the_contact_not_the_handle() {
    let mut press = Press::new();
    assert!(tick(&mut press, Vec3::X).is_none());
    let opening = tick(&mut press, Vec3::X * 1.06).unwrap();
    assert_eq!(opening.point, Vec3::X);
    assert_eq!(opening.outward, Vec3::X);
    assert_eq!(opening.speed, MAX_OPEN_MPS);
    assert!(tick(&mut press, Vec3::X * 1.2).is_none());
    assert!(tick(&mut press, Vec3::X).is_none());
    assert!(tick(&mut press, Vec3::X * 1.2).is_none());
}

#[test]
fn inward_and_tangential_displacement_do_not_open() {
    for displacement in [Vec3::NEG_X, Vec3::Y, Vec3::Z] {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        assert!(tick(&mut press, Vec3::X + displacement).is_none());
    }
}

#[test]
fn acquisition_does_not_spend_an_existing_gap_or_penetration() {
    for initial in [Vec3::X * 0.8, Vec3::X * 1.2] {
        let mut press = Press::new();
        assert!(tick(&mut press, initial).is_none());
        assert!(tick(&mut press, initial + Vec3::X * 0.04).is_none());
        assert!(tick(&mut press, initial + Vec3::X * 0.06).is_some());
    }
}

#[test]
fn outside_contact_cannot_arm_a_push() {
    let mut press = Press::new();
    for palm in [Vec3::X, Vec3::X * 1.2] {
        let mut exterior = sample(palm).unwrap();
        exterior.hit.normal = Vec3::X;
        assert!(press.intent(Some(exterior), 0.01, 1.0).is_none());
    }
    assert!(press.contact.is_none());
}

#[test]
fn vehicle_translation_and_rotation_do_not_spend_the_pull() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    for translation in [Vec3::new(50.0, -20.0, 90.0), Vec3::ZERO] {
        let world = Mat4::from_rotation_translation(Quat::from_rotation_y(1.7), translation);
        let mut moving = sample(Vec3::X).unwrap();
        moving.world = world;
        moving.palm = world.transform_point3(Vec3::X);
        moving.hit.position = world.transform_point3(Vec3::X);
        moving.hit.normal = world.transform_vector3(Vec3::NEG_X);
        moving.outward = world.transform_vector3(Vec3::X);
        assert!(press.intent(Some(moving), 0.01, 1.0).is_none());
        moving.palm = world.transform_point3(Vec3::X * 1.06);
        let opening = press.intent(Some(moving), 0.01, 1.0).unwrap();
        assert!(opening.point.distance(world.transform_point3(Vec3::X)) < 1e-4);
        assert!(opening.outward.distance(world.transform_vector3(Vec3::X)) < 1e-4);
        press.reset();
        tick(&mut press, Vec3::X);
    }
}

#[test]
fn contact_loss_and_each_target_identity_change_rebase_the_pull() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    assert!(press.intent(None, 0.01, 1.0).is_none());
    assert!(tick(&mut press, Vec3::X * 1.2).is_none());
    assert!(tick(&mut press, Vec3::X * 1.26).is_some());
    for change in 0..4 {
        press.reset();
        tick(&mut press, Vec3::X);
        let mut swapped = sample(Vec3::X * 1.2).unwrap();
        match change {
            0 => swapped.target.vehicle = NonZeroUsize::new(128).unwrap(),
            1 => swapped.target.body += 1,
            2 => swapped.target.part += 1,
            _ => swapped.target.slot = DoorSlot::FRONT_RIGHT,
        }
        assert!(press.intent(Some(swapped), 0.01, 1.0).is_none());
        swapped.palm += Vec3::X * 0.06;
        assert!(press.intent(Some(swapped), 0.01, 1.0).is_some());
    }
}

#[test]
fn invalid_timing_and_scale_reset_and_never_open() {
    for (dt, scale) in [
        (0.0, 1.0),
        (-0.01, 1.0),
        (0.3, 1.0),
        (f32::NAN, 1.0),
        (f32::INFINITY, 1.0),
        (0.01, 0.0),
        (0.01, -1.0),
        (0.01, f32::NAN),
        (0.01, f32::INFINITY),
    ] {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        assert!(press.intent(sample(Vec3::X * 1.2), dt, scale).is_none());
        assert!(press.contact.is_none());
        assert!(tick(&mut press, Vec3::X * 1.2).is_none());
    }
}

#[test]
fn scale_controls_the_threshold_and_changes_rebase() {
    let mut press = Press::new();
    assert!(press.intent(sample(Vec3::X), 0.01, 2.0).is_none());
    assert!(press.intent(sample(Vec3::X * 1.06), 0.01, 2.0).is_none());
    assert!(press.intent(sample(Vec3::X * 1.11), 0.01, 2.0).is_some());
    assert!(press.intent(sample(Vec3::X * 1.3), 0.01, 1.0).is_none());
}

#[test]
fn only_real_owned_panel_contacts_are_candidates() {
    let (reach, hit, door) = fixture();
    assert!(candidate(Some(reach), None).is_none());
    assert!(candidate(None, Some(hit)).is_none());
    for surface in [
        Surface::Solid {
            object: None,
            body: 7,
        },
        Surface::Solid {
            object: Some(reach.object),
            body: 8,
        },
        Surface::Character {
            object: reach.object,
            body: 7,
        },
    ] {
        assert!(candidate(Some(reach), Some(SurfaceHit { surface, ..hit })).is_none());
    }
    let off_panel = SurfaceHit {
        position: Vec3::ZERO,
        ..hit
    };
    assert!(Sample::new(reach, off_panel, door, Mat4::IDENTITY, Vec3::X).is_none());
    assert!(Sample::new(reach, hit, door, Mat4::ZERO, Vec3::X).is_none());
    assert!(Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::NAN).is_none());
}

#[test]
fn ordinary_glass_is_excluded_and_known_canopy_glass_is_allowed() {
    let (reach, mut hit, mut door) = fixture();
    hit.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
    assert!(Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::X).is_none());
    let entries = table::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/door_handles/door_handles.json"
    )))
    .unwrap();
    let canopy = entries
        .iter()
        .find(|entry| table::is_canopy(Some(entry.part_hash)))
        .expect("the handle table contains a verified canopy");
    door.part_hash = Some(canopy.part_hash);
    let mut press = Press::new();
    assert!(
        press
            .intent(
                Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::X),
                0.01,
                1.0
            )
            .is_none()
    );
    assert!(
        press
            .intent(
                Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::X * 1.06),
                0.01,
                1.0
            )
            .is_some()
    );
}

#[test]
fn open_panels_accept_only_bounded_pressure_into_the_panel() {
    assert_eq!(push_speed(Vec3::NEG_Y, Vec3::Y, 0.01, 1.0), 0.25);
    assert_eq!(push_speed(Vec3::Y, Vec3::Y, 0.01, 1.0), 0.0);
    assert_eq!(push_speed(Vec3::X, Vec3::Y, 0.01, 1.0), 0.0);
    assert_eq!(push_speed(Vec3::NEG_Y, Vec3::Y, 0.0, 1.0), 0.0);
    assert_eq!(push_speed(Vec3::NEG_Y, Vec3::Y, 0.01, f32::NAN), 0.0);
}

#[test]
fn impulses_are_finite_bounded_and_never_target_the_chassis() {
    assert_eq!(opening_impulse(Vec3::X, Some(10.0), 0.5), Vec3::X * 5.0);
    assert_eq!(
        opening_impulse(Vec3::X, Some(f32::MAX), 10.0),
        Vec3::X * MAX_IMPULSE_NS
    );
    for mass in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(f32::NAN),
        Some(f32::INFINITY),
    ] {
        assert_eq!(opening_impulse(Vec3::X, mass, 1.0), Vec3::ZERO);
    }
    for speed in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(opening_impulse(Vec3::X, Some(10.0), speed), Vec3::ZERO);
    }
    let vehicle = NonZeroUsize::new(64).unwrap();
    let owner = NonZeroUsize::new(128).unwrap();
    assert!(door_body(Some((owner, 8)), owner, 8, vehicle, 7));
    assert!(!door_body(Some((owner, 7)), owner, 7, vehicle, 7));
    assert!(!door_body(Some((vehicle, 8)), vehicle, 8, vehicle, 7));
    assert!(!door_body(Some((owner, 9)), owner, 8, vehicle, 7));
    assert!(!door_body(None, owner, 8, vehicle, 7));
}

#[test]
fn contact_feedback_is_once_per_valid_contact_and_not_replayed_after_grace() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    assert!(press.take_contact_cue());
    assert!(!press.take_contact_cue());
    tick(&mut press, Vec3::X * 1.02);
    assert!(!press.take_contact_cue());
    assert!(press.retain_sample(sample(Vec3::X * 1.025), 0.01, 1.0));
    assert!(tick(&mut press, Vec3::X * 1.03).is_none());
    assert!(!press.take_contact_cue());
    press.reset();
    tick(&mut press, Vec3::X);
    assert!(press.take_contact_cue());
    press.reset();
    let mut exterior = sample(Vec3::X).unwrap();
    exterior.hit.normal = Vec3::X;
    press.intent(Some(exterior), 0.01, 1.0);
    assert!(!press.take_contact_cue());
}

#[test]
fn grace_freezes_progress_and_reacquisition_then_requires_current_outward_contact() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    tick(&mut press, Vec3::X * 1.04);
    for _ in 0..4 {
        assert!(press.retain_sample(sample(Vec3::X * 1.055), 0.01, 1.0));
        assert!((press.diagnostics.readout.displacement_m - 0.04).abs() < 1e-6);
        assert!(!press.contact.unwrap().fired);
    }
    assert!(tick(&mut press, Vec3::X * 1.06).is_none());
    assert!((press.diagnostics.readout.displacement_m - 0.04).abs() < 1e-6);
    assert!(tick(&mut press, Vec3::X * 1.069).is_none());
    assert!(tick(&mut press, Vec3::X * 1.071).is_some());
}

#[test]
fn grace_is_bounded_and_cannot_be_extended_by_missing_ticks() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    assert!(press.retain_sample(sample(Vec3::X), 0.04, 1.0));
    assert!(press.retain_sample(sample(Vec3::X), 0.04, 1.0));
    assert!(!press.retain_sample(sample(Vec3::X), 0.001, 1.0));
    assert!(tick(&mut press, Vec3::X).is_none());
    assert!(press.contact.is_none());
    tick(&mut press, Vec3::X);
    assert!(press.retain_sample(sample(Vec3::X), 0.07, 1.0));
    assert!(press.intent(sample(Vec3::X), 0.02, 1.0).is_none());
    assert!(press.contact.is_none());
}

#[test]
fn grace_rejects_missing_proof_glass_obstructions_and_changed_identity() {
    for change in 0..8 {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        let mut proof = sample(Vec3::X).unwrap();
        let proof = match change {
            0 => None,
            1 => {
                proof.hit.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
                Some(proof)
            }
            2 => {
                proof.hit.material = None;
                Some(proof)
            }
            3 => {
                proof.target.vehicle = NonZeroUsize::new(128).unwrap();
                Some(proof)
            }
            4 => {
                proof.target.body += 1;
                Some(proof)
            }
            5 => {
                proof.target.part += 1;
                Some(proof)
            }
            6 => {
                proof.target.slot = DoorSlot::FRONT_RIGHT;
                Some(proof)
            }
            _ => {
                proof.hit.normal = Vec3::X;
                Some(proof)
            }
        };
        assert!(!press.retain_sample(proof, 0.01, 1.0));
        press.missing_contact(None, None);
        assert!(!press.engaged());
    }
    for glass in [false, true] {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        let (_, mut obstruction, _) = fixture();
        if glass {
            obstruction.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
        }
        press.missing_contact(None, Some(obstruction));
        assert!(!press.engaged());
    }
}

#[test]
fn grace_drift_is_bounded_from_last_contact_not_last_missing_tick() {
    for displacement in [
        Vec3::X * 0.026,
        Vec3::Y * 0.026,
        Vec3::NEG_X * 0.006,
        Vec3::NAN,
    ] {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        assert!(!press.retain_sample(sample(Vec3::X + displacement), 0.01, 1.0));
    }
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    assert!(press.retain_sample(sample(Vec3::X + Vec3::Y * 0.02), 0.01, 1.0));
    assert!(!press.retain_sample(sample(Vec3::X + Vec3::Y * 0.03), 0.01, 1.0));
    assert!(press.retain_sample(sample(Vec3::X), 0.01, 1.0));
    assert!(tick(&mut press, Vec3::X + Vec3::Y * 0.03).is_none());
    assert!(!press.engaged());
}

#[test]
fn grace_revalidates_timing_scale_frame_and_closed_panel() {
    for (dt, scale) in [
        (0.0, 1.0),
        (0.3, 1.0),
        (f32::NAN, 1.0),
        (0.01, 2.0),
        (0.01, f32::NAN),
    ] {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        assert!(!press.retain_sample(sample(Vec3::X), dt, scale));
    }
    let (reach, hit, door) = fixture();
    for world in [Mat4::ZERO, Mat4::NAN, Mat4::from_scale(Vec3::splat(2.0))] {
        assert!(Sample::new(reach, hit, door, world, Vec3::X).is_none());
    }
    for current in [
        DoorRead {
            latched: false,
            ..door
        },
        DoorRead {
            body: Some((reach.object, 9)),
            ..door
        },
    ] {
        assert!(Sample::new(reach, hit, current, Mat4::IDENTITY, Vec3::X).is_none());
    }
    let mut reowned = hit;
    reowned.surface = Surface::Solid {
        object: Some(NonZeroUsize::new(128).unwrap()),
        body: reach.body,
    };
    assert!(Sample::new(reach, reowned, door, Mat4::IDENTITY, Vec3::X).is_none());
}

#[test]
fn grace_uses_current_vehicle_frame_without_spending_motion() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    tick(&mut press, Vec3::X * 1.04);
    let world =
        Mat4::from_rotation_translation(Quat::from_rotation_y(1.7), Vec3::new(50.0, -20.0, 90.0));
    let mut proof = sample(Vec3::X * 1.05).unwrap();
    proof.world = world;
    proof.palm = world.transform_point3(proof.palm);
    proof.hit.position = world.transform_point3(proof.hit.position);
    proof.hit.normal = world.transform_vector3(proof.hit.normal);
    proof.outward = world.transform_vector3(proof.outward);
    assert!(press.retain_sample(Some(proof), 0.01, 1.0));
    assert!(press.intent(Some(proof), 0.01, 1.0).is_none());
    assert!((press.diagnostics.readout.displacement_m - 0.04).abs() < 1e-5);
    proof.palm = world.transform_point3(Vec3::X * 1.065);
    assert!(press.intent(Some(proof), 0.01, 1.0).is_some());
}

#[test]
fn unlatch_threshold_remains_five_centimetres() {
    let mut press = Press::new();
    tick(&mut press, Vec3::ZERO);
    assert!(tick(&mut press, Vec3::X * 0.049).is_none());
    assert!(tick(&mut press, Vec3::X * UNLATCH_PULL_M).is_some());
}

#[test]
fn withdrawal_resets_progress_even_with_current_panel_contact() {
    let mut press = Press::new();
    tick(&mut press, Vec3::X);
    tick(&mut press, Vec3::X * 1.04);
    assert!(tick(&mut press, Vec3::X * 1.03).is_none());
    assert!(!press.engaged());
    assert!(tick(&mut press, Vec3::X * 1.06).is_none());
    assert!(press.take_contact_cue());
}

#[test]
fn hidden_canopy_panes_cannot_arm_or_bridge_pressure() {
    let (reach, mut hit, mut door) = fixture();
    hit.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
    let entries = table::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/door_handles/door_handles.json"
    )))
    .unwrap();
    door.part_hash = entries
        .iter()
        .find(|entry| table::is_canopy(Some(entry.part_hash)))
        .map(|entry| entry.part_hash);
    assert!(Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::X).is_some());
    assert!(matches!(classify_probe(hit, || true), Probe::Hidden));
    assert!(matches!(classify_probe(hit, || false), Probe::Visible(_)));
    for already_armed in [false, true] {
        let mut press = Press::new();
        if already_armed {
            tick(&mut press, Vec3::X);
            tick(&mut press, Vec3::X * 1.04);
            assert!(press.retain_sample(sample(Vec3::X * 1.04), 0.01, 1.0));
        }
        let pressure_hit = press.observe_probe(classify_probe(hit, || true));
        assert!(pressure_hit.is_none());
        assert!(!press.engaged());
        assert!(!press.take_contact_cue());
        assert_eq!(press.diagnostics.readout.reason, Reason::GlassRejected);
        let mut moved = hit;
        moved.position += Vec3::X * 0.06;
        assert!(
            press
                .observe_probe(classify_probe(moved, || true))
                .is_none()
        );
        assert!(!press.engaged());
    }
}

#[test]
fn hidden_provenance_wins_over_visible_hits_and_misses_across_palm_probes() {
    let (_, hit, _) = fixture();
    for probes in [
        [Probe::Hidden, Probe::Miss, Probe::Visible(hit)],
        [Probe::Visible(hit), Probe::Hidden, Probe::Miss],
        [Probe::Miss, Probe::Visible(hit), Probe::Hidden],
    ] {
        let result = probes.into_iter().fold(Probe::Miss, |previous, next| {
            prefer_probe(previous, next, Vec3::X)
        });
        assert!(matches!(result, Probe::Hidden));
    }
    let (_, mut glass, _) = fixture();
    glass.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
    assert!(matches!(
        prefer_probe(Probe::Visible(glass), Probe::Visible(hit), Vec3::X),
        Probe::Visible(chosen) if glass_hit(chosen)
    ));
}
