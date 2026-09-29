//! The hand tiers: which of the plan's four tiers each scripted state assigns the hands, and the
//! per-hand weight that blends a hand between its controller and the animation.
//!
//! The plan of record ("Hand tiers" in `docs/mod/input/controllers-and-roomscale.md`) assigns every
//! scripted state one of four tiers. The assignments are data: [`HandTierTable`] holds one tier per
//! [`TierState`], its default is the plan's table ([`HandTierTable::PLAN`]), and the config can
//! override any row. The tiers:
//!
//! - [`HandTier::Free`]: the hand follows its controller;
//! - [`HandTier::ActionRuns`]: the game plays the action (a reload's timer and the weapon model's
//!   own magazine and bolt animation, a draw, a holster) while the hand stays on its controller;
//!   the weapon handling holds the fire inputs back for the action's duration
//!   ([`crate::controllers::weapons`]);
//! - [`HandTier::Anchored`]: the hand is attached to a world point, and its movement drives the
//!   game. It is built for the grapple states and the mounted guns ([`TierState::anchor_built`]):
//!   the grapple hand holds the line ([`crate::controllers::hands::anchor`]), and while hanging its
//!   displacement swings the character. A grapple hand that holds a gun (the second pistol, or a
//!   single gun in a left-handed layout) stays free instead, so that its gun aims where the
//!   controller points rather than along the line; on a mounted gun both hands hold the handles, and their
//!   positions aim the gun ([`crate::controllers::vehicles::mounted`]). A state assigned the tier
//!   where it is not built yields. The physical vehicle controls anchor a holding hand within the
//!   seated state's free tier ([`crate::controllers::vehicles::physical`]), because a hand holds a
//!   control only while the player grips it;
//! - [`HandTier::Yielded`]: the animation owns the hand, which blends out from the controller over
//!   [`HandsConfig::yield_blend_s`] and back over [`HandsConfig::return_blend_s`].
//!
//! The classification runs once per input tick ([`tick`]), on the main thread, from the mode
//! detector's snapshot plus a few state-machine predicates, and publishes one weight per hand for
//! the IK and the late update to read ([`weights`]).

use jc3gi::{
    camera::game_camera_manager::GameCameraManager, character::character::Character,
    state::StateBitFlag,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    controllers::{
        config::HandsConfig,
        frame::Hand,
        mode::{
            self, AirborneVariant, CharacterSnapshot, Detection, Mode, OnFootVariant, Traversal,
        },
        weapons,
    },
};

/// The plan's hand tiers.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandTier {
    /// The hand follows its controller and nothing is attached to it.
    Free,
    /// The game performs the action while the hand stays on its controller.
    ActionRuns,
    /// The hand is attached to a world point, and its movement drives the game.
    Anchored,
    /// The animation owns the hand.
    Yielded,
}

impl HandTier {
    /// Every tier, in the plan's order.
    pub const ALL: [HandTier; 4] = [
        HandTier::Free,
        HandTier::ActionRuns,
        HandTier::Anchored,
        HandTier::Yielded,
    ];

    /// The tier the hands actually take in `state`: the anchored tier where it is built and
    /// switched on (`anchor`), and the yielded tier in its place elsewhere, as the plan starts every
    /// scripted state yielded until its tier is built.
    pub const fn effective_in(self, state: Option<TierState>, anchor: bool) -> HandTier {
        match self {
            HandTier::Anchored => match state {
                Some(state) if anchor && state.anchor_built() => HandTier::Anchored,
                _ => HandTier::Yielded,
            },
            other => other,
        }
    }

    /// The weight the hand's controller blends toward in this effective tier: one follows the
    /// controller, zero leaves the hand to the animation. An anchored hand stays where its
    /// controller is.
    pub const fn controller_weight(self) -> f32 {
        match self {
            HandTier::Free | HandTier::ActionRuns | HandTier::Anchored => 1.0,
            HandTier::Yielded => 0.0,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            HandTier::Free => "free",
            HandTier::ActionRuns => "action runs",
            HandTier::Anchored => "anchored",
            HandTier::Yielded => "yielded",
        }
    }
}

