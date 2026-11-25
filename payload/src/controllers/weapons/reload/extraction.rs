//! Acquisition and gun-relative pull of an inserted detachable magazine.
//!
//! A claim owns only the magazine, not the gun or its foregrip. Until the pull completes the
//! magazine remains inserted; cancellation therefore changes neither ammunition nor the chamber.
//! Completion hands the exact retained engine prop to the existing loose-magazine insertion path.

use glam::Vec3;

use crate::controllers::{
    frame::{Hand, Pose},
    grab::{Candidate, GrabVolume, GripTarget, resolve::reaches, target::TargetReach},
    haptics::{self, Event},
    weapons::{
        capabilities::tracked_hands,
        foregrip,
        reload::{
            FinishInputs, GunTick, STATE, State, Tick, WeaponTick,
            chamber::WorkStage,
            config::ExtractionConfig,
            engine,
            insert::{InsertInputs, InsertionSource},
            loose,
            plan::{LoadKind, WellFrame},
            serial,
            tuning::Reload,
            zones::{self, MagazineZone, ZoneView},
        },
    },
};

/// The inserted magazine that owns a pull, including the gun within a dual-wield weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct MagazineId {
    pub weapon: usize,
    pub gun: usize,
    pub holder: Hand,
}

/// One tracked sample, in the cockpit frame, of the magazine and the hand pulling it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Sample {
    id: MagazineId,
    expected: serial::Identity,
    frame: Pose,
    outward: Vec3,
    grip: Vec3,
    /// The pulling hand is in the magazine's zone under the well ([`zones`]).
    in_zone: bool,
    world_scale: f32,
}

/// An acquired pull. Distances are measured in the moving gun frame at the player's scale.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct Pull {
    held: Option<Acquired>,
}

impl Pull {
    pub(super) const fn new() -> Self {
        Self { held: None }
    }

    pub(super) fn hand(&self) -> Option<Hand> {
        self.held.map(|h| h.id.holder.other())
    }

    pub(super) fn reset(&mut self) {
        self.held = None;
    }

    /// Only `claimed` starts a pull; a held grip over the magazine never acquires it implicitly.
    pub(super) fn step(
        &mut self,
        sample: Option<Sample>,
        claimed: bool,
        grip_held: bool,
        config: &ExtractionConfig,
    ) -> bool {
        let Some(sample) = sample.filter(|s| valid(s, config) && grip_held) else {
            self.reset();
            return false;
        };
        let local = sample.frame.orientation.inverse() * (sample.grip - sample.frame.position);
        let Some(acquired) = self.held else {
            if claimed && (local.length() <= config.grab_radius_m || sample.in_zone) {
                self.held = Some(Acquired {
                    id: sample.id,
                    expected: sample.expected,
                    local,
                    world_scale: sample.world_scale,
                });
            }
            return false;
        };
        if acquired.id != sample.id
            || acquired.expected != sample.expected
            || acquired.world_scale != sample.world_scale
        {
            self.reset();
            return false;
        }
        let delta = local - acquired.local;
        let outward = sample.frame.orientation.inverse() * sample.outward;
        let travel = delta.dot(outward);
        let sideways = (delta - outward * travel).length();
        if sideways > config.lateral_tolerance_m || travel < -config.grab_radius_m {
            self.reset();
            return false;
        }
        if travel >= config.pull_m {
            self.reset();
            return true;
        }
        false
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum DetachKind {
    /// The free hand pulls the magazine out of the well.
    Pull,
    /// The reload button or a flick drops the magazine.
    Eject,
    /// A push onto the belt pouch drops the magazine, and a fresh one loads once it is out.
    Belt,
    /// A held pouch magazine struck against the inserted one knocks it out of the well.
    Knock,
}

/// A request is a value, not permission to call the engine from the input tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct DetachRequest {
    pub id: MagazineId,
    pub generation: u64,
    pub expected: serial::Identity,
    pub kind: DetachKind,
}

/// Input context retained until the actual result can be assigned to a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct PendingDetach {
    request: DetachRequest,
    world_scale: f32,
    insertion_taken: u64,
}

/// The input side submits and later polls. The implementation executes only inside the verified
/// native serial callback and returns the actual engine outcome for the same request.
pub(super) trait DetachEngine {
    fn available(&self) -> bool;
    fn submit(&mut self, request: DetachRequest) -> bool;
    fn poll(&mut self, request: DetachRequest) -> Option<engine::DetachOutcome>;
    fn cancel(&mut self, request: DetachRequest);
    fn take(&mut self, hand: Hand, magazine: loose::Loose, cockpit: Vec3) -> bool;
}

struct SerialEngine;

