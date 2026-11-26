//! The per-shot seam: a detour on [`WeaponBase::Fire_ProjectileBase`], the base of every weapon
//! class's shot except the thrown grenades, which runs once per shot on the thread that updates the
//! weapon.
//!
//! For a shot of the local player's weapon from a gun held in a hand, under the controller scheme:
//!
//! - before the shot, it notes the gun's hand and the weapon's per-tick recoil
//!   ([`WeaponBase::m_RecoilPitch`] and [`WeaponBase::m_RecoilYaw`]), and marks the shot on the
//!   thread ([`jc3vr_body::controllers::haptics::shot`]) so that the fire rumble spawned inside it goes
//!   to that hand ([`jc3vr_body::controllers::haptics::rumble`]);
//! - after the shot, the change in the recoil is the shot's kick, which kicks the hand's recoil
//!   offset ([`crate::controllers::weapons::recoil`]), by the class's one-handed kick scale for a
//!   long gun in one hand ([`crate::controllers::weapons::heft`]), and the reload learns the shot's
//!   hand and the rounds left ([`crate::controllers::weapons::reload`]). A gun whose body the hands
//!   hold is kicked by an impulse on that body instead of the offset
//!   ([`crate::controllers::weapons::gun_bodies::queue_shot`]). A weapon without a fire rumble of
//!   its own still pulses the hand with the kick alone.
//!
//! A shot of the mounted gun the hands aim ([`jc3vr_body::controllers::facts::mounted`]) rides no hand:
//! after it, the detour hands it to the observer the vehicle handling registers
//! ([`register_mounted_shot_observer`]), which kicks the handles.

use std::sync::OnceLock;

use detours_macro::detour;
use jc3gi::equipment::weapon::{WeaponBase, WeaponData, WeaponState};
use jc3vr_body::controllers::{
    aim::gun,
    facts::mounted,
    hands::late,
    haptics::{rumble, shot, shot::Shot},
    scheme,
};
use jc3vr_input::frame::Hand;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::{
    config::WeaponSettings, dual_wield, gun_bodies, heft, hold::armed, recoil, reload,
};

/// Register the observer of each shot of the mounted gun the hands aim, called after the shot on
/// the thread that updates the weapon. One observer: a second registration is refused and logged.
pub(crate) fn register_mounted_shot_observer(observer: fn()) {
    if MOUNTED_SHOT_OBSERVER.set(observer).is_err() {
        tracing::error!("fire: a mounted shot observer is already registered; keeping the first");
    }
}

/// Whether the mounted shot observer is registered.
pub(crate) fn mounted_shot_observer_registered() -> bool {
    MOUNTED_SHOT_OBSERVER.get().is_some()
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&FIRE_PROJECTILE_BASE_BINDER)
}

static MOUNTED_SHOT_OBSERVER: OnceLock<fn()> = OnceLock::new();

#[detour(address = jc3gi::equipment::weapon::WeaponBase::Fire_ProjectileBase_ADDRESS)]
fn fire_projectile_base(weapon: *mut WeaponBase, gun_data: *mut WeaponData) {
    let original = || FIRE_PROJECTILE_BASE.get().unwrap().call(weapon, gun_data);
    let config = WeaponSettings::read();
    let acting = config.enabled && scheme::is_controllers();
    // The mounted gun the hands aim rides no hand; its kick goes to the handles.
    if acting && mounted::is_mounted_weapon(weapon as usize) {
        original();
        if let Some(observe) = MOUNTED_SHOT_OBSERVER.get() {
            observe();
        }
        return;
    }
    let Some(w) = unsafe { weapon.as_ref() }.filter(|w| acting && w.m_IsUserLocalPlayer) else {
        original();
        // The shot advanced the next gun; a single pistol's stays its first.
        if let Some(w) = unsafe { weapon.as_mut() } {
            dual_wield::single::pin(w);
        }
        return;
    };
    let shot = Shot {
        weapon: weapon as usize,
        hand: unsafe { gun_hand(w, gun_data) },
        pitch: w.m_RecoilPitch,
        yaw: w.m_RecoilYaw,
        rumbled: false,
    };
    let outer = shot::begin(shot);
    original();
    let shot = shot::end(outer).unwrap_or(shot);
    if let Some(w) = unsafe { weapon.as_mut() } {
        dual_wield::single::pin(w);
    }

    let (Some(hand), Some(w)) = (shot.hand, unsafe { weapon.as_ref() }) else {
        return;
    };
    let (pitch, yaw) = shot.kick(w);
    // A gun whose body the hands hold takes the shot's kick as an impulse on that body; the step
    // drops the shot for a gun without one, whose hands the recoil offset below moves instead.
    gun_bodies::queue_shot(hand, weapon as usize);
    recoil::kick(
        hand,
        pitch,
        yaw,
        &config.weapons.recoil,
        heft::one_hand_kick(hand),
    );
    reload::on_shot(weapon as usize, hand, w.m_MagazineAmmo);
    if !shot.rumbled {
        rumble::kick(hand, pitch.hypot(yaw));
    }
}

/// The hand the firing gun rides, when the weapon is wielded in the hands or is the second weapon.
///
/// # Safety
///
/// `weapon` must be live, and `gun_data` null or one of its guns.
pub(super) unsafe fn gun_hand(weapon: &WeaponBase, gun_data: *const WeaponData) -> Option<Hand> {
    if !matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) && !armed::is(weapon as *const WeaponBase as usize)
    {
        return None;
    }
    let gun = unsafe { gun_data.as_ref() }.or_else(|| {
        unsafe { gun::guns(weapon) }
            .nth(weapon.m_NextWeaponToFire as usize)
            .flatten()
    })?;
    let bone = *gun
        .m_lBoneAttachementPerWeaponState
        .get(weapon.m_State as usize)?;
    late::attach_hand(bone)
}
