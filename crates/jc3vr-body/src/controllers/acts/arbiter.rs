//! The arbiter's engine side: the filter both act-queue detours ask, and the flush that queues the
//! acts the owners submit.

use std::{cell::RefCell, sync::OnceLock};

use jc3gi::{
    animation::symbol_table::EventIdSymbolTable,
    character::character::{AnimatedModel, Character},
};

use crate::{
    controllers::{
        acts::claim::{
            ActAction, ActClaim, ActDecision, ActId, Entry, Guard, Owner, first_claim, rank, settle,
        },
        aim_state,
        locomotion::{acts as locomotion_acts, config::LOCOMOTION_CONFIG},
        scheme,
    },
    hooks::{character::resolve_act_id, input::wingsuit},
};

/// Decide an act queued on the local player at [`Character::QueueAct`]: the character entry's
/// filters in precedence order, with a replacement's pre-flight.
///
/// # Safety
///
/// `character` must be the live local character, on the thread queuing its act.
pub(crate) unsafe fn filter_character(character: &mut Character, act: ActId) -> ActDecision {
    let claim = first_claim(Entry::Character, |owner| match owner {
        Owner::VehicleReverse => reverse_act_ids()
            .is_some_and(|ids| ids.contains(&act))
            .then_some(drop(owner, act)),
        Owner::MentalState => aim_state::swallow_local_act(act).then_some(drop(owner, act)),
        Owner::Locomotion => {
            let locomotion = *LOCOMOTION_CONFIG.lock();
            // SAFETY: the caller's contract.
            unsafe { locomotion_acts::claim(character, act, &locomotion, scheme::enabled()) }
        }
        Owner::WingsuitEvade | Owner::WeaponReconcile | Owner::SlideStart => None,
    });
    let decision = settle(claim, |with| {
        // SAFETY: the pre-flight only reads the animation state machine, on the thread that is
        // queuing the act.
        unsafe { character.m_AnimatedModel.TryAct(&(with as u32)) }
    });
    if let Some(ActClaim {
        owner: Owner::Locomotion,
        action: ActAction::Replace { .. },
        ..
    }) = claim
    {
        locomotion_acts::note_replacement(decision);
    }
    decision
}

/// Decide an act queued at [`AnimatedModel::QueueAct`], for any model: the model entry's filters,
/// each of which turns away a model other than the local player's.
pub(crate) fn filter_model(model: *const AnimatedModel, act: ActId) -> ActDecision {
    let claim = first_claim(Entry::Model, |owner| match owner {
        Owner::WingsuitEvade => wingsuit::drops_act(model, act).then_some(drop(owner, act)),
        Owner::VehicleReverse
        | Owner::MentalState
        | Owner::Locomotion
        | Owner::WeaponReconcile
        | Owner::SlideStart => None,
    });
    settle(claim, |_| false)
}

/// Submit an act for the local player, queued at the next [`flush`] on this thread.
pub fn submit(owner: Owner, act: ActId, guard: Guard) {
    PENDING.with(|pending| {
        pending.borrow_mut().push(ActClaim {
            owner,
            act,
            action: ActAction::Queue(guard),
        })
    });
}

/// What a flush did with the submitted acts.
#[derive(Debug, Default)]
pub struct Flushed {
    queued: Vec<ActClaim>,
}

impl Flushed {
    /// Whether an act `owner` submitted was queued.
    pub fn queued(&self, owner: Owner) -> bool {
        self.queued.iter().any(|claim| claim.owner == owner)
    }
}

/// Queue the acts submitted on this thread on `character`, in precedence order, through
/// [`Character::QueueAct`], so the character entry's filters see them as they see the game's. A
/// guarded act is queued only when the state machine takes it. Every submitted act expires here,
/// queued or not, and none is queued on a character that is not the local player. The only code
/// that queues an act on, or pre-flights one for, the local player.
///
/// # Safety
///
/// `character` must be a live character, on the thread that queues its acts.
pub unsafe fn flush(character: &mut Character) -> Flushed {
    let mut claims = PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    let mut flushed = Flushed::default();
    if !character.m_IsLocalCharacter {
        return flushed;
    }
    claims.sort_by_key(|claim| rank(claim.owner));
    for claim in claims {
        let ActAction::Queue(guard) = claim.action else {
            continue;
        };
        let act = claim.act as u32;
        // SAFETY: the caller's contract; the pre-flight only reads the animation state machine.
        if guard == Guard::PreFlight && !unsafe { character.m_AnimatedModel.TryAct(&act) } {
            continue;
        }
        // SAFETY: the caller's contract.
        unsafe { character.QueueAct(&act) };
        flushed.queued.push(claim);
    }
    flushed
}

const fn drop(owner: Owner, act: ActId) -> ActClaim {
    ActClaim {
        owner,
        act,
        action: ActAction::Drop,
    }
}

/// The runtime ids of the reversing acts (`ACT_REVERSE` / `ACT_REVERSE_MOTORBIKE`), which drive the
/// `S_REVERSE_*` states in `rico_base.afsmb`, where Rico turns to look over his shoulder. With a
/// player-driven head, looking behind is the player's job, so the acts are dropped and the driving
/// pose stays forward; the state machine's rule system drops the matching return transitions on its
/// own (they only fire from the reverse states). Act ids are sequential registration indices, not
/// name hashes, so they are resolved the first time the event-id symbol table is live and cached
/// thereafter; `None` until then, and the drop does not fire until it is (the acts are registered by
/// loaded animation data before gameplay queues them).
fn reverse_act_ids() -> Option<[ActId; 2]> {
    if let Some(ids) = REVERSE_ACT_IDS.get() {
        return Some(*ids);
    }
    let ids = [
        resolve_act_id(EventIdSymbolTable::ACT_REVERSE)?,
        resolve_act_id(EventIdSymbolTable::ACT_REVERSE_MOTORBIKE)?,
    ];
    Some(*REVERSE_ACT_IDS.get_or_init(|| ids))
}

static REVERSE_ACT_IDS: OnceLock<[ActId; 2]> = OnceLock::new();

thread_local! {
    /// The acts submitted on this thread since its last flush. Per thread, so a flush queues only
    /// what its own thread submitted.
    static PENDING: RefCell<Vec<ActClaim>> = const { RefCell::new(Vec::new()) };
}
