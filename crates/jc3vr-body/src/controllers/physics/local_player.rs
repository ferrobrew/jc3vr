//! The local player character for one game tick.
//!
//! A [`LocalPlayer`] is reached from a [`GameTick`] and borrows it, so it lives no longer than the
//! tick that found it and never leaves the game thread. Reaching it fetches the character once
//! ([`Character::GetLocalPlayerCharacter`]) and checks it is non-null once; the accessors below are
//! safe because the tick's invariant (the game thread at a serial point of the frame, where a live
//! game object stays live) and the non-null check together are everything the engine calls they
//! wrap require.
//!
//! The handle inherits the tick's limits (`jc3vr_core::game_tick`): an engine call made through it
//! may free or retire parts of the character (its proxy, its weapons) and may reenter the mod's
//! detours, which can reach the same character, so code that keeps a part across such a call
//! re-resolves it. The handle is neither `Clone` nor `Copy`, and lends `&mut Character` only through
//! `&mut self`; a second handle reached in the same tick names the same character, as a second
//! engine pointer to it does.

use std::{marker::PhantomData, ptr::NonNull};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character, equipment::grappling_hook::GrapplingHook, game::GameObject,
};
use jc3vr_core::game_tick::GameTick;

use crate::controllers::physics::characters::{character_frame, character_object};

/// The local player character, non-null, for the tick `'t` that reached it.
pub struct LocalPlayer<'t> {
    character: NonNull<Character>,
    /// Bounded by the borrow of the tick, and neither `Send` nor `Sync`.
    _tick: PhantomData<(&'t (), *const ())>,
}

impl<'t> LocalPlayer<'t> {
    /// The local player character, or `None` when there is none (the frontend, a load, or a death
    /// between characters). One engine call and a null check.
    #[inline]
    pub fn get(_tick: &'t GameTick<'_>) -> Option<Self> {
        // SAFETY: the tick puts the caller on the game thread at a serial point of the frame, where
        // the engine's local player lookup reads its own player registry.
        let character = NonNull::new(unsafe { Character::GetLocalPlayerCharacter() })?;
        Some(Self {
            character,
            _tick: PhantomData,
        })
    }

    /// The character.
    #[inline]
    pub fn character(&self) -> &Character {
        // SAFETY: non-null, and live for the tick the handle borrows.
        unsafe { self.character.as_ref() }
    }

    /// The character, mutably.
    #[inline]
    pub fn character_mut(&mut self) -> &mut Character {
        // SAFETY: non-null, live for the tick the handle borrows, and lent only through `&mut self`.
        unsafe { self.character.as_mut() }
    }

    /// The character's address, as the engine's pointers and the mod's identity checks hold it.
    #[inline]
    pub fn as_ptr(&self) -> *mut Character {
        self.character.as_ptr()
    }

    /// The character's address as an integer, for identity comparisons and the raycast filters.
    #[inline]
    pub fn address(&self) -> usize {
        self.character.as_ptr() as usize
    }

    /// The character's `CGameObject` subobject ([`character_object`]).
    #[inline]
    pub fn object(&self) -> *mut GameObject {
        character_object(self.character.as_ptr())
    }

    /// The character's world transform, as its last simulation tick placed it ([`character_frame`]).
    #[inline]
    pub fn frame(&self) -> Mat4 {
        character_frame(self.character())
    }

    /// The character's world position, the translation of [`Self::frame`].
    #[inline]
    pub fn position(&self) -> Vec3 {
        self.frame().w_axis.truncate()
    }

    /// The character's faction ([`Character::GetFaction`]).
    #[inline]
    pub fn faction(&self) -> i32 {
        // SAFETY: a live character on the game thread at a serial point.
        unsafe { self.character().GetFaction() }
    }

    /// The character and its grappling hook, a separate object its inventory owns, or `None` before
    /// the inventory has one.
    #[inline]
    pub fn character_and_hook(&mut self) -> Option<(&mut Character, &mut GrapplingHook)> {
        let character = self.character_mut();
        // SAFETY: the inventory owns its hook, a separate allocation, for as long as the character
        // lives, which is the tick.
        let hook = unsafe { character.m_Inventory.m_GrapplingHook.px.as_mut() }?;
        Some((character, hook))
    }

    /// Whether the character mans a mounted gun ([`Character::IsInMountedGunState`]).
    #[inline]
    pub fn in_mounted_gun_state(&self) -> bool {
        // SAFETY: as [`Self::faction`].
        unsafe { self.character().IsInMountedGunState() }
    }

    /// Whether the character rides in a vehicle ([`Character::IsInRidingInVehicleState`]).
    #[inline]
    pub fn in_riding_in_vehicle_state(&self) -> bool {
        // SAFETY: as [`Self::faction`].
        unsafe { self.character().IsInRidingInVehicleState() }
    }

    /// Whether the character traverses a vehicle's stunt area ([`Character::IsStuntTraversing`]).
    #[inline]
    pub fn stunt_traversing(&self) -> bool {
        // SAFETY: as [`Self::faction`].
        unsafe { self.character().IsStuntTraversing() }
    }

    /// Whether the character hangs from a vehicle's stunt area ([`Character::IsStuntHanging`]).
    #[inline]
    pub fn stunt_hanging(&self) -> bool {
        // SAFETY: as [`Self::faction`].
        unsafe { self.character().IsStuntHanging() }
    }
}

impl std::fmt::Debug for LocalPlayer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalPlayer").field(&self.character).finish()
    }
}
