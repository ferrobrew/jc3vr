//! The per-tick mode detector: which of the game's control modes the player is in, and so which
//! OpenXR action set and which bindings apply.
//!
//! It runs once per engine input tick, from the `InputDeviceManager::Update` detour
//! ([`crate::hooks::input::look`]), in two halves:
//!
//! - [`read_snapshot`], the imperative shell: one read of the local character's state words, the
//!   vehicle predicates only while a vehicle state is indicated, the grapple traversal bits, and the
//!   pause and UI signals, into a [`ModeSnapshot`];
//! - [`ModeDetector::observe`], the functional core: the ordered rules of the plan of record
//!   (`docs/mod/input/controllers-and-roomscale.md`, "Mode detection") over the snapshot, with the
//!   debounce and the hold cases.
//!
//! The rules, first match wins:
//!
//! 1. A menu is up: [`Mode::Ui`]. The signals are the clock's pause (which covers the pause menu,
//!    the commlink, the pause utility, and the other pause reasons), the player action map's UI
//!    blocker (a game screen owns input, which covers the map without a pause), and the mod's own
//!    panel.
//! 2. A teleport is in flight, or the character is not readable (no character, no control
//!    parameters, or no animation instance in the first frames after a spawn): hold the current
//!    mode.
//! 3. Seated in a vehicle, including the entry and exit animations: [`Mode::Vehicle`]. The attach
//!    state is a state-machine predicate, so it outranks the raw state bits an ejection or an exit
//!    passes through.
//! 4. Grapple traversal: the gameplay mode (on foot or airborne) holds, and the traversal is flagged.
//! 5. to 7. The wingsuit, the parachute, and falling: [`Mode::Airborne`].
//! 8. and 9. Swimming and everything else: [`Mode::OnFoot`].
//!
//! A change between gameplay modes must persist for [`ModeConfig::debounce_ticks`] ticks before it
//! takes effect, so a one-tick flicker during an animation blend does not switch the action set.
//! Entering and leaving the UI is immediate: the UI signals do not flicker, and a late switch would
//! send a menu press to the gameplay bindings.

use jc3gi::{
    character::character::Character,
    clock::Clock,
    game_world::GameWorld,
    input::input_action_map::{InputSystemBlocker, LocalPlayerActionMap},
    state::StateBitFlag,
    vehicle::vehicle::Seat,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{config::ModeConfig, frame::ActionSetKind},
    hooks::animation::active_state_type_id,
};

/// The control mode the bindings act in.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Mode {
    /// On foot, including swimming.
    OnFoot(OnFootVariant),
    /// Seated in a vehicle.
    Vehicle(VehicleSeat),
    /// Falling, the wingsuit, or the parachute.
    Airborne(AirborneVariant),
    /// A game menu (or the mod's panel) is up.
    Ui,
}

impl Mode {
    /// The OpenXR action set this mode selects.
    pub const fn action_set(self) -> ActionSetKind {
        match self {
            Mode::OnFoot(_) => ActionSetKind::OnFoot,
            Mode::Vehicle(_) => ActionSetKind::Vehicle,
            Mode::Airborne(_) => ActionSetKind::Airborne,
            Mode::Ui => ActionSetKind::Ui,
        }
    }

    /// Whether this is one of the two gameplay modes grapple traversal holds.
    const fn is_traversal_mode(self) -> bool {
        matches!(self, Mode::OnFoot(_) | Mode::Airborne(_))
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::OnFoot(OnFootVariant::Ground)
    }
}

/// The on-foot variants.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OnFootVariant {
    Ground,
    Swimming,
}

/// The airborne variants.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AirborneVariant {
    /// Falling, free-falling, or fast-falling.
    Falling,
    Wingsuit,
    /// The parachute, including hanging from a vehicle-attached parachute.
    Parachute,
}

/// The seat the player occupies.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VehicleSeat {
    pub class: VehicleClass,
    /// Whether the player is in the driver's seat, which selects the driver bindings.
    pub driver: bool,
}

