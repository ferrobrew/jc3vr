//! The pickups' simulation tick: take hold and let go as the input tick asked, keep the grips on
//! latched doors, and drive what the hands carry.

use std::{num::NonZeroUsize, time::Instant};

use glam::Vec3;
use jc3gi::vehicle::vehicle::DoorSlot;

use crate::controllers::{
    diagnostics::{Released, Thrown, note_release, note_throw},
    frame::Hand,
    physical_hands::{
        STALE_AFTER, bodies,
        characters::{CharactersConfig, Seize},
        doors::{self, Latch},
        grasp::{self, Grip, PalmMotion, Reach, TargetKind},
        hold::{self, HandHold, HoldTuning, PlayerKeepOut},
        physics,
        physics::HoldTarget,
        pickup::{
            HeldReport, PickupTuning,
            diagnostics::{self, ConsumerResult, DriveRecord},
            grips::{keep_latch, keep_regrip, keep_seize, shove_on_release},
            state::{GripSubject, Held, HeldKind, PickupState, Request},
            tearing::{self, Frame as TearFrame, Observation},
        },
        rigid::Velocities,
    },
};

/// What the hand bodies tell a pickup tick about the joints.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct JointInputs {
    /// Whether joint grabs are enabled (the hand bodies are on), independent of which hands
    /// currently act.
    pub enabled: bool,
    /// Which hands take what they grab through a joint: those their bodies drive.
    pub join: [bool; 2],
    /// Which hands lost the joint their grip held.
    pub lost: [bool; 2],
    /// The share of each hand body's strongest pull and twist the last step used.
    pub loads: [(f32, f32); 2],
}

/// What a pickup tick works from.
pub(crate) struct TickInputs {
    /// The acting hands' palms; a hand without one lets go.
    pub palms: [Option<PalmMotion>; 2],
    /// The player's body, which a carried object is kept out of.
    pub space: Option<PlayerKeepOut>,
    pub joints: JointInputs,
    /// The tick's length, seconds of game time.
    pub dt: f32,
    /// The game's time scale, game seconds per real second: below one in slow motion.
    pub time_scale: f32,
    /// How far a hand gripping a latched door pulls it before the latch gives, world metres.
    pub unlatch_pull_m: f32,
    pub tearing: doors::config::TearTuning,
    /// The VR world scale.
    pub world_scale: f32,
    pub characters: CharactersConfig,
    /// What each hand reaches this tick, from which a hand that yanked a character takes hold of
    /// its ragdoll.
    pub reaches: [Option<Reach>; 2],
    /// The player character's world velocity, metres per second, whose change a carried object
    /// takes on ([`PickupTuning::carrier_motion_share`]); none when it is not known.
    pub carrier_velocity: Option<Vec3>,
}

/// What a pickup tick did, for the readout, the haptics, and the overlay.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TickOutcome {
    /// Each hand's hold after the tick.
    pub reports: [Option<HeldReport>; 2],
    /// The hands that took hold this tick.
    pub taken: [bool; 2],
    /// The hands that let go this tick.
    pub released: [bool; 2],
    /// The speed of an object thrown this tick.
    pub throw: Option<f32>,
    /// The velocities each hand's release gave what it threw this tick.
    pub thrown: [Option<Velocities>; 2],
    /// Holds lost this tick.
    pub lost: u64,
    /// Why each hand's hold of a body was lost this tick, when it was.
    pub losses: [Option<HoldLoss>; 2],
    pub error: Option<String>,
    /// Each hand's grab that was refused this tick: where it reached (world space), and why.
    pub refused: [Option<(Vec3, String)>; 2],
    /// What each hand holds through a joint, for the hand bodies to join.
    pub joints: [Option<HoldTarget>; 2],
    /// Where each hand gripping a latched door is held, world space.
    pub pins: [Option<Vec3>; 2],
    /// The latches the hands undid this tick.
    pub unlatched: [bool; 2],
    /// Grips taken on a latched door this tick.
    pub door_grips: u64,
    pub tears: u64,
    pub tear_refusals: u64,
    /// The slot of a door a hand took this tick.
    pub door: Option<DoorSlot>,
    /// Grips taken on characters this tick.
    pub character_grips: u64,
    /// Seated characters the hands pulled out this tick.
    pub hijacks: u64,
    /// Articulated objects the hands took hold of this tick.
    pub articulated_grips: u64,
    /// Characters on foot the hands yanked this tick.
    pub yanks: u64,
    /// Fallen characters' ragdolls the hands took hold of this tick.
    pub regrips: u64,
}

