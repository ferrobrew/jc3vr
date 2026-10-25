//! Ordered live/replay input shared by gameplay, system commands, and controller pointers.

#[cfg(test)]
use std::time::Duration;
use std::{collections::VecDeque, time::Instant};

#[cfg(test)]
use tracking_format::input::policy::PolicyOutcome;
use tracking_format::input::{
    GroupOrder, HISTORY_CAPACITY, InputGroup, InputRecord, InputRoute, InputStamp, RouteSample,
    ValueBits,
    policy::{DirectAcceptance, DirectDecision},
};

use crate::{
    controllers::{
        bindings::{
            controls::ButtonControl,
            intents::{Intent, SemanticInput, shape},
            policy_a::{
                Arbitration, ClockEpoch, Control, Disposition, PolicyA, PolicyAConfig, ResetReason,
                ResetRequest, SingleEvent, SystemCommand,
                attribution::{Attribution, SourceIdentity, raw_location, raw_verified},
                recording,
                runtime::evaluation::EvaluationInput,
            },
        },
        config::BindingsConfig,
        frame::{Button, ControllerFrame, Edges, Hand, MAX_FRAME_AGE},
    },
    vr::input::semantic::{
        ActionKind, SCHEMA_VERSION, SemanticSet,
        bound::Physical,
        defaults::{Component, default_binding},
        frame::ActionValue,
        history::{self, Publication},
        vocabulary::{ActionId, GlobalAction},
    },
};

mod evaluation;

pub(crate) struct TickInput<'a> {
    pub(crate) publications: Vec<Publication>,
    pub(crate) replay: bool,
    pub(crate) frame: Option<&'a ControllerFrame>,
    pub(crate) context: u64,
    pub(crate) gameplay: bool,
    pub(crate) usable: bool,
    pub(crate) config: &'a BindingsConfig,
}

#[derive(Default)]
pub(crate) struct Runtime {
    policy: Option<PolicyA>,
    config: Option<PolicyAConfig>,
    replay: bool,
    epoch: u64,
    last_observed: Option<Instant>,
    last_frame: Option<u64>,
    interpretation: Option<(u64, u64, bool)>,
    consumer_context: Option<(u64, bool)>,
    routes: Vec<Route>,
    raw: [Delivery; 3],
    claims: u32,
    replay_groups: VecDeque<InputGroup>,
    replay_ready: bool,
    raw_neutral: [bool; 3],
    reset_serial: u64,
    watermark: Option<(InputStamp, GroupOrder)>,
}

#[derive(Default)]
pub(crate) struct Tick {
    pub(crate) commands: Vec<SystemCommand>,
    raw: [ButtonControl; 3],
    routes: Vec<(ActionId, Hand, Intent)>,
    claims: u32,
}

impl Runtime {
    /// Consume transitions on an ownership change, including a transition returned after an
    /// out-of-tick request. The next real neutral sample is consumed rather than dispatched.
    pub(crate) fn cancel(&mut self) {
        self.reset(ResetReason::MaskedObservation);
    }

    pub(crate) fn cancel_publications(&mut self, replay: bool, publications: Vec<Publication>) {
        if !replay {
            for publication in publications {
                if let InputRecord::Group(group) = publication.record {
                    history::record_policy(recording::cancelled(&group, self.config, 0, false));
                }
            }
        }
        self.cancel();
    }

