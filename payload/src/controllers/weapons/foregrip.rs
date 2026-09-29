//! The two-handed weapon's foregrip: the off-hand grabs it, and the line between the hands then
//! sets the weapon's direction.
//!
//! A two-handed weapon rides the hand the game attaches it to (the holding hand) and aims by that
//! hand alone. The foregrip is a grab volume for the other hand around the weapon's own grip point
//! for that hand (`WeaponBase::GetGripPosition`), or, for a weapon without one, a point along the
//! barrel. While the off-hand holds it:
//!
//! - the holding hand's poses turn about its grip so that the barrel lies along the line from the
//!   holding hand to the off-hand;
//! - the off-hand's poses move so that its grip lands on the foregrip, keeping the controller's
//!   rotation;
//! - [`two_hand_grip`] reports the grip, for the recoil spring to stiffen under.
//!
//! Both happen in [`apply`], on the cockpit-frame hands every hand subsystem starts from
//! ([`crate::controllers::hands::active_hands`]), so the IK targets, the render-time late update,
//! the gun's sight line, and the overlay all see the same two-handed pose. The engine's own
//! off-hand grip IK stays suppressed ([`crate::controllers::hands::secondary`]): the mod places the
//! off-hand itself, on the `MAIN` pass with the rest of the arm, so the late update corrects it
//! like any other controller-driven hand.
//!
//! The weapon's geometry in the holding wrist's frame (the grip point and the barrel axis) is
//! measured on the input tick from the gun's last placement and the wrist target it was placed from
//! ([`GripGeometry`]), and frozen for the length of a hold.
//!
//! The grip ends on the off-hand's next grip press, when the off-hand moves further than the release
//! distance from the foregrip, or when the weapon leaves the hands. Engaging and releasing blend
//! over the configured time.

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::{CockpitHand, pose as hand_pose},
    weapons::config::ForegripConfig,
};

/// The weapon's geometry in the holding hand's wrist frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripGeometry {
    /// Where the off-hand's grip lands.
    pub foregrip: Vec3,
    /// The barrel's direction, a unit vector.
    pub barrel: Vec3,
    /// Whether the foregrip is the weapon's own grip point rather than the fallback along the
    /// barrel.
    pub native: bool,
}

/// A two-handed hold, engaged or blending out.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Hold {
    /// The hand the weapon rides.
    pub holder: Hand,
    /// The held weapon's address, which ends the hold when it changes.
    pub weapon: usize,
    pub geometry: GripGeometry,
    /// Whether the off-hand holds the foregrip; false while the hold blends out.
    pub engaged: bool,
    /// The share of the two-handed pose, `0..=1`.
    pub weight: f32,
}

/// Why a hold ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// The off-hand pressed its grip again.
    GripPress,
    /// The off-hand moved away from the foregrip.
    MovedAway,
    /// The weapon left the hands, or another weapon was drawn.
    WeaponGone,
    /// A hand lost tracking, the controllers went inert, or the foregrip was switched off.
    HandsGone,
}

impl Release {
    pub const fn label(self) -> &'static str {
        match self {
            Release::GripPress => "grip press",
            Release::MovedAway => "moved away",
            Release::WeaponGone => "weapon gone",
            Release::HandsGone => "hands gone",
        }
    }
}

/// The foregrip's state, for the debug UI and the recoil.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ForegripStatus {
    pub hold: Option<Hold>,
    /// The last release, and how many holds have ended.
    pub last_release: Option<Release>,
    pub grabs: u64,
    pub releases: u64,
}

/// An engaged two-hand grip, for the recoil spring.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TwoHandGrip {
    pub holder: Hand,
    /// The share of the two-handed pose, `0..=1`: one once the grip has fully engaged.
    pub weight: f32,
}

/// The engaged two-hand grip, or `None` while the weapon is held in one hand.
pub fn two_hand_grip() -> Option<TwoHandGrip> {
    STATE
        .lock()
        .hold
        .filter(|h| h.engaged)
        .map(|h| TwoHandGrip {
            holder: h.holder,
            weight: h.weight,
        })
}

pub fn status() -> ForegripStatus {
    *STATE.lock()
}

