//! The weapon wheel's state machine: when the wheel opens and closes, which arm the steering hand
//! chooses, and which slot-select action to hold for it. Pure; the shell
//! ([`crate::controllers::weapons::wheel`]) feeds it the tick's inputs and the game's state and
//! carries out what it decides.
//!
//! The game shows its selection cross while any slot-select action is held, highlights the held
//! one's arm, and commits the highlighted slot when every slot-select action is released, unless it
//! is the wielded weapon's slot. So the wheel holds exactly one slot-select action from the open to
//! the close, switching which one in a single tick (a tick with none held would commit):
//!
//! - in the centre it holds the resting slot ([`WheelInput::resting`]), the wielded weapon's where
//!   it is on the cross, which the game reads as no change;
//! - toward an arm whose slot is offered it holds that arm's action; an arm whose slot is not
//!   offered is drawn by nothing on the cross, so it counts as the centre;
//! - a trackpad press that latched to a quadrant starts on that quadrant's arm, until the hand
//!   first leaves the centre, so a tap still selects its quadrant as the game's d-pad does.
//!
//! The close commits the chosen arm when it is not the wielded slot, and otherwise cancels. A
//! cancel is reported for the shell to clear the game's pending selection, since a release in the
//! centre with nothing wielded would otherwise draw the resting slot.

use glam::{Quat, Vec3};
use jc3gi::{character::inventory::WeaponSlot, input::input_action_map::Action};

use crate::controllers::{
    bindings::{controls::ButtonControl, tables::sink::PadZone},
    frame::{Hand, Pose},
    weapons::wheel::{
        config::WheelConfig,
        steer::{Aimed, SteerOrigin, aim},
    },
};

/// An arm of the game's selection cross, in the layout it draws under gamepad semantics.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WheelSlot {
    /// Up: the dual-wielded pistols.
    DualWield,
    /// Right: the two-handed weapon.
    TwoHanded,
    /// Left: the special weapon.
    Special,
    /// Down: the planted explosive.
    Explosives,
}

impl WheelSlot {
    pub const ALL: [WheelSlot; 4] = [
        WheelSlot::DualWield,
        WheelSlot::TwoHanded,
        WheelSlot::Special,
        WheelSlot::Explosives,
    ];

    /// The slot-select action that highlights the arm.
    pub const fn action(self) -> Action {
        match self {
            WheelSlot::DualWield => Action::SELECT_DUEL_WIELD,
            WheelSlot::TwoHanded => Action::SELECT_TWO_HANDED,
            WheelSlot::Special => Action::SELECT_TWO_HANDED_SPECIAL,
            WheelSlot::Explosives => Action::SELECT_EXPLOSIVES,
        }
    }

    /// The arm a slot-select action highlights, for the four slot-select actions.
    pub fn of_action(action: Action) -> Option<Self> {
        WheelSlot::ALL.into_iter().find(|s| s.action() == action)
    }

    /// The inventory slot the arm shows.
    pub const fn inventory_slot(self) -> WeaponSlot {
        match self {
            WheelSlot::DualWield => WeaponSlot::E_SLOT_DUAL_WIELD,
            WheelSlot::TwoHanded => WeaponSlot::E_SLOT_TWO_HANDED,
            WheelSlot::Special => WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
            WheelSlot::Explosives => WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE,
        }
    }

    /// The arm that shows an inventory slot, for the four slots on the cross.
    pub fn of_inventory(slot: WeaponSlot) -> Option<Self> {
        WheelSlot::ALL
            .into_iter()
            .find(|s| s.inventory_slot() == slot)
    }

    /// The arm in a trackpad quadrant, as on the gamepad's d-pad.
    pub const fn of_zone(zone: PadZone) -> Self {
        match zone {
            PadZone::Up => WheelSlot::DualWield,
            PadZone::Right => WheelSlot::TwoHanded,
            PadZone::Left => WheelSlot::Special,
            PadZone::Down => WheelSlot::Explosives,
        }
    }

    /// The arm's direction, degrees clockwise from up.
    pub const fn centre_deg(self) -> f32 {
        match self {
            WheelSlot::DualWield => 0.0,
            WheelSlot::TwoHanded => 90.0,
            WheelSlot::Explosives => 180.0,
            WheelSlot::Special => 270.0,
        }
    }

    /// The arm's index into a by-arm table such as [`WheelInput::offered`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// What opens the wheel.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Opener {
    /// The weapon-select trackpad press.
    Trackpad,
    /// The `weapon_wheel` action pushed past its open threshold.
    Stick,
}

