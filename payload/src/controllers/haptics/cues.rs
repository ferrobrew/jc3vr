//! Every haptic source the mod has, and the cue each one plays: a primitive of the vocabulary, a
//! strength within it, and the category whose switch gates it. `docs/mod/input/haptics.md` keeps the
//! same table for the headset.

use serde::{Deserialize, Serialize};

use crate::controllers::haptics::vocabulary::Primitive;

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
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Length {
    Normal,
    /// Several times the primitive's duration, for the one cue that marks an event far bigger than
    /// the hand: a detonation.
    Long,
}

impl Length {
    pub const fn factor(self) -> f32 {
        match self {
            Length::Normal => 1.0,
            Length::Long => 3.5,
        }
    }
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

impl Cue {
    const fn new(primitive: Primitive, strength: f32, category: Category) -> Self {
        Self {
            primitive,
            strength,
            category,
            length: Length::Normal,
        }
    }

    const fn long(self) -> Self {
        Self {
            length: Length::Long,
            ..self
        }
    }
}

/// A one-off haptic source.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
    /// The magazine or the rounds ejected.
    Eject,
    /// A round or a magazine taken from the pouch.
    PouchTake,
    /// A grab at an empty ammunition pouch.
    PouchEmpty,
    /// A round or a magazine caught by the well.
    WellCatch,
    /// A magazine seated.
    MagazineSeat,
    /// A belt seated.
    BeltSeat,
    /// A full pull of the action.
    Rack,
    /// The action's part meeting a stop of its travel: home, a bolt turned all the way up or down,
    /// or a slide caught at the rear.
    ActionDetent,
    /// A round chambered.
    Chambered,
    /// A grenade primed in the hand.
    GrenadePrime,
    /// A cooking grenade's fuse warning.
    GrenadeWarning,
    /// A live grenade caught.
    GrenadeCatch,
    /// A grab at a pouch that the game refused: no grenade or charge to give.
    PouchRefused,
    /// A C4 charge primed in the hand.
    C4Prime,
    /// A C4 charge thrown.
    C4Throw,
    /// A C4 charge pressed onto a surface and stuck there.
    C4Stick,
    /// The placed charges detonating, on both hands.
    C4Detonate,
    /// The wrist detonator pressed with charges placed, on both hands.
    DetonatorPress,
    /// The wrist detonator pressed with nothing placed.
    DetonatorEmpty,
    /// The pressing fingertip coming within reach of the wrist detonator.
    DetonatorNear,
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
    /// The grapple hand's grip arming a snatch on a target out of reach.
    SnatchArm,
    /// A snatch refused or failed.
    SnatchRefused,
    /// The tethers starting to retract under the grapple hand's pull.
    TetherRetract,
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
    // Interface.
    /// A controller chord the mod acts on: the recenter or the debug panel.
    Chord,
}

impl Event {
    pub const ALL: [Event; 52] = [
        Event::PalmTouch,
        Event::HandStrike,
        Event::Take,
        Event::LetGo,
        Event::Unlatch,
        Event::Shot,
        Event::Kick,
        Event::DryFire,
        Event::VolumeEnter,
        Event::VolumeGrab,
        Event::Eject,
        Event::PouchTake,
        Event::PouchEmpty,
        Event::WellCatch,
        Event::MagazineSeat,
        Event::BeltSeat,
        Event::Rack,
        Event::ActionDetent,
        Event::Chambered,
        Event::GrenadePrime,
        Event::GrenadeWarning,
        Event::GrenadeCatch,
        Event::PouchRefused,
        Event::C4Prime,
        Event::C4Throw,
        Event::C4Stick,
        Event::C4Detonate,
        Event::DetonatorPress,
        Event::DetonatorEmpty,
        Event::DetonatorNear,
        Event::WheelTick,
        Event::WheelCommit,
        Event::SteeringStop,
        Event::SteeringCentre,
        Event::ControlStop,
        Event::ControlNeutral,
        Event::ThrottleGate,
        Event::ButtonPress,
        Event::ButtonRelease,
        Event::HandleGrab,
        Event::MountedKick,
        Event::GrappleFire,
        Event::GrappleAttach,
        Event::Yank,
        Event::SnatchArm,
        Event::SnatchRefused,
        Event::TetherRetract,
        Event::ClimbGrip,
        Event::ClimbRelease,
        Event::ClimbFling,
        Event::StuntEnter,
        Event::StuntRelease,
    ];

