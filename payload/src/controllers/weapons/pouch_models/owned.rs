//! A model instance the payload owns, made from a model the game holds
//! ([`ModelInstanceManager::CreateInstanceFromModel`]), and the source of a gun's magazine model.
//!
//! The instance takes its own reference to the model ([`ModelInstance::m_spModel`]), so the model
//! lives as long as the instance whatever happens to the game object it came from. That is what
//! keeps a gun's magazine drawn across the gap after each magazine the gun drops, while the game
//! respawns the gun's spare magazine prop ([`WeaponData::m_MagazineObject`]): the instance made
//! from the spare's model ([`RigidObject::m_Model`]) is kept for that weapon, and another
//! instance of the same magazine can be made from its reference ([`magazine_source`]). A model
//! that only the game's model cache holds by name is instanced from the cache
//! ([`OwnedModel::show_cached`]).

use glam::Mat4;
use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponData},
    graphics_engine::model_instance::{
        Model, ModelInstance, ModelInstanceFlags, ModelInstanceManager,
    },
    types::{math::Matrix4, shared_ptr::SharedPtr},
};

use crate::controllers::aim::gun;

/// One owned instance of a game model, and the weapon it was made for, if any.
#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct OwnedModel {
    /// The instance (a `ModelInstance` address), or `None` before one is made.
    instance: Option<usize>,
    /// The model the instance was made from.
    model: Option<usize>,
    /// The weapon whose model it is (a `WeaponBase` address).
    weapon: Option<usize>,
    visible: bool,
}

impl OwnedModel {
    pub(crate) const EMPTY: OwnedModel = OwnedModel {
        instance: None,
        model: None,
        weapon: None,
        visible: false,
    };

    pub(crate) const fn visible(&self) -> bool {
        self.visible
    }

    /// The model the instance was made from, as an identity never dereferenced.
    pub(crate) const fn model(&self) -> Option<usize> {
        self.model
    }

    pub(crate) const fn weapon(&self) -> Option<usize> {
        self.weapon
    }

    /// Make this the model of `weapon` (or of no weapon), releasing an instance made for another.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(crate) unsafe fn retarget(&mut self, weapon: Option<usize>) {
        if self.weapon != weapon {
            unsafe { self.release() };
            self.weapon = weapon;
        }
    }

    /// The instance's own reference to its model, for `weapon`: what the instance is shown with,
    /// and another instance of the same model made from, while the model's game object is gone.
    pub(crate) fn kept_for(&self, weapon: Option<usize>) -> Option<*const SharedPtr<Model>> {
        let instance = self.keeps(weapon)?;
        // SAFETY: the instance is the payload's own, live until it is released on the game thread,
        // which is the thread every caller is on.
        let model = unsafe { &raw const (*(instance as *const ModelInstance)).m_spModel };
        (!unsafe { (*model).px }.is_null()).then_some(model)
    }

    /// The instance, when it was made for `weapon`.
    fn keeps(&self, weapon: Option<usize>) -> Option<usize> {
        self.instance.filter(|_| self.weapon == weapon)
    }

    /// Show an instance of `model`, making it first (at `world`) or remaking it when its model
    /// changed. Returns whether it made an instance; `None` when the engine gave none.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update; `model` is a live shared model.
    pub(crate) unsafe fn show(
        &mut self,
        model: *const SharedPtr<Model>,
        world: Mat4,
    ) -> Option<bool> {
        let source = unsafe { (*model).px } as usize;
        if self.instance.is_some() && self.model != Some(source) {
            let weapon = self.weapon;
            unsafe { self.release() };
            self.weapon = weapon;
        }
        let manager = unsafe { ModelInstanceManager::get() }?;
        let mut created = false;
        if self.instance.is_none() {
            let made = unsafe {
                manager.CreateInstanceFromModel(model, &Matrix4::from(world), true, false, true)
            };
            unsafe { self.adopt(made) }?;
            created = true;
        }
        unsafe { self.unhide() };
        Some(created)
    }

    /// Show an instance of the model the game's model cache holds under `name_hash`, making it
    /// first (at `world`) or remaking it when the instance is of another model. Returns whether
    /// it made an instance; `None` when the model is not cached or the engine gave no instance.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(crate) unsafe fn show_cached(&mut self, name_hash: u32, world: Mat4) -> Option<bool> {
        if let Some(instance) = self.instance {
            // SAFETY: the instance is the payload's own, live until it is released on this thread,
            // and it holds its model.
            let model = unsafe { (*(instance as *const ModelInstance)).m_spModel.px.as_ref() };
            if model.is_some_and(|m| m.m_Id.m_Hash == name_hash) {
                unsafe { self.unhide() };
                return Some(false);
            }
            let weapon = self.weapon;
            unsafe { self.release() };
            self.weapon = weapon;
        }
        let manager = unsafe { ModelInstanceManager::get() }?;
        // The cache lookup loads nothing: a model the game has not loaded gives no instance.
        let made = unsafe {
            manager.CreateInstanceFromHash(name_hash, &Matrix4::from(world), true, false, true)
        };
        unsafe { self.adopt(made) }?;
        unsafe { self.unhide() };
        Some(true)
    }

    /// Take `made`, a fresh instance from the engine, as this one, hidden. `None` for null.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update; `made` is null or a fresh instance.
    unsafe fn adopt(&mut self, made: *mut ModelInstance) -> Option<()> {
        let made = unsafe { made.as_mut() }?;
        // The engine's own owners of a model held on a character keep it drawn whatever the
        // camera's distance, as the explosives do.
        made.m_Flags.remove(ModelInstanceFlags::FLAG_CLIPONDISTANCE);
        self.model = Some(made.m_spModel.px as usize);
        self.instance = Some(made as *mut ModelInstance as usize);
        self.visible = false;
        Some(())
    }

    /// Show the instance already made, if it is hidden. Returns whether one is shown.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(crate) unsafe fn unhide(&mut self) -> bool {
        if !self.visible
            && let (Some(instance), Some(manager)) =
                (self.instance, unsafe { ModelInstanceManager::get() })
        {
            unsafe { manager.ShowModelInstance(instance as *mut ModelInstance) };
            self.visible = true;
        }
        self.visible
    }

    /// Hide the instance, keeping it.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(crate) unsafe fn hide(&mut self) {
        if let (Some(instance), true) = (self.instance, self.visible)
            && let Some(manager) = unsafe { ModelInstanceManager::get() }
        {
            unsafe { manager.HideModelInstance(instance as *mut ModelInstance) };
        }
        self.visible = false;
    }

    /// Release the instance.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(crate) unsafe fn release(&mut self) {
        if let Some(instance) = self.instance.take()
            && let Some(manager) = unsafe { ModelInstanceManager::get() }
        {
            unsafe { manager.AddToDeleteQueue(instance as *mut ModelInstance, false) };
        }
        *self = OwnedModel::EMPTY;
    }

    /// Push the instance's world matrix for the frame. Returns whether there was one to move.
    ///
    /// # Safety
    ///
    /// The game thread, outside the world update.
    pub(crate) unsafe fn push(&self, matrix: Mat4) -> bool {
        let Some(instance) = self.instance.filter(|_| self.visible) else {
            return false;
        };
        // SAFETY: the instance is the payload's own, live until it is released on this thread.
        unsafe { (*(instance as *mut ModelInstance)).UpdateMatrix(&Matrix4::from(matrix)) };
        true
    }
}

