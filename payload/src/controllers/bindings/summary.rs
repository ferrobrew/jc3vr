//! Current wrist eligibility from the input tick's effective routes and post-finish owners.

use jc3gi::{
    character::character::{Character, WeaponInput},
    equipment::weapon::WeaponBase,
    input::input_action_map::Action,
};

use crate::{
    controllers::{
        aim::gun,
        bindings::{
            controls::Controls,
            grapple_button::GrapplePermissions,
            intents::SemanticInput,
            policy_a::runtime::Tick,
            tables::{
                self,
                describe::{Control, Effect, Gesture, ModAction, Row},
            },
            trigger_ownership::{Owner, TriggerMask},
        },
        config::ControllersConfig,
        frame::Hand,
        grab::{GrabVolume, GripTarget, PickupKind},
        hands::{
            late,
            tiers::{self, HandTier, TierState},
        },
        mode::{AirborneVariant, Detection, Mode},
        snatch,
        summary::{
            current::{CurrentRouting, TriggerUse},
            eligibility::{Eligibility, SnatchHint},
        },
        vehicles::{
            parachute::HandleHold, physical::ControlKind, wingsuit_gestures::GestureAvailability,
        },
        weapons::{
            self,
            capabilities::{GunHand, GunOwner},
            reload::{BeltEligibility, ReloadStatus, config::ReloadButton},
        },
    },
    vr::input::semantic::{bound::PhysicalSet, vocabulary::ActionId},
};

/// State that can invalidate an offer between the provider pass and final publication.
pub(super) struct BeforeFinish {
    guns: [Option<GunOwner>; 2],
    reload: ReloadStatus,
    foregrip: Option<(usize, Hand, bool)>,
    grenade: [bool; 2],
    c4: Option<Hand>,
    parachute: [Option<HandleHold>; 2],
}

impl BeforeFinish {
    pub(super) fn capture(parachute: [Option<HandleHold>; 2]) -> Self {
        let guns = weapons::capabilities::gun_owners_now();
        Self {
            guns,
            reload: current_reload(guns),
            foregrip: foregrip_owner(),
            grenade: Hand::BOTH.map(weapons::grenade::holds),
            c4: weapons::c4::throwing_hand(),
            parachute,
        }
    }
}

pub(super) struct Refresh<'a> {
    pub sequence: u64,
    pub belt_reload: Option<BeltEligibility>,
    pub config: &'a ControllersConfig,
    pub owners: [Owner; 2],
    pub ready: [bool; 2],
    pub grapple_routes: Vec<(ActionId, Hand)>,
    pub trigger_blocked: [bool; 2],
    pub chosen: [Option<GrabVolume>; 2],
    pub selected: [Option<GripTarget>; 2],
    pub grabs: [Option<GrabVolume>; 2],
    pub before: &'a BeforeFinish,
    pub grapple: GrapplePermissions,
    pub yank: Option<Hand>,
    pub flight: GestureAvailability,
    /// The vehicle owner's active handle holds after finish, indexed by physical hand.
    pub parachute: [Option<HandleHold>; 2],
}

pub(super) fn grapple_routes(
    bindings: &tables::Bindings,
    mode: Mode,
    controls: &Controls,
    input: &SemanticInput,
    policy: &Tick,
    triggers: [TriggerMask; 2],
) -> Vec<(ActionId, Hand)> {
    let Some((actions, _)) = tables::grapple_press(mode) else {
        return Vec::new();
    };
    if !tables::Bindings::grapple_gate_open(mode, controls) {
        return Vec::new();
    }
    let masks = bindings.semantic_masks(mode, controls);
    actions
        .iter()
        .flat_map(|&id| Hand::BOTH.map(|hand| (id, hand)))
        .filter(|&(id, hand)| {
            policy.permits(id, hand)
                && input.get(id).is_some_and(|action| {
                    let bound = action.bound[hand.index()];
                    bound != PhysicalSet::EMPTY
                        && !triggers[hand.index()].covers(mode, id, bound)
                        && !masks
                            .iter()
                            .any(|mask| mask.hand == hand && mask.covers(id, bound))
                })
        })
        .collect()
}

