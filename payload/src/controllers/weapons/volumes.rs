//! Placing the body's grab volumes: the holsters, the grenade pouch, and the magazine pouch. Pure.
//!
//! The volumes are placed in the cockpit frame, where the hands' grab points are, relative to the
//! character's body frame. The cockpit frame composes onto the character's world transform with the
//! character's animated head at its origin (see `headpose::xr`), so in the cockpit frame the body
//! faces the frame's own axes (+X right, +Y up, -Z forward) with the eyes at the origin. The volumes
//! therefore stay on the body when the player turns or leans their head to look at them, and move
//! with the body when a stick turn or the roomscale chase moves it.
//!
//! Vertically, the body frame follows the head down once the head drops further than a deadband
//! below its upright height, so a crouch carries the holsters down while glancing at one does not.
//!
//! Each volume sits at an offset from a body landmark estimated from the body dimensions and the
//! body's [`Stance`]:
//!
//! - the hips, at a fixed fraction of the standing height, level with the shoulders' width and the
//!   shoulders' depth; seated, where the hands reach for the hips instead: higher up the torso and
//!   forward over the thighs, because the seat is where a standing body's hip joints are;
//! - the shoulder joints ([`body::shoulder_positions`]); seated, a little forward, over the tops of
//!   the shoulders, because a backrest is behind them;
//! - the chest, at the middle of the shoulder joints.
//!
//! The dimensions are the character's while the hands are remapped onto the character's proportions
//! (a hand the player puts on their own hip lands on the character's), and the player's otherwise.
//!
//! The stance comes from the posture ([`SeatedDetector`]): the seated posture is seated, and the
//! automatic posture, which never resolves to seated on its own because OpenXR has no signal for it,
//! counts the body as seated once the head has stayed well below the player's standing eye height
//! for long enough.

use glam::{Quat, Vec3};

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume, HolsterSite},
    body::{self, BodyDimensions},
    config::Posture,
    frame::Hand,
    weapons::config::{HolstersConfig, VolumeSpec},
};

/// How the body the volumes are placed on is posed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Stance {
    /// Standing upright with the eyes at the cockpit frame's origin height.
    Standing,
    /// Seated, with the upright seated eyes at the cockpit frame height `eye_y`.
    Seated { eye_y: f32 },
}

impl Stance {
    pub fn label(self) -> &'static str {
        match self {
            Stance::Standing => "standing",
            Stance::Seated { .. } => "seated",
        }
    }

    /// The cockpit frame height of the eyes when the body is upright in this stance.
    fn upright_eye_y(self) -> f32 {
        match self {
            Stance::Standing => 0.0,
            Stance::Seated { eye_y } if eye_y.is_finite() => eye_y,
            Stance::Seated { .. } => 0.0,
        }
    }
}

/// What the volumes are placed from: where the body frame's eyes are, and how the body is posed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    /// The body frame's origin in the cockpit frame: the eyes at their upright height, dropped with
    /// a head that has dropped further than the deadband below it.
    pub origin: Vec3,
    pub seated: bool,
}

/// The body frame for a head at `head` in `stance`: the eyes at the stance's upright height, until
/// the head drops more than `crouch_deadband` below it.
pub fn body_frame(head: Vec3, stance: Stance, crouch_deadband: f32) -> BodyFrame {
    let upright = stance.upright_eye_y();
    let deadband = if crouch_deadband.is_finite() {
        crouch_deadband.max(0.0)
    } else {
        0.0
    };
    let y = if head.y.is_finite() {
        (head.y + deadband).min(upright)
    } else {
        upright
    };
    BodyFrame {
        origin: Vec3::new(0.0, y, 0.0),
        seated: matches!(stance, Stance::Seated { .. }),
    }
}

/// What the stance is decided from on one input tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StanceInputs {
    /// The posture setting, before the automatic setting is resolved.
    pub setting: Posture,
    /// The head's height in the cockpit frame, metres.
    pub head_y: f32,
    /// The head's height above the STAGE floor, metres, when the runtime can locate it.
    pub floor_eye_height: Option<f32>,
    /// The player's standing eye height, metres.
    pub standing_eye_height: f32,
    /// The time since the last tick, seconds.
    pub dt: f32,
}