/// Why a hand's hold of a body ended other than by the grip opening.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum HoldLoss {
    /// The hand stopped being tracked or acting.
    Untracked,
    /// The joint between the hand's body and the object ended.
    JointEnded,
    /// The grab point fell past the break distance behind the hand.
    TornAway,
    /// The held body could not be read: it went away or changed owner.
    BodyGone,
    /// The carry's velocities were refused.
    DriveRefused,
    /// A held door's body went away: it latched shut or broke off.
    DoorGone,
    /// A held door's vehicle is gone, or the door is no longer the one gripped.
    DoorIdentity,
}

impl HoldLoss {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            HoldLoss::Untracked => "the hand stopped acting",
            HoldLoss::JointEnded => "the hand's joint to it ended",
            HoldLoss::TornAway => "it was torn from the hand",
            HoldLoss::BodyGone => "its body went away",
            HoldLoss::DriveRefused => "its carry was refused",
            HoldLoss::DoorGone => "the door latched shut or broke off",
            HoldLoss::DoorIdentity => "the door or its vehicle is no longer the one gripped",
        }
    }
}

impl TickOutcome {
    /// Note that each hand holding `held` lost its hold, `loss`.
    pub(super) fn lose_hold(&mut self, holders: [bool; 2], loss: HoldLoss) {
        for (k, holds) in holders.into_iter().enumerate() {
            if holds {
                self.losses[k] = Some(loss);
            }
        }
    }

    /// Note that `hand`'s grab of what it reached at `at` (world space) was refused, `why`.
    fn refuse(&mut self, hand: Hand, at: Vec3, why: String) {
        self.refused[hand.index()] = Some((at, why.clone()));
        self.error = Some(why);
    }
}

/// The pickups' simulation tick: take hold and let go as the input tick asked, keep the grips on
/// latched doors, and drive what the hands carry.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(crate) unsafe fn tick(
    state: &mut PickupState,
    inputs: TickInputs,
    config: &PickupTuning,
) -> TickOutcome {
    let subjects = Hand::BOTH.map(|hand| state.grip_subject(hand));
    let opened = state
        .requests
        .map(|request| matches!(request, Some(Request::Release)));
    let time_scale = inputs.time_scale;
    let outcome = unsafe { tick_with(state, inputs, config, &mut NativeBodies) };
    log_losses(subjects, &outcome);
    note_releases(subjects, opened, time_scale, &outcome);
    outcome
}

/// Log each hold lost this tick, with what the hand held and why it was lost.
fn log_losses(subjects: [Option<GripSubject>; 2], outcome: &TickOutcome) {
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(loss) = outcome.losses[i] else {
            continue;
        };
        tracing::info!(
            target: "controllers",
            ?hand,
            held = subjects[i].map_or("an object", |subject| subject.held),
            body = subjects[i].and_then(|subject| subject.body),
            detail = outcome.error.as_deref(),
            "physical hands: pickup: a hold was lost: {}",
            loss.label()
        );
    }
}

