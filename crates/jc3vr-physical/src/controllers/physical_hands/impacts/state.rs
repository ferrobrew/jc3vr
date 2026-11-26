//! Owned hold generations, event leases, and feedback planning without native calls.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::physics::{hknpBody, hknpWorld};
use jc3vr_core::published::Stamp;

use crate::controllers::physical_hands::{
    characters::tuning::{CHARACTERS, ImpactsTuning},
    impacts::{
        Status,
        policy::{Gate, Motion, damage_impulse, strength},
    },
};

pub(super) type Held = (u32, NonZeroUsize);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Identity {
    pub world: usize,
    pub body: u32,
    pub owner: NonZeroUsize,
    pub instance: usize,
    pub control: usize,
}

#[derive(Copy, Clone)]
pub(super) struct Observation {
    pub identity: Identity,
    pub flags: u32,
    pub motion: Motion,
}

#[derive(Copy, Clone)]
pub(super) struct Lease {
    pub identity: Identity,
    native_flag: bool,
    pub motion: Motion,
    holders: [Option<u64>; 2],
    pub armed: bool,
}

impl Lease {
    pub fn contact(self, at: Vec3, normal: Vec3, impulse: f32, character: bool) -> Contact {
        Contact {
            identity: self.identity,
            holders: self.holders,
            motion: self.motion,
            target: None,
            target_motion: None,
            retention: 0,
            at,
            normal,
            impulse,
            character,
        }
    }
}

#[derive(Copy, Clone)]
pub(super) struct Contact {
    pub identity: Identity,
    holders: [Option<u64>; 2],
    pub motion: Motion,
    pub target: Option<Identity>,
    pub target_motion: Option<Motion>,
    pub retention: u64,
    pub at: Vec3,
    pub normal: Vec3,
    pub impulse: f32,
    pub character: bool,
}

pub(super) struct Feedback {
    pub contact: Contact,
    pub share: f32,
    pub hands: [bool; 2],
    pub damage_impulse: Option<f32>,
    pub native: [bool; 2],
}

/// One original world flag call, including the caller's cache-rebuild mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct FlagCall {
    pub world: usize,
    pub body: u32,
    pub flags: u32,
    pub mode: i32,
    pub enable: bool,
}

#[derive(Debug, Copy, Clone)]
pub(super) struct Enabled {
    pub controllers: bool,
    pub gameplay: bool,
    pub running: bool,
}

impl Enabled {
    pub fn active(self) -> bool {
        self.controllers && self.gameplay && self.running
    }
}

pub(super) struct State {
    published: Option<Stamp>,
    wanted: [Option<Held>; 2],
    generations: [u64; 2],
    pub leases: [Option<Lease>; 2],
    queue: Vec<(Contact, Stamp)>,
    recent_contacts: Vec<(Contact, Stamp)>,
    gate: Gate,
    damage_gate: Vec<([Identity; 2], f64)>,
    native: Vec<([Identity; 2], Identity, Stamp)>,
    start: Option<Stamp>,
    pub status: Status,
}

impl State {
    pub const fn new() -> Self {
        Self {
            published: None,
            wanted: [None; 2],
            generations: [0; 2],
            leases: [None; 2],
            queue: Vec::new(),
            recent_contacts: Vec::new(),
            gate: Gate { last: [None; 2] },
            damage_gate: Vec::new(),
            native: Vec::new(),
            start: None,
            status: Status {
                listening: 0,
                contacts: 0,
                feedback: 0,
                suppressed: 0,
                dropped: 0,
            },
        }
    }

    pub fn publish(&mut self, held: [Option<Held>; 2], now: Stamp) {
        let tuning = &CHARACTERS.get().impacts;
        let stale = !self.fresh(now, tuning);
        for (i, wanted) in held.iter().enumerate() {
            if self.wanted[i] != *wanted || stale {
                self.generations[i] = self.generations[i]
                    .checked_add(1)
                    .expect("held-impact generation exhausted");
            }
        }
        self.wanted = held;
        self.published = Some(now);
        self.prune_contacts(now, tuning);
    }

    pub fn wanted(&self, now: Stamp, enabled: Enabled) -> [Option<Held>; 2] {
        self.wanted_in(now, enabled, &CHARACTERS.get().impacts)
    }

    fn wanted_in(&self, now: Stamp, enabled: Enabled, tuning: &ImpactsTuning) -> [Option<Held>; 2] {
        if enabled.active() && self.fresh(now, tuning) {
            self.wanted
        } else {
            [None; 2]
        }
    }

