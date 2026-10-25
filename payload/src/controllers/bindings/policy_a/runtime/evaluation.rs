//! Policy evaluation, recording, and verification of recorded replay decisions.

use std::time::Duration;

use tracking_format::input::{
    GroupOrder, InputGroup, InputStamp,
    policy::{PolicyEvaluation, PolicyOutcome},
};

use crate::{
    controllers::{
        bindings::{
            controls::ButtonControl,
            policy_a::{
                Arbitration, BindingGeneration, ClockEpoch, Control, Disposition, GameplayContext,
                Observation, ObservationStamp, ObservationStatus, PolicyA, ResetReason, SingleEvent,
                SystemCommand,
                attribution::{SourceIdentity, raw_location},
                recording,
                runtime::{Delivery, Runtime, Tick, complete_inventory, group_neutral, raw_evidence},
            },
        },
        config::BindingsConfig,
    },
    vr::input::semantic::{SCHEMA_VERSION, history},
};

#[cfg(test)]
use crate::controllers::bindings::policy_a::InputEvidence;

pub(super) struct EvaluationInput<'a> {
    pub(super) context: u64,
    pub(super) gameplay: bool,
    pub(super) config: &'a BindingsConfig,
    pub(super) expected: Option<PolicyEvaluation>,
}

impl Runtime {
    pub(super) fn replay_policy(
        &mut self,
        expected: PolicyEvaluation,
        config: &BindingsConfig,
        tick: &mut Tick,
    ) {
        let cancellation = !expected.is_valid()
            || expected.action_schema != SCHEMA_VERSION
            || self
                .replay_groups
                .front()
                .is_none_or(|group| group.stamp != expected.stamp)
            || self.config != recording::config(expected.timing)
            || expected.outcome != PolicyOutcome::Evaluated;
        if cancellation {
            tick.commands.clear();
        }
        if !expected.is_valid() || expected.action_schema != SCHEMA_VERSION {
            self.replay_ready = false;
            self.reset(ResetReason::InvalidHistory);
            return;
        }
        let Some(index) = self
            .replay_groups
            .iter()
            .position(|group| group.stamp == expected.stamp)
        else {
            self.replay_ready = false;
            self.reset(ResetReason::InvalidHistory);
            return;
        };
        if index != 0 {
            self.replay_ready = false;
            self.reset(ResetReason::InvalidHistory);
        }
        for _ in 0..index {
            self.replay_groups.pop_front();
        }
        let Some(group) = self.replay_groups.pop_front() else {
            return;
        };
        let next_config = recording::config(expected.timing);
        if self.config != next_config {
            self.config = next_config;
            self.replay_ready = false;
            self.reset(ResetReason::BindingChanged);
        }
        if matches!(
            expected.outcome,
            PolicyOutcome::Cancelled | PolicyOutcome::UnavailablePrefix
        ) {
            self.replay_ready = false;
            self.reset(ResetReason::InvalidHistory);
            return;
        }
        if !self.replay_ready || expected.outcome == PolicyOutcome::NeutralBoundary {
            if !ordered_after(self.watermark, &group) {
                self.reset(ResetReason::InvalidHistory);
                return;
            }
            if !expected.neutral || !group_neutral(&group, config) {
                self.reset(ResetReason::InvalidHistory);
                return;
            }
            self.policy = self
                .config
                .and_then(|config| PolicyA::new(config, ClockEpoch(group.stamp.epoch)).ok());
            self.routes.clear();
            self.raw = Default::default();
            self.claims = 0;
            self.interpretation = None;
            self.epoch = group.stamp.epoch;
            // A validated boundary establishes state only. It cannot dispatch recorded commands.
            let serial = self.reset_serial;
            self.observe(
                &group,
                EvaluationInput {
                    context: expected.context,
                    gameplay: expected.gameplay,
                    config,
                    expected: None,
                },
                &mut Tick::default(),
            );
            self.replay_ready = self.reset_serial == serial
                && self.watermark == Some((group.stamp, group.order))
                && self.interpretation
                    == Some((
                        group.binding_generation,
                        expected.context,
                        expected.gameplay,
                    ));
            return;
        }
        self.observe(
            &group,
            EvaluationInput {
                context: expected.context,
                gameplay: expected.gameplay,
                config,
                expected: Some(expected),
            },
            tick,
        );
    }

