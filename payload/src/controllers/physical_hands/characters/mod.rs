//! Taking hold of characters with an empty hand. A grip on a character seated in a vehicle, reached
//! through an open door, holds the hand on them; a yank then pulls them out the game's own way, by
//! the vehicle-entry action, which hijacks an occupied seat. The design is in
//! `docs/mod/input/character-grabs.md`, the engine side in `docs/engine/gameplay/vehicle-doors.md`.
//!
//! - [`config`]: the settings.
//!
//! The pickup's reach ([`crate::controllers::physical_hands::pickup::reach`]) asks [`reach`] about
//! every ray hit on a character. The pickup tick
//! ([`crate::controllers::physical_hands::pickup::tick`]) keeps a grip on a character
//! ([`Seize`]): the hand stays at its grab point on the character ([`SeizeStep::Pin`]) until it pulls
//! away past [`CharactersConfig::yank_m`] ([`SeizeStep::Yank`]).

pub mod config;

use std::num::NonZeroUsize;

pub use config::CharactersConfig;
use glam::{Mat4, Vec3};
use jc3gi::{character::character::Character, vehicle::vehicle::DoorSlot};

use crate::controllers::physical_hands::{
    bodies::owner_of,
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
}

/// A character seated in a vehicle, reached through an open door.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seated {
    pub vehicle: NonZeroUsize,
    pub door: DoorSlot,
    /// How far the door stands open, degrees.
    pub open_deg: f32,
}

/// What a palm centred at `palm` reaches of the character `object` through the ray hit `hit` on
/// its body `body`: a character seated in a vehicle with a door open beside the hand. `None` for
/// anything else.
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
) -> Option<Reach> {
    let character = unsafe { &*(object.get() as *const Character) };
    if !(config.hijack && unsafe { character.IsAttachedToVehicle() }) {
        return None;
    }
    let vehicle = NonZeroUsize::new(unsafe { character.GetVehiclePtr() } as usize)?;
    let (door, open_deg) = unsafe { open_door_near(vehicle, palm, world_scale, config) }?;
    Some(Reach {
        object,
        body,
        point: hit.position,
        distance: hit.position.distance(palm),
        kind: TargetKind::Character(CharacterTarget {
            character: object,
            body,
            seated: Some(Seated {
                vehicle,
                door,
                open_deg,
            }),
        }),
    })
}

/// A hand holding on to a character.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seize {
    pub target: CharacterTarget,
    /// The grab point in the character's frame.
    pub grab: Vec3,
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
    /// The game thread, outside the physics step; the character must be live.
    pub unsafe fn take(target: CharacterTarget, point: Vec3) -> Option<Self> {
        let grab = unsafe { frame(target.character) }
            .inverse()
            .transform_point3(point);
        grab.is_finite().then_some(Self { target, grab })
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

/// Whether the hand, with its palm at `palm`, still reaches in through an open door of the vehicle
/// a seated character sits in: the door and how far it stands open, degrees.
///
/// # Safety
///
/// The game thread, outside the physics step; `seated`'s vehicle must be live.
pub(crate) unsafe fn door_still_open(
    seated: &Seated,
    palm: Vec3,
    world_scale: f32,
    config: &CharactersConfig,
) -> Option<(DoorSlot, f32)> {
    unsafe { open_door_near(seated.vehicle, palm, world_scale, config) }
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
    config: &CharactersConfig,
) -> Option<(DoorSlot, f32)> {
    let reach_m = config.hijack_door_reach_m * world_scale;
    unsafe { door_engine::doors(vehicle) }
        .into_iter()
        .filter(|d| !d.latched)
        .filter_map(|d| {
            let distance = d.door_box?.centre().distance(palm);
            let open_deg = unsafe { door_engine::open_angle(vehicle, d.part_index) }.to_degrees();
            (distance <= reach_m && open_deg >= config.hijack_min_open_deg)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_yanks_once_it_pulls_far_enough() {
        let at = Vec3::new(1.0, 1.4, 0.0);
        assert_eq!(yank(at, at + Vec3::X * 0.1, 0.2), None);
        let direction = yank(at, at + Vec3::new(-0.3, 0.1, 0.0), 0.2).unwrap();
        assert!(direction.x < -0.9 && (direction.length() - 1.0).abs() < 1e-5);
        assert_eq!(yank(at, Vec3::NAN, 0.2), None);
    }
}
