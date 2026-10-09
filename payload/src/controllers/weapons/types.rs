//! The weapon handling's public readout types: what the tick's two halves hand each other and the
//! bindings, and what the debug UI reads back.

use crate::controllers::{
    frame::Hand,
    grab::{GrabVolume, GripTarget},
    weapons::{
        engine::HeldWeapon,
        foregrip::{GripGeometry, Transition},
        holster::HolsterAction,
        volumes::Stance,
    },
};

/// What the first half of the weapon handling's input tick hands the bindings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickStart {
    /// The weapon handling's grip targets.
    pub targets: Vec<GripTarget>,
    /// The hands holding a pistol of the dual-wield slot while the dual-wield split acts (with the
    /// split on, a pistol in at least one hand, in any mode), indexed by [`Hand::index`].
    pub dual_wield: Option<[bool; 2]>,
    /// The hands holding a gun of the wielded weapon while the hold puts it in either hand,
    /// indexed by [`Hand::index`]: a hand's trigger fires the gun it holds, and only that hand's.
    pub held: Option<[bool; 2]>,
}

/// What a grab did, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum GrabOutcome {
    Holster(HolsterAction),
    /// A holster grab with no local player to act on.
    NoCharacter,
    Foregrip(Transition),
    /// A grab of the held gun's grip by the free hand, which took the gun when `moved`.
    Handover {
        moved: bool,
    },
    /// A foregrip grab that could not engage: the weapon's geometry is not measured yet.
    ForegripUnmeasured,
    /// A grenade-pouch grab: a grenade is asked for.
    Pouch,
    /// A grab of the held grenade's pin.
    Pin,
    /// A grab of a live grenade, to catch it.
    Catch,
    /// A reload-pouch grab: a round is taken for the reload.
    Magazine,
    /// A grab of a gun's slide, charging handle, or bolt.
    Action,
    /// A C4-pouch grab: a charge is asked for.
    C4,
    /// A grab of a weapon without a body in the world (in a character's hands, or on a rack),
    /// which went into the hand when `taken`.
    Take {
        taken: bool,
    },
    /// A grab of a socket for a weapon carried beyond the slots: a carried weapon drawn, or the
    /// held one put there, when `done`.
    Socket {
        done: bool,
    },
}

impl GrabOutcome {
    pub fn label(self) -> String {
        match self {
            GrabOutcome::Holster(action) => action.label(),
            GrabOutcome::NoCharacter => "no character".to_owned(),
            GrabOutcome::Foregrip(Transition::Grabbed) => "foregrip grabbed".to_owned(),
            GrabOutcome::Foregrip(Transition::Released(r)) => {
                format!("foregrip released ({})", r.label())
            }
            GrabOutcome::Handover { moved: true } => "gun handed over".to_owned(),
            GrabOutcome::Handover { moved: false } => "gun not handed over".to_owned(),
            GrabOutcome::ForegripUnmeasured => "foregrip not measured yet".to_owned(),
            GrabOutcome::Pouch => "grenade asked for".to_owned(),
            GrabOutcome::Pin => "grenade pin grabbed".to_owned(),
            GrabOutcome::Catch => "live grenade grabbed".to_owned(),
            GrabOutcome::Magazine => "reload pouch".to_owned(),
            GrabOutcome::Action => "action grabbed".to_owned(),
            GrabOutcome::C4 => "charge asked for".to_owned(),
            GrabOutcome::Take { taken: true } => "weapon taken".to_owned(),
            GrabOutcome::Take { taken: false } => "weapon not taken".to_owned(),
            GrabOutcome::Socket { done: true } => "socket used".to_owned(),
            GrabOutcome::Socket { done: false } => "socket refused".to_owned(),
        }
    }
}

/// One grab, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrabEvent {
    pub hand: Hand,
    pub volume: GrabVolume,
    pub outcome: GrabOutcome,
}

/// The wielded weapon as the weapon handling saw it on the last tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WieldedReadout {
    pub weapon: HeldWeapon,
    /// The two-handed geometry in use, when measured.
    pub geometry: Option<GripGeometry>,
}

/// The weapon handling's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeaponsStatus {
    /// Whether the last input tick ran the weapon handling.
    pub active: bool,
    /// What each hand would take among the weapon handling's targets, indexed by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// How many targets were placed.
    pub candidates: usize,
    /// The stance the body volumes were placed for.
    pub stance: Option<Stance>,
    pub wielded: Option<WieldedReadout>,
    /// Whether the fire inputs were held back for a weapon action on the last tick.
    pub fire_blocked: bool,
    /// The most recent grabs, newest last.
    pub events: Vec<GrabEvent>,
    pub grabs: u64,
    /// How many holsters were moved onto their docked guns on the last tick.
    pub docks_followed: usize,
}
