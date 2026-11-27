use super::*;
use crate::controllers::physical_hands::doors::tuning::DOORS;

fn config() -> &'static TearTuning {
    &DOORS.get().tearing
}

fn target(vehicle_body: Option<u32>) -> DoorTarget {
    DoorTarget {
        vehicle: NonZeroUsize::new(16).unwrap(),
        vehicle_body,
        part_index: 2,
        slot: jc3gi::vehicle::vehicle::DoorSlot::FRONT_LEFT,
        latched: false,
        outward: Vec3::X,
    }
}

struct QueuedPart {
    owner: Option<NonZeroUsize>,
    captured_alive: bool,
    weak_vehicle: Option<NonZeroUsize>,
    current_body: Option<u32>,
    result: TearResult,
    proof: bool,
    reads: usize,
}

impl QueuedPart {
    fn live() -> Self {
        Self {
            owner: NonZeroUsize::new(8),
            captured_alive: true,
            weak_vehicle: Some(target(None).vehicle),
            current_body: Some(3),
            result: TearResult::Refused,
            proof: false,
            reads: 0,
        }
    }
}

impl AcquisitionEngine for QueuedPart {
    fn owner(&mut self, _: u32) -> Option<NonZeroUsize> {
        self.owner
    }
    fn alive(&mut self, _: NonZeroUsize, _: u32) -> bool {
        self.proof = self.captured_alive;
        self.captured_alive
    }
    fn vehicle_of(&mut self, object: NonZeroUsize) -> Option<NonZeroUsize> {
        assert_eq!(self.owner, Some(object));
        self.proof = self.weak_vehicle == Some(target(None).vehicle);
        self.weak_vehicle
    }
    fn vehicle_body(&mut self, _: NonZeroUsize) -> Option<u32> {
        assert!(
            self.proof,
            "vehicle dereferenced before current liveness proof"
        );
        self.reads += 1;
        self.current_body
    }
    fn continuity(&mut self, _: &TearGrip, _: NonZeroUsize, _: u32) -> TearResult {
        assert!(self.proof);
        self.result
    }
}

#[test]
fn queued_surviving_part_never_dereferences_a_destroyed_or_different_vehicle() {
    let object = NonZeroUsize::new(8).unwrap();
    for capture in [Some(3), None] {
        let mut engine = QueuedPart {
            captured_alive: false,
            weak_vehicle: None,
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(target(capture), object, 7, &mut engine),
            Err(AcquireRefusal::VehicleUnproven)
        );
        assert_eq!(engine.reads, 0);
    }
    let mut engine = QueuedPart {
        captured_alive: false,
        weak_vehicle: NonZeroUsize::new(32),
        ..QueuedPart::live()
    };
    assert_eq!(
        take_with(target(Some(3)), object, 7, &mut engine),
        Err(AcquireRefusal::VehicleUnproven)
    );
    assert_eq!(engine.reads, 0);
    let mut engine = QueuedPart {
        weak_vehicle: NonZeroUsize::new(32),
        ..QueuedPart::live()
    };
    assert_eq!(
        take_with(target(None), object, 7, &mut engine),
        Err(AcquireRefusal::VehicleUnproven)
    );
    assert_eq!(engine.reads, 0);
    let mut engine = QueuedPart {
        owner: None,
        ..QueuedPart::live()
    };
    assert_eq!(
        take_with(target(None), object, 7, &mut engine),
        Err(AcquireRefusal::BodyReowned)
    );
    assert_eq!(engine.reads, 0);
}

#[test]
fn an_unlatched_door_keeps_its_grip_when_the_captured_vehicle_body_changes() {
    let object = NonZeroUsize::new(8).unwrap();
    let door = target(Some(3));
    // The captured body still names the vehicle, but the vehicle's own body is another.
    let mut engine = QueuedPart {
        current_body: Some(4),
        ..QueuedPart::live()
    };
    assert_eq!(
        take_with(door, object, 7, &mut engine),
        Ok(Acquisition::Attached(TearGrip::new(door, 4)))
    );
    // The captured body no longer names the vehicle; the part's weak reference proves it.
    let mut engine = QueuedPart {
        captured_alive: false,
        current_body: Some(4),
        ..QueuedPart::live()
    };
    assert_eq!(
        take_with(door, object, 7, &mut engine),
        Ok(Acquisition::Attached(TearGrip::new(door, 4)))
    );
}