/// The vehicle classes the bindings distinguish, resolved in the order listed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VehicleClass {
    Helicopter,
    Plane,
    Motorcycle,
    Car,
    /// Boats and jetskis, which share one class until the jetski discriminator is pinned.
    Sea,
    /// The attach state is up but the vehicle is not (yet) resolvable, or is of no class above
    /// (a mech). Bound like a car.
    Unknown,
}

/// A grapple traversal sub-state, flagged for the hand tiers. The gameplay mode holds through it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Traversal {
    /// The reel-in wind-up.
    PreReel,
    /// Reeling in.
    ReelingIn,
    /// The wind-up to a hang.
    PreHang,
    /// Reeled onto an attach point with a live wire.
    ReeledIn,
}

/// What the detector concluded this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Detection {
    pub mode: Mode,
    pub traversal: Option<Traversal>,
    /// Why the mode was held rather than re-evaluated, if it was.
    pub held: Option<HoldReason>,
}

/// Why the detector held the previous mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HoldReason {
    /// A teleport is in flight.
    Teleporting,
    /// The character, its control parameters, or its animation instance is not readable yet.
    CharacterUnavailable,
    /// A new mode is waiting out the debounce.
    Debouncing,
}

/// The engine state the rules read, gathered once per tick by [`read_snapshot`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ModeSnapshot {
    pub ui: UiSignals,
    /// A deferred teleport is being serviced.
    pub teleporting: bool,
    /// `None` when the local character is not readable (rule 2).
    pub character: Option<CharacterSnapshot>,
}

/// The menu signals of rule 1.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct UiSignals {
    /// The game clock is stopped: some pause reason (the pause menu, the commlink, the pause
    /// utility, a message box, ...) holds.
    pub clock_stopped: bool,
    /// A game UI screen has input priority (the player action map's UI blocker).
    pub screen_focus: bool,
    /// The mod's own panel is up.
    pub mod_panel: bool,
}

impl UiSignals {
    pub const fn any(self) -> bool {
        self.clock_stopped || self.screen_focus || self.mod_panel
    }
}

/// The local character's state for the gameplay rules.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CharacterSnapshot {
    /// The first three state-bitflag words, indexed by [`StateBitFlag`] value.
    pub state_bits: [u64; 3],
    /// `Some` while the character is in a vehicle attach state (seated, or entering or leaving).
    pub vehicle: Option<VehicleSeat>,
    /// Whether the grappling hook has a live wire.
    pub grapple_wire: bool,
    /// Whether the character is reeled in or hangs from the grapple.
    pub reeled_in: bool,
}

impl CharacterSnapshot {
    /// Whether `flag` is set in the state words.
    pub fn has(&self, flag: StateBitFlag) -> bool {
        let bit = flag as u32 as usize;
        self.state_bits
            .get(bit / 64)
            .is_some_and(|word| (word >> (bit % 64)) & 1 != 0)
    }
}

/// The debounced mode state machine.
#[derive(Debug, Clone, Default)]
pub struct ModeDetector {
    current: Detection,
    /// A gameplay mode that differs from the current one, and for how many consecutive ticks it
    /// has been classified.
    candidate: Option<(Mode, u32)>,
}

impl ModeDetector {
    pub const fn new() -> Self {
        Self {
            current: Detection {
                mode: Mode::OnFoot(OnFootVariant::Ground),
                traversal: None,
                held: None,
            },
            candidate: None,
        }
    }

    /// The last detection.
    pub fn current(&self) -> Detection {
        self.current
    }