/// The scripted states the tier table distinguishes, classified per tick by [`classify`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TierState {
    /// On foot, empty-handed or holding weapons, including grapple fire and falling.
    OnFoot,
    /// The reel-in wind-up and the reel-in flight.
    GrappleReel,
    /// The wind-up to a hang, and reeled onto an attach point (hang, upside-down, and stunt
    /// positions).
    GrappleHang,
    /// Stunt traversal: the step-ups and step-downs, and the other traversal animations.
    StuntTraversal,
    /// Entering, exiting, or switching a vehicle seat, and hijacks.
    VehicleTransition,
    /// Seated in a vehicle and riding it.
    VehicleSeated,
    /// On a mounted gun or turret.
    MountedGun,
    /// The parachute.
    Parachute,
    /// The wingsuit, and the transition into it.
    Wingsuit,
    /// Swimming, on the surface or underwater.
    Swimming,
    /// Context actions and interactions: sabotage, planting explosives, using items, pickups, and
    /// grapple struggles.
    Interaction,
    /// Melee attacks.
    Melee,
    /// A reload, a weapon draw, or a holster.
    WeaponAction,
    /// Ragdoll, getting up, hit reactions, hard landings, death, and scripted warps.
    Ragdoll,
    /// A cutscene or scripted sequence camera.
    Cutscene,
}

impl TierState {
    /// Every state, in the table's order.
    pub const ALL: [TierState; 15] = [
        TierState::OnFoot,
        TierState::GrappleReel,
        TierState::GrappleHang,
        TierState::StuntTraversal,
        TierState::VehicleTransition,
        TierState::VehicleSeated,
        TierState::MountedGun,
        TierState::Parachute,
        TierState::Wingsuit,
        TierState::Swimming,
        TierState::Interaction,
        TierState::Melee,
        TierState::WeaponAction,
        TierState::Ragdoll,
        TierState::Cutscene,
    ];

    /// Whether the anchored tier is built for this state: the grapple states, where the grapple
    /// hand holds the line, and the mounted guns, where the hands hold the handles.
    pub const fn anchor_built(self) -> bool {
        matches!(
            self,
            TierState::GrappleReel | TierState::GrappleHang | TierState::MountedGun
        )
    }

    /// Which hands the state's tier applies to. The grapple states give the tier to the hand that
    /// holds the line; the other hand stays free.
    pub const fn scope(self) -> TierScope {
        match self {
            TierState::GrappleReel | TierState::GrappleHang => TierScope::GrappleHand,
            _ => TierScope::BothHands,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TierState::OnFoot => "on foot",
            TierState::GrappleReel => "grapple reel-in",
            TierState::GrappleHang => "grapple hang",
            TierState::StuntTraversal => "stunt traversal",
            TierState::VehicleTransition => "vehicle entry and exit",
            TierState::VehicleSeated => "vehicle seated",
            TierState::MountedGun => "mounted gun",
            TierState::Parachute => "parachute",
            TierState::Wingsuit => "wingsuit",
            TierState::Swimming => "swimming",
            TierState::Interaction => "interaction",
            TierState::Melee => "melee",
            TierState::WeaponAction => "reload, draw, and holster",
            TierState::Ragdoll => "ragdoll, hit, and death",
            TierState::Cutscene => "cutscene",
        }
    }
}

/// Which hands a state's tier applies to.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TierScope {
    BothHands,
    /// Only the grapple hand (the non-dominant hand); the other hand is free.
    GrappleHand,
}

/// The tier of each [`TierState`]: the plan's table as data, overridable from the config.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandTierTable {
    pub on_foot: HandTier,
    pub grapple_reel: HandTier,
    pub grapple_hang: HandTier,
    pub stunt_traversal: HandTier,
    pub vehicle_transition: HandTier,
    pub vehicle_seated: HandTier,
    pub mounted_gun: HandTier,
    pub parachute: HandTier,
    pub wingsuit: HandTier,
    pub swimming: HandTier,
    pub interaction: HandTier,
    pub melee: HandTier,
    pub weapon_action: HandTier,
    pub ragdoll: HandTier,
    pub cutscene: HandTier,
}