pub(super) fn capture(input: Refresh<'_>) -> Eligibility {
    let parachute = input.parachute;
    let guns = weapons::capabilities::gun_owners_now();
    let reload = current_reload(guns);
    let fire = fire_ready(input.ready, input.config, guns);
    let reload_permissions = read_reload_permissions(input.config);
    let doors = Hand::BOTH.map(|hand| {
        input.selected[hand.index()]
            .and_then(|target| target.door_eligibility)
            .is_some_and(|offer| offer.current(hand, &input.config.physical_hands))
    });
    let hint = snatch::hint().filter(|(hand, _)| {
        input.ready[hand.index()]
            && matches!(input.owners[hand.index()].mode, Mode::OnFoot(_))
            && input.config.weapons.snatch.enabled
    });
    let modifier_enabled = input.config.weapons.snatch.sling;
    let mut input = input;
    let routes = std::mem::take(&mut input.grapple_routes);
    let mut eligibility = capture_current(
        input,
        BeforeFinish {
            guns,
            reload,
            foregrip: foregrip_owner(),
            grenade: Hand::BOTH.map(weapons::grenade::holds),
            c4: weapons::c4::throwing_hand(),
            parachute,
        },
        fire,
        reload_permissions,
        doors,
    );
    eligibility.snatch = hint.map(|(hand, swinging)| SnatchHint {
        hand,
        swinging,
        modifier_enabled,
        routes,
    });
    eligibility
}

