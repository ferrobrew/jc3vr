//! The native phase of an explicit safe-grenade throw.
//!
//! Construction reuses the grenade weapon's own pool allocator, constructor parameters, owner
//! binding, and update registration. Its ammunition deduction alone is suppressed for that weapon
//! on that thread. No ammunition is added or refunded.

use std::{cell::Cell, collections::VecDeque, ptr};

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::character::Character,
    equipment::{
        explosive::Explosive,
        weapon::{ThrownGrenadeWeapon, WeaponBase, WeaponData},
    },
    types::shared_ptr::WeakPtrGameObject,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    frame::Hand,
    loadout::{live, safe_items::SafeItemView},
    weapons::{
        grenade::{LAUNCH, PlannedLaunch, primed_explosive, safe, stage::Release},
        hold::refs::Strong,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CONSUME_AMMO_BINDER)
}

/// Submit one intentional throw. The safe item stays inert until creation and launch succeed.
pub(super) fn submit(item: SafeItemView, hand: Hand, world: Mat4, release: Release) -> bool {
    if !world.is_finite() || !release.velocity.is_finite() {
        return false;
    }
    let mut state = STATE.lock();
    if state
        .queue
        .iter()
        .any(|request| request.item.item == item.item)
        || state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request.item.item == item.item)
    {
        return false;
    }
    state.queue.push_back(Request {
        item,
        hand,
        world,
        release,
    });
    true
}

/// Cancel queued throws and remove an already created actor through its retained native owners.
pub(super) fn cancel() {
    let (requests, pending): (Vec<_>, _) = {
        let mut state = STATE.lock();
        state.cancel = true;
        (state.queue.drain(..).collect(), state.pending.take())
    };
    for request in requests {
        safe::activation_failed(request.item);
    }
    if let Some(pending) = pending {
        // SAFETY: the retained weapon and grenade references keep both native objects alive.
        unsafe {
            let weapon = &mut *pending.weapon_ptr();
            let same_actor = primed_explosive(weapon) == Some(pending.explosive());
            remove(&pending, weapon, same_actor);
        }
        safe::activation_failed(pending.request.item);
    }
}

/// Service deliberate throws after the local grenade weapon's normal update.
///
/// # Safety
///
/// `weapon` is the live local grenade weapon on its update thread.
pub(super) unsafe fn update(weapon: &mut ThrownGrenadeWeapon, dt: f32) {
    let base = (weapon as *mut ThrownGrenadeWeapon).cast::<WeaponBase>();
    let weapon_id = base as usize;
    let (pending, cancelled) = {
        let mut state = STATE.lock();
        (state.pending.take(), std::mem::take(&mut state.cancel))
    };
    if let Some(mut pending) = pending {
        let valid = live::safe_item(pending.request.item.item) == Some(pending.request.item);
        let current = unsafe { primed_explosive(weapon) };
        let same_actor = current == Some(pending.explosive());
        pending.age_s += if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if cancelled || !valid || pending.weapon != weapon_id || !same_actor || pending.age_s > 1.0
        {
            // SAFETY: the pending request retains its own weapon even if this update is a replacement.
            unsafe {
                let owner = &mut *pending.weapon_ptr();
                let owned_binding = primed_explosive(owner) == Some(pending.explosive());
                remove(&pending, owner, owned_binding);
            }
            safe::activation_failed(pending.request.item);
            return;
        }
        let Some(explosive) = (unsafe { pending.explosive().as_mut() }) else {
            safe::activation_failed(pending.request.item);
            return;
        };
        if explosive.m_PfxInstance.is_null() {
            STATE.lock().pending = Some(pending);
            return;
        }
        explosive.m_WorldMatrixT0 = pending.request.world.into();
        explosive.m_WorldMatrixT1 = pending.request.world.into();
        let Some(data) = (unsafe { weapon_data(&*base) }) else {
            unsafe { remove(&pending, weapon, true) };
            safe::activation_failed(pending.request.item);
            return;
        };
        LAUNCH.set(Some(PlannedLaunch {
            explosive: pending.explosive() as usize,
            transform: Some(pending.request.world.into()),
            velocity: pending.request.release.velocity,
        }));
        // The verified grenade override launches the primed actor and clears its weak binding.
        unsafe { (*base).Fire_Projectile(data) };
        let launched = LAUNCH.get().is_none();
        LAUNCH.set(None);
        if launched {
            safe::launched(pending.request.item);
            crate::controllers::weapons::grenade::report_native_throw(
                pending.request.hand,
                pending.request.release,
            );
        } else {
            let same_actor = unsafe { primed_explosive(weapon) } == Some(pending.explosive());
            unsafe { remove(&pending, weapon, same_actor) };
            safe::activation_failed(pending.request.item);
        }
        return;
    }
    let Some(request) = STATE.lock().queue.pop_front() else {
        return;
    };
    if live::safe_item(request.item.item) != Some(request.item) {
        safe::activation_failed(request.item);
        return;
    }
    let allowed = unsafe { (*base).m_User[0].cast::<Character>().as_ref() }
        .is_some_and(|character| character.m_Inventory.m_DoesAnimationAllowGrenades);
    if !allowed || unsafe { primed_explosive(weapon) }.is_some() {
        safe::activation_failed(request.item);
        return;
    }
    let Some(data) = (unsafe { weapon_data(&*base) }) else {
        safe::activation_failed(request.item);
        return;
    };
    let Some(owner) = (unsafe { Strong::lock(&(*base).m_WeakThis) }) else {
        safe::activation_failed(request.item);
        return;
    };
    let scope = AmmoNeutral::enter(weapon_id);
    unsafe { weapon.PrimeGrenade(data) };
    drop(scope);
    let Some(owned) = (unsafe { Strong::lock(&weapon.m_PrimedGrenade) }) else {
        tracing::debug!(target: "controllers", "grenade: native ammunition pool refused activation");
        safe::activation_failed(request.item);
        return;
    };
    let pending = Pending {
        request,
        weapon: weapon_id,
        owner,
        owned,
        age_s: 0.0,
    };
    if let Some(explosive) = unsafe { pending.explosive().as_mut() } {
        explosive.m_WorldMatrixT0 = pending.request.world.into();
        explosive.m_WorldMatrixT1 = pending.request.world.into();
    }
    STATE.lock().pending = Some(pending);
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::ConsumeAmmoBase_ADDRESS)]
fn consume_ammo(weapon: *mut WeaponBase, data: *mut WeaponData, count: i32) {
    if AMMO_NEUTRAL.get() == Some(weapon as usize) {
        return;
    }
    CONSUME_AMMO.get().unwrap().call(weapon, data, count);
}

