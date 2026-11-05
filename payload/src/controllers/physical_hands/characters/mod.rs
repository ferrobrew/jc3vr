//! Taking hold of characters with an empty hand. A grip on a character on foot holds the hand on
//! them: a yank throws a character the player is hostile to off their feet, by the character's own
//! hit reaction, and the hand then takes hold of the ragdoll they fall as, which it drags and
//! throws as any loose object; letting go with a push shoves them. A grip on a character seated in
//! a vehicle, reached through an open door, holds the hand on them too; a yank then pulls them out
//! the game's own way, by the vehicle-entry action, which hijacks an occupied seat. The design is
//! in `docs/mod/input/character-grabs.md`, the engine side in
//! `docs/engine/gameplay/melee-and-damage.md` and `docs/engine/gameplay/vehicle-doors.md`.
//!
//! - [`config`]: the settings;
//! - [`engine`]: a character's capsule body, the player's hostility to it, and whether it is dead;
//! - [`ragdoll`]: a ragdolling character's bodies, the bone nearest a point, and the whole
//!   ragdoll's mass.
//!
//! A ragdolling character, fallen or dead, is taken by the dynamic body of its ragdoll nearest the
//! point the reach hit, whichever of the character's bodies the hit was on, and held as a loose
//! object of the whole ragdoll's weight.
//!
//! The pickup's reach ([`crate::controllers::physical_hands::pickup::reach`]) asks [`reach`] about
//! every ray hit on a character. The pickup tick
//! ([`crate::controllers::physical_hands::pickup::tick`]) keeps a grip on a character
//! ([`Seize`]): the hand stays at its grab point on the character ([`SeizeStep::Pin`]) until it pulls
//! away past [`YANK_M`] ([`SeizeStep::Yank`]). The yank's and the shove's hit
//! reactions are dealt by the hand bodies' blows on the next frame tick
//! ([`crate::controllers::physical_hands::physics::queue_shove`]), outside the game's update.

mod acquisition;
pub mod config;
pub mod engine;
pub mod ragdoll;

use std::num::NonZeroUsize;

pub use config::CharactersConfig;
use glam::{Mat4, Vec3};
use jc3gi::{character::character::Character, vehicle::vehicle::DoorSlot};

use crate::controllers::physical_hands::{
    bodies::owner_of,
    characters::ragdoll::Ragdoll,
    contact::SurfaceHit,
    doors::engine as door_engine,
    grasp::{Reach, TargetKind},
};

/// A character a hand can take hold of.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CharacterTarget {
    pub character: NonZeroUsize,
    /// The body the reach found, which proves the character alive while the hand holds on.
    pub body: u32,
    /// The vehicle the character is seated in, and the open door the hand reaches in through.
    pub seated: Option<Seated>,
    /// Whether the body is one of the character's ragdoll's, which the hand holds as a loose
    /// object, rather than its capsule.
    pub ragdoll: bool,
    /// Whether the player's faction is hostile to the character.
    pub hostile: bool,
    /// The whole ragdoll's mass, kilograms, for a body of its ragdoll: what holding that one body
    /// drags. Zero for the capsule.
    pub mass_kg: f32,
}

/// Why a character the reach hit is not one a hand takes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// The grab, or the hijack for a seated character, is off.
    Off,
    /// The character is seated in a vehicle with no door open beside the hand.
    NoOpenDoor,
    /// The character is dead but not yet a ragdoll: the death animation still drives it.
    Dying,
    /// The hit was on a body of a standing character's ragdoll, which its animation drives.
    Animated,
    /// The character ragdolls but none of its ragdoll's bodies could be read.
    NoBody,
}

impl Unavailable {
    pub const fn label(self) -> &'static str {
        match self {
            Unavailable::Off => "the grab is off",
            Unavailable::NoOpenDoor => "seated with no door open beside the hand",
            Unavailable::Dying => "dying, not yet a ragdoll",
            Unavailable::Animated => "a limb its animation drives",
            Unavailable::NoBody => "no body of its ragdoll could be read",
        }
    }
}