fn capture_current(
    input: Refresh<'_>,
    current: BeforeFinish,
    fire: [bool; 2],
    reload_permissions: ReloadPermissions,
    doors: [bool; 2],
) -> Eligibility {
    let guns = current.guns;
    let reload = current.reload;
    let owners_changed = input.before.guns != guns
        || reload_changed(&input.before.reload, &reload)
        || input.before.foregrip != current.foregrip
        || input.before.grenade != current.grenade
        || input.before.c4 != current.c4
        || input.before.parachute != current.parachute;
    let chosen = retained_choices(input.chosen, input.grabs, owners_changed);
    let fires_guns = tables::triggers_fire_hand_weapons(input.owners[0].mode);
    let mut actions = Vec::new();
    if fire.contains(&true) && fires_guns {
        actions.extend([Action::FIRE_RIGHT, Action::R2]);
    }
    if input.grapple.release {
        actions.extend([
            Action::RETRACT_GRAPPLE,
            Action::RELEASE_GRAPPLE,
            Action::VEHICLE_RELEASE_GRAPPLE,
        ]);
    }
    let mut mechanics = tables::describe::flight_mechanics(input.flight);
    if let Some(row) = input.belt_reload.and_then(|belt| {
        belt.row(
            input.sequence,
            input.config.acting_hand(),
            guns,
            input.ready,
        )
    }) {
        mechanics.push(row);
    }
    if let Some(hand) = input.yank {
        mechanics.push(Row {
            control: Control::Motion(hand),
            gesture: Gesture::Yank,
            effect: Effect::Actions(vec![Action::FIRE_GRAPPLE]),
        });
    }
    for hand in Hand::BOTH {
        let i = hand.index();
        if input.config.vehicles.parachute_handles.enabled
            && input.owners[i].mode == Mode::Airborne(AirborneVariant::Parachute)
            && input.ready[i]
            && chosen[i] == Some(GrabVolume::ParachuteHandle(hand))
            && let Some(target) = input.selected[i].filter(|target| {
                target.volume == GrabVolume::ParachuteHandle(hand) && target.hand == Some(hand)
            })
        {
            let action = match (target.held_by, current.parachute[i]) {
                (None, None) => Some((Control::Grip(hand), ModAction::ParachuteHandles)),
                (Some(holder), Some(_)) if holder == hand => {
                    Some((Control::Motion(hand), ModAction::PullParachuteHandle))
                }
                _ => None,
            };
            if let Some((control, action)) = action {
                mechanics.push(Row {
                    control,
                    gesture: Gesture::Press,
                    effect: Effect::Mod(action),
                });
            }
        }
        if input.ready[i]
            && chosen[i] == Some(GrabVolume::Climb)
            && input.selected[i].is_some_and(|target| {
                target.volume == GrabVolume::Climb
                    && target.hand == Some(hand)
                    && target.held_by.is_none()
            })
        {
            mechanics.push(Row {
                control: Control::Grip(hand),
                gesture: Gesture::Press,
                effect: Effect::Mod(ModAction::Climb),
            });
        }
        if doors[i]
            && input.ready[i]
            && chosen[i] == Some(GrabVolume::Pickup(PickupKind::Door))
            && input.selected[i].is_some_and(|target| {
                target.volume == GrabVolume::Pickup(PickupKind::Door)
                    && target.hand == Some(hand)
                    && target.held_by.is_none()
            })
        {
            mechanics.push(Row {
                control: Control::Grip(hand),
                gesture: Gesture::Press,
                effect: Effect::Mod(ModAction::OpenVehicleDoor),
            });
        }
        if input.config.weapons.reload.enabled
            && input.config.weapons.reload.flick_eject
            && flick_ready(&reload, guns, hand)
        {
            mechanics.push(Row {
                control: Control::Motion(hand),
                gesture: Gesture::Flick,
                effect: Effect::Mod(ModAction::FlickEject),
            });
        }
        if let Some(GrabVolume::VehicleControl(kind)) = chosen[i] {
            let action = match kind {
                ControlKind::Wheel => ModAction::Wheel,
                ControlKind::Handlebars => ModAction::Handlebars,
                ControlKind::Cyclic | ControlKind::Collective => ModAction::HelicopterControls,
                ControlKind::FlightStick | ControlKind::Throttle => ModAction::PlaneControls,
            };
            mechanics.push(Row {
                control: Control::Grip(hand),
                gesture: Gesture::Press,
                effect: Effect::Mod(action),
            });
        }
    }
    Eligibility {
        routing: CurrentRouting {
            ready: input.ready,
            triggers: Hand::BOTH.map(|hand| {
                let i = hand.index();
                trigger_use(input.owners[i], input.trigger_blocked[i], fire[i])
            }),
        },
        chosen,
        actions,
        mechanics,
        snatch: None,
        native_reload: reload_permissions.native_reload,
        held_holster: reload_permissions.held_holster,
        tighten: input.grapple.tighten,
        tethers: input.grapple.release,
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
struct ReloadPermissions {
    native_reload: bool,
    held_holster: bool,
}

struct ReloadPermissionRead {
    native_button: bool,
    wielded: bool,
    reload_animation: bool,
    detonate_clear: bool,
    can_reload: bool,
    holster_animation: bool,
}

fn read_reload_permissions(config: &ControllersConfig) -> ReloadPermissions {
    // SAFETY: the input tick reads the live local character and its wielded inventory entry.
    let Some(owner) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return ReloadPermissions::default();
    };
    // SAFETY: GetWieldingWeapon returns an inventory-owned shared pointer or a static empty one.
    let weapon = unsafe { owner.GetWieldingWeapon(false).as_ref() }
        .and_then(|weapon| unsafe { weapon.px.as_ref() });
    reload_permission(ReloadPermissionRead {
        native_button: !config.weapons.reload.enabled
            || config.weapons.reload.button == ReloadButton::Native,
        wielded: weapon.is_some(),
        // SAFETY: the inventory and wielded weapon remain live for this input-thread read.
        reload_animation: unsafe { owner.m_Inventory.GetAnimationAllowsReloading() },
        detonate_clear: owner.m_WeaponInputs[WeaponInput::E_INPUT_DETONATE_EXPLOSIVES as usize]
            == 0.0,
        can_reload: weapon.is_some_and(|weapon| unsafe { weapon.CanReload() }),
        // SAFETY: this query also includes the inventory owner's holster-disable state.
        holster_animation: unsafe { owner.m_Inventory.GetAnimationAllowsHolstering() },
    })
}

fn reload_permission(read: ReloadPermissionRead) -> ReloadPermissions {
    ReloadPermissions {
        native_reload: read.wielded
            && read.native_button
            && read.reload_animation
            && read.detonate_clear
            && read.can_reload,
        held_holster: read.wielded && read.holster_animation,
    }
}

fn trigger_use(owner: Owner, blocked: bool, fire: bool) -> TriggerUse {
    if blocked {
        return TriggerUse::Reserved;
    }
    match owner.gun.map(|gun| gun.role) {
        None => TriggerUse::Free,
        Some(GunHand::Grip) if fire => TriggerUse::Gun,
        Some(GunHand::NativePair(_)) if fire && tables::triggers_fire_hand_weapons(owner.mode) => {
            TriggerUse::NativePair
        }
        Some(_) => TriggerUse::Reserved,
    }
}

fn current_reload(guns: [Option<GunOwner>; 2]) -> ReloadStatus {
    let mut reload = weapons::reload::status();
    refresh_magazine_out(&mut reload, guns, |address| {
        // SAFETY: matching the current gun owner proves this weapon is live on the input thread.
        unsafe { (address as *const WeaponBase).as_ref() }
            .map(|weapon| weapon.m_WeaponIsWithoutMagazine)
    });
    reload
}

fn refresh_magazine_out(
    reload: &mut ReloadStatus,
    guns: [Option<GunOwner>; 2],
    mut read: impl FnMut(usize) -> Option<bool>,
) {
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(weapon) = reload.weapons[i].as_mut() else {
            continue;
        };
        if guns[i].is_some_and(|gun| gun.weapon == weapon.address)
            && let Some(out) = read(weapon.address)
        {
            weapon.out = out;
        }
    }
}

