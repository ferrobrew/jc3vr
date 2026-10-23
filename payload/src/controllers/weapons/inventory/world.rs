//! The game's side of the loadout table's weapons: a weak reference to each weapon object the
//! inventory has seen, which tells whether it still lives and where it is without keeping it alive,
//! and the pins' strong references.
//!
//! The table says where each of the player's weapons is; this says what the game did with the ones
//! the old handling no longer keeps in a slot, the carry set, or a hand. Each input tick, for each
//! such weapon of the table's, the weak reference ([`WeaponBase::m_WeakThis`]) gives its fate: gone,
//! held by another character (its holder), or lying in the world at a distance from the player,
//! which the pins plan from ([`crate::controllers::weapons::inventory::pins`]).
//!
//! A weapon another character takes leaves the table, since the game's word on ownership is final,
//! but it is still watched while that character holds it near the player, so the readouts can count
//! it and a single pistol stays one pistol in their hands
//! ([`crate::controllers::weapons::dual_wield::single`]).
//!
//! Everything here runs on the input tick, on the main thread outside the world update.

use glam::Vec3;
use jc3gi::{character::character::Character, equipment::weapon::WeaponBase};

use crate::controllers::{
    loadout::{ObjectAddress, live},
    weapons::{
        hold::refs::{Strong, Weak},
        inventory::{
            engine::{self, Fate},
            pins::{PinPlan, WorldWeapon},
        },
    },
};

/// The inventory's references to the weapons it has seen.
pub(super) struct World {
    handles: Vec<Handle>,
}

impl World {
    pub(super) const fn new() -> World {
        World {
            handles: Vec::new(),
        }
    }

    /// Watch `weapon`, a slot's or the carry set's, while it lives.
    ///
    /// # Safety
    ///
    /// `weapon` must be live.
    pub(super) unsafe fn watch(&mut self, weapon: &WeaponBase) {
        let Some(object) = engine::object_address(weapon) else {
            return;
        };
        if self.handles.iter().any(|h| h.object == object) {
            return;
        }
        self.handles.push(Handle {
            object,
            weak: unsafe { engine::weak_to(weapon) },
            pin: None,
            distance_m: None,
            alive: true,
            held_by_other: false,
            with_other: false,
        });
    }

    /// The fate of each of `objects`, seen from `owner` at `player`, noting each one's distance for
    /// the pins. A weapon never watched is taken for gone.
    ///
    /// # Safety
    ///
    /// `owner` must be the live local player character.
    pub(super) unsafe fn fates(
        &mut self,
        objects: &[ObjectAddress],
        owner: &Character,
        player: Vec3,
    ) -> Vec<(ObjectAddress, live::Fate)> {
        objects
            .iter()
            .map(|&object| {
                let fate = match self.handles.iter_mut().find(|h| h.object == object) {
                    Some(handle) => unsafe { handle.observe(owner, player) },
                    None => Fate::Gone,
                };
                let fate = match fate {
                    Fate::Lying { .. } => live::Fate::Lying,
                    Fate::HeldByOther { .. } => live::Fate::HeldByOther,
                    Fate::Gone => live::Fate::Gone,
                };
                (object, fate)
            })
            .collect()
    }

    /// The weapons the table has lying in the world (`lying`), as the pins see them.
    pub(super) fn world(&self, lying: &[ObjectAddress]) -> Vec<WorldWeapon> {
        lying
            .iter()
            .map(|&object| {
                let handle = self.handles.iter().find(|h| h.object == object);
                WorldWeapon {
                    object,
                    distance_m: handle.and_then(|h| h.distance_m),
                    pinned: handle.is_some_and(|h| h.pin.is_some()),
                }
            })
            .collect()
    }

    /// Carry out `plan`'s pins and unpins: a pin takes a strong reference, while the weapon still
    /// lives, and an unpin releases it. Returns how many were pinned.
    pub(super) fn apply(&mut self, plan: &PinPlan) -> u64 {
        let mut pinned = 0;
        for handle in &mut self.handles {
            if plan.pin.contains(&handle.object) && handle.pin.is_none() {
                handle.pin = handle.weak.lock();
                if handle.pin.is_some() {
                    pinned += 1;
                    tracing::info!(target: "controllers", object = %handle.object, "inventory: keeping a weapon lying in the world from the despawn");
                }
            }
            if plan.unpin.contains(&handle.object) && handle.pin.take().is_some() {
                tracing::info!(target: "controllers", object = %handle.object, "inventory: no longer keeping a weapon lying in the world");
            }
        }
        pinned
    }

    /// Watch the weapons another character took (`taken`) from now on, and settle the watched
    /// weapons against the table: a weapon it still has (`kept`) keeps its handle, and a pin only
    /// while it lies in the world (`lying`); a weapon with another character keeps its handle while
    /// it lives and is no further away than `forget_m`; every other handle goes.
    pub(super) fn settle(
        &mut self,
        kept: &[ObjectAddress],
        lying: &[ObjectAddress],
        taken: &[ObjectAddress],
        forget_m: f32,
    ) {
        for handle in &mut self.handles {
            if taken.contains(&handle.object) {
                handle.with_other = true;
            }
            if kept.contains(&handle.object) {
                handle.with_other = false;
            }
            if !lying.contains(&handle.object) {
                handle.pin = None;
            }
        }
        self.handles.retain(|h| {
            kept.contains(&h.object)
                || (h.with_other
                    && h.alive
                    && h.distance_m.is_none_or(|d| !d.is_finite() || d <= forget_m))
        });
    }

    /// The weapons another character took that are still watched.
    pub(super) fn with_others(&self) -> impl Iterator<Item = ObjectAddress> + '_ {
        self.handles
            .iter()
            .filter(|h| h.with_other)
            .map(|h| h.object)
    }

    /// The watched weapons another character holds now.
    pub(super) fn held_by_others(&self) -> usize {
        self.handles
            .iter()
            .filter(|h| h.with_other && h.held_by_other)
            .count()
    }

    /// A strong reference to each of `objects` that still lives.
    pub(super) fn strong(&self, objects: &[ObjectAddress]) -> Vec<Strong> {
        self.handles
            .iter()
            .filter(|h| objects.contains(&h.object))
            .filter_map(|h| h.weak.lock())
            .collect()
    }

    /// Release every reference.
    pub(super) fn clear(&mut self) {
        self.handles.clear();
    }
}

/// One weapon the inventory holds references to.
struct Handle {
    object: ObjectAddress,
    /// Tells whether the weapon still lives without keeping it alive.
    weak: Weak,
    /// The pin's strong reference, while the weapon lies in the world pinned.
    pin: Option<Strong>,
    /// How far it was from the player at its last fate.
    distance_m: Option<f32>,
    /// Whether it lived, and whether another character held it, at its last fate.
    alive: bool,
    held_by_other: bool,
    /// Another character took it, and it is watched while they keep it near.
    with_other: bool,
}

impl Handle {
    /// The weapon's fate now, noting its distance.
    ///
    /// # Safety
    ///
    /// `owner` must be the live local player character.
    unsafe fn observe(&mut self, owner: &Character, player: Vec3) -> Fate {
        let fate = unsafe { engine::fate(&self.weak, owner, player) };
        self.distance_m = match fate {
            Fate::Lying { distance_m } | Fate::HeldByOther { distance_m } => distance_m,
            Fate::Gone => None,
        };
        self.alive = fate != Fate::Gone;
        self.held_by_other = matches!(fate, Fate::HeldByOther { .. });
        fate
    }
}