    /// Keep and acquire leases from live observations, and plan only necessary native flag changes.
    pub fn plan_step(
        &mut self,
        now: Stamp,
        enabled: Enabled,
        observations: &[Observation],
    ) -> Vec<FlagCall> {
        let tuning = &CHARACTERS.get().impacts;
        if !enabled.active() {
            self.clear(now);
        }
        let wanted = self.wanted_in(now, enabled, tuning);
        let mut changes = Vec::new();
        for slot in &mut self.leases {
            let Some(lease) = slot else {
                continue;
            };
            let observed = observations.iter().find(|o| o.identity == lease.identity);
            if wanted.contains(&Some((lease.identity.body, lease.identity.owner)))
                && observed.is_some()
            {
                lease.armed = true;
            } else if let Some(lease) = slot.take()
                && observed.is_some()
                && !lease.native_flag
            {
                changes.push(flag_change(lease.identity, false));
            }
        }
        for held in wanted.into_iter().flatten() {
            let Some(observed) = observations
                .iter()
                .find(|o| (o.identity.body, o.identity.owner) == held)
            else {
                continue;
            };
            if self
                .leases
                .iter()
                .flatten()
                .any(|l| l.identity == observed.identity)
            {
                continue;
            }
            let Some(slot) = self.leases.iter_mut().find(|l| l.is_none()) else {
                break;
            };
            let native_flag = observed.flags & hknpBody::RAISE_CONTACT_IMPULSE_EVENTS != 0;
            *slot = Some(Lease {
                identity: observed.identity,
                native_flag,
                motion: observed.motion,
                holders: [None; 2],
                armed: false,
            });
            if !native_flag {
                changes.push(flag_change(observed.identity, true));
            }
        }
        for lease in self.leases.iter_mut().flatten() {
            lease.holders = [0, 1].map(|i| {
                (wanted[i] == Some((lease.identity.body, lease.identity.owner)))
                    .then_some(self.generations[i])
            });
            if let Some(observed) = observations.iter().find(|o| o.identity == lease.identity) {
                lease.motion = observed.motion;
            }
        }
        self.prune_contacts(now, tuning);
        self.status.listening = self.leases.iter().flatten().count();
        changes
    }

    /// Preserve unrelated bits and cache mode while retaining the leased event bit.
    pub fn intercept_flags(&mut self, mut call: FlagCall, identity: Option<Identity>) -> FlagCall {
        if call.flags & hknpBody::RAISE_CONTACT_IMPULSE_EVENTS != 0
            && let Some(lease) = self.leases.iter_mut().flatten().find(|l| {
                Some(l.identity) == identity
                    && l.identity.world == call.world
                    && l.identity.body == call.body
            })
        {
            lease.native_flag = call.enable;
            if !call.enable {
                call.flags &= !hknpBody::RAISE_CONTACT_IMPULSE_EVENTS;
            }
        }
        call
    }

    pub fn queue(&mut self, contact: Contact, now: Stamp) {
        let tuning = &CHARACTERS.get().impacts;
        self.prune_contacts(now, tuning);
        if !contact.character
            && contact.impulse.is_finite()
            && contact.impulse > 0.0
            && contact.at.is_finite()
            && contact.normal.is_finite()
            && contact.normal.try_normalize().is_some()
            && self.contact_live(contact, now, tuning)
        {
            if self.recent_contacts.len() >= tuning.max_recent_contacts {
                self.recent_contacts.remove(0);
            }
            self.recent_contacts.push((contact, now));
        }
        self.status.contacts += 1;
        if self.queue.len() >= tuning.max_queued {
            self.status.dropped += 1;
        } else {
            self.queue.push((contact, now));
        }
    }

    pub fn native(&mut self, pair: [Identity; 2], recipient: Identity, now: Stamp) {
        let tuning = &CHARACTERS.get().impacts;
        let stale_after = tuning.stale_after();
        self.native.retain(|(_, _, at)| stale_after.holds(*at, now));
        if self.native.len() >= tuning.max_native_hits {
            self.native.remove(0);
        }
        self.native.push((pair, recipient, now));
    }

