//! The two-handed weapon's foregrip: the off-hand grabs it, and the line between the hands then
//! sets the weapon's direction.
//!
//! A two-handed weapon rides its holding hand and aims by that hand alone. The foregrip acts on the
//! long gun held in one hand while the other is free, the wielded weapon or one armed in a hand
//! ([`crate::controllers::weapons::engine::lone_gun`]): a brace takes the other hand, so there is
//! one hold at most, keyed by its holding hand and its weapon. The foregrip is a grab volume for
//! the other hand around the weapon's own grip point for that hand (`WeaponBase::GetGripPosition`),
//! or, for a weapon without one, a point along the barrel. While the off-hand holds it:
//!
//! - the holding hand's poses turn about its grip so that the foregrip lies on the line from the
//!   holding hand to the off-hand, up to a limit, and less as the hands close in;
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
//! A pump-action's pump slides along the barrel under the off-hand
//! ([`crate::controllers::weapons::reload::travel`]): the reload publishes how far back the pump
//! is ([`set_pump_slide`]), and the foregrip moves back with it, so the drawn off-hand rides the
//! pump.
//!
//! The grip ends when the off-hand lets go of its grip ([`ForegripConfig::hold`], the default), or
//! with the grip a toggle, on its next grip press; when the off-hand moves further than the release
//! distance from the foregrip; or when the weapon leaves the hands. Engaging and releasing blend
//! over the configured time.

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::{CockpitHand, pose as hand_pose},
    weapons::{config::ForegripConfig, regions::GunRegion},
};

/// The weapon's geometry in the holding hand's wrist frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripGeometry {
    /// Where the off-hand's grip lands.
    pub foregrip: Vec3,
    /// The barrel's direction, a unit vector: the direction of the gun's sight line.
    pub barrel: Vec3,
    /// The sight line's origin.
    pub sight: Vec3,
    /// The first gun's muzzle: its current fire point.
    pub muzzle: Vec3,
    /// Whether the foregrip is the weapon's own grip point rather than the fallback along the
    /// barrel.
    pub native: bool,
    /// The first gun's own frame: its pose on the wrist, with the position in the wrist frame's
    /// units.
    pub gun: Pose,
}

/// A two-handed hold, engaged or blending out.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Hold {
    /// The hand the weapon rides.
    pub holder: Hand,
    /// The held weapon's address, which ends the hold when it changes.
    pub weapon: usize,
    pub geometry: GripGeometry,
    /// The part of the gun the off-hand holds; the foregrip's point is where it closes on it.
    pub region: GunRegion,
    /// Whether the off-hand holds the foregrip; false while the hold blends out.
    pub engaged: bool,
    /// The share of the two-handed pose, `0..=1`.
    pub weight: f32,
}

/// Why a hold ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// The off-hand pressed its grip again, with the grip a toggle.
    GripPress,
    /// The off-hand let go of its grip, with the grip held to hold ([`ForegripConfig::hold`]).
    GripLetGo,
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
            Release::GripLetGo => "grip let go",
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
    /// The turn the last two-handed pose asked of the weapon, before the limit and the blend.
    pub turn: Option<Turn>,
}

/// The turn of one two-handed pose.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Turn {
    /// The angle between where the holding hand points the foregrip and the line to the off-hand,
    /// degrees.
    pub wanted_deg: f32,
    /// Whether the limit ([`ForegripConfig::max_turn_deg`]) held the weapon back.
    pub limited: bool,
    /// The share of the pose the span between the hands lets through, `0..=1`.
    pub span_fade: f32,
}

/// An engaged two-hand grip, for the recoil spring.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TwoHandGrip {
    pub holder: Hand,
    /// The share of the two-handed pose, `0..=1`: one once the grip has fully engaged.
    pub weight: f32,
    /// The part of the gun the off-hand holds.
    pub region: GunRegion,
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
            region: h.region,
        })
}

pub fn status() -> ForegripStatus {
    *STATE.lock()
}

