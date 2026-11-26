//! A swap onto a taken socket: a weapon let go of over a shoulder or a hip whose socket holds
//! another weapon docks there, and the weapon that was docked falls clear of the player. The swap
//! is the loadout table's decision ([`crate::controllers::loadout::grab::swaps`]); this carries out
//! its first half, the docked weapon's fall ([`eject`]), after which the release docks the held
//! weapon on the freed socket as any dock does.

use glam::Vec3;
use jc3gi::equipment::weapon::WeaponBase;
use jc3vr_body::controllers::physics::{characters::character_position, local_player::LocalPlayer};
use jc3vr_core::game_tick::GameTick;
use tracking_format::event::WorldMotion;

use crate::controllers::{
    loadout::{ObjectAddress, Socket, live},
    weapons::{
        docks,
        dual_wield::single,
        engine::grip_points,
        hold::{
            equip::{self, DockRefusal},
            release_diagnostics::ReleaseMotion,
            tuning::HOLD,
        },
        inventory::{
            engine::{strong_to, weapon_of},
            reconcile,
        },
    },
};

/// Let the weapon docked on `socket` fall into the world, moving away from the player at the swap's
/// [`eject_mps`](crate::controllers::weapons::hold::tuning::SwapTuning::eject_mps) so that it
/// falls clear of the body rather than onto it, and take it off the socket in the table.
pub(crate) fn eject(game: &GameTick<'_>, socket: Socket) -> Result<(), DockRefusal> {
    let mut local_player = LocalPlayer::get(game).ok_or(DockRefusal::NoCharacter)?;
    let owner = local_player.character_mut();
    let snapshot = live::snapshot_now().ok_or(DockRefusal::NoTable)?;
    let docked = unsafe { docks::socket_weapons(owner, &snapshot) }
        .into_iter()
        .find(|(s, _)| *s == socket)
        .map(|(_, weapon)| weapon)
        .ok_or(DockRefusal::NothingThere)?;
    let object = docked as *const WeaponBase as usize;
    let address = ObjectAddress::from_address(object).ok_or(DockRefusal::NothingThere)?;
    // The game's pair is one weapon and cannot fall; a single pistol is a weapon alone.
    if unsafe { single::is_pair(docked) } {
        return Err(DockRefusal::Unsupported);
    }
    let character = character_position(owner);
    // SAFETY: as above.
    let at = unsafe { grip_points(docked) }.map(|([grip, _], _)| grip);
    let away = at.map_or(Vec3::ZERO, |at| away_from(character, at));
    // SAFETY: a slot or the carry set keeps the docked weapon alive.
    let strong = unsafe { strong_to(docked) }.ok_or(DockRefusal::NotDone)?;
    live::dropped(address);
    // SAFETY: the strong reference keeps the weapon alive, and the game stage serializes mutation.
    let weapon = unsafe { &mut *weapon_of(&strong) };
    let motion = ReleaseMotion {
        world: WorldMotion {
            linear: away,
            angular: Vec3::ZERO,
        },
        ..ReleaseMotion::REST
    };
    let thrown =
        unsafe { equip::drop_carried(owner, weapon, HOLD.get().hold.swap.eject_mps, motion) };
    drop(strong);
    unsafe { reconcile::apply(owner) };
    tracing::info!(
        target: "controllers",
        socket = socket.label(),
        speed_mps = thrown.linear.length(),
        set_by_the_mod = thrown.set,
        "hold: the {} was taken, so its weapon falls clear for the one let go of there",
        socket.label()
    );
    Ok(())
}

/// The velocity that carries a weapon at `at` (world) clear of a character standing at
/// `character`: out from the body horizontally and a little up, at the swap's
/// [`eject_mps`](crate::controllers::weapons::hold::tuning::SwapTuning::eject_mps). Straight back
/// for a weapon over the character's own position.
fn away_from(character: Vec3, at: Vec3) -> Vec3 {
    let tuning = &HOLD.get().hold.swap;
    let out = Vec3::new(at.x - character.x, 0.0, at.z - character.z)
        .try_normalize()
        .unwrap_or(Vec3::ZERO);
    let direction = (out + Vec3::Y * tuning.eject_lift).normalize_or(Vec3::Y);
    direction * tuning.eject_mps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ejected_weapon_leaves_outward_and_a_little_up() {
        let character = Vec3::new(10.0, 2.0, -5.0);
        let v = away_from(character, character + Vec3::new(0.3, 1.4, 0.2));
        assert!((v.length() - HOLD.get().hold.swap.eject_mps).abs() < 1e-4);
        assert!(v.y > 0.0 && v.y < v.length() * 0.5, "{v}");
        let flat = Vec3::new(v.x, 0.0, v.z).normalize();
        assert!(
            flat.distance(Vec3::new(0.3, 0.0, 0.2).normalize()) < 1e-4,
            "{v}"
        );
        // A weapon straight above the character goes up rather than nowhere.
        let up = away_from(character, character + Vec3::Y);
        assert!(up.is_finite() && up.y > 0.0);
    }
}