impl HandTierTable {
    /// The plan of record's starting assignments. Driving with stick bindings is free; physical
    /// vehicle controls, which anchor the hands, are opt-in and come later.
    pub const PLAN: HandTierTable = HandTierTable {
        on_foot: HandTier::Free,
        grapple_reel: HandTier::Anchored,
        grapple_hang: HandTier::Anchored,
        stunt_traversal: HandTier::Yielded,
        vehicle_transition: HandTier::Yielded,
        vehicle_seated: HandTier::Free,
        mounted_gun: HandTier::Anchored,
        parachute: HandTier::Free,
        wingsuit: HandTier::Yielded,
        swimming: HandTier::Yielded,
        interaction: HandTier::Yielded,
        melee: HandTier::Yielded,
        weapon_action: HandTier::ActionRuns,
        ragdoll: HandTier::Yielded,
        cutscene: HandTier::Yielded,
    };

    /// The tier assigned to `state`.
    pub fn tier(&self, state: TierState) -> HandTier {
        *self.slot(state)
    }

    /// The mutable slot for `state`, for the debug UI's table editor.
    pub fn tier_mut(&mut self, state: TierState) -> &mut HandTier {
        self.slot_mut(state)
    }

    /// The tier of `hand` in `state`, given which hand is dominant.
    pub fn hand_tier(&self, state: TierState, hand: Hand, dominant: Hand) -> HandTier {
        match state.scope() {
            TierScope::BothHands => self.tier(state),
            TierScope::GrappleHand if hand == dominant.other() => self.tier(state),
            TierScope::GrappleHand => self.on_foot,
        }
    }

    fn slot(&self, state: TierState) -> &HandTier {
        match state {
            TierState::OnFoot => &self.on_foot,
            TierState::GrappleReel => &self.grapple_reel,
            TierState::GrappleHang => &self.grapple_hang,
            TierState::StuntTraversal => &self.stunt_traversal,
            TierState::VehicleTransition => &self.vehicle_transition,
            TierState::VehicleSeated => &self.vehicle_seated,
            TierState::MountedGun => &self.mounted_gun,
            TierState::Parachute => &self.parachute,
            TierState::Wingsuit => &self.wingsuit,
            TierState::Swimming => &self.swimming,
            TierState::Interaction => &self.interaction,
            TierState::Melee => &self.melee,
            TierState::WeaponAction => &self.weapon_action,
            TierState::Ragdoll => &self.ragdoll,
            TierState::Cutscene => &self.cutscene,
        }
    }

    fn slot_mut(&mut self, state: TierState) -> &mut HandTier {
        match state {
            TierState::OnFoot => &mut self.on_foot,
            TierState::GrappleReel => &mut self.grapple_reel,
            TierState::GrappleHang => &mut self.grapple_hang,
            TierState::StuntTraversal => &mut self.stunt_traversal,
            TierState::VehicleTransition => &mut self.vehicle_transition,
            TierState::VehicleSeated => &mut self.vehicle_seated,
            TierState::MountedGun => &mut self.mounted_gun,
            TierState::Parachute => &mut self.parachute,
            TierState::Wingsuit => &mut self.wingsuit,
            TierState::Swimming => &mut self.swimming,
            TierState::Interaction => &mut self.interaction,
            TierState::Melee => &mut self.melee,
            TierState::WeaponAction => &mut self.weapon_action,
            TierState::Ragdoll => &mut self.ragdoll,
            TierState::Cutscene => &mut self.cutscene,
        }
    }
}

/// The engine signals the classification reads, gathered once per input tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TierSignals {
    /// The mode detector's detection this tick.
    pub detection: Detection,
    /// The character's state words and vehicle seat, `None` while the character is unreadable.
    pub character: Option<CharacterSnapshot>,
    /// Whether the character is seated and riding (not entering, exiting, or switching seats).
    pub riding_vehicle: bool,
    /// Whether the character is in a stunt traversal.
    pub stunt_traversing: bool,
    /// Whether the character is on a mounted gun.
    pub mounted_gun: bool,
    /// Whether the selected camera is a cinematic one.
    pub cinematic_camera: bool,
}