    pub(crate) fn step(&mut self, input: TickInput<'_>) -> Tick {
        let config = if input.replay {
            if self.replay { self.config } else { None }
        } else {
            input.config.policy_a()
        };
        if self.config != config || self.replay != input.replay {
            let clock_changed = self.replay != input.replay;
            let watermark = self.watermark;
            self.config = config;
            self.replay = input.replay;
            self.invalidate();
            if clock_changed {
                self.epoch = 1;
            } else {
                self.watermark = watermark;
            }
            self.policy =
                config.and_then(|config| PolicyA::new(config, ClockEpoch(self.epoch.max(1))).ok());
            self.epoch = self.epoch.max(1);
        }
        if self.consumer_context != Some((input.context, input.gameplay)) {
            self.cancel_gameplay();
            for route in &mut self.routes {
                if route.system.is_some() {
                    route.neutral = true;
                    route.claimed = true;
                    route.delivery = Delivery::default();
                }
            }
            self.consumer_context = Some((input.context, input.gameplay));
        }
        let mut tick = Tick::default();
        for publication in input.publications {
            let reset_before = self.reset_serial;
            match publication.record {
                InputRecord::Discontinuity { .. } => {
                    self.replay_groups.clear();
                    self.replay_ready = false;
                    self.reset(ResetReason::InvalidHistory);
                    tick.commands.clear();
                }
                InputRecord::Policy(expected) if input.replay => {
                    self.replay_policy(expected, input.config, &mut tick);
                }
                InputRecord::Policy(_) => {}
                InputRecord::Group(group) if input.replay => {
                    if self.replay_groups.len() >= HISTORY_CAPACITY {
                        self.replay_groups.clear();
                        self.replay_ready = false;
                        self.reset(ResetReason::InvalidHistory);
                    }
                    self.replay_groups.push_back(group);
                }
                InputRecord::Group(group) => {
                    if !input.usable
                        || input.frame.is_none()
                        || publication.observed_at.elapsed() > MAX_FRAME_AGE
                    {
                        self.reset(ResetReason::MissingOrStaleInput);
                        tick.commands.clear();
                        history::record_policy(recording::cancelled(
                            &group,
                            self.config,
                            input.context,
                            input.gameplay,
                        ));
                    } else {
                        if self.last_observed.is_some_and(|previous| {
                            publication.observed_at.saturating_duration_since(previous)
                                > MAX_FRAME_AGE
                        }) {
                            self.reset(ResetReason::MissingOrStaleInput);
                        }
                        self.last_observed = Some(publication.observed_at);
                        self.last_frame = publication.frame_sequence;
                        self.observe(
                            &group,
                            EvaluationInput {
                                context: input.context,
                                gameplay: input.gameplay,
                                config: input.config,
                                expected: None,
                            },
                            &mut tick,
                        );
                    }
                }
            }
            if self.reset_serial != reset_before {
                tick.commands.clear();
                self.raw = Default::default();
                for route in &mut self.routes {
                    route.delivery = Delivery::default();
                }
            }
            tick.claims |= self.claims;
        }
        // A group without its final decision cannot extend an accepted hold. Keep bounded stamp
        // candidates for a later record, but require validated neutral before resuming dispatch.
        if input.replay && !self.replay_groups.is_empty() {
            self.replay_ready = false;
            self.reset(ResetReason::InvalidHistory);
            tick.commands.clear();
        }
        if !input.usable
            || input.frame.is_none()
            || !input.replay
                && input
                    .frame
                    .is_some_and(|frame| Some(frame.sequence) != self.last_frame)
            || self.raw.iter().any(|delivery| delivery.overflowed)
            || self.routes.iter().any(|route| route.delivery.overflowed)
        {
            self.reset(ResetReason::MissingOrStaleInput);
            tick.commands.clear();
        }
        tick.raw = self.raw.each_mut().map(Delivery::take);
        tick.routes = self
            .routes
            .iter_mut()
            .filter(|route| route.present && route.system.is_none())
            .map(|route| {
                let mut intent = route.intent;
                intent.button = route.delivery.take();
                if route.blocked && !intent.button.held() && !intent.button.released {
                    intent = Intent::default();
                }
                (route.id, route.hand, intent)
            })
            .collect();
        tick.claims |= self.claims;
        tick
    }

    fn invalidate(&mut self) {
        self.routes.clear();
        self.raw = Default::default();
        self.claims = 0;
        self.interpretation = None;
        self.last_frame = None;
        self.last_observed = None;
        self.consumer_context = None;
        self.replay_groups.clear();
        self.replay_ready = false;
        self.watermark = None;
    }

    fn cancel_gameplay(&mut self) {
        for route in &mut self.routes {
            if route.system.is_none() {
                route.neutral = true;
                route.blocked = true;
                route.delivery = Delivery::default();
            }
        }
        self.raw_neutral = [true; 3];
        self.raw = Default::default();
    }

