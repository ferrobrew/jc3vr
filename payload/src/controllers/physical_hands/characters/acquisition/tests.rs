use jc3gi::vehicle::vehicle::DoorSlot;

use super::*;

#[test]
fn a_missing_or_replaced_body_owner_prevents_every_character_read() {
    let target = target(None);
    for owner in [None, Some(address(96))] {
        let mut engine = engine(Attachment::OnFoot);
        engine.owner = owner;
        assert!(acquire(target, &mut engine).is_none());
        assert_eq!(engine.calls, [Call::Owner]);
    }
}

#[test]
fn a_changed_or_missing_attachment_refuses_before_any_saved_vehicle_or_frame_read() {
    let captured = seated(64);
    for attachment in [
        None,
        Some(Attachment::OnFoot),
        Some(Attachment::Vehicle(address(96))),
    ] {
        let mut engine = engine(Attachment::OnFoot);
        engine.attachment = attachment;
        assert!(acquire(target(Some(captured)), &mut engine).is_none());
        assert_eq!(engine.calls, [Call::Owner, Call::Attachment]);
    }
    let mut engine = engine(Attachment::Vehicle(address(64)));
    assert!(acquire(target(None), &mut engine).is_none());
    assert_eq!(engine.calls, [Call::Owner, Call::Attachment]);
}

#[test]
fn a_seated_grab_requires_a_fresh_open_door_before_reading_the_frame() {
    let mut engine = engine(Attachment::Vehicle(address(64)));
    engine.door = None;
    assert!(acquire(target(Some(seated(64))), &mut engine).is_none());
    assert_eq!(
        engine.calls,
        [Call::Owner, Call::Attachment, Call::Door(address(64))]
    );
}

#[test]
fn a_valid_seated_grab_refreshes_the_door_and_preserves_hostility_policy() {
    for hostile in [false, true] {
        let mut target = target(Some(seated(64)));
        target.hostile = hostile;
        let mut engine = engine(Attachment::Vehicle(address(64)));
        let current = Seated {
            door: DoorSlot::REAR_LEFT,
            open_deg: 75.0,
            ..seated(64)
        };
        engine.door = Some(current);
        let seize = acquire(target, &mut engine).unwrap();
        assert_eq!(seize.target.seated, Some(current));
        assert_eq!(seize.target.hostile, hostile);
        assert_eq!(seize.grab, Vec3::X);
        assert_eq!(
            engine.calls,
            [
                Call::Owner,
                Call::Attachment,
                Call::Door(address(64)),
                Call::Frame
            ]
        );
    }
}

#[test]
fn a_valid_on_foot_grab_reads_the_frame_only_after_owner_and_attachment_checks() {
    for hostile in [false, true] {
        let mut target = target(None);
        target.hostile = hostile;
        let mut engine = engine(Attachment::OnFoot);
        let seize = acquire(target, &mut engine).unwrap();
        assert_eq!(seize.target, target);
        assert_eq!(seize.grab, Vec3::X);
        assert_eq!(engine.calls, [Call::Owner, Call::Attachment, Call::Frame]);
    }
}

#[test]
fn a_non_finite_frame_refuses_the_checked_acquisition() {
    let mut engine = engine(Attachment::OnFoot);
    engine.frame = Mat4::NAN;
    assert!(acquire(target(None), &mut engine).is_none());
    assert_eq!(engine.calls, [Call::Owner, Call::Attachment, Call::Frame]);
}

#[test]
fn disabled_current_capabilities_do_not_reuse_the_queued_offer() {
    let config = CharactersConfig {
        grab: false,
        hijack: false,
        ..CharactersConfig::default()
    };
    for (target, attachment) in [
        (target(None), Attachment::OnFoot),
        (target(Some(seated(64))), Attachment::Vehicle(address(64))),
    ] {
        let mut engine = engine(attachment);
        assert!(take_with(target, Vec3::X, conditions(&config), &mut engine).is_none());
        assert_eq!(engine.calls, [Call::Owner, Call::Attachment]);
    }
}

#[test]
fn refreshing_a_held_seated_grip_uses_the_same_owner_and_attachment_checks_without_a_frame() {
    let config = CharactersConfig::default();
    let mut engine = engine(Attachment::Vehicle(address(64)));
    assert!(refresh(target(Some(seated(64))), conditions(&config), &mut engine).is_some());
    assert_eq!(
        engine.calls,
        [Call::Owner, Call::Attachment, Call::Door(address(64))]
    );
    engine.calls.clear();
    engine.attachment = Some(Attachment::Vehicle(address(96)));
    assert!(refresh(target(Some(seated(64))), conditions(&config), &mut engine).is_none());
    assert_eq!(engine.calls, [Call::Owner, Call::Attachment]);
}

fn acquire(target: CharacterTarget, engine: &mut Engine) -> Option<Seize> {
    let config = CharactersConfig::default();
    take_with(target, Vec3::X, conditions(&config), engine)
}

fn conditions(config: &CharactersConfig) -> Conditions<'_> {
    Conditions {
        palm: Vec3::Y,
        world_scale: 2.0,
        config,
    }
}

fn address(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

fn target(seated: Option<Seated>) -> CharacterTarget {
    CharacterTarget {
        character: address(32),
        body: 7,
        seated,
        ragdoll: false,
        hostile: true,
    }
}

fn seated(vehicle: usize) -> Seated {
    Seated {
        vehicle: address(vehicle),
        door: DoorSlot::FRONT_LEFT,
        open_deg: 40.0,
    }
}

fn engine(attachment: Attachment) -> Engine {
    Engine {
        owner: Some(address(32)),
        attachment: Some(attachment),
        door: Some(seated(64)),
        frame: Mat4::IDENTITY,
        calls: Vec::new(),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Owner,
    Attachment,
    Door(NonZeroUsize),
    Frame,
}

struct Engine {
    owner: Option<NonZeroUsize>,
    attachment: Option<Attachment>,
    door: Option<Seated>,
    frame: Mat4,
    calls: Vec<Call>,
}

impl AcquisitionEngine for Engine {
    fn owner(&mut self, body: u32) -> Option<NonZeroUsize> {
        assert_eq!(body, 7);
        self.calls.push(Call::Owner);
        self.owner
    }

    fn attachment(&mut self, character: NonZeroUsize) -> Option<Attachment> {
        assert_eq!(self.owner, Some(character));
        assert_eq!(self.calls.last(), Some(&Call::Owner));
        self.calls.push(Call::Attachment);
        self.attachment
    }

    fn open_door(&mut self, vehicle: NonZeroUsize, conditions: Conditions<'_>) -> Option<Seated> {
        assert_eq!(self.attachment, Some(Attachment::Vehicle(vehicle)));
        assert_eq!(self.calls.last(), Some(&Call::Attachment));
        assert_eq!(conditions.palm, Vec3::Y);
        assert_eq!(conditions.world_scale, 2.0);
        self.calls.push(Call::Door(vehicle));
        self.door
    }

    fn frame(&mut self, character: NonZeroUsize) -> Mat4 {
        assert_eq!(self.owner, Some(character));
        assert!(matches!(
            self.calls.last(),
            Some(Call::Attachment | Call::Door(_))
        ));
        self.calls.push(Call::Frame);
        self.frame
    }
}
