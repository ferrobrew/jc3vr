use std::num::NonZeroUsize;

use glam::{Mat4, Quat};

use super::*;
use crate::controllers::{
    bindings::controls::ButtonControl,
    body::rig::frames::Frames,
    config::ControllersConfig,
    hands::pose::CockpitHand,
    weapons::reload::{
        ReloadStatus,
        belt::{BeltPush, PushStage},
        chamber::Chamber,
        eject::{FlickDetector, TapDetector},
        engine::{DetachOutcome, Reading},
        frames::GunFrame,
        insert::{InsertStage, Well},
        plan::ReloadPlan,
        records::{RecordKey, Records},
    },
};

#[derive(Default)]
struct FakeEngine {
    authority: bool,
    submitted: Vec<DetachRequest>,
    result: Option<DetachOutcome>,
    held: Option<loose::Loose>,
    canceled: usize,
    delivered: usize,
}

impl DetachEngine for FakeEngine {
    fn available(&self) -> bool {
        self.authority
    }
    fn submit(&mut self, request: DetachRequest) -> bool {
        if !self.authority {
            return false;
        }
        self.submitted.push(request);
        true
    }
    fn poll(&mut self, request: DetachRequest) -> Option<DetachOutcome> {
        assert_eq!(self.submitted.last(), Some(&request));
        let result = self.result.take();
        self.delivered += usize::from(result.is_some());
        result
    }
    fn cancel(&mut self, _: DetachRequest) {
        self.canceled += 1;
        self.result = None;
    }
    fn take(&mut self, _: Hand, magazine: loose::Loose, _: Vec3) -> bool {
        self.held = Some(magazine);
        true
    }
}

fn state() -> State {
    State {
        status: ReloadStatus::default(),
        tick: None,
        pouch: None,
        stocked_pouch: None,
        free: Some(Hand::Left),
        loose_seen: Vec::new(),
        frames: Some(Frames {
            head_world: Vec3::ZERO,
            head_cockpit: Vec3::ZERO,
            view: Quat::IDENTITY,
            world_scale: 1.0,
            character: Mat4::IDENTITY,
        }),
        records: Records::new(),
        measured: [None; 2],
        extraction: Pull::new(),
        pending_detach: None,
        pending_ejects: [None; 2],
        detach_generation: 0,
        extraction_blocks_belt: [false; 2],
        extraction_needs_neutral: [false; 2],
        insertion_source: InsertionSource::Pouch,
        tap: TapDetector::new(),
        flicks: [FlickDetector::new(); 2],
        belts: [BeltPush::new(); 2],
        last_grips: [None; 2],
        last_mouths: [None; 2],
        drawn: None,
        since_draw_s: 10.0,
    }
}

fn tick(pull: f32, rounds: i32) -> Tick {
    let grip = Pose {
        position: Vec3::NEG_Y * (0.06 + pull),
        ..Pose::IDENTITY
    };
    Tick {
        sequence: Some(7),
        weapons: [
            Some(WeaponTick {
                address: 1,
                key: RecordKey::Object(1),
                plan: ReloadPlan {
                    load: LoadKind::Magazine,
                    action: crate::controllers::weapons::reload::chamber::ActionKind::Slide,
                    well: WellFrame::Gun {
                        mouth: Vec3::ZERO,
                        axis: Vec3::Y,
                    },
                    handle: None,
                    travel_m: 0.06,
                    part_top_m: None,
                },
                from_table: true,
                reading: Reading {
                    class: 0,
                    name_hash: 1,
                    capacity: 30,
                    magazine: rounds,
                    chambers: 0,
                    out: false,
                    exempt: false,
                    unlimited: false,
                },
                reserve: 70,
            }),
            None,
        ],
        dominant: Hand::Right,
        guns: [
            None,
            Some(GunTick {
                weapon: 0,
                index: 0,
                magazine: Some(serial::Identity::test()),
                wrist: Pose::IDENTITY,
                frame: Some(GunFrame {
                    pose: Pose::IDENTITY,
                    world_scale: 1.0,
                }),
                well: Some(Well {
                    mouth: Vec3::ZERO,
                    axis: Vec3::Y,
                }),
                handle: None,
            }),
        ],
        hands: [
            Some(CockpitHand {
                wrist: grip,
                grip,
                aim: grip,
            }),
            None,
        ],
        world_scale: 1.0,
    }
}

fn inputs(claim: bool, held: bool) -> FinishInputs {
    FinishInputs {
        grabs: [claim.then_some(GrabVolume::InsertedMagazine), None],
        grips: [
            ButtonControl {
                down: held,
                pressed: claim,
                released: !held,
            },
            ButtonControl::default(),
        ],
        triggers: [ButtonControl::default(); 2],
        reload_button: ButtonControl::default(),
        dt: 0.01,
    }
}