/// Record each hand that let go this tick: what it held, as the tick found it, why, and the motion
/// of what it threw.
fn note_releases(
    subjects: [Option<GripSubject>; 2],
    opened: [bool; 2],
    time_scale: f32,
    outcome: &TickOutcome,
) {
    for hand in Hand::BOTH {
        let i = hand.index();
        if !outcome.released[i] {
            continue;
        }
        let thrown = outcome.thrown[i];
        if let Some(velocities) = thrown {
            note_throw(Thrown {
                hand,
                item: subjects[i].map_or("an object", |subject| subject.held),
                linear: velocities.linear,
                angular: velocities.angular,
                time_scale,
                source: None,
                armed: None,
            });
        }
        let Some(subject) = subjects[i] else {
            continue;
        };
        note_release(Released {
            hand,
            held: subject.held,
            object: subject.object,
            body: subject.body,
            cause: if opened[i] {
                "the grip opened"
            } else if let Some(loss) = outcome.losses[i] {
                loss.label()
            } else if outcome.lost > 0 {
                "the hold was lost"
            } else {
                "the hold ended"
            },
            thrown: thrown.is_some(),
        });
    }
}

unsafe fn tick_with(
    state: &mut PickupState,
    inputs: TickInputs,
    config: &PickupTuning,
    engine: &mut impl BodyEngine,
) -> TickOutcome {
    let TickInputs {
        palms,
        space,
        joints,
        dt,
        time_scale,
        unlatch_pull_m,
        tearing,
        world_scale,
        characters: character_config,
        reaches,
        carrier_velocity,
    } = inputs;
    let mut outcome = TickOutcome::default();
    let carried = carrier_change(state.carrier_velocity, carrier_velocity, config);
    state.carrier_velocity = carrier_velocity;
    if state
        .joints_enabled
        .is_some_and(|previous| previous != joints.enabled)
    {
        outcome.released = Hand::BOTH.map(|h| state.has_interaction(h));
        state.drop_all();
    }
    state.joints_enabled = Some(joints.enabled);
    for hand in Hand::BOTH {
        let i = hand.index();
        if joints.lost[i] && state.held[i].is_some_and(|h| h.joined) {
            state.held[i] = None;
            outcome.released[i] = true;
            outcome.lost += 1;
            outcome.losses[i] = Some(HoldLoss::JointEnded);
            outcome.error =
                Some("physical hands: pickup: the hand's joint to the object ended".to_owned());
        }
    }
    let now = Instant::now();
    let input_fresh = state
        .input_at
        .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER);
    if !input_fresh {
        // The bindings stopped reading the grips: no release will come.
        state.drop_all();
    }

    for hand in Hand::BOTH {
        let i = hand.index();
        match state.requests[i].take() {
            Some(Request::Release) => {
                state.doors[i] = None;
                state.evidence_holds[i] = None;
                if state.latches[i].take().is_some() {
                    outcome.released[i] = true;
                }
                if let Some(seize) = state.seizes[i].take() {
                    outcome.released[i] = true;
                    if let Some(palm) = palms[i] {
                        shove_on_release(hand, &seize, &palm);
                    }
                }
                if state.regrips[i].take().is_some() {
                    outcome.released[i] = true;
                }
                if let Some(held) = state.held[i].take() {
                    let shared = state.held[hand.other().index()].is_some_and(|o| o.same(&held));
                    // A joined object leaves with the velocity the hand gave it.
                    if !shared && !held.joined {
                        // SAFETY: forwarded from the caller.
                        match unsafe { throw(&held, palms[i], config) } {
                            Ok(Some((speed, velocities))) => {
                                outcome.throw = Some(speed);
                                outcome.thrown[i] = Some(velocities);
                            }
                            Ok(None) => outcome.throw = Some(0.0),
                            Err(e) => outcome.error = Some(e.to_string()),
                        }
                    }
                    outcome.released[i] = true;
                }
            }
            Some(request @ (Request::Grab(_) | Request::ObservedGrab(_))) => 'grab: {
                let (reach, evidence) = request.grab().expect("grab request");
                state.doors[i] = None;
                state.evidence_holds[i] = None;
                let record = |result| {
                    if let Some(key) = evidence {
                        diagnostics::consumed(hand, key, result);
                    }
                };
                let Some(palm) = palms[i] else {
                    record(ConsumerResult::NoPalm);
                    break 'grab;
                };
                if let TargetKind::Door(door) = reach.kind {
                    outcome.door = Some(door.slot);
                    if door.latched {
                        // SAFETY: forwarded from the caller; the door's vehicle was reached by
                        // this tick's casts.
                        match unsafe { Latch::take(door, reach.point, palm.pose) } {
                            Some(latch) => {
                                tracing::info!(
                                    hand = hand.label(),
                                    slot = doors::slot_label(door.slot),
                                    part = door.part_index,
                                    "physical hands: doors: a hand gripped a latched door"
                                );
                                record(ConsumerResult::LatchAccepted);
                                state.evidence_holds[i] = evidence;
                                state.latches[i] = Some(latch);
                                outcome.taken[i] = true;
                                outcome.door_grips += 1;
                            }
                            None => {
                                record(ConsumerResult::LatchRefused);
                                outcome.refuse(
                                    hand,
                                    reach.point,
                                    "physical hands: doors: the door's grip could not be placed"
                                        .to_owned(),
                                );
                            }
                        }
                        break 'grab;
                    }
                }
                if let TargetKind::Character(target) = reach.kind
                    && !target.ragdoll
                {
                    // SAFETY: the queued target is checked before any character access.
                    match unsafe {
                        Seize::take(
                            target,
                            reach.point,
                            palm.pose.position,
                            world_scale,
                            &character_config,
                        )
                    } {
                        Some(seize) => {
                            tracing::info!(
                                hand = hand.label(),
                                seated = target.seated.is_some(),
                                "physical hands: characters: a hand took hold of a character"
                            );
                            record(ConsumerResult::CharacterAccepted);
                            state.evidence_holds[i] = evidence;
                            state.seizes[i] = Some(seize);
                            outcome.taken[i] = true;
                            outcome.character_grips += 1;
                        }
                        None => {
                            // SAFETY: forwarded from the caller; only the body's owner is read.
                            let owned =
                                unsafe { bodies::owner_of(target.body) } == Some(target.character);
                            tracing::info!(
                                hand = hand.label(),
                                seated = target.seated.is_some(),
                                owned,
                                "physical hands: characters: a hand's grip on a character could not be placed"
                            );
                            record(ConsumerResult::CharacterRefused);
                            outcome.refuse(
                                hand,
                                reach.point,
                                "physical hands: characters: the grip could not be placed"
                                    .to_owned(),
                            );
                        }
                    }
                    break 'grab;
                }
                // SAFETY: forwarded from the caller.
                let body = engine.read(reach.body, reach.object);
                let mut result = take_body(
                    state,
                    BodyGrab {
                        hand,
                        reach,
                        palm,
                        joined: joints.join[i],
                    },
                    body,
                    config,
                    &mut outcome,
                );
                if matches!(result, ConsumerResult::BodyAccepted { .. })
                    && let TargetKind::Door(door) = reach.kind
                {
                    let acquired =
                        unsafe { doors::tearing::TearGrip::take(door, reach.object, reach.body) }
                            .inspect_err(|refusal| {
                                tracing::info!(
                                    hand = hand.label(),
                                    slot = doors::slot_label(door.slot),
                                    part = door.part_index,
                                    body = reach.body,
                                    %refusal,
                                    "physical hands: doors: the reached door's identity changed"
                                );
                            })
                            .ok();
                    if !crate::controllers::physical_hands::pickup::tearing::acquired(
                        state, i, acquired,
                    ) {
                        outcome.taken[i] = false;
                        result = ConsumerResult::BodyRefused(bodies::BodyError::Reowned);
                        outcome.refuse(
                            hand,
                            reach.point,
                            "physical hands: doors: the reached door's identity changed".to_owned(),
                        );
                    } else {
                        result = ConsumerResult::BodyAccepted {
                            joined: state.held[i].is_some_and(|h| h.joined),
                        };
                    }
                }
                record(result);
                if matches!(result, ConsumerResult::BodyAccepted { .. }) {
                    state.evidence_holds[i] = evidence;
                }
            }
            None => {}
        }
        if state.held[i].is_some() && palms[i].is_none() {
            state.held[i] = None;
            outcome.lost += 1;
            outcome.losses[i] = Some(HoldLoss::Untracked);
        }
        if state.latches[i].is_some() && palms[i].is_none() {
            state.latches[i] = None;
            outcome.lost += 1;
        }
        if state.seizes[i].is_some() && palms[i].is_none() {
            state.seizes[i] = None;
            outcome.lost += 1;
        }
        if state.regrips[i].is_some() && palms[i].is_none() {
            state.regrips[i] = None;
            outcome.lost += 1;
        }
        // The fallen character's wait runs on the game's clock, as the fall does.
        if let Some(regrip) = state.regrips[i].as_mut() {
            regrip.run_down(dt);
        }
        // SAFETY: forwarded from the caller.
        unsafe {
            keep_regrip(
                state,
                hand,
                palms[i],
                reaches[i],
                &joints,
                config,
                &mut outcome,
            )
        };
        // SAFETY: forwarded from the caller.
        unsafe {
            keep_seize(
                state,
                hand,
                palms[i],
                world_scale,
                &character_config,
                &mut outcome,
            )
        };
        // SAFETY: forwarded from the caller.
        unsafe { keep_latch(state, hand, palms[i], unlatch_pull_m, &mut outcome) };
        if state.held[i].is_none()
            && state.latches[i].is_none()
            && state.seizes[i].is_none()
            && state.regrips[i].is_none()
        {
            state.evidence_holds[i] = None;
        }
    }

    // Drive each carried body once, from every hand that carries it; a joined hold is only
    // reported.
    let mut driven = [false; 2];
    let mut observed = [None; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(held) = state.held[i].filter(|_| !driven[i]) else {
            continue;
        };
        let joined = state
            .held
            .iter()
            .flatten()
            .filter(|h| h.same(&held))
            .all(|h| h.joined);
        for holder in state.held.iter_mut().flatten().filter(|h| h.same(&held)) {
            holder.joined = joined;
        }
        let holders = [0, 1].map(|k| state.held[k].filter(|h| h.same(&held)));
        for (k, holder) in holders.iter().enumerate() {
            driven[k] |= holder.is_some();
        }
        // SAFETY: forwarded from the caller.
        let body = match engine.read(held.body, held.object) {
            Ok(body) => body,
            Err(e) => {
                let loss = if held.kind == HeldKind::Door {
                    HoldLoss::DoorGone
                } else {
                    HoldLoss::BodyGone
                };
                lose(state, &held, &mut outcome, loss, e.to_string());
                continue;
            }
        };
        for (k, holder) in holders.iter().enumerate() {
            if holder.is_some() {
                observed[k] = Some(Observation::of(&held, body));
            }
        }
        let holding = holders.iter().flatten().count();
        let mut holds = [None; 2];
        for k in 0..2 {
            let (Some(holder), Some(palm)) = (holders[k], palms[k]) else {
                continue;
            };
            let anchor = holder.grip.anchor(body.pose);
            if holder.joined {
                let grip_target = holder.grip.target(palm.pose);
                // The hand's body carries the object through the joint, so the keep-out moves the
                // hand's goal: the object's centre stays out of the player's body as a carried
                // one does.
                let keep_out = space.as_ref().map_or(Vec3::ZERO, |space| {
                    space.offset(holder.grip.centre_at(grip_target))
                });
                let target = grip_target.point + keep_out;
                outcome.joints[k] = Some(HoldTarget {
                    object: held.object,
                    body: held.body,
                    keep_out,
                    shared: holding > 1,
                });
                let (load, twist) = joints.loads[k];
                outcome.reports[k] = Some(HeldReport {
                    mass_kg: held.mass_kg,
                    liftable: grasp::can_lift(held.mass_kg, holding, config),
                    shared: holding > 1,
                    centre: body.pose.centre,
                    anchor,
                    target,
                    lag_m: anchor.distance(target),
                    load,
                    twist,
                    joined: true,
                });
                record_drive(
                    state,
                    k,
                    &held,
                    DriveRecord {
                        object: held.object.get(),
                        body: held.body,
                        before: body,
                        commanded: None,
                        result: None,
                        report: outcome.reports[k],
                    },
                );
                continue;
            }
            let mut target = holder.grip.target(palm.pose);
            if let Some(space) = &space {
                target.point += space.offset(holder.grip.centre_at(target));
            }
            if grasp::torn_away(anchor, target.point, config.break_distance_m) {
                state.held[k] = None;
                outcome.released[k] = true;
                outcome.lost += 1;
                outcome.losses[k] = Some(HoldLoss::TornAway);
                outcome.error = Some("physical hands: pickup: the object was torn away".to_owned());
                continue;
            }
            holds[k] = Some(HandHold {
                anchor,
                target,
                velocity: palm.point_velocity(target.point),
                angular_velocity: palm.angular_velocity,
            });
        }
        let hands = holds.iter().flatten().count();
        if hands == 0 {
            continue;
        }
        let tuning = carry_tuning(&held, &holds, config).in_real_time(time_scale);
        let mut rigid = body.rigid();
        // A ragdoll's body drags the rest of the ragdoll, so the hands' strength moves it as the
        // whole ragdoll's weight.
        if held.kind == HeldKind::Ragdoll && held.mass_kg.is_finite() && held.mass_kg > 0.0 {
            rigid.inverse_mass = rigid.inverse_mass.min(1.0 / held.mass_kg);
        }
        // The hands ride on the character, so a loose object they can lift takes on the
        // character's own change of velocity outright, and the hands' strength works only on
        // their motion relative to the body: a jump carries the object along instead of tearing
        // it from the hands.
        if held.kind == HeldKind::Prop
            && held.mass_kg * tuning.gravity.length() <= hands as f32 * tuning.hand_force_n
        {
            rigid.velocities.linear += carried;
        }
        let solution = hold::solve(&rigid, holds, dt, &tuning);
        // SAFETY: forwarded from the caller.
        engine.release_joints(&held);
        let result = engine.drive(&held, solution.velocities);
        if let Err(e) = result {
            for (k, hold) in holds.iter().enumerate() {
                if hold.is_some() {
                    record_drive(
                        state,
                        k,
                        &held,
                        DriveRecord {
                            object: held.object.get(),
                            body: held.body,
                            before: body,
                            commanded: Some(solution.velocities),
                            result: Some(result),
                            report: None,
                        },
                    );
                }
            }
            lose(
                state,
                &held,
                &mut outcome,
                HoldLoss::DriveRefused,
                e.to_string(),
            );
            continue;
        }
        for (k, hold) in holds.iter().enumerate() {
            let Some(hold) = hold else {
                continue;
            };
            if let Some(palm) = palms[k] {
                outcome.pins[k] = Some(palm.pose.position + hold.anchor - hold.target.point);
            }
            outcome.reports[k] = Some(HeldReport {
                mass_kg: held.mass_kg,
                liftable: held.mass_kg * tuning.gravity.length()
                    <= hands as f32 * tuning.hand_force_n,
                shared: hands > 1,
                centre: body.pose.centre,
                anchor: hold.anchor,
                target: hold.target.point,
                lag_m: hold.anchor.distance(hold.target.point),
                load: solution.loads[k],
                twist: solution.torque_loads[k],
                joined: false,
            });
            record_drive(
                state,
                k,
                &held,
                DriveRecord {
                    object: held.object.get(),
                    body: held.body,
                    before: body,
                    commanded: Some(solution.velocities),
                    result: Some(result),
                    report: outcome.reports[k],
                },
            );
        }
    }
    unsafe {
        tearing::step(
            state,
            TearFrame {
                palms,
                observed,
                dt,
                world_scale,
            },
            &tearing,
            &mut outcome,
        )
    };
    crate::controllers::physical_hands::impacts::publish(state.held.map(|h| {
        h.filter(|h| h.kind != HeldKind::Ragdoll)
            .map(|h| (h.body, h.object))
    }));
    outcome
}

