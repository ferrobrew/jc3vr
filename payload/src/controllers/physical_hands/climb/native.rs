//! Native attachment gate backed by serialized body validation and bounded immutable proofs.
//!
//! Workers read only their caller-owned character and atomic snapshots. A proof does not pin any
//! engine allocation: destruction stamps invalidate it, observed player/world changes advance its
//! generation, and unobserved lifecycle changes can conservatively deny attachment only until expiry.

use std::{
    cell::Cell,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use arc_swap::ArcSwapOption;
use jc3gi::character::character::Character;

use crate::controllers::{
    physical_hands::climb::{
        Hold,
        engine::{self, BodyIdentity},
        lifetime,
        state::STALE_AFTER,
        types::Landing,
    },
    roomscale::teleport,
};

/// Copied only at the serialized input/pre-simulation boundary, never looked up by a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub character: usize,
    pub proxy: usize,
    pub world: usize,
    pub bodies: usize,
    pub epoch: u64,
}

pub(crate) struct LandingOffer<'a> {
    gate: &'a Gate,
    claim: Arc<Claim>,
    pub landing: Landing,
}

impl Drop for LandingOffer<'_> {
    fn drop(&mut self) {
        self.gate.clear_exact(&self.claim);
    }
}

/// Update lifecycle ownership from a serialized live-world observation.
pub(crate) fn observe(context: Option<Context>) {
    GATE.observe(context);
}

/// Publish only identities already validated by the serialized pre-simulation step.
pub(crate) unsafe fn hold(context: Context, grips: [Option<Hold>; 2], now: Instant) {
    GATE.validated(context, grips, Kind::Grips, now, |body| unsafe {
        engine::body_identity(body)
    });
}

pub(crate) unsafe fn landed(context: Context, hold: Hold, landing: Landing, now: Instant) {
    GATE.validated(
        context,
        [Some(hold), None],
        Kind::Landing(landing),
        now,
        |body| unsafe { engine::body_identity(body) },
    );
}

/// End physical locomotion without discarding a completed mantle awaiting input handoff.
pub(crate) fn finish_grips(now: Instant) {
    GATE.finish_grips(now);
}

pub(crate) fn clear() {
    GATE.observe(None);
}

pub(crate) fn reject_landing() {
    GATE.reject_landing();
}

pub(crate) fn take_landing(
    now: Instant,
    valid: impl FnOnce(Landing) -> bool,
) -> Option<LandingOffer<'static>> {
    GATE.take_landing(now, valid)
}

/// Read no singleton, world, body storage, property buffer, instance, or owner on this caller.
///
/// # Safety
///
/// The native caller keeps `character` and its proxy member alive for this call, as required by
/// the original attachment function. Only members of that character are read here.
pub(crate) unsafe fn blocks(character: *const Character) -> bool {
    let Some(current) = (unsafe { character.as_ref() }) else {
        return false;
    };
    let caller = Caller {
        character: character as usize,
        proxy: current.m_PfxCharacter.px as usize,
        local: current.m_IsLocalCharacter,
        epoch: teleport::epoch(),
    };
    if DELIBERATE.with(|owner| owner.get() == caller.character && owner.get() != 0) {
        return false;
    }
    GATE.blocks(caller, Instant::now(), lifetime::stamp)
}

/// Permit only the synchronous, owner-checked mantle entry on the calling thread.
pub(crate) fn deliberate<R>(character: *const Character, enter: impl FnOnce() -> R) -> R {
    let previous = DELIBERATE.with(|owner| owner.replace(character as usize));
    let _restore = Restore(previous);
    enter()
}

struct Restore(usize);
impl Drop for Restore {
    fn drop(&mut self) {
        DELIBERATE.with(|owner| owner.set(self.0));
    }
}

thread_local! {
    static DELIBERATE: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Copy)]
struct Caller {
    character: usize,
    proxy: usize,
    local: bool,
    epoch: u64,
}

struct Observed {
    context: Context,
    generation: u64,
}

#[derive(Clone, Copy)]
enum Kind {
    Grips,
    Landing(Landing),
}

struct Claim {
    context: Context,
    generation: u64,
    at: Instant,
    proofs: [Option<Hold>; 2],
    kind: Kind,
}