#[test]
fn queued_acquisition_classifies_external_detachment_and_lost_continuity() {
    let object = NonZeroUsize::new(8).unwrap();
    for capture in [Some(3), None] {
        let door = target(capture);
        let mut engine = QueuedPart::live();
        assert_eq!(
            take_with(door, object, 7, &mut engine),
            Ok(Acquisition::Attached(TearGrip::new(door, 3)))
        );
        let mut engine = QueuedPart {
            result: TearResult::Detached,
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(door, object, 7, &mut engine),
            Ok(Acquisition::Detached)
        );
        let mut engine = QueuedPart {
            result: TearResult::Lost,
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(door, object, 7, &mut engine),
            Err(AcquireRefusal::ContinuityLost)
        );
        let mut engine = QueuedPart {
            current_body: None,
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(door, object, 7, &mut engine),
            Err(AcquireRefusal::NoVehicleBody)
        );
    }
}

fn quiet() -> Gesture {
    Gesture {
        dt: 0.02,
        lag_m: 0.01,
        lag_direction: Vec3::X,
        separating_speed_mps: 0.0,
        opening_push_m: 0.0,
        load: 0.0,
        at_limit: false,
        free_swing: false,
    }
}

fn tears(intent: &mut Intent, gesture: Gesture, c: &TearTuning) -> bool {
    intent.step(gesture, c).is_some()
}

fn armed() -> Intent {
    settled(quiet())
}

fn armed_at_limit() -> Intent {
    settled(Gesture {
        at_limit: true,
        ..quiet()
    })
}

fn settled(input: Gesture) -> Intent {
    let mut intent = Intent::default();
    for _ in 0..15 {
        assert!(!tears(&mut intent, input, config()));
    }
    intent
}

#[test]
fn native_body_loss_change_and_relatch_never_count_as_detachment() {
    assert_eq!(body_continuity(true, false, true), TearResult::Refused);
    assert_eq!(body_continuity(true, true, false), TearResult::Detached);
    assert_eq!(body_continuity(false, true, false), TearResult::Lost);
    assert_eq!(body_continuity(false, false, true), TearResult::Lost);
    assert_eq!(body_continuity(true, false, false), TearResult::Lost);
}

#[test]
fn push_direction_requires_observed_swing_and_tracks_opening_not_closing() {
    let mut motion = OpeningMotion::default();
    assert_eq!(motion.observe(1.0, Vec3::ZERO), Vec3::ZERO);
    assert_eq!(motion.observe(1.0, Vec3::ZERO), Vec3::ZERO);
    assert_eq!(motion.observe(1.1, Vec3::X * 0.1), Vec3::X);
    assert_eq!(motion.observe(1.0, Vec3::ZERO), Vec3::X);
    assert_eq!(motion.observe(f32::NAN, Vec3::ZERO), Vec3::ZERO);
}

#[test]
fn opening_pull_and_unsettled_handoff_never_tear() {
    let mut intent = Intent::default();
    for lag in [0.06, 0.10, 0.18, 0.25] {
        assert!(!tears(
            &mut intent,
            Gesture {
                lag_m: lag,
                separating_speed_mps: 4.0,
                ..quiet()
            },
            config()
        ));
    }
}

#[test]
fn sharp_yank_requires_new_lag_and_does_not_repeat() {
    let mut intent = armed();
    let yank = Gesture {
        lag_m: 0.22,
        separating_speed_mps: 3.0,
        ..quiet()
    };
    assert!(tears(&mut intent, yank, config()));
    assert!(!tears(&mut intent, yank, config()));
    assert!(!tears(
        &mut armed(),
        Gesture {
            separating_speed_mps: 0.5,
            ..yank
        },
        config()
    ));
}

#[test]
fn push_requires_limit_direction_and_duration() {
    let push = Gesture {
        lag_m: 0.16,
        opening_push_m: 0.16,
        load: 0.9,
        at_limit: true,
        ..quiet()
    };
    let c = *config();
    let mut intent = armed_at_limit();
    for _ in 0..17 {
        assert!(!tears(&mut intent, push, &c));
    }
    assert!(tears(&mut intent, Gesture { dt: 0.03, ..push }, &c));
    for bad in [
        Gesture { load: 0.2, ..push },
        Gesture {
            at_limit: false,
            ..push
        },
        Gesture {
            opening_push_m: -0.16,
            ..push
        },
        Gesture {
            opening_push_m: 0.05,
            ..push
        },
    ] {
        let mut intent = armed_at_limit();
        for _ in 0..30 {
            assert!(!tears(&mut intent, bad, &c));
        }
    }
}

