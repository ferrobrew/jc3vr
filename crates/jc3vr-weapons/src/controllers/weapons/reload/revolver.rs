//! A revolver's cylinder swung out and reloaded by hand. Pure.
//!
//! The revolvers' own skeleton carries the crane ([`WeaponData::CHAMBER_BONE_HASH`]) and the
//! cylinder on it ([`WeaponData::CHAMBER_SPIN_BONE_HASH`]). The game's reload clips swing the
//! cylinder out by turning the crane a quarter turn about the barrel's line, and back, so the
//! cylinder here swings the way the clip for the holding hand swings it ([`CRANE`]).
//!
//! - A flick of the gun hand rolling the gun away from the side the cylinder swings to throws it
//!   out; a flick rolling it back throws it shut, and the cylinder spins as it closes.
//! - Tipping the muzzle up with the cylinder out dumps every chamber, spent or not.
//! - A speedloader from the belt pouch seats into the emptied cylinder as it touches it.
//!
//! [`WeaponData::CHAMBER_BONE_HASH`]: jc3gi::equipment::weapon::WeaponData::CHAMBER_BONE_HASH
//! [`WeaponData::CHAMBER_SPIN_BONE_HASH`]: jc3gi::equipment::weapon::WeaponData::CHAMBER_SPIN_BONE_HASH

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Quat, Vec3};
use jc3vr_input::frame::Hand;

use crate::controllers::weapons::reload::tuning::RevolverTuning;

/// The crane's swing, in the gun's model frame, metres, as the revolvers' reload clips move it.
pub(super) struct Crane {
    /// The crane bone's pivot: the line about which it turns runs through it along Z.
    pub pivot: Vec3,
    /// How far the crane turns out, radians.
    pub open_rad: f32,
    /// How far the crane slides along X as it opens, metres, the way it turns.
    pub shift_m: f32,
}

/// The revolvers' crane, from the Peacekeeper's skeleton and its reload clips (decoded with
/// `tools/door_handles/havok.py`): the crane bone sits 78 mm up and 84 mm back in the model, and
/// both clips turn it a quarter turn about Z, with about 5 mm of slide along X, held open.
pub(super) const CRANE: Crane = Crane {
    pivot: Vec3::new(0.0, 0.0782, -0.0838),
    open_rad: FRAC_PI_2,
    shift_m: 0.005,
};

/// The sign of the crane's turn about the model's +Z for a gun in `hand`: the left-hand clip turns
/// it positively, sending the cylinder to the model's -X, and the right-hand clip the other way.
pub(super) fn swing_sign(hand: Hand) -> f32 {
    match hand {
        Hand::Left => 1.0,
        Hand::Right => -1.0,
    }
}

/// What changed in a cylinder over a tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum CylinderEvent {
    Opened,
    Closed,
    /// The muzzle tipped up with the cylinder out: every chamber falls out.
    Dumped,
}

/// One gun hand's revolver cylinder.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Cylinder {
    /// The weapon whose cylinder it is (a `WeaponBase` address).
    pub weapon: usize,
    pub hand: Hand,
    /// How far out the cylinder is, `0..=1`.
    pub open: f32,
    /// Whether it is swinging out (or out) rather than shut.
    pub opening: bool,
    /// Whether every chamber is empty: dumped and not loaded since.
    pub empty: bool,
    /// The seconds since the cylinder began to spin shut, while it spins.
    pub spin_s: Option<f32>,
    last: Option<Quat>,
    cooldown_s: f32,
}

/// What a cylinder steps on: the gun's model frame's orientation in the cockpit frame, when
/// measured, and the tick's length, seconds.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct CylinderInputs {
    pub orientation: Option<Quat>,
    pub dt: f32,
}

impl Cylinder {
    pub(super) fn new(weapon: usize, hand: Hand) -> Self {
        Self {
            weapon,
            hand,
            open: 0.0,
            opening: false,
            empty: false,
            spin_s: None,
            last: None,
            cooldown_s: 0.0,
        }
    }

    /// Whether the cylinder is out far enough to dump and to take a speedloader.
    pub(super) fn is_out(&self) -> bool {
        self.opening && self.open >= 1.0
    }

    /// Whether the cylinder is shut, so the gun may fire.
    pub(super) fn is_shut(&self) -> bool {
        !self.opening && self.open <= 0.0
    }

    /// Whether the cylinder takes a speedloader: out, with every chamber empty.
    pub(super) fn takes_speedloader(&self) -> bool {
        self.is_out() && self.empty
    }