fn flick_ready(reload: &ReloadStatus, guns: [Option<GunOwner>; 2], hand: Hand) -> bool {
    let i = hand.index();
    reload.weapons[i].is_some_and(|weapon| {
        guns[i].is_some_and(|gun| gun.weapon == weapon.address)
            && weapon.plan.load.ejects()
            && !weapon.out
    })
}

fn foregrip_owner() -> Option<(usize, Hand, bool)> {
    weapons::foregrip::status()
        .hold
        .map(|hold| (hold.weapon, hold.holder, hold.engaged))
}

fn retained_choices(
    chosen: [Option<GrabVolume>; 2],
    grabs: [Option<GrabVolume>; 2],
    changed: bool,
) -> [Option<GrabVolume>; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        // A finish grab can consume or replace its target, including an asynchronous pickup.
        // Owner mutations can also invalidate the other hand's offer; a normal provider pass
        // supplies replacements on the next input tick.
        chosen[i].filter(|_| grabs[i].is_none() && !changed)
    })
}

fn reload_changed(before: &ReloadStatus, after: &ReloadStatus) -> bool {
    before.weapons != after.weapons
        || before.insertion != after.insertion
        || before.insert_hands != after.insert_hands
        || before.belt != after.belt
        || before.work != after.work
        || before.fire_held != after.fire_held
}

/// The same line and traversal gate used by yank execution; the caller also requires the button.
pub(super) fn yank_armed_in(wire: bool, detection: &Detection) -> bool {
    wire && detection.traversal.is_none()
        && matches!(detection.mode, Mode::OnFoot(_) | Mode::Airborne(_))
}

fn fire_ready(
    ready: [bool; 2],
    config: &ControllersConfig,
    guns: [Option<GunOwner>; 2],
) -> [bool; 2] {
    let base = weapons::capabilities::fire_ready_now(ready, false);
    let action = tiers::status();
    let action_running = Hand::BOTH.map(|hand| {
        config.weapons.block_fire_during_actions
            && action.state == Some(TierState::WeaponAction)
            && action.effective[hand.index()] == HandTier::ActionRuns
    });
    // SAFETY: the input tick reads the current local character and wielded weapon.
    let weapon = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .and_then(|owner| unsafe { gun::held_weapon(owner) });
    let primary = weapon.map(|weapon| weapon as *const _ as usize);
    let holder = primary.and_then(weapons::hold::owned_holder).or_else(|| {
        let weapon = weapon?;
        // SAFETY: the live wielded weapon owns this gun array on the input thread.
        unsafe { gun::guns(weapon) }.flatten().find_map(|data| {
            data.m_lBoneAttachementPerWeaponState
                .get(weapon.m_State as usize)
                .and_then(|&bone| late::attach_hand(bone))
        })
    });
    fire_permission(FirePermission {
        base,
        guns,
        docking: weapons::hold::act::docking(),
        carrying: guns
            .map(|gun| gun.is_some_and(|gun| weapons::regions::carrying(gun.weapon).is_some())),
        primary,
        holder,
        action_running,
    })
}