trait BodyEngine {
    fn read(
        &mut self,
        body: u32,
        object: NonZeroUsize,
    ) -> Result<bodies::BodyState, bodies::BodyError>;
    fn release_joints(&mut self, held: &Held);
    fn drive(&mut self, held: &Held, velocity: Velocities) -> Result<(), bodies::BodyError>;
}

struct NativeBodies;

impl BodyEngine for NativeBodies {
    fn read(
        &mut self,
        body: u32,
        object: NonZeroUsize,
    ) -> Result<bodies::BodyState, bodies::BodyError> {
        unsafe { bodies::read(body, object) }
    }
    fn release_joints(&mut self, held: &Held) {
        unsafe { physics::release_body_joints(held.object, held.body) };
    }
    fn drive(&mut self, held: &Held, velocity: Velocities) -> Result<(), bodies::BodyError> {
        unsafe { bodies::drive(held.body, held.object, velocity) }
    }
}

#[cfg(test)]
mod tests;

pub(super) struct BodyGrab {
    pub hand: Hand,
    pub reach: Reach,
    pub palm: PalmMotion,
    pub joined: bool,
}

pub(super) fn take_body(
    state: &mut PickupState,
    grab: BodyGrab,
    body: Result<bodies::BodyState, bodies::BodyError>,
    config: &PickupTuning,
    outcome: &mut TickOutcome,
) -> ConsumerResult {
    let BodyGrab {
        hand,
        reach,
        palm,
        joined,
    } = grab;
    let i = hand.index();
    match body {
        Ok(body) => match body
            .mass_kg
            .filter(|&m| grasp::can_take(&reach.kind, m, config))
        {
            Some(bone_kg) => {
                let mass_kg = held_mass(&reach.kind, bone_kg);
                let (kind, joined) =
                    state.body_mode(reach.object, reach.body, HeldKind::of(&reach.kind), joined);
                state.held[i] = Some(Held {
                    object: reach.object,
                    body: reach.body,
                    grip: Grip::of(palm.pose, body.pose, reach.point),
                    mass_kg,
                    joined,
                    kind,
                });
                outcome.taken[i] = true;
                if let TargetKind::Character(target) = reach.kind {
                    tracing::info!(
                        hand = hand.label(),
                        body = reach.body,
                        bone_kg,
                        mass_kg,
                        hostile = target.hostile,
                        joined,
                        "physical hands: characters: a hand took hold of a ragdoll"
                    );
                }
                if matches!(reach.kind, TargetKind::Articulated { .. }) {
                    tracing::info!(
                        hand = hand.label(),
                        body = reach.body,
                        mass_kg,
                        joined,
                        "physical hands: a hand took hold of an articulated object"
                    );
                    outcome.articulated_grips += 1;
                }
                ConsumerResult::BodyAccepted { joined }
            }
            None => {
                outcome.refuse(
                    hand,
                    reach.point,
                    match body.mass_kg {
                        None => "physical hands: pickup: the reached body has no finite dynamic mass"
                            .to_owned(),
                        Some(mass) if !mass.is_finite() || mass <= 0.0 => {
                            "physical hands: pickup: the reached body's mass is invalid".to_owned()
                        }
                        Some(mass) => format!(
                            "physical hands: pickup: the reached body is {mass:.0} kg, above the supported {:.0} kg grip limit",
                            if matches!(reach.kind, TargetKind::Articulated { .. }) {
                                config.max_articulated_mass_kg
                            } else {
                                config.max_mass_kg
                            },
                        ),
                    },
                );
                ConsumerResult::FixedOrTooHeavy
            }
        },
        Err(e) => {
            outcome.refuse(hand, reach.point, e.to_string());
            ConsumerResult::BodyRefused(e)
        }
    }
}

