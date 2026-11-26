//! The live grenades' diagnostic trail: the log lines that tell, after a session, how each grenade
//! whose pin the player pulled met its end.
//!
//! A grenade is followed from its pin ([`lit`]) through each input tick's holder ([`observe`]), its
//! release ([`released`]), the contact trigger's arming ([`contact_trigger`]), and the contacts its
//! collision handler sees, to its detonation, which is logged with its cause and where it was
//! relative to the hand that held or let go of it and to the head. The cause is what the engine
//! exposes: the contact upgrades detonate a grenade by zeroing its timer in its collision handler
//! ([`Grenade::OnCollision`]), so a contact whose handler zeroed a running timer is the contact
//! trigger; a timer that ran out on its own is the fuse; a detonation with time left is something
//! else, such as damage.
//!
//! Nothing here changes what a grenade does: the two hooks call the engine's own functions and only
//! read around them.

use std::time::Instant;

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    equipment::explosive::{Explosive, Grenade},
    physics::Collision,
};
use jc3vr_input::frame::{Hand, Pose};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::grenade::{Release, pin::PinSource, safe};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GRENADE_ON_COLLISION_BINDER)
        .with_static_binder(&GRENADE_DETONATE_BINDER)
}

/// Follow the grenade `hand` holds, whose pin was just pulled `by` the trigger, the other hand, or
/// the teeth, and log its fuse.
pub(super) fn lit(hand: Hand, by: PinSource) {
    let Some(explosive) = safe::explosive_in_hand(hand) else {
        return;
    };
    // SAFETY: the item's actor is live on the game thread for this call.
    let explosive = unsafe { &*explosive };
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        by = by.label(),
        fuse_s = explosive.m_StartDetonationTimer,
        remaining_s = explosive.m_DetonationTimer,
        "grenade: the pin is pulled, and the fuse runs",
    );
    let mut log = LOG.lock();
    let identity = Identity::of(explosive);
    log.tracked.retain(|tracked| tracked.identity != identity);
    if log.tracked.len() >= MAX_TRACKED {
        log.tracked.remove(0);
    }
    log.tracked.push(Tracked {
        identity,
        lit_at: Instant::now(),
        holder: Some(hand),
        released: None,
        contacts: 0,
        contact_fired: false,
    });
}

/// Note which hand holds each followed grenade, and where the hands and the head are, for a
/// detonation to measure against. Runs every input tick.
pub(super) fn observe(grips: [Option<Pose>; 2]) {
    let held = Hand::BOTH.map(|hand| safe::explosive_in_hand(hand).map(|e| e as usize));
    let head = jc3vr_view::hud::render_camera_pose().map(|(position, _)| position);
    let mut log = LOG.lock();
    log.grips = grips.map(|grip| grip.map(|grip| grip.position));
    log.head = head;
    let now = Instant::now();
    log.tracked
        .retain(|tracked| now.duration_since(tracked.lit_at).as_secs_f32() < FORGET_AFTER_S);
    for tracked in &mut log.tracked {
        if tracked.released.is_some() {
            continue;
        }
        tracked.holder = Hand::BOTH
            .into_iter()
            .find(|hand| held[hand.index()] == Some(tracked.identity.address));
    }
}

/// Log the release of a live grenade by `hand` (`None` for one the world let go of) with `release`,
/// through the weapon's launch (`launched`) or inertly, and from then measure its detonation
/// against that hand.
pub(super) fn released(
    hand: Option<Hand>,
    explosive: &Explosive,
    release: Release,
    launched: bool,
    contact_trigger: bool,
) {
    let speed_mps = release.velocity.length();
    tracing::info!(
        target: "controllers",
        hand = hand.map_or("none", Hand::label),
        speed_mps,
        kind = if speed_mps >= THROW_MIN_MPS { "throw" } else { "drop" },
        path = if launched { "the weapon's launch" } else { "inert" },
        contact_trigger,
        burnt_s = explosive.m_StartDetonationTimer - explosive.m_DetonationTimer,
        remaining_s = explosive.m_DetonationTimer,
        "grenade: a live grenade left the hand",
    );
    let identity = Identity::of(explosive);
    let mut log = LOG.lock();
    if let Some(tracked) = log
        .tracked
        .iter_mut()
        .find(|tracked| tracked.identity == identity)
    {
        tracked.released = Some((hand.or(tracked.holder), Instant::now()));
        tracked.holder = None;
    }
}

/// Log a live grenade's contact trigger as `why` has just left it: the contact upgrades act only
/// while the grenade's own upgrade flag is on ([`Grenade::m_CanUseUpgrades`]).
pub(super) fn contact_trigger(grenade: &Grenade, why: &'static str) {
    tracing::info!(
        target: "controllers",
        armed = grenade.m_CanUseUpgrades,
        why,
        "grenade: the contact trigger changes",
    );
}

/// How long a followed grenade is kept at most, seconds: far longer than any fuse.
const FORGET_AFTER_S: f32 = 60.0;
/// How many grenades are followed at once; the oldest gives way.
const MAX_TRACKED: usize = 8;
/// How many of a grenade's contacts are logged, besides one that fires the contact trigger.
const MAX_LOGGED_CONTACTS: u32 = 4;
/// The release speed from which the log calls a release a throw rather than a drop, metres per
/// second. It labels the line only.
const THROW_MIN_MPS: f32 = 2.0;