    fn reset(&mut self, reason: ResetReason) {
        self.reset_serial = self.reset_serial.wrapping_add(1);
        self.raw_neutral = [true; 3];
        self.replay_ready = false;
        if let Some(policy) = &mut self.policy {
            policy.reset(ResetRequest { reason });
        }
        for route in &mut self.routes {
            route.neutral = true;
            route.blocked = true;
            route.delivery = Delivery::default();
            route.intent = Intent::default();
            route.system_fired = false;
            if route.system.is_some() {
                route.claimed = true;
            }
        }
        self.raw = Default::default();
        self.claims = self
            .routes
            .iter()
            .filter(|route| route.system.is_some())
            .fold(0, |claims, route| {
                claims | route.attribution.claim_mask(route.hand)
            });
        self.interpretation = None;
    }

    fn update_routes(
        &mut self,
        group: &InputGroup,
        decision: &Arbitration,
        gameplay: bool,
        config: &BindingsConfig,
        commands: &mut Vec<SystemCommand>,
    ) -> ([DirectDecision; 4], u32) {
        for route in &mut self.routes {
            route.present = false;
            route.blocked = true;
        }
        let mut samples = Vec::new();
        for sample in &group.samples {
            let InputRoute::Semantic { set, action, hand } = sample.route else {
                continue;
            };
            let Some(&set) = SemanticSet::ALL.get(set as usize) else {
                continue;
            };
            let Some(&hand) = Hand::BOTH.get(hand as usize) else {
                continue;
            };
            if action as usize >= set.actions().len() {
                continue;
            }
            let id = ActionId {
                set,
                index: action as u8,
            };
            let index = self
                .routes
                .iter()
                .position(|route| route.id == id && route.hand == hand)
                .unwrap_or_else(|| {
                    self.routes.push(Route::new(id, hand));
                    self.routes.len() - 1
                });
            let route = &mut self.routes[index];
            route.present = true;
            let attribution = Attribution::of(&sample.sources, hand);
            if route.attribution != attribution {
                route.neutral = true;
                route.claimed = true;
                route.delivery = Delivery::default();
            }
            route.attribution = attribution;
            route.sample_down = sample.active && sample.down;
            samples.push((index, sample));
        }
        let mut direct = [DirectDecision::default(); 4];
        let mut candidates = Vec::new();
        let raw_claims = Control::ALL
            .into_iter()
            .filter(|control| {
                matches!(
                    decision.sources[control.index()].disposition,
                    Disposition::Pending | Disposition::Consumed | Disposition::AwaitingNeutral
                )
            })
            .fold(0, |mask, control| {
                let (hand, physical) = raw_location(control);
                mask | SourceIdentity { hand, physical }.bit()
            });
        let mut claims = 0;
        for &(index, sample) in &samples {
            let route = &mut self.routes[index];
            let Some(command) = route.system else {
                continue;
            };
            let slot = direct_slot(command, route.hand);
            let reported = route.attribution.mask();
            let mask = route.attribution.claim_mask(route.hand);
            let available = sample.active
                && !route.attribution.unresolved
                && reported != 0
                && matches!(sample.value, ValueBits::Bool(value) if value == sample.down);
            if route.sample_down || route.previous || route.neutral {
                route.claimed = true;
            }
            if route.claimed {
                claims |= mask;
            }
            if available && !route.sample_down && neutral_value(sample.value, config) {
                route.claimed = false;
            }
            let permitted = route.update(sample, decision, available, config);
            let acceptance = if !available {
                DirectAcceptance::Unavailable
            } else if permitted.pressed && !route.system_fired {
                DirectAcceptance::Accepted
            } else if !route.sample_down {
                DirectAcceptance::Neutral
            } else if route.system_fired {
                DirectAcceptance::Held
            } else {
                DirectAcceptance::Withheld
            };
            direct[slot] = DirectDecision {
                sources: mask,
                acceptance,
            };
            if acceptance == DirectAcceptance::Accepted {
                candidates.push((index, slot, command, mask));
            }
            if !route.sample_down && available {
                route.system_fired = false;
            }
        }
        // Conflicting direct routes are both consumed. Raw ownership also excludes a direct route,
        // even when the raw chord has not yet emitted its command. Disjoint commands remain valid.
        for &(index, slot, command, mask) in &candidates {
            let conflict = mask & raw_claims != 0
                || samples.iter().any(|&(other, _)| {
                    other != index
                        && self.routes[other].system.is_some()
                        && self.routes[other].sample_down
                        && self.routes[other]
                            .attribution
                            .claim_mask(self.routes[other].hand)
                            & mask
                            != 0
                });
            let route = &mut self.routes[index];
            route.system_fired = route.sample_down;
            if conflict {
                direct[slot].acceptance = DirectAcceptance::Conflict;
                route.neutral = true;
                route.delivery = Delivery::default();
            } else {
                commands.push(command);
            }
        }
        for (index, sample) in samples {
            let route = &mut self.routes[index];
            if route.system.is_some() {
                continue;
            }
            let claimed = route.attribution.intersects(claims);
            if claimed {
                route.neutral = true;
            }
            route.update(sample, decision, gameplay && !claimed, config);
        }
        for route in &mut self.routes {
            if !route.present {
                route.neutral = true;
                route.delivery = Delivery::default();
                route.intent = Intent::default();
            }
        }
        self.claims = claims;
        (direct, self.claims)
    }
}

