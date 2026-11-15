use std::collections::HashMap;

use glam::{Quat, Vec2, Vec3};
use proptest::prelude::*;

use super::*;

const DT: f32 = 1.0 / 60.0;
const TUNING: ContactTuning = ContactTuning::new();
const LEG_M: f32 = 0.83;
const ANKLE_M: f32 = 0.08;
const HIP_HALF_M: f32 = 0.1;
const HIP_DROP_M: f32 = 0.09;
/// The pelvis's height that stands the legs straight over ankles on the floor.
const STANDING_M: f32 = ANKLE_M + LEG_M + HIP_DROP_M;
const PLATFORM: SupportId = SupportId(7);

/// Flat ground at zero; past `ledge_x` (if any) there is none; a platform support moves.
#[derive(Default)]
struct World {
    ledge_x: Option<f32>,
    /// How many frames each probe takes to answer.
    latency: u32,
    asked: HashMap<usize, (Vec2, u32)>,
    platform: Option<SupportFrame>,
    on_platform: bool,
    probes: usize,
}

impl Surroundings for World {
    fn frame(&self, support: SupportId) -> Option<SupportFrame> {
        match support {
            SupportId::WORLD => Some(SupportFrame::IDENTITY),
            PLATFORM => self.platform,
            _ => None,
        }
    }

    fn probe(&mut self, side: Side, at: Vec2, _near: f32) -> Probe {
        self.probes += 1;
        if self.latency > 0 {
            let entry = self.asked.entry(side.index()).or_insert((at, 0));
            if entry.0.distance(at) > 1e-4 {
                *entry = (at, 0);
            }
            entry.1 += 1;
            if entry.1 <= self.latency {
                return Probe::Pending;
            }
        }
        if self.ledge_x.is_some_and(|x| at.x > x) {
            return Probe::Miss;
        }
        if self.on_platform {
            let frame = self.platform.unwrap();
            return Probe::Hit(Ground {
                height: frame.translation.y,
                support: PLATFORM,
            });
        }
        Probe::Hit(Ground {
            height: 0.0,
            support: SupportId::WORLD,
        })
    }
}

fn stance() -> Stance {
    Stance {
        width_m: 0.29,
        toe_out_deg: 10.0,
    }
}

/// The frame of a pelvis at `pelvis` facing `yaw`, the neck over it by `lean` toward the facing.
fn frame(pelvis: Vec3, yaw: f32, velocity: Vec2) -> ContactFrame {
    let turn = Quat::from_rotation_y(yaw);
    let hips = [-1.0, 1.0].map(|s| pelvis + turn * Vec3::new(s * HIP_HALF_M, -HIP_DROP_M, 0.0));
    let mut f = ContactFrame {
        pelvis,
        pelvis_yaw: yaw,
        neck: pelvis + Vec3::Y * 0.6,
        hips,
        leg_m: LEG_M,
        ankle_height_m: ANKLE_M,
        pelvis_velocity: velocity,
        external_shift: Vec2::ZERO,
        stance: stance(),
        animated: [FootPose {
            ankle: Vec3::ZERO,
            yaw: 0.0,
        }; 2],
        dt: DT,
    };
    f.animated = Side::BOTH.map(|side| {
        let (spot, yaw) = stance_spot(&f, side);
        FootPose {
            ankle: Vec3::new(spot.x, ANKLE_M, spot.y),
            yaw,
        }
    });
    f
}

fn standing_at(x: f32, z: f32, yaw: f32) -> ContactFrame {
    frame(Vec3::new(x, STANDING_M, z), yaw, Vec2::ZERO)
}