/// The mass a hand holding a body of `bone_kg` reached as `kind` carries: the whole ragdoll's for
/// a body of a character's ragdoll, which drags the rest of it, and the body's own otherwise.
pub(super) fn held_mass(kind: &TargetKind, bone_kg: f32) -> f32 {
    match kind {
        TargetKind::Character(target) if target.ragdoll && target.mass_kg.is_finite() => {
            target.mass_kg.max(bone_kg)
        }
        _ => bone_kg,
    }
}

/// The change in the carrier's velocity since the last tick that a carried object takes on, scaled
/// by [`PickupTuning::carrier_motion_share`] and capped at the hold's top speed; zero unless both
/// velocities are known and finite.
fn carrier_change(previous: Option<Vec3>, now: Option<Vec3>, config: &PickupTuning) -> Vec3 {
    let (Some(previous), Some(now)) = (previous, now) else {
        return Vec3::ZERO;
    };
    let change = now - previous;
    if !change.is_finite() {
        return Vec3::ZERO;
    }
    (change * config.carrier_motion_share.clamp(0.0, 1.0))
        .clamp_length_max(config.max_speed_mps.max(0.0))
}

pub(super) fn carry_tuning(
    held: &Held,
    holds: &[Option<HandHold>; 2],
    config: &PickupTuning,
) -> HoldTuning {
    if held.kind == HeldKind::Prop {
        HoldTuning::carrying(config, held.mass_kg, holds)
    } else {
        HoldTuning::of(config)
    }
}