impl Tick {
    pub(crate) fn filter_raw(&self, frame: &mut ControllerFrame, edges: &mut Edges) {
        for control in Control::ALL {
            let (hand, _) = raw_location(control);
            let button = if control == Control::RightB {
                Button::BClick
            } else {
                Button::ThumbstickClick
            };
            let state = self.raw[control.index()];
            frame.hands[hand.index()].buttons.set(button, state.down);
            edges.pressed[hand.index()].set(button, state.pressed);
            edges.released[hand.index()].set(button, state.released);
        }
        for hand in Hand::BOTH {
            for physical in [
                Physical::Trigger,
                Physical::Grip,
                Physical::Thumbstick,
                Physical::ThumbstickClick,
                Physical::Trackpad,
                Physical::LowerFace,
                Physical::UpperFace,
                Physical::System,
            ] {
                if self.claims & (SourceIdentity { hand, physical }).bit() == 0 {
                    continue;
                }
                let buttons: &[Button] = match physical {
                    Physical::Trigger => &[Button::TriggerPress],
                    Physical::Grip => &[Button::SqueezePress],
                    Physical::ThumbstickClick => &[Button::ThumbstickClick],
                    Physical::LowerFace => &[Button::AClick],
                    Physical::UpperFace => &[Button::BClick],
                    Physical::System => &[Button::SystemClick],
                    Physical::Trackpad => &[Button::TrackpadPressLeft, Button::TrackpadPressRight],
                    Physical::Thumbstick => &[],
                };
                let input = &mut frame.hands[hand.index()].inputs;
                match physical {
                    Physical::Trigger => input.trigger = 0.0,
                    Physical::Grip => {
                        input.squeeze = 0.0;
                        input.squeeze_force = 0.0;
                    }
                    Physical::Thumbstick => input.thumbstick = Default::default(),
                    Physical::Trackpad => {
                        input.trackpad = Default::default();
                        input.trackpad_force = 0.0;
                    }
                    _ => {}
                }
                for &button in buttons {
                    frame.hands[hand.index()].buttons.set(button, false);
                    edges.pressed[hand.index()].set(button, false);
                    edges.released[hand.index()].set(button, false);
                }
            }
        }
    }

    pub(crate) fn filter_semantic(&self, input: &mut SemanticInput) {
        for set in [&mut input.global, &mut input.mode] {
            for action in &mut set.actions {
                action.hands = Default::default();
            }
        }
        for &(id, hand, intent) in &self.routes {
            if let Some(action) = input.get_mut(id) {
                action.hands[hand.index()] = intent;
            }
        }
    }
}

fn raw_evidence(group: &InputGroup) -> Option<[bool; 3]> {
    if group.source_generation != group.binding_generation {
        return None;
    }
    let mut down = [false; 3];
    for control in Control::ALL {
        let sample = group
            .samples
            .iter()
            .find(|sample| sample.route == InputRoute::Raw(control.index() as u8))?;
        let ValueBits::Bool(value) = sample.value else {
            return None;
        };
        if !sample.active || value != sample.down || !raw_verified(&sample.sources, control) {
            return None;
        }
        down[control.index()] = value;
    }
    Some(down)
}

