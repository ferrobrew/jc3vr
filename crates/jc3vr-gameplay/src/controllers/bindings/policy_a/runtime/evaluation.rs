//! Policy evaluation and its recording.

use std::time::Duration;

use jc3vr_body::controllers::intent::{config::BindingsConfig, controls::ButtonControl};
use jc3vr_input::frame::ActionSetKind;
use jc3vr_view::vr::input::semantic::{SCHEMA_VERSION, history};
use tracking_format::input::{GroupOrder, InputGroup, InputStamp};

#[cfg(test)]
use crate::controllers::bindings::policy_a::InputEvidence;
use crate::controllers::bindings::policy_a::{
    Arbitration, BindingGeneration, CONTROLS, ClockEpoch, Control, Disposition, GameplayContext,
    Observation, ObservationStamp, ObservationStatus, ResetReason, SingleEvent, SystemCommand,
    attribution::{SourceIdentity, raw_location},
    recording,
    runtime::{
        Delivery, Runtime, Tick, arbitration, complete_inventory, group_neutral, raw_evidence,
    },
};

pub(super) struct EvaluationInput<'a> {
    pub(super) context: u64,
    pub(super) gameplay: bool,
    pub(super) config: &'a BindingsConfig,
}

impl Runtime {
    pub(super) fn observe(
        &mut self,
        group: &InputGroup,
        input: EvaluationInput<'_>,
        tick: &mut Tick,
    ) {
        let EvaluationInput {
            context,
            gameplay,
            config,
        } = input;
        let mut evaluation = recording::cancelled(group, self.config, context, gameplay);
        if group.action_schema != SCHEMA_VERSION
            || !group.is_bounded()
            || !group.focused
            || group.source_generation != group.binding_generation
            || group.context != context_set(context)
            || !complete_inventory(group)
        {
            self.reset(ResetReason::BindingUnresolved);
            tick.commands.clear();
            history::record_policy(evaluation);
            return;
        }
        if group.stamp.epoch != self.epoch {
            if group.stamp.epoch < self.epoch {
                self.reset(ResetReason::InvalidHistory);
                tick.commands.clear();
                history::record_policy(evaluation);
                return;
            }
            self.epoch = group.stamp.epoch;
            self.policy = arbitration(self.config, ClockEpoch(self.epoch));
            self.reset(ResetReason::ClockDiscontinuity);
        }
        if !ordered_after(self.watermark, group) {
            self.reset(ResetReason::InvalidHistory);
            tick.commands.clear();
            history::record_policy(evaluation);
            return;
        }
        self.watermark = Some((group.stamp, group.order));
        let interpretation = (group.binding_generation, context, gameplay);
        if self.interpretation != Some(interpretation) {
            self.cancel_gameplay();
            for route in &mut self.routes {
                if route.system.is_some() {
                    route.neutral = true;
                    route.delivery.cancel();
                }
            }
            self.interpretation = Some(interpretation);
        }
        let physical = raw_evidence(group);
        let available = physical.map(|value| value.is_some());
        let down = physical.map(|value| value.unwrap_or(false));
        let Some(policy) = &mut self.policy else {
            self.reset(ResetReason::BindingUnresolved);
            tick.commands.clear();
            history::record_policy(evaluation);
            return;
        };
        let decision = policy.observe_available(
            Observation {
                stamp: ObservationStamp {
                    epoch: ClockEpoch(group.stamp.epoch),
                    at: Duration::from_nanos(group.stamp.time_ns),
                    sync: group.stamp.sync,
                    sequence: group.stamp.sequence,
                },
                generation: BindingGeneration(group.binding_generation),
                context: GameplayContext(context),
                singles_enabled: true,
                vehicle: vehicle_set(context),
                down,
                #[cfg(test)]
                evidence: InputEvidence::VerifiedUnmasked,
            },
            available,
        );
        evaluation = recording::evaluation(group, self.config, context, gameplay, &decision);
        evaluation.neutral = group_neutral(group, config, self.dominant);
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
                if !available[i] {
                    self.raw_neutral[i] = true;
                    self.raw[i] = Delivery::default();
                }
                let neutral = self.raw_neutral[i];
                if available[i] && physical_is_neutral(&decision, down, i) {
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
            tick.commands.extend(commands);
        }
        history::record_policy(evaluation);
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

fn physical_is_neutral(decision: &Arbitration, down: [bool; CONTROLS], index: usize) -> bool {
    !down[index] && !matches!(decision.status, ObservationStatus::Reset(_))
}
fn command_bit(command: SystemCommand) -> u8 {
    match command {
        SystemCommand::Recenter => 1,
        SystemCommand::TogglePanel => 2,
        SystemCommand::Mark => 4,
    }
}
fn flags(button: ButtonControl) -> u8 {
    u8::from(button.down) | u8::from(button.pressed) << 1 | u8::from(button.released) << 2
}
fn context_set(context: u64) -> u64 {
    context & 0xff
}
/// Whether the context's action set is one of the vehicle sets.
fn vehicle_set(context: u64) -> bool {
    usize::try_from(context_set(context))
        .ok()
        .and_then(|index| ActionSetKind::ALL.get(index))
        .is_some_and(|kind| kind.is_vehicle())
}
