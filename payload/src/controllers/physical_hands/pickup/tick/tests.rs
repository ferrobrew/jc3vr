use std::num::NonZeroUsize;

use glam::Quat;

use super::*;
use crate::controllers::{
    frame::Pose,
    physical_hands::{
        characters::CharacterTarget, doors::config::TEARING, pickup::PICKUP, rigid::BodyPose,
    },
};

struct FakeBodies {
    body: bodies::BodyState,
    calls: Vec<&'static str>,
    driven: Option<Velocities>,
}

impl BodyEngine for FakeBodies {
    fn read(&mut self, _: u32, _: NonZeroUsize) -> Result<bodies::BodyState, bodies::BodyError> {
        self.calls.push("read");
        Ok(self.body)
    }
    fn release_joints(&mut self, _: &Held) {
        self.calls.push("unjoin");
    }
    fn drive(&mut self, _: &Held, velocities: Velocities) -> Result<(), bodies::BodyError> {
        assert_eq!(self.calls.last(), Some(&"unjoin"));
        self.calls.push("drive");
        self.driven = Some(velocities);
        Ok(())
    }
}

fn body() -> bodies::BodyState {
    bodies::BodyState {
        pose: BodyPose {
            centre: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        },
        velocities: Velocities {
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
        },
        mass_kg: Some(5.0),
        inverse_inertia: Vec3::ONE,
    }
}

fn palm() -> PalmMotion {
    PalmMotion {
        pose: Pose {
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        },
        velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
    }
}

fn held(joined: bool, kind: HeldKind) -> Held {
    Held {
        object: NonZeroUsize::new(100).unwrap(),
        body: 7,
        grip: Grip::of(palm().pose, body().pose, Vec3::ZERO),
        mass_kg: 5.0,
        joined,
        kind,
    }
}

fn inputs(enabled: bool) -> TickInputs {
    TickInputs {
        palms: [Some(palm()); 2],
        space: None,
        joints: JointInputs {
            enabled,
            join: [enabled; 2],
            ..Default::default()
        },
        dt: 1.0 / 30.0,
        time_scale: 1.0,
        unlatch_pull_m: 0.1,
        tearing: TEARING,
        world_scale: 1.0,
        characters: CharactersConfig::default(),
        reaches: [None; 2],
        carrier_velocity: None,
    }
}

fn run(state: &mut PickupState, enabled: bool, fake: &mut FakeBodies) -> TickOutcome {
    state.input_at = Some(Instant::now());
    unsafe { tick_with(state, inputs(enabled), &PICKUP, fake) }
}

#[test]
fn a_second_holder_inherits_carry_or_a_preserved_joint_in_the_production_tick() {
    for joined in [false, true] {
        let mut state = PickupState::new();
        state.held[0] = Some(held(joined, HeldKind::Prop));
        state.requests[1] = Some(Request::Grab(Reach {
            object: held(joined, HeldKind::Prop).object,
            body: 7,
            point: Vec3::ZERO,
            distance: 0.0,
            normal: Vec3::Y,
            kind: TargetKind::Prop { mass_kg: 5.0 },
        }));
        let mut fake = FakeBodies {
            body: body(),
            calls: Vec::new(),
            driven: None,
        };
        let outcome = run(&mut state, true, &mut fake);
        assert_eq!(state.held.map(|h| h.unwrap().joined), [joined; 2]);
        assert_eq!(outcome.joints.map(|j| j.is_some()), [joined; 2]);
        assert!(
            outcome
                .reports
                .iter()
                .flatten()
                .all(|r| r.shared && r.joined == joined)
        );
        assert_eq!(
            fake.calls.iter().filter(|&&c| c == "drive").count(),
            usize::from(!joined)
        );
    }
}