    pub const fn cue(self) -> Cue {
        match self {
            Event::PalmTouch | Event::HandStrike => {
                Cue::new(Primitive::Bump, 0.4, Category::Contact)
            }
            Event::Take => Cue::new(Primitive::Bump, 0.6, Category::Contact),
            Event::LetGo => Cue::new(Primitive::Click, 0.25, Category::Contact),
            Event::Unlatch => Cue::new(Primitive::Bump, 0.9, Category::Contact),
            Event::Shot => Cue::new(Primitive::Thud, 1.0, Category::Weapons),
            Event::Kick => Cue::new(Primitive::Bump, 1.0, Category::Weapons),
            Event::DryFire => Cue::new(Primitive::Click, 0.4, Category::Weapons),
            Event::VolumeEnter => Cue::new(Primitive::Tick, 0.3, Category::Weapons),
            Event::VolumeGrab => Cue::new(Primitive::Click, 0.5, Category::Weapons),
            Event::Eject => Cue::new(Primitive::Click, 0.6, Category::Weapons),
            Event::PouchTake => Cue::new(Primitive::Click, 0.25, Category::Weapons),
            Event::PouchEmpty => Cue::new(Primitive::Tick, 0.75, Category::Weapons),
            Event::WellCatch => Cue::new(Primitive::Click, 0.4, Category::Weapons),
            Event::MagazineSeat => Cue::new(Primitive::Bump, 0.9, Category::Weapons),
            Event::BeltSeat => Cue::new(Primitive::Thud, 0.75, Category::Weapons),
            Event::Rack => Cue::new(Primitive::Click, 0.9, Category::Weapons),
            Event::ActionDetent => Cue::new(Primitive::Click, 0.35, Category::Weapons),
            Event::Chambered => Cue::new(Primitive::Click, 0.5, Category::Weapons),
            Event::GrenadePrime => Cue::new(Primitive::Click, 0.4, Category::Weapons),
            Event::GrenadeWarning => Cue::new(Primitive::Click, 0.75, Category::Weapons),
            Event::GrenadeCatch => Cue::new(Primitive::Bump, 0.75, Category::Weapons),
            Event::PouchRefused => Cue::new(Primitive::Buzz, 0.75, Category::Weapons),
            Event::C4Prime => Cue::new(Primitive::Click, 0.4, Category::Weapons),
            Event::C4Throw => Cue::new(Primitive::Click, 0.6, Category::Weapons),
            Event::C4Stick => Cue::new(Primitive::Bump, 0.6, Category::Weapons),
            Event::C4Detonate => Cue::new(Primitive::Thud, 1.0, Category::Weapons).long(),
            Event::DetonatorPress => Cue::new(Primitive::Click, 1.0, Category::Weapons),
            Event::DetonatorEmpty => Cue::new(Primitive::Buzz, 0.1, Category::Weapons),
            Event::DetonatorNear => Cue::new(Primitive::Tick, 0.4, Category::Weapons),
            Event::WheelTick => Cue::new(Primitive::Tick, 0.75, Category::Weapons),
            Event::WheelCommit => Cue::new(Primitive::Click, 0.75, Category::Weapons),
            Event::SteeringStop | Event::ControlStop => {
                Cue::new(Primitive::Bump, 0.6, Category::Vehicles)
            }
            Event::SteeringCentre | Event::ControlNeutral | Event::ButtonRelease => {
                Cue::new(Primitive::Tick, 0.5, Category::Vehicles)
            }
            Event::ThrottleGate => Cue::new(Primitive::Bump, 0.9, Category::Vehicles),
            Event::ButtonPress => Cue::new(Primitive::Click, 0.75, Category::Vehicles),
            Event::HandleGrab => Cue::new(Primitive::Click, 0.5, Category::Vehicles),
            Event::MountedKick => Cue::new(Primitive::Bump, 0.6, Category::Vehicles),
            Event::GrappleFire => Cue::new(Primitive::Bump, 0.4, Category::Traversal),
            Event::GrappleAttach => Cue::new(Primitive::Thud, 0.5, Category::Traversal),
            Event::Yank => Cue::new(Primitive::Bump, 0.6, Category::Traversal),
            Event::SnatchArm => Cue::new(Primitive::Tick, 0.5, Category::Traversal),
            Event::SnatchRefused => Cue::new(Primitive::Buzz, 0.6, Category::Traversal),
            Event::TetherRetract => Cue::new(Primitive::Bump, 0.75, Category::Traversal),
            Event::ClimbGrip => Cue::new(Primitive::Bump, 0.75, Category::Traversal),
            Event::ClimbRelease => Cue::new(Primitive::Click, 0.25, Category::Traversal),
            Event::ClimbFling => Cue::new(Primitive::Thud, 0.85, Category::Traversal),
            Event::StuntEnter => Cue::new(Primitive::Thud, 0.65, Category::Traversal),
            Event::StuntRelease => Cue::new(Primitive::Click, 0.5, Category::Traversal),
            Event::Chord => Cue::new(Primitive::Bump, 0.6, Category::Interface),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Event::PalmTouch => "palm touch",
            Event::HandStrike => "hand strike",
            Event::Take => "take hold",
            Event::LetGo => "let go",
            Event::Unlatch => "unlatch",
            Event::Shot => "shot",
            Event::Kick => "recoil kick",
            Event::DryFire => "dry fire",
            Event::VolumeEnter => "holster or pouch entered",
            Event::VolumeGrab => "draw, holster, or foregrip",
            Event::Eject => "eject",
            Event::PouchTake => "pouch take",
            Event::PouchEmpty => "pouch empty",
            Event::WellCatch => "well catch",
            Event::MagazineSeat => "magazine seat",
            Event::BeltSeat => "belt seat",
            Event::Rack => "rack",
            Event::ActionDetent => "action detent",
            Event::Chambered => "chambered",
            Event::GrenadePrime => "grenade prime",
            Event::GrenadeWarning => "grenade warning",
            Event::GrenadeCatch => "grenade catch",
            Event::PouchRefused => "pouch refused",
            Event::C4Prime => "C4 prime",
            Event::C4Throw => "C4 throw",
            Event::C4Stick => "C4 stick",
            Event::C4Detonate => "C4 detonate",
            Event::DetonatorPress => "detonator press",
            Event::DetonatorEmpty => "detonator empty",
            Event::DetonatorNear => "detonator near",
            Event::WheelTick => "weapon wheel tick",
            Event::WheelCommit => "weapon wheel commit",
            Event::SteeringStop => "steering stop",
            Event::SteeringCentre => "steering centre",
            Event::ControlStop => "control stop",
            Event::ControlNeutral => "control neutral",
            Event::ThrottleGate => "throttle gate",
            Event::ButtonPress => "button press",
            Event::ButtonRelease => "button release",
            Event::HandleGrab => "handle grab",
            Event::MountedKick => "mounted kick",
            Event::GrappleFire => "grapple fire",
            Event::GrappleAttach => "grapple attach",
            Event::Yank => "yank",
            Event::SnatchArm => "snatch armed",
            Event::SnatchRefused => "snatch refused",
            Event::TetherRetract => "tether retract",
            Event::ClimbGrip => "climb grip",
            Event::ClimbRelease => "climb release",
            Event::ClimbFling => "climb fling",
            Event::StuntEnter => "stunt enter",
            Event::StuntRelease => "stunt release",
            Event::Chord => "chord",
        }
    }
}

