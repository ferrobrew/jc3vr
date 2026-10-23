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
//! placed from, for the long gun in each hand. There is one aiming eye, so one gun is lined up with
//! it: the one raised to it ([`raised`]), whose sights take the most hold of the eye. The other
//! hand's gun stays where its hand holds it.

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

/// A long gun in a hand, as the input tick measured it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AdsWeapon {
    pub holder: Hand,
    pub sight: WristSight,
    pub scoped: bool,
}

/// The aim's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AdsStatus {
    /// Whether a long gun's sight line is known, in either hand.
    pub weapon: bool,
    /// Whether the gun raised to the eye is scoped, or with none raised, whether either is.
    pub scoped: bool,
    /// The hand whose gun is raised to the eye on the last pose.
    pub raised: Option<Hand>,
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

/// Note the long gun in each hand for the poses that follow, by [`Hand::index`], or `None` for a
/// hand without one. A gun is taken only in its own hand's place.
pub(crate) fn observe(weapons: [Option<AdsWeapon>; 2]) {
    let weapons = Hand::BOTH.map(|hand| weapons[hand.index()].filter(|w| w.holder == hand));
    let mut state = STATE.lock();
    state.weapons = weapons;
    let any = weapons.iter().flatten();
    state.status.weapon = any.clone().next().is_some();
    if !state.status.weapon {
        state.status.raised = None;
        state.status.scoped = false;
        state.status.shoulder = 0.0;
        state.status.eye = 0.0;
        state.status.weld = 0.0;
        state.status.eye_offset_m = None;
        return;
    }
    state.status.scoped = match state.status.raised.and_then(|h| weapons[h.index()]) {
        Some(raised) => raised.scoped,
        None => any.into_iter().any(|w| w.scoped),
    };
}

/// Pose the hands holding a long gun for its sights: the gun raised to the eye, with the hand on
/// its foregrip. `head` is the eyes' midpoint, and `shoulders` each shoulder joint, by
/// [`Hand::index`], in the cockpit frame; `torso` turns the torso's frame into the cockpit's.
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
    let weapons = STATE.lock().weapons;
    if weapons.iter().all(Option::is_none) {
        return;
    }
    let brace = foregrip::status().hold.filter(|h| h.engaged);
    // The hand on the foregrip of the gun in `holder`.
    let support_hand = |holder: Hand| {
        brace
            .filter(|h| h.holder == holder)
            .map(|h| h.holder.other())
    };
    let solutions = Hand::BOTH.map(|hand| {
        let weapon = weapons[hand.index()]?;
        let holder = hands[hand.index()]?;
        let support = support_hand(hand)
            .and_then(|h| hands[h.index()])
            .map(|h| h.grip.position);
        let inputs = AdsInputs {
            holder,
            sight: weapon.sight,
            support,
            eye: aiming_eye(head, hand, config),
            forward: head.orientation * Vec3::NEG_Z,
            pocket: shoulders[hand.index()]
                .map(|s| s + torso * pocket_offset(hand, config.pocket_offset_m)),
            scoped: weapon.scoped,
        };
        Some(solve::solve(&inputs, config))
    });
    let Some(hand) = raised(solutions) else {
        record(None, &AdsSolution::NONE);
        return;
    };
    let Some(solution) = solutions[hand.index()] else {
        return;
    };
    record(Some(hand), &solution);
    if let Some(held) = hands[hand.index()] {
        hands[hand.index()] = Some(solution.moved.hand(held));
    }
    if let Some(h) = support_hand(hand)
        && let Some(bracing) = hands[h.index()]
    {
        hands[h.index()] = Some(solution.moved.hand(bracing));
    }
}