/// Classify the tick's state, highest priority first: a cutscene, then the body being thrown
/// about, then the vehicle states, the scripted actions, the traversal and flight states, a weapon
/// action, and finally on foot. `None` while a menu is up or the character is unreadable: the previous state
/// holds.
pub fn classify(signals: &TierSignals) -> Option<TierState> {
    if signals.detection.mode == Mode::Ui {
        return None;
    }
    let character = signals.character?;
    let any = |flags: &[StateBitFlag]| flags.iter().any(|&f| character.has(f));
    if signals.cinematic_camera {
        return Some(TierState::Cutscene);
    }
    if any(&[
        StateBitFlag::SB_IN_RAGDOLL,
        StateBitFlag::SB_IS_GETTING_UP_FROM_RAGDOLL,
        StateBitFlag::SB_IN_RAGDOLL_GRAPPLE_FIRE,
        StateBitFlag::SB_IS_IN_DEAD_STATE,
        StateBitFlag::SB_IN_HIT_REACTION,
        StateBitFlag::SB_IS_LANDING_HARD,
        StateBitFlag::SB_IN_ANCHOR_WARPING,
    ]) {
        return Some(TierState::Ragdoll);
    }
    // A gunner seat is a vehicle attach state outside the riding set, so the mounted gun is tested
    // first, or the seat would read as an entry.
    if signals.mounted_gun {
        return Some(TierState::MountedGun);
    }
    if character.vehicle.is_some() {
        return Some(if signals.riding_vehicle {
            TierState::VehicleSeated
        } else {
            TierState::VehicleTransition
        });
    }
    if any(&[
        StateBitFlag::SB_IN_LOOP_CONTEXT_ACTION,
        StateBitFlag::SB_IN_LOOP_CHILD_CONTEXT_ACTION,
        StateBitFlag::SB_IN_USE_ITEM,
        StateBitFlag::SB_IN_FULLBODY_PICKUP,
        StateBitFlag::SB_IS_PLANTING_EXPLOSIVES,
        StateBitFlag::SB_IN_GRAPPLE_STRUGGLE,
        StateBitFlag::SB_IN_HIJACKLING_EXPLOSIVE_STATE,
    ]) {
        return Some(TierState::Interaction);
    }
    if character.has(StateBitFlag::SB_IN_MELEE) {
        return Some(TierState::Melee);
    }
    if signals.stunt_traversing {
        return Some(TierState::StuntTraversal);
    }
    if any(&[
        StateBitFlag::SB_IS_WINGSUIT,
        StateBitFlag::SB_IN_TRANSITION_TO_WINGSUIT,
    ]) {
        return Some(TierState::Wingsuit);
    }
    match signals.detection.traversal {
        Some(Traversal::PreReel | Traversal::ReelingIn) => return Some(TierState::GrappleReel),
        Some(Traversal::PreHang | Traversal::ReeledIn) => return Some(TierState::GrappleHang),
        None => {}
    }
    if any(&[
        StateBitFlag::SB_IS_RELOADING,
        StateBitFlag::SB_IS_DRAWING_WEAPON,
        StateBitFlag::SB_IS_HOLSTERING_WEAPON,
    ]) {
        return Some(TierState::WeaponAction);
    }
    Some(match signals.detection.mode {
        Mode::Airborne(AirborneVariant::Parachute) => TierState::Parachute,
        Mode::Airborne(AirborneVariant::Wingsuit) => TierState::Wingsuit,
        Mode::OnFoot(OnFootVariant::Swimming) => TierState::Swimming,
        _ => TierState::OnFoot,
    })
}