    /// Fold one tick's snapshot in and return the detection.
    pub fn observe(&mut self, snapshot: &ModeSnapshot, config: &ModeConfig) -> Detection {
        if snapshot.ui.any() {
            self.candidate = None;
            self.current = Detection {
                mode: Mode::Ui,
                traversal: None,
                held: None,
            };
            return self.current;
        }
        // Leaving the UI is immediate too: the mode under the menu is re-classified from scratch.
        let leaving_ui = self.current.mode == Mode::Ui;

        let hold = |reason| Detection {
            held: Some(reason),
            ..self.current
        };
        if snapshot.teleporting {
            self.current = hold(HoldReason::Teleporting);
            return self.current;
        }
        let Some(character) = snapshot.character else {
            self.current = hold(HoldReason::CharacterUnavailable);
            return self.current;
        };

        let traversal = traversal(&character);
        let classified = match (traversal, self.current.mode) {
            (Some(_), previous) if previous.is_traversal_mode() && character.vehicle.is_none() => {
                previous
            }
            _ => classify(&character),
        };

        let mode = if leaving_ui || classified == self.current.mode {
            self.candidate = None;
            classified
        } else {
            let seen = match self.candidate {
                Some((mode, ticks)) if mode == classified => ticks + 1,
                _ => 1,
            };
            if seen >= config.debounce_ticks.max(1) {
                self.candidate = None;
                classified
            } else {
                self.candidate = Some((classified, seen));
                self.current = Detection {
                    mode: self.current.mode,
                    traversal,
                    held: Some(HoldReason::Debouncing),
                };
                return self.current;
            }
        };
        self.current = Detection {
            mode,
            traversal,
            held: None,
        };
        self.current
    }
}

/// Rules 3 and 5 to 9 over a readable character.
pub fn classify(character: &CharacterSnapshot) -> Mode {
    if let Some(seat) = character.vehicle {
        return Mode::Vehicle(seat);
    }
    if character.has(StateBitFlag::SB_IS_WINGSUIT) {
        return Mode::Airborne(AirborneVariant::Wingsuit);
    }
    if character.has(StateBitFlag::SB_IS_PARACHUTING)
        || character.has(StateBitFlag::SB_IN_ATTACHED_TO_PARACHUTE)
    {
        return Mode::Airborne(AirborneVariant::Parachute);
    }
    if character.has(StateBitFlag::SB_IS_FALLING)
        || character.has(StateBitFlag::SB_IS_FREE_FALLING)
        || character.has(StateBitFlag::SB_IS_FASTFALLING)
    {
        return Mode::Airborne(AirborneVariant::Falling);
    }
    if character.has(StateBitFlag::SB_IS_SWIMMING) || character.has(StateBitFlag::SB_IS_UW_SWIMMING)
    {
        return Mode::OnFoot(OnFootVariant::Swimming);
    }
    Mode::OnFoot(OnFootVariant::Ground)
}

/// Rule 4's traversal sub-state. The character bits come first: the hook's own state is sticky
/// after a reel ends, so the reeled-in state also needs a live wire.
pub fn traversal(character: &CharacterSnapshot) -> Option<Traversal> {
    if character.has(StateBitFlag::SB_IN_PRE_REEL) {
        Some(Traversal::PreReel)
    } else if character.has(StateBitFlag::SB_IN_REELING_IN) {
        Some(Traversal::ReelingIn)
    } else if character.has(StateBitFlag::SB_IN_PRE_HANG) {
        Some(Traversal::PreHang)
    } else if character.reeled_in && character.grapple_wire {
        Some(Traversal::ReeledIn)
    } else {
        None
    }
}

/// Run the detector for this input tick: read the engine, fold the snapshot in, select the action
/// set, and publish the detection for [`current`]. Called from the input tick while the controllers
/// are enabled.
pub(crate) fn tick() -> Detection {
    let config = Config::lock_query(|c| c.controllers.mode);
    // SAFETY: called on the main thread from the input tick, outside the world update that writes
    // the state read here.
    let snapshot = unsafe { read_snapshot() };
    let mut detector = DETECTOR.lock();
    let before = detector.current().mode;
    let detection = detector.observe(&snapshot, &config);
    drop(detector);
    if before != detection.mode {
        tracing::debug!(target: "controllers", ?before, after = ?detection.mode, "mode change");
    }
    *LAST_SNAPSHOT.lock() = Some(snapshot);
    crate::vr::input::set_active_action_set(detection.mode.action_set());
    detection
}

/// The last detection, for the debug UI and the other subsystems.
pub fn current() -> Detection {
    DETECTOR.lock().current()
}