/// Decides the body's [`Stance`] from tick to tick.
///
/// - The seated posture is seated, with the eyes at the recenter baseline, which that posture
///   calibrates as the seated eye height.
/// - The standing and roomscale postures are standing.
/// - The automatic posture (with `detect_seated`) is seated once the head has stayed below
///   `seated_eye_fraction` of the standing eye height for `seated_after_s`, with the eyes at the
///   highest the head reached over that time, and is standing again as soon as the head rises
///   [`SEATED_EXIT_MARGIN`] above that fraction. Without a floor to measure from, it is standing.
///
/// A crouch shorter than `seated_after_s` keeps the standing placement, whose body frame drops
/// with the head. A crouch held longer is placed as seated, which costs little: a crouching body
/// folds at the hips, bringing them closer to the head and its thighs forward, as a seated one's
/// are.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SeatedDetector {
    /// How long the head has been low, seconds.
    low_s: f32,
    /// The highest cockpit frame head height while low.
    peak_y: f32,
    /// The seated eye height once the body counts as seated.
    seated_y: Option<f32>,
}

impl SeatedDetector {
    pub const fn new() -> Self {
        Self {
            low_s: 0.0,
            peak_y: f32::NEG_INFINITY,
            seated_y: None,
        }
    }

    /// Advance by one tick and return the stance.
    pub fn step(&mut self, inputs: &StanceInputs, config: &HolstersConfig) -> Stance {
        match inputs.setting {
            Posture::Seated => {
                *self = Self::new();
                return Stance::Seated { eye_y: 0.0 };
            }
            Posture::Standing | Posture::Roomscale => {
                *self = Self::new();
                return Stance::Standing;
            }
            Posture::Auto => {}
        }
        let standing = inputs.standing_eye_height;
        let (true, Some(height)) = (config.detect_seated, inputs.floor_eye_height) else {
            *self = Self::new();
            return Stance::Standing;
        };
        if !(height.is_finite() && standing.is_finite() && standing > 0.0) {
            *self = Self::new();
            return Stance::Standing;
        }
        let ratio = height / standing;
        if let Some(eye_y) = self.seated_y {
            if ratio > config.seated_eye_fraction + SEATED_EXIT_MARGIN {
                *self = Self::new();
                return Stance::Standing;
            }
            return Stance::Seated { eye_y };
        }
        if ratio >= config.seated_eye_fraction {
            *self = Self::new();
            return Stance::Standing;
        }
        let dt = if inputs.dt.is_finite() {
            inputs.dt.clamp(0.0, MAX_STEP_S)
        } else {
            0.0
        };
        self.low_s += dt;
        if inputs.head_y.is_finite() {
            self.peak_y = self.peak_y.max(inputs.head_y);
        }
        if self.low_s >= config.seated_after_s.max(0.0) {
            let eye_y = if self.peak_y.is_finite() {
                self.peak_y
            } else {
                0.0
            };
            self.seated_y = Some(eye_y);
            return Stance::Seated { eye_y };
        }
        Stance::Standing
    }
}

impl Default for SeatedDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// How far above `seated_eye_fraction`, as a fraction of the standing eye height, the head must
/// rise for a seated body to count as standing again.
pub const SEATED_EXIT_MARGIN: f32 = 0.06;

/// The holster volumes (and the grenade pouch with `grenade_pouch`) for a body in `frame` with
/// dimensions `dims`, in the order the sites are listed. The hip and shoulder holsters may be
/// grabbed with either hand; the pouch only with the dominant hand, which throws.
pub fn holster_candidates(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    config: &HolstersConfig,
    dominant: Hand,
    grenade_pouch: bool,
) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(6);
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Hip(side)),
            hip(frame, dims, side),
            &config.hip,
            side,
            None,
        ));
    }
    let shoulders = body::shoulder_positions(frame.origin, Quat::IDENTITY, dims);
    let over_shoulder = if frame.seated {
        Vec3::NEG_Z * SEATED_SHOULDER_FORWARD_M
    } else {
        Vec3::ZERO
    };
    for side in Hand::BOTH {
        out.push(place(
            GrabVolume::Holster(HolsterSite::Shoulder(side)),
            shoulders[side.index()] + over_shoulder,
            &config.shoulder,
            side,
            None,
        ));
    }
    out.push(place(
        GrabVolume::Holster(HolsterSite::Chest),
        (shoulders[0] + shoulders[1]) * 0.5,
        &config.chest,
        Hand::Right,
        None,
    ));
    if grenade_pouch {
        out.push(place(
            GrabVolume::GrenadePouch,
            hip(frame, dims, dominant),
            &config.pouch,
            dominant,
            Some(dominant),
        ));
    }
    out
}