/// Put the hands into the two-handed pose of the current hold, if any. `hands` are the
/// cockpit-frame hands indexed by [`Hand::index`].
pub fn apply(hands: &mut [Option<CockpitHand>; 2], config: &ForegripConfig) {
    let Some(hold) = posed_hold() else {
        return;
    };
    let (Some(holder), Some(off)) = (
        hands[hold.holder.index()],
        hands[hold.holder.other().index()],
    ) else {
        return;
    };
    let pose = hold_pose(&hold, holder, off, config);
    STATE.lock().turn = pose.map(|p| p.turn);
    if let Some(pose) = pose {
        hands[hold.holder.index()] = Some(pose.holder);
        hands[hold.holder.other().index()] = Some(pose.off);
    }
}

/// The current hold's placement geometry, including the pump's current travel.
pub(crate) fn posed_hold() -> Option<Hold> {
    let mut hold = STATE.lock().hold?;
    if let Some((weapon, back_m)) = *PUMP_SLIDE.lock()
        && weapon == hold.weapon
    {
        hold.geometry.foregrip = slid_foregrip(&hold.geometry, back_m);
    }
    Some(hold)
}

/// How far back along the barrel the pump of the weapon at `weapon` is, cockpit metres, or `None`
/// for no pump out. Published by the reload's input tick.
pub(crate) fn set_pump_slide(slide: Option<(usize, f32)>) {
    *PUMP_SLIDE.lock() = slide.filter(|(_, m)| m.is_finite());
}

/// Whether the reload has the pump of the weapon at `weapon` out, so the pump sets where the
/// off-hand is along the barrel.
pub(crate) fn pumping(weapon: usize) -> bool {
    PUMP_SLIDE.lock().is_some_and(|(w, _)| w == weapon)
}

/// The foregrip moved `back_m` back along the barrel, with the pump.
pub fn slid_foregrip(geometry: &GripGeometry, back_m: f32) -> Vec3 {
    geometry.foregrip - geometry.barrel * back_m
}

/// The hands in the pose of `hold`: the two-handed pose for an off-hand that steers the gun, and
/// for one that does not (on the stock), the off-hand moved onto its point on the gun with the
/// holding hand as it is.
pub fn hold_pose(
    hold: &Hold,
    holder: CockpitHand,
    off: CockpitHand,
    config: &ForegripConfig,
) -> Option<TwoHandPose> {
    if hold.region.steers()
        && let Some(pose) = two_hand(holder, off, &hold.geometry, hold.weight, config)
    {
        return Some(pose);
    }
    Some(TwoHandPose {
        holder,
        off: onto_foregrip(&holder, off, &hold.geometry, hold.weight),
        turn: Turn::default(),
    })
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

/// The hands in a two-handed pose.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TwoHandPose {
    pub holder: CockpitHand,
    pub off: CockpitHand,
    pub turn: Turn,
}

