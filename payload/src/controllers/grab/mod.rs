//! The grip model: what each hand's grip would take, when it takes it, and the markers that show
//! it. The design is in `docs/mod/input/grip.md`.
//!
//! Everything a hand can take is one [`GripTarget`]: what a squeeze takes ([`GrabVolume`], which
//! also names the subsystem that acts on it, [`GrabVolume::consumer`]), its grab point and how near
//! a hand must come, how it ranks ([`Tier`]), and its marker ([`Mark`]). Each subsystem only
//! provides its targets on the input tick and consumes the grabs of the targets it owns:
//!
//! - the weapon handling ([`crate::controllers::weapons`]): the sockets and pouches on the body, the
//!   held gun's parts, the guns in the world without a body (in characters' hands, on racks, in
//!   lockers), the grenade's pin, a live grenade, a magazine in the world, and the carried weapons'
//!   sockets;
//! - the vehicle handling ([`crate::controllers::vehicles`]): the driver's controls, the cockpit's
//!   buttons, the parachute's handles, and the mounted gun's handles;
//! - the physical hands ([`crate::controllers::physical_hands`]): what each hand's reach found in
//!   the world (an object, a weapon, a door, a character), the vehicles' doors near the hands, a
//!   surface to climb, and the helicopter the player hangs from.
//!
//! [`tick`] gathers them, resolves each hand ([`resolve`]): of the targets a hand is within reach
//! of, the one whose grab point is nearest wins, with the fallbacks (a surface to climb, the
//! helicopter to hang from, a reach margin) taken only when nothing else is, and a remote target
//! for the grapple's snatch ([`crate::controllers::snatch`]) only when not even those are. It
//! publishes the
//! result for the readouts and the markers ([`markers`]), and logs each grip press with what the
//! hand was within reach of and why the others lost. The grip takes the chosen target through
//! [`latch::GripLatch`], continuously while it is held, and lets go of what it took when it opens;
//! a held thing over a socket that fits it docks there ([`resolve::dock_target`]).

pub mod latch;
pub mod markers;
pub mod resolve;
pub mod target;
pub(crate) mod tick;

#[cfg(test)]
mod tests;

use glam::Vec3;

pub use crate::controllers::grab::{
    resolve::{Reach, Resolved},
    target::{GripTarget, Mark, MarkAt, MarkStyle, Shown, Tier},
};
use crate::controllers::{
    frame::Hand,
    loadout::Socket,
    vehicles::physical::{ControlKind, buttons::Button},
};