/// The magazine pouch on the belt at `off`'s side, a volume only `off` may grab: the reload
/// gesture's first step.
pub fn magazine_pouch(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    spec: &VolumeSpec,
    off: Hand,
) -> Candidate {
    place(
        GrabVolume::MagazinePouch,
        hip(frame, dims, off),
        spec,
        off,
        Some(off),
    )
}

/// A grab point outside every holster that `hand` may grab: the nearest holster, how far the point
/// is outside its surface, and the point's offset from its centre.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct NearMiss {
    pub volume: GrabVolume,
    pub outside_m: f32,
    pub offset: Vec3,
}

/// The holster nearest `point` among `candidates` that `hand` may grab, when `point` is outside
/// every one of them and within `margin` of that one's surface.
pub fn near_miss(
    point: Vec3,
    hand: Hand,
    candidates: &[Candidate],
    margin: f32,
) -> Option<NearMiss> {
    if !point.is_finite() {
        return None;
    }
    let holsters = candidates.iter().filter(|c| {
        matches!(c.volume, GrabVolume::Holster(_))
            && c.hand.is_none_or(|h| h == hand)
            && c.radius.is_finite()
            && c.radius > 0.0
    });
    let (nearest, outside) = holsters
        .map(|c| (c, point.distance(c.centre) - c.radius))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    (outside > 0.0 && outside <= margin).then(|| NearMiss {
        volume: nearest.volume,
        outside_m: outside,
        offset: point - nearest.centre,
    })
}

/// The standing height of the hip joints as a fraction of the standing height: a generic
/// anthropometric figure, which the body file does not carry because it varies little between
/// adults.
pub const HIP_HEIGHT_FRACTION: f32 = 0.53;

/// The longest tick the seated detector counts, seconds, so that a stall does not count as a long
/// stretch with the head low.
const MAX_STEP_S: f32 = 0.25;

/// How far up the torso, from the hip joints toward the shoulder joints and as a fraction of that
/// length, a seated player's hands reach for the hips: the belt line at the side of the waist, above
/// the seat. A best guess.
const SEATED_HIP_RISE_FRACTION: f32 = 0.35;

/// How far forward of the hip joints a seated player's hands reach for the hips, over the thighs,
/// metres. A best guess.
const SEATED_HIP_FORWARD_M: f32 = 0.15;

/// How far forward of the shoulder joints the shoulder volumes sit for a seated body, metres: over
/// the tops of the shoulders rather than behind them, where a backrest is. A best guess.
const SEATED_SHOULDER_FORWARD_M: f32 = 0.05;

/// The hip landmark on `side`: as far out as the shoulder joint; standing, at the hip joints' height
/// and the shoulders' depth; seated, higher up the torso and forward over the thighs.
fn hip(frame: &BodyFrame, dims: &BodyDimensions, side: Hand) -> Vec3 {
    let out = dims.shoulder_width * 0.5 * side_sign(side);
    let hip_height = dims.height * HIP_HEIGHT_FRACTION;
    let standing = Vec3::new(
        out,
        hip_height - dims.eye_height,
        body::EYES_FORWARD_OF_SHOULDERS,
    );
    let landmark = if frame.seated {
        let torso = (dims.shoulder_height - hip_height).max(0.0);
        standing + Vec3::new(0.0, torso * SEATED_HIP_RISE_FRACTION, -SEATED_HIP_FORWARD_M)
    } else {
        standing
    };
    frame.origin + landmark
}

/// A volume at `spec`'s offset from `landmark`, mirrored for the left side.
fn place(
    volume: GrabVolume,
    landmark: Vec3,
    spec: &VolumeSpec,
    side: Hand,
    hand: Option<Hand>,
) -> Candidate {
    let offset = Vec3::new(
        spec.offset_m.x * side_sign(side),
        spec.offset_m.y,
        spec.offset_m.z,
    );
    Candidate {
        volume,
        centre: landmark + offset,
        radius: spec.radius_m.max(0.0),
        hand,
    }
}

