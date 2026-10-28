use std::{cell::Cell, time::Duration};

use glam::Vec3;

use super::*;
use crate::controllers::physical_hands::climb::engine::BodyIdentity;

fn context() -> Context {
    Context {
        character: 1,
        proxy: 2,
        world: 3,
        bodies: 4,
        epoch: 5,
    }
}

fn hold(body: u32) -> Hold {
    Hold {
        point: Vec3::Y,
        normal: Vec3::Y,
        body,
        moving: true,
        identity: BodyIdentity {
            world: 3,
            bodies: 4,
            instance: 6,
            owner: 7,
            lifetime: 8,
            ..Default::default()
        },
    }
}

fn caller() -> Caller {
    Caller {
        character: 1,
        proxy: 2,
        local: true,
        epoch: 5,
    }
}

fn landing(owner: Hold) -> Landing {
    Landing {
        body: owner.body,
        identity: owner.identity,
        character: 1,
        proxy: 2,
        epoch: 5,
        feet: Vec3::ZERO,
    }
}

fn pending(gate: &Gate, at: Instant) {
    gate.observe(Some(context()));
    gate.publish(
        context(),
        [Some(hold(10)), None],
        Kind::Landing(landing(hold(10))),
        at,
    );
}

#[test]
fn worker_adapter_uses_only_caller_proof_and_atomic_stamps() {
    let gate = Gate::new();
    let now = Instant::now();
    gate.observe(Some(context()));
    // Serialized validation rejects the recycled identity before it can become a proof.
    gate.validated(context(), [Some(hold(10)), None], Kind::Grips, now, |_| {
        None
    });
    assert!(!gate.blocks(caller(), now, |_, _| 8));
    gate.validated(context(), [Some(hold(10)), None], Kind::Grips, now, |_| {
        Some(BodyIdentity {
            owner: 90,
            ..hold(10).identity
        })
    });
    assert!(!gate.blocks(caller(), now, |_, _| 8));
    gate.validated(context(), [Some(hold(10)), None], Kind::Grips, now, |_| {
        Some(hold(10).identity)
    });
    let reads = Cell::new(0);
    assert!(gate.blocks(caller(), now, |world, body| {
        assert_eq!((world, body), (3, 10));
        reads.set(reads.get() + 1);
        8
    }));
    assert_eq!(reads.get(), 1);
    assert!(!gate.blocks(caller(), now, |_, _| 9));
    for wrong in [
        Caller {
            local: false,
            ..caller()
        },
        Caller {
            character: 9,
            ..caller()
        },
        Caller {
            proxy: 9,
            ..caller()
        },
        Caller {
            epoch: 9,
            ..caller()
        },
    ] {
        assert!(!gate.blocks(wrong, now, |_, _| panic!(
            "unrelated caller must not read even an atomic body stamp"
        )));
    }
    assert!(!gate.blocks(
        caller(),
        now + STALE_AFTER + Duration::from_nanos(1),
        |_, _| panic!("expired proof must not read a stamp")
    ));
}

#[test]
fn context_changes_and_clear_prevent_resurrecting_old_claims() {
    let gate = Gate::new();
    let now = Instant::now();
    for next in [
        None,
        Some(Context {
            world: 30,
            ..context()
        }),
        Some(Context {
            bodies: 40,
            ..context()
        }),
        Some(Context {
            character: 10,
            ..context()
        }),
        Some(Context {
            proxy: 20,
            ..context()
        }),
        Some(Context {
            epoch: 50,
            ..context()
        }),
    ] {
        pending(&gate, now);
        gate.observe(next);
        assert!(!gate.blocks(caller(), now, |_, _| 8));
        gate.observe(Some(context()));
        assert!(!gate.blocks(caller(), now, |_, _| 8));
    }
}

#[test]
fn pending_landing_survives_extra_no_grip_steps_until_handoff_reject_consume_or_expiry() {
    let gate = Gate::new();
    let at = Instant::now();
    pending(&gate, at);
    let mut state = crate::controllers::physical_hands::climb::state::State::new();
    for millis in [10, 20, 40] {
        state.finish_climb_with(at + Duration::from_millis(millis), |now| {
            gate.finish_grips(now)
        });
        assert!(gate.blocks(caller(), at + Duration::from_millis(millis), |_, _| 8));
    }
    let offer = gate
        .take_landing(at + Duration::from_millis(50), |l| {
            l.identity == hold(10).identity
        })
        .unwrap();
    assert_eq!(offer.landing.body, 10);
    assert!(gate.blocks(caller(), at + Duration::from_millis(50), |_, _| 8));
    drop(offer); // The production stunt tick drops this after success or rejection.
    assert!(!gate.blocks(caller(), at + Duration::from_millis(51), |_, _| 8));
    assert!(gate.take_landing(at, |_| true).is_none());
    pending(&gate, at);
    gate.reject_landing();
    assert!(!gate.blocks(caller(), at, |_, _| 8));

    pending(&gate, at);
    assert!(gate.take_landing(at, |_| false).is_none());
    assert!(!gate.blocks(caller(), at, |_, _| 8));

    pending(&gate, at);
    let expired = at + STALE_AFTER + Duration::from_nanos(1);
    gate.finish_grips(expired);
    assert!(
        gate.take_landing(expired, |_| panic!(
            "expired landing must not read live identity"
        ))
        .is_none()
    );
    assert!(!gate.blocks(caller(), expired, |_, _| 8));
}

#[test]
fn dropping_an_old_offer_cannot_clear_a_new_grip_claim() {
    let gate = Gate::new();
    let at = Instant::now();
    pending(&gate, at);
    let offer = gate.take_landing(at, |_| true).unwrap();
    gate.publish(context(), [Some(hold(11)), None], Kind::Grips, at);
    drop(offer);
    assert!(gate.blocks(caller(), at, |_, body| {
        assert_eq!(body, 11);
        8
    }));
    gate.reject_landing();
    assert!(gate.blocks(caller(), at, |_, _| 8));
    gate.finish_grips(at);
    assert!(!gate.blocks(caller(), at, |_, _| 8));
}

#[test]
fn deliberate_bypass_restores_its_owner() {
    let owners = [0u8; 2];
    let owner = std::ptr::from_ref(&owners[0]).cast::<Character>();
    let other = std::ptr::from_ref(&owners[1]).cast::<Character>();
    deliberate(owner, || {
        assert_eq!(DELIBERATE.with(Cell::get), owner as usize);
        deliberate(other, || {
            assert_eq!(DELIBERATE.with(Cell::get), other as usize)
        });
        assert_eq!(DELIBERATE.with(Cell::get), owner as usize);
    });
    assert_eq!(DELIBERATE.with(Cell::get), 0);
}