/// What a squeeze takes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GrabVolume {
    /// The held two-handed weapon's foregrip.
    Foregrip,
    /// The held gun's grip, for the hand without the gun, which takes the gun from the other hand
    /// (see [`crate::controllers::weapons::hold`]).
    Handle,
    /// The grenade pouch.
    GrenadePouch,
    /// A visible grenade display at an interaction-backed locker or pickup source.
    GrenadeLocker,
    /// The universal reload pouch on the belt, for the hand without a gun, while the true reload
    /// reloads the held weapon (see [`crate::controllers::weapons::reload`]).
    MagazinePouch,
    /// The held gun's slide, charging handle, or bolt, for the hand without a gun, while its
    /// chamber waits on the action.
    WeaponAction,
    /// The inserted detachable magazine, removed by an offhand grip and outward pull.
    InsertedMagazine,
    /// The held gun's magazine well, while the other hand holds a round for it: the round goes in
    /// as the hand brings it there, so the well is marked but never taken by a squeeze.
    MagazineWell,
    /// The C4 pouch on the chest, while the C4 throw is on.
    C4Pouch,
    /// The surface a held charge is coming to, where it sticks when pressed on: marked, never taken
    /// by a squeeze (see [`crate::controllers::weapons::c4::stick`]).
    ChargeSurface,
    /// A physical vehicle control of the driver's seat (see
    /// [`crate::controllers::vehicles::physical`]).
    VehicleControl(ControlKind),
    /// A cockpit button, pressed by the palm rather than taken by the grip (see
    /// [`crate::controllers::vehicles::physical::buttons`]).
    CockpitButton(Button),
    /// The parachute's handle on that side (see [`crate::controllers::vehicles::parachute`]).
    ParachuteHandle(Hand),
    /// The mounted gun's handle for that hand (see
    /// [`crate::controllers::vehicles::mounted_grip`]).
    MountedHandle(Hand),
    /// What the empty hand's reach found in the world, or a vehicle's door near it (see
    /// [`crate::controllers::physical_hands::pickup`]).
    Pickup(PickupKind),
    /// The static world, or a heavy moving body, the empty hand's body touches, to climb (see
    /// [`crate::controllers::physical_hands::climb`]).
    Climb,
    /// The helicopter the player hangs from, held by either hand's grip (see
    /// [`crate::controllers::physical_hands::stunt`]).
    StuntHold,
    /// A weapon without a body in the world for the free hand to take: a gun in a character's
    /// hands, or a weapon on a rack or in a locker (see [`crate::controllers::weapons::hold::take`]).
    WorldWeapon,
    /// A live grenade in flight or on the ground near a free hand, to catch (see
    /// [`crate::controllers::weapons::grenade::catch`]).
    LiveGrenade,
    /// A magazine lying in the world that fits the held gun, for the hand without a gun while the
    /// gun's magazine is out (see [`crate::controllers::weapons::reload::loose`]).
    WorldMagazine,
    /// A socket on the body: the weapon on it, for an empty hand to draw, or the free socket, for
    /// the hand holding a weapon that fits it to dock it there (see
    /// [`crate::controllers::weapons::docks`]).
    Socket(Socket),
    /// A thing out of reach under the grapple's aim, for the empty grapple hand to snatch (see
    /// [`crate::controllers::snatch`]).
    Snatch(RemoteKind),
}