#[test]
fn stale_tracking_resets_intent() {
    let mut intent = armed();
    assert!(!tears(
        &mut intent,
        Gesture { dt: 0.5, ..quiet() },
        config()
    ));
    assert!(intent.settled_lag.is_none());
}

/// A recorded attempt: a door at rest, held through a joint, pulled at under a metre per second.
/// The pull grows the lag as fast as the hand moves, so it is a yank.
#[test]
fn a_firm_pull_on_a_door_that_does_not_give_tears_it() {
    let mut intent = armed();
    let mut lag = 0.01;
    let mut torn = None;
    for _ in 0..40 {
        lag += 0.8 * 0.02;
        let gesture = Gesture {
            lag_m: lag,
            separating_speed_mps: 0.8,
            ..quiet()
        };
        if let Some(kind) = intent.step(gesture, config()) {
            torn = Some((kind, lag));
            break;
        }
    }
    let (kind, lag) = torn.expect("the pull tears the door");
    assert_eq!(kind, TearGesture::Yank);
    assert!(lag < 0.25, "{lag}");
}

/// A heavy door hanging well off the hand settles at that lag, and the gestures measure from it.
#[test]
fn a_grip_settled_far_off_the_hand_measures_from_its_settled_lag() {
    let mut intent = Intent::default();
    let hanging = Gesture {
        lag_m: 0.12,
        ..quiet()
    };
    for _ in 0..15 {
        assert!(!tears(&mut intent, hanging, config()));
    }
    assert_eq!(intent.settled_lag, Some(0.12));
    // An absolute lag threshold would read this as a yank already; above the settled lag it is
    // not.
    let small = Gesture {
        lag_m: 0.2,
        separating_speed_mps: 3.0,
        ..quiet()
    };
    assert!(!tears(&mut intent, small, config()));
    let mut intent = Intent::default();
    for _ in 0..15 {
        tears(&mut intent, hanging, config());
    }
    assert!(tears(
        &mut intent,
        Gesture {
            lag_m: 0.32,
            ..small
        },
        config()
    ));
}

/// A slow pull the door will not follow tears once its lag has held far enough above the
/// settled lag, short of where the hand's body lets go of a joined door.
#[test]
fn a_slow_hard_pull_tears_and_a_slower_creep_settles_instead() {
    let pull = |speed: f32| {
        let mut intent = armed();
        let mut lag = 0.01;
        for tick in 0..200 {
            lag += speed * 0.02;
            let gesture = Gesture {
                lag_m: lag,
                separating_speed_mps: speed,
                ..quiet()
            };
            if let Some(kind) = intent.step(gesture, config()) {
                return Some((kind, lag, tick));
            }
        }
        None
    };
    let (kind, lag, _) = pull(0.3).expect("a slow hard pull tears");
    assert_eq!(kind, TearGesture::HardPull);
    assert!(lag < 0.4, "{lag}");
    assert_eq!(
        pull(0.1),
        None,
        "a creep under the settling speed resettles"
    );
}

/// A pull the door would swing with does not tear it, unless the hinge holds it there.
#[test]
fn a_pull_along_a_free_swing_never_tears() {
    let mut intent = armed();
    let mut lag: f32 = 0.01;
    for _ in 0..60 {
        lag += 1.5 * 0.02;
        let gesture = Gesture {
            lag_m: lag.min(0.5),
            separating_speed_mps: 1.5,
            free_swing: true,
            ..quiet()
        };
        assert!(!tears(&mut intent, gesture, config()));
    }
}

#[test_case::test_case(1.0 / 30.0)]
#[test_case::test_case(1.0 / 50.0)]
#[test_case::test_case(1.0 / 90.0)]
#[test_case::test_case(1.0 / 120.0)]
fn settled_opening_cannot_carry_a_baseline_into_the_stop(dt: f32) {
    let mut intent = armed();
    let opening = Gesture {
        dt,
        free_swing: true,
        opening_push_m: 0.01,
        ..quiet()
    };
    for _ in 0..(1.0 / dt) as usize {
        assert_eq!(intent.step(opening, config()), None);
        assert!(intent.settled_lag.is_none());
    }
    for tick in 1..=10 {
        let lag = 0.01 + tick as f32 * 0.03;
        assert_eq!(
            intent.step(
                Gesture {
                    dt,
                    lag_m: lag,
                    opening_push_m: lag,
                    separating_speed_mps: 1.0,
                    at_limit: true,
                    load: 1.0,
                    ..quiet()
                },
                config()
            ),
            None
        );
    }
    for _ in 0..(2.0 / dt) as usize {
        assert_eq!(
            intent.step(
                Gesture {
                    dt,
                    lag_m: 0.31,
                    opening_push_m: 0.31,
                    at_limit: true,
                    load: 1.0,
                    ..quiet()
                },
                config()
            ),
            None
        );
    }
    assert_eq!(intent.settled_lag, Some(0.31));
}