fn side_sign(side: Hand) -> f32 {
    match side {
        Hand::Left => -1.0,
        Hand::Right => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{bindings::grab::grab_volume, weapons::config::MagazineConfig};

    const DIMS: BodyDimensions = BodyDimensions::GENERIC_ADULT;
    const STANDING: BodyFrame = BodyFrame {
        origin: Vec3::ZERO,
        seated: false,
    };
    const SEATED: BodyFrame = BodyFrame {
        origin: Vec3::ZERO,
        seated: true,
    };

    fn candidates(config: &HolstersConfig) -> Vec<Candidate> {
        holster_candidates(&STANDING, &DIMS, config, Hand::Right, false)
    }

    fn centre(c: &[Candidate], volume: GrabVolume) -> Vec3 {
        c.iter().find(|c| c.volume == volume).unwrap().centre
    }

    const RIGHT_HIP: GrabVolume = GrabVolume::Holster(HolsterSite::Hip(Hand::Right));
    const RIGHT_SHOULDER: GrabVolume = GrabVolume::Holster(HolsterSite::Shoulder(Hand::Right));

    #[test]
    fn the_holsters_sit_where_the_body_has_them() {
        let config = HolstersConfig::new();
        let c = candidates(&config);
        let right_hip = centre(&c, RIGHT_HIP);
        let left_hip = centre(&c, GrabVolume::Holster(HolsterSite::Hip(Hand::Left)));
        let right_shoulder = centre(&c, RIGHT_SHOULDER);
        let chest = centre(&c, GrabVolume::Holster(HolsterSite::Chest));
        assert!(right_hip.x > 0.0 && left_hip.x < 0.0);
        assert!((right_hip.x + left_hip.x).abs() < 1e-6, "the hips mirror");
        // The hips are well below the eyes, the shoulders just below, the chest between.
        assert!(right_hip.y < -0.6 && right_hip.y > -0.8);
        assert!(right_shoulder.y > -0.2 && right_shoulder.y < 0.0);
        assert!(chest.y < right_shoulder.y && chest.y > right_hip.y);
        // Over the shoulder is behind it; the chest is in front.
        assert!(right_shoulder.z > body::EYES_FORWARD_OF_SHOULDERS);
        assert!(chest.z < 0.0);
        assert_eq!(chest.x, 0.0);
    }

    #[test]
    fn seated_hips_are_higher_and_forward_and_the_rest_follow_the_torso() {
        let config = HolstersConfig::new();
        for dims in [
            BodyDimensions::GENERIC_ADULT,
            BodyDimensions::CHARACTER_ESTIMATE,
        ] {
            let standing = holster_candidates(&STANDING, &dims, &config, Hand::Right, false);
            let seated = holster_candidates(&SEATED, &dims, &config, Hand::Right, false);
            let (up, sh) = (centre(&standing, RIGHT_HIP), centre(&seated, RIGHT_HIP));
            // At the belt line beside the waist: about half a metre below the eyes, close to the
            // body, and in front of the eyes over the thighs.
            assert!(sh.y > up.y + 0.1, "{sh} vs {up}");
            assert!(sh.y < -0.4 && sh.y > -0.65, "{sh}");
            assert!(sh.z < 0.0 && sh.z > -0.15, "{sh}");
            assert_eq!(sh.x, up.x);
            // The shoulders come forward a little, clear of a backrest; the chest stays.
            let (us, ss) = (
                centre(&standing, RIGHT_SHOULDER),
                centre(&seated, RIGHT_SHOULDER),
            );
            assert!(ss.z < us.z && ss.z > 0.0 && ss.y == us.y);
            let chest = GrabVolume::Holster(HolsterSite::Chest);
            assert_eq!(centre(&standing, chest), centre(&seated, chest));
        }
    }

    #[test]
    fn the_default_holsters_do_not_overlap() {
        let config = HolstersConfig::new();
        for frame in [STANDING, SEATED] {
            for dims in [
                BodyDimensions::GENERIC_ADULT,
                BodyDimensions::CHARACTER_ESTIMATE,
            ] {
                let c = holster_candidates(&frame, &dims, &config, Hand::Right, false);
                for (i, a) in c.iter().enumerate() {
                    for b in &c[i + 1..] {
                        assert!(
                            a.centre.distance(b.centre) > a.radius + b.radius,
                            "{:?} and {:?} overlap (seated {})",
                            a.volume,
                            b.volume,
                            frame.seated
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_seated_hand_at_the_waist_grabs_the_hip_holster() {
        let config = HolstersConfig::new();
        let dims = BodyDimensions::CHARACTER_ESTIMATE;
        // A hand resting at the side of the waist, over the front of the seat: half a metre below
        // the eyes, a hand's width out from the body, a little in front of the eyes.
        let hand = Vec3::new(0.22, -0.5, -0.08);
        let seated = holster_candidates(&SEATED, &dims, &config, Hand::Right, false);
        assert_eq!(grab_volume(hand, Hand::Right, &seated), Some(RIGHT_HIP));
        let standing = holster_candidates(&STANDING, &dims, &config, Hand::Right, false);
        assert_eq!(grab_volume(hand, Hand::Right, &standing), None);
    }

    #[test]
    fn the_pouch_exists_only_when_enabled_and_only_for_the_dominant_hand() {
        let config = HolstersConfig::new();
        assert!(
            candidates(&config)
                .iter()
                .all(|c| c.volume != GrabVolume::GrenadePouch)
        );
        let c = holster_candidates(&STANDING, &DIMS, &config, Hand::Right, true);
        let pouch = c
            .iter()
            .find(|c| c.volume == GrabVolume::GrenadePouch)
            .unwrap();
        assert_eq!(pouch.hand, Some(Hand::Right));
        assert_eq!(grab_volume(pouch.centre, Hand::Left, &c), None);
        assert_eq!(
            grab_volume(pouch.centre, Hand::Right, &c),
            Some(GrabVolume::GrenadePouch)
        );
    }

    #[test]
    fn the_magazine_pouch_is_on_the_off_hand_side_for_the_off_hand() {
        let spec = MagazineConfig::new().pouch;
        for frame in [STANDING, SEATED] {
            let pouch = magazine_pouch(&frame, &DIMS, &spec, Hand::Left);
            assert!(pouch.centre.x < 0.0);
            assert_eq!(pouch.hand, Some(Hand::Left));
            let mut c =
                holster_candidates(&frame, &DIMS, &HolstersConfig::new(), Hand::Right, true);
            c.push(pouch);
            assert_eq!(
                grab_volume(pouch.centre, Hand::Left, &c),
                Some(GrabVolume::MagazinePouch)
            );
            assert_ne!(
                grab_volume(pouch.centre, Hand::Right, &c),
                Some(GrabVolume::MagazinePouch)
            );
        }
    }

    #[test]
    fn a_hand_at_a_holster_grabs_it_and_a_hand_in_front_grabs_nothing() {
        for frame in [STANDING, SEATED] {
            let c = holster_candidates(&frame, &DIMS, &HolstersConfig::new(), Hand::Right, false);
            for candidate in &c {
                assert_eq!(
                    grab_volume(candidate.centre, Hand::Left, &c),
                    Some(candidate.volume),
                    "{:?}",
                    candidate.volume
                );
            }
            // A hand held out in front at chest height, as when firing the grapple.
            assert_eq!(
                grab_volume(Vec3::new(0.0, -0.3, -0.45), Hand::Left, &c),
                None
            );
        }
        // A hand hanging relaxed at the side, a little forward of the holster, still reaches it.
        let c = candidates(&HolstersConfig::new());
        let hip = centre(&c, RIGHT_HIP);
        assert!(grab_volume(hip + Vec3::new(0.0, 0.0, -0.08), Hand::Right, &c).is_some());
    }

    #[test]
    fn the_body_drops_only_past_the_deadband() {
        let origin = |head, stance| body_frame(head, stance, 0.15).origin;
        assert_eq!(
            origin(Vec3::new(0.3, -0.1, 0.2), Stance::Standing),
            Vec3::ZERO
        );
        assert_eq!(
            origin(Vec3::new(0.0, 0.2, 0.0), Stance::Standing),
            Vec3::ZERO
        );
        let crouched = origin(Vec3::new(0.0, -0.5, 0.0), Stance::Standing);
        assert!((crouched.y + 0.35).abs() < 1e-6);
        assert_eq!(origin(Vec3::NAN, Stance::Standing), Vec3::ZERO);
        // Seated, the upright height is the seated eyes'.
        let seated = Stance::Seated { eye_y: -0.5 };
        assert_eq!(origin(Vec3::new(0.0, -0.45, 0.0), seated).y, -0.5);
        assert_eq!(origin(Vec3::new(0.0, -0.6, 0.0), seated).y, -0.5);
        assert!((origin(Vec3::new(0.0, -0.8, 0.0), seated).y + 0.65).abs() < 1e-6);
        assert!(body_frame(Vec3::ZERO, seated, 0.15).seated);
        assert!(!body_frame(Vec3::ZERO, Stance::Standing, 0.15).seated);
    }

    const EYES: f32 = 1.68;

    fn tick(d: &mut SeatedDetector, setting: Posture, floor: Option<f32>, head_y: f32) -> Stance {
        d.step(
            &StanceInputs {
                setting,
                head_y,
                floor_eye_height: floor,
                standing_eye_height: EYES,
                dt: 0.1,
            },
            &HolstersConfig::new(),
        )
    }

    #[test]
    fn the_explicit_postures_decide_the_stance() {
        let mut d = SeatedDetector::new();
        assert_eq!(
            tick(&mut d, Posture::Seated, Some(1.7), 0.3),
            Stance::Seated { eye_y: 0.0 }
        );
        for setting in [Posture::Standing, Posture::Roomscale] {
            for _ in 0..100 {
                assert_eq!(tick(&mut d, setting, Some(1.1), -0.5), Stance::Standing);
            }
        }
    }

    #[test]
    fn auto_sits_after_the_head_stays_low_and_stands_when_it_rises() {
        let mut d = SeatedDetector::new();
        assert_eq!(
            tick(&mut d, Posture::Auto, Some(EYES), 0.0),
            Stance::Standing
        );
        // Seated on a chair: the head is low for longer than the dwell.
        let mut stance = Stance::Standing;
        for i in 0..50 {
            let head_y = if i == 10 { -0.4 } else { -0.45 };
            stance = tick(&mut d, Posture::Auto, Some(1.22), head_y);
        }
        assert_eq!(stance, Stance::Seated { eye_y: -0.4 });
        // Leaning forward and slightly up again keeps it seated.
        assert_eq!(
            tick(&mut d, Posture::Auto, Some(1.40), -0.3),
            Stance::Seated { eye_y: -0.4 }
        );
        // Standing up ends it at once.
        assert_eq!(
            tick(&mut d, Posture::Auto, Some(1.60), 0.0),
            Stance::Standing
        );
    }

    #[test]
    fn a_short_crouch_stays_standing() {
        let mut d = SeatedDetector::new();
        for _ in 0..30 {
            assert_eq!(
                tick(&mut d, Posture::Auto, Some(1.0), -0.6),
                Stance::Standing
            );
        }
        assert_eq!(
            tick(&mut d, Posture::Auto, Some(EYES), 0.0),
            Stance::Standing
        );
        for _ in 0..30 {
            assert_eq!(
                tick(&mut d, Posture::Auto, Some(1.0), -0.6),
                Stance::Standing
            );
        }
    }

    #[test]
    fn auto_without_a_floor_or_detection_stays_standing() {
        let mut d = SeatedDetector::new();
        for _ in 0..100 {
            assert_eq!(tick(&mut d, Posture::Auto, None, -0.5), Stance::Standing);
        }
        let off = HolstersConfig {
            detect_seated: false,
            ..HolstersConfig::new()
        };
        let inputs = StanceInputs {
            setting: Posture::Auto,
            head_y: -0.5,
            floor_eye_height: Some(1.1),
            standing_eye_height: EYES,
            dt: 1.0,
        };
        for _ in 0..100 {
            assert_eq!(d.step(&inputs, &off), Stance::Standing);
        }
    }

    #[test]
    fn a_stall_does_not_count_as_a_long_stretch_low() {
        let mut d = SeatedDetector::new();
        let inputs = StanceInputs {
            setting: Posture::Auto,
            head_y: -0.5,
            floor_eye_height: Some(1.1),
            standing_eye_height: EYES,
            dt: 30.0,
        };
        assert_eq!(d.step(&inputs, &HolstersConfig::new()), Stance::Standing);
    }

    #[test]
    fn a_near_miss_names_the_nearest_holster_and_its_distance() {
        let c = candidates(&HolstersConfig::new());
        let hip = c.iter().find(|c| c.volume == RIGHT_HIP).unwrap();
        let point = hip.centre + Vec3::new(0.0, hip.radius + 0.05, 0.0);
        let miss = near_miss(point, Hand::Right, &c, 0.3).unwrap();
        assert_eq!(miss.volume, RIGHT_HIP);
        assert!((miss.outside_m - 0.05).abs() < 1e-5);
        assert!((miss.offset.y - (hip.radius + 0.05)).abs() < 1e-5);
        // Inside a holster, or far from all of them, is not a near miss.
        assert_eq!(near_miss(hip.centre, Hand::Right, &c, 0.3), None);
        assert_eq!(
            near_miss(Vec3::new(0.0, 0.5, -1.0), Hand::Right, &c, 0.3),
            None
        );
        assert_eq!(near_miss(Vec3::NAN, Hand::Right, &c, 0.3), None);
    }
}
