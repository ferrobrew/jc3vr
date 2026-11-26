//! Every haptic source the mod has, and the cue each one plays: a primitive of the vocabulary, a
//! strength within it, and the category whose switch gates it. The category is fixed here; the
//! primitive and the strength are tuning ([`crate::haptics::tuning::HapticsTuning`]).

use serde::{Deserialize, Serialize};

use crate::haptics::vocabulary::Primitive;

/// The groups of sources a player switches on and off together.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    /// The hands meeting, pressing, holding, and striking the world and the characters in it.
    Contact,
    /// The guns and explosives: shots, holsters and pouches, the foregrip, reloading, grenades, C4,
    /// and the weapon wheel.
    Weapons,
    /// The physical vehicle controls and the mounted guns' handles.
    Vehicles,
    /// Climbing, the stunt positions, and the grapple.
    Traversal,
    /// The mod's own confirmations: the controller chords.
    Interface,
    /// The game's own rumble, forwarded.
    Game,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Contact,
        Category::Weapons,
        Category::Vehicles,
        Category::Traversal,
        Category::Interface,
        Category::Game,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Category::Contact => "contact",
            Category::Weapons => "weapons",
            Category::Vehicles => "vehicles",
            Category::Traversal => "traversal",
            Category::Interface => "interface",
            Category::Game => "game rumble",
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// How long a one-off cue plays against its primitive's duration.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
pub enum Length {
    Normal,
    /// Several times the primitive's duration
    /// ([`HapticsTuning::long_factor`](crate::haptics::tuning::HapticsTuning::long_factor)), for a
    /// cue that marks an event far bigger than the hand: a detonation.
    Long,
}

/// What a source plays.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Cue {
    pub primitive: Primitive,
    /// Where in the primitive's range the cue sits at full scale, `0..=1`.
    pub strength: f32,
    pub category: Category,
    pub length: Length,
}