impl DetachEngine for SerialEngine {
    fn available(&self) -> bool {
        serial::available()
    }
    fn submit(&mut self, request: DetachRequest) -> bool {
        serial::submit(request)
    }
    fn poll(&mut self, request: DetachRequest) -> Option<engine::DetachOutcome> {
        serial::poll(request)
    }
    fn cancel(&mut self, request: DetachRequest) {
        serial::cancel(request);
    }
    fn take(&mut self, hand: Hand, magazine: loose::Loose, cockpit: Vec3) -> bool {
        loose::take(hand, loose::Sighting { magazine, cockpit })
    }
}

/// Keep an existing hold, then prefer a direct inserted-magazine claim over overlapping gun
/// support, transfer, and action targets. Other targets keep the ordinary resolver's ranking.
///
/// A free hand in the magazine's zone under the other hand's gun's well is at the magazine: the
/// handover is not offered to it, and the magazine is in its reach wherever in the zone it is.
pub(super) fn prioritize(targets: &mut Vec<GripTarget>, grips: [Option<Vec3>; 2]) {
    let views = prioritize_with(targets, grips, magazine_zones());
    zones::publish(views);
}

/// [`prioritize`] with each held gun's magazine zone (`zones`, by the holding hand). Returns the
/// zones as resolved, for the grab view.
fn prioritize_with(
    targets: &mut Vec<GripTarget>,
    grips: [Option<Vec3>; 2],
    zones: [Option<MagazineZone>; 2],
) -> [Option<ZoneView>; 2] {
    let views = zone_views(targets, grips, zones);
    for view in views.iter().flatten().filter(|v| v.at_magazine) {
        let hand = view.holder.other();
        let Some(distance) = view
            .magazine
            .zip(grips[hand.index()])
            .and_then(|(zone, grip)| zone.distance(grip))
        else {
            continue;
        };
        targets.retain(|t| !(t.volume == GrabVolume::Handle && t.hand == Some(hand)));
        for target in targets
            .iter_mut()
            .filter(|t| t.volume == GrabVolume::InsertedMagazine && t.hand == Some(hand))
        {
            target.reach = TargetReach::Measured { distance };
        }
    }
    for hand in Hand::BOTH {
        let reachable = reaches(hand, targets, grips[hand.index()]);
        let held = reachable
            .iter()
            .any(|r| targets[r.index].held_by == Some(hand));
        let magazine = reachable
            .iter()
            .any(|r| r.volume == GrabVolume::InsertedMagazine);
        let action = reachable
            .iter()
            .any(|r| r.volume == GrabVolume::WeaponAction);
        if action && !magazine && !held {
            targets.retain(|t| {
                !t.for_hand(hand) || !matches!(t.volume, GrabVolume::Foregrip | GrabVolume::Handle)
            });
        }
        if magazine && !held {
            targets.retain(|t| {
                !t.for_hand(hand)
                    || !matches!(
                        t.volume,
                        GrabVolume::Foregrip | GrabVolume::Handle | GrabVolume::WeaponAction
                    )
            });
        }
    }
    views
}

impl State {
    /// The explicit magazine target, for a free, tracked offhand only.
    pub(super) fn extraction_candidate(
        &mut self,
        tick: &Tick,
        free: Option<Hand>,
        reload: &Reload,
    ) -> Option<Candidate> {
        let hand = free?;
        if !engine::physical_detach_available()
            || self.extraction_needs_neutral[hand.index()]
            || !tracked_hands().into_iter().all(|tracked| tracked)
            || self.pending_ejects[hand.other().index()].is_some()
        {
            return None;
        }
        if self.status.insertion.holding()
            || loose::held().is_some()
            || foregrip::status()
                .hold
                .is_some_and(|h| h.holder.other() == hand)
        {
            return None;
        }
        let (weapon, gun) = tick.gun(hand.other())?;
        let record = self.records.get(weapon.key);
        if record.travels[gun.entry()].grip.is_some()
            || matches!(self.status.work.stage, WorkStage::Held { .. })
        {
            return None;
        }
        let sample = sample(tick, weapon, gun, hand.other(), reload)?;
        Some(Candidate {
            volume: GrabVolume::InsertedMagazine,
            centre: sample.frame.position,
            radius: reload.extraction.grab_radius_m,
            hand: Some(hand),
        })
    }

    /// Complete extraction after the ordinary insertion step so it cannot consume a stale
    /// magazine-in snapshot on the tick the engine prop enters the hand.
    pub(super) fn extract(&mut self, tick: &Tick, inputs: &FinishInputs, reload: &Reload) {
        if !tracked_hands().into_iter().all(|tracked| tracked) {
            self.cancel_extraction();
            return;
        }
        self.extract_with(tick, inputs, reload, &mut SerialEngine);
    }