#[test]
fn stop_arrival_and_departure_require_fresh_settling_even_without_free_swing() {
    for at_limit in [true, false] {
        let mut intent = if at_limit { armed() } else { armed_at_limit() };
        let arrival = Gesture {
            at_limit,
            lag_m: 0.35,
            separating_speed_mps: 3.0,
            ..quiet()
        };
        assert_eq!(intent.step(arrival, config()), None);
        assert!(intent.settled_lag.is_none());
        for _ in 0..30 {
            assert_eq!(intent.step(arrival, config()), None);
        }
        assert!(intent.settled_lag.is_none());
        let resting = Gesture {
            separating_speed_mps: 0.0,
            ..arrival
        };
        for _ in 0..15 {
            assert_eq!(intent.step(resting, config()), None);
        }
        assert_eq!(intent.settled_lag, Some(0.35));
        assert_eq!(
            intent.step(
                Gesture {
                    lag_m: 0.53,
                    separating_speed_mps: 3.0,
                    ..resting
                },
                config()
            ),
            Some(TearGesture::Yank)
        );
    }
}

#[test]
fn an_opening_aligned_yank_at_the_stop_must_use_the_sustained_push_gate() {
    let mut intent = armed_at_limit();
    let push = Gesture {
        at_limit: true,
        lag_m: 0.33,
        opening_push_m: 0.33,
        separating_speed_mps: 3.0,
        load: 1.0,
        ..quiet()
    };
    assert_eq!(intent.step(push, config()), None);
    assert_eq!(intent.wrench_s, 0.0);
    for _ in 0..16 {
        assert_eq!(intent.step(push, config()), None);
    }
    assert_eq!(intent.step(push, config()), Some(TearGesture::LimitPush));
}

#[test]
fn a_resting_loaded_stop_needs_additional_push_not_absolute_lag() {
    let resting = Gesture {
        at_limit: true,
        lag_m: 0.18,
        opening_push_m: 0.18,
        load: 1.0,
        ..quiet()
    };
    let mut intent = settled(resting);
    for _ in 0..100 {
        assert_eq!(intent.step(resting, config()), None);
    }
    let push = Gesture {
        lag_m: 0.34,
        opening_push_m: 0.34,
        ..resting
    };
    for _ in 0..17 {
        assert_eq!(intent.step(push, config()), None);
    }
    assert_eq!(intent.step(push, config()), Some(TearGesture::LimitPush));
    for _ in 0..50 {
        assert_eq!(intent.step(push, config()), None);
    }
}

#[test]
fn a_large_sideways_lag_with_a_small_opening_component_does_not_count_as_push() {
    let mut intent = armed_at_limit();
    for _ in 0..100 {
        assert_eq!(
            intent.step(
                Gesture {
                    at_limit: true,
                    lag_m: 0.25,
                    opening_push_m: 0.15,
                    load: 1.0,
                    ..quiet()
                },
                config()
            ),
            None
        );
    }
}

#[test]
fn a_brief_wrist_sized_lag_spike_does_not_become_a_hard_pull() {
    let mut intent = armed();
    let snag = Gesture {
        lag_m: 0.32,
        ..quiet()
    };
    for _ in 0..6 {
        assert_eq!(intent.step(snag, config()), None);
    }
    assert_eq!(intent.step(quiet(), config()), None);
    assert_eq!(intent.wrench_s, 0.0);
    for _ in 0..30 {
        assert_eq!(intent.step(quiet(), config()), None);
    }
}

#[test]
fn a_sustained_hard_pull_is_not_resettled_away() {
    let mut intent = armed();
    let pull = Gesture {
        lag_m: 0.32,
        ..quiet()
    };
    for _ in 0..8 {
        assert_eq!(intent.step(pull, config()), None);
    }
    assert_eq!(
        intent.step(Gesture { dt: 0.03, ..pull }, config()),
        Some(TearGesture::HardPull)
    );
}