/// Move a hand's controller weight toward `target` at the rate the blend times set: toward zero
/// over `yield_s`, toward one over `return_s`. A non-positive time switches at once.
pub fn step_weight(weight: f32, target: f32, dt: f32, yield_s: f32, return_s: f32) -> f32 {
    let duration = if target < weight { yield_s } else { return_s };
    if duration <= 0.0 {
        return target;
    }
    let step = dt.max(0.0) / duration;
    if target < weight {
        (weight - step).max(target)
    } else {
        (weight + step).min(target)
    }
}

/// The last tick's classification, for the debug UI and the overlay's ghost hands.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TierStatus {
    /// The classified state, `None` before the first readable tick.
    pub state: Option<TierState>,
    /// Each hand's assigned tier, indexed by [`Hand::index`].
    pub tiers: [HandTier; 2],
    /// Each hand's effective tier ([`HandTier::effective_in`]).
    pub effective: [HandTier; 2],
    /// Each hand's controller weight: one on the controller, zero on the animation. A yielded
    /// hand below one is where the overlay draws the ghost of the controller-driven hand.
    pub weights: [f32; 2],
    /// Each hand's share of the anchored pose: one while anchored, blending over
    /// [`AnchorConfig::blend_s`](crate::controllers::hands::anchor::AnchorConfig::blend_s).
    pub anchor_weights: [f32; 2],
}

impl TierStatus {
    const INITIAL: TierStatus = TierStatus {
        state: None,
        tiers: [HandTier::Free; 2],
        effective: [HandTier::Free; 2],
        weights: [1.0; 2],
        anchor_weights: [0.0; 2],
    };
}

/// The last tick's classification.
pub fn status() -> TierStatus {
    *STATUS.lock()
}

/// Each hand's controller weight, indexed by [`Hand::index`].
pub fn weights() -> [f32; 2] {
    STATUS.lock().weights
}

/// Classify this input tick and advance the blends. Called from the input tick after the mode
/// detector, while the controllers are enabled.
pub(crate) fn tick(dt: f32) {
    let (hands, dominant, mounted_anchor) = Config::lock_query(|c| {
        (
            c.controllers.hands,
            c.controllers.dominant_hand,
            c.controllers.vehicles.mounted.anchor_hands,
        )
    });
    // SAFETY: the input tick runs on the main thread outside the world update, as the mode
    // detector's own reads require.
    let signals = unsafe { read_signals() };
    // The weapon handling's last tick: one tick behind, which the anchor's blend absorbs.
    let weapons = weapons::status();
    let gun_hands = weapons
        .wielded
        .filter(|_| weapons.active)
        .map_or([false; 2], |w| w.weapon.wielded.hands);
    let mut status = STATUS.lock();
    *status = advance(
        *status,
        classify(&signals),
        dt,
        &hands,
        dominant,
        mounted_anchor,
        gun_hands,
    );
}

/// Fold one tick into the status: a new state (or the held one) assigns each hand its tier, and
/// the weights step toward the tiers' targets. `mounted_anchor` is the mounted guns' own anchor
/// switch; the grapple states follow [`HandsConfig::anchor`]. `gun_hands` are the hands with a gun
/// of the wielded weapon, indexed by [`Hand::index`]: a grapple hand holding a gun stays free
/// rather than taking the line, so that the gun keeps aiming where the controller points.
pub fn advance(
    previous: TierStatus,
    state: Option<TierState>,
    dt: f32,
    config: &HandsConfig,
    dominant: Hand,
    mounted_anchor: bool,
    gun_hands: [bool; 2],
) -> TierStatus {
    let state = state.or(previous.state);
    let tiers = Hand::BOTH.map(|hand| {
        state.map_or(HandTier::Free, |s| {
            config.tiers.hand_tier(s, hand, dominant)
        })
    });
    let anchor = match state {
        Some(TierState::MountedGun) => mounted_anchor,
        _ => config.anchor.enabled,
    };
    let grapple = state.is_some_and(|s| s.scope() == TierScope::GrappleHand);
    let effective = Hand::BOTH.map(
        |hand| match tiers[hand.index()].effective_in(state, anchor) {
            HandTier::Anchored if grapple && gun_hands[hand.index()] => HandTier::Free,
            tier => tier,
        },
    );
    let weights = Hand::BOTH.map(|hand| {
        step_weight(
            previous.weights[hand.index()],
            effective[hand.index()].controller_weight(),
            dt,
            config.yield_blend_s,
            config.return_blend_s,
        )
    });
    let anchor_weights = Hand::BOTH.map(|hand| {
        let target = if effective[hand.index()] == HandTier::Anchored {
            1.0
        } else {
            0.0
        };
        step_weight(
            previous.anchor_weights[hand.index()],
            target,
            dt,
            config.anchor.blend_s,
            config.anchor.blend_s,
        )
    });
    TierStatus {
        state,
        tiers,
        effective,
        weights,
        anchor_weights,
    }
}