impl GrabVolume {
    pub const fn label(self) -> &'static str {
        match self {
            GrabVolume::Foregrip => "foregrip",
            GrabVolume::Handle => "gun grip",
            GrabVolume::GrenadePouch => "grenade pouch",
            GrabVolume::GrenadeLocker => "locker grenade",
            GrabVolume::MagazinePouch => "reload pouch",
            GrabVolume::WeaponAction => "weapon action",
            GrabVolume::InsertedMagazine => "inserted magazine",
            GrabVolume::MagazineWell => "magazine well",
            GrabVolume::C4Pouch => "C4 pouch",
            GrabVolume::ChargeSurface => "surface for the charge",
            GrabVolume::VehicleControl(kind) => kind.label(),
            GrabVolume::CockpitButton(button) => button.label(),
            GrabVolume::ParachuteHandle(Hand::Left) => "left parachute handle",
            GrabVolume::ParachuteHandle(Hand::Right) => "right parachute handle",
            GrabVolume::MountedHandle(Hand::Left) => "mounted gun's left handle",
            GrabVolume::MountedHandle(Hand::Right) => "mounted gun's right handle",
            GrabVolume::Pickup(kind) => kind.label(),
            GrabVolume::Climb => "surface to climb",
            GrabVolume::StuntHold => "helicopter to hang from",
            GrabVolume::WorldWeapon => "weapon to take",
            GrabVolume::LiveGrenade => "live grenade",
            GrabVolume::WorldMagazine => "magazine in the world",
            GrabVolume::Socket(socket) => socket_label(socket),
            GrabVolume::Snatch(RemoteKind::Weapon) => "weapon to snatch",
            GrabVolume::Snatch(RemoteKind::Object) => "object to snatch",
            GrabVolume::Snatch(RemoteKind::Door) => "car door to pull open",
        }
    }

    /// The kind of thing this is, which its marker's colour shows.
    pub const fn category(self) -> Category {
        match self {
            GrabVolume::Socket(_)
            | GrabVolume::GrenadePouch
            | GrabVolume::MagazinePouch
            | GrabVolume::C4Pouch => Category::Body,
            GrabVolume::Foregrip
            | GrabVolume::Handle
            | GrabVolume::WeaponAction
            | GrabVolume::InsertedMagazine
            | GrabVolume::MagazineWell
            | GrabVolume::WorldWeapon
            | GrabVolume::Snatch(RemoteKind::Weapon)
            | GrabVolume::Pickup(PickupKind::Weapon) => Category::Weapon,
            GrabVolume::VehicleControl(_)
            | GrabVolume::CockpitButton(_)
            | GrabVolume::ParachuteHandle(_)
            | GrabVolume::MountedHandle(_)
            | GrabVolume::StuntHold
            | GrabVolume::Snatch(RemoteKind::Door)
            | GrabVolume::Pickup(PickupKind::Door) => Category::Vehicle,
            GrabVolume::Pickup(PickupKind::Character) => Category::Character,
            GrabVolume::GrenadeLocker | GrabVolume::LiveGrenade | GrabVolume::ChargeSurface => {
                Category::Explosive
            }
            GrabVolume::Climb => Category::Climb,
            GrabVolume::Pickup(PickupKind::Prop | PickupKind::Hinged)
            | GrabVolume::Snatch(RemoteKind::Object)
            | GrabVolume::WorldMagazine => Category::Prop,
        }
    }

    /// The subsystem that acts on a grab of this.
    pub const fn consumer(self) -> Consumer {
        match self {
            GrabVolume::Foregrip
            | GrabVolume::Handle
            | GrabVolume::GrenadePouch
            | GrabVolume::GrenadeLocker
            | GrabVolume::MagazinePouch
            | GrabVolume::WeaponAction
            | GrabVolume::InsertedMagazine
            | GrabVolume::MagazineWell
            | GrabVolume::C4Pouch
            | GrabVolume::ChargeSurface
            | GrabVolume::WorldWeapon
            | GrabVolume::LiveGrenade
            | GrabVolume::WorldMagazine
            | GrabVolume::Socket(_) => Consumer::Weapons,
            GrabVolume::VehicleControl(_)
            | GrabVolume::CockpitButton(_)
            | GrabVolume::ParachuteHandle(_)
            | GrabVolume::MountedHandle(_) => Consumer::Vehicles,
            GrabVolume::Pickup(_) => Consumer::Pickup,
            GrabVolume::Climb => Consumer::Climb,
            GrabVolume::StuntHold => Consumer::Stunt,
            GrabVolume::Snatch(_) => Consumer::Snatch,
        }
    }
}

/// What a provider places on an input tick: its targets, and what it noted about each hand's
/// reach, by [`Hand::index`], for the grip press's log.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Provided {
    pub targets: Vec<GripTarget>,
    pub notes: [Vec<String>; 2],
}

impl Provided {
    /// The targets alone, with nothing noted.
    pub fn targets(targets: Vec<GripTarget>) -> Self {
        Self {
            targets,
            notes: Default::default(),
        }
    }

    /// Add `other`'s targets and notes after these.
    pub fn extend(&mut self, other: Provided) {
        self.targets.extend(other.targets);
        for (notes, more) in self.notes.iter_mut().zip(other.notes) {
            notes.extend(more);
        }
    }
}

/// The subsystems that act on grabs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Consumer {
    Weapons,
    Vehicles,
    Pickup,
    Climb,
    Stunt,
    Snatch,
}

/// The grabs of `grabs` (by [`Hand::index`]) that `consumer` acts on.
pub fn grabs_for(grabs: [Option<GrabVolume>; 2], consumer: Consumer) -> [Option<GrabVolume>; 2] {
    grabs.map(|g| g.filter(|v| v.consumer() == consumer))
}

/// What the physical hands' reach found in the world.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PickupKind {
    /// A loose object.
    Prop,
    /// An object hinged to the world or another body.
    Hinged,
    /// A weapon lying in the world.
    Weapon,
    /// A vehicle's door, hood, trunk, or hatch.
    Door,
    /// A character.
    Character,
}

