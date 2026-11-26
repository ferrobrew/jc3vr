use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::*;

struct Owner(Arc<AtomicUsize>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn state() -> State<Owner> {
    let mut authority = Authority::new();
    authority.observe(11);
    State {
        authority,
        records: [None; SLOTS],
        queued: Vec::new(),
        members_closed: None,
    }
}

fn arrival(state: &State<Owner>, drops: &Arc<AtomicUsize>) -> Arrival<Owner> {
    Arrival {
        identity: state.authority.identity(11).unwrap(),
        weapon: Owner(Arc::clone(drops)),
        slot: WeaponSlot::E_SLOT_DUAL_WIELD,
        source: GiveSource::Game,
        member: None,
    }
}

#[test]
fn pre_reset_retires_retained_owners_before_inline_and_later_callbacks() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut state = state();
    state.queued.push(arrival(&state, &drops));
    let old = arrival(&state, &drops);
    let retired = state.pre_reset();
    assert!(state.queued.is_empty());
    assert!(!state.admits(&old, 11));
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(retired);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert!(state.records.iter().all(Option::is_none));
    // Neither an inline callback nor a later same-owner respawn inherits an old record.
    assert!(!state.admits(&old, 11));
    state.authority.observe(22);
    assert!(!state.admits(&old, 11));
    assert!(!state.admits(&old, 22));
    drop(old);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}

#[test]
fn overwritten_member_keeps_closure_evidence_and_late_callback_runs_original() {
    for retire in [
        MemberClosure::Reset,
        MemberClosure::StandDown,
        MemberClosure::InventoryChanged,
    ] {
        let mut state = state();
        let slot = WeaponSlot::E_SLOT_DUAL_WIELD as usize;
        let member = Record {
            identity: state.authority.identity(11).unwrap(),
            tag_index: 7,
            source: GiveSource::RackCopy,
            member: Some(CopyRequest {
                member: Member {
                    source: 33,
                    generation: 1,
                    index: 0,
                },
                ammo: 4,
            }),
        };
        state.replace_record(slot, Some(member), MemberClosure::ReplacedGive);
        state.replace_record(
            slot,
            Some(Record {
                tag_index: 8,
                source: GiveSource::Game,
                member: None,
                ..member
            }),
            MemberClosure::ReplacedGive,
        );
        assert_eq!(state.members_closed, Some(MemberClosure::ReplacedGive));
        if retire == MemberClosure::Reset {
            drop(state.pre_reset());
        } else {
            state.retire_records(retire);
        }
        assert_eq!(state.members_closed, Some(MemberClosure::ReplacedGive));
        let mut original = 0;
        let mut staged = 0;
        for tag in [7, 8] {
            if state.claim_record(slot, tag, 11).is_some() {
                staged += 1;
            } else {
                original += 1;
            }
        }
        assert_eq!((original, staged), (2, 0));
        let ordinary = Record {
            identity: state.authority.identity(11).unwrap(),
            member: None,
            source: GiveSource::Game,
            tag_index: 9,
        };
        state.replace_record(slot, Some(ordinary), MemberClosure::ReplacedGive);
        assert_eq!(state.claim_record(slot, 9, 11), Some(ordinary));
        assert_eq!(state.members_closed, Some(MemberClosure::ReplacedGive));
    }
}

#[test]
fn reset_cannot_claim_a_normal_same_slot_tag_arrival_as_a_member() {
    let mut state = state();
    state.records[WeaponSlot::E_SLOT_DUAL_WIELD as usize] = Some(Record {
        identity: state.authority.identity(11).unwrap(),
        tag_index: 7,
        source: GiveSource::RackCopy,
        member: Some(CopyRequest {
            member: Member {
                source: 33,
                generation: 1,
                index: 0,
            },
            ammo: 4,
        }),
    });
    drop(state.pre_reset());
    assert_eq!(state.members_closed, Some(MemberClosure::Reset));
    assert!(state.records.iter().all(Option::is_none));
    // New member submissions remain excluded until native callback retirement is proven.
    drop(state.pre_reset());
    assert_eq!(state.members_closed, Some(MemberClosure::Reset));
}
