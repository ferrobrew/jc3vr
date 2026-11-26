//! The shot in progress on this thread: the weapon handling marks a shot of a gun held in a hand
//! around the weapon's fire call ([`begin`] and [`end`]), so that the weapon's fire rumble spawned
//! inside it goes to that hand, raised by the recoil the shot has kicked so far
//! ([`crate::controllers::haptics::rumble`]).

use std::cell::Cell;

use jc3gi::equipment::weapon::WeaponBase;
use jc3vr_input::frame::Hand;

/// A shot in progress on this thread.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Shot {
    /// The firing weapon's address, live for the length of its fire call.
    pub weapon: usize,
    /// The hand the firing gun rides, when it rides one.
    pub hand: Option<Hand>,
    /// The weapon's recoil pitch when the shot began, radians.
    pub pitch: f32,
    /// The weapon's recoil yaw when the shot began, radians.
    pub yaw: f32,
    /// Whether the shot's fire rumble was routed to its hand.
    pub rumbled: bool,
}

impl Shot {
    /// The recoil `weapon` has added since the shot began, `(pitch, yaw)` radians.
    pub fn kick(&self, weapon: &WeaponBase) -> (f32, f32) {
        let pitch = weapon.m_RecoilPitch - self.pitch;
        let yaw = weapon.m_RecoilYaw - self.yaw;
        if pitch.is_finite() && yaw.is_finite() {
            (pitch, yaw)
        } else {
            (0.0, 0.0)
        }
    }
}

/// Mark `shot` as the shot in progress on this thread, before the weapon's fire call, and return
/// the shot it interrupts, for [`end`].
pub fn begin(shot: Shot) -> Option<Shot> {
    SHOT.replace(Some(shot))
}

/// End the shot in progress on this thread, after the weapon's fire call, restoring `outer` (what
/// [`begin`] returned), and return the shot as the fire rumble left it.
pub fn end(outer: Option<Shot>) -> Option<Shot> {
    SHOT.replace(outer)
}

/// The hand of the shot in progress on this thread and the recoil kick so far, radians, when a
/// shot of a gun held in a hand is in progress. Marks the shot's fire rumble routed.
pub(crate) fn rumble_in_shot() -> Option<(Hand, f32)> {
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

thread_local! {
    static SHOT: Cell<Option<Shot>> = const { Cell::new(None) };
}
