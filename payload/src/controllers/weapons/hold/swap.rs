//! A swap onto a taken socket on the back: a weapon let go of over a shoulder whose socket holds
//! another weapon docks there, and the weapon that was docked falls clear of the player. The swap
//! is the loadout table's decision ([`crate::controllers::loadout::grab::swaps`]); this carries out
//! its first half, the docked weapon's fall ([`eject`]), after which the release docks the held
//! weapon on the freed socket as any dock does.

use glam::{Mat4, Vec3};
use jc3gi::equipment::weapon::WeaponBase;

use crate::controllers::{
    loadout::{ObjectAddress, Socket, live},
    weapons::{
        docks,
        dual_wield::single,
        engine::{self, slot_of},
        hold::{
            equip::{self, DockRefusal},
            release_diagnostics::{ReleaseMotion, WorldMotion},
            steal,
        },
        inventory::{
            carry,
            engine::{strong_to, weapon_of},
            reconcile,
        },
    },
};

/// Let the weapon docked on `socket` fall into the world, moving away from the player at
/// [`EJECT_MPS`] so that it falls clear of the body rather than onto it, and take it off the socket
/// in the table.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn eject(socket: Socket) -> Result<(), DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    let snapshot = live::snapshot_now().ok_or(DockRefusal::NoTable)?;
    let docked = unsafe { docks::socket_weapons(owner, &snapshot) }
        .into_iter()
        .find(|(s, _)| *s == socket)
        .map(|(_, weapon)| weapon)
        .ok_or(DockRefusal::NothingThere)?;
    let object = docked as *const WeaponBase as usize;
    let address = ObjectAddress::from_address(object).ok_or(DockRefusal::NothingThere)?;
    // SAFETY: a slot or the carry set keeps the docked weapon alive.
    let slot = unsafe { slot_of(docked) };
    // The game's pair is one weapon and cannot fall; a single pistol is a weapon alone.
    if unsafe { single::is_pair(docked) } {
        return Err(DockRefusal::Unsupported);
    }
    let in_slot = slot.filter(|slot| {
        owner
            .m_Inventory
            .m_Weapons
            .get(*slot as usize)
            .is_some_and(|entry| entry.px as usize == object)
    });
    let character = Mat4::from(owner.m_WorldMatrixT1).w_axis.truncate();
    // SAFETY: as above.
    let at = unsafe { steal::gun_points(docked) }.map(|([grip, _], _)| grip);
    let away = at.map_or(Vec3::ZERO, |at| away_from(character, at));
    // SAFETY: as above; the strong reference keeps it through the drop, which can unequip a heavy
    // weapon through its slot.
    let strong = unsafe { strong_to(docked) }.ok_or(DockRefusal::NotDone)?;
    let carried = carry::release(object);
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
    let thrown = unsafe { equip::release_loose(weapon, EJECT_MPS, motion) };
    if let Some(slot) = in_slot {
        unsafe { owner.m_Inventory.DropWeapon(slot, false) };
    }
    drop(carried);
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

/// How fast an ejected weapon leaves its socket, metres per second: enough to clear the body
/// before it lands, slow enough to stay at the player's feet.
const EJECT_MPS: f32 = 1.5;

/// The velocity that carries a weapon at `at` (world) clear of a character standing at
/// `character`: out from the body horizontally and a little up, at [`EJECT_MPS`]. Straight back
/// for a weapon over the character's own position.
fn away_from(character: Vec3, at: Vec3) -> Vec3 {
    let out = Vec3::new(at.x - character.x, 0.0, at.z - character.z)
        .try_normalize()
        .unwrap_or(Vec3::ZERO);
    let direction = (out + Vec3::Y * EJECT_LIFT).normalize_or(Vec3::Y);
    direction * EJECT_MPS
}

/// The upward share of an ejected weapon's direction against its outward one.
const EJECT_LIFT: f32 = 0.4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ejected_weapon_leaves_outward_and_a_little_up() {
        let character = Vec3::new(10.0, 2.0, -5.0);
        let v = away_from(character, character + Vec3::new(0.3, 1.4, 0.2));
        assert!((v.length() - EJECT_MPS).abs() < 1e-4);
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