unsafe fn weapon_data(weapon: &WeaponBase) -> Option<*mut WeaponData> {
    let data = &weapon.m_lWeaponData;
    if data.m_Begin.is_null() || data.m_End.is_null() {
        return None;
    }
    let count = unsafe { data.m_End.offset_from(data.m_Begin) };
    let index = weapon.m_NextWeaponToFire as isize;
    if count <= index || index < 0 {
        return None;
    }
    let selected = unsafe { *data.m_Begin.offset(index) };
    (!selected.is_null()).then_some(selected)
}

unsafe fn remove(pending: &Pending, weapon: &mut ThrownGrenadeWeapon, clear_binding: bool) {
    if let Some(explosive) = unsafe { pending.explosive().as_mut() } {
        // RemoveSelf is the native pool removal used by CancelPrimedGrenade, without its refund.
        unsafe { explosive.RemoveSelf() };
    }
    if clear_binding {
        let empty = WeakPtrGameObject {
            px: ptr::null_mut(),
            pn: ptr::null_mut(),
        };
        unsafe { weapon.m_PrimedGrenade.Assign(&empty) };
    }
}

struct AmmoNeutral(Option<usize>);

impl AmmoNeutral {
    fn enter(weapon: usize) -> Self {
        Self(AMMO_NEUTRAL.replace(Some(weapon)))
    }
}

impl Drop for AmmoNeutral {
    fn drop(&mut self) {
        AMMO_NEUTRAL.set(self.0);
    }
}

#[derive(Copy, Clone)]
struct Request {
    item: SafeItemView,
    hand: Hand,
    world: Mat4,
    release: Release,
}

struct Pending {
    request: Request,
    weapon: usize,
    owner: Strong,
    owned: Strong,
    age_s: f32,
}

impl Pending {
    fn weapon_ptr(&self) -> *mut ThrownGrenadeWeapon {
        self.owner
            .object()
            .cast::<u8>()
            .wrapping_sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
            .cast::<ThrownGrenadeWeapon>()
    }

    fn explosive(&self) -> *mut Explosive {
        self.owned
            .object()
            .cast::<u8>()
            .wrapping_sub(Explosive::GAME_OBJECT_BASE_OFFSET as usize)
            .cast::<Explosive>()
    }
}

struct State {
    queue: VecDeque<Request>,
    pending: Option<Pending>,
    cancel: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    queue: VecDeque::new(),
    pending: None,
    cancel: false,
});

thread_local! {
    static AMMO_NEUTRAL: Cell<Option<usize>> = const { Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ammo_neutral_scope_is_thread_local_and_restores_nested_owner() {
        assert_eq!(AMMO_NEUTRAL.get(), None);
        let outer = AmmoNeutral::enter(11);
        assert_eq!(AMMO_NEUTRAL.get(), Some(11));
        {
            let _inner = AmmoNeutral::enter(22);
            assert_eq!(AMMO_NEUTRAL.get(), Some(22));
        }
        assert_eq!(AMMO_NEUTRAL.get(), Some(11));
        assert_eq!(
            std::thread::spawn(|| AMMO_NEUTRAL.get()).join().unwrap(),
            None
        );
        drop(outer);
        assert_eq!(AMMO_NEUTRAL.get(), None);
    }
}