#[test]
fn state_refuses_unavailable_adapter_without_queueing_or_mutating_records() {
    let mut state = state();
    let before = *state.records.get(RecordKey::Object(1));
    let mut engine = FakeEngine::default();
    state.extract_with(
        &tick(0.0, 17),
        &inputs(true, true),
        &ReloadConfig::new(),
        &mut engine,
    );
    state.extract_with(
        &tick(0.2, 17),
        &inputs(false, true),
        &ReloadConfig::new(),
        &mut engine,
    );
    assert!(engine.submitted.is_empty());
    assert_eq!(state.extraction_hand(), None);
    assert_eq!(*state.records.get(RecordKey::Object(1)), before);
    assert_eq!(state.status.counts.ejects, 0);
}

#[test]
fn state_waits_for_actual_result_and_no_throw_keeps_readiness_and_ownership_unchanged() {
    let mut state = state();
    let key = RecordKey::Object(1);
    state.records.get(key).chambers = [Chamber::Chambered; 2];
    let record = *state.records.get(key);
    let mut engine = FakeEngine {
        authority: true,
        ..FakeEngine::default()
    };
    let config = ReloadConfig::new();
    state.extract_with(&tick(0.0, 17), &inputs(true, true), &config, &mut engine);
    state.extract_with(&tick(0.1, 17), &inputs(false, true), &config, &mut engine);
    assert_eq!(engine.submitted.len(), 1);
    assert_eq!(state.extraction_hand(), Some(Hand::Left));
    assert_eq!(state.status.insert_hands, None);
    state.extract_with(&tick(0.1, 17), &inputs(false, true), &config, &mut engine);
    assert!(state.pending_detach.is_some());
    engine.result = Some(DetachOutcome::NoThrow);
    state.extract_with(&tick(0.1, 17), &inputs(false, true), &config, &mut engine);
    assert!(state.pending_detach.is_none());
    assert_eq!(state.status.counts.ejects, 0);
    assert_eq!(state.status.insert_hands, None);
    assert!(engine.held.is_none());
    assert_eq!(*state.records.get(key), record);
    // Neutral then a new direct claim is required for a retry.
    state.extract_with(&tick(0.0, 17), &inputs(false, false), &config, &mut engine);
    state.extract_with(&tick(0.0, 17), &inputs(true, true), &config, &mut engine);
    assert_eq!(state.extraction_hand(), Some(Hand::Left));
}

#[test]
fn state_mode_cancellation_cannot_resume_a_held_claim() {
    let mut state = state();
    let mut engine = FakeEngine {
        authority: true,
        ..FakeEngine::default()
    };
    let config = ReloadConfig::new();
    state.extract_with(&tick(0.0, 17), &inputs(true, true), &config, &mut engine);
    state.extract_with(&tick(0.04, 17), &inputs(false, true), &config, &mut engine);
    state.cancel_extraction();
    // UI skips begin/finish; the resumed frame can contain a stale routed claim.
    state.extract_with(&tick(0.2, 17), &inputs(true, true), &config, &mut engine);
    assert!(engine.submitted.is_empty());
    assert_eq!(state.extraction_hand(), None);
    state.extract_with(&tick(0.0, 17), &inputs(false, false), &config, &mut engine);
    state.extract_with(&tick(0.0, 17), &inputs(true, true), &config, &mut engine);
    state.extract_with(&tick(0.1, 17), &inputs(false, true), &config, &mut engine);
    assert_eq!(engine.submitted.len(), 1);
}

#[test]
fn state_retained_result_enters_nonexpiring_insertion_once_without_chamber_changes() {
    for rounds in [0, 17] {
        let mut state = state();
        let key = RecordKey::Object(1);
        state.records.get(key).chambers = [if rounds == 0 {
            Chamber::Empty
        } else {
            Chamber::Chambered
        }; 2];
        let record = *state.records.get(key);
        let magazine = loose::Loose::test(
            NonZeroUsize::new(1).unwrap(),
            1,
            NonZeroUsize::new(1).unwrap(),
            2,
            rounds,
        );
        let mut engine = FakeEngine {
            authority: true,
            ..FakeEngine::default()
        };
        let config = ReloadConfig::new();
        state.extract_with(
            &tick(0.0, rounds),
            &inputs(true, true),
            &config,
            &mut engine,
        );
        state.extract_with(
            &tick(0.1, rounds),
            &inputs(false, true),
            &config,
            &mut engine,
        );
        engine.result = Some(DetachOutcome::Retained(magazine));
        let mut detached = tick(0.1, 0);
        detached.weapons[0].as_mut().unwrap().reading.out = true;
        state.extract_with(&detached, &inputs(false, true), &config, &mut engine);
        assert!(state.extraction_blocks_belt[Hand::Right.index()]);
        assert_eq!(engine.held, Some(magazine));
        assert_eq!(state.insertion_source, InsertionSource::RetainedMagazine);
        assert_eq!(
            state.status.insertion.stage,
            InsertStage::Holding { elapsed_s: 0.0 }
        );
        assert_eq!(state.status.counts.ejects, 1);
        assert_eq!(*state.records.get(key), record);
    }
}