/// A character seated in a vehicle, reached through an open door.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seated {
    pub vehicle: NonZeroUsize,
    pub door: DoorSlot,
    /// How far the door stands open, degrees.
    pub open_deg: f32,
}

/// How far a hand gripping a character must pull away from its grab point to yank, metres of the
/// player's hand.
pub(crate) const YANK_M: f32 = 0.2;

/// How hard a yank throws a character the player is hostile to, as a share of the knockdown blow
/// ([`HandHitsTuning::knockdown_impulse_ns`]); one and above knocks the character down.
///
/// [`HandHitsTuning::knockdown_impulse_ns`]: crate::controllers::physical_hands::physics::HandHitsTuning::knockdown_impulse_ns
pub(crate) const YANK_STRENGTH: f32 = 1.3;

/// How long after a yank the hand looks for the fallen character's ragdoll to hold, seconds of
/// game time: the fall takes as long as the game's clock says, so slow motion gives it longer.
pub(crate) const REGRIP_S: f32 = 1.0;

/// The slowest hand that shoves a character it lets go of, metres per second.
pub(crate) const SHOVE_MIN_SPEED_MPS: f32 = 1.5;

/// What a palm centred at `palm` reaches of the character `object` through the ray hit `hit` on
/// its body `body`: a character on foot, by its capsule, a ragdolling character, by the body of
/// its ragdoll nearest the hit, or a character seated in a vehicle with a door open beside the
/// hand; or why it reaches none of these.
///
/// # Safety
///
/// The game thread, outside the physics step; `object` must be a live character from a cast this
/// tick.
pub(crate) unsafe fn reach(
    object: NonZeroUsize,
    body: u32,
    hit: &SurfaceHit,
    palm: Vec3,
    world_scale: f32,
    config: &CharactersConfig,
) -> Result<Reach, Unavailable> {
    let character = unsafe { &*(object.get() as *const Character) };
    let hostile = unsafe { engine::hostile(character) };
    let distance = hit.position.distance(palm);
    let reach =
        |reached: NonZeroUsize, body: u32, seated: Option<Seated>, mass_kg: Option<f32>| Reach {
            object: reached,
            body,
            point: hit.position,
            distance,
            normal: hit.normal,
            kind: TargetKind::Character(CharacterTarget {
                character: object,
                body,
                seated,
                ragdoll: mass_kg.is_some(),
                hostile,
                mass_kg: mass_kg.unwrap_or(0.0),
            }),
        };
    if unsafe { character.IsAttachedToVehicle() } {
        if !config.hijack {
            return Err(Unavailable::Off);
        }
        let vehicle = NonZeroUsize::new(unsafe { character.GetVehiclePtr() } as usize)
            .ok_or(Unavailable::NoOpenDoor)?;
        let (door, open_deg) =
            unsafe { open_door_near(vehicle, palm, world_scale) }.ok_or(Unavailable::NoOpenDoor)?;
        let seated = Seated {
            vehicle,
            door,
            open_deg,
        };
        return Ok(reach(object, body, Some(seated), None));
    }
    if !config.grab {
        return Err(Unavailable::Off);
    }
    if let Some(ragdoll) = unsafe { Ragdoll::of(character) } {
        // The hit may be on the capsule or the ragdoll's hit proxy rather than on a body of the
        // ragdoll itself: the bone nearest the hit stands in for it.
        let bone = ragdoll
            .bone(body)
            .or_else(|| ragdoll.nearest(hit.position))
            .ok_or(Unavailable::NoBody)?;
        return Ok(reach(bone.owner, bone.body, None, Some(ragdoll.mass_kg())));
    }
    if unsafe { engine::dead(character) } {
        return Err(Unavailable::Dying);
    }
    let capsule = unsafe { engine::proxy_body(character) };
    if capsule.is_none_or(|capsule| capsule == body) {
        Ok(reach(object, body, None, None))
    } else {
        Err(Unavailable::Animated)
    }
}

