//! The one-off sounds the hands' own work plays at a gun, from the gun's attached effects
//! ([`WeaponData::m_AttachedEffects`]): the game voices a reload, an action's cycle, a draw, and
//! a holster through its clips' track messages and its reload start, none of which plays while the
//! hands do the work, so each move plays its sound here instead, positioned at the gun.
//!
//! A sound is a list of usages in order of preference, and the first the gun's effects carry plays.
//! What each gun's effect file carries is in `docs/engine/gameplay/weapon-ammo-and-reload.md`, "The
//! guns' sounds"; a move with no usage on a gun is silent there and leaves its haptic cue alone.

use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponData},
    physics::GameMaterialId,
};

use crate::controllers::weapons::engine::gun_ptr;

/// A move of the hands that plays a sound at the gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum GunSound {
    /// A round or a magazine seated in the well.
    Seat,
    /// The well refused what was pushed at it, or the gun refused an action.
    Refuse,
    /// The action reached the rear of its travel.
    ActionBack,
    /// The action came home.
    ActionHome,
    /// A bolt opened after a shot: the casing the game's bolt act throws.
    BoltOpened,
    /// The gun came off a socket into a hand.
    Draw,
    /// The gun went onto a socket.
    Holster,
}

impl GunSound {
    /// The usages that play this sound, the first a gun carries winning.
    pub(crate) const fn usages(self) -> &'static [u32] {
        match self {
            // The insert sound, the reload's end, and the dry-fire click borrowed as the
            // magazine's catch.
            Self::Seat => &[
                WeaponData::RELOAD_INSERT_AMMO_USAGE,
                WeaponData::RELOAD_END_USAGE,
                WeaponData::OUT_OF_AMMO_FIRE_USAGE,
            ],
            Self::Refuse => &[WeaponData::OUT_OF_AMMO_FIRE_USAGE],
            Self::ActionBack => &[WeaponData::COCK_BACK_USAGE],
            Self::ActionHome => &[WeaponData::COCK_FORWARD_USAGE],
            Self::BoltOpened => &[WeaponData::BOLT_ACTION_USAGE],
            Self::Draw => &[WeaponData::DRAW_WEAPON_USAGE],
            Self::Holster => &[WeaponData::HOLSTER_WEAPON_USAGE],
        }
    }
}

/// Play `sound` on gun `index` of `weapon`, the first of its usages the gun carries. Returns the
/// usage played, or `None` when the gun carries none of them.
///
/// # Safety
///
/// `weapon` must be live and the local player's, on the game thread before the world update.
pub(crate) unsafe fn play(weapon: &WeaponBase, index: usize, sound: GunSound) -> Option<u32> {
    let mut gun = unsafe { gun_ptr(weapon, index) }?;
    let effects = unsafe { &mut gun.as_mut().m_AttachedEffects };
    let material = GameMaterialId::EGameMaterialId_Default;
    sound.usages().iter().copied().find(|&hash| {
        let usage = unsafe { effects.FindAttachedEffectUsage(hash) };
        // A looping usage would play until stopped, and nothing here stops it.
        !usage.is_null()
            && !unsafe { effects.IsAttachedEffectLooping(usage, material) }
            && unsafe { effects.ActivateAttachedEffect(usage, true, material) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [GunSound; 7] = [
        GunSound::Seat,
        GunSound::Refuse,
        GunSound::ActionBack,
        GunSound::ActionHome,
        GunSound::BoltOpened,
        GunSound::Draw,
        GunSound::Holster,
    ];

    #[test]
    fn every_sound_has_a_usage_and_no_usage_twice() {
        for sound in ALL {
            let usages = sound.usages();
            assert!(!usages.is_empty(), "{sound:?}");
            for (i, usage) in usages.iter().enumerate() {
                assert!(!usages[..i].contains(usage), "{sound:?}");
            }
        }
    }

    #[test]
    fn the_seat_prefers_the_insert_sound_and_falls_back_to_the_click() {
        let usages = GunSound::Seat.usages();
        assert_eq!(usages.first(), Some(&WeaponData::RELOAD_INSERT_AMMO_USAGE));
        assert_eq!(usages.last(), Some(&WeaponData::OUT_OF_AMMO_FIRE_USAGE));
    }

    #[test]
    fn the_action_never_plays_the_whole_reload() {
        let reload = [
            WeaponData::RELOAD_START_USAGE,
            WeaponData::RELOAD_REMOVE_MAGAZINE_USAGE,
        ];
        for sound in [GunSound::ActionBack, GunSound::ActionHome] {
            assert!(
                sound.usages().iter().all(|u| !reload.contains(u)),
                "{sound:?}"
            );
        }
        assert_ne!(GunSound::ActionBack.usages(), GunSound::ActionHome.usages());
    }
}