    pub(super) fn cancel_extraction(&mut self) {
        self.extraction.reset();
        if let Some(pending) = self.pending_detach.take() {
            serial::cancel(pending.request);
        }
        self.extraction_needs_neutral = [true; 2];
    }

    pub(super) fn extraction_hand(&self) -> Option<Hand> {
        self.extraction
            .hand()
            .or_else(|| self.pending_detach.map(|p| p.request.id.holder.other()))
    }

    pub(super) fn extract_with(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        reload: &Reload,
        adapter: &mut impl DetachEngine,
    ) {
        if let Some(hand) = self.extraction_hand() {
            self.extraction_blocks_belt[hand.other().index()] = true;
        }
        for hand in Hand::BOTH {
            if !inputs.grips[hand.index()].held() {
                self.extraction_needs_neutral[hand.index()] = false;
            }
        }
        if !adapter.available() {
            if let Some(pending) = self.pending_detach {
                adapter.cancel(pending.request);
            }
            self.cancel_extraction();
            return;
        }
        if let Some(pending) = self.pending_detach {
            let request = pending.request;
            let hand = request.id.holder.other();
            let same_gun = tick.gun(request.id.holder).is_some_and(|(w, g)| {
                w.address == request.id.weapon
                    && g.index == request.id.gun
                    && g.magazine
                        .is_some_and(|identity| request.expected.same_gun(identity))
                    && g.frame.is_some_and(|f| {
                        f.world_scale == pending.world_scale
                            && f.pose.position.is_finite()
                            && f.pose.orientation.is_finite()
                    })
                    && g.well.is_some_and(|well| {
                        well.mouth.is_finite()
                            && well.axis.is_finite()
                            && well.axis.length_squared() > 0.0
                    })
            });
            let grip = tick.hands[hand.index()]
                .map(|h| h.grip.position)
                .filter(|p| p.is_finite());
            if !inputs.grips[hand.index()].held()
                || !same_gun
                || grip.is_none()
                || self.free != Some(hand)
                || self
                    .frames
                    .is_none_or(|f| f.cockpit.world_scale != pending.world_scale)
                || tick.world_scale != pending.world_scale
                || self.status.insertion.holding()
                || self.status.insertion.taken != pending.insertion_taken
                || self.status.insert_hands.is_some()
                || loose::held().is_some()
                || !reload.enabled
                || !reload.extraction.enabled
                || !reload.loose.enabled
            {
                adapter.cancel(request);
                self.cancel_extraction();
                return;
            }
            let Some(outcome) = adapter.poll(request) else {
                return;
            };
            self.pending_detach = None;
            self.extraction_needs_neutral[hand.index()] = true;
            match outcome {
                engine::DetachOutcome::NoThrow => {
                    self.event("the magazine pull did not detach".to_owned())
                }
                engine::DetachOutcome::DroppedUnretained => {
                    self.status.counts.ejects += 1;
                    self.event("the magazine dropped without a retained prop".to_owned());
                }
                engine::DetachOutcome::Retained(magazine) => {
                    if !adapter.take(hand, magazine, grip.unwrap_or(Vec3::ZERO)) {
                        return;
                    }
                    self.status.insert_hands = Some((hand, hand.other()));
                    self.insertion_source = InsertionSource::RetainedMagazine;
                    self.status.insertion.step_with_source(
                        InsertInputs {
                            taken: true,
                            grip_held: true,
                            held: None,
                            well: None,
                            occupied: false,
                            dt: 0.0,
                        },
                        self.insertion_source,
                        reload,
                    );
                    self.status.counts.ejects += 1;
                    self.event(format!("{hand:?} pulled a magazine of {}", magazine.rounds));
                    haptics::play(hand, Event::Eject);
                }
            }
            return;
        }
        let hand = self.extraction.hand().or_else(|| {
            Hand::BOTH
                .into_iter()
                .find(|h| inputs.grabs[h.index()] == Some(GrabVolume::InsertedMagazine))
        });
        let target = hand
            .filter(|h| {
                self.free == Some(*h)
                    && !self.extraction_needs_neutral[h.index()]
                    && self.frames.is_some()
                    && !self.status.insertion.holding()
            })
            .and_then(|h| tick.gun(h.other()).map(|(w, g)| (h, *w, *g)));
        let current = target.and_then(|(h, w, g)| sample(tick, &w, &g, h.other(), reload));
        let claimed =
            hand.is_some_and(|h| inputs.grabs[h.index()] == Some(GrabVolume::InsertedMagazine));
        let held = hand.is_some_and(|h| inputs.grips[h.index()].held());
        if !self
            .extraction
            .step(current, claimed, held, &reload.extraction)
        {
            return;
        }
        let Some(current) = current else {
            return;
        };
        let Some(generation) = self.detach_generation.checked_add(1) else {
            self.cancel_extraction();
            return;
        };
        self.detach_generation = generation;
        let request = DetachRequest {
            id: current.id,
            generation,
            expected: current.expected,
            kind: DetachKind::Pull,
        };
        if adapter.submit(request) {
            self.pending_detach = Some(PendingDetach {
                request,
                world_scale: current.world_scale,
                insertion_taken: self.status.insertion.taken,
            });
        } else {
            self.cancel_extraction();
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Acquired {
    id: MagazineId,
    expected: serial::Identity,
    local: Vec3,
    world_scale: f32,
}

fn valid(sample: &Sample, config: &ExtractionConfig) -> bool {
    config.enabled
        && sample.frame.position.is_finite()
        && sample.frame.orientation.is_finite()
        && sample.frame.orientation.is_normalized()
        && sample.grip.is_finite()
        && sample.outward.is_finite()
        && (sample.outward.length_squared() - 1.0).abs() <= f32::EPSILON * 8.0
        && sample.world_scale.is_finite()
        && sample.world_scale > 0.0
        && config.grab_radius_m.is_finite()
        && config.grab_radius_m > 0.0
        && config.pull_m.is_finite()
        && config.pull_m > 0.0
        && config.lateral_tolerance_m.is_finite()
        && config.lateral_tolerance_m > 0.0
}

fn sample(
    tick: &Tick,
    weapon: &WeaponTick,
    gun: &GunTick,
    holder: Hand,
    reload: &Reload,
) -> Option<Sample> {
    if !eligible(weapon.plan.load, weapon.reading.out, reload)
        || !matches!(weapon.plan.well, WellFrame::Gun { .. })
        || !reload.loose.half_length_m.is_finite()
        || reload.loose.half_length_m <= 0.0
    {
        return None;
    }
    let frame = gun.frame?;
    let well = gun.well?;
    let outward = -well.axis.try_normalize()?;
    let centre = well.mouth + outward * (reload.loose.half_length_m / tick.world_scale);
    let id = MagazineId {
        weapon: weapon.address,
        gun: gun.index,
        holder,
    };
    let expected = gun.magazine?;
    if !expected.has_spare() {
        return None;
    }
    let grip = tick.hands[holder.other().index()]?.grip.position;
    let in_zone = zone_of(tick, weapon, gun).is_some_and(|zone| zone.contains(grip));
    let sample = Sample {
        id,
        expected,
        frame: Pose {
            position: centre,
            orientation: frame.pose.orientation,
        },
        outward,
        grip,
        in_zone,
        world_scale: tick.world_scale,
    };
    valid(&sample, &reload.extraction).then_some(sample)
}

/// Each held gun's magazine zone for the free hand, by the holding hand's [`Hand::index`].
fn magazine_zones() -> [Option<MagazineZone>; 2] {
    let state = STATE.lock();
    let Some(tick) = state.tick else {
        return [None; 2];
    };
    Hand::BOTH.map(|holder| {
        if state.free != Some(holder.other()) {
            return None;
        }
        let (weapon, gun) = tick.gun(holder)?;
        zone_of(&tick, weapon, gun)
    })
}

/// `gun`'s magazine zone, with the well where the tick placed it.
fn zone_of(tick: &Tick, weapon: &WeaponTick, gun: &GunTick) -> Option<MagazineZone> {
    zones::zone_for(
        weapon.plan.load,
        weapon.plan.well,
        weapon.reading.out,
        gun.well.map(|well| (well.mouth, well.axis)),
        tick.world_scale,
    )
}

/// The zones of each held gun for the free hand (`zones`, by the holding hand), with the handover
/// offered to that hand in `targets`, and whether its grip is at the magazine.
fn zone_views(
    targets: &[GripTarget],
    grips: [Option<Vec3>; 2],
    zones: [Option<MagazineZone>; 2],
) -> [Option<ZoneView>; 2] {
    Hand::BOTH.map(|holder| {
        let hand = holder.other();
        let magazine = zones[holder.index()];
        let handover = targets
            .iter()
            .filter(|t| t.volume == GrabVolume::Handle && t.hand == Some(hand))
            .find_map(|t| match t.reach {
                TargetReach::Sphere { centre, radius } => Some((centre, radius)),
                TargetReach::Measured { .. } => None,
            });
        if magazine.is_none() && handover.is_none() {
            return None;
        }
        let at_magazine = magazine
            .zip(grips[hand.index()])
            .is_some_and(|(zone, grip)| zone.contains(grip));
        Some(ZoneView {
            holder,
            magazine,
            handover,
            at_magazine,
        })
    })
}

pub(super) fn eligible(load: LoadKind, out: bool, reload: &Reload) -> bool {
    reload.enabled
        && reload.extraction.enabled
        && reload.loose.enabled
        && load == LoadKind::Magazine
        && !out
}

#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tests;