#[test]
fn state_unretained_result_and_pending_hand_or_weapon_loss_do_not_create_insertion() {
    for invalid in [0, 1, 2] {
        let mut state = state();
        let mut engine = FakeEngine {
            authority: true,
            ..FakeEngine::default()
        };
        let config = ReloadConfig::new();
        state.extract_with(&tick(0.0, 17), &inputs(true, true), &config, &mut engine);
        state.extract_with(&tick(0.1, 17), &inputs(false, true), &config, &mut engine);
        engine.result = Some(DetachOutcome::DroppedUnretained);
        let mut current = tick(0.1, 17);
        if invalid == 1 {
            current.hands[0] = None;
        }
        if invalid == 2 {
            current.guns[1] = None;
        }
        state.extract_with(&current, &inputs(false, true), &config, &mut engine);
        assert!(state.pending_detach.is_none());
        assert_eq!(state.status.insert_hands, None);
        assert!(engine.held.is_none());
        assert_eq!(state.status.counts.ejects, u64::from(invalid == 0));
        assert!(state.extraction_needs_neutral[0]);
    }
}

#[test]
fn delayed_retained_result_cannot_replace_changed_input_context() {
    for invalid in 0..5 {
        let mut state = state();
        let mut engine = FakeEngine {
            authority: true,
            ..FakeEngine::default()
        };
        let config = ReloadConfig::new();
        let key = RecordKey::Object(1);
        state.records.get(key).chambers = [Chamber::Chambered; 2];
        let record = *state.records.get(key);
        state.extract_with(&tick(0.0, 17), &inputs(true, true), &config, &mut engine);
        state.extract_with(&tick(0.1, 17), &inputs(false, true), &config, &mut engine);
        assert_eq!(engine.submitted.len(), 1);
        let magazine = loose::Loose::test(
            NonZeroUsize::new(1).unwrap(),
            1,
            NonZeroUsize::new(1).unwrap(),
            2,
            17,
        );
        engine.result = Some(DetachOutcome::Retained(magazine));
        let mut current = tick(0.1, 0);
        current.weapons[0].as_mut().unwrap().reading.out = true;
        match invalid {
            0 => state.frames = None,
            1 => current.world_scale = 2.0,
            2 => state.free = None,
            3 => {
                state.status.insertion.stage = InsertStage::Holding { elapsed_s: 0.0 };
                state.status.insertion.taken += 1;
                state.status.insert_hands = Some((Hand::Right, Hand::Left));
            }
            4 => state.frames.as_mut().unwrap().world_scale = 2.0,
            _ => unreachable!(),
        }
        let insertion = state.status.insertion;
        let owners = state.status.insert_hands;
        state.extract_with(&current, &inputs(false, true), &config, &mut engine);
        state.extract_with(&current, &inputs(false, true), &config, &mut engine);
        assert!(state.pending_detach.is_none());
        assert_eq!(state.status.insertion, insertion);
        assert_eq!(state.status.insert_hands, owners);
        assert!(engine.held.is_none());
        assert_eq!(engine.canceled, 1);
        assert_eq!(engine.delivered, 0);
        assert!(engine.result.is_none());
        assert_eq!(state.status.counts.ejects, 0);
        assert_eq!(*state.records.get(key), record);
        assert_eq!(current.weapons[0].unwrap().reserve, 70);
    }
}

#[test]
fn state_active_pull_cancels_fresh_and_already_pushing_belt_without_engine_work() {
    for stage in [PushStage::Outside, PushStage::Pushing] {
        let mut state = state();
        let mut engine = FakeEngine {
            authority: true,
            ..FakeEngine::default()
        };
        let config = ControllersConfig::default();
        let tick = tick(0.0, 17);
        state.extract_with(
            &tick,
            &inputs(true, true),
            &config.weapons.reload,
            &mut engine,
        );
        let record = *state.records.get(RecordKey::Object(1));
        state.pouch = Some((Vec3::ZERO, 0.1));
        state.belts[Hand::Right.index()].stage = stage;
        state.belt(&tick, &config, 0.01);
        assert_eq!(state.belts[Hand::Right.index()].stage, PushStage::Outside);
        assert_eq!(state.status.counts.belt_reloads, 0);
        assert_eq!(*state.records.get(RecordKey::Object(1)), record);
        assert_eq!(state.extraction_hand(), Some(Hand::Left));
    }
}