/// A sustained haptic source: one that sends its level every tick it lasts.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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

    pub const fn cue(self) -> Cue {
        match self {
            Sustain::PalmPressure | Sustain::HeldLoad | Sustain::BodyPressure => {
                Cue::new(Primitive::Tension, 0.5, Category::Contact)
            }
            Sustain::ClimbStrain => Cue::new(Primitive::Tension, 0.5, Category::Traversal),
            Sustain::StuntHang => Cue::new(Primitive::Tension, 0.35, Category::Traversal),
            Sustain::GrappleTension => Cue::new(Primitive::Tension, 0.75, Category::Traversal),
            Sustain::GrappleReel => Cue::new(Primitive::Hum, 0.35, Category::Traversal),
            Sustain::GameRumble => Cue::new(Primitive::Hum, 1.0, Category::Game),
        }
    }

    /// Whether a one-off cue's onset leaves this source out entirely, rather than mixing a share of
    /// it in underneath: the game's rumble, so the mod's own cues stand clear of it.
    pub const fn ducks_fully(self) -> bool {
        matches!(self, Sustain::GameRumble)
    }

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

    #[test]
    fn every_event_plays_once_and_every_sustain_follows_a_level() {
        for event in Event::ALL {
            let cue = event.cue();
            assert!(!cue.primitive.is_sustained(), "{}", event.label());
            assert!((0.0..=1.0).contains(&cue.strength), "{}", event.label());
            assert_ne!(cue.category, Category::Game, "{}", event.label());
        }
        for sustain in Sustain::ALL {
            let cue = sustain.cue();
            assert!(cue.primitive.is_sustained(), "{}", sustain.label());
            assert!((0.0..=1.0).contains(&cue.strength), "{}", sustain.label());
            assert_eq!(cue.length, Length::Normal);
        }
    }

    #[test]
    fn the_lists_hold_every_source_once() {
        assert_eq!(
            Event::ALL.iter().collect::<HashSet<_>>().len(),
            Event::ALL.len()
        );
        assert_eq!(
            Sustain::ALL.iter().collect::<HashSet<_>>().len(),
            Sustain::ALL.len()
        );
        let labels: HashSet<_> = Event::ALL.iter().map(|e| e.label()).collect();
        assert_eq!(labels.len(), Event::ALL.len());
    }

    #[test]
    fn only_the_game_rumble_is_the_game_category() {
        for sustain in Sustain::ALL {
            assert_eq!(
                sustain.cue().category == Category::Game,
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