/// Put the hands into the two-handed pose of the current hold, if any. `hands` are the
/// cockpit-frame hands indexed by [`Hand::index`].
pub fn apply(hands: &mut [Option<CockpitHand>; 2], config: &ForegripConfig) {
    let Some(hold) = STATE.lock().hold else {
        return;
    };
    let (Some(holder), Some(off)) = (
        hands[hold.holder.index()],
        hands[hold.holder.other().index()],
    ) else {
        return;
    };
    if let Some((holder, off)) =
        two_hand(holder, off, &hold.geometry, hold.weight, config.min_span_m)
    {
        hands[hold.holder.index()] = Some(holder);
        hands[hold.holder.other().index()] = Some(off);
    }
}

/// The foregrip's point in the frame `holder` is given in.
pub fn foregrip_point(holder: &CockpitHand, geometry: &GripGeometry) -> Vec3 {
    hand_pose::mul(
        holder.wrist,
        Pose {
            position: geometry.foregrip,
            orientation: Quat::IDENTITY,
        },
    )
    .position
}

/// The two-handed pose at `weight`: the holding hand turned about its grip so its barrel lies along
/// the line from its grip to the off-hand's, and the off-hand moved onto the foregrip. `None` when
/// the hands are closer than `min_span` or the geometry is degenerate, leaving the hands as they
/// are.
pub fn two_hand(
    holder: CockpitHand,
    off: CockpitHand,
    geometry: &GripGeometry,
    weight: f32,
    min_span: f32,
) -> Option<(CockpitHand, CockpitHand)> {
    let weight = weight.clamp(0.0, 1.0);
    let pivot = holder.grip.position;
    let line = (off.grip.position - pivot).try_normalize()?;
    if off.grip.position.distance(pivot) < min_span {
        return None;
    }
    let barrel = (holder.wrist.orientation * geometry.barrel).try_normalize()?;
    let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(barrel, line), weight);
    let rotate = |p: Pose| Pose {
        position: pivot + turn * (p.position - pivot),
        orientation: (turn * p.orientation).normalize(),
    };
    let holder = CockpitHand {
        wrist: rotate(holder.wrist),
        grip: rotate(holder.grip),
        aim: rotate(holder.aim),
    };
    let shift = (foregrip_point(&holder, geometry) - off.grip.position) * weight;
    let moved = |p: Pose| Pose {
        position: p.position + shift,
        orientation: p.orientation,
    };
    let off = CockpitHand {
        wrist: moved(off.wrist),
        grip: moved(off.grip),
        aim: moved(off.aim),
    };
    Some((holder, off))
}

/// What the input tick knows about the hands and the weapon for the foregrip.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Inputs {
    /// Whether the foregrip may act: the toggle, the controller scheme, and the hands active.
    pub active: bool,
    /// The raw cockpit-frame hands, before any two-handed pose.
    pub hands: [Option<CockpitHand>; 2],
    /// The two-handed weapon in the hands and its measured geometry.
    pub weapon: Option<TwoHandedWeapon>,
    /// Whether the off-hand pressed its grip in the foregrip volume this tick.
    pub grab_pressed: bool,
}

/// A two-handed weapon in the hands.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TwoHandedWeapon {
    pub holder: Hand,
    pub address: usize,
    /// `None` until the geometry has been measured.
    pub geometry: Option<GripGeometry>,
}

/// What a tick did to the hold.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Transition {
    Grabbed,
    Released(Release),
}

/// Advance the hold by one input tick of `dt` seconds.
pub fn step(
    hold: Option<Hold>,
    inputs: &Inputs,
    dt: f32,
    config: &ForegripConfig,
) -> (Option<Hold>, Option<Transition>) {
    let rate = if config.blend_s > 0.0 {
        dt.max(0.0) / config.blend_s
    } else {
        1.0
    };
    let release = match (hold.filter(|h| h.engaged), inputs.weapon) {
        (Some(_), _) if !inputs.active => Some(Release::HandsGone),
        (Some(h), Some(w)) if w.address != h.weapon || w.holder != h.holder => {
            Some(Release::WeaponGone)
        }
        (Some(_), None) => Some(Release::WeaponGone),
        (Some(h), Some(_)) => match (
            inputs.hands[h.holder.index()],
            inputs.hands[h.holder.other().index()],
        ) {
            _ if inputs.grab_pressed => Some(Release::GripPress),
            (Some(holder), Some(off)) => moved_away(h, holder, off, config),
            _ => Some(Release::HandsGone),
        },
        (None, _) => None,
    };
    if let Some(release) = release {
        let hold = hold.map(|h| Hold {
            engaged: false,
            ..h
        });
        return (fade(hold, rate), Some(Transition::Released(release)));
    }

    if let Some(h) = hold.filter(|h| h.engaged) {
        let weight = (h.weight + rate).min(1.0);
        return (Some(Hold { weight, ..h }), None);
    }
    let grab = inputs
        .weapon
        .filter(|_| inputs.active && inputs.grab_pressed)
        .and_then(|w| w.geometry.map(|g| (w, g)));
    if let Some((weapon, geometry)) = grab {
        let weight = hold
            .filter(|h| h.weapon == weapon.address && h.holder == weapon.holder)
            .map_or(0.0, |h| h.weight);
        let hold = Hold {
            holder: weapon.holder,
            weapon: weapon.address,
            geometry,
            engaged: true,
            weight: (weight + rate).min(1.0),
        };
        return (Some(hold), Some(Transition::Grabbed));
    }
    (fade(hold, rate), None)
}