/// A one-off haptic source.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    // Contact.
    /// A palm meeting a surface, scaled by how fast it met it.
    PalmTouch,
    /// A hand body's contact, or a blow on a character, scaled by the arm's momentum.
    HandStrike,
    /// A hand taking hold of an object, a character, a door, or a weapon off the ground.
    Take,
    /// A hand letting go of what it held.
    LetGo,
    /// A door's latch giving under the hand.
    Unlatch,
    DoorPressureContact,
    // Weapons.
    /// A weapon's fire rumble, on the hand whose gun fired: the game sets the strength and length.
    Shot,
    /// A shot of a weapon with no fire rumble of its own: the recoil kick alone, scaled by it.
    Kick,
    /// A trigger pull of an empty gun that cannot reload.
    DryFire,
    /// A hand entering a holster or a pouch.
    VolumeEnter,
    /// A draw, a holster, or a foregrip grab or release.
    VolumeGrab,
    /// A hand holding a weapon coming into a free socket's attraction: letting go there docks it.
    /// Its cue is firmer than a volume's tick, as it is felt with the hand out of sight behind the
    /// head, and lighter than the dock's click that follows the release.
    DockAttraction,
    /// A weapon let go of over a taken socket on the back swapped for the one docked there, which
    /// falls clear.
    /// Its cue is a knock rather than the dock's click, so a swap is told from a dock by feel.
    SocketSwap,
    /// The magazine or the rounds ejected.
    Eject,
    /// A round or a magazine taken from the pouch.
    PouchTake,
    /// A grab at an empty ammunition pouch.
    PouchEmpty,
    /// A round or a magazine caught by the well.
    WellCatch,
    /// A magazine seated.
    /// The heaviest cue on the gun hand, as the magazine's seat is the knock the gun takes.
    MagazineSeat,
    /// The inserted magazine knocked out of the well by a held one.
    MagazineKnock,
    /// A belt seated.
    BeltSeat,
    /// A full pull of the action.
    Rack,
    /// The action's part meeting a stop of its travel: home, a bolt turned all the way up or down,
    /// or a slide caught at the rear.
    ActionDetent,
    /// A round chambered.
    Chambered,
    /// A seated round leaving the gun ready to fire: a double pulse, told apart from the seat's
    /// thud before it.
    ReadyToFire,
    /// A grenade drawn into the hand, its pin still in.
    GrenadePrime,
    /// A held grenade's pin pulled: its fuse runs from now.
    GrenadePin,
    /// A cooking grenade's fuse warning.
    GrenadeWarning,
    /// A live grenade caught.
    GrenadeCatch,
    /// A grab at a pouch that the game refused: no grenade or charge to give.
    PouchRefused,
    /// A grenade pressed onto a full grenade pouch: a long, full buzz, apart from the brief
    /// refusal, so the grenade staying in the hand (and falling once it opens) is noticed.
    PouchFull,
    /// A held grenade or C4 charge pressed back into its pouch.
    PouchReturn,
    /// A C4 charge primed in the hand.
    C4Prime,
    /// A C4 charge thrown.
    C4Throw,
    /// A C4 charge pressed onto a surface and stuck there.
    C4Stick,
    /// The placed charges detonating, on both hands.
    C4Detonate,
    /// The detonator's trigger pressed with charges placed, on both hands.
    DetonatorPress,
    /// The detonator's trigger pressed with nothing to detonate.
    DetonatorEmpty,
    /// The weapon wheel opening or its choice changing.
    WheelTick,
    /// The weapon wheel's release committing a weapon.
    WheelCommit,
    // Vehicles.
    /// The held steering wheel reaching its lock.
    SteeringStop,
    /// The held steering wheel passing centre.
    SteeringCentre,
    /// A held stick or lever reaching a stop.
    ControlStop,
    /// A held lever passing neutral.
    ControlNeutral,
    /// The held throttle reaching its afterburner gate.
    ThrottleGate,
    /// The held throttle passing a step of its forward section.
    /// Its cue is light enough to count through a push without drowning the stops.
    ThrottleStep,
    /// The held throttle pulled back to zero, stopping at its reverse gate.
    /// The hardest cue on the throttle, so zero is found without looking.
    ThrottleZero,
    /// The held throttle going through its zero gate into reverse or back out.
    /// A buzz rather than a knock, so going into reverse is told from the zero stop.
    ThrottleReverse,
    /// A cockpit button pressed, or a twist grip twisted far enough to act.
    ButtonPress,
    /// A cockpit button or a twist grip let go.
    ButtonRelease,
    /// A mounted gun's handle taken.
    HandleGrab,
    /// A shot of the mounted gun the hands hold.
    MountedKick,
    // Traversal.
    /// The grapple firing.
    GrappleFire,
    /// The grapple's hook taking hold.
    GrappleAttach,
    /// A yank of the grapple hand.
    Yank,
    /// The grapple hand's grip arming a snatch on a target out of reach: the hand has latched on.
    SnatchArm,
    /// The yank that fires an armed snatch, stronger than the arming.
    SnatchFire,
    /// The snatch's hook biting its target, as the line goes taut.
    SnatchHit,
    /// The hand catching what the snatch's line yanked to it: the hardest knock the hand takes.
    SnatchCatch,
    /// A snatch refused or failed.
    SnatchRefused,
    /// The tethers starting to retract under the grapple hand's pull.
    TetherRetract,
    /// The grapple's line locked during the reel, to swing from.
    LineLock,
    /// The locked line let go of: the button released or the character landed.
    LineRelease,
    /// The locked line breaking.
    LineSnap,
    /// A hand gripping the world to climb.
    ClimbGrip,
    /// A climbing hand letting go.
    ClimbRelease,
    /// The body flung off a climbing grip, scaled by the throw.
    ClimbFling,
    /// A hand taking a stunt position.
    StuntEnter,
    /// A hand letting go of a stunt position.
    StuntRelease,
    /// A swimming hand's stroke catching the water, scaled by how hard it pulls.
    SwimStroke,
    // Interface.
    /// A controller chord the mod acts on: the recenter or the debug panel.
    Chord,
}