/// The hand whose gun is raised to the eye, of the guns' solved poses by [`Hand::index`]: the one
/// whose sights take the most hold of the eye (the iron sights or the scope's weld), then whose
/// stock takes the most hold of the shoulder, then whose sight line passes nearest the eye. A gun
/// whose pose takes no hold at all is not raised.
pub fn raised(solutions: [Option<AdsSolution>; 2]) -> Option<Hand> {
    let hold = |s: &AdsSolution| s.eye.max(s.weld).max(s.shoulder);
    let rank = |s: &AdsSolution| {
        (
            s.eye.max(s.weld),
            s.shoulder,
            -s.eye_offset_m.unwrap_or(f32::INFINITY),
        )
    };
    Hand::BOTH
        .into_iter()
        .filter_map(|hand| Some((hand, solutions[hand.index()]?)))
        .filter(|(_, s)| hold(s) > 0.0)
        .max_by(|(_, a), (_, b)| {
            let (a, b) = (rank(a), rank(b));
            a.0.total_cmp(&b.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.2.total_cmp(&b.2))
        })
        .map(|(hand, _)| hand)
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
    /// Each hand's long gun, by [`Hand::index`].
    weapons: [Option<AdsWeapon>; 2],
    status: AdsStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    weapons: [None; 2],
    status: AdsStatus {
        weapon: false,
        scoped: false,
        raised: None,
        shoulder: 0.0,
        eye: 0.0,
        weld: 0.0,
        eye_offset_m: None,
        sighted: 0,
        shouldered: 0,
        welded: 0,
    },
});

/// Note the solution of the gun in `raised`, counting and logging each step as it takes hold.
fn record(raised: Option<Hand>, solution: &AdsSolution) {
    let mut state = STATE.lock();
    let s = &mut state.status;
    s.raised = raised;
    let rose = |was: f32, now: f32| was < TAKEN && now >= TAKEN;
    if rose(s.eye, solution.eye) {
        s.sighted += 1;
        tracing::info!(target: "controllers", hand = ?raised, "ads: the sights are on the eye");
    }
    if rose(s.shoulder, solution.shoulder) {
        s.shouldered += 1;
        tracing::info!(target: "controllers", hand = ?raised, "ads: the stock is in the shoulder");
    }
    if rose(s.weld, solution.weld) {
        s.welded += 1;
        tracing::info!(target: "controllers", hand = ?raised, "ads: the scope is on the eye");
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

    fn holding(eye: f32, weld: f32, shoulder: f32, offset: Option<f32>) -> AdsSolution {
        AdsSolution {
            eye,
            weld,
            shoulder,
            eye_offset_m: offset,
            ..AdsSolution::NONE
        }
    }

    #[test]
    fn the_gun_raised_to_the_eye_is_the_one_lined_up() {
        let (left, right) = (Hand::Left.index(), Hand::Right.index());
        let mut both = [None; 2];
        // A gun held low takes no hold, and is never raised.
        both[left] = Some(AdsSolution::NONE);
        assert_eq!(raised(both), None);
        both[right] = Some(holding(0.6, 0.0, 0.0, Some(0.03)));
        assert_eq!(raised(both), Some(Hand::Right));
        // The sights' hold of the eye wins over the stock's of the shoulder.
        both[left] = Some(holding(0.0, 0.0, 1.0, None));
        assert_eq!(raised(both), Some(Hand::Right));
        // A scope's weld counts as the sights'.
        both[left] = Some(holding(0.0, 0.9, 0.0, Some(0.01)));
        assert_eq!(raised(both), Some(Hand::Left));
        // Both on the eye: the stock, then the sight line nearer the eye.
        both = [
            Some(holding(1.0, 0.0, 0.5, Some(0.01))),
            Some(holding(1.0, 0.0, 0.0, Some(0.0))),
        ];
        assert_eq!(raised(both), Some(Hand::Left));
        both[left] = Some(holding(1.0, 0.0, 0.0, Some(0.01)));
        assert_eq!(raised(both), Some(Hand::Right));
        // A lone gun at the shoulder is raised.
        assert_eq!(
            raised([None, Some(holding(0.0, 0.0, 0.2, None))]),
            Some(Hand::Right)
        );
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
