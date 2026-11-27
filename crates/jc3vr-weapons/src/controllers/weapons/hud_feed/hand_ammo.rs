//! Each hand's gun as the input tick samples it for the HUD's ammunition counter
//! (`hud::anchored`): its magazine, its reserve, and its model's box.
//!
//! The input tick samples each hand's gun ([`observe`], on the game thread where the table keeps
//! the guns alive), and the HUD reads the latest sample ([`sampled`]) on the render thread. The
//! sample carries each gun's model box, so the HUD can place the counter over the middle of the
//! drawn gun.

use glam::Vec3;
use jc3gi::equipment::weapon::WeaponBase;
use parking_lot::Mutex;

use crate::controllers::weapons::{engine::reserve, regions::engine::model_box};

/// A gun model's box in the gun's own frame (+Y up, -Z toward the muzzle, the origin at the
/// pistol grip), metres.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ModelBox {
    pub min: Vec3,
    pub max: Vec3,
}

/// One hand's gun as the input tick read it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandAmmo {
    /// The gun's address.
    pub weapon: usize,
    /// The rounds in its magazine.
    pub magazine: i32,
    /// The magazine is out of the gun ([`WeaponBase::m_WeaponIsWithoutMagazine`]).
    pub out: bool,
    /// The rounds of its ammunition its user carries.
    pub reserve: i32,
    /// Its model's box, when it has a model.
    pub model: Option<ModelBox>,
}

/// The input tick's sample: each hand's gun, by [`Hand::index`](jc3vr_input::frame::Hand::index).
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandsAmmo {
    pub hands: [Option<HandAmmo>; 2],
}

/// Sample the gun in each hand, by [`Hand::index`](jc3vr_input::frame::Hand::index), for the HUD.
/// Called each input tick of the weapon handling.
///
/// # Safety
///
/// Each address must be a live weapon of the local player, read on the game thread before the
/// world update.
pub(crate) unsafe fn observe(guns: [Option<usize>; 2]) {
    let previous = *SAMPLE.lock();
    let mut hands = [None; 2];
    for (index, gun) in guns.into_iter().enumerate() {
        let Some(address) = gun else { continue };
        let Some(weapon) = (unsafe { (address as *const WeaponBase).as_ref() }) else {
            continue;
        };
        // A gun's model does not change while it is held, so its box is read once per gun.
        let known = previous
            .and_then(|p| p.hands[index])
            .filter(|h| h.weapon == address)
            .and_then(|h| h.model);
        let model =
            known.or_else(|| unsafe { model_box(weapon) }.map(|(min, max)| ModelBox { min, max }));
        hands[index] = Some(HandAmmo {
            weapon: address,
            magazine: weapon.m_MagazineAmmo,
            out: weapon.m_WeaponIsWithoutMagazine,
            reserve: unsafe { reserve(weapon) },
            model,
        });
    }
    *SAMPLE.lock() = Some(HandsAmmo { hands });
}

/// Forget the sample: the hands went inert and the weapon handling stood down.
pub(crate) fn clear() {
    *SAMPLE.lock() = None;
}

/// The last sample.
pub fn sampled() -> Option<HandsAmmo> {
    *SAMPLE.lock()
}

static SAMPLE: Mutex<Option<HandsAmmo>> = Mutex::new(None);