    pub fn deliver(&mut self, now: Stamp, enabled: Enabled, valid: &[Identity]) -> Vec<Feedback> {
        let tuning = &CHARACTERS.get().impacts;
        let stale_after = tuning.stale_after();
        let wanted = self.wanted_in(now, enabled, tuning);
        let now_s = now
            .saturating_duration_since(*self.start.get_or_insert(now))
            .as_secs_f64();
        let native_hits = std::mem::take(&mut self.native);
        let mut feedback = Vec::new();
        for (contact, queued) in std::mem::take(&mut self.queue) {
            let hands = [0, 1].map(|i| {
                wanted[i] == Some((contact.identity.body, contact.identity.owner))
                    && contact.holders[i] == Some(self.generations[i])
            });
            let live = valid.contains(&contact.identity)
                && contact.target.is_none_or(|target| valid.contains(&target))
                && self
                    .leases
                    .iter()
                    .flatten()
                    .any(|l| l.identity == contact.identity);
            let share = strength(
                contact.motion,
                contact.at,
                contact.normal,
                contact.impulse,
                &tuning.feedback,
            );
            if !stale_after.holds(queued, now) || !live || !hands.into_iter().any(|h| h) {
                self.status.suppressed += 1;
                continue;
            }
            let pair = contact.target.map(|target| [contact.identity, target]);
            self.damage_gate
                .retain(|(_, at)| now_s >= *at && now_s - *at < tuning.strike.repeat_s);
            let damage_impulse = pair.and_then(|pair| {
                contact
                    .target_motion
                    .filter(|_| contact.retention != 0)
                    .and_then(|target| {
                        damage_impulse(
                            contact.motion,
                            target,
                            contact.at,
                            contact.normal,
                            contact.impulse,
                            &tuning.strike,
                        )
                    })
                    .filter(|_| {
                        !self
                            .damage_gate
                            .iter()
                            .any(|(recent, _)| *recent == pair || *recent == [pair[1], pair[0]])
                    })
            });
            let native = pair.map_or([false; 2], |pair| {
                pair.map(|recipient| {
                    native_hits.iter().any(|(native_pair, target, at)| {
                        *target == recipient
                            && (*native_pair == pair || *native_pair == [pair[1], pair[0]])
                            && stale_after.holds(*at, now)
                    })
                })
            });
            if let Some(pair) = pair.filter(|_| damage_impulse.is_some()) {
                if self.damage_gate.len() >= tuning.max_struck_pairs {
                    self.damage_gate.remove(0);
                }
                self.damage_gate.push((pair, now_s));
            }
            let hands = [0, 1].map(|i| {
                hands[i] && share.is_some() && self.gate.admit(i, now_s, tuning.feedback.repeat_s)
            });
            if hands.into_iter().any(|h| h) || damage_impulse.is_some() {
                self.status.feedback += 1;
                feedback.push(Feedback {
                    contact,
                    share: share.unwrap_or(0.0),
                    hands,
                    damage_impulse,
                    native,
                });
            }
        }
        feedback
    }

    pub fn obstruction(
        &self,
        body: u32,
        owner: NonZeroUsize,
        lag: Vec3,
        now: Stamp,
        enabled: Enabled,
    ) -> Vec3 {
        if !enabled.active() || !lag.is_finite() {
            return Vec3::ZERO;
        }
        let tuning = &CHARACTERS.get().impacts;
        let freshness = tuning.contact_freshness();
        let mut basis = [Vec3::ZERO; 3];
        let mut count = 0;
        for &(contact, at) in &self.recent_contacts {
            if contact.identity.body != body
                || contact.identity.owner != owner
                || at > now
                || !freshness.holds(at, now)
                || !self.contact_live(contact, now, tuning)
            {
                continue;
            }
            let Some(mut normal) = contact.normal.try_normalize() else {
                continue;
            };
            for axis in &basis[..count] {
                normal -= *axis * normal.dot(*axis);
            }
            if normal.length_squared() <= 1.0e-6 {
                continue;
            }
            let Some(normal) = normal.try_normalize() else {
                continue;
            };
            basis[count] = normal;
            count += 1;
            if count == basis.len() {
                break;
            }
        }
        let projected = basis[..count]
            .iter()
            .fold(Vec3::ZERO, |sum, axis| sum + *axis * lag.dot(*axis));
        if projected.is_finite() {
            projected
        } else {
            Vec3::ZERO
        }
    }

    fn contact_live(&self, contact: Contact, now: Stamp, tuning: &ImpactsTuning) -> bool {
        self.fresh(now, tuning)
            && self.leases.iter().flatten().any(|lease| {
                lease.armed
                    && lease.identity == contact.identity
                    && (0..2).any(|i| {
                        self.wanted[i] == Some((contact.identity.body, contact.identity.owner))
                            && contact.holders[i] == Some(self.generations[i])
                            && lease.holders[i] == contact.holders[i]
                    })
            })
    }

    fn prune_contacts(&mut self, now: Stamp, tuning: &ImpactsTuning) {
        let freshness = tuning.contact_freshness();
        let contacts = std::mem::take(&mut self.recent_contacts);
        self.recent_contacts = contacts
            .into_iter()
            .filter(|(contact, at)| {
                *at <= now && freshness.holds(*at, now) && self.contact_live(*contact, now, tuning)
            })
            .collect();
    }

    pub fn clear(&mut self, now: Stamp) {
        self.publish([None; 2], now);
        self.queue.clear();
        self.recent_contacts.clear();
        self.gate = Gate::default();
        self.damage_gate.clear();
        self.native.clear();
    }

    fn fresh(&self, now: Stamp, tuning: &ImpactsTuning) -> bool {
        tuning.stale_after().holds(self.published, now)
    }
}

fn flag_change(identity: Identity, enable: bool) -> FlagCall {
    FlagCall {
        world: identity.world,
        body: identity.body,
        flags: hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
        mode: hknpWorld::REBUILD_COLLISION_CACHES,
        enable,
    }
}