/// The two-handed pose at `weight`: the holding hand turned about its grip so that the foregrip
/// lies on the line from its grip to the off-hand's, and the off-hand moved onto the foregrip.
///
/// The weapon turns by the rotation that carries the direction from the holding grip to the
/// foregrip onto that line, the shorter way round, so the weapon sits between the hands as the
/// weapon's own geometry has it (a foregrip below or beside the barrel leaves the barrel off the
/// line by as much) and keeps the roll the holding hand gives it. The turn stops at
/// `config.max_turn_deg`, and it fades out as the hands close in from twice `config.min_span_m` to
/// `config.min_span_m`. `None` when nothing of the pose is left or the geometry is degenerate,
/// leaving the hands as they are.
pub fn two_hand(
    holder: CockpitHand,
    off: CockpitHand,
    geometry: &GripGeometry,
    weight: f32,
    config: &ForegripConfig,
) -> Option<TwoHandPose> {
    let pivot = holder.grip.position;
    let to_off = off.grip.position - pivot;
    let line = to_off.try_normalize()?;
    let pivot_in_wrist = hand_pose::mul(hand_pose::inverse(holder.wrist), holder.grip).position;
    let toward_foregrip =
        (holder.wrist.orientation * (geometry.foregrip - pivot_in_wrist)).try_normalize()?;
    let span_fade = span_fade(to_off.length(), config.min_span_m);
    let contact_weight = weight.clamp(0.0, 1.0);
    let steering_weight = contact_weight * span_fade;
    let full = Quat::from_rotation_arc(toward_foregrip, line);
    let (axis, wanted) = full.to_axis_angle();
    let max = config.max_turn_deg.to_radians().max(0.0);
    let turn_info = Turn {
        wanted_deg: wanted.to_degrees(),
        limited: wanted > max,
        span_fade,
    };
    if contact_weight <= 0.0 || !axis.is_finite() {
        return None;
    }
    let turn = Quat::from_axis_angle(axis, wanted.min(max) * steering_weight);
    let rotate = |p: Pose| Pose {
        position: pivot + turn * (p.position - pivot),
        orientation: (turn * p.orientation).normalize(),
    };
    let holder = CockpitHand {
        wrist: rotate(holder.wrist),
        grip: rotate(holder.grip),
        aim: rotate(holder.aim),
    };
    let off = onto_foregrip(&holder, off, geometry, contact_weight);
    Some(TwoHandPose {
        holder,
        off,
        turn: turn_info,
    })
}

/// The off-hand moved `weight` of the way onto the foregrip of the gun on `holder`, keeping its
/// rotation.
pub fn onto_foregrip(
    holder: &CockpitHand,
    off: CockpitHand,
    geometry: &GripGeometry,
    weight: f32,
) -> CockpitHand {
    let shift = (foregrip_point(holder, geometry) - off.grip.position) * weight.clamp(0.0, 1.0);
    let moved = |p: Pose| Pose {
        position: p.position + shift,
        orientation: p.orientation,
    };
    CockpitHand {
        wrist: moved(off.wrist),
        grip: moved(off.grip),
        aim: moved(off.aim),
    }
}

/// The share of the two-handed pose a span of `span` metres between the grips lets through: none
/// below `min_span`, all of it from twice `min_span`, and a smooth ramp between.
pub fn span_fade(span: f32, min_span: f32) -> f32 {
    if min_span <= 0.0 {
        return if span > 0.0 { 1.0 } else { 0.0 };
    }
    let t = ((span - min_span) / min_span).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    /// Whether the off-hand's grip is held, which keeps a hold under
    /// [`ForegripConfig::hold`].
    pub grip_held: bool,
    /// Where an engaged hold's off-hand has slid to along the gun, in the holding wrist's frame,
    /// and the region it is in there: a hold on a region the hand slides along follows it.
    pub slide: Option<(Vec3, GunRegion)>,
}

/// A two-handed weapon in the hands.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TwoHandedWeapon {
    pub holder: Hand,
    pub address: usize,
    /// `None` until the geometry has been measured. Its foregrip is where a grab this tick closes
    /// on the gun.
    pub geometry: Option<GripGeometry>,
    /// The part of the gun a grab this tick closes on.
    pub region: GunRegion,
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
    let hold = hold.map(|h| slid(h, inputs.slide));
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
            _ if config.hold && !inputs.grip_held => Some(Release::GripLetGo),
            _ if !config.hold && inputs.grab_pressed => Some(Release::GripPress),
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
            region: weapon.region,
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
    let holder = if hold.region.steers() {
        two_hand(holder, off, &hold.geometry, 1.0, config).map_or(holder, |p| p.holder)
    } else {
        holder
    };
    let distance = foregrip_point(&holder, &hold.geometry).distance(off.grip.position);
    (distance > config.release_distance_m).then_some(Release::MovedAway)
}