    /// A speedloader seated: the chambers are full again.
    pub(super) fn loaded(&mut self) {
        self.empty = false;
    }

    /// Step the cylinder by a tick.
    pub(super) fn step(
        &mut self,
        inputs: CylinderInputs,
        tuning: &RevolverTuning,
    ) -> Option<CylinderEvent> {
        let dt = if inputs.dt.is_finite() {
            inputs.dt.max(0.0)
        } else {
            0.0
        };
        self.cooldown_s = (self.cooldown_s - dt).max(0.0);
        self.spin_s = self.spin_s.map(|s| s + dt).filter(|s| *s < tuning.spin_s);
        let roll = inputs
            .orientation
            .zip(self.last)
            .and_then(|(now, last)| roll_dps(last, now, dt));
        self.last = inputs.orientation;
        let mut event = None;
        // Rolling the gun away from the side the cylinder swings to throws it out.
        let away = roll.map_or(0.0, |r| -r * swing_sign(self.hand));
        if self.cooldown_s <= 0.0 {
            if !self.opening && away >= tuning.flick_dps {
                self.opening = true;
                self.cooldown_s = tuning.flick_cooldown_s;
                event = Some(CylinderEvent::Opened);
            } else if self.opening && -away >= tuning.flick_dps {
                self.opening = false;
                self.cooldown_s = tuning.flick_cooldown_s;
                self.spin_s = Some(0.0);
                event = Some(CylinderEvent::Closed);
            }
        }
        self.open = if self.opening {
            (self.open + dt / tuning.open_s.max(1e-3)).min(1.0)
        } else {
            (self.open - dt / tuning.close_s.max(1e-3)).max(0.0)
        };
        if event.is_none()
            && self.is_out()
            && !self.empty
            && inputs
                .orientation
                .is_some_and(|o| muzzle_up(o, tuning.dump_elevation_deg))
        {
            self.empty = true;
            event = Some(CylinderEvent::Dumped);
        }
        event
    }

    /// The crane's turn about the model's +Z, radians.
    pub(super) fn crane_rad(&self) -> f32 {
        swing_sign(self.hand) * CRANE.open_rad * smoothstep(self.open)
    }

    /// The crane's slide along the model's X, metres.
    pub(super) fn crane_shift_m(&self) -> f32 {
        -swing_sign(self.hand) * CRANE.shift_m * smoothstep(self.open)
    }

    /// The cylinder's spin about its axis, radians: easing out over the spin, and none at rest.
    pub(super) fn spin_rad(&self, tuning: &RevolverTuning) -> f32 {
        self.spin_s.map_or(0.0, |s| {
            let t = (s / tuning.spin_s.max(1e-3)).clamp(0.0, 1.0);
            -swing_sign(self.hand) * tuning.spin_turns * TAU * (1.0 - (1.0 - t) * (1.0 - t))
        })
    }

    /// A model-frame point on the cylinder (on the crane) where the crane has it now.
    pub(super) fn carried(&self, point: Vec3) -> Vec3 {
        let pivot = Vec3::new(CRANE.pivot.x, CRANE.pivot.y, point.z);
        pivot
            + Quat::from_rotation_z(self.crane_rad()) * (point - pivot)
            + Vec3::X * self.crane_shift_m()
    }
}

/// The gun's roll about its barrel's line (the model's +Z) between two orientations `dt` apart,
/// degrees per second.
fn roll_dps(last: Quat, now: Quat, dt: f32) -> Option<f32> {
    if dt <= 1e-4 {
        return None;
    }
    let (axis, angle) = (last.inverse() * now).normalize().to_axis_angle();
    let angle = if angle > std::f32::consts::PI {
        angle - TAU
    } else {
        angle
    };
    let roll = axis.z * angle / dt;
    roll.is_finite().then(|| roll.to_degrees())
}