struct Gate {
    context: ArcSwapOption<Observed>,
    claim: ArcSwapOption<Claim>,
    generation: AtomicU64,
}

impl Gate {
    fn new() -> Self {
        Self {
            context: ArcSwapOption::empty(),
            claim: ArcSwapOption::empty(),
            generation: AtomicU64::new(0),
        }
    }

    fn observe(&self, context: Option<Context>) {
        let previous = self.context.load_full();
        if previous.as_ref().map(|c| c.context) == context {
            return;
        }
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.claim.store(None);
        self.context.store(context.map(|context| {
            Arc::new(Observed {
                context,
                generation,
            })
        }));
    }

    fn validated(
        &self,
        context: Context,
        holds: [Option<Hold>; 2],
        kind: Kind,
        now: Instant,
        mut identity: impl FnMut(u32) -> Option<BodyIdentity>,
    ) {
        let proofs = holds.map(|hold| hold.filter(|h| identity(h.body) == Some(h.identity)));
        self.publish(context, proofs, kind, now);
    }

    fn publish(&self, context: Context, proofs: [Option<Hold>; 2], kind: Kind, now: Instant) {
        let observed = self.context.load_full();
        let Some(observed) = observed.filter(|o| o.context == context) else {
            return;
        };
        let landing_matches = match kind {
            Kind::Grips => true,
            Kind::Landing(landing) => {
                landing.character == context.character
                    && landing.proxy == context.proxy
                    && landing.epoch == context.epoch
                    && proofs
                        .iter()
                        .flatten()
                        .any(|h| h.body == landing.body && h.identity == landing.identity)
            }
        };
        if !landing_matches
            || !proofs
                .iter()
                .flatten()
                .all(|h| h.identity.world == context.world && h.identity.bodies == context.bodies)
            || proofs.iter().all(Option::is_none)
        {
            self.claim.store(None);
            return;
        }
        self.claim.store(Some(Arc::new(Claim {
            context,
            generation: observed.generation,
            at: now,
            proofs,
            kind,
        })));
    }

    fn current(&self, claim: &Claim, now: Instant) -> bool {
        let observed = self.context.load();
        now.saturating_duration_since(claim.at) <= STALE_AFTER
            && claim.generation == self.generation.load(Ordering::Acquire)
            && observed
                .as_ref()
                .is_some_and(|o| o.generation == claim.generation && o.context == claim.context)
    }

    fn blocks(&self, caller: Caller, now: Instant, stamp: impl Fn(usize, u32) -> u64) -> bool {
        let Some(claim) = self.claim.load_full() else {
            return false;
        };
        caller.local
            && caller.character == claim.context.character
            && caller.proxy == claim.context.proxy
            && caller.epoch == claim.context.epoch
            && self.current(&claim, now)
            && claim
                .proofs
                .iter()
                .flatten()
                .any(|h| stamp(h.identity.world, h.body) == h.identity.lifetime)
            && claim.generation == self.generation.load(Ordering::Acquire)
    }

    fn clear_exact(&self, claim: &Arc<Claim>) {
        self.claim.compare_and_swap(&Some(Arc::clone(claim)), None);
    }

    fn finish_grips(&self, now: Instant) {
        if let Some(claim) = self.claim.load_full()
            && (matches!(claim.kind, Kind::Grips) || !self.current(&claim, now))
        {
            self.clear_exact(&claim);
        }
    }

    fn reject_landing(&self) {
        if let Some(claim) = self.claim.load_full()
            && matches!(claim.kind, Kind::Landing(_))
        {
            self.clear_exact(&claim);
        }
    }

    fn take_landing(
        &self,
        now: Instant,
        valid: impl FnOnce(Landing) -> bool,
    ) -> Option<LandingOffer<'_>> {
        let claim = self.claim.load_full()?;
        let Kind::Landing(landing) = claim.kind else {
            return None;
        };
        if !self.current(&claim, now) || !valid(landing) {
            self.clear_exact(&claim);
            return None;
        }
        Some(LandingOffer {
            gate: self,
            claim,
            landing,
        })
    }
}

static GATE: LazyLock<Gate> = LazyLock::new(Gate::new);

#[cfg(test)]
mod tests;