/// One hand's opening inputs over the tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct WheelPress {
    /// The weapon-select trackpad press.
    pub pad: ButtonControl,
    /// The quadrant the trackpad press latched to, while it is held.
    pub zone: Option<PadZone>,
    /// The `weapon_wheel` action's value.
    pub stick: f32,
}

/// One hand's pose, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandPose {
    pub aim: Pose,
    pub grip: Vec3,
}

/// Everything one tick of the wheel reads.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WheelInput {
    /// Whether the wheel may act: it is on, the mode holds hand weapons, and the player character
    /// exists. Off, an open wheel cancels.
    pub acting: bool,
    /// Each hand's opening inputs, by [`Hand::index`].
    pub presses: [WheelPress; 2],
    /// The head's orientation in the cockpit frame, for the steering frame.
    pub head: Quat,
    /// Each hand's pose, by [`Hand::index`]; `None` while untracked.
    pub hands: [Option<HandPose>; 2],
    /// Whether the game reads the slot-select inputs.
    pub gate: bool,
    /// Whether the game offers each arm's slot, by [`WheelSlot::index`].
    pub offered: [bool; 4],
    /// The wielded weapon's slot, if a weapon is wielded.
    pub wielded: Option<WeaponSlot>,
    /// The slot held in the centre: see [`resting`].
    pub resting: Option<WheelSlot>,
}

/// How an open wheel closed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Close {
    /// The release chose a slot other than the wielded one: the game switches to it.
    Commit(WheelSlot),
    /// Nothing is to change: a release in the centre or on the wielded slot, the game's input
    /// closing, or the wheel no longer acting.
    Cancel,
}

/// What one tick of the wheel decided.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct WheelOutput {
    /// The slot-select action to hold this tick, as its arm.
    pub hold: Option<WheelSlot>,
    /// The wheel opened this tick, by this opener on this hand.
    pub opened: Option<(Opener, Hand)>,
    /// The chosen arm changed this tick (to another arm or to the centre).
    pub changed: bool,
    /// The wheel closed this tick.
    pub closed: Option<Close>,
    /// The hand the open wheel is steered by.
    pub steering: Option<Hand>,
    /// The open wheel was opened by the stick, whose push the body turn must not read.
    pub by_stick: bool,
    /// The chosen arm, or `None` in the centre.
    pub chosen: Option<WheelSlot>,
}

/// The wheel and its state across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Wheel {
    phase: Phase,
}

impl Wheel {
    pub const fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    /// Whether the wheel is open.
    pub fn is_open(&self) -> bool {
        matches!(self.phase, Phase::Open(_))
    }

    /// Step the wheel by one tick.
    pub fn step(&mut self, input: &WheelInput, config: &WheelConfig) -> WheelOutput {
        if !input.acting {
            let closed = self.is_open().then_some(Close::Cancel);
            self.phase = Phase::Idle;
            return WheelOutput {
                closed,
                ..Default::default()
            };
        }
        match self.phase {
            Phase::Idle => self.open(input, config),
            Phase::Refused(opener, hand) => {
                if !held(opener, &input.presses[hand.index()], config, false) {
                    self.phase = Phase::Idle;
                }
                WheelOutput::default()
            }
            Phase::Open(session) => self.continue_open(session, input, config),
        }
    }

    /// Close the wheel as a cancel, as when the wheel stops acting. Returns whether it was open.
    pub fn cancel(&mut self) -> bool {
        let open = self.is_open();
        self.phase = Phase::Idle;
        open
    }

    fn open(&mut self, input: &WheelInput, config: &WheelConfig) -> WheelOutput {
        let Some((opener, hand)) = find_opener(input, config) else {
            return WheelOutput::default();
        };
        if !input.gate {
            self.phase = Phase::Refused(opener, hand);
            return WheelOutput::default();
        }
        let press = input.presses[hand.index()];
        let mut session = Session {
            opener,
            hand,
            origin: input.hands[hand.index()]
                .and_then(|pose| SteerOrigin::new(input.head, pose.aim, pose.grip)),
            aimed: Aimed::Centre,
            seed: seed(opener, press),
            left_centre: false,
            chosen: None,
        };
        session.chosen = chosen(&session, &input.offered);
        self.phase = Phase::Open(session);
        WheelOutput {
            hold: session.chosen.or(input.resting),
            opened: Some((opener, hand)),
            steering: Some(hand),
            by_stick: opener == Opener::Stick,
            chosen: session.chosen,
            ..Default::default()
        }
    }