/// The model a magazine of the gun at `weapon` is drawn with: its spare magazine prop's model, or
/// while the game respawns the prop, the model one of `kept` already holds for that weapon.
///
/// # Safety
///
/// `weapon` is a live weapon of the local player, on the game thread before the world update.
pub(crate) unsafe fn magazine_source(
    weapon: usize,
    kept: [&OwnedModel; 2],
) -> Option<*const SharedPtr<Model>> {
    unsafe { spare_model(weapon) }
        .or_else(|| kept.into_iter().find_map(|k| k.kept_for(Some(weapon))))
}

/// The name hash of the weapon at `weapon` when its first gun has no magazine prop at all
/// ([`WeaponData::NO_MAGAZINE_UNIQUE_TAG`]), so it never has a spare magazine model to draw.
///
/// # Safety
///
/// As [`magazine_source`].
pub(crate) unsafe fn without_magazine_prop(weapon: usize) -> Option<u32> {
    let weapon = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    (first.m_MagazineUniqueTag == WeaponData::NO_MAGAZINE_UNIQUE_TAG)
        .then_some(weapon.m_WeaponNameHash)
}

/// The model of the spare magazine prop of the first gun of the weapon at `weapon`, or `None`
/// while the gun has no spare (respawning, or none at all).
///
/// # Safety
///
/// As [`magazine_source`].
pub(crate) unsafe fn spare_model(weapon: usize) -> Option<*const SharedPtr<Model>> {
    let weapon = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let magazine = unsafe { first.m_MagazineObject.px.as_ref() }?;
    let model: *const SharedPtr<Model> = &magazine.m_Model;
    (!unsafe { (*model).px }.is_null()).then_some(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instance_keeps_its_model_for_its_own_weapon_only() {
        let owned = OwnedModel {
            instance: Some(8),
            model: Some(7),
            weapon: Some(1),
            visible: true,
        };
        assert_eq!(owned.keeps(Some(1)), Some(8));
        assert_eq!(owned.keeps(Some(2)), None);
        assert_eq!(owned.keeps(None), None);
        assert_eq!(OwnedModel::EMPTY.keeps(None), None);
        let grenade = OwnedModel {
            weapon: None,
            ..owned
        };
        assert_eq!(grenade.keeps(None), Some(8));
    }
}