struct FirePermission {
    base: [bool; 2],
    guns: [Option<GunOwner>; 2],
    docking: [bool; 2],
    carrying: [bool; 2],
    primary: Option<usize>,
    holder: Option<Hand>,
    action_running: [bool; 2],
}

fn fire_permission(input: FirePermission) -> [bool; 2] {
    let primary_blocked = input.holder.is_some_and(|holder| {
        input.docking[holder.index()] || input.action_running[holder.index()]
    });
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let Some(gun) = input.guns[i] else {
            return false;
        };
        let primary = Some(gun.weapon) == input.primary;
        match gun.role {
            GunHand::NativePair(_) => {
                // The pair keeps its shared semantic route unless both reload gates hold fire.
                input.base.contains(&true)
                    && !input.carrying.contains(&true)
                    && !(primary && primary_blocked)
            }
            GunHand::Grip => {
                input.base[i]
                    && !input.docking[i]
                    && !input.carrying[i]
                    && !(primary && primary_blocked)
            }
            _ => false,
        }
    })
}

#[cfg(test)]
mod capture_tests;

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::controllers::{
        mode::{OnFootVariant, Traversal, VehicleClass, VehicleSeat},
        weapons::{
            capabilities::NativePair,
            reload::{
                WeaponReadout,
                chamber::{ActionKind, Chamber},
                plan::{LoadKind, ReloadPlan, WellFrame},
            },
        },
    };

    fn reload_read() -> ReloadPermissionRead {
        ReloadPermissionRead {
            native_button: true,
            wielded: true,
            reload_animation: true,
            detonate_clear: true,
            can_reload: true,
            holster_animation: true,
        }
    }

    #[test]
    fn full_magazine_does_not_block_allowed_held_holster() {
        let read = ReloadPermissionRead {
            can_reload: false,
            ..reload_read()
        };
        assert_eq!(
            reload_permission(read),
            ReloadPermissions {
                native_reload: false,
                held_holster: true
            },
        );
    }

    #[test]
    fn reload_permission_is_independent_of_denied_holster() {
        let read = ReloadPermissionRead {
            holster_animation: false,
            ..reload_read()
        };
        assert_eq!(
            reload_permission(read),
            ReloadPermissions {
                native_reload: true,
                held_holster: false
            },
        );
    }

    #[test]
    fn eject_mode_still_allows_held_holster_without_a_reload_gate() {
        let read = ReloadPermissionRead {
            native_button: false,
            reload_animation: false,
            detonate_clear: false,
            can_reload: false,
            ..reload_read()
        };
        assert_eq!(
            reload_permission(read),
            ReloadPermissions {
                native_reload: false,
                held_holster: true
            },
        );
        assert_eq!(
            reload_permission(ReloadPermissionRead {
                wielded: false,
                ..reload_read()
            }),
            ReloadPermissions::default(),
        );
    }

    #[test]
    fn independent_grips_fire_in_driver_and_wingsuit_modes_but_pairs_do_not() {
        for mode in [
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
            Mode::Airborne(AirborneVariant::Wingsuit),
        ] {
            let owner = Owner {
                mode,
                gun: Some(GunOwner {
                    weapon: 1,
                    role: GunHand::Grip,
                }),
            };
            assert_eq!(trigger_use(owner, false, true), TriggerUse::Gun);
            assert_eq!(trigger_use(owner, true, true), TriggerUse::Reserved);
            assert_eq!(trigger_use(owner, false, false), TriggerUse::Reserved);
            let pair = Owner {
                gun: Some(GunOwner {
                    weapon: 1,
                    role: GunHand::NativePair(NativePair {
                        first_hand: Hand::Right,
                    }),
                }),
                ..owner
            };
            assert_eq!(trigger_use(pair, false, true), TriggerUse::Reserved);
        }
    }

    #[test]
    fn post_finish_magazine_reader_removes_successful_but_not_refused_ejects() {
        let hand = Hand::Right;
        let i = hand.index();
        let mut stale = ReloadStatus::default();
        stale.weapons[i] = Some(WeaponReadout {
            address: 1,
            name_hash: 0,
            plan: ReloadPlan {
                load: LoadKind::Magazine,
                action: ActionKind::None,
                well: WellFrame::Wrist { offset: Vec3::ZERO },
                handle: None,
                travel_m: 0.0,
                part_top_m: None,
            },
            from_table: false,
            magazine: 1,
            capacity: 2,
            reserve: 1,
            unlimited: false,
            out: false,
            chamber: Chamber::Chambered,
            action: None,
        });
        let guns = Hand::BOTH.map(|h| {
            (h == hand).then_some(GunOwner {
                weapon: 1,
                role: GunHand::Grip,
            })
        });
        let mut before = stale.clone();
        refresh_magazine_out(&mut before, guns, |_| Some(false));
        assert!(flick_ready(&before, guns, hand));
        let chosen = Hand::BOTH.map(|h| (h == hand).then_some(GrabVolume::InsertedMagazine));
        for successful in [false, true] {
            let mut after = stale.clone();
            refresh_magazine_out(&mut after, guns, |address| {
                assert_eq!(address, 1);
                Some(successful)
            });
            assert_eq!(flick_ready(&after, guns, hand), !successful);
            assert_eq!(reload_changed(&before, &after), successful);
            assert_eq!(
                retained_choices(chosen, [None; 2], reload_changed(&before, &after))[i],
                if successful { None } else { chosen[i] },
            );
        }
        let mut mismatched = stale;
        refresh_magazine_out(&mut mismatched, [None; 2], |_| {
            panic!("unowned weapon read")
        });
        assert_eq!(mismatched, before);
    }

    #[test]
    fn the_yank_is_armed_only_on_a_free_line() {
        let on_foot = Detection {
            mode: Mode::OnFoot(OnFootVariant::Ground),
            ..Default::default()
        };
        assert!(yank_armed_in(true, &on_foot));
        assert!(!yank_armed_in(false, &on_foot));
        let falling = Detection {
            mode: Mode::Airborne(AirborneVariant::Falling),
            ..Default::default()
        };
        assert!(yank_armed_in(true, &falling));
        for traversal in [
            Traversal::PreReel,
            Traversal::ReelingIn,
            Traversal::PreHang,
            Traversal::ReeledIn,
        ] {
            let carried = Detection {
                traversal: Some(traversal),
                ..on_foot
            };
            assert!(!yank_armed_in(true, &carried), "{traversal:?}");
        }
        assert!(!yank_armed_in(
            true,
            &Detection {
                mode: Mode::Ui,
                ..on_foot
            }
        ));
    }

    fn pair() -> FirePermission {
        FirePermission {
            base: [true; 2],
            guns: [Some(GunOwner {
                weapon: 1,
                role: GunHand::NativePair(NativePair {
                    first_hand: Hand::Right,
                }),
            }); 2],
            docking: [false; 2],
            carrying: [false; 2],
            primary: Some(1),
            holder: Some(Hand::Right),
            action_running: [false; 2],
        }
    }

    #[test]
    fn pair_docking_and_carrying_block_shared_fire() {
        let mut input = pair();
        input.docking[Hand::Right.index()] = true;
        assert_eq!(fire_permission(input), [false; 2]);
        let mut input = pair();
        input.carrying[Hand::Left.index()] = true;
        assert_eq!(fire_permission(input), [false; 2]);
    }

    #[test]
    fn pair_reload_is_shared_but_primary_action_uses_the_holder() {
        let mut input = pair();
        input.base = [false, true];
        assert_eq!(fire_permission(input), [true; 2]);
        let mut input = pair();
        input.base = [false; 2];
        assert_eq!(fire_permission(input), [false; 2]);
        let mut input = pair();
        input.action_running[Hand::Left.index()] = true;
        assert_eq!(fire_permission(input), [true; 2]);
        let mut input = pair();
        input.action_running[Hand::Right.index()] = true;
        assert_eq!(fire_permission(input), [false; 2]);
    }

    #[test]
    fn consumed_and_changed_choices_wait_for_the_next_provider_pass() {
        let chosen = [Some(GrabVolume::MagazinePouch), Some(GrabVolume::Foregrip)];
        assert_eq!(
            retained_choices(chosen, [Some(GrabVolume::MagazinePouch), None], false),
            [None, chosen[1]],
        );
        assert_eq!(retained_choices(chosen, [None; 2], true), [None; 2]);
        assert_eq!(retained_choices(chosen, [None; 2], false), chosen);
    }
}