/// Run frames from `at(t)`, checking that a planted foot never moves while it stays planted and
/// that at most one foot swings, and collect the events.
fn run(
    contacts: &mut FootContacts,
    world: &mut World,
    seconds: f32,
    at: impl Fn(f32) -> ContactFrame,
) -> (Vec<ContactEvent>, Option<Handover>, ContactOutput) {
    let mut events = Vec::new();
    let mut last: Option<ContactOutput> = None;
    let mut handover = None;
    for i in 0..(seconds / DT).round() as usize {
        let out = contacts.update(&at(i as f32 * DT), world, &TUNING);
        let swinging = out
            .feet
            .iter()
            .filter(|f| matches!(f, FootTarget::Swinging { .. }))
            .count();
        assert!(swinging <= 1, "two feet swing at once");
        if let Some(previous) = &last
            && !world.on_platform
        {
            for side in Side::BOTH {
                if let (FootTarget::Planted(a), FootTarget::Planted(b)) =
                    (previous.feet[side.index()], out.feet[side.index()])
                {
                    assert!(a.ankle.distance(b.ankle) < 1e-5, "a planted foot slid");
                }
            }
        }
        events.extend(out.events.iter().copied());
        handover = handover.or(out.handover);
        last = Some(out);
    }
    (events, handover, last.unwrap())
}

fn released(events: &[ContactEvent]) -> Vec<(Side, Release)> {
    events
        .iter()
        .filter_map(|e| match *e {
            ContactEvent::Released { side, reason, .. } => Some((side, reason)),
            _ => None,
        })
        .collect()
}

#[test]
fn standing_still_plants_both_feet_and_never_steps() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    // A standing player sways a few centimetres and turns the hips a little.
    let (events, handover, out) = run(&mut contacts, &mut world, 20.0, |t| {
        let sway = Vec3::new(
            0.03 * (t * 1.3).sin(),
            -0.01 * (t * 0.7).sin(),
            0.02 * t.cos(),
        );
        frame(
            Vec3::new(0.0, STANDING_M, 0.0) + sway,
            0.15 * (t * 0.4).sin(),
            Vec2::ZERO,
        )
    });
    assert_eq!(handover, None);
    assert!(released(&events).is_empty(), "{events:?}");
    assert_eq!(out.planted(), 2);
    assert_eq!(contacts.steps(), 0);
}

#[test]
fn a_bend_over_planted_feet_moves_no_foot() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    // The player bends over to look down: the head goes forward, the pelvis back a little and
    // down.
    let (events, ..) = run(&mut contacts, &mut world, 4.0, |t| {
        let bend = (t / 1.0).min(1.0);
        let mut f = frame(
            Vec3::new(0.0, STANDING_M - 0.04 * bend, 0.08 * bend),
            0.0,
            Vec2::ZERO,
        );
        f.neck = f.pelvis + Vec3::new(0.0, 0.6 - 0.25 * bend, -0.45 * bend);
        f
    });
    assert!(released(&events).is_empty(), "{events:?}");
}

#[test]
fn a_physical_step_aside_steps_one_foot_then_the_other() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    let (events, handover, out) = run(&mut contacts, &mut world, 3.0, |t| {
        let s = (t / 0.6).min(1.0);
        let velocity = if t < 0.6 { 0.35 / 0.6 } else { 0.0 };
        frame(
            Vec3::new(0.35 * s, STANDING_M, 0.0),
            0.0,
            Vec2::new(velocity, 0.0),
        )
    });
    assert_eq!(handover, None);
    let steps = released(&events);
    assert!((2..=3).contains(&steps.len()), "{steps:?}");
    // The feet alternate.
    for pair in steps.windows(2) {
        assert_ne!(pair[0].0, pair[1].0, "{steps:?}");
    }
    // They end square under the pelvis.
    let end = standing_at(0.35, 0.0, 0.0);
    for side in Side::BOTH {
        let FootTarget::Planted(pose) = out.feet[side.index()] else {
            panic!("not planted: {out:?}");
        };
        let (spot, yaw) = stance_spot(&end, side);
        assert!(
            (pose.flat() - spot).length() < TUNING.tidy_m,
            "{side:?}: {pose:?}"
        );
        assert!(wrap(pose.yaw - yaw).abs() < 0.1);
        assert!((pose.ankle.y - ANKLE_M).abs() < 1e-5);
    }
}