    pub(super) fn observe(&mut self, group: &InputGroup, input: EvaluationInput<'_>, tick: &mut Tick) {
        let EvaluationInput {
            context,
            gameplay,
            config,
            expected,
        } = input;
        let mut evaluation = recording::cancelled(group, self.config, context, gameplay);
        if group.action_schema != SCHEMA_VERSION
            || !group.is_bounded()
            || !group.focused
            || group.context != context_set(context)
            || !complete_inventory(group)
        {
            self.reset(ResetReason::BindingUnresolved);
            tick.commands.clear();
            if !self.replay {
                history::record_policy(evaluation);
            }
            return;
        }
        if group.stamp.epoch != self.epoch {
            if group.stamp.epoch < self.epoch {
                self.reset(ResetReason::InvalidHistory);
                tick.commands.clear();
                if !self.replay {
                    history::record_policy(evaluation);
                }
                return;
            }
            self.epoch = group.stamp.epoch;
            self.policy = self
                .config
                .and_then(|config| PolicyA::new(config, ClockEpoch(self.epoch)).ok());
            self.reset(ResetReason::ClockDiscontinuity);
        }
        if !ordered_after(self.watermark, group) {
            self.reset(ResetReason::InvalidHistory);
            tick.commands.clear();
            if !self.replay {
                history::record_policy(evaluation);
            }
            return;
        }
        self.watermark = Some((group.stamp, group.order));
        let interpretation = (group.binding_generation, context, gameplay);
        if self.interpretation != Some(interpretation) {
            self.cancel_gameplay();
            for route in &mut self.routes {
                if route.system.is_some() {
                    route.neutral = true;
                    route.delivery = Delivery::default();
                }
            }
            self.interpretation = Some(interpretation);
        }
        let physical = raw_evidence(group);
        let Some(down) = physical else {
            self.reset(ResetReason::BindingUnresolved);
            if !self.replay {
                history::record_policy(evaluation);
            }
            return;
        };
        let Some(policy) = &mut self.policy else {
            self.reset(ResetReason::BindingUnresolved);
            tick.commands.clear();
            if !self.replay {
                history::record_policy(evaluation);
            }
            return;
        };
        let decision = policy.observe(Observation {
            stamp: ObservationStamp {
                epoch: ClockEpoch(group.stamp.epoch),
                at: Duration::from_nanos(group.stamp.time_ns),
                sync: group.stamp.sync,
                sequence: group.stamp.sequence,
            },
            generation: BindingGeneration(group.binding_generation),
            context: GameplayContext(context),
            singles_enabled: true,
            down,
            #[cfg(test)]
            evidence: InputEvidence::VerifiedUnmasked,
        });
        evaluation = recording::evaluation(group, self.config, context, gameplay, &decision);
        evaluation.neutral = group_neutral(group, config);
        if let ObservationStatus::Reset(reason) = decision.status {
            self.reset(reason);
            tick.commands.clear();
            evaluation = recording::cancelled(group, self.config, context, gameplay);
        } else {
            let mut commands = Vec::new();
            let (direct, claims) = self.update_routes(
                group,
                &decision,
                gameplay && group.context == context_set(context),
                config,
                &mut commands,
            );
            evaluation.direct = direct;
            evaluation.claims = claims;
            if let Some(request) = decision.request {
                commands.push(request.command);
            }
            for control in Control::ALL {
                let i = control.index();
                let (hand, physical) = raw_location(control);
                let claimed = claims & (SourceIdentity { hand, physical }).bit() != 0;
                let neutral = self.raw_neutral[i];
                if physical_is_neutral(&decision, down, i) {
                    self.raw_neutral[i] = false;
                }
                let button = if claimed
                    || neutral
                    || decision.withheld[i]
                        && decision.sources[i].disposition != Disposition::Single
                {
                    self.raw[i].withhold();
                    ButtonControl::default()
                } else {
                    let event = decision.singles[i];
                    let button = ButtonControl {
                        down: decision.sources[i].single_down,
                        pressed: matches!(
                            event,
                            Some(SingleEvent::Pressed { .. } | SingleEvent::Tap { .. })
                        ),
                        released: matches!(
                            event,
                            Some(SingleEvent::Released { .. } | SingleEvent::Tap { .. })
                        ),
                    };
                    self.raw[i].accept(button);
                    button
                };
                evaluation.raw[i] = flags(button);
            }
            commands.dedup();
            let mut seen = 0;
            commands.retain(|command| {
                let bit = command_bit(*command);
                let fresh = seen & bit == 0;
                seen |= bit;
                fresh
            });
            evaluation.commands = commands
                .iter()
                .fold(0, |bits, command| bits | command_bit(*command));
            if expected.is_none_or(|expected| expected == evaluation) {
                tick.commands.extend(commands);
            }
        }
        if expected.is_some_and(|expected| expected != evaluation) {
            tracing::warn!(target: "controllers", ?expected, ?evaluation, "input replay: final decision mismatch");
            self.replay_ready = false;
            self.reset(ResetReason::InvalidHistory);
            tick.commands.clear();
            return;
        }
        if !self.replay {
            history::record_policy(evaluation);
        }
    }
}

fn ordered_after(watermark: Option<(InputStamp, GroupOrder)>, group: &InputGroup) -> bool {
    watermark.is_none_or(|(previous, order)| {
        group.stamp.epoch > previous.epoch
            || group.stamp.epoch == previous.epoch
                && group.stamp.clock == previous.clock
                && group.stamp.time_ns > previous.time_ns
                && group.stamp.sequence > previous.sequence
                && group.stamp.sync >= previous.sync
                && (group.stamp.sync != previous.sync
                    || group.order == GroupOrder::ObservedGroup
                        && order == GroupOrder::ObservedGroup)
    })
}

fn physical_is_neutral(decision: &Arbitration, down: [bool; 3], index: usize) -> bool {
    !down[index] && !matches!(decision.status, ObservationStatus::Reset(_))
}
fn command_bit(command: SystemCommand) -> u8 {
    match command {
        SystemCommand::Recenter => 1,
        SystemCommand::TogglePanel => 2,
    }
}
fn flags(button: ButtonControl) -> u8 {
    u8::from(button.down) | u8::from(button.pressed) << 1 | u8::from(button.released) << 2
}
fn context_set(context: u64) -> u64 {
    context & 0xff
}
