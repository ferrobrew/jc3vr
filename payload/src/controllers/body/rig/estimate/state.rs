//! Transactional simulation commits, completed-motion accounting, and bounded contact transitions.

use glam::{Quat, Vec3};

use crate::controllers::{
    body::rig::{
        estimate::{
            Confidence, EstimateError, GaitOutput, Provenance, Quality, RootRequest, Snapshot,
            Stance, Temporal, Transition, Update,
            contacts::{
                Contact, ContactState, SWING_SECONDS, Support, rest_feet, rigid_upright, yaw,
            },
            fit::{
                self, Candidate, Context, HAND_TOLERANCE, NECK_TOLERANCE, PARAMETERS, Parameters,
                difference, horizontal,
            },
            reuse::Fits,
            temporal::{self, Envelope, TemporalStatus},
            work::Work,
        },
        observation::{Observation, PairSide},
        root::owner::RootOwner,
        torso::{EYES_TO_NECK_PIVOT, head_heading, wrap},
    },
    frame::Hand,
};

/// One shadow state per simulation stream. Failed corrections never mutate the committed state.
#[derive(Debug, Clone, Default)]
pub(crate) struct Estimator {
    state: Option<Temporal>,
    last_observation: Option<Observation>,
    last_support: Option<Support>,
    last_dt: f32,
    last_work: Work,
    #[cfg(test)]
    reference_mode: bool,
}