impl Event {
    /// Every event, in declaration order, so an event's index in it is its discriminant.
    pub const ALL: [Event; 70] = [
        Event::PalmTouch,
        Event::HandStrike,
        Event::Take,
        Event::LetGo,
        Event::Unlatch,
        Event::DoorPressureContact,
        Event::Shot,
        Event::Kick,
        Event::DryFire,
        Event::VolumeEnter,
        Event::VolumeGrab,
        Event::DockAttraction,
        Event::SocketSwap,
        Event::Eject,
        Event::PouchTake,
        Event::PouchEmpty,
        Event::WellCatch,
        Event::MagazineSeat,
        Event::MagazineKnock,
        Event::BeltSeat,
        Event::Rack,
        Event::ActionDetent,
        Event::Chambered,
        Event::ReadyToFire,
        Event::GrenadePrime,
        Event::GrenadePin,
        Event::GrenadeWarning,
        Event::GrenadeCatch,
        Event::PouchRefused,
        Event::PouchFull,
        Event::PouchReturn,
        Event::C4Prime,
        Event::C4Throw,
        Event::C4Stick,
        Event::C4Detonate,
        Event::DetonatorPress,
        Event::DetonatorEmpty,
        Event::WheelTick,
        Event::WheelCommit,
        Event::SteeringStop,
        Event::SteeringCentre,
        Event::ControlStop,
        Event::ControlNeutral,
        Event::ThrottleGate,
        Event::ThrottleStep,
        Event::ThrottleZero,
        Event::ThrottleReverse,
        Event::ButtonPress,
        Event::ButtonRelease,
        Event::HandleGrab,
        Event::MountedKick,
        Event::GrappleFire,
        Event::GrappleAttach,
        Event::Yank,
        Event::SnatchArm,
        Event::SnatchFire,
        Event::SnatchHit,
        Event::SnatchCatch,
        Event::SnatchRefused,
        Event::TetherRetract,
        Event::LineLock,
        Event::LineRelease,
        Event::LineSnap,
        Event::ClimbGrip,
        Event::ClimbRelease,
        Event::ClimbFling,
        Event::StuntEnter,
        Event::StuntRelease,
        Event::SwimStroke,
        Event::Chord,
    ];

    pub const COUNT: usize = Self::ALL.len();