#[test]
fn a_held_body_turn_takes_a_turn_step_on_each_foot() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    let turn = 70f32.to_radians();
    let (events, handover, out) = run(&mut contacts, &mut world, 3.0, |t| {
        standing_at(0.0, 0.0, turn * (t / 0.3).min(1.0))
    });
    assert_eq!(handover, None);
    let steps = released(&events);
    assert!(steps.len() >= 2, "{steps:?}");
    // A turn to the left opens with the left foot, on the twist.
    assert_eq!(steps[0], (Side::Left, Release::Twist));
    assert_eq!(steps[1].0, Side::Right);
    let end = standing_at(0.0, 0.0, turn);
    for side in Side::BOTH {
        let pose = out.feet[side.index()].pose().unwrap();
        assert!(
            wrap(pose.yaw - stance_spot(&end, side).1).abs() < 0.15,
            "{side:?}"
        );
    }
}

#[test]
fn a_glance_that_twists_the_hips_less_than_the_threshold_moves_no_foot() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    let glance = 30f32.to_radians();
    let (events, ..) = run(&mut contacts, &mut world, 4.0, |t| {
        standing_at(
            0.0,
            0.0,
            glance * (std::f32::consts::PI * (t / 1.5).min(1.0)).sin(),
        )
    });
    assert!(released(&events).is_empty(), "{events:?}");
}

#[test]
fn a_brief_twist_past_the_threshold_does_not_step() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    let (events, ..) = run(&mut contacts, &mut world, 2.0, |t| {
        // Past the threshold for a tenth of a second only.
        let yaw = if (0.2..0.3).contains(&t) { 0.9 } else { 0.0 };
        standing_at(0.0, 0.0, yaw)
    });
    assert!(released(&events).is_empty(), "{events:?}");
}

#[test]
fn walking_away_hands_over_to_the_walk_and_drops_the_contacts() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    let speed = 1.4;
    let (_, handover, out) = run(&mut contacts, &mut world, 1.0, |t| {
        frame(
            Vec3::new(0.0, STANDING_M, -speed * t),
            0.0,
            Vec2::new(0.0, -speed),
        )
    });
    assert!(handover.is_some());
    // Dropped on the frame of the hand-over, and acquired again on the frames after.
    assert!(out.planted() <= 2);
    let mut after = FootContacts::new();
    let mut world = World::default();
    let first = after.update(&standing_at(0.0, 0.0, 0.0), &mut world, &TUNING);
    assert_eq!(first.planted(), 2);
}

#[test]
fn a_landing_over_a_ledge_leaves_the_foot_unsupported() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    world.ledge_x = Some(0.2);
    let (events, ..) = run(&mut contacts, &mut world, 1.0, |t| {
        let s = (t / 0.4).min(1.0);
        frame(
            Vec3::new(0.3 * s, STANDING_M, 0.0),
            0.0,
            Vec2::new(if t < 0.4 { 0.75 } else { 0.0 }, 0.0),
        )
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ContactEvent::Missed { side: Side::Right })),
        "{events:?}"
    );
    assert!(matches!(
        contacts.state(Side::Right),
        FootState::Unsupported
    ));
}

#[test]
fn a_late_probe_still_lands_the_step_on_its_ground() {
    let mut contacts = FootContacts::new();
    let mut world = World {
        latency: 3,
        ..World::default()
    };
    let (_, _, out) = run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    assert_eq!(out.planted(), 2, "acquired once the probes answer");
    let (events, ..) = run(&mut contacts, &mut world, 2.0, |t| {
        let s = (t / 0.5).min(1.0);
        frame(Vec3::new(0.3 * s, STANDING_M, 0.0), 0.0, Vec2::ZERO)
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ContactEvent::Landed { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ContactEvent::Missed { .. }))
    );
}