#[test]
fn wrist_direction_changes_discard_the_old_lag_baseline() {
    for direction in [Vec3::Y, -Vec3::X] {
        let mut intent = armed();
        let adjusted = Gesture {
            lag_m: 0.32,
            lag_direction: direction,
            separating_speed_mps: 3.0,
            ..quiet()
        };
        assert_eq!(intent.step(adjusted, config()), None);
        assert!(intent.settled_lag.is_none());
        let resting = Gesture {
            separating_speed_mps: 0.0,
            ..adjusted
        };
        for _ in 0..30 {
            assert_eq!(intent.step(resting, config()), None);
        }
        assert_eq!(intent.settled_lag, Some(0.32));
        assert_eq!(
            intent.step(
                Gesture {
                    lag_m: 0.50,
                    ..adjusted
                },
                config()
            ),
            Some(TearGesture::Yank)
        );
    }
}

#[test]
fn gradual_wrist_rotation_cannot_keep_an_old_baseline() {
    let mut intent = armed();
    for tick in 1..=50 {
        let angle = tick as f32 * 0.02;
        assert_eq!(
            intent.step(
                Gesture {
                    lag_m: 0.01 + tick as f32 * 0.006,
                    lag_direction: Vec3::new(angle.cos(), angle.sin(), 0.0),
                    separating_speed_mps: 0.3,
                    ..quiet()
                },
                config()
            ),
            None
        );
    }
    assert!(intent.settled_lag.is_none());
}

#[test]
fn low_load_or_leaving_the_stop_clears_partial_push_intent() {
    let push = Gesture {
        at_limit: true,
        lag_m: 0.17,
        opening_push_m: 0.17,
        load: 1.0,
        ..quiet()
    };
    for interruption in [
        Gesture { load: 0.0, ..push },
        Gesture {
            at_limit: false,
            ..push
        },
        Gesture {
            free_swing: true,
            ..push
        },
    ] {
        let mut intent = armed_at_limit();
        for _ in 0..10 {
            assert_eq!(intent.step(push, config()), None);
        }
        assert_eq!(intent.step(interruption, config()), None);
        assert_eq!(intent.push_s, 0.0);
        for _ in 0..10 {
            assert_eq!(intent.step(push, config()), None);
        }
    }
}

#[test]
fn invalid_samples_clear_the_armed_baseline_and_partial_effort() {
    for invalid in [
        Gesture { dt: 0.0, ..quiet() },
        Gesture {
            dt: -0.02,
            ..quiet()
        },
        Gesture {
            dt: f32::NAN,
            ..quiet()
        },
        Gesture {
            lag_m: f32::NAN,
            ..quiet()
        },
        Gesture {
            lag_m: -0.1,
            ..quiet()
        },
        Gesture {
            lag_direction: Vec3::NAN,
            ..quiet()
        },
        Gesture {
            separating_speed_mps: f32::INFINITY,
            ..quiet()
        },
        Gesture {
            opening_push_m: f32::NAN,
            ..quiet()
        },
        Gesture {
            load: f32::NAN,
            ..quiet()
        },
    ] {
        let mut intent = armed();
        assert_eq!(intent.step(invalid, config()), None);
        assert_eq!(intent, Intent::default());
        assert_eq!(
            intent.step(
                Gesture {
                    lag_m: 0.32,
                    separating_speed_mps: 3.0,
                    ..quiet()
                },
                config()
            ),
            None
        );
    }
}

#[test]
fn healthy_authored_parts_are_allowed_but_protections_and_invalid_parts_are_not() {
    let allowed = Permit {
        vehicle_protected: false,
        part_protected: false,
        authored: true,
        body_survives: true,
        valid: true,
        health: 100,
        parallel: false,
    };
    assert!(allowed.allowed());
    for refused in [
        Permit {
            vehicle_protected: true,
            ..allowed
        },
        Permit {
            part_protected: true,
            ..allowed
        },
        Permit {
            authored: false,
            ..allowed
        },
        Permit {
            body_survives: false,
            ..allowed
        },
        Permit {
            valid: false,
            ..allowed
        },
        Permit {
            health: -1,
            ..allowed
        },
        Permit {
            health: i32::MAX,
            ..allowed
        },
        Permit {
            parallel: true,
            ..allowed
        },
    ] {
        assert!(!refused.allowed());
    }
}