/// The last snapshot the detector read, for the debug UI.
pub fn last_snapshot() -> Option<ModeSnapshot> {
    *LAST_SNAPSHOT.lock()
}

/// Read the engine state the rules need.
///
/// # Safety
///
/// Must run on the main thread outside the world update (the input tick): the vehicle predicates
/// walk the animation rule system and the interaction graph, which the world update mutates.
pub unsafe fn read_snapshot() -> ModeSnapshot {
    let ui = UiSignals {
        clock_stopped: unsafe { Clock::get() }.is_some_and(|clock| clock.m_Stop),
        screen_focus: unsafe { LocalPlayerActionMap::get() }.is_some_and(|map| {
            map.as_input_action_map().m_Disabled & (1 << InputSystemBlocker::Ui as u32) != 0
        }),
        mod_panel: Config::lock_query(|c| c.hud.egui_panel.enabled),
    };
    let teleporting =
        unsafe { GameWorld::get() }.is_some_and(|world| unsafe { world.IsTeleporting() });
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .and_then(|c| unsafe { read_character(c) });
    ModeSnapshot {
        ui,
        teleporting,
        character,
    }
}

/// The detector state, advanced by [`tick`].
static DETECTOR: Mutex<ModeDetector> = Mutex::new(ModeDetector::new());

static LAST_SNAPSHOT: Mutex<Option<ModeSnapshot>> = Mutex::new(None);

/// The character half of [`read_snapshot`], or `None` while any pointer the rules need is null.
unsafe fn read_character(character: &Character) -> Option<CharacterSnapshot> {
    let control = unsafe { character.m_ControlParameters.as_ref() }?;
    // The state-machine predicates chase the body rule system's current state with no null checks.
    let rule_system_ready = unsafe { active_state_type_id(character) }.is_some();
    if character.m_AnimatedModel.m_InstanceData.is_null() || !rule_system_ready {
        return None;
    }
    let vm = &control.m_UserVM;
    let state_bits = [
        vm.m_CurrentStateBitFlags0,
        vm.m_CurrentStateBitFlags1,
        vm.m_CurrentStateBitFlags2,
    ];
    let vehicle =
        unsafe { character.IsInVehicleAttachState() }.then(|| unsafe { read_vehicle(character) });
    let hook = unsafe { character.m_Inventory.m_GrapplingHook.px.as_ref() };
    let grapple_wire = hook.is_some_and(|h| !h.m_ActiveWire.px.is_null());
    let reeled_in = hook.is_some()
        && (unsafe { character.IsReeledIn() } || unsafe { character.IsGrappleHanging() });
    Some(CharacterSnapshot {
        state_bits,
        vehicle,
        grapple_wire,
        reeled_in,
    })
}

