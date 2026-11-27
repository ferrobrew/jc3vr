//! The climb's refusal of the game's automatic entry into a vehicle's stunt area below the player
//! while the hands climb ([`blocks`]), and the hand-over of a mantle that landed on a moving body
//! to the stunt positions ([`take_landing`]).
//!
//! The pre-simulation step publishes a claim of the bodies the hands grip, checked against the
//! live world on its own thread ([`hold`], [`landed`]); the input tick and the step record the
//! player and world the claims belong to ([`observe`]). The refusal runs on the game's character
//! workers, so it reads only the calling character's own members and the gate's atomic snapshots.
//! A claim pins no engine allocation: a body's destruction advances its lifetime stamp
//! ([`lifetime`]), a change of player or world advances the claim's generation, and a change
//! neither catches can only refuse an entry until the claim goes stale.

use std::{
    cell::Cell,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
};

use arc_swap::ArcSwapOption;
use jc3gi::character::character::Character;
use jc3vr_body::controllers::facts::climb::BodyIdentity;
use jc3vr_core::published::Stamp;
use jc3vr_view::headpose::teleport;

use crate::controllers::physical_hands::climb::{
    Hold, engine, lifetime, tuning::CLIMB, types::Landing,
};

/// The player and world a claim belongs to, copied on the game thread by the input tick or the
/// pre-simulation step, never looked up by a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub character: usize,
    pub proxy: usize,
    pub world: usize,
    pub bodies: usize,
    pub epoch: u64,
}

pub struct LandingOffer<'a> {
    gate: &'a Gate,
    claim: Arc<Claim>,
    pub landing: Landing,
}

impl Drop for LandingOffer<'_> {
    fn drop(&mut self) {
        self.gate.clear_exact(&self.claim);
    }
}

/// Record the player and world the claims belong to; a change drops the claim and advances the
/// generation.
pub(crate) fn observe(context: Option<Context>) {
    GATE.observe(context);
}

/// Publish the grips as the claim, keeping only those whose bodies still have the identity the
/// hand took.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub(crate) unsafe fn hold(context: Context, grips: [Option<Hold>; 2], now: Stamp) {
    GATE.validated(context, grips, Kind::Grips, now, |body| unsafe {
        engine::body_identity(body)
    });
}

/// Publish a mantle's landing on the moving body `hold` as the claim, for the stunt positions to
/// take.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub(crate) unsafe fn landed(context: Context, hold: Hold, landing: Landing, now: Stamp) {
    GATE.validated(
        context,
        [Some(hold), None],
        Kind::Landing(landing),
        now,
        |body| unsafe { engine::body_identity(body) },
    );
}

/// Drop the grips' claim as the climb ends, keeping a current landing the stunt positions have
/// yet to take.
pub(crate) fn finish_grips(now: Stamp) {
    GATE.finish_grips(now);
}

pub(crate) fn clear() {
    GATE.observe(None);
}

pub(crate) fn reject_landing() {
    GATE.reject_landing();
}

pub(crate) fn take_landing(
    now: Stamp,
    valid: impl FnOnce(Landing) -> bool,
) -> Option<LandingOffer<'static>> {
    GATE.take_landing(now, valid)
}

/// Whether the climb refuses the automatic stunt-area entry of `character`: a current claim taken
/// for this local character, its proxy, and this teleport epoch, with a claimed body not destroyed
/// since. Runs on a character worker, so it reads no singleton, world, body storage, property
/// buffer, instance, or owner.
///
/// # Safety
///
/// The native caller keeps `character` and its proxy member alive for this call, as the hooked
/// attachment function requires. Only members of that character are read here.
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
    GATE.blocks(caller, Stamp::now(), lifetime::stamp)
}

/// Run `enter` with the refusal lifted for `character` on the calling thread, so that the stunt
/// positions' own entry is not refused.
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
    at: Stamp,
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
        now: Stamp,
        mut identity: impl FnMut(u32) -> Option<BodyIdentity>,
    ) {
        let proofs = holds.map(|hold| hold.filter(|h| identity(h.body) == Some(h.identity)));
        self.publish(context, proofs, kind, now);
    }

    fn publish(&self, context: Context, proofs: [Option<Hold>; 2], kind: Kind, now: Stamp) {
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

    fn current(&self, claim: &Claim, now: Stamp) -> bool {
        let observed = self.context.load();
        CLIMB.get().climb.stale_after().holds(claim.at, now)
            && claim.generation == self.generation.load(Ordering::Acquire)
            && observed
                .as_ref()
                .is_some_and(|o| o.generation == claim.generation && o.context == claim.context)
    }

    fn blocks(&self, caller: Caller, now: Stamp, stamp: impl Fn(usize, u32) -> u64) -> bool {
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

    fn finish_grips(&self, now: Stamp) {
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
        now: Stamp,
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
