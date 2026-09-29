//! The per-shot seam: a detour on [`WeaponBase::Fire_ProjectileBase`], the base of every weapon
//! class's shot except the thrown grenades, which runs once per shot on the thread that updates the
//! weapon.
//!
//! For a shot of the local player's weapon from a gun held in a hand, under the controller scheme:
//!
//! - before the shot, it notes the gun's hand and the weapon's per-tick recoil
//!   ([`WeaponBase::m_RecoilPitch`] and [`WeaponBase::m_RecoilYaw`]), and marks the shot on the
//!   thread so that the fire rumble spawned inside it goes to that hand
//!   ([`crate::controllers::weapons::haptics`]);
//! - after the shot, the change in the recoil is the shot's kick, which kicks the hand's recoil
//!   offset ([`crate::controllers::weapons::recoil`]), and the dual-wield split learns which hand
//!   fired ([`crate::controllers::weapons::dual_wield`]). A weapon without a fire rumble of its own
//!   still pulses the hand with the kick alone.

use std::cell::Cell;

use detours_macro::detour;
use jc3gi::equipment::weapon::{WeaponBase, WeaponData, WeaponState};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        frame::Hand,
        hands::late,
        scheme,
        weapons::{dual_wield, haptics, recoil},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&FIRE_PROJECTILE_BASE_BINDER)
}

/// The hand of the shot in progress on this thread and the recoil kick so far, radians, when a
/// shot of a gun held in a hand is in progress.
pub(super) fn rumble_in_shot() -> Option<(Hand, f32)> {
    let shot = SHOT.get()?;
    let hand = shot.hand?;
    SHOT.set(Some(Shot {
        rumbled: true,
        ..shot
    }));
    // SAFETY: the shot's weapon is live for the length of its fire call, on this thread.
    let weapon = unsafe { (shot.weapon as *const WeaponBase).as_ref() }?;
    let (pitch, yaw) = shot.kick(weapon);
    Some((hand, pitch.hypot(yaw)))
}

/// A shot in progress on this thread.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Shot {
    weapon: usize,
    hand: Option<Hand>,
    pitch: f32,
    yaw: f32,
    /// Whether the shot's fire rumble was routed to its hand.
    rumbled: bool,
}

impl Shot {
    /// The recoil `weapon` has added since the shot began, `(pitch, yaw)` radians.
    fn kick(&self, weapon: &WeaponBase) -> (f32, f32) {
        let pitch = weapon.m_RecoilPitch - self.pitch;
        let yaw = weapon.m_RecoilYaw - self.yaw;
        if pitch.is_finite() && yaw.is_finite() {
            (pitch, yaw)
        } else {
            (0.0, 0.0)
        }
    }
}

thread_local! {
    static SHOT: Cell<Option<Shot>> = const { Cell::new(None) };
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::Fire_ProjectileBase_ADDRESS)]
fn fire_projectile_base(weapon: *mut WeaponBase, gun_data: *mut WeaponData) {
    let original = || FIRE_PROJECTILE_BASE.get().unwrap().call(weapon, gun_data);
    let config = Config::lock_query(|c| c.controllers);
    let acting = config.enabled && scheme::is_controllers();
    let Some(w) = unsafe { weapon.as_ref() }.filter(|w| acting && w.m_IsUserLocalPlayer) else {
        return original();
    };
    let shot = Shot {
        weapon: weapon as usize,
        hand: unsafe { gun_hand(w, gun_data) },
        pitch: w.m_RecoilPitch,
        yaw: w.m_RecoilYaw,
        rumbled: false,
    };
    let outer = SHOT.replace(Some(shot));
    original();
    let shot = SHOT.replace(outer).unwrap_or(shot);

    let (Some(hand), Some(w)) = (shot.hand, unsafe { weapon.as_ref() }) else {
        return;
    };
    let (pitch, yaw) = shot.kick(w);
    recoil::kick(hand, pitch, yaw, &config.weapons.recoil);
    dual_wield::on_shot(hand);
    if !shot.rumbled {
        haptics::kick_pulse(hand, pitch.hypot(yaw));
    }
}

/// The hand the firing gun rides, when the weapon is wielded in the hands.
///
/// # Safety
///
/// `weapon` must be live, and `gun_data` null or one of its guns.
unsafe fn gun_hand(weapon: &WeaponBase, gun_data: *const WeaponData) -> Option<Hand> {
    if !matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) {
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
