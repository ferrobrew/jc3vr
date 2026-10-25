//! Per-physical-hand gun trigger ownership and neutral transfer between gameplay consumers.

use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::{
        bindings::{
            controls::{ButtonControl, Controls},
            hold::{Desired, MapKind},
            intents::{Intent, SemanticInput},
            tables::{
                declare::{TableCtx, declare},
                grapple_press,
                sink::{Axis, Input, Press, Sink},
            },
        },
        frame::Hand,
        mode::{AirborneVariant, Mode},
        weapons::capabilities::{GunHand, GunOwner},
    },
    vr::input::semantic::{
        SemanticSet,
        bound::Physical,
        vocabulary::{ActionId, HelicopterAction, LandVehicleAction, OnFootAction, PlaneAction},
    },
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Owner {
    pub mode: Mode,
    pub gun: Option<GunOwner>,
}

#[derive(Debug, Copy, Clone, Default)]
pub(super) struct RawTrigger {
    pub button: ButtonControl,
    pub value: f32,
    pub usable: bool,
}

pub(super) struct TriggerOwnership {
    owners: [Option<Owner>; 2],
    awaiting_neutral: [bool; 2],
}

impl Default for TriggerOwnership {
    fn default() -> Self {
        Self::new()
    }
}

impl TriggerOwnership {
    pub const fn new() -> Self {
        Self {
            owners: [None; 2],
            awaiting_neutral: [true; 2],
        }
    }

    pub fn suspend(&mut self) {
        self.owners = [None; 2];
        self.awaiting_neutral = [true; 2];
    }

    /// A neutral sample rearms the following tick, never its own buffered edges.
    pub fn observe(
        &mut self,
        owners: [Owner; 2],
        raw: [RawTrigger; 2],
        deadzone: f32,
    ) -> [bool; 2] {
        Hand::BOTH.map(|hand| {
            let i = hand.index();
            if self.owners[i] != Some(owners[i]) || !raw[i].usable {
                self.awaiting_neutral[i] = true;
                self.owners[i] = Some(owners[i]);
            }
            let blocked = self.awaiting_neutral[i] || !raw[i].usable;
            let button = raw[i].button;
            if raw[i].usable
                && !button.down
                && !button.pressed
                && !button.released
                && raw[i].value.is_finite()
                && raw[i].value <= deadzone
            {
                self.awaiting_neutral[i] = false;
            }
            blocked
        })
    }

