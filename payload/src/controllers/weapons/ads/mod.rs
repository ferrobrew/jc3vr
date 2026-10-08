//! Aiming a long gun down its sights: the plan of record's true aiming down sights
//! (`docs/mod/input/physical-weapons.md`, "Aiming down the sights").
//!
//! A long gun raised to the eye is lined up with it ([`solve`]): its stock seats in the shoulder
//! pocket and the gun pivots about it, its sight line turns through the aiming eye, and a scope
//! moves across its axis onto the eye (a cheek weld). The gun aim already shoots along the gun's
//! own sight line, so with the sights on the eye the shot goes where the player sees them point;
//! nothing aims the gun toward a target.
//!
//! The pose is applied in [`apply`], on the cockpit-frame hands every hand subsystem starts from
//! ([`crate::controllers::hands::active_hands`]), after the two-handed pose and the weight, so the
//! IK targets, the render-time late update, the sight line the gun aim casts, and the scope's lens
//! all see the gun where the eye has it. The holding hand rides the gun, and the hand on the
//! foregrip with it.
//!
//! The gun's sight line in the holding wrist's frame is measured on the input tick with the
//! foregrip's geometry ([`observe`]), from the gun's last placement and the wrist target it was
//! placed from.

pub mod config;
pub mod solve;

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::CockpitHand,
    weapons::{
        ads::{
            config::{AdsConfig, AimingEye},
            solve::{AdsInputs, AdsSolution, WristSight},
        },
        foregrip,
    },
};

/// The long gun the hands hold, as the input tick measured it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AdsWeapon {
    pub holder: Hand,
    pub sight: WristSight,
    pub scoped: bool,
}

/// The aim's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AdsStatus {
    /// Whether a long gun's sight line is known.
    pub weapon: bool,
    pub scoped: bool,
    /// The last pose's share of each step, `0..=1`.
    pub shoulder: f32,
    pub eye: f32,
    pub weld: f32,
    /// How far the eye was off the sight line, metres, while it was behind the gun.
    pub eye_offset_m: Option<f32>,
    /// Times the sights came onto the eye, the stock into the shoulder, and a scope onto the eye.
    pub sighted: u64,
    pub shouldered: u64,
    pub welded: u64,
}

pub fn status() -> AdsStatus {
    STATE.lock().status
}

/// Note the held long gun for the poses that follow, or `None` without one.
pub(crate) fn observe(weapon: Option<AdsWeapon>) {
    let mut state = STATE.lock();
    state.weapon = weapon;
    state.status.weapon = weapon.is_some();
    state.status.scoped = weapon.is_some_and(|w| w.scoped);
    if weapon.is_none() {
        state.status.shoulder = 0.0;
        state.status.eye = 0.0;
        state.status.weld = 0.0;
        state.status.eye_offset_m = None;
    }
}

/// Pose the hands holding a long gun for its sights. `head` is the eyes' midpoint, and `shoulders`
/// each shoulder joint, by [`Hand::index`], in the cockpit frame; `torso` turns the torso's frame
/// into the cockpit's.
pub fn apply(
    hands: &mut [Option<CockpitHand>; 2],
    head: Pose,
    shoulders: [Option<Vec3>; 2],
    torso: glam::Quat,
    config: &AdsConfig,
) {
    if !config.enabled {
        return;
    }
    let Some(weapon) = STATE.lock().weapon else {
        return;
    };
    let Some(holder) = hands[weapon.holder.index()] else {
        return;
    };
    let support_hand = foregrip::status()
        .hold
        .filter(|h| h.engaged && h.holder == weapon.holder)
        .map(|h| h.holder.other());
    let support = support_hand
        .and_then(|h| hands[h.index()])
        .map(|h| h.grip.position);
    let inputs = AdsInputs {
        holder,
        sight: weapon.sight,
        support,
        eye: aiming_eye(head, weapon.holder, config),
        forward: head.orientation * Vec3::NEG_Z,
        pocket: shoulders[weapon.holder.index()]
            .map(|s| s + torso * pocket_offset(weapon.holder, config.pocket_offset_m)),
        scoped: weapon.scoped,
    };
    let solution = solve::solve(&inputs, config);
    record(&solution);
    hands[weapon.holder.index()] = Some(solution.moved.hand(holder));
    if let Some(h) = support_hand
        && let Some(hand) = hands[h.index()]
    {
        hands[h.index()] = Some(solution.moved.hand(hand));
    }
}

/// The aiming eye's position: the eyes' midpoint moved half the eyes' spacing to the chosen side.
pub fn aiming_eye(head: Pose, holder: Hand, config: &AdsConfig) -> Vec3 {
    let side = match config.eye {
        AimingEye::HolderSide => holder,
        AimingEye::Left => Hand::Left,
        AimingEye::Right => Hand::Right,
    };
    let sign = match side {
        Hand::Left => -1.0,
        Hand::Right => 1.0,
    };
    head.position + head.orientation * Vec3::new(sign * config.half_ipd_m, 0.0, 0.0)
}

/// The pocket's offset from `hand`'s shoulder in the torso's frame: the configured right-side
/// offset, mirrored for the left.
pub fn pocket_offset(hand: Hand, right: Vec3) -> Vec3 {
    match hand {
        Hand::Left => Vec3::new(-right.x, right.y, right.z),
        Hand::Right => right,
    }
}

/// A step counts as taken once its share passes this.
const TAKEN: f32 = 0.99;

#[derive(Debug, Copy, Clone, PartialEq)]
struct State {
    weapon: Option<AdsWeapon>,
    status: AdsStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    weapon: None,
    status: AdsStatus {
        weapon: false,
        scoped: false,
        shoulder: 0.0,
        eye: 0.0,
        weld: 0.0,
        eye_offset_m: None,
        sighted: 0,
        shouldered: 0,
        welded: 0,
    },
});

/// Note the solution, counting and logging each step as it takes hold.
fn record(solution: &AdsSolution) {
    let mut state = STATE.lock();
    let s = &mut state.status;
    let rose = |was: f32, now: f32| was < TAKEN && now >= TAKEN;
    if rose(s.eye, solution.eye) {
        s.sighted += 1;
        tracing::info!(target: "controllers", "ads: the sights are on the eye");
    }
    if rose(s.shoulder, solution.shoulder) {
        s.shouldered += 1;
        tracing::info!(target: "controllers", "ads: the stock is in the shoulder");
    }
    if rose(s.weld, solution.weld) {
        s.welded += 1;
        tracing::info!(target: "controllers", "ads: the scope is on the eye");
    }
    s.shoulder = solution.shoulder;
    s.eye = solution.eye;
    s.weld = solution.weld;
    s.eye_offset_m = solution.eye_offset_m;
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn the_aiming_eye_is_on_the_chosen_side() {
        let head = Pose {
            position: Vec3::new(0.0, 1.7, 0.0),
            orientation: Quat::IDENTITY,
        };
        let config = AdsConfig::new();
        assert!(aiming_eye(head, Hand::Right, &config).x > 0.0);
        assert!(aiming_eye(head, Hand::Left, &config).x < 0.0);
        let left = AdsConfig {
            eye: AimingEye::Left,
            ..config
        };
        assert!(aiming_eye(head, Hand::Right, &left).x < 0.0);
    }

    #[test]
    fn the_pocket_mirrors_for_the_left_shoulder() {
        let right = Vec3::new(-0.06, -0.04, -0.04);
        assert_eq!(pocket_offset(Hand::Right, right), right);
        assert_eq!(
            pocket_offset(Hand::Left, right),
            Vec3::new(0.06, -0.04, -0.04)
        );
    }
}