/// An engaged hold on a region the hand slides along, moved to where the hand has slid to while it
/// stays on such a region.
fn slid(hold: Hold, slide: Option<(Vec3, GunRegion)>) -> Hold {
    match slide {
        Some((point, region))
            if hold.engaged && hold.region.slides() && region.slides() && point.is_finite() =>
        {
            Hold {
                geometry: GripGeometry {
                    foregrip: point,
                    ..hold.geometry
                },
                region,
                ..hold
            }
        }
        _ => hold,
    }
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
        Some(Transition::Grabbed) => {
            state.grabs += 1;
            if let Some(h) = hold {
                tracing::debug!(
                    target: "controllers",
                    holder = ?h.holder,
                    foregrip = ?h.geometry.foregrip,
                    barrel = ?h.geometry.barrel,
                    native = h.geometry.native,
                    "foregrip grabbed"
                );
            }
        }
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
    let mut state = STATE.lock();
    state.hold = None;
    state.turn = None;
    drop(state);
    *PUMP_SLIDE.lock() = None;
}

/// The pump's slide back along the barrel, by weapon; a leaf slot.
static PUMP_SLIDE: Mutex<Option<(usize, f32)>> = Mutex::new(None);

static STATE: Mutex<ForegripStatus> = Mutex::new(ForegripStatus {
    hold: None,
    last_release: None,
    grabs: 0,
    releases: 0,
    turn: None,
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
        sight: Vec3::new(0.0, 0.08, 0.0),
        muzzle: Vec3::new(0.0, 0.05, -0.7),
        native: true,
        gun: Pose::IDENTITY,
    };

    #[test]
    fn a_pump_slides_the_foregrip_back_along_the_barrel() {
        let slid = slid_foregrip(&RIFLE, 0.08);
        assert!((slid - Vec3::new(0.0, 0.0, -0.32)).length() < 1e-6);
        assert_eq!(slid_foregrip(&RIFLE, 0.0), RIFLE.foregrip);
    }

    fn config() -> ForegripConfig {
        ForegripConfig {
            blend_s: 0.0,
            hold: false,
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
                region: GunRegion::Handguard,
            }),
            grab_pressed: grab,
            grip_held: true,
            slide: None,
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn the_barrel_turns_onto_the_line_between_the_hands() {
        let off = Vec3::new(0.3, 0.0, -0.3);
        let pose = two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 1.0, &config()).unwrap();
        let barrel = pose.holder.wrist.orientation * RIFLE.barrel;
        assert!(close(barrel, off.normalize()));
        // The holding hand turns about its grip, which stays put.
        assert!(close(pose.holder.grip.position, Vec3::ZERO));
        // The off-hand lands on the foregrip, along the line, keeping its rotation.
        assert!(close(pose.off.grip.position, off.normalize() * 0.4));
        assert_eq!(pose.off.grip.orientation, Quat::IDENTITY);
    }

    #[test]
    fn half_weight_turns_half_way() {
        let angle = 60f32.to_radians();
        let off = Vec3::new(angle.sin(), 0.0, -angle.cos()) * 0.4;
        let pose = two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 0.5, &config()).unwrap();
        let barrel = pose.holder.wrist.orientation * RIFLE.barrel;
        let turned = barrel.angle_between(Vec3::NEG_Z).to_degrees();
        assert!((turned - 30.0).abs() < 0.1, "{turned}");
    }

    #[test]
    fn hands_too_close_stop_steering_but_still_reach_the_selected_contact() {
        let off = Vec3::new(0.01, 0.0, -0.01);
        let pose = two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 1.0, &config()).unwrap();
        assert_eq!(pose.holder, hand(Vec3::ZERO));
        assert!(close(pose.off.grip.position, RIFLE.foregrip));
        assert_eq!(pose.turn.span_fade, 0.0);
    }

    #[test]
    fn the_pose_fades_out_as_the_hands_close_in() {
        let c = config();
        let span = c.min_span_m * 1.5;
        let off = Vec3::new(span, 0.0, 0.0);
        let pose = two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 1.0, &c).unwrap();
        assert!((pose.turn.span_fade - 0.5).abs() < 1e-5);
        let barrel = pose.holder.wrist.orientation * RIFLE.barrel;
        let turned = barrel.angle_between(Vec3::NEG_Z).to_degrees();
        assert!((turned - c.max_turn_deg * 0.5).abs() < 0.1, "{turned}");
        assert_eq!(span_fade(c.min_span_m * 0.9, c.min_span_m), 0.0);
        assert_eq!(span_fade(c.min_span_m * 2.0, c.min_span_m), 1.0);
    }

    #[test]
    fn an_off_hand_behind_the_holding_hand_cannot_spin_the_weapon_round() {
        let c = config();
        let off = Vec3::new(0.02, 0.0, 0.25);
        let pose = two_hand(hand(Vec3::ZERO), hand(off), &RIFLE, 1.0, &c).unwrap();
        assert!(pose.turn.limited);
        let barrel = pose.holder.wrist.orientation * RIFLE.barrel;
        let turned = barrel.angle_between(Vec3::NEG_Z).to_degrees();
        assert!(turned <= c.max_turn_deg + 0.01, "{turned}");
        // The weapon still points away from the player, and the hold lets go.
        assert!(barrel.z < 0.0);
        let (hold, _) = step(None, &inputs(Vec3::new(0.0, 0.0, -0.4), true), DT, &c);
        let (_, t) = step(hold, &inputs(off, false), DT, &c);
        assert_eq!(t, Some(Transition::Released(Release::MovedAway)));
    }

    #[test]
    fn an_off_axis_foregrip_keeps_the_barrel_where_the_hands_hold_it() {
        // A foregrip below the barrel: an off-hand on it turns nothing, though the line between
        // the hands runs below the barrel.
        let geometry = GripGeometry {
            foregrip: Vec3::new(0.0, -0.06, -0.35),
            barrel: Vec3::NEG_Z,
            sight: Vec3::new(0.0, 0.08, 0.0),
            muzzle: Vec3::new(0.0, 0.05, -0.7),
            native: true,
            gun: Pose::IDENTITY,
        };
        let pose = two_hand(
            hand(Vec3::ZERO),
            hand(geometry.foregrip),
            &geometry,
            1.0,
            &config(),
        )
        .unwrap();
        let barrel = pose.holder.wrist.orientation * geometry.barrel;
        assert!(close(barrel, Vec3::NEG_Z));
        assert!(close(pose.off.grip.position, geometry.foregrip));
    }

    #[test]
    fn the_turn_keeps_the_holding_hand_roll() {
        // A hand rolled onto its side keeps the roll: the turn has no component about the line.
        let rolled = CockpitHand {
            wrist: Pose {
                position: Vec3::ZERO,
                orientation: Quat::from_rotation_z(0.7),
            },
            ..hand(Vec3::ZERO)
        };
        let off = Vec3::new(0.0, 0.2, -0.35);
        let pose = two_hand(rolled, hand(off), &RIFLE, 1.0, &config()).unwrap();
        let line = off.normalize();
        let turn = pose.holder.wrist.orientation * rolled.wrist.orientation.inverse();
        let (axis, angle) = turn.to_axis_angle();
        assert!(angle > 0.1);
        assert!(axis.dot(line).abs() < 1e-4);
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
    fn a_held_grip_holds_and_letting_go_releases() {
        let config = ForegripConfig {
            hold: true,
            ..config()
        };
        let near = Vec3::new(0.0, 0.0, -0.4);
        let held = |grab, grip_held| Inputs {
            grip_held,
            ..inputs(near, grab)
        };
        let (hold, t) = step(None, &held(true, true), DT, &config);
        assert_eq!(t, Some(Transition::Grabbed));
        // Held, the hold stays, and a press while held (a re-squeeze) does not let go.
        let (hold, t) = step(hold, &held(false, true), DT, &config);
        assert_eq!(t, None);
        let (hold, t) = step(hold, &held(true, true), DT, &config);
        assert_eq!(t, None);
        assert!(hold.unwrap().engaged);
        // Opening the hand lets go.
        let (gone, t) = step(hold, &held(false, false), DT, &config);
        assert_eq!(t, Some(Transition::Released(Release::GripLetGo)));
        assert_eq!(gone, None);
        // A grip that is not held never keeps a hold, and a grab needs a press.
        let (none, t) = step(None, &held(false, true), DT, &config);
        assert_eq!((none, t), (None, None));
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
            ..config()
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

    #[test]
    fn a_hold_slides_along_the_handguard_and_barrel_but_not_off_them() {
        let config = config();
        let near = Vec3::new(0.0, 0.0, -0.4);
        let (hold, _) = step(None, &inputs(near, true), DT, &config);
        assert_eq!(hold.unwrap().region, GunRegion::Handguard);
        let further = Vec3::new(0.0, 0.0, -0.55);
        let slide = |point, region| Inputs {
            slide: Some((point, region)),
            ..inputs(further, false)
        };
        let (hold, t) = step(hold, &slide(further, GunRegion::Barrel), DT, &config);
        assert_eq!(t, None);
        let hold = hold.unwrap();
        assert_eq!(hold.region, GunRegion::Barrel);
        assert!(close(hold.geometry.foregrip, further));
        // A hand that slides onto the magazine keeps its hold where it was.
        let (kept, _) = step(
            Some(hold),
            &slide(Vec3::new(0.0, -0.1, -0.2), GunRegion::Magazine),
            DT,
            &config,
        );
        assert!(close(kept.unwrap().geometry.foregrip, further));
    }

    #[test]
    fn engaged_contact_tracks_the_selected_region_for_either_holder() {
        for holder in Hand::BOTH {
            for region in [
                GunRegion::Handguard,
                GunRegion::Barrel,
                GunRegion::Magazine,
                GunRegion::Stock,
            ] {
                let geometry = GripGeometry {
                    foregrip: Vec3::new(
                        -0.03,
                        -0.08,
                        if region == GunRegion::Stock {
                            0.18
                        } else {
                            -0.25
                        },
                    ),
                    ..RIFLE
                };
                let hold = Hold {
                    holder,
                    weapon: 1,
                    geometry,
                    region,
                    engaged: true,
                    weight: 1.0,
                };
                for frame in 0..12 {
                    let t = frame as f32 * 0.1;
                    let rotation = Quat::from_rotation_y(t) * Quat::from_rotation_z(t * 0.3);
                    let origin = Vec3::new(t, 1.3, -t * 0.4);
                    let tracked = CockpitHand {
                        wrist: Pose {
                            position: origin,
                            orientation: rotation,
                        },
                        grip: Pose {
                            position: origin + rotation * Vec3::new(0.01, -0.02, -0.04),
                            orientation: rotation,
                        },
                        aim: Pose {
                            position: origin,
                            orientation: rotation,
                        },
                    };
                    let off = hand(origin + rotation * Vec3::new(-0.04, -0.05, -0.19));
                    let posed = hold_pose(&hold, tracked, off, &config()).unwrap();
                    assert!(close(
                        posed.off.grip.position,
                        foregrip_point(&posed.holder, &geometry)
                    ));
                    assert_eq!(posed.off.wrist.orientation, off.wrist.orientation);
                }
            }
        }
    }

    #[test]
    fn a_hand_on_the_stock_holds_the_gun_without_steering_it() {
        let c = config();
        let stock = Vec3::new(0.0, 0.0, 0.25);
        let geometry = GripGeometry {
            foregrip: stock,
            ..RIFLE
        };
        let hold = Hold {
            holder: Hand::Right,
            weapon: 0x1000,
            geometry,
            region: GunRegion::Stock,
            engaged: true,
            weight: 1.0,
        };
        let pose = hold_pose(&hold, hand(Vec3::ZERO), hand(Vec3::new(0.05, 0.0, 0.3)), &c).unwrap();
        assert_eq!(pose.holder.wrist.orientation, Quat::IDENTITY);
        assert!(close(pose.off.grip.position, stock));
    }
}