    /// Mutations during weapon finish cannot transfer a press accepted for the old identity.
    pub fn after_finish(&mut self, owners: [Owner; 2]) -> [bool; 2] {
        Hand::BOTH.map(|hand| {
            let i = hand.index();
            let changed = self.owners[i] != Some(owners[i]);
            if changed {
                self.owners[i] = Some(owners[i]);
                self.awaiting_neutral[i] = true;
            }
            changed
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(super) enum SemanticKeep {
    #[default]
    None,
    NativePairFire,
}

#[derive(Debug, Copy, Clone, Default)]
pub(super) struct TriggerMask {
    pub reserved: bool,
    pub keep: SemanticKeep,
}

pub(super) fn masks(owners: [Owner; 2], blocked: [bool; 2]) -> [TriggerMask; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        TriggerMask {
            reserved: blocked[i] || owners[i].gun.is_some(),
            keep: if !blocked[i]
                && owners[i]
                    .gun
                    .is_some_and(|gun| matches!(gun.role, GunHand::NativePair(_)))
            {
                SemanticKeep::NativePairFire
            } else {
                SemanticKeep::None
            },
        }
    })
}

impl SemanticKeep {
    fn retains(self, mode: Mode, id: ActionId) -> bool {
        if self != Self::NativePairFire {
            return false;
        }
        let hand_fire = match mode {
            Mode::OnFoot(_)
            | Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Parachute) => true,
            Mode::Vehicle(seat) => !seat.driver,
            Mode::Airborne(AirborneVariant::Wingsuit) | Mode::Ui => false,
        };
        hand_fire
            && match id.set {
                SemanticSet::OnFoot => [
                    OnFootAction::Fire.id(),
                    OnFootAction::FireOffHandPistol.id(),
                ]
                .contains(&id),
                SemanticSet::LandVehicle => [
                    LandVehicleAction::Fire.id(),
                    LandVehicleAction::FireOffHandPistol.id(),
                ]
                .contains(&id),
                SemanticSet::Helicopter => [
                    HelicopterAction::Fire.id(),
                    HelicopterAction::FireOffHandPistol.id(),
                ]
                .contains(&id),
                SemanticSet::Plane => {
                    [PlaneAction::Fire.id(), PlaneAction::FireOffHandPistol.id()].contains(&id)
                }
                SemanticSet::Global | SemanticSet::Wingsuit | SemanticSet::Menu => false,
            }
    }
}

/// Keep native-pair fire sampled from semantics; never synthesize it from a raw trigger.
pub(super) fn mask_routes(input: &mut SemanticInput, mode: Mode, masks: [TriggerMask; 2]) {
    for set in [&mut input.global, &mut input.mode] {
        for (index, action) in set.actions.iter_mut().enumerate() {
            let id = ActionId {
                set: set.set,
                index: index as u8,
            };
            for hand in Hand::BOTH {
                let i = hand.index();
                let bound = action.bound[i];
                if masks[i].reserved
                    && !masks[i].keep.retains(mode, id)
                    && (bound.contains(Physical::Trigger) || bound == bound.with_unknown())
                {
                    action.hands[i] = Intent::default();
                }
            }
        }
    }
}

pub(super) fn retain_fire(fire: [bool; 2], changed: [bool; 2]) -> [bool; 2] {
    Hand::BOTH.map(|hand| fire[hand.index()] && !changed[hand.index()])
}

/// Remove only engine targets declared from a newly changed trigger source before injection.
pub(super) fn suppress_changed_routes(
    desired: &mut Vec<Desired>,
    mode: Mode,
    controls: &Controls,
    input: &SemanticInput,
    changed: [bool; 2],
) {
    if !changed.contains(&true) {
        return;
    }
    let mut routes = ChangedRoutes {
        input,
        changed,
        actions: Vec::new(),
    };
    declare(&mut routes, mode, &TableCtx::of(controls));
    if let Some((inputs, actions)) = grapple_press(mode)
        && inputs.iter().any(|&id| routes.affected(id))
    {
        routes.actions.extend_from_slice(actions.fire);
        routes.actions.extend_from_slice(actions.release);
        routes.actions.push(Action::RETRACT_GRAPPLE);
    }
    desired.retain(|desired| {
        desired.target.map != MapKind::Player || !routes.actions.contains(&desired.target.action)
    });
}

struct ChangedRoutes<'a> {
    input: &'a SemanticInput,
    changed: [bool; 2],
    actions: Vec<Action>,
}

impl ChangedRoutes<'_> {
    fn affected(&self, id: ActionId) -> bool {
        self.input.get(id).is_some_and(|action| {
            Hand::BOTH.into_iter().any(|hand| {
                let bound = action.bound[hand.index()];
                self.changed[hand.index()]
                    && (bound.contains(Physical::Trigger) || bound == bound.with_unknown())
            })
        })
    }
}

