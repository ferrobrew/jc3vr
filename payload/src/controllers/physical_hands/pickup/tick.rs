//! The pickups' simulation tick: take hold and let go as the input tick asked, keep the grips on
//! latched doors, and drive what the hands carry.

use std::time::Instant;

use glam::Vec3;
use jc3gi::vehicle::vehicle::DoorSlot;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        STALE_AFTER, bodies,
        characters::{CharactersConfig, Seize},
        config::PickupConfig,
        doors::{self, Latch},
        grasp::{self, Grip, PalmMotion, Reach, TargetKind},
        hold::{self, HandHold, HoldTuning, PlayerKeepOut},
        physics::HoldTarget,
        pickup::{
            HeldReport,
            diagnostics::{self, ConsumerResult, DriveRecord},
            grips::{keep_latch, keep_regrip, keep_seize, shove_on_release},
            state::{Held, HeldKind, PickupState, Request},
        },
        rigid::Velocities,
    },
};

/// What the hand bodies tell a pickup tick about the joints.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct JointInputs {
    /// Which hands take what they grab through a joint: those their bodies drive, with
    /// `joint_grabs`.
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
    pub dt: f32,
    /// How far a hand gripping a latched door pulls it before the latch gives, world metres.
    pub unlatch_pull_m: f32,
    /// The VR world scale.
    pub world_scale: f32,
    pub characters: CharactersConfig,
    /// What each hand reaches this tick, from which a hand that yanked a character takes hold of
    /// its ragdoll.
    pub reaches: [Option<Reach>; 2],
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
    /// Holds lost this tick.
    pub lost: u64,
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

impl TickOutcome {
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
    config: &PickupConfig,
) -> TickOutcome {
    let TickInputs {
        palms,
        space,
        joints,
        dt,
        unlatch_pull_m,
        world_scale,
        characters: character_config,
        reaches,
    } = inputs;
    let mut outcome = TickOutcome::default();
    for hand in Hand::BOTH {
        let i = hand.index();
        if joints.lost[i] && state.held[i].is_some_and(|h| h.joined) {
            state.held[i] = None;
            outcome.released[i] = true;
            outcome.lost += 1;
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
                state.evidence_holds[i] = None;
                if state.latches[i].take().is_some() {
                    outcome.released[i] = true;
                }
                if let Some(seize) = state.seizes[i].take() {
                    outcome.released[i] = true;
                    if let Some(palm) = palms[i] {
                        shove_on_release(hand, &seize, &palm, &character_config);
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
                            Ok(speed) => outcome.throw = Some(speed),
                            Err(e) => outcome.error = Some(e.to_string()),
                        }
                    }
                    outcome.released[i] = true;
                }
            }
            Some(request @ (Request::Grab(_) | Request::ObservedGrab(_))) => 'grab: {
                let (reach, evidence) = request.grab().expect("grab request");
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
                        match unsafe { Latch::take(door, reach.point) } {
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
                    // SAFETY: forwarded from the caller; the character was reached by this
                    // tick's casts.
                    match unsafe { Seize::take(target, reach.point) } {
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
                let body = unsafe { bodies::read(reach.body, reach.object) };
                let result = take_body(
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
        unsafe { keep_latch(state, hand, palms[i], &joints, unlatch_pull_m, &mut outcome) };
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
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(held) = state.held[i].filter(|_| !driven[i]) else {
            continue;
        };
        let holders = [0, 1].map(|k| state.held[k].filter(|h| h.same(&held)));
        for (k, holder) in holders.iter().enumerate() {
            driven[k] |= holder.is_some();
        }
        // SAFETY: forwarded from the caller.
        let body = match unsafe { bodies::read(held.body, held.object) } {
            Ok(body) => body,
            Err(e) => {
                lose(state, &held, &mut outcome, e.to_string());
                continue;
            }
        };
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
        let solution = hold::solve(&body.rigid(), holds, dt, &HoldTuning::of(config));
        // SAFETY: forwarded from the caller.
        let result = unsafe { bodies::drive(held.body, held.object, solution.velocities) };
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
            lose(state, &held, &mut outcome, e.to_string());
            continue;
        }
        for (k, hold) in holds.iter().enumerate() {
            let Some(hold) = hold else {
                continue;
            };
            outcome.reports[k] = Some(HeldReport {
                mass_kg: held.mass_kg,
                liftable: grasp::can_lift(held.mass_kg, hands, config),
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
    outcome
}

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
    config: &PickupConfig,
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
            Some(mass_kg) => {
                state.held[i] = Some(Held {
                    object: reach.object,
                    body: reach.body,
                    grip: Grip::of(palm.pose, body.pose, reach.point),
                    mass_kg,
                    joined,
                    kind: HeldKind::of(&reach.kind),
                });
                outcome.taken[i] = true;
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
                    "physical hands: pickup: the body is fixed or too heavy".to_owned(),
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
    config: &PickupConfig,
) -> Result<f32, bodies::BodyError> {
    let body = unsafe { bodies::read(held.body, held.object) }?;
    let Some(palm) = palm.filter(|_| grasp::can_lift(held.mass_kg, 1, config)) else {
        return Ok(0.0);
    };
    let anchor = held.grip.anchor(body.pose);
    let point = grasp::throw_velocity(palm.point_velocity(anchor), config);
    let spin = body.velocities.angular;
    let linear = (point - spin.cross(anchor - body.pose.centre))
        .clamp_length_max(config.max_throw_speed_mps);
    unsafe {
        bodies::drive(
            held.body,
            held.object,
            Velocities {
                linear,
                angular: spin,
            },
        )
    }?;
    Ok(point.length())
}

/// Let go of `held` in every hand that holds it, as a lost hold.
fn lose(state: &mut PickupState, held: &Held, outcome: &mut TickOutcome, why: String) {
    for hand in Hand::BOTH {
        let i = hand.index();
        if state.held[i].is_some_and(|h| h.same(held)) {
            state.held[i] = None;
            outcome.released[i] = true;
        }
    }
    outcome.lost += 1;
    outcome.error = Some(why);
}