/// Resolve the vehicle class and seat. The class predicates each walk the interaction graph, so
/// they run only while the attach state holds, in the resolution order.
unsafe fn read_vehicle(character: &Character) -> VehicleSeat {
    let class = unsafe {
        if character.IsAttachedToHelicopter() {
            VehicleClass::Helicopter
        } else if character.IsAttachedToAirVehicle() {
            VehicleClass::Plane
        } else if character.IsAttachedToLandVehicle() {
            if character.IsRidingMC() {
                VehicleClass::Motorcycle
            } else {
                VehicleClass::Car
            }
        } else if character.IsAttachedToSeaVehicle() {
            VehicleClass::Sea
        } else {
            VehicleClass::Unknown
        }
    };
    let driver = unsafe { character.GetVehiclePtr().as_ref() }
        .is_some_and(|vehicle| vehicle.m_LocalPlayerSeat == Seat::EDriverSeat);
    VehicleSeat { class, driver }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ModeConfig = ModeConfig { debounce_ticks: 2 };

    fn bits(flags: &[StateBitFlag]) -> [u64; 3] {
        let mut words = [0u64; 3];
        for &f in flags {
            let bit = f as u32 as usize;
            words[bit / 64] |= 1 << (bit % 64);
        }
        words
    }

    fn with_flags(flags: &[StateBitFlag]) -> ModeSnapshot {
        ModeSnapshot {
            character: Some(CharacterSnapshot {
                state_bits: bits(flags),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn in_vehicle(class: VehicleClass, driver: bool) -> ModeSnapshot {
        ModeSnapshot {
            character: Some(CharacterSnapshot {
                vehicle: Some(VehicleSeat { class, driver }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Observe `snapshot` until the debounce lets it through.
    fn settle(detector: &mut ModeDetector, snapshot: &ModeSnapshot) -> Detection {
        for _ in 0..CONFIG.debounce_ticks {
            detector.observe(snapshot, &CONFIG);
        }
        detector.current()
    }

    #[test]
    fn flags_read_across_words() {
        let c = CharacterSnapshot {
            state_bits: bits(&[StateBitFlag::SB_IS_SWIMMING, StateBitFlag::SB_IS_WINGSUIT]),
            ..Default::default()
        };
        assert!(c.has(StateBitFlag::SB_IS_SWIMMING));
        assert!(c.has(StateBitFlag::SB_IS_WINGSUIT));
        assert!(!c.has(StateBitFlag::SB_IS_PARACHUTING));
    }

    #[test]
    fn rules_follow_the_plan_order() {
        let classify_flags = |flags: &[StateBitFlag]| {
            classify(&CharacterSnapshot {
                state_bits: bits(flags),
                ..Default::default()
            })
        };
        assert_eq!(classify_flags(&[]), Mode::OnFoot(OnFootVariant::Ground));
        assert_eq!(
            classify_flags(&[StateBitFlag::SB_IS_UW_SWIMMING]),
            Mode::OnFoot(OnFootVariant::Swimming)
        );
        assert_eq!(
            classify_flags(&[StateBitFlag::SB_IS_FREE_FALLING]),
            Mode::Airborne(AirborneVariant::Falling)
        );
        // The parachute outranks the fall bits it overlaps with, and the wingsuit outranks both.
        assert_eq!(
            classify_flags(&[StateBitFlag::SB_IS_FALLING, StateBitFlag::SB_IS_PARACHUTING]),
            Mode::Airborne(AirborneVariant::Parachute)
        );
        assert_eq!(
            classify_flags(&[
                StateBitFlag::SB_IS_WINGSUIT,
                StateBitFlag::SB_IS_PARACHUTING
            ]),
            Mode::Airborne(AirborneVariant::Wingsuit)
        );
        assert_eq!(
            classify_flags(&[StateBitFlag::SB_IN_ATTACHED_TO_PARACHUTE]),
            Mode::Airborne(AirborneVariant::Parachute)
        );
        // A vehicle attach state outranks every bit (ejecting passes through it).
        let seated = CharacterSnapshot {
            state_bits: bits(&[StateBitFlag::SB_IS_FALLING]),
            vehicle: Some(VehicleSeat {
                class: VehicleClass::Plane,
                driver: true,
            }),
            ..Default::default()
        };
        assert!(matches!(classify(&seated), Mode::Vehicle(_)));
    }

    #[test]
    fn ui_is_immediate_both_ways() {
        let mut d = ModeDetector::new();
        settle(&mut d, &in_vehicle(VehicleClass::Car, true));
        let mut paused = in_vehicle(VehicleClass::Car, true);
        paused.ui.clock_stopped = true;
        assert_eq!(d.observe(&paused, &CONFIG).mode, Mode::Ui);
        let resumed = in_vehicle(VehicleClass::Car, true);
        assert!(matches!(
            d.observe(&resumed, &CONFIG).mode,
            Mode::Vehicle(_)
        ));
    }

    #[test]
    fn the_ui_signals_each_select_the_ui() {
        for ui in [
            UiSignals {
                clock_stopped: true,
                ..Default::default()
            },
            UiSignals {
                screen_focus: true,
                ..Default::default()
            },
            UiSignals {
                mod_panel: true,
                ..Default::default()
            },
        ] {
            let mut d = ModeDetector::new();
            let snapshot = ModeSnapshot {
                ui,
                ..Default::default()
            };
            assert_eq!(d.observe(&snapshot, &CONFIG).mode, Mode::Ui);
        }
    }

    #[test]
    fn a_one_tick_flicker_does_not_switch() {
        let mut d = ModeDetector::new();
        let ground = with_flags(&[]);
        let falling = with_flags(&[StateBitFlag::SB_IS_FALLING]);
        settle(&mut d, &ground);
        let flicker = d.observe(&falling, &CONFIG);
        assert_eq!(flicker.mode, Mode::OnFoot(OnFootVariant::Ground));
        assert_eq!(flicker.held, Some(HoldReason::Debouncing));
        assert_eq!(
            d.observe(&ground, &CONFIG).mode,
            Mode::OnFoot(OnFootVariant::Ground)
        );
        // The interrupted candidate starts over.
        d.observe(&falling, &CONFIG);
        assert_eq!(
            d.observe(&falling, &CONFIG).mode,
            Mode::Airborne(AirborneVariant::Falling)
        );
    }

    #[test]
    fn a_zero_debounce_switches_at_once() {
        let mut d = ModeDetector::new();
        let config = ModeConfig { debounce_ticks: 0 };
        let falling = with_flags(&[StateBitFlag::SB_IS_FALLING]);
        assert_eq!(
            d.observe(&falling, &config).mode,
            Mode::Airborne(AirborneVariant::Falling)
        );
    }

    #[test]
    fn spawn_and_teleport_hold_the_mode() {
        let mut d = ModeDetector::new();
        settle(&mut d, &in_vehicle(VehicleClass::Helicopter, true));
        let spawn = ModeSnapshot::default();
        let held = d.observe(&spawn, &CONFIG);
        assert!(matches!(held.mode, Mode::Vehicle(_)));
        assert_eq!(held.held, Some(HoldReason::CharacterUnavailable));

        let mut teleport = with_flags(&[]);
        teleport.teleporting = true;
        let held = d.observe(&teleport, &CONFIG);
        assert!(matches!(held.mode, Mode::Vehicle(_)));
        assert_eq!(held.held, Some(HoldReason::Teleporting));
    }

    #[test]
    fn traversal_holds_the_gameplay_mode() {
        let mut d = ModeDetector::new();
        let falling = with_flags(&[StateBitFlag::SB_IS_FREE_FALLING]);
        settle(&mut d, &falling);
        // Reeling in from the air, the character loses the fall bits; the set stays airborne.
        let reeling = with_flags(&[StateBitFlag::SB_IN_REELING_IN]);
        let detection = settle(&mut d, &reeling);
        assert_eq!(detection.mode, Mode::Airborne(AirborneVariant::Falling));
        assert_eq!(detection.traversal, Some(Traversal::ReelingIn));

        // From the ground, the same traversal stays on foot even though it sets the fall bits.
        let mut d = ModeDetector::new();
        settle(&mut d, &with_flags(&[]));
        let pre = with_flags(&[StateBitFlag::SB_IN_PRE_REEL, StateBitFlag::SB_IS_FALLING]);
        let detection = settle(&mut d, &pre);
        assert_eq!(detection.mode, Mode::OnFoot(OnFootVariant::Ground));
        assert_eq!(detection.traversal, Some(Traversal::PreReel));
    }

    #[test]
    fn a_reeled_state_needs_a_live_wire() {
        let mut c = CharacterSnapshot {
            reeled_in: true,
            ..Default::default()
        };
        assert_eq!(traversal(&c), None);
        c.grapple_wire = true;
        assert_eq!(traversal(&c), Some(Traversal::ReeledIn));
    }

    #[test]
    fn modes_select_their_action_sets() {
        assert_eq!(
            Mode::OnFoot(OnFootVariant::Swimming).action_set(),
            ActionSetKind::OnFoot
        );
        assert_eq!(
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Sea,
                driver: false
            })
            .action_set(),
            ActionSetKind::Vehicle
        );
        assert_eq!(
            Mode::Airborne(AirborneVariant::Parachute).action_set(),
            ActionSetKind::Airborne
        );
        assert_eq!(Mode::Ui.action_set(), ActionSetKind::Ui);
    }
}
