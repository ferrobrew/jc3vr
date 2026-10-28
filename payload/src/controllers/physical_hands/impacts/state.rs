//! Owned hold generations, event leases, and feedback planning without native calls.

use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use glam::Vec3;
use jc3gi::physics::{hknpBody, hknpWorld};

use crate::controllers::physical_hands::impacts::{
    Status,
    policy::{Gate, Motion, strength},
};

pub(super) type Held = (u32, NonZeroUsize);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Identity {
    pub world: usize,
    pub body: u32,
    pub owner: NonZeroUsize,
    pub instance: usize,
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
    pub at: Vec3,
    pub normal: Vec3,
    pub impulse: f32,
    pub character: bool,
}

pub(super) struct Feedback {
    pub contact: Contact,
    pub share: f32,
    pub hands: [bool; 2],
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
    pub hands: bool,
    pub pickup: bool,
    pub feature: bool,
    pub gameplay: bool,
    pub running: bool,
}

impl Enabled {
    pub fn active(self) -> bool {
        self.controllers
            && self.hands
            && self.pickup
            && self.feature
            && self.gameplay
            && self.running
    }
}

pub(super) struct State {
    published: Option<Instant>,
    wanted: [Option<Held>; 2],
    generations: [u64; 2],
    pub leases: [Option<Lease>; 2],
    queue: Vec<(Contact, Instant)>,
    gate: Gate,
    start: Option<Instant>,
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
            gate: Gate { last: [None; 2] },
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

    pub fn publish(&mut self, held: [Option<Held>; 2], now: Instant) {
        let stale = !self.fresh(now);
        for (i, wanted) in held.iter().enumerate() {
            if self.wanted[i] != *wanted || stale {
                self.generations[i] = self.generations[i]
                    .checked_add(1)
                    .expect("held-impact generation exhausted");
            }
        }
        self.wanted = held;
        self.published = Some(now);
    }

    pub fn wanted(&self, now: Instant, enabled: Enabled) -> [Option<Held>; 2] {
        if enabled.active() && self.fresh(now) {
            self.wanted
        } else {
            [None; 2]
        }
    }

    /// Keep and acquire leases from live observations, and plan only necessary native flag changes.
    pub fn plan_step(
        &mut self,
        now: Instant,
        enabled: Enabled,
        observations: &[Observation],
    ) -> Vec<FlagCall> {
        if !enabled.active() {
            self.clear(now);
        }
        let wanted = self.wanted(now, enabled);
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

    pub fn queue(&mut self, contact: Contact, now: Instant) {
        self.status.contacts += 1;
        if self.queue.len() >= 32 {
            self.status.dropped += 1;
        } else {
            self.queue.push((contact, now));
        }
    }

    pub fn deliver(&mut self, now: Instant, enabled: Enabled, valid: &[Identity]) -> Vec<Feedback> {
        let wanted = self.wanted(now, enabled);
        let now_s = now
            .saturating_duration_since(*self.start.get_or_insert(now))
            .as_secs_f64();
        let mut feedback = Vec::new();
        for (contact, queued) in std::mem::take(&mut self.queue) {
            let hands = [0, 1].map(|i| {
                wanted[i] == Some((contact.identity.body, contact.identity.owner))
                    && contact.holders[i] == Some(self.generations[i])
            });
            let live = valid.contains(&contact.identity)
                && self
                    .leases
                    .iter()
                    .flatten()
                    .any(|l| l.identity == contact.identity);
            let share = strength(contact.motion, contact.at, contact.normal, contact.impulse);
            if now.saturating_duration_since(queued) > STALE_AFTER
                || !live
                || !hands.into_iter().any(|h| h)
                || share.is_none()
            {
                self.status.suppressed += 1;
                continue;
            }
            let hands = [0, 1].map(|i| hands[i] && self.gate.admit(i, now_s));
            if hands.into_iter().any(|h| h) {
                self.status.feedback += 1;
                feedback.push(Feedback {
                    contact,
                    share: share.unwrap(),
                    hands,
                });
            }
        }
        feedback
    }

    pub fn clear(&mut self, now: Instant) {
        self.publish([None; 2], now);
        self.queue.clear();
        self.gate = Gate::default();
    }

    fn fresh(&self, now: Instant) -> bool {
        self.published
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }
}

const STALE_AFTER: Duration = Duration::from_millis(250);

fn flag_change(identity: Identity, enable: bool) -> FlagCall {
    FlagCall {
        world: identity.world,
        body: identity.body,
        flags: hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
        mode: hknpWorld::REBUILD_COLLISION_CACHES,
        enable,
    }
}
