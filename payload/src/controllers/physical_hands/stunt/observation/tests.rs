use std::{
    cell::Cell,
    ptr::{self, NonNull},
};

use jc3gi::{
    animation::rule_system::{AnimationRuleSystem, State, StateMachineInstance},
    types::shared_ptr::SharedPtr,
};

use super::*;

#[test]
fn every_missing_readiness_link_skips_native_dispatch_and_recovery_works_with_assistance_off() {
    let mut character: Character = unsafe { std::mem::zeroed() };
    let mut rule: AnimationRuleSystem = unsafe { std::mem::zeroed() };
    let mut machine: StateMachineInstance = unsafe { std::mem::zeroed() };
    let mut state: State = unsafe { std::mem::zeroed() };
    let mut rules: [SharedPtr<AnimationRuleSystem>; 1] = unsafe { std::mem::zeroed() };
    let calls = Cell::new(0);
    let native = |_: &Character| {
        calls.set(calls.get() + 1);
        PositionRead {
            kind: None,
            owns_movement: false,
            vehicle_speed_mps: 0.0,
        }
    };
    for missing in [
        "control", "model", "rules", "rule", "machine", "state", "none",
    ] {
        character.m_ControlParameters = if missing == "control" {
            ptr::null_mut()
        } else {
            NonNull::dangling().as_ptr()
        };
        character.m_AnimatedModel.m_InstanceData = if missing == "model" {
            ptr::null_mut()
        } else {
            NonNull::dangling().as_ptr()
        };
        rules[0].px = if missing == "rule" {
            ptr::null_mut()
        } else {
            &mut rule
        };
        let begin = rules.as_mut_ptr();
        character.m_AnimatedModel.m_RuleSystems.begin = begin;
        character.m_AnimatedModel.m_RuleSystems.end =
            unsafe { begin.add(usize::from(missing != "rules")) };
        rule.m_StateMachineInstance.px = if missing == "machine" {
            ptr::null_mut()
        } else {
            &mut machine
        };
        machine.m_CurrentState = if missing == "state" {
            ptr::null_mut()
        } else {
            &mut state
        };
        let read = unsafe { read_ready(&character, native) };
        let mut presentation = crate::controllers::physical_hands::stunt::State {
            tracker: crate::controllers::physical_hands::stunt::solve::Tracker::new(),
            ticked: None,
            status: crate::controllers::physical_hands::stunt::StuntStatus::default(),
        };
        presentation.observe_read(read, false);
        if missing == "none" {
            assert!(!read.unwrap().owns_movement);
            assert_eq!(calls.get(), 1);
        } else {
            assert!(read.is_none(), "{missing}");
            assert_eq!(calls.get(), 0, "{missing}");
        }
    }
}

fn context() -> Context {
    Context {
        character: 1,
        proxy: 2,
        world: 3,
        bodies: 4,
        epoch: 5,
    }
}

#[test]
fn only_fresh_matching_explicit_not_owned_observations_admit_movement() {
    let now = Instant::now();
    for owner in [
        MovementOwner::ObservedKnown(false),
        MovementOwner::ObservedKnown(true),
        MovementOwner::Unavailable,
    ] {
        let observation = Observation {
            owner,
            context: Some(context()),
            at: now,
        };
        assert_eq!(
            observation.current(Some(context()), now).unwrap().yields(),
            owner != MovementOwner::ObservedKnown(false)
        );
        assert_eq!(observation.current(None, now), None);
        for field in 0..5 {
            let mut replaced = context();
            match field {
                0 => replaced.character += 1,
                1 => replaced.proxy += 1,
                2 => replaced.world += 1,
                3 => replaced.bodies += 1,
                _ => replaced.epoch += 1,
            }
            assert_eq!(observation.current(Some(replaced), now), None);
        }
        assert_eq!(
            observation.current(Some(context()), now + STALE_AFTER + Duration::from_nanos(1)),
            None
        );
    }
    assert!(MovementOwner::Unavailable.yields());
}