#[test]
fn latch_handoff_tear_and_regrab_keep_one_body_mode_in_the_production_tick() {
    for initially_joined in [false, true] {
        let mut state = PickupState::new();
        let original = held(initially_joined, HeldKind::Door);
        state.held = [Some(original); 2];
        if !initially_joined {
            state.carry_body(original.object, original.body);
        }
        tearing::apply(
            &mut state,
            &original,
            doors::tearing::TearResult::Detached,
            &mut TickOutcome::default(),
        );
        state.held[1] = None;
        state.requests[1] = Some(Request::Grab(Reach {
            object: original.object,
            body: original.body,
            point: Vec3::ZERO,
            distance: 0.0,
            normal: Vec3::Y,
            kind: TargetKind::Prop { mass_kg: 5.0 },
        }));
        let mut fake = FakeBodies {
            body: body(),
            calls: Vec::new(),
            driven: None,
        };
        let outcome = run(&mut state, true, &mut fake);
        assert_eq!(state.held.map(|h| h.unwrap().joined), [initially_joined; 2]);
        assert_eq!(outcome.joints.map(|j| j.is_some()), [initially_joined; 2]);
        assert_eq!(
            fake.calls.iter().filter(|&&c| c == "drive").count(),
            usize::from(!initially_joined)
        );
    }
    let mut state = PickupState::new();
    state.held = [
        Some(held(true, HeldKind::Prop)),
        Some(held(false, HeldKind::Prop)),
    ];
    let mut fake = FakeBodies {
        body: body(),
        calls: Vec::new(),
        driven: None,
    };
    let outcome = run(&mut state, true, &mut fake);
    assert_eq!(state.held.map(|h| h.unwrap().joined), [false; 2]);
    assert_eq!(outcome.joints, [None; 2]);
    assert_eq!(fake.calls.iter().filter(|&&c| c == "drive").count(), 1);
    assert!(outcome.reports.iter().flatten().all(|r| r.shared));
}

#[test]
fn a_joint_feature_toggle_releases_the_whole_group_without_a_carry_command() {
    for joined in [false, true] {
        let mut state = PickupState::new();
        state.held = [Some(held(joined, HeldKind::Prop)); 2];
        state.joints_enabled = Some(true);
        let mut fake = FakeBodies {
            body: body(),
            calls: Vec::new(),
            driven: None,
        };
        let outcome = run(&mut state, false, &mut fake);
        assert_eq!(state.held, [None; 2]);
        assert_eq!(outcome.released, [true; 2]);
        assert_eq!(outcome.joints, [None; 2]);
        assert!(fake.calls.is_empty());
        assert_eq!(outcome.throw, None);
    }
}

#[test]
fn a_carried_object_the_hands_can_lift_takes_on_the_characters_jump() {
    // The character leaves the ground at 7 m/s, and the palms rise with it.
    let jump = Vec3::new(0.0, 7.0, 0.0);
    let rising_palm = PalmMotion {
        velocity: jump,
        ..palm()
    };
    let driven_rise = |mass_kg: f32, share: f32| {
        let mut state = PickupState::new();
        state.held = [Some(Held {
            mass_kg,
            ..held(false, HeldKind::Prop)
        }); 2];
        let mut fake = FakeBodies {
            body: bodies::BodyState {
                mass_kg: Some(mass_kg),
                ..body()
            },
            calls: Vec::new(),
            driven: None,
        };
        let config = PickupTuning {
            carrier_motion_share: share,
            ..PICKUP
        };
        for carrier in [Vec3::ZERO, jump] {
            state.input_at = Some(Instant::now());
            let inputs = TickInputs {
                palms: [Some(rising_palm); 2],
                carrier_velocity: Some(carrier),
                ..inputs(false)
            };
            let outcome = unsafe { tick_with(&mut state, inputs, &config, &mut fake) };
            assert_eq!(state.held.map(|h| h.is_some()), [true; 2], "{outcome:?}");
        }
        fake.driven.unwrap().linear.y
    };
    // A tyre's weight is within the two hands' lift, so it rises with the character.
    assert!((driven_rise(30.0, 1.0) - jump.y).abs() < 0.5);
    // Without the carrier's motion the hands' strength alone gets it well short of the jump.
    assert!(driven_rise(30.0, 0.0) < jump.y * 0.5);
    // What the hands cannot lift is not carried along: it stays a snag.
    assert!(driven_rise(100.0, 1.0) < jump.y * 0.5);
}

#[test]
fn a_ragdolls_body_is_held_as_the_whole_ragdolls_weight() {
    let target = |ragdoll: bool, mass_kg: f32| {
        TargetKind::Character(CharacterTarget {
            character: NonZeroUsize::new(32).unwrap(),
            body: 7,
            seated: None,
            ragdoll,
            hostile: false,
            mass_kg,
        })
    };
    assert_eq!(held_mass(&target(true, 72.0), 6.0), 72.0);
    // A ragdoll whose total could not be read is never lighter than the body held.
    assert_eq!(held_mass(&target(true, 0.0), 6.0), 6.0);
    assert_eq!(held_mass(&target(true, f32::NAN), 6.0), 6.0);
    assert_eq!(held_mass(&target(false, 72.0), 6.0), 6.0);
    assert_eq!(held_mass(&TargetKind::Prop { mass_kg: 6.0 }, 6.0), 6.0);
}