/// Whether the off-hand has moved further than the release distance from the foregrip, measured
/// with the holding hand in its full two-handed pose.
fn moved_away(
    hold: Hold,
    holder: CockpitHand,
    off: CockpitHand,
    config: &ForegripConfig,
) -> Option<Release> {
    let holder = two_hand(holder, off, &hold.geometry, 1.0, config.min_span_m)
        .map_or(holder, |(holder, _)| holder);
    let distance = foregrip_point(&holder, &hold.geometry).distance(off.grip.position);
    (distance > config.release_distance_m).then_some(Release::MovedAway)
}

/// A released hold blending out, dropped once its weight reaches zero.
fn fade(hold: Option<Hold>, rate: f32) -> Option<Hold> {
    hold.map(|h| Hold {
        weight: (h.weight - rate).max(0.0),
        ..h
    })
    .filter(|h| h.weight > 0.0)
}

/// Advance the shared hold by one input tick, and record what happened.
pub(super) fn tick(inputs: &Inputs, dt: f32, config: &ForegripConfig) -> Option<Transition> {
    let mut state = STATE.lock();
    let (hold, transition) = step(state.hold, inputs, dt, config);
    state.hold = hold;
    match transition {
        Some(Transition::Grabbed) => state.grabs += 1,
        Some(Transition::Released(release)) => {
            state.releases += 1;
            state.last_release = Some(release);
        }
        None => {}
    }
    transition
}

/// Drop any hold at once, for when the controllers stop acting.
pub(super) fn clear() {
    STATE.lock().hold = None;
}