impl Sink for ChangedRoutes<'_> {
    fn hold(&mut self, presses: &[Press], actions: &[Action]) {
        if presses.iter().any(|press| match press.input {
            Input::Action(id) | Input::Zone(id, _) => self.affected(id),
            Input::MenuStick(_) => false,
        }) {
            self.actions.extend_from_slice(actions);
        }
    }
    fn click(&mut self, press: Press, action: Action) {
        self.hold(&[press], &[action]);
    }
    fn analog(&mut self, axis: Axis, action: Action) {
        if self.affected(axis.action()) {
            self.actions.push(action);
        }
    }
    fn axis(&mut self, axis: Axis, negative: Action, positive: Action) {
        if self.affected(axis.action()) {
            self.actions.extend([negative, positive]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        controllers::{
            bindings::{
                grapple_button::GrappleContext,
                hold::{Target, Want},
                intents::simulate::simulate,
                tables::Bindings,
            },
            config::BindingsConfig,
            mode::{OnFootVariant, VehicleClass, VehicleSeat},
            weapons::capabilities::{Capabilities, NativePair, trigger_claims},
        },
        vr::input::semantic::{
            bound::PhysicalSet, defaults::default_binding, vocabulary::WingsuitAction,
        },
    };

    fn owner(weapon: Option<usize>, role: GunHand) -> Owner {
        Owner {
            mode: Mode::OnFoot(OnFootVariant::Ground),
            gun: weapon.map(|weapon| GunOwner { weapon, role }),
        }
    }
    fn raw(down: bool) -> RawTrigger {
        RawTrigger {
            button: ButtonControl {
                down,
                ..Default::default()
            },
            value: if down { 1.0 } else { 0.0 },
            usable: true,
        }
    }

    #[test]
    fn held_trigger_never_transfers_between_gun_and_state_action() {
        for hand in Hand::BOTH {
            let mut gate = TriggerOwnership::new();
            let mut owners = [owner(None, GunHand::Empty); 2];
            assert_eq!(gate.observe(owners, [raw(false); 2], 0.05), [true; 2]);
            assert_eq!(gate.observe(owners, [raw(true); 2], 0.05), [false; 2]);
            owners[hand.index()] = owner(Some(1), GunHand::Grip);
            assert!(gate.observe(owners, [raw(true); 2], 0.05)[hand.index()]);
            assert!(gate.observe(owners, [raw(false); 2], 0.05)[hand.index()]);
            assert!(!gate.observe(owners, [raw(true); 2], 0.05)[hand.index()]);
            owners[hand.index()] = owner(None, GunHand::Empty);
            assert!(gate.after_finish(owners)[hand.index()]);
            assert!(gate.observe(owners, [raw(true); 2], 0.05)[hand.index()]);
        }
    }

    #[test]
    fn gun_identity_role_and_mode_changes_require_neutral() {
        let mut gate = TriggerOwnership::new();
        let owners = [owner(Some(1), GunHand::Grip), owner(Some(2), GunHand::Grip)];
        gate.observe(owners, [raw(false); 2], 0.05);
        assert_eq!(gate.observe(owners, [raw(true); 2], 0.05), [false; 2]);
        assert_eq!(gate.after_finish([owners[1], owners[0]]), [true; 2]);
        let mut changed = [owners[1], owners[0]];
        changed[0].gun.as_mut().unwrap().role = GunHand::Support;
        changed[1].mode = Mode::Airborne(AirborneVariant::Wingsuit);
        assert_eq!(gate.after_finish(changed), [true; 2]);
    }

    #[test]
    fn masks_and_post_finish_suppression_follow_physical_trigger_sources() {
        let controls = Controls {
            dominant: Hand::Right,
            ..Default::default()
        };
        let modes = [
            (VehicleClass::Car, Action::ACCELERATE, Action::REVERSE),
            (
                VehicleClass::Motorcycle,
                Action::ACCELERATE,
                Action::REVERSE,
            ),
            (VehicleClass::Sea, Action::ACCELERATE, Action::REVERSE),
            (
                VehicleClass::Helicopter,
                Action::HELI_INC_ALTITUDE,
                Action::HELI_DEC_ALTITUDE,
            ),
            (
                VehicleClass::Plane,
                Action::PLANE_INC_TRUST,
                Action::PLANE_DEC_TRUST,
            ),
            (
                VehicleClass::Mech,
                Action::MECH_FIRE_RIGHT_HAND_WEAPON,
                Action::MECH_FIRE_GRAVITY_WEAPON_PRIMARY,
            ),
            (VehicleClass::Unknown, Action::ACCELERATE, Action::REVERSE),
        ];
        for (class, primary, secondary) in modes {
            for driver in [false, true] {
                let mode = Mode::Vehicle(VehicleSeat { class, driver });
                let input = simulate(
                    &controls,
                    SemanticSet::of(mode.action_set()),
                    default_binding,
                );
                for hand in Hand::BOTH {
                    let changed = Hand::BOTH.map(|h| h == hand);
                    let mut desired = [primary, secondary, Action::TURN_LEFT, Action::EXIT_VEHICLE]
                        .map(|action| Desired {
                            target: Target::player(action),
                            want: Want::Hold(1.0),
                        })
                        .to_vec();
                    suppress_changed_routes(&mut desired, mode, &controls, &input, changed);
                    assert!(desired.iter().any(|d| d.target.action == Action::TURN_LEFT));
                    assert!(
                        desired
                            .iter()
                            .any(|d| d.target.action == Action::EXIT_VEHICLE)
                    );
                    if driver {
                        let removed = if hand == Hand::Right {
                            primary
                        } else {
                            secondary
                        };
                        assert!(
                            !desired.iter().any(|d| d.target.action == removed),
                            "{mode:?} {hand:?}"
                        );
                    }
                }
            }
        }
        let mut input = simulate(&controls, SemanticSet::Wingsuit, default_binding);
        let action = input.get_mut(WingsuitAction::FireSecondary.id()).unwrap();
        action.bound[Hand::Left.index()] = PhysicalSet::of(Physical::Trigger).with(Physical::Grip);
        action.hands[Hand::Left.index()].button.down = true;
        mask_routes(
            &mut input,
            Mode::Airborne(AirborneVariant::Wingsuit),
            [
                TriggerMask {
                    reserved: true,
                    keep: SemanticKeep::None,
                },
                TriggerMask::default(),
            ],
        );
        assert_eq!(
            input.get(WingsuitAction::FireSecondary.id()).unwrap().hands[0],
            Intent::default()
        );
        let mut desired = [
            Action::FIRE_WINGSUIT_WEAPON_SECONDARY,
            Action::WINGSUIT_BOOST,
        ]
        .map(|action| Desired {
            target: Target::player(action),
            want: Want::Hold(1.0),
        })
        .to_vec();
        suppress_changed_routes(
            &mut desired,
            Mode::Airborne(AirborneVariant::Wingsuit),
            &controls,
            &input,
            [true, false],
        );
        assert_eq!(desired.len(), 1);
        assert_eq!(desired[0].target.action, Action::WINGSUIT_BOOST);
    }

    #[test]
    fn native_pair_keeps_sampled_offhand_fire_not_retract_for_both_dominances() {
        let mode = Mode::OnFoot(OnFootVariant::Ground);
        for dominant in Hand::BOTH {
            let pair = GunHand::NativePair(NativePair {
                first_hand: dominant,
            });
            let owners = [owner(Some(1), pair); 2];
            let mut gate = TriggerOwnership::new();
            gate.observe(owners, [raw(false); 2], 0.05);
            let mut controls = Controls {
                dominant,
                dual_wield: true,
                ..Default::default()
            };
            controls.hands[dominant.other().index()].trigger = 1.0;
            controls.hands[dominant.other().index()].trigger_press = ButtonControl {
                down: true,
                ..Default::default()
            };
            let mut input = simulate(&controls, SemanticSet::OnFoot, default_binding);
            let blocked = gate.observe(
                owners,
                Hand::BOTH.map(|hand| raw(hand == dominant.other())),
                0.05,
            );
            mask_routes(&mut input, mode, masks(owners, blocked));
            let sampled = input
                .get(OnFootAction::FireOffHandPistol.id())
                .unwrap()
                .hands[dominant.other().index()];
            assert!(sampled.button.down);
            let step =
                Bindings::new().step(mode, &controls, &input, 0.01, &BindingsConfig::default());
            assert!(
                step.desired
                    .iter()
                    .any(|desired| desired.target.action == Action::FIRE_RIGHT)
            );
            assert!(
                !step
                    .desired
                    .iter()
                    .any(|desired| desired.target.action == Action::RETRACT_GRAPPLE)
            );
            assert_eq!(
                trigger_claims(Capabilities::of(mode), [pair; 2], [true; 2])
                    .map(|claim| claim.fire),
                [false; 2]
            );
            let mut absent = input.clone();
            absent
                .get_mut(OnFootAction::FireOffHandPistol.id())
                .unwrap()
                .hands[dominant.other().index()] = Intent::default();
            mask_routes(&mut absent, mode, masks(owners, blocked));
            let absent_step =
                Bindings::new().step(mode, &controls, &absent, 0.01, &BindingsConfig::default());
            assert!(
                !absent_step
                    .desired
                    .iter()
                    .any(|desired| desired.target.action == Action::FIRE_RIGHT),
                "raw trigger cannot synthesize a false semantic fire sample"
            );
            let moved = [owner(
                Some(1),
                GunHand::NativePair(NativePair {
                    first_hand: dominant.other(),
                }),
            ); 2];
            assert_eq!(gate.after_finish(moved), [true; 2]);
            let blocked = gate.observe(moved, [raw(true); 2], 0.05);
            let mut held_input = simulate(&controls, SemanticSet::OnFoot, default_binding);
            mask_routes(&mut held_input, mode, masks(moved, blocked));
            assert!(
                !Bindings::new()
                    .step(
                        mode,
                        &controls,
                        &held_input,
                        0.01,
                        &BindingsConfig::default()
                    )
                    .desired
                    .iter()
                    .any(|desired| desired.target.action == Action::FIRE_RIGHT)
            );
        }
    }

    #[test]
    fn late_pickup_clears_old_trigger_action_and_fire_until_neutral() {
        let mode = Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver: true,
        });
        let owners = [Owner { mode, gun: None }; 2];
        let mut gate = TriggerOwnership::new();
        gate.observe(owners, [raw(false); 2], 0.05);
        assert_eq!(gate.observe(owners, [raw(true); 2], 0.05), [false; 2]);
        assert_eq!(gate.after_finish(owners), [false; 2]);
        let mut picked_up = owners;
        picked_up[Hand::Right.index()].gun = Some(GunOwner {
            weapon: 2,
            role: GunHand::Grip,
        });
        let changed = gate.after_finish(picked_up);
        assert_eq!(changed, [false, true]);
        assert_eq!(retain_fire([true; 2], changed), [true, false]);
        let controls = Controls {
            dominant: Hand::Right,
            ..Default::default()
        };
        let input = simulate(&controls, SemanticSet::LandVehicle, default_binding);
        let mut desired = [Action::ACCELERATE, Action::REVERSE, Action::TURN_LEFT]
            .map(|action| Desired {
                target: Target::player(action),
                want: Want::Hold(1.0),
            })
            .to_vec();
        suppress_changed_routes(&mut desired, mode, &controls, &input, changed);
        assert!(
            !desired
                .iter()
                .any(|desired| desired.target.action == Action::ACCELERATE)
        );
        assert!(
            desired
                .iter()
                .any(|desired| desired.target.action == Action::REVERSE)
        );
        assert!(gate.observe(picked_up, [raw(true); 2], 0.05)[Hand::Right.index()]);
        assert!(gate.observe(picked_up, [raw(false); 2], 0.05)[Hand::Right.index()]);
        assert!(!gate.observe(picked_up, [raw(true); 2], 0.05)[Hand::Right.index()]);
    }

    #[test]
    fn ownership_change_cancels_delayed_trigger_grapple_but_preserves_independent_taps() {
        let mode = Mode::OnFoot(OnFootVariant::Ground);
        let config = BindingsConfig::default();
        let sources = [
            PhysicalSet::of(Physical::Trigger),
            PhysicalSet::of(Physical::Trigger).with(Physical::UpperFace),
            PhysicalSet::EMPTY.with_unknown(),
            PhysicalSet::EMPTY,
            PhysicalSet::of(Physical::UpperFace),
            PhysicalSet::of(Physical::Grip),
        ];
        for hand in Hand::BOTH {
            let controls = Controls {
                dominant: hand.other(),
                grapple: GrappleContext {
                    tethers: true,
                    ..Default::default()
                },
                ..Default::default()
            };
            for source in sources {
                for released_before_transfer in [false, true] {
                    for after_table in [false, true] {
                        let mut gate = TriggerOwnership::new();
                        let mut owners = [owner(None, GunHand::Empty); 2];
                        gate.observe(owners, [raw(false); 2], config.trigger_deadzone);
                        let raw_held = Hand::BOTH.map(|h| raw(h == hand));
                        assert_eq!(
                            gate.observe(owners, raw_held, config.trigger_deadzone),
                            [false; 2]
                        );
                        let mut bindings = Bindings::new();
                        let mut input = simulate(&controls, SemanticSet::OnFoot, default_binding);
                        let action = input.get_mut(OnFootAction::Grapple.id()).unwrap();
                        action.bound[hand.index()] = source;
                        action.hands[hand.index()].button = ButtonControl {
                            down: true,
                            pressed: true,
                            released: false,
                        };
                        let step = bindings.step(mode, &controls, &input, 0.01, &config);
                        assert!(
                            !step
                                .desired
                                .iter()
                                .any(|d| d.target.action == Action::FIRE_GRAPPLE)
                        );
                        if released_before_transfer {
                            input.get_mut(OnFootAction::Grapple.id()).unwrap().hands
                                [hand.index()]
                            .button = ButtonControl {
                                down: false,
                                pressed: false,
                                released: true,
                            };
                            bindings.step(mode, &controls, &input, 0.01, &config);
                        }
                        input.get_mut(OnFootAction::Grapple.id()).unwrap().hands[hand.index()]
                            .button = ButtonControl {
                            down: !released_before_transfer,
                            ..Default::default()
                        };
                        owners[hand.index()].gun = Some(GunOwner {
                            weapon: 1,
                            role: GunHand::Grip,
                        });
                        if after_table {
                            bindings.step(mode, &controls, &input, 0.01, &config);
                            let changed = gate.after_finish(owners);
                            assert_eq!(changed, Hand::BOTH.map(|h| h == hand));
                            bindings.cancel_trigger_gestures(changed);
                        } else {
                            let blocked = gate.observe(owners, raw_held, config.trigger_deadzone);
                            assert_eq!(blocked, Hand::BOTH.map(|h| h == hand));
                            bindings.cancel_trigger_gestures(blocked);
                            mask_routes(&mut input, mode, masks(owners, blocked));
                            bindings.step(mode, &controls, &input, 0.01, &config);
                        }
                        input.get_mut(OnFootAction::Grapple.id()).unwrap().hands[hand.index()]
                            .button = ButtonControl::default();
                        let mut fired = false;
                        for _ in 0..6 {
                            let step = bindings.step(mode, &controls, &input, 0.1, &config);
                            fired |= step
                                .desired
                                .iter()
                                .any(|d| d.target.action == Action::FIRE_GRAPPLE);
                        }
                        let independent = source == PhysicalSet::of(Physical::UpperFace)
                            || source == PhysicalSet::of(Physical::Grip);
                        assert_eq!(
                            fired, independent,
                            "{hand:?} {source:?}, released first {released_before_transfer}, after table {after_table}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn tracking_loss_cancels_pending_trigger_grapple_before_masked_release() {
        let mode = Mode::OnFoot(OnFootVariant::Ground);
        let hand = Hand::Left;
        let controls = Controls {
            dominant: hand.other(),
            grapple: GrappleContext {
                tethers: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let config = BindingsConfig::default();
        let owners = [owner(None, GunHand::Empty); 2];
        let mut gate = TriggerOwnership::new();
        gate.observe(owners, [raw(false); 2], config.trigger_deadzone);
        let raw_held = Hand::BOTH.map(|h| raw(h == hand));
        assert_eq!(
            gate.observe(owners, raw_held, config.trigger_deadzone),
            [false; 2]
        );
        let mut bindings = Bindings::new();
        let mut input = simulate(&controls, SemanticSet::OnFoot, default_binding);
        let action = input.get_mut(OnFootAction::Grapple.id()).unwrap();
        action.bound[hand.index()] = PhysicalSet::of(Physical::Trigger);
        action.hands[hand.index()].button = ButtonControl {
            down: true,
            pressed: true,
            released: false,
        };
        bindings.step(mode, &controls, &input, 0.01, &config);
        let mut raw_lost = raw_held;
        raw_lost[hand.index()].usable = false;
        let blocked = gate.observe(owners, raw_lost, config.trigger_deadzone);
        assert_eq!(blocked, [true, false]);
        bindings.cancel_trigger_gestures(blocked);
        mask_routes(&mut input, mode, masks(owners, blocked));
        for _ in 0..6 {
            let step = bindings.step(mode, &controls, &input, 0.1, &config);
            assert!(
                !step
                    .desired
                    .iter()
                    .any(|d| d.target.action == Action::FIRE_GRAPPLE)
            );
        }
    }

    #[test]
    fn buffered_taps_unusable_tracking_and_suspend_do_not_rearm() {
        let mut gate = TriggerOwnership::new();
        let owners = [owner(Some(1), GunHand::Grip); 2];
        let tap = RawTrigger {
            button: ButtonControl {
                pressed: true,
                released: true,
                down: false,
            },
            ..raw(false)
        };
        assert_eq!(gate.observe(owners, [tap; 2], 0.05), [true; 2]);
        assert_eq!(gate.observe(owners, [raw(true); 2], 0.05), [true; 2]);
        gate.observe(owners, [raw(false); 2], 0.05);
        assert_eq!(
            gate.observe(
                owners,
                [RawTrigger {
                    usable: false,
                    ..raw(false)
                }; 2],
                0.05
            ),
            [true; 2]
        );
        assert_eq!(gate.observe(owners, [raw(true); 2], 0.05), [true; 2]);
        gate.suspend();
        assert_eq!(gate.observe(owners, [raw(true); 2], 0.05), [true; 2]);
    }
}