/// A hand holding on to a character.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seize {
    pub target: CharacterTarget,
    /// The grab point in the character's frame.
    pub grab: Vec3,
}

/// A hand that yanked a character off their feet, waiting to take hold of the ragdoll they fall
/// as: it stays at the grab point on the character until its reach finds one of the character's
/// ragdoll's bodies, or until it gives up.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Regrip {
    pub seize: Seize,
    /// The game time left before the hand gives up, seconds.
    pub remaining_s: f32,
}

impl Regrip {
    /// A regrip of `seize` that waits [`REGRIP_S`].
    pub fn new(seize: Seize) -> Self {
        Self {
            seize,
            remaining_s: REGRIP_S,
        }
    }

    /// Count `dt` seconds of game time off the wait, and return whether it has run out.
    pub fn run_down(&mut self, dt: f32) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.remaining_s -= dt;
        self.expired()
    }

    /// Whether the wait has run out.
    pub fn expired(&self) -> bool {
        self.remaining_s <= 0.0
    }

    /// Whether `reach` is a body of the regripped character's ragdoll.
    pub fn takes(&self, reach: &Reach) -> bool {
        matches!(
            reach.kind,
            TargetKind::Character(target)
                if target.ragdoll && target.character == self.seize.target.character
        )
    }
}

/// What a grip on a character does on a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SeizeStep {
    /// The hand holds on at this world point.
    Pin(Vec3),
    /// The hand yanked from the grab point at `at` along `direction`, a unit vector.
    Yank { at: Vec3, direction: Vec3 },
    /// The character is gone.
    Lost,
}

impl Seize {
    /// The grip of a hand that took `target` at the world point `point`.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step. The queued target need not still be live.
    pub unsafe fn take(
        target: CharacterTarget,
        point: Vec3,
        palm: Vec3,
        world_scale: f32,
        config: &CharactersConfig,
    ) -> Option<Self> {
        // SAFETY: the adapter checks body ownership before any character access.
        unsafe { acquisition::take(target, point, palm, world_scale, config) }
    }

    /// What the grip does with the palm at `palm`, world space, yanking at `yank_m` from the grab
    /// point.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step.
    pub unsafe fn step(&self, palm: Vec3, yank_m: f32) -> SeizeStep {
        if unsafe { owner_of(self.target.body) } != Some(self.target.character) {
            return SeizeStep::Lost;
        }
        let at = unsafe { frame(self.target.character) }.transform_point3(self.grab);
        match yank(at, palm, yank_m) {
            Some(direction) => SeizeStep::Yank { at, direction },
            None => SeizeStep::Pin(at),
        }
    }
}

/// Whether the hand still reaches through an open door of the character's current vehicle.
/// The character must still occupy the vehicle captured by the grip.
///
/// # Safety
///
/// The game thread, outside the physics step. The target's current owner is checked first.
pub(crate) unsafe fn door_still_open(
    target: CharacterTarget,
    palm: Vec3,
    world_scale: f32,
    config: &CharactersConfig,
) -> Option<(DoorSlot, f32)> {
    let seated = unsafe { acquisition::seated(target, palm, world_scale, config) }?;
    Some((seated.door, seated.open_deg))
}