static STATE: Mutex<ForegripStatus> = Mutex::new(ForegripStatus {
    hold: None,
    last_release: None,
    grabs: 0,
    releases: 0,
});

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn pose(position: Vec3) -> Pose {
        Pose {
            position,
            orientation: Quat::IDENTITY,
        }
    }

    fn hand(position: Vec3) -> CockpitHand {
        CockpitHand {
            wrist: pose(position),
            grip: pose(position),
            aim: pose(position),
        }
    }

    /// A rifle whose barrel runs along the wrist's -Z with the foregrip 0.4 m out along it.
    const RIFLE: GripGeometry = GripGeometry {
        foregrip: Vec3::new(0.0, 0.0, -0.4),
        barrel: Vec3::NEG_Z,
        native: true,
    };

    fn config() -> ForegripConfig {
        ForegripConfig {
            blend_s: 0.0,
            ..ForegripConfig::new()
        }
    }

    fn inputs(off: Vec3, grab: bool) -> Inputs {
        let mut hands = [None; 2];
        hands[Hand::Right.index()] = Some(hand(Vec3::ZERO));
        hands[Hand::Left.index()] = Some(hand(off));
        Inputs {
            active: true,
            hands,
            weapon: Some(TwoHandedWeapon {
                holder: Hand::Right,
                address: 0x1000,
                geometry: Some(RIFLE),
            }),
            grab_pressed: grab,
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn the_barrel_turns_onto_the_line_between_the_hands() {
        let off = Vec3::new(0.3, 0.0, -0.3);
        let (holder, moved) = two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 1.0, 0.05).unwrap();
        let barrel = holder.wrist.orientation * RIFLE.barrel;
        assert!(close(barrel, off.normalize()));
        // The holding hand turns about its grip, which stays put.
        assert!(close(holder.grip.position, Vec3::ZERO));
        // The off-hand lands on the foregrip, along the line, keeping its rotation.
        assert!(close(moved.grip.position, off.normalize() * 0.4));
        assert_eq!(moved.grip.orientation, Quat::IDENTITY);
    }

    #[test]
    fn half_weight_turns_half_way() {
        let off = Vec3::new(0.4, 0.0, 0.0);
        let (holder, _) = two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 0.5, 0.05).unwrap();
        let barrel = holder.wrist.orientation * RIFLE.barrel;
        let angle = barrel.angle_between(Vec3::NEG_Z).to_degrees();
        assert!((angle - 45.0).abs() < 0.1, "{angle}");
    }

    #[test]
    fn hands_too_close_keep_their_poses() {
        let off = Vec3::new(0.01, 0.0, -0.01);
        assert_eq!(
            two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 1.0, 0.05),
            None
        );
    }

    #[test]
    fn a_grab_engages_and_a_second_press_releases() {
        let config = config();
        let near = Vec3::new(0.0, 0.0, -0.4);
        let (hold, t) = step(None, &inputs(near, true), DT, &config);
        assert_eq!(t, Some(Transition::Grabbed));
        let hold = hold.unwrap();
        assert!(hold.engaged);
        assert_eq!(hold.weight, 1.0);

        let (still, t) = step(Some(hold), &inputs(near, false), DT, &config);
        assert_eq!(t, None);
        assert!(still.unwrap().engaged);

        let (gone, t) = step(still, &inputs(near, true), DT, &config);
        assert_eq!(t, Some(Transition::Released(Release::GripPress)));
        assert_eq!(gone, None);
    }

    #[test]
    fn sliding_along_the_barrel_holds_until_the_release_distance() {
        let config = config();
        let near = Vec3::new(0.0, 0.0, -0.4);
        let (hold, _) = step(None, &inputs(near, true), DT, &config);
        // The off-hand slides back along the line: the direction still follows it.
        let slid = Vec3::new(0.0, 0.0, -0.4 + config.release_distance_m * 0.8);
        let (hold, t) = step(hold, &inputs(slid, false), DT, &config);
        assert_eq!(t, None);
        let far = Vec3::new(0.0, 0.0, -0.4 - config.release_distance_m * 1.2);
        let (_, t) = step(hold, &inputs(far, false), DT, &config);
        assert_eq!(t, Some(Transition::Released(Release::MovedAway)));
    }

    #[test]
    fn a_new_weapon_or_a_lost_hand_releases() {
        let config = config();
        let near = Vec3::new(0.0, 0.0, -0.4);
        let (hold, _) = step(None, &inputs(near, true), DT, &config);

        let mut other = inputs(near, false);
        other.weapon.as_mut().unwrap().address = 0x2000;
        assert_eq!(
            step(hold, &other, DT, &config).1,
            Some(Transition::Released(Release::WeaponGone))
        );

        let mut lost = inputs(near, false);
        lost.hands[Hand::Left.index()] = None;
        assert_eq!(
            step(hold, &lost, DT, &config).1,
            Some(Transition::Released(Release::HandsGone))
        );
    }

    #[test]
    fn a_grab_needs_measured_geometry() {
        let mut unmeasured = inputs(Vec3::new(0.0, 0.0, -0.4), true);
        unmeasured.weapon.as_mut().unwrap().geometry = None;
        assert_eq!(step(None, &unmeasured, DT, &config()), (None, None));
    }

    #[test]
    fn the_grip_blends_in_and_out() {
        let config = ForegripConfig {
            blend_s: 0.1,
            ..ForegripConfig::new()
        };
        let near = Vec3::new(0.0, 0.0, -0.4);
        let (hold, _) = step(None, &inputs(near, true), 0.05, &config);
        assert!((hold.unwrap().weight - 0.5).abs() < 1e-5);
        let (hold, _) = step(hold, &inputs(near, false), 0.05, &config);
        assert_eq!(hold.unwrap().weight, 1.0);
        let (hold, _) = step(hold, &inputs(near, true), 0.05, &config);
        let hold = hold.unwrap();
        assert!(!hold.engaged);
        assert!((hold.weight - 0.5).abs() < 1e-5);
        let (hold, _) = step(Some(hold), &inputs(near, false), 0.05, &config);
        assert_eq!(hold, None);
    }
}