    /// The category whose switch gates the event's cue.
    pub const fn category(self) -> Category {
        match self {
            Event::PalmTouch
            | Event::HandStrike
            | Event::Take
            | Event::LetGo
            | Event::Unlatch
            | Event::DoorPressureContact
            | Event::SnatchCatch => Category::Contact,
            Event::Shot
            | Event::Kick
            | Event::DryFire
            | Event::VolumeEnter
            | Event::VolumeGrab
            | Event::DockAttraction
            | Event::SocketSwap
            | Event::Eject
            | Event::PouchTake
            | Event::PouchEmpty
            | Event::WellCatch
            | Event::MagazineSeat
            | Event::MagazineKnock
            | Event::BeltSeat
            | Event::Rack
            | Event::ActionDetent
            | Event::Chambered
            | Event::ReadyToFire
            | Event::GrenadePrime
            | Event::GrenadePin
            | Event::GrenadeWarning
            | Event::GrenadeCatch
            | Event::PouchRefused
            | Event::PouchFull
            | Event::PouchReturn
            | Event::C4Prime
            | Event::C4Throw
            | Event::C4Stick
            | Event::C4Detonate
            | Event::DetonatorPress
            | Event::DetonatorEmpty
            | Event::WheelTick
            | Event::WheelCommit => Category::Weapons,
            Event::SteeringStop
            | Event::ControlStop
            | Event::SteeringCentre
            | Event::ControlNeutral
            | Event::ButtonRelease
            | Event::ThrottleGate
            | Event::ThrottleStep
            | Event::ThrottleZero
            | Event::ThrottleReverse
            | Event::ButtonPress
            | Event::HandleGrab
            | Event::MountedKick => Category::Vehicles,
            Event::GrappleFire
            | Event::GrappleAttach
            | Event::Yank
            | Event::SnatchArm
            | Event::SnatchFire
            | Event::SnatchHit
            | Event::SnatchRefused
            | Event::TetherRetract
            | Event::LineLock
            | Event::LineRelease
            | Event::LineSnap
            | Event::ClimbGrip
            | Event::ClimbRelease
            | Event::ClimbFling
            | Event::StuntEnter
            | Event::StuntRelease
            | Event::SwimStroke => Category::Traversal,
            Event::Chord => Category::Interface,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Event::PalmTouch => "palm touch",
            Event::HandStrike => "hand strike",
            Event::Take => "take hold",
            Event::LetGo => "let go",
            Event::Unlatch => "unlatch",
            Event::DoorPressureContact => "door pressure contact",
            Event::Shot => "shot",
            Event::Kick => "recoil kick",
            Event::DryFire => "dry fire",
            Event::VolumeEnter => "holster or pouch entered",
            Event::VolumeGrab => "draw, holster, or foregrip",
            Event::DockAttraction => "socket attraction entered",
            Event::SocketSwap => "socket swap",
            Event::Eject => "eject",
            Event::PouchTake => "pouch take",
            Event::PouchEmpty => "pouch empty",
            Event::WellCatch => "well catch",
            Event::MagazineSeat => "magazine seat",
            Event::MagazineKnock => "magazine knock",
            Event::BeltSeat => "belt seat",
            Event::Rack => "rack",
            Event::ActionDetent => "action detent",
            Event::Chambered => "chambered",
            Event::ReadyToFire => "ready to fire",
            Event::GrenadePrime => "grenade prime",
            Event::GrenadePin => "grenade pin",
            Event::GrenadeWarning => "grenade warning",
            Event::GrenadeCatch => "grenade catch",
            Event::PouchRefused => "pouch refused",
            Event::PouchFull => "pouch full",
            Event::PouchReturn => "pouch return",
            Event::C4Prime => "C4 prime",
            Event::C4Throw => "C4 throw",
            Event::C4Stick => "C4 stick",
            Event::C4Detonate => "C4 detonate",
            Event::DetonatorPress => "detonator press",
            Event::DetonatorEmpty => "detonator empty",
            Event::WheelTick => "weapon wheel tick",
            Event::WheelCommit => "weapon wheel commit",
            Event::SteeringStop => "steering stop",
            Event::SteeringCentre => "steering centre",
            Event::ControlStop => "control stop",
            Event::ControlNeutral => "control neutral",
            Event::ThrottleGate => "throttle gate",
            Event::ThrottleStep => "throttle step",
            Event::ThrottleZero => "throttle zero stop",
            Event::ThrottleReverse => "throttle reverse gate",
            Event::ButtonPress => "button press",
            Event::ButtonRelease => "button release",
            Event::HandleGrab => "handle grab",
            Event::MountedKick => "mounted kick",
            Event::GrappleFire => "grapple fire",
            Event::GrappleAttach => "grapple attach",
            Event::Yank => "yank",
            Event::SnatchArm => "snatch armed",
            Event::SnatchFire => "snatch fired",
            Event::SnatchHit => "snatch hook bit",
            Event::SnatchCatch => "snatch caught",
            Event::SnatchRefused => "snatch refused",
            Event::TetherRetract => "tether retract",
            Event::LineLock => "line lock",
            Event::LineRelease => "line release",
            Event::LineSnap => "line snap",
            Event::ClimbGrip => "climb grip",
            Event::ClimbRelease => "climb release",
            Event::ClimbFling => "climb fling",
            Event::StuntEnter => "stunt enter",
            Event::StuntRelease => "stunt release",
            Event::SwimStroke => "swim stroke",
            Event::Chord => "chord",
        }
    }
}