#[test]
fn a_moving_support_carries_its_planted_foot() {
    let mut contacts = FootContacts::new();
    let mut world = World {
        platform: Some(SupportFrame {
            translation: Vec3::new(0.0, 0.5, 0.0),
            rotation: Quat::IDENTITY,
        }),
        on_platform: true,
        ..World::default()
    };
    let up = |x: f32| frame(Vec3::new(x, STANDING_M + 0.5, 0.0), 0.0, Vec2::ZERO);
    let first = contacts.update(&up(0.0), &mut world, &TUNING);
    // The animated feet stand on the floor's height; acquired, they are on the platform's.
    let left = first.feet[0].pose().unwrap();
    assert!((left.ankle.y - (0.5 + ANKLE_M)).abs() < 1e-5);
    world.platform.as_mut().unwrap().translation.x += 0.05;
    let moved = contacts.update(&up(0.05), &mut world, &TUNING);
    let carried = moved.feet[0].pose().unwrap();
    assert!((carried.ankle.x - left.ankle.x - 0.05).abs() < 1e-5);
    // The platform leaves: its feet are lost.
    world.platform = None;
    world.on_platform = false;
    let gone = contacts.update(&up(0.05), &mut world, &TUNING);
    assert!(
        gone.events
            .iter()
            .any(|e| matches!(e, ContactEvent::Lost { .. }))
    );
}

#[test]
fn the_root_carried_past_the_tolerance_steps_a_foot() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    let (events, ..) = run(&mut contacts, &mut world, 1.0, |t| {
        let mut f = standing_at(0.0, 0.0, 0.0);
        if t < 0.25 {
            f.external_shift = Vec2::new(0.0, 0.02);
        }
        f
    });
    assert!(
        released(&events)
            .iter()
            .any(|(_, reason)| *reason == Release::RootMoved),
        "{events:?}"
    );
}

#[test]
fn the_centre_of_mass_out_past_the_toes_steps_a_foot() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    // A crouch carrying the pelvis forward over bent knees: the legs reach, but the body's weight
    // is ahead of the toes.
    let (events, ..) = run(&mut contacts, &mut world, 1.0, |_| {
        let mut f = frame(Vec3::new(0.0, 0.75, -0.25), 0.0, Vec2::ZERO);
        f.neck = f.pelvis + Vec3::new(0.0, 0.4, -0.45);
        f
    });
    assert_eq!(
        released(&events).first().map(|(_, r)| *r),
        Some(Release::CentreOfMass),
        "{events:?}"
    );
}

#[test]
fn a_reset_drops_the_contacts_and_they_are_acquired_where_the_feet_stand() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    contacts.reset();
    assert!(matches!(contacts.state(Side::Left), FootState::Unsupported));
    let out = contacts.update(&standing_at(2.0, 0.0, 0.0), &mut world, &TUNING);
    assert_eq!(out.planted(), 2);
    assert!(
        out.events
            .iter()
            .all(|e| matches!(e, ContactEvent::Acquired { .. }))
    );
    assert!((out.feet[0].pose().unwrap().ankle.x - (2.0 - 0.145)).abs() < 1e-4);
}

#[test]
fn a_step_too_long_hands_over() {
    let mut contacts = FootContacts::new();
    let mut world = World::default();
    run(&mut contacts, &mut world, 0.5, |_| {
        standing_at(0.0, 0.0, 0.0)
    });
    // A teleport-like jump of the pelvis that leaves the legs far behind, slowly enough not to be
    // a walk's speed.
    let out = contacts.update(&standing_at(0.0, -0.9, 0.0), &mut world, &TUNING);
    assert_eq!(out.handover, Some(Handover::StepTooLong));
    assert_eq!(out.planted(), 0);
}

proptest! {
    #[test]
    fn the_feet_never_slide_and_never_swing_together(
        moves in proptest::collection::vec((-0.6f32..0.6, -0.6f32..0.6, -1.5f32..1.5), 1..6),
    ) {
        let mut contacts = FootContacts::new();
        let mut world = World::default();
        let mut at = Vec3::new(0.0, STANDING_M, 0.0);
        let mut yaw = 0.0;
        for (dx, dz, dyaw) in moves {
            let from = at;
            let from_yaw = yaw;
            at += Vec3::new(dx, 0.0, dz);
            yaw += dyaw;
            run(&mut contacts, &mut world, 1.5, |t| {
                let s = (t / 0.8).min(1.0);
                frame(from.lerp(at, s), from_yaw + (yaw - from_yaw) * s, Vec2::ZERO)
            });
        }
    }
}
