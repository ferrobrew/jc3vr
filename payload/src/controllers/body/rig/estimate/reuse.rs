//! Update-local numerical search reuse. Contact metadata remains on each topology trial.

use crate::controllers::body::rig::estimate::{
    EstimateError,
    contacts::Contact,
    fit::{self, Candidate, Context, Parameters},
    state::{release_trial, topology_better},
    temporal::{self, Envelope},
    work::Work,
};

/// The copied non-contact inputs never change during the lifetime of this cache.
pub(crate) struct Fits<'a> {
    context: Context<'a>,
    envelope: Option<Envelope>,
    entries: [Option<Entry>; 8],
    len: usize,
    optimized: bool,
}

impl<'a> Fits<'a> {
    pub(crate) fn new(context: &Context<'a>, optimized: bool, work: &mut Work) -> Self {
        Self {
            context: context.clone(),
            envelope: optimized.then(|| Envelope::new(context, work)),
            entries: [None; 8],
            len: 0,
            optimized,
        }
    }

    /// Prediction is installed before any search result exists; temporal intervals do not read it.
    pub(crate) fn with_prediction(mut self, predicted: Parameters) -> Self {
        assert_eq!(self.len, 0);
        self.context.predicted = predicted;
        self
    }

    pub(crate) fn constrain(
        &self,
        parameters: &mut Parameters,
        work: &mut Work,
    ) -> Result<(), EstimateError> {
        temporal::constrain_counted(parameters, &self.context, self.envelope.as_ref(), work)
    }

    pub(crate) fn last_contacts(&self) -> [Contact; 2] {
        self.context.contacts
    }

    pub(crate) fn fit_contacts(
        &mut self,
        baseline: [Contact; 2],
        work: &mut Work,
    ) -> Result<(Candidate, [Contact; 2]), EstimateError> {
        let mut best: Option<(Candidate, [Contact; 2])> = None;
        for mask in [0_u8, 1, 2, 3] {
            let trial = release_trial(baseline, mask);
            match self.search(trial, work) {
                Ok(candidate) => {
                    if best.is_none_or(|(held, held_contacts)| {
                        topology_better(candidate, trial, held, held_contacts)
                    }) {
                        best = Some((candidate, trial));
                    }
                }
                Err(EstimateError::SearchExhausted) => {}
                Err(error) => return Err(error),
            }
            if mask == 0
                && best.is_some_and(|(candidate, _)| fit::measurement_admissible(candidate))
            {
                break;
            }
        }
        if let Some((_, contacts)) = best {
            self.context.contacts = contacts;
        }
        best.ok_or(EstimateError::SearchExhausted)
    }

    pub(crate) fn reuse_preliminary(
        &self,
        candidate: Candidate,
        before: [Contact; 2],
        after: [Contact; 2],
        work: &mut Work,
    ) -> bool {
        if self.optimized
            && fit::measurement_admissible(candidate)
            && before
                .into_iter()
                .zip(after)
                .all(|(left, right)| left.same_bits(right))
        {
            work.preliminary_reuses += 1;
            work.search_requests += 1;
            work.logical_evaluations += fit::SEARCH_EVALUATIONS;
            true
        } else {
            false
        }
    }

    fn search(
        &mut self,
        contacts: [Contact; 2],
        work: &mut Work,
    ) -> Result<Candidate, EstimateError> {
        self.context.contacts = contacts;
        let signature = contacts.map(signature);
        work.search_requests += 1;
        if self.optimized
            && let Some(entry) = self.entries[..self.len]
                .iter()
                .flatten()
                .find(|entry| entry.signature == signature)
        {
            work.cache_hits += 1;
            work.logical_evaluations += entry.trials;
            return entry.result;
        }
        let mut context = self.context.clone();
        context.contacts = contacts;
        let before = work.evaluations;
        let result = fit::search_counted(&context, self.envelope.as_ref(), work);
        if self.optimized {
            assert!(
                self.len < self.entries.len(),
                "at most eight normal topology requests exist"
            );
            self.entries[self.len] = Some(Entry {
                signature,
                result,
                trials: work.evaluations - before,
            });
            self.len += 1;
        }
        result
    }
}

#[derive(Copy, Clone)]
struct Entry {
    signature: [Signature; 2],
    result: Result<Candidate, EstimateError>,
    trials: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Signature {
    Released,
    Swinging([u32; 3]),
    Planted([u32; 3], [u32; 4]),
}

/// Search reads active positions and planted rotations, not support IDs or swing history.
fn signature(contact: Contact) -> Signature {
    if !contact.active() {
        Signature::Released
    } else if contact.planted() {
        Signature::Planted(
            contact.pose.position.to_array().map(f32::to_bits),
            contact.pose.rotation.to_array().map(f32::to_bits),
        )
    } else {
        Signature::Swinging(contact.pose.position.to_array().map(f32::to_bits))
    }
}