/// The open door of `vehicle` nearest the palm at `palm`, within the hijack's reach and open at
/// least its angle, and how far it stands open, degrees.
///
/// # Safety
///
/// The game thread, outside the physics step; `vehicle` must be live.
unsafe fn open_door_near(
    vehicle: NonZeroUsize,
    palm: Vec3,
    world_scale: f32,
) -> Option<(DoorSlot, f32)> {
    let reach_m = HIJACK_DOOR_REACH_M * world_scale;
    unsafe { door_engine::doors(vehicle) }
        .into_iter()
        .filter(|d| !d.latched)
        .filter_map(|d| {
            let distance = d.door_box?.centre().distance(palm);
            let open_deg = unsafe { door_engine::open_angle(vehicle, d.part_index) }.to_degrees();
            (distance <= reach_m && open_deg >= HIJACK_MIN_OPEN_DEG)
                .then_some((d.slot, open_deg, distance))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(slot, open_deg, _)| (slot, open_deg))
}

/// The character's frame: its world transform at the end of the current simulation step.
///
/// # Safety
///
/// `character` must be a live character.
unsafe fn frame(character: NonZeroUsize) -> Mat4 {
    let character = unsafe { &*(character.get() as *const Character) };
    Mat4::from(character.m_WorldMatrixT1)
}

/// The way a palm at `palm` yanked from a grab point at `at`, once it is `yank_m` or more away.
fn yank(at: Vec3, palm: Vec3, yank_m: f32) -> Option<Vec3> {
    let pulled = palm - at;
    (pulled.is_finite() && pulled.length() >= yank_m.max(1e-3))
        .then(|| pulled.normalize_or_zero())
        .filter(|d| *d != Vec3::ZERO)
}

/// How far open a door beside the hand must stand for a hand to reach in, degrees.
const HIJACK_MIN_OPEN_DEG: f32 = 25.0;

/// How far from the palm an open door may be for the hand to reach in through it, metres of the
/// player's hand.
const HIJACK_DOOR_REACH_M: f32 = 1.2;

#[cfg(test)]
mod tests {
    use super::*;

    fn target(character: usize, ragdoll: bool) -> CharacterTarget {
        CharacterTarget {
            character: NonZeroUsize::new(character).unwrap(),
            body: 7,
            seated: None,
            ragdoll,
            hostile: true,
            mass_kg: if ragdoll { 70.0 } else { 0.0 },
        }
    }

    fn reach(kind: TargetKind) -> Reach {
        Reach {
            object: NonZeroUsize::new(64).unwrap(),
            body: 9,
            point: Vec3::ZERO,
            distance: 0.05,
            normal: Vec3::ZERO,
            kind,
        }
    }

    #[test]
    fn a_regrip_takes_only_its_own_characters_ragdoll() {
        let regrip = Regrip {
            seize: Seize {
                target: target(32, false),
                grab: Vec3::Y,
            },
            remaining_s: REGRIP_S,
        };
        assert!(regrip.takes(&reach(TargetKind::Character(target(32, true)))));
        // Not the capsule, not another character's ragdoll, and not a prop.
        assert!(!regrip.takes(&reach(TargetKind::Character(target(32, false)))));
        assert!(!regrip.takes(&reach(TargetKind::Character(target(48, true)))));
        assert!(!regrip.takes(&reach(TargetKind::Prop { mass_kg: 5.0 })));
    }

    #[test]
    fn a_regrip_waits_on_the_game_clock() {
        let mut regrip = Regrip::new(Seize {
            target: target(32, false),
            grab: Vec3::Y,
        });
        // A second of real time at a quarter speed is a quarter of the wait.
        for _ in 0..60 {
            assert!(!regrip.run_down(0.25 / 60.0));
        }
        assert!((regrip.remaining_s - (REGRIP_S - 0.25)).abs() < 1e-4);
        // A paused clock, or a broken time step, waits on.
        assert!(!regrip.run_down(0.0));
        assert!(!regrip.run_down(f32::NAN));
        assert!(!regrip.run_down(-1.0));
        assert!(regrip.run_down(REGRIP_S));
    }

    #[test]
    fn a_hand_yanks_once_it_pulls_far_enough() {
        let at = Vec3::new(1.0, 1.4, 0.0);
        assert_eq!(yank(at, at + Vec3::X * 0.1, 0.2), None);
        let direction = yank(at, at + Vec3::new(-0.3, 0.1, 0.0), 0.2).unwrap();
        assert!(direction.x < -0.9 && (direction.length() - 1.0).abs() < 1e-5);
        assert_eq!(yank(at, Vec3::NAN, 0.2), None);
    }
}