static STATUS: Mutex<TierStatus> = Mutex::new(TierStatus::INITIAL);

/// Read the signals: the mode detector's detection and snapshot from this tick, plus the
/// state-machine predicates, each only while the character is readable.
///
/// # Safety
///
/// Must run on the main thread outside the world update, like [`mode::read_snapshot`].
unsafe fn read_signals() -> TierSignals {
    let detection = mode::current();
    let character = mode::last_snapshot().and_then(|s| s.character);
    let mut signals = TierSignals {
        detection,
        character,
        ..TierSignals::default()
    };
    let Some(snapshot) = character else {
        return signals;
    };
    let Some(local) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        signals.character = None;
        return signals;
    };
    unsafe {
        signals.riding_vehicle = snapshot.vehicle.is_some() && local.IsInRidingInVehicleState();
        signals.stunt_traversing = local.IsStuntTraversing();
        signals.mounted_gun = local.IsInMountedGunState();
        signals.cinematic_camera =
            GameCameraManager::get().is_some_and(|camera| camera.IsInCinematicCamera());
    }
    signals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::mode::{VehicleClass, VehicleSeat};

    fn bits(flags: &[StateBitFlag]) -> [u64; 3] {
        let mut words = [0u64; 3];
        for &f in flags {
            let bit = f as u32 as usize;
            words[bit / 64] |= 1 << (bit % 64);
        }
        words
    }

    fn signals(mode: Mode, flags: &[StateBitFlag]) -> TierSignals {
        TierSignals {
            detection: Detection {
                mode,
                traversal: None,
                held: None,
            },
            character: Some(CharacterSnapshot {
                state_bits: bits(flags),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

    #[test]
    fn plain_on_foot_is_free() {
        let state = classify(&signals(ON_FOOT, &[])).unwrap();
        assert_eq!(state, TierState::OnFoot);
        assert_eq!(HandTierTable::PLAN.tier(state), HandTier::Free);
    }

    #[test]
    fn the_research_signals_yield() {
        for (flags, expected) in [
            (&[StateBitFlag::SB_IN_RAGDOLL][..], TierState::Ragdoll),
            (&[StateBitFlag::SB_IS_IN_DEAD_STATE], TierState::Ragdoll),
            (&[StateBitFlag::SB_IN_USE_ITEM], TierState::Interaction),
            (
                &[StateBitFlag::SB_IS_PLANTING_EXPLOSIVES],
                TierState::Interaction,
            ),
            (
                &[StateBitFlag::SB_IN_TRANSITION_TO_WINGSUIT],
                TierState::Wingsuit,
            ),
            (&[StateBitFlag::SB_IN_MELEE], TierState::Melee),
        ] {
            let state = classify(&signals(ON_FOOT, flags)).unwrap();
            assert_eq!(state, expected, "{flags:?}");
            assert_eq!(
                HandTierTable::PLAN
                    .tier(state)
                    .effective_in(Some(state), true),
                HandTier::Yielded
            );
        }
        let swimming = classify(&signals(Mode::OnFoot(OnFootVariant::Swimming), &[])).unwrap();
        assert_eq!(swimming, TierState::Swimming);
    }

    #[test]
    fn vehicle_entry_yields_and_riding_is_free() {
        let seat = VehicleSeat {
            class: VehicleClass::Car,
            driver: true,
        };
        let mut s = signals(Mode::Vehicle(seat), &[]);
        s.character.as_mut().unwrap().vehicle = Some(seat);
        assert_eq!(classify(&s), Some(TierState::VehicleTransition));
        s.riding_vehicle = true;
        assert_eq!(classify(&s), Some(TierState::VehicleSeated));
        assert_eq!(
            HandTierTable::PLAN.tier(TierState::VehicleSeated),
            HandTier::Free
        );
    }

    #[test]
    fn a_gunner_seat_is_a_mounted_gun_not_an_entry() {
        let seat = VehicleSeat {
            class: VehicleClass::Car,
            driver: false,
        };
        let mut s = signals(Mode::Vehicle(seat), &[]);
        s.character.as_mut().unwrap().vehicle = Some(seat);
        s.mounted_gun = true;
        assert_eq!(classify(&s), Some(TierState::MountedGun));
        let config = HandsConfig::new();
        let status = advance(
            TierStatus::INITIAL,
            Some(TierState::MountedGun),
            1.0,
            &config,
            Hand::Right,
            false,
            [false; 2],
        );
        assert_eq!(status.effective, [HandTier::Yielded; 2]);
    }

    #[test]
    fn a_cutscene_outranks_everything() {
        let mut s = signals(ON_FOOT, &[StateBitFlag::SB_IN_RAGDOLL]);
        s.cinematic_camera = true;
        assert_eq!(classify(&s), Some(TierState::Cutscene));
    }

    #[test]
    fn a_menu_or_an_unreadable_character_holds() {
        assert_eq!(classify(&signals(Mode::Ui, &[])), None);
        let mut s = signals(ON_FOOT, &[]);
        s.character = None;
        assert_eq!(classify(&s), None);
    }

    #[test]
    fn grapple_states_apply_to_the_grapple_hand_only() {
        let mut s = signals(ON_FOOT, &[]);
        s.detection.traversal = Some(Traversal::ReelingIn);
        let state = classify(&s).unwrap();
        assert_eq!(state, TierState::GrappleReel);
        let table = HandTierTable::PLAN;
        assert_eq!(
            table.hand_tier(state, Hand::Left, Hand::Right),
            HandTier::Anchored
        );
        assert_eq!(
            table.hand_tier(state, Hand::Right, Hand::Right),
            HandTier::Free
        );
        assert_eq!(
            table.hand_tier(state, Hand::Right, Hand::Left),
            HandTier::Anchored
        );
    }

    #[test]
    fn the_anchored_tier_is_built_for_the_grapple_states_and_mounted_guns() {
        let anchored = HandTier::Anchored;
        for state in [
            TierState::GrappleReel,
            TierState::GrappleHang,
            TierState::MountedGun,
        ] {
            assert_eq!(anchored.effective_in(Some(state), true), HandTier::Anchored);
            assert_eq!(anchored.effective_in(Some(state), false), HandTier::Yielded);
        }
        assert_eq!(
            anchored.effective_in(Some(TierState::VehicleSeated), true),
            HandTier::Yielded
        );
        assert_eq!(anchored.effective_in(None, true), HandTier::Yielded);
        assert_eq!(HandTier::Anchored.controller_weight(), 1.0);
        assert_eq!(HandTier::Yielded.controller_weight(), 0.0);
        assert_eq!(HandTier::Free.controller_weight(), 1.0);
    }

    #[test]
    fn a_hang_anchors_the_grapple_hand_and_leaves_the_other_free() {
        let config = HandsConfig::new();
        let status = advance(
            TierStatus::INITIAL,
            Some(TierState::GrappleHang),
            1.0,
            &config,
            Hand::Right,
            true,
            [false; 2],
        );
        assert_eq!(status.effective, [HandTier::Anchored, HandTier::Free]);
        assert_eq!(status.weights, [1.0; 2]);
        assert_eq!(status.anchor_weights, [1.0, 0.0]);
        let released = advance(
            status,
            Some(TierState::OnFoot),
            1.0,
            &config,
            Hand::Right,
            true,
            [false; 2],
        );
        assert_eq!(released.anchor_weights, [0.0; 2]);
    }

    #[test]
    fn a_grapple_hand_holding_a_gun_stays_free() {
        let config = HandsConfig::new();
        for state in [TierState::GrappleReel, TierState::GrappleHang] {
            // The left hand is the grapple hand; it holds the second pistol, or a single gun.
            let status = advance(
                TierStatus::INITIAL,
                Some(state),
                1.0,
                &config,
                Hand::Right,
                true,
                [true, true],
            );
            assert_eq!(status.effective, [HandTier::Free; 2], "{state:?}");
            assert_eq!(status.anchor_weights, [0.0; 2], "{state:?}");
        }
        // A mounted gun's hands anchor to its handles whatever they hold.
        let mounted = advance(
            TierStatus::INITIAL,
            Some(TierState::MountedGun),
            1.0,
            &config,
            Hand::Right,
            true,
            [true, false],
        );
        assert_eq!(mounted.effective, [HandTier::Anchored; 2]);
    }

    #[test]
    fn weapon_actions_run_with_the_hands_on_the_controllers() {
        for flag in [
            StateBitFlag::SB_IS_RELOADING,
            StateBitFlag::SB_IS_DRAWING_WEAPON,
            StateBitFlag::SB_IS_HOLSTERING_WEAPON,
        ] {
            let state = classify(&signals(ON_FOOT, &[flag])).unwrap();
            assert_eq!(state, TierState::WeaponAction, "{flag:?}");
            let tier = HandTierTable::PLAN
                .tier(state)
                .effective_in(Some(state), true);
            assert_eq!(tier, HandTier::ActionRuns);
            assert_eq!(tier.controller_weight(), 1.0);
        }
        // A reload never outranks a state that takes the hands.
        let s = signals(
            ON_FOOT,
            &[StateBitFlag::SB_IS_RELOADING, StateBitFlag::SB_IN_RAGDOLL],
        );
        assert_eq!(classify(&s), Some(TierState::Ragdoll));
        let mut s = signals(ON_FOOT, &[StateBitFlag::SB_IS_RELOADING]);
        s.detection.traversal = Some(Traversal::ReelingIn);
        assert_eq!(classify(&s), Some(TierState::GrappleReel));
    }

    #[test]
    fn the_weight_blends_out_and_back_at_the_configured_rates() {
        let w = step_weight(1.0, 0.0, 0.1, 0.2, 0.4);
        assert!((w - 0.5).abs() < 1e-6);
        let w = step_weight(w, 0.0, 0.2, 0.2, 0.4);
        assert_eq!(w, 0.0);
        let w = step_weight(0.0, 1.0, 0.1, 0.2, 0.4);
        assert!((w - 0.25).abs() < 1e-6);
        assert_eq!(step_weight(0.3, 1.0, 0.1, 0.2, 0.0), 1.0);
    }

    #[test]
    fn a_yielding_state_blends_both_hands_and_a_held_state_persists() {
        let config = HandsConfig::new();
        let status = advance(
            TierStatus::INITIAL,
            Some(TierState::Swimming),
            config.yield_blend_s,
            &config,
            Hand::Right,
            true,
            [false; 2],
        );
        assert_eq!(status.tiers, [HandTier::Yielded; 2]);
        assert_eq!(status.weights, [0.0; 2]);
        let held = advance(status, None, 0.01, &config, Hand::Right, true, [false; 2]);
        assert_eq!(held.state, Some(TierState::Swimming));
        assert_eq!(held.weights, [0.0; 2]);
    }

    #[test]
    fn every_state_has_its_own_row() {
        let mut table = HandTierTable::PLAN;
        for state in TierState::ALL {
            *table.tier_mut(state) = HandTier::ActionRuns;
            assert_eq!(table.tier(state), HandTier::ActionRuns);
        }
        assert!(
            TierState::ALL
                .iter()
                .all(|&s| table.tier(s) == HandTier::ActionRuns)
        );
    }
}