fn record_drive(state: &PickupState, index: usize, held: &Held, drive: DriveRecord) {
    if let Some(key) = state.evidence_holds[index]
        .filter(|key| key.target.object == held.object && key.target.body == held.body)
    {
        diagnostics::driven(Hand::BOTH[index], key, drive);
    }
}

/// Let go of `held` from the palm `palm`, and return the throw's speed: the grab point leaves with
/// the palm's velocity at it, scaled and capped, and the body keeps its spin. An object one hand
/// cannot lift, or one let go by an untracked hand, keeps the velocity it has.
///
/// # Safety
///
/// Main thread, outside the physics step.
unsafe fn throw(
    held: &Held,
    palm: Option<PalmMotion>,
    config: &PickupTuning,
) -> Result<Option<(f32, Velocities)>, bodies::BodyError> {
    let body = unsafe { bodies::read(held.body, held.object) }?;
    let Some(palm) = palm.filter(|_| grasp::can_lift(held.mass_kg, 1, config)) else {
        return Ok(None);
    };
    let anchor = held.grip.anchor(body.pose);
    let point = grasp::throw_velocity(palm.point_velocity(anchor), config);
    let spin = body.velocities.angular;
    let linear = (point - spin.cross(anchor - body.pose.centre))
        .clamp_length_max(config.max_throw_speed_mps);
    let velocities = Velocities {
        linear,
        angular: spin,
    };
    unsafe { bodies::drive(held.body, held.object, velocities) }?;
    Ok(Some((point.length(), velocities)))
}

/// Let go of `held` in every hand that holds it, as a lost hold.
fn lose(
    state: &mut PickupState,
    held: &Held,
    outcome: &mut TickOutcome,
    loss: HoldLoss,
    why: String,
) {
    for hand in Hand::BOTH {
        let i = hand.index();
        if state.held[i].is_some_and(|h| h.same(held)) {
            state.held[i] = None;
            outcome.released[i] = true;
            outcome.losses[i] = Some(loss);
        }
    }
    outcome.lost += 1;
    outcome.error = Some(why);
}