impl Estimator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn seed_nonviable_rate_for_test(&mut self, axis: usize, position: f32, rate: f32) {
        if let Some(state) = self.state.as_mut() {
            state.parameters.0[axis] = position;
            state.rates[axis] = rate;
        }
    }

    #[cfg(test)]
    pub(crate) fn seed_contacts_for_test(&mut self, contacts: [Contact; 2], hip_height: f32) {
        if let Some(state) = self.state.as_mut() {
            state.contacts = contacts;
            state.parameters.0[5] = hip_height;
            state.rates = [0.0; PARAMETERS];
        }
    }

    #[cfg(test)]
    pub(crate) fn seed_candidate_for_test(
        &mut self,
        candidate: Candidate,
        rates: [f32; PARAMETERS],
        contacts: [Contact; 2],
    ) {
        let state = self
            .state
            .as_mut()
            .expect("the fixture has committed a state");
        state.parameters = candidate.parameters;
        state.rates = rates;
        state.contacts = contacts;
        state.snapshot.stance = candidate.parameters.stance(state.snapshot.stance.origin.y);
        state.snapshot.body = candidate.body;
        state.snapshot.feet = contacts.map(|contact| contact.pose);
        state.snapshot.contacts = contacts.map(|contact| contact.state);
    }

    #[cfg(test)]
    pub(crate) fn rates_for_test(&self) -> [f32; PARAMETERS] {
        self.state
            .as_ref()
            .expect("the fixture has committed a state")
            .rates
    }

    #[cfg(test)]
    pub(crate) fn state_for_test(&self) -> Option<(Parameters, [f32; PARAMETERS], [Contact; 2])> {
        self.state
            .map(|state| (state.parameters, state.rates, state.contacts))
    }

    /// Read-only presentation. Repeated renders cannot integrate contacts, gait, or confidence.
    pub(crate) fn snapshot(&self) -> Option<&Snapshot> {
        self.state.as_ref().map(|state| &state.snapshot)
    }

    /// Work remains available on failure without changing the committed pose or input identity.
    pub(crate) fn last_work(&self) -> Work {
        self.last_work
    }

    #[cfg(test)]
    pub(crate) fn set_reference_mode_for_test(&mut self, reference: bool) {
        self.reference_mode = reference;
    }

    pub(crate) fn update(&mut self, update: Update<'_>) -> Result<Snapshot, EstimateError> {
        #[cfg(test)]
        let mut reference = (!self.reference_mode && !crate::controllers::body::rig::estimate::tests::profile::timing_active())
            .then(|| {
                let mut reference = self.clone();
                reference.reference_mode = true;
                reference
            });
        #[cfg(test)]
        let reference_update = Update { ..update };
        let mut work = Work::default();
        let result = self.update_counted(update, &mut work);
        self.last_work = work;
        #[cfg(test)]
        if let Some(reference) = reference.as_mut() {
            let expected = reference.update(reference_update);
            crate::controllers::body::rig::estimate::tests::cost::assert_same_result(result, expected);
            crate::controllers::body::rig::estimate::tests::cost::assert_same_estimator(self, reference);
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn assert_same_state_for_test(&self, other: &Self) {
        match (self.state, other.state) {
            (Some(mut left), Some(mut right)) => {
                left.snapshot.quality.evaluations = 0;
                right.snapshot.quality.evaluations = 0;
                crate::controllers::body::rig::estimate::tests::cost::assert_same_snapshot(left.snapshot, right.snapshot);
                assert_eq!(left.parameters.0.map(f32::to_bits), right.parameters.0.map(f32::to_bits));
                assert_eq!(left.rates.map(f32::to_bits), right.rates.map(f32::to_bits));
                assert!(left.contacts.into_iter().zip(right.contacts)
                    .all(|(left, right)| left.same_bits(right)));
            }
            (None, None) => {}
            _ => panic!("reference and optimized committed-state presence differs"),
        }
        assert!(crate::controllers::body::rig::estimate::tests::retained::same_observation_bits(
            self.last_observation.as_ref(), other.last_observation.as_ref(),
        ));
        assert!(crate::controllers::body::rig::estimate::tests::retained::same_support_bits(
            self.last_support, other.last_support,
        ));
        assert_eq!(self.last_dt.to_bits(), other.last_dt.to_bits());
    }

    fn update_counted(&mut self, update: Update<'_>, work: &mut Work) -> Result<Snapshot, EstimateError> {
        update.anatomy.validate()?;
        let observation = update.observation;
        let raw = observation.raw();
        let previous = *observation.pair().side(PairSide::Previous);
        let current = *observation.pair().side(PairSide::Current);
        observation.check_at(update.now, update.max_age, current.stamp.epochs)?;
        if !update.dt.is_finite() || update.dt <= 0.0 || update.dt > 0.1 {
            return Err(EstimateError::InvalidTimeStep);
        }
        if update.anatomy.calibration != current.stamp.epochs.calibration {
            return Err(EstimateError::InvalidAnatomy);
        }
        if !rigid_upright(previous.root) || !rigid_upright(current.root) {
            return Err(EstimateError::InvalidRoot);
        }
        if !matches!(current.owner, RootOwner::Player | RootOwner::Seated) {
            return Err(EstimateError::Ownership);
        }
        if let Some(surface) = update.support {
            surface.validate(previous.stamp, current.stamp)?;
        }
        let old = self.state;
        let reseed =
            old.is_none() || matches!(update.transition, Transition::Reseed | Transition::Teleport);
        if let Some(old) = old {
            let provenance = old.snapshot.provenance;
            if current.stamp.step < provenance.current.step {
                return Err(EstimateError::ReversedStep);
            }
            if current.stamp.step == provenance.current.step {
                if self.last_observation.as_ref().is_some_and(|last| {
                    crate::controllers::body::rig::estimate::equivalence::same_observation(
                        last,
                        observation,
                    )
                }) && self.last_support == update.support
                    && self.last_dt == update.dt
                    && provenance.transition == update.transition
                {
                    return Ok(old.snapshot);
                }
                return Err(EstimateError::ChangedCommittedStep);
            }
            if !reseed {
                let collapsed_recenter =
                    update.transition == Transition::Recenter && previous.stamp == current.stamp;
                if current.stamp.step != provenance.current.step + 1
                    || (!collapsed_recenter && previous.stamp.step != provenance.current.step)
                {
                    return Err(EstimateError::MissedStep);
                }
                let epochs = provenance.current.epochs;
                let next = current.stamp.epochs;
                if epochs.calibration != next.calibration
                    || epochs.world_scale != next.world_scale
                    || epochs.authority != next.authority
                {
                    return Err(EstimateError::EpochRequiresReseed);
                }
                if epochs.origin != next.origin && update.transition != Transition::Recenter {
                    return Err(EstimateError::UnexpectedRecenter);
                }
                if raw.head.stamp.sequence < provenance.sample.sequence
                    || raw.head.stamp.display_time_ns < provenance.sample.display_time_ns
                    || (raw.head.stamp.sequence == provenance.sample.sequence
                        && raw.head.stamp.display_time_ns != provenance.sample.display_time_ns)
                {
                    return Err(EstimateError::ReversedSample);
                }
            }
        }
        if !reseed
            && update.transition != Transition::Recenter
            && let (Some(last), Some(next)) = (self.last_support, update.support)
            && last.key() == next.key()
            && last.current != next.previous
        {
            return Err(EstimateError::SupportPairMismatch);
        }
        let frames = observation.frames(PairSide::Current)?;
        let head = observation.head_world(PairSide::Current)?;
        let head_yaw = head_heading(head.orientation).ok_or(EstimateError::NoFiniteCandidate)?;
        let hands =
            [Hand::Left, Hand::Right].map(|hand| observation.grip_world(hand, PairSide::Current));
        let hands = [hands[0]?, hands[1]?];
        let root_position = current.root.transform_point3(Vec3::ZERO);
        let previous_position = previous.root.transform_point3(Vec3::ZERO);
        let root_rotation = current.root.to_scale_rotation_translation().1;
        let root_yaw = yaw(root_rotation);
        let previous_yaw = yaw(previous.root.to_scale_rotation_translation().1);
        let root_stance = current.root.transform_point3(update.anatomy.root_to_stance);
        let floor_y = update
            .support
            .map_or(root_stance.y, |surface| surface.current.w_axis.y);
        let initial_stance = Stance {
            origin: Vec3::new(root_stance.x, floor_y, root_stance.z),
            yaw: root_yaw,
        };
        let actual = horizontal(root_position - previous_position);
        let surface_transport = if !reseed {
            update
                .support
                .filter(|surface| {
                    self.last_support
                        .is_some_and(|last| last.key() == surface.key())
                })
                .map_or(Vec3::ZERO, |surface| {
                    let local = surface
                        .previous
                        .inverse()
                        .transform_point3(previous_position);
                    horizontal(surface.current.transform_point3(local) - previous_position)
                })
        } else {
            Vec3::ZERO
        };
        let completed = if reseed || update.transition == Transition::Recenter {
            Vec3::ZERO
        } else {
            actual - surface_transport
        };
        let previous_parameters = old.filter(|_| !reseed).map_or_else(
            || Parameters::neutral(initial_stance, update.anatomy),
            |old| {
                let mut parameters = old
                    .parameters
                    .translated(actual, wrap(root_yaw - previous_yaw));
                let previous_origin = old.parameters.stance(old.snapshot.stance.origin.y).origin;
                let carried = current
                    .root
                    .transform_point3(previous.root.inverse().transform_point3(previous_origin));
                parameters.0[0] = carried.x;
                parameters.0[1] = carried.z;
                parameters
            },
        );
        let root_turn = root_rotation * previous.root.to_scale_rotation_translation().1.inverse();
        let mut rates = old
            .filter(|_| !reseed && update.transition != Transition::Recenter)
            .map_or([0.0; PARAMETERS], |state| {
                carry_planar_rates(state.rates, root_turn)
            });
        let mut predicted = previous_parameters;
        for (axis, rate) in rates.iter().copied().enumerate() {
            predicted.0[axis] += rate * update.dt * (-update.dt / 0.18).exp();
        }
        let mut contacts = if reseed {
            rest_feet(
                initial_stance,
                update.anatomy.character,
                update.anatomy.ankle_height,
            )
            .map(|pose| Contact::seed(pose, update.support, update.anatomy.ankle_height))
        } else {
            old.expect("a continuing estimate has a state").contacts
        };
        for contact in &mut contacts {
            contact.carry(update.support, update.dt);
        }
        let tracking_floor = frames.cockpit_to_world(Vec3::ZERO);
        let mut context = Context {
            anatomy: update.anatomy,
            neck: head.position + head.orientation * EYES_TO_NECK_PIVOT * frames.world_scale,
            head_yaw,
            root_yaw,
            hands,
            tracking_floor,
            world_scale: frames.world_scale,
            predicted,
            previous: previous_parameters,
            rates,
            dt: update.dt,
            continuity: !reseed,
            floor_y,
            contacts,
        };
        let mut temporal_status =
            temporal::prepare_counted(&mut context, work);
        #[cfg(not(test))]
        let optimized = true;
        #[cfg(test)]
        let optimized = !self.reference_mode;
        let prepared = Fits::new(&context, optimized, work);
        prepared.constrain(&mut predicted, work)?;
        context.predicted = predicted;
        // Prediction settles before the immutable numerical search context is copied.
        let mut fits = prepared.with_prediction(predicted);
        let mut held_contacts = old
            .filter(|_| !reseed)
            .map_or(contacts, |state| state.contacts);
        for contact in &mut held_contacts {
            contact.carry(update.support, 0.0);
        }
        let preliminary_result = fits.fit_contacts(contacts, work);
        context.contacts = fits.last_contacts();
        let (preliminary, selected_contacts) = match preliminary_result {
            Ok(result) => result,
            Err(EstimateError::SearchExhausted) if !reseed => {
                let (candidate, recovered, _) =
                    recover_constraints_counted(&mut context, held_contacts, optimized, work)?;
                temporal_status = TemporalStatus::ConstraintRecovery;
                (candidate, recovered)
            }
            Err(error) => return Err(error),
        };
        contacts = selected_contacts;
        // Relative hip velocity removes observed support transport before predicting local placement.
        let carried_hips = old
            .filter(|_| !reseed && update.transition != Transition::Recenter)
            .map(|old| {
                old.snapshot.body.hips.map(|hip| {
                    update
                        .support
                        .filter(|surface| {
                            self.last_support
                                .is_some_and(|last| last.key() == surface.key())
                        })
                        .map_or(hip, |surface| surface.carry_point(hip))
                })
            });
        let touchdown_hips = predict_hips(preliminary, carried_hips, completed, update.dt);
        let recovering = temporal_status == TemporalStatus::ConstraintRecovery;
        if !recovering {
            transition_contacts_counted(&mut contacts, preliminary, touchdown_hips, &update, work);
        }
        // Every transition, including release and lift, precedes this final fit.
        context.contacts = contacts;
        let final_result = if recovering
            || fits.reuse_preliminary(preliminary, selected_contacts, contacts, work)
        {
            Ok((preliminary, contacts))
        } else {
            let result = fits.fit_contacts(contacts, work);
            context.contacts = fits.last_contacts();
            result
        };
        let (mut candidate, selected_contacts) = match final_result {
            Ok(result) => result,
            Err(EstimateError::SearchExhausted) if !reseed => {
                let (candidate, recovered, _) =
                    recover_constraints_counted(&mut context, held_contacts, optimized, work)?;
                temporal_status = TemporalStatus::ConstraintRecovery;
                (candidate, recovered)
            }
            Err(error) => return Err(error),
        };
        contacts = selected_contacts;
        // The final fit either validates a tentative plant or returns its explicit relaxation.
        context.contacts = contacts;
        work.check();
        candidate = fit::evaluate_counted(candidate.parameters, &context, work)
            .ok_or(EstimateError::SearchExhausted)?;
        let (ambiguity, _) = fit::ambiguity_counted(candidate, &context, work);
        rates = context.rates;
        for (axis, rate) in rates.iter_mut().enumerate() {
            *rate = difference(
                candidate.parameters.0[axis],
                previous_parameters.0[axis],
                axis,
            ) / update.dt;
            if reseed || update.transition == Transition::Recenter
                || temporal_status == TemporalStatus::ConstraintRecovery
            { *rate = 0.0; }
        }
        fit::publish_feet(&mut candidate, &mut contacts);
        let stance = candidate.parameters.stance(floor_y);
        let touchdown_hips = predict_hips(candidate, carried_hips, completed, update.dt);
        let feasible = candidate.neck_error <= NECK_TOLERANCE * frames.world_scale * 1.05
            && candidate
                .arm_errors
                .iter()
                .flatten()
                .all(|error| *error <= HAND_TOLERANCE * frames.world_scale * 1.05)
            && candidate.neck_limit_error < 0.01
            && candidate
                .contact_errors
                .iter()
                .all(|error| *error <= fit::CONTACT_TOLERANCE);
        let distance = old
            .filter(|_| !reseed)
            .map_or(0.0, |old| old.snapshot.gait.distance)
            + completed.length();
        let stride = update.anatomy.character.leg(0) * 0.75;
        let permitted = current.owner == RootOwner::Player;
        let snapshot = Snapshot {
            provenance: Provenance {
                sample: raw.head.stamp, previous: previous.stamp, current: current.stamp,
                support: update.support.map(Support::key), transition: update.transition,
                fresh_measurement: old.is_none_or(|old| old.snapshot.provenance.sample.sequence != raw.head.stamp.sequence),
            },
            stance, body: candidate.body, player_shoulders: candidate.player_shoulders,
            remapped_hands: candidate.remapped_hands, feet: contacts.map(|contact| contact.pose),
            contacts: contacts.map(|contact| contact.state),
            root: RootRequest {
                displacement: horizontal(stance.origin - root_stance),
                turn: wrap(stance.yaw - root_yaw), permitted,
            },
            gait: GaitOutput { completed_across_support: completed, distance, phase: (distance / stride).rem_euclid(1.0), touchdown_hips },
            quality: Quality {
                head: raw.head.tracked.into(),
                hands: raw.grips.map(|grip| grip.map(|grip| grip.tracked.into())),
                neck_residual_m: candidate.neck_error, hand_reach_residual_m: candidate.arm_errors,
                neck_limit_residual_rad: candidate.neck_limit_error, contact_residual_m: candidate.contact_errors,
                feasible, reduced_authority: !feasible || contacts.iter().any(|contact| !contact.planted())
                    || temporal_status != TemporalStatus::Normal,
                yaw_confidence: Confidence::Low,
                yaw_ambiguity: ambiguity, lower_body_confidence: Confidence::Low, evaluations: work.evaluations,
                search: if fit::measurement_admissible(candidate) { fit::SearchStatus::Admissible }
                    else { fit::SearchStatus::ResidualAfterBudget },
                temporal: temporal_status,
            },
        };
        self.state = Some(Temporal {
            parameters: candidate.parameters,
            rates,
            contacts,
            snapshot,
        });
        self.last_observation = Some(*observation);
        self.last_support = update.support;
        self.last_dt = update.dt;
        Ok(snapshot)
    }
}

/// Stance translation rates are world vectors; pelvis compensation and articulation rates stay local.
pub(crate) fn carry_planar_rates(
    mut rates: [f32; PARAMETERS],
    rotation: Quat,
) -> [f32; PARAMETERS] {
    let planar = rotation * Vec3::new(rates[0], 0.0, rates[1]);
    rates[0] = planar.x;
    rates[1] = planar.z;
    rates
}

pub(crate) fn release_trial(baseline: [Contact; 2], mask: u8) -> [Contact; 2] {
    let mut trial = baseline;
    for (side, contact) in trial.iter_mut().enumerate() {
        if mask & (1 << side) != 0 && contact.active() {
            contact.release();
        }
    }
    trial
}

pub(crate) fn fit_contacts(
    context: &mut Context<'_>,
    baseline: [Contact; 2],
) -> Result<(Candidate, [Contact; 2], usize), EstimateError> {
    let mut work = Work::default();
    let mut fits = Fits::new(context, true, &mut work);
    let result = fits.fit_contacts(baseline, &mut work);
    #[cfg(test)]
    {
        let mut expected_work = Work::default();
        let mut reference = Fits::new(context, false, &mut expected_work);
        let expected = reference.fit_contacts(baseline, &mut expected_work);
        crate::controllers::body::rig::estimate::tests::cost::assert_same_debug(&result, &expected);
    }
    context.contacts = fits.last_contacts();
    result.map(|(candidate, contacts)| (candidate, contacts, work.evaluations))
}

/// Hold the carried pose only after normal search fails; every retained contact is revalidated.
pub(crate) fn recover_constraints(
    context: &mut Context<'_>,
    baseline: [Contact; 2],
) -> Result<(Candidate, [Contact; 2], usize), EstimateError> {
    recover_constraints_counted(context, baseline, true, &mut Work::default())
}

fn recover_constraints_counted(
    context: &mut Context<'_>,
    baseline: [Contact; 2],
    optimized: bool,
    work: &mut Work,
) -> Result<(Candidate, [Contact; 2], usize), EstimateError> {
    let rates = context.rates;
    let predicted = context.predicted;
    let original_contacts = context.contacts;
    context.rates = [0.0; PARAMETERS];
    context.predicted = context.previous;
    let envelope = optimized.then(|| Envelope::new(context, work));
    let mut best: Option<(Candidate, [Contact; 2])> = None;
    for mask in [0_u8, 1, 2, 3] {
        let trial = release_trial(baseline, mask);
        context.contacts = trial;
        work.check();
        if temporal::accepts_counted(context.previous, context, envelope.as_ref(), work)
            && let Some(candidate) = fit::evaluate_counted(context.previous, context, work)
            && best.is_none_or(|(held, held_contacts)| {
                topology_better(candidate, trial, held, held_contacts)
            })
        {
            best = Some((candidate, trial));
        }
    }
    if let Some((candidate, contacts)) = best {
        context.contacts = contacts;
        Ok((candidate, contacts, 4))
    } else {
        context.rates = rates;
        context.predicted = predicted;
        context.contacts = original_contacts;
        Err(EstimateError::SearchExhausted)
    }
}

pub(crate) fn topology_better(
    candidate: Candidate,
    contacts: [Contact; 2],
    held: Candidate,
    held_contacts: [Contact; 2],
) -> bool {
    let admissible = fit::measurement_admissible(candidate);
    let held_admissible = fit::measurement_admissible(held);
    if admissible != held_admissible {
        return admissible;
    }
    let count = contacts.iter().filter(|contact| contact.active()).count();
    let held_count = held_contacts
        .iter()
        .filter(|contact| contact.active())
        .count();
    if (admissible || (candidate.tracking_cost - held.tracking_cost).abs() <= 0.05)
        && count != held_count
    {
        return count > held_count;
    }
    candidate.tracking_cost + 1.0e-5 < held.tracking_cost
        || ((candidate.tracking_cost - held.tracking_cost).abs() <= 1.0e-5
            && candidate.secondary_cost < held.secondary_cost)
}

pub(crate) fn predict_hips(
    candidate: Candidate,
    carried_previous: Option<[Vec3; 2]>,
    completed: Vec3,
    dt: f32,
) -> [Vec3; 2] {
    [0, 1].map(|side| {
        let velocity = carried_previous.map_or(completed / dt, |previous| {
            (candidate.body.hips[side] - previous[side]) / dt
        });
        candidate.body.hips[side] + velocity.clamp_length_max(3.0) * SWING_SECONDS
    })
}

pub(crate) fn transition_contacts(
    contacts: &mut [Contact; 2],
    candidate: Candidate,
    predicted_hips: [Vec3; 2],
    update: &Update<'_>,
) {
    transition_contacts_counted(contacts, candidate, predicted_hips, update, &mut Work::default());
}

fn transition_contacts_counted(
    contacts: &mut [Contact; 2],
    candidate: Candidate,
    predicted_hips: [Vec3; 2],
    update: &Update<'_>,
    work: &mut Work,
) {
    let Some(surface) = update.support else {
        return;
    };
    let stance = candidate.parameters.stance(surface.current.w_axis.y);
    let rest = rest_feet(
        stance,
        update.anatomy.character,
        update.anatomy.ankle_height,
    );
    let has_swing = contacts
        .iter()
        .any(|contact| matches!(contact.state, ContactState::Swinging { .. }));
    let mut selected = None;
    let mut worst = 0.12;
    if !has_swing {
        for side in 0..2 {
            let error = horizontal(rest[side].position - contacts[side].pose.position).length()
                + wrap(yaw(contacts[side].pose.rotation) - stance.yaw).abs() * 0.15;
            if !contacts[side].active() || error > worst {
                selected = Some(side);
                worst = error;
            }
        }
    }
    for side in 0..2 {
        let swinging = matches!(contacts[side].state, ContactState::Swinging { .. });
        if !swinging && selected != Some(side) {
            continue;
        }
        let lead =
            horizontal(predicted_hips[side] - candidate.body.hips[side]).clamp_length_max(0.25);
        let desired = rest[side].position + lead;
        if let Some(endpoint) = surface.foothold(desired, stance.yaw, update.anatomy.ankle_height) {
            let mut predicted = candidate;
            predicted.body.hips = predicted_hips;
            if fit::endpoint_feasible_counted(
                predicted,
                surface.world(endpoint),
                contacts[1 - side].pose.position,
                update.anatomy.character,
                side,
                work,
            ) {
                if swinging {
                    contacts[side].retarget(surface, endpoint, update.dt);
                } else {
                    contacts[side].lift(surface, endpoint);
                }
            }
        }
    }
    validate_touchdowns(contacts, candidate, update, work);
}

fn validate_touchdowns(
    contacts: &mut [Contact; 2],
    candidate: Candidate,
    update: &Update<'_>,
    work: &mut Work,
) {
    let Some(surface) = update.support else {
        return;
    };
    for side in 0..2 {
        let endpoint = contacts[side].endpoint();
        let valid = surface.contains(endpoint)
            && fit::endpoint_feasible_counted(
                candidate,
                surface.world(endpoint),
                contacts[1 - side].pose.position,
                update.anatomy.character,
                side,
                work,
            );
        contacts[side].touchdown(surface, valid);
    }
}