fn complete_inventory(group: &InputGroup) -> bool {
    let mode = match group.context {
        0 => SemanticSet::OnFoot,
        1 => SemanticSet::Wingsuit,
        2 => SemanticSet::LandVehicle,
        3 => SemanticSet::Helicopter,
        4 => SemanticSet::Plane,
        5 => SemanticSet::Menu,
        _ => return false,
    };
    if group.samples.len() != 3 + 2 * (SemanticSet::Global.actions().len() + mode.actions().len()) {
        return false;
    }
    for set in [SemanticSet::Global, mode] {
        for (action, definition) in set.actions().iter().enumerate() {
            for hand in Hand::BOTH {
                let Some(sample) = group.samples.iter().find(|sample| {
                    sample.route
                        == InputRoute::Semantic {
                            set: set.index() as u8,
                            action: action as u16,
                            hand: hand.index() as u8,
                        }
                }) else {
                    return false;
                };
                if !matches!(
                    (definition.kind, sample.value),
                    (ActionKind::Bool, ValueBits::Bool(_))
                        | (ActionKind::Float, ValueBits::Float(_))
                        | (ActionKind::Vec2, ValueBits::Vec2(_))
                ) {
                    return false;
                }
            }
        }
    }
    Control::ALL.into_iter().all(|control| {
        group
            .samples
            .iter()
            .any(|sample| sample.route == InputRoute::Raw(control.index() as u8))
    })
}

pub(crate) fn group_neutral(group: &InputGroup, config: &BindingsConfig) -> bool {
    group.action_schema == SCHEMA_VERSION
        && group.focused
        && group.is_bounded()
        && complete_inventory(group)
        && raw_evidence(group) == Some([false; 3])
        && group.samples.iter().all(|sample| {
            let InputRoute::Semantic { hand, .. } = sample.route else {
                return true;
            };
            let Some(&hand) = Hand::BOTH.get(hand as usize) else {
                return false;
            };
            let attribution = Attribution::of(&sample.sources, hand);
            // Successful-empty is an unbound route. Inactive bound actions cannot prove neutral.
            !attribution.unresolved
                && !sample.down
                && neutral_value(sample.value, config)
                && if attribution.mask() == 0 {
                    !sample.active
                } else {
                    sample.active
                }
        })
}