impl PickupKind {
    pub const fn label(self) -> &'static str {
        match self {
            PickupKind::Prop => "object in reach",
            PickupKind::Hinged => "hinged object in reach",
            PickupKind::Weapon => "weapon in the world",
            PickupKind::Door => "vehicle door",
            PickupKind::Character => "character",
        }
    }
}

/// What a remote target for the grapple's snatch is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RemoteKind {
    /// A weapon: on the ground, or on a character.
    Weapon,
    /// A loose object.
    Object,
    /// A vehicle's closed door, pulled open on the line.
    Door,
}

/// The kinds of target, each with its marker's colour.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Category {
    /// A loose or hinged object: white.
    Prop,
    /// A weapon, or a part of the held one: gold.
    Weapon,
    /// A socket or a pouch on the body: blue.
    Body,
    /// A vehicle's control, button, door, or handle, or the parachute's: green.
    Vehicle,
    /// A surface to climb: orange.
    Climb,
    /// A character: violet.
    Character,
    /// A live grenade or a held grenade's pin: red.
    Explosive,
}

impl Category {
    #[cfg(test)]
    pub const ALL: [Category; 7] = [
        Category::Prop,
        Category::Weapon,
        Category::Body,
        Category::Vehicle,
        Category::Climb,
        Category::Character,
        Category::Explosive,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Category::Prop => "object",
            Category::Weapon => "weapon",
            Category::Body => "body",
            Category::Vehicle => "vehicle",
            Category::Climb => "climb",
            Category::Character => "character",
            Category::Explosive => "explosive",
        }
    }
}

/// The name of the grab volume on `socket`. The sides are the body's own, not the dominant hand's.
pub const fn socket_label(socket: Socket) -> &'static str {
    match socket {
        Socket::Hip(Hand::Left) => "left hip socket",
        Socket::Hip(Hand::Right) => "right hip socket",
        Socket::Shoulder(Hand::Left) => "left shoulder socket",
        Socket::Shoulder(Hand::Right) => "right shoulder socket",
        Socket::BackOuter => "outer back socket",
        Socket::Chest => "chest socket",
    }
}

/// A sphere a provider places: its grab point in the cockpit frame, the reach around it, and the
/// hands that may take it. The providers' pure placement works in these; the provider turns each
/// into a [`GripTarget`] with its marker.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Candidate {
    pub volume: GrabVolume,
    /// The grab point.
    pub centre: Vec3,
    /// How far from the grab point a hand takes it, metres.
    pub radius: f32,
    /// The one hand that may take it, or `None` for either.
    pub hand: Option<Hand>,
}

impl Candidate {
    /// How far `point` is from the grab point, when it is within reach: `None` outside the reach,
    /// for a degenerate reach, or for a point that is not finite.
    #[cfg(test)]
    pub fn reach(&self, point: Vec3) -> Option<f32> {
        if self.radius.is_nan() || self.radius <= 0.0 || !point.is_finite() {
            return None;
        }
        let distance = point.distance(self.centre);
        (distance <= self.radius).then_some(distance)
    }

    /// The target this places, reached by the hand as it is drawn and marked by `mark`.
    pub fn target(&self, mark: Mark) -> GripTarget {
        GripTarget::sphere(self.volume, self.hand, self.centre, self.radius, mark)
    }
}

/// The candidate `hand`, at `point`, would take among `candidates`, by the same nearest-grab-point
/// rule the resolver applies: for the tests of the providers' placement.
#[cfg(test)]
pub fn grab_volume(point: Vec3, hand: Hand, candidates: &[Candidate]) -> Option<GrabVolume> {
    candidates
        .iter()
        .filter(|c| c.hand.is_none_or(|h| h == hand))
        .filter_map(|c| c.reach(point).map(|d| (d, Tier::of(c.volume), c.volume)))
        .filter(|(_, tier, _)| *tier != Tier::Touch)
        .min_by(|a, b| a.1.cmp(&b.1).then(a.0.total_cmp(&b.0)))
        .map(|(_, _, volume)| volume)
}
