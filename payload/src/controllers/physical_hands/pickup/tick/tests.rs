use std::num::NonZeroUsize;

use glam::Quat;

use super::*;
use crate::controllers::{
    frame::Pose,
    physical_hands::{
        characters::CharacterTarget, doors::config::TEARING, grasp::GripTarget, pickup::PICKUP,
        rigid::BodyPose,
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

/// The ticks a one-handed hold of `held` lasts, up to `limit`, with the hand `lag` from the grab
/// point and the body moving at `velocity`.
fn ticks_held(held: Held, lag: Vec3, velocity: Vec3, limit: usize) -> (usize, TickOutcome) {
    let mut state = PickupState::new();
    state.held[0] = Some(held);
    let mut fake = FakeBodies {
        body: bodies::BodyState {
            velocities: Velocities {
                linear: velocity,
                angular: Vec3::ZERO,
            },
            mass_kg: Some(held.mass_kg),
            ..body()
        },
        calls: Vec::new(),
        driven: None,
    };
    let moved = PalmMotion {
        pose: Pose {
            position: lag,
            ..palm().pose
        },
        ..palm()
    };
    let mut last = TickOutcome::default();
    for tick in 0..limit {
        state.input_at = Some(Instant::now());
        let inputs = TickInputs {
            palms: [Some(moved), None],
            ..inputs(held.joined)
        };
        last = unsafe { tick_with(&mut state, inputs, &PICKUP, &mut fake) };
        if state.held[0].is_none() {
            return (tick, last);
        }
    }
    (limit, last)
}

#[test]
fn a_heavy_object_dragging_behind_the_hand_stays_held_carried_or_joined() {
    // Too heavy for one hand to lift, past the break distance, and coming after the hand.
    let lag = Vec3::new(0.0, 0.0, -(PICKUP.break_distance_m + 0.2));
    for (joined, kind) in [(false, HeldKind::Prop), (true, HeldKind::Articulated)] {
        let heavy = Held {
            mass_kg: 75.0,
            ..held(joined, kind)
        };
        let (ticks, outcome) = ticks_held(heavy, lag, lag.normalize() * 0.6, 90);
        assert_eq!(ticks, 90, "joined {joined}: {outcome:?}");
        let report = outcome.reports[0].unwrap();
        assert!(!report.liftable && report.joined == joined);
    }
}

#[test]
fn a_caught_hold_is_torn_away_the_same_way_carried_or_joined() {
    let lag = Vec3::new(PICKUP.break_distance_m + 0.1, 0.0, 0.0);
    let expected = (PICKUP.stuck_s / inputs(false).dt).ceil() as usize;
    for (joined, kind) in [(false, HeldKind::Prop), (true, HeldKind::Articulated)] {
        let (ticks, outcome) = ticks_held(held(joined, kind), lag, Vec3::ZERO, 90);
        assert!(
            ticks.abs_diff(expected) <= 1,
            "joined {joined}: {ticks} vs {expected}"
        );
        assert_eq!(outcome.losses[0], Some(HoldLoss::TornAway));
        assert_eq!(outcome.joints, [None; 2]);
    }
    // Past the tear distance it goes at once.
    let far = Vec3::new(PICKUP.tear_distance_m + 0.1, 0.0, 0.0);
    for (joined, kind) in [(false, HeldKind::Prop), (true, HeldKind::Articulated)] {
        let (ticks, _) = ticks_held(held(joined, kind), far, far.normalize(), 10);
        assert_eq!(ticks, 0, "joined {joined}");
    }
}

#[test]
fn two_hands_on_a_joined_heavy_object_pull_as_a_carry_of_it_would() {
    // A torn-off door held at both ends, joined to both hands' bodies.
    let ends = [Vec3::new(-0.4, 0.0, 0.0), Vec3::new(0.4, 0.0, 0.0)];
    let mut state = PickupState::new();
    for (k, end) in ends.into_iter().enumerate() {
        let at = Pose {
            position: end,
            ..palm().pose
        };
        state.held[k] = Some(Held {
            grip: Grip::of(at, body().pose, end),
            mass_kg: 75.0,
            ..held(true, HeldKind::Prop)
        });
    }
    let mut fake = FakeBodies {
        body: bodies::BodyState {
            mass_kg: Some(75.0),
            ..body()
        },
        calls: Vec::new(),
        driven: None,
    };
    state.input_at = Some(Instant::now());
    let palms = ends.map(|end| {
        Some(PalmMotion {
            pose: Pose {
                position: end,
                ..palm().pose
            },
            ..palm()
        })
    });
    let inputs = TickInputs {
        palms,
        ..inputs(true)
    };
    let outcome = unsafe { tick_with(&mut state, inputs, &PICKUP, &mut fake) };
    let carry = HoldTuning::carrying(
        &PICKUP,
        75.0,
        &ends.map(|end| {
            Some(HandHold {
                anchor: end,
                target: GripTarget {
                    point: end,
                    orientation: Quat::IDENTITY,
                },
                velocity: Vec3::ZERO,
                angular_velocity: Vec3::ZERO,
            })
        }),
    );
    assert!(carry.hand_force_n > PICKUP.hand_force_n);
    for joint in outcome.joints {
        let joint = joint.unwrap();
        assert!(joint.shared);
        assert_eq!(joint.hand_force_n, carry.hand_force_n);
    }
    // The pull lifts it, and the report says so, as it would for the carry.
    assert!(outcome.reports.iter().flatten().all(|r| r.liftable));
    assert!(fake.calls.iter().all(|&c| c != "drive"));
}

#[test]
fn a_hold_that_ends_takes_its_strain_with_it() {
    // A caught hold builds up stuck time, then ends without a tear: a later hold of the same
    // body starts fresh rather than inheriting it.
    let lag = Vec3::new(PICKUP.break_distance_m + 0.1, 0.0, 0.0);
    let caught = PalmMotion {
        pose: Pose {
            position: lag,
            ..palm().pose
        },
        ..palm()
    };
    let mut fake = FakeBodies {
        body: body(),
        calls: Vec::new(),
        driven: None,
    };
    let endings: [fn(&mut PickupState) -> TickInputs; 2] = [
        // The hand stops being tracked.
        |_| TickInputs {
            palms: [None; 2],
            ..inputs(false)
        },
        // The grips stop being read.
        |state| {
            state.input_at = None;
            inputs(false)
        },
    ];
    for end in endings {
        let mut state = PickupState::new();
        state.held[0] = Some(held(false, HeldKind::Prop));
        for _ in 0..2 {
            state.input_at = Some(Instant::now());
            let inputs = TickInputs {
                palms: [Some(caught), None],
                ..inputs(false)
            };
            unsafe { tick_with(&mut state, inputs, &PICKUP, &mut fake) };
        }
        assert!(state.held[0].is_some() && state.strains[0].is_some());
        state.input_at = Some(Instant::now());
        let inputs = end(&mut state);
        unsafe { tick_with(&mut state, inputs, &PICKUP, &mut fake) };
        assert_eq!(state.held[0], None);
        assert_eq!(state.strains[0], None);
    }
}