    fn continue_open(
        &mut self,
        mut session: Session,
        input: &WheelInput,
        config: &WheelConfig,
    ) -> WheelOutput {
        let hand = session.hand;
        let press = input.presses[hand.index()];
        if !held(session.opener, &press, config, true) {
            self.phase = Phase::Idle;
            let close = match session.chosen {
                Some(slot) if input.wielded != Some(slot.inventory_slot()) => Close::Commit(slot),
                _ => Close::Cancel,
            };
            return WheelOutput {
                closed: Some(close),
                ..Default::default()
            };
        }
        if !input.gate {
            self.phase = Phase::Refused(session.opener, hand);
            return WheelOutput {
                closed: Some(Close::Cancel),
                ..Default::default()
            };
        }
        if let Some(pose) = input.hands[hand.index()] {
            match session.origin {
                Some(origin) => {
                    let (leave, back) = config.deadzone();
                    let deflection = origin.deflection(config.steer, pose.aim, pose.grip);
                    session.aimed = aim(
                        session.aimed,
                        deflection,
                        leave,
                        back,
                        config.hysteresis_deg,
                    );
                }
                // A hand untracked at the open steers from where it is first seen.
                None => session.origin = SteerOrigin::new(input.head, pose.aim, pose.grip),
            }
        }
        if session.aimed != Aimed::Centre {
            session.left_centre = true;
        }
        if !session.left_centre && session.opener == Opener::Trackpad {
            // A press on the line between two quadrants latches once the thumb moves off it.
            session.seed = seed(session.opener, press).or(session.seed);
        }
        let previous = session.chosen;
        session.chosen = chosen(&session, &input.offered);
        self.phase = Phase::Open(session);
        WheelOutput {
            hold: session.chosen.or(input.resting),
            changed: session.chosen != previous,
            steering: Some(hand),
            by_stick: session.opener == Opener::Stick,
            chosen: session.chosen,
            ..Default::default()
        }
    }
}

/// The slot to hold in the centre: the wielded weapon's arm where the game offers it, which the
/// game records as no change; with nothing on the cross wielded, the selected slot's arm (the one
/// the game would draw), or else the first offered arm. `None` with no arm offered.
pub fn resting(
    wielded: Option<WeaponSlot>,
    selected: Option<WeaponSlot>,
    offered: &[bool; 4],
) -> Option<WheelSlot> {
    let offered_arm = |slot: Option<WeaponSlot>| {
        slot.and_then(WheelSlot::of_inventory)
            .filter(|arm| offered[arm.index()])
    };
    offered_arm(wielded)
        .or_else(|| offered_arm(selected))
        .or_else(|| WheelSlot::ALL.into_iter().find(|arm| offered[arm.index()]))
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
enum Phase {
    #[default]
    Idle,
    /// An opener was pressed while the game did not read the slot-select inputs, or the input
    /// closed while the wheel was open: nothing opens until it is released.
    Refused(Opener, Hand),
    Open(Session),
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Session {
    opener: Opener,
    hand: Hand,
    origin: Option<SteerOrigin>,
    aimed: Aimed,
    /// The arm a trackpad press's quadrant starts on, until the hand first leaves the centre.
    seed: Option<WheelSlot>,
    left_centre: bool,
    chosen: Option<WheelSlot>,
}

/// The first opener held, the trackpad before the stick and the left hand before the right.
fn find_opener(input: &WheelInput, config: &WheelConfig) -> Option<(Opener, Hand)> {
    [Opener::Trackpad, Opener::Stick]
        .into_iter()
        .flat_map(|opener| Hand::BOTH.map(|hand| (opener, hand)))
        .find(|&(opener, hand)| held(opener, &input.presses[hand.index()], config, false))
}

/// Whether `opener` is held on a hand whose inputs are `press`. An open stick stays held down to
/// its release threshold.
fn held(opener: Opener, press: &WheelPress, config: &WheelConfig, open: bool) -> bool {
    match opener {
        Opener::Trackpad => config.trackpad && press.pad.held(),
        Opener::Stick => {
            let threshold = if open {
                config.stick_release.min(config.stick_open)
            } else {
                config.stick_open
            };
            config.stick && press.stick.is_finite() && press.stick >= threshold.max(0.05)
        }
    }
}

/// The arm a press starts on: its trackpad quadrant's.
fn seed(opener: Opener, press: WheelPress) -> Option<WheelSlot> {
    match opener {
        Opener::Trackpad => press.zone.map(WheelSlot::of_zone),
        Opener::Stick => None,
    }
}

/// The chosen arm: the aimed arm when the game offers it, the centre for an arm it does not, and
/// in the centre the seed until the hand has left it.
fn chosen(session: &Session, offered: &[bool; 4]) -> Option<WheelSlot> {
    let arm = match session.aimed {
        Aimed::Arm(arm) => Some(arm),
        Aimed::Centre if !session.left_centre => session.seed,
        Aimed::Centre => None,
    };
    arm.filter(|arm| offered[arm.index()])
}

#[cfg(test)]
mod tests;