/// A pooled grenade's identity: its address and its allocation, so a recycled slot is not taken
/// for the grenade that held it before.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Identity {
    address: usize,
    pool_index: u32,
    instance: u32,
}

impl Identity {
    fn of(explosive: &Explosive) -> Self {
        Self {
            address: std::ptr::from_ref(explosive) as usize,
            pool_index: explosive.m_PoolIndex,
            instance: explosive.m_InstanceUniqueId,
        }
    }
}

struct Tracked {
    identity: Identity,
    lit_at: Instant,
    /// The hand holding the grenade as of the last input tick, while it is held.
    holder: Option<Hand>,
    /// The hand that let go of it, and when.
    released: Option<(Option<Hand>, Instant)>,
    contacts: u32,
    contact_fired: bool,
}

struct Log {
    tracked: Vec<Tracked>,
    grips: [Option<Vec3>; 2],
    head: Option<Vec3>,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    tracked: Vec::new(),
    grips: [None; 2],
    head: None,
});

#[detour(address = jc3gi::equipment::explosive::Grenade::OnCollision_ADDRESS)]
fn grenade_on_collision(listener: *mut std::ffi::c_void, collision: *const Collision) {
    let explosive = listener
        .cast::<u8>()
        .wrapping_sub(Explosive::PFX_BODY_LISTENER_OFFSET as usize)
        .cast::<Explosive>();
    // SAFETY: the handler's listener is the grenade's own, live for its collision handling.
    let before = unsafe { explosive.as_ref() }.map(|explosive| explosive.m_DetonationTimer);
    GRENADE_ON_COLLISION
        .get()
        .unwrap()
        .call(listener, collision);
    // SAFETY: as above.
    let (Some(explosive), Some(before)) = (unsafe { explosive.as_ref() }, before) else {
        return;
    };
    let identity = Identity::of(explosive);
    let after = explosive.m_DetonationTimer;
    let fired = before > 0.0 && after <= 0.0;
    let contacts = {
        let mut log = LOG.lock();
        let Some(tracked) = log
            .tracked
            .iter_mut()
            .find(|tracked| tracked.identity == identity)
        else {
            return;
        };
        tracked.contacts += 1;
        tracked.contact_fired |= fired;
        tracked.contacts
    };
    if contacts > MAX_LOGGED_CONTACTS && !fired {
        return;
    }
    // SAFETY: the engine's own contact report, live for the handler.
    let Some(collision) = (unsafe { collision.as_ref() }) else {
        return;
    };
    tracing::info!(
        target: "controllers",
        contact = contacts,
        touched = if collision.m_GB.is_null() { "the static world" } else { "an object" },
        object = collision.m_GB as usize,
        material = collision.m_MatB,
        separation_mps = collision.m_SeparationVelocity,
        impulse = collision.m_Impulse,
        remaining_s = after,
        fired,
        "grenade: a live grenade touched something",
    );
}

#[detour(address = jc3gi::equipment::explosive::Grenade::Detonate_ADDRESS)]
fn grenade_detonate(grenade: *mut Grenade, dt: f32) {
    // SAFETY: the engine detonates a live grenade; it is read before the detonation removes it.
    if let Some(explosive) = unsafe { grenade.cast::<Explosive>().as_ref() } {
        report_detonation(explosive);
    }
    GRENADE_DETONATE.get().unwrap().call(grenade, dt);
}

fn report_detonation(explosive: &Explosive) {
    let identity = Identity::of(explosive);
    let (tracked, grips, head) = {
        let mut log = LOG.lock();
        let Some(index) = log
            .tracked
            .iter()
            .position(|tracked| tracked.identity == identity)
        else {
            return;
        };
        (log.tracked.swap_remove(index), log.grips, log.head)
    };
    let remaining_s = explosive.m_DetonationTimer;
    let cause = match (tracked.contact_fired, remaining_s <= 0.0) {
        (true, _) => "the contact trigger",
        (false, true) => "the fuse",
        (false, false) => "other, with time left on the fuse",
    };
    let position = Mat4::from(explosive.m_WorldMatrixT1).w_axis.truncate();
    let (hand, relation, since_release_s) = match (tracked.holder, tracked.released) {
        (Some(hand), _) => (Some(hand), "in the hand", None),
        (None, Some((hand, at))) => (hand, "let go", Some(at.elapsed().as_secs_f32())),
        (None, None) => (None, "out of the hands", None),
    };
    let from_hand_m = hand
        .and_then(|hand| grips[hand.index()])
        .map(|grip| position.distance(grip));
    tracing::info!(
        target: "controllers",
        cause,
        relation,
        hand = hand.map_or("none", Hand::label),
        from_hand_m,
        from_head_m = head.map(|head| position.distance(head)),
        since_pin_s = tracked.lit_at.elapsed().as_secs_f32(),
        since_release_s,
        burnt_s = explosive.m_StartDetonationTimer - remaining_s,
        remaining_s,
        contacts = tracked.contacts,
        "grenade: a live grenade detonated",
    );
}