/// Whether a gun whose model frame is at `orientation` points its muzzle (the model's -Z) at least
/// `elevation_deg` above the horizon.
fn muzzle_up(orientation: Quat, elevation_deg: f32) -> bool {
    (orientation * Vec3::NEG_Z).y >= elevation_deg.to_radians().sin()
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::weapons::reload::tuning::RELOAD;

    const DT: f32 = 1.0 / 30.0;

    fn revolver_tuning() -> &'static RevolverTuning {
        &RELOAD.get().revolver
    }

    /// Step `cylinder` with the gun rolled `degrees` about its barrel over one tick from `from`,
    /// returning the event and the new orientation.
    fn roll(cylinder: &mut Cylinder, from: Quat, degrees: f32) -> (Option<CylinderEvent>, Quat) {
        let to = from * Quat::from_rotation_z(degrees.to_radians());
        (
            cylinder.step(
                CylinderInputs {
                    orientation: Some(to),
                    dt: DT,
                },
                revolver_tuning(),
            ),
            to,
        )
    }

    fn hold(cylinder: &mut Cylinder, at: Quat, ticks: usize) -> Vec<CylinderEvent> {
        (0..ticks)
            .filter_map(|_| {
                cylinder.step(
                    CylinderInputs {
                        orientation: Some(at),
                        dt: DT,
                    },
                    revolver_tuning(),
                )
            })
            .collect()
    }

    #[test]
    fn a_flick_away_from_the_swing_opens_and_one_back_closes_with_a_spin() {
        for hand in Hand::BOTH {
            let mut c = Cylinder::new(1, hand);
            let level = Quat::IDENTITY;
            assert!(hold(&mut c, level, 2).is_empty());
            // 30 degrees in a tick is 900 degrees per second.
            let away = -swing_sign(hand) * 30.0;
            // A flick toward the swing side does nothing while shut.
            let (event, at) = roll(&mut c, level, -away);
            assert_eq!(event, None);
            assert!(hold(&mut c, at, 12).is_empty());
            let (event, at) = roll(&mut c, at, away);
            assert_eq!(event, Some(CylinderEvent::Opened), "{hand:?}");
            assert!(hold(&mut c, at, 12).is_empty());
            assert!(c.is_out());
            assert!((c.crane_rad().abs() - FRAC_PI_2).abs() < 1e-4);
            assert_eq!(c.crane_rad().signum(), swing_sign(hand));
            let (event, at) = roll(&mut c, at, -away);
            assert_eq!(event, Some(CylinderEvent::Closed), "{hand:?}");
            assert!(c.spin_s.is_some());
            assert!(hold(&mut c, at, 30).is_empty());
            assert!(c.is_shut());
            assert_eq!(c.spin_s, None);
            assert_eq!(c.spin_rad(revolver_tuning()), 0.0);
        }
    }

    #[test]
    fn a_slow_roll_is_no_flick() {
        let mut c = Cylinder::new(1, Hand::Right);
        let mut at = Quat::IDENTITY;
        for _ in 0..20 {
            // 10 degrees a tick is 300 degrees per second.
            let (event, next) = roll(&mut c, at, 10.0);
            assert_eq!(event, None);
            at = next;
        }
    }

    #[test]
    fn tipping_the_muzzle_up_with_the_cylinder_out_dumps_it_once() {
        let mut c = Cylinder::new(1, Hand::Right);
        let level = Quat::IDENTITY;
        let up = Quat::from_rotation_x(60_f32.to_radians());
        assert!((up * Vec3::NEG_Z).y > 0.8);
        // Shut, the muzzle up does nothing.
        hold(&mut c, level, 1);
        assert!(hold(&mut c, up, 3).is_empty());
        hold(&mut c, level, 1);
        let (event, open) = roll(&mut c, level, 30.0);
        assert_eq!(event, Some(CylinderEvent::Opened));
        assert!(hold(&mut c, open, 10).is_empty());
        assert!(!c.takes_speedloader());
        let up = open * Quat::from_rotation_x(60_f32.to_radians());
        assert_eq!(hold(&mut c, up, 5), vec![CylinderEvent::Dumped]);
        assert!(c.takes_speedloader());
        c.loaded();
        assert!(!c.takes_speedloader());
    }

    #[test]
    fn the_cylinder_s_rear_face_swings_out_to_the_side_of_its_clip() {
        let mouth = Vec3::new(0.0, 0.1067, -0.0553);
        let mut c = Cylinder::new(1, Hand::Left);
        assert!(c.carried(mouth).distance(mouth) < 1e-6);
        c.opening = true;
        c.open = 1.0;
        let out = c.carried(mouth);
        // A quarter turn about the pivot puts the cylinder beside it, on the left-hand clip's
        // side (-X), at the same depth.
        assert!((out.y - CRANE.pivot.y).abs() < 1e-4, "{out}");
        assert!(out.x < -0.02, "{out}");
        assert!((out.z - mouth.z).abs() < 1e-6, "{out}");
    }
}