/// A sustained haptic source: one that sends its level every tick it lasts.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sustain {
    /// A palm pressing into an object, by its shove's load.
    PalmPressure,
    /// A hand pulling or twisting what it holds, by the load.
    HeldLoad,
    /// A hand body held back by what it presses, by its lag.
    BodyPressure,
    /// The climbing hands while they move the body, by its speed.
    ClimbStrain,
    /// A hand hanging from a stunt position, by the vehicle's speed.
    StuntHang,
    /// The grapple hand while the tethers are squeezed, by their tension.
    GrappleTension,
    /// The grapple hand while the reel carries the character, by the reel's speed.
    GrappleReel,
    /// The game's own rumble: the rumble manager's per-frame motor strength, by side.
    GameRumble,
}

impl Sustain {
    /// Every source, in declaration order, so a source's index in it is its discriminant.
    pub const ALL: [Sustain; 8] = [
        Sustain::PalmPressure,
        Sustain::HeldLoad,
        Sustain::BodyPressure,
        Sustain::ClimbStrain,
        Sustain::StuntHang,
        Sustain::GrappleTension,
        Sustain::GrappleReel,
        Sustain::GameRumble,
    ];

    pub const COUNT: usize = Self::ALL.len();

    /// The category whose switch gates the source's cue.
    pub const fn category(self) -> Category {
        match self {
            Sustain::PalmPressure | Sustain::HeldLoad | Sustain::BodyPressure => Category::Contact,
            Sustain::ClimbStrain
            | Sustain::StuntHang
            | Sustain::GrappleTension
            | Sustain::GrappleReel => Category::Traversal,
            Sustain::GameRumble => Category::Game,
        }
    }

    /// Whether a one-off cue's onset leaves this source out entirely, rather than mixing a share of
    /// it in underneath: the game's rumble, so the mod's own cues stand clear of it.
    pub const fn ducks_fully(self) -> bool {
        matches!(self, Sustain::GameRumble)
    }

    #[cfg(test)]
    pub const fn label(self) -> &'static str {
        match self {
            Sustain::PalmPressure => "palm pressure",
            Sustain::HeldLoad => "held load",
            Sustain::BodyPressure => "hand body pressure",
            Sustain::ClimbStrain => "climb strain",
            Sustain::StuntHang => "stunt hang",
            Sustain::GrappleTension => "grapple tension",
            Sustain::GrappleReel => "grapple reel",
            Sustain::GameRumble => "game rumble",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::haptics::tuning::tests::shipped;

    #[test]
    fn door_pressure_contact_is_a_subtle_one_shot_distinct_from_unlatch() {
        let contact = shipped().event(Event::DoorPressureContact);
        let unlatch = shipped().event(Event::Unlatch);
        assert!(!contact.primitive.is_sustained());
        assert_eq!(contact.primitive, Primitive::Tick);
        assert_eq!(contact.length, Length::Normal);
        assert!(contact.strength < unlatch.strength);
        assert_ne!(contact.primitive, unlatch.primitive);
    }

    #[test]
    fn no_event_is_the_games_rumble() {
        for event in Event::ALL {
            assert_ne!(event.category(), Category::Game, "{}", event.label());
        }
    }

    #[test]
    fn the_lists_hold_every_source_once_in_declaration_order() {
        for (i, event) in Event::ALL.into_iter().enumerate() {
            assert_eq!(event as usize, i, "{}", event.label());
        }
        for (i, sustain) in Sustain::ALL.into_iter().enumerate() {
            assert_eq!(sustain as usize, i, "{}", sustain.label());
        }
        let labels: HashSet<_> = Event::ALL.iter().map(|e| e.label()).collect();
        assert_eq!(labels.len(), Event::ALL.len());
    }

    #[test]
    fn only_the_game_rumble_is_the_game_category() {
        for sustain in Sustain::ALL {
            assert_eq!(
                sustain.category() == Category::Game,
                sustain == Sustain::GameRumble
            );
            assert_eq!(sustain.ducks_fully(), sustain == Sustain::GameRumble);
        }
    }

    #[test]
    fn category_indices_match_their_order() {
        for (i, category) in Category::ALL.into_iter().enumerate() {
            assert_eq!(category.index(), i);
        }
    }
}