fn neutral_value(value: ValueBits, config: &BindingsConfig) -> bool {
    match value {
        ValueBits::Bool(value) => !value,
        ValueBits::Float(value) => {
            f32::from_bits(value).is_finite()
                && f32::from_bits(value).abs() <= config.trigger_deadzone.min(config.move_deadzone)
        }
        ValueBits::Vec2(value) => value.into_iter().all(|value| {
            f32::from_bits(value).is_finite() && f32::from_bits(value).abs() <= config.move_deadzone
        }),
    }
}
fn direct_slot(command: SystemCommand, hand: Hand) -> usize {
    (if command == SystemCommand::Recenter {
        0
    } else {
        2
    }) + hand.index()
}
struct Route {
    id: ActionId,
    hand: Hand,
    attribution: Attribution,
    neutral: bool,
    blocked: bool,
    sample_down: bool,
    present: bool,
    previous: bool,
    intent: Intent,
    delivery: Delivery,
    system: Option<SystemCommand>,
    system_fired: bool,
    claimed: bool,
}
impl Route {
    fn new(id: ActionId, hand: Hand) -> Self {
        Self {
            id,
            hand,
            attribution: Attribution::of(&tracking_format::input::SourceQuery::NotEnumerated, hand),
            neutral: true,
            blocked: true,
            sample_down: false,
            present: false,
            previous: false,
            intent: Intent::default(),
            delivery: Delivery::default(),
            system_fired: false,
            claimed: false,
            system: if id == GlobalAction::RecenterView.id() {
                Some(SystemCommand::Recenter)
            } else if id == GlobalAction::ToggleDebugPanel.id() {
                Some(SystemCommand::TogglePanel)
            } else {
                None
            },
        }
    }
    fn update(
        &mut self,
        sample: &RouteSample,
        decision: &Arbitration,
        enabled: bool,
        config: &BindingsConfig,
    ) -> ButtonControl {
        let down = self.sample_down;
        let previous = std::mem::replace(&mut self.previous, down);
        let component =
            default_binding(self.id.set, self.id.index()).map(|binding| binding.component);
        let analog = matches!(sample.value, ValueBits::Vec2(_))
            || matches!(sample.value, ValueBits::Float(_))
                && matches!(
                    component,
                    Some(
                        Component::ThumbstickX
                            | Component::ThumbstickY
                            | Component::Thumbstick
                            | Component::Trackpad
                    )
                );
        let unresolved = self.attribution.unresolved;
        let mut withheld = unresolved
            || self.attribution.identities.is_empty() && !analog
            || !sample.active
            || !enabled;
        let mut consumed = false;
        for control in Control::ALL {
            if self.attribution.controls[control.index()] {
                withheld |= decision.withheld[control.index()]
                    && decision.sources[control.index()].disposition != Disposition::Single;
                consumed |= matches!(
                    decision.sources[control.index()].disposition,
                    Disposition::Consumed | Disposition::AwaitingNeutral
                );
            }
        }
        let merged = self.attribution.identities.len() > 1;
        let cancel = consumed || unresolved || !sample.active || !enabled || merged && withheld;
        if cancel {
            self.neutral = true;
            self.delivery = Delivery::default();
        }
        let was_neutral = self.neutral;
        if !down && neutral_value(sample.value, config) && !unresolved && sample.active && !withheld
        {
            self.neutral = false;
        }
        self.blocked = withheld || was_neutral;
        let mut button = ButtonControl {
            down,
            pressed: down && !previous,
            released: !down && previous,
        };
        if let Some(control) = self.attribution.single_control() {
            let event = decision.singles[control.index()];
            button.down &= decision.sources[control.index()].single_down;
            button.pressed = button.down
                && (!self.delivery.latest.down
                    || matches!(event, Some(SingleEvent::Pressed { .. })))
                || matches!(event, Some(SingleEvent::Tap { .. })) && (previous || down);
            button.released |= matches!(
                event,
                Some(SingleEvent::Released { .. } | SingleEvent::Tap { .. })
            );
        }
        if self.blocked {
            button = ButtonControl::default();
            self.delivery.withhold();
        }
        let value = match sample.value {
            ValueBits::Bool(value) => ActionValue::Bool(value),
            ValueBits::Float(value) => ActionValue::Float(f32::from_bits(value)),
            ValueBits::Vec2(value) => ActionValue::Vec2(glam::Vec2::new(
                f32::from_bits(value[0]),
                f32::from_bits(value[1]),
            )),
        };
        self.intent = if self.blocked {
            Intent::default()
        } else {
            shape(value, button, component, config)
        };
        self.delivery.accept(button);
        button
    }
}

#[derive(Default)]
struct Delivery {
    pending: VecDeque<ButtonControl>,
    latest: ButtonControl,
    overflowed: bool,
}
impl Delivery {
    fn withhold(&mut self) {
        self.latest = ButtonControl::default();
    }
    fn accept(&mut self, button: ButtonControl) {
        if button.pressed || button.released {
            if self.pending.len() >= HISTORY_CAPACITY - 1 {
                self.pending.clear();
                self.latest = ButtonControl::default();
                self.overflowed = true;
                return;
            }
            if button.pressed && button.released {
                self.pending.push_back(ButtonControl {
                    down: true,
                    pressed: true,
                    released: false,
                });
                self.pending.push_back(ButtonControl {
                    down: false,
                    pressed: false,
                    released: true,
                });
            } else {
                self.pending.push_back(button);
            }
        }
        self.latest = ButtonControl {
            down: button.down,
            ..Default::default()
        };
    }
    fn take(&mut self) -> ButtonControl {
        self.pending.pop_front().unwrap_or(self.latest)
    }
}

#[cfg(test)]
mod tests;
