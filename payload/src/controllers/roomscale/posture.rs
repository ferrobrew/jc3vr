//! The player's posture and the head height it implies. Pure.
//!
//! The posture decides where the eyes sit and whether the body chases the head:
//!
//! - seated: the seated eye reference ([`crate::controllers::roomscale::seated`]) maps to the
//!   character's standing eye height, and vertical travel from it scales by the height ratio; the
//!   crouch toggle lowers the eyes by a preset. The reference is measured against the STAGE floor
//!   when the runtime has one, and is the eye height at the last recenter otherwise;
//! - standing: the head's true height above the STAGE floor is remapped onto the character
//!   ([`remap_head_height`]), so that ducking and crouching are physical, and the body follows the
//!   head about the room ([`crate::controllers::body::rig::root`]).
//!
//! OpenXR has no signal for a seated user, so the automatic posture tells seated from standing by
//! the head's height above the floor ([`PostureClassifier`]).

use glam::Vec3;

use crate::controllers::{
    body::{BodyDimensions, remap_head_height, remap_head_translation},
    config::{AutoPostureConfig, Posture},
    roomscale::seated::{SeatedEyeReference, SeatedReferenceSource},
};

/// The posture in effect after the automatic setting is resolved.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EffectivePosture {
    Seated,
    Standing,
}

impl EffectivePosture {
    pub fn label(self) -> &'static str {
        match self {
            EffectivePosture::Seated => "seated",
            EffectivePosture::Standing => "standing",
        }
    }
}

/// Resolve the posture setting. The automatic setting takes the classifier's `detected` posture.
pub fn resolve(setting: Posture, detected: DetectedPosture) -> EffectivePosture {
    match setting {
        Posture::Seated => EffectivePosture::Seated,
        Posture::Standing => EffectivePosture::Standing,
        Posture::Auto => match detected {
            DetectedPosture::Seated => EffectivePosture::Seated,
            DetectedPosture::Standing => EffectivePosture::Standing,
        },
    }
}

/// What the automatic posture detected from the head height.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DetectedPosture {
    Seated,
    Standing,
}

/// What the classifier reads on one frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ClassifierInput {
    /// The head's height above the STAGE floor, metres, when the runtime can locate it.
    pub head_height: Option<f32>,
    /// The player's standing eye height, metres.
    pub standing_eye_m: f32,
    /// The player's calibrated seated eye height, metres, when one is saved.
    pub calibrated_seated_eye_m: Option<f32>,
}

/// Why the classifier holds its posture.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DetectionReason {
    /// No head height has been located yet: seated, the default that never forces a crouch.
    NoEvidence,
    /// The first located head height, against the midpoint between the two eye heights.
    FirstSample,
    /// A manual recenter, against the midpoint.
    Recenter,
    /// The head stayed at standing height for the stand dwell.
    StoodUp,
    /// The head stayed at seated height for the sit dwell.
    SatDown,
}

impl DetectionReason {
    pub fn label(self) -> &'static str {
        match self {
            DetectionReason::NoEvidence => "no head height yet",
            DetectionReason::FirstSample => "first head height",
            DetectionReason::Recenter => "recenter",
            DetectionReason::StoodUp => "stood up",
            DetectionReason::SatDown => "sat down",
        }
    }
}

/// The classifier's decision and what it was made from, for the readouts.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ClassifierStatus {
    pub detected: DetectedPosture,
    pub reason: DetectionReason,
    pub head_height_m: Option<f32>,
    pub thresholds: Thresholds,
    /// How long the head has been past the threshold that would change the posture, seconds.
    pub dwell_s: f32,
    /// Whether a standing player's head is at floor level, below a chair's height: sitting or
    /// kneeling on the floor of the play area, which keeps the standing posture.
    pub floor_level: bool,
}

/// The heights the classifier compares the head with, metres above the floor.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Thresholds {
    /// The seated eye height: the calibrated one, or an estimate from the standing eye height.
    pub seated_eye_m: f32,
    /// Whether [`seated_eye_m`](Self::seated_eye_m) is an estimate.
    pub seated_estimated: bool,
    pub standing_eye_m: f32,
    /// A standing player whose head stays below this, steady and above
    /// [`floor_below_m`](Self::floor_below_m), for the sit dwell is seated.
    pub sit_below_m: f32,
    /// Below this a standing player's head is at floor level, lower than a chair puts it: the
    /// player sits or kneels on the floor and stays standing.
    pub floor_below_m: f32,
    /// A seated player whose head stays above this for the stand dwell is standing.
    pub stand_above_m: f32,
    /// The first sample and a recenter classify against this, with no dwell.
    pub midpoint_m: f32,
}

impl Thresholds {
    /// The thresholds for one frame's eye heights. The seated eye height is the calibrated one when
    /// it is below the standing eye height, and otherwise [`SEATED_EYE_FRACTION`] of it.
    pub fn new(standing_eye_m: f32, calibrated_seated_eye_m: Option<f32>) -> Self {
        let standing_eye_m = if standing_eye_m.is_finite() && standing_eye_m > 0.0 {
            standing_eye_m
        } else {
            BodyDimensions::GENERIC_ADULT.eye_height
        };
        let calibrated = calibrated_seated_eye_m
            .filter(|h| h.is_finite() && *h > 0.0 && *h < standing_eye_m - MIN_EYE_GAP_M);
        let (seated_eye_m, seated_estimated) = match calibrated {
            Some(h) => (h, false),
            None => (standing_eye_m * SEATED_EYE_FRACTION, true),
        };
        let gap = standing_eye_m - seated_eye_m;
        Self {
            seated_eye_m,
            seated_estimated,
            standing_eye_m,
            sit_below_m: seated_eye_m + SIT_FRACTION * gap,
            floor_below_m: seated_eye_m - FLOOR_MARGIN_FRACTION * standing_eye_m,
            stand_above_m: seated_eye_m + STAND_FRACTION * gap,
            midpoint_m: seated_eye_m + 0.5 * gap,
        }
    }

    fn classify(&self, head_height_m: f32) -> DetectedPosture {
        if head_height_m < self.midpoint_m {
            DetectedPosture::Seated
        } else {
            DetectedPosture::Standing
        }
    }
}

/// The automatic posture's seated and standing detection, from the head's height above the floor
/// against the player's two eye heights.
///
/// - Until a head height is located, the player counts as seated: a seated treatment never forces
///   a crouch, while a standing one turns a seated player into a crouching stander.
/// - The first located height, and the height at a manual recenter, classify at once against the
///   midpoint between the seated and standing eye heights.
/// - After that, a seated player stands once the head stays above [`Thresholds::stand_above_m`] for
///   `stand_after_s`, which a seated player cannot reach without standing up, and a standing player
///   sits once the head stays below [`Thresholds::sit_below_m`] for `sit_after_s`, which is long so
///   that a crouch behind cover stays a physical crouch.
/// - The standing player's head must stay at a chair's height for that: steady, within
///   [`STEADY_BAND_M`] of where the dwell started, and above [`Thresholds::floor_below_m`]. A head
///   that moves further starts the dwell again from where it is, and a head at floor level does not
///   count: a standing player who got down onto the floor of the play area (sitting on it, kneeling)
///   stays standing, so the head keeps its true height and the body sits on the ground under it.
///
/// The controllers' heights are not used: a crouching player's hands sit as low as a seated one's.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PostureClassifier {
    detected: Option<(DetectedPosture, DetectionReason)>,
    dwell_s: f32,
    /// The head's height when the sit dwell started: the dwell holds while the head stays near it.
    dwell_from: Option<f32>,
    floor_level: bool,
}

impl PostureClassifier {
    pub const fn new() -> Self {
        Self {
            detected: None,
            dwell_s: 0.0,
            dwell_from: None,
            floor_level: false,
        }
    }

    /// Advance by one frame of `dt` seconds.
    pub fn update(
        &mut self,
        input: &ClassifierInput,
        dt: f32,
        config: &AutoPostureConfig,
    ) -> ClassifierStatus {
        let thresholds = Thresholds::new(input.standing_eye_m, input.calibrated_seated_eye_m);
        let head = input.head_height.filter(|h| h.is_finite());
        self.floor_level = false;
        match (self.detected, head) {
            (None, Some(height)) => {
                self.detected = Some((thresholds.classify(height), DetectionReason::FirstSample));
                self.restart_dwell();
            }
            (Some((detected, _)), Some(height)) => {
                let (crossing, dwell, next, reason) = match detected {
                    DetectedPosture::Seated => (
                        height > thresholds.stand_above_m,
                        config.stand_after_s,
                        DetectedPosture::Standing,
                        DetectionReason::StoodUp,
                    ),
                    DetectedPosture::Standing => {
                        self.floor_level =
                            config.chair_height_only && height < thresholds.floor_below_m;
                        (
                            height < thresholds.sit_below_m && !self.floor_level,
                            config.sit_after_s,
                            DetectedPosture::Seated,
                            DetectionReason::SatDown,
                        )
                    }
                };
                if !crossing {
                    self.restart_dwell();
                } else if detected == DetectedPosture::Standing
                    && config.chair_height_only
                    && self
                        .dwell_from
                        .is_none_or(|from| (height - from).abs() > STEADY_BAND_M)
                {
                    // A head that is not steady at a chair's height starts the dwell over from
                    // where it is.
                    self.dwell_s = 0.0;
                    self.dwell_from = Some(height);
                } else {
                    self.dwell_s += step(dt);
                }
                if crossing && self.dwell_s >= dwell.max(0.0) {
                    self.detected = Some((next, reason));
                    self.restart_dwell();
                }
            }
            (_, None) => self.restart_dwell(),
        }
        self.status(head, thresholds)
    }

    /// Classify at once against the midpoint, for a manual recenter: the player has just said this
    /// is how they are. Returns the posture detected, `None` without a located head height.
    pub fn reclassify(&mut self, input: &ClassifierInput) -> Option<DetectedPosture> {
        let height = input.head_height.filter(|h| h.is_finite())?;
        let thresholds = Thresholds::new(input.standing_eye_m, input.calibrated_seated_eye_m);
        let detected = thresholds.classify(height);
        self.detected = Some((detected, DetectionReason::Recenter));
        self.restart_dwell();
        Some(detected)
    }

    /// The posture detected so far, seated before any evidence.
    pub fn detected(&self) -> DetectedPosture {
        self.detected
            .map_or(DetectedPosture::Seated, |(detected, _)| detected)
    }

    /// Forget the detection, when the posture setting stops being automatic, so that returning to
    /// it classifies the first head height afresh.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    fn restart_dwell(&mut self) {
        self.dwell_s = 0.0;
        self.dwell_from = None;
    }

    fn status(&self, head: Option<f32>, thresholds: Thresholds) -> ClassifierStatus {
        let (detected, reason) = self
            .detected
            .unwrap_or((DetectedPosture::Seated, DetectionReason::NoEvidence));
        ClassifierStatus {
            detected,
            reason,
            head_height_m: head,
            thresholds,
            dwell_s: self.dwell_s,
            floor_level: self.floor_level,
        }
    }
}

impl Default for PostureClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// The seated eye height estimated from the standing eye height, without a calibration: an adult on
/// an ordinary chair.
pub const SEATED_EYE_FRACTION: f32 = 0.72;

/// The smallest gap between a calibrated seated eye height and the standing eye height the
/// thresholds take it with, metres; a closer one is replaced by the estimate.
const MIN_EYE_GAP_M: f32 = 0.2;

/// Where between the seated and the standing eye heights the sit threshold sits.
const SIT_FRACTION: f32 = 0.4;

/// Where between the seated and the standing eye heights the stand threshold sits.
const STAND_FRACTION: f32 = 0.6;

/// How far below the seated eye height a head is at floor level, as a share of the standing eye
/// height: about twenty centimetres for a standing eye height of 1.7 m, below the lowest chair and
/// above a head sitting on the floor.
const FLOOR_MARGIN_FRACTION: f32 = 0.12;

/// How far a standing player's head may move up or down and still count as steady at a chair's
/// height, metres: a seated head holds its height, and a crouch behind cover bobs.
pub const STEADY_BAND_M: f32 = 0.08;

/// The longest frame step the dwell accounts for, seconds, so that a stall does not complete one.
const MAX_STEP_S: f32 = 0.25;

fn step(dt: f32) -> f32 {
    if dt.is_finite() {
        dt.clamp(0.0, MAX_STEP_S)
    } else {
        0.0
    }
}

/// What the head height is computed from on one frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeadHeightInput {
    pub posture: EffectivePosture,
    /// The head's height relative to the recenter baseline, metres (the cockpit position's
    /// vertical component).
    pub cockpit_y: f32,
    /// The head's height above the STAGE floor, metres, when the runtime can locate it.
    pub stage_height: Option<f32>,
    /// The seated eye reference, in the seated posture once it has one.
    pub seated_reference: Option<SeatedEyeReference>,
    /// Whether the seated crouch toggle lowers the eyes on this frame.
    pub seated_crouch: bool,
    /// How far the seated crouch lowers the eyes, metres at the character's scale.
    pub seated_drop_m: f32,
}

/// Where the head height came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeadHeightSource {
    /// The STAGE floor, standing.
    Floor,
    /// The STAGE floor, seated, against the seated eye reference from this source.
    SeatedReference(SeatedReferenceSource),
    /// The recenter baseline, as the seated eye height (the seated posture, or a standing posture
    /// on a runtime without a STAGE space).
    Baseline,
}

impl HeadHeightSource {
    pub fn label(self) -> &'static str {
        match self {
            HeadHeightSource::Floor => "the floor",
            HeadHeightSource::SeatedReference(SeatedReferenceSource::Calibrated) => {
                "the calibrated seated eye height"
            }
            HeadHeightSource::SeatedReference(SeatedReferenceSource::FirstSample) => {
                "the first seated head height"
            }
            HeadHeightSource::SeatedReference(SeatedReferenceSource::Recenter) => {
                "the seated head height at the last recenter"
            }
            HeadHeightSource::SeatedReference(SeatedReferenceSource::Adaptive) => {
                "the adaptive seated head height"
            }
            HeadHeightSource::Baseline => "the recenter baseline",
        }
    }
}

/// The remapped head height.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeadHeight {
    /// The cockpit position's vertical component to compose the view with: the offset of the eyes
    /// from the character's standing eye height, metres before the world scale.
    pub cockpit_y: f32,
    pub source: HeadHeightSource,
}

/// Remap the head height onto the character. In a standing posture with a floor, the eyes sit at
/// [`remap_head_height`] of the true eye height; the cockpit offset is how far that is from the
/// character's standing eye height, which is where the animated head anchor already puts the eyes.
/// Seated with a floor and a seated eye reference, the offset is the head's travel from the
/// reference, scaled by the height ratio, less the seated crouch. Otherwise the baseline is the eye
/// height at the last recenter, and travel from it scales the same way.
pub fn head_height(
    input: &HeadHeightInput,
    player: &BodyDimensions,
    character: &BodyDimensions,
) -> HeadHeight {
    match (input.posture, input.stage_height, input.seated_reference) {
        (EffectivePosture::Standing, Some(height), _) => HeadHeight {
            cockpit_y: remap_head_height(height, player, character) - character.eye_height,
            source: HeadHeightSource::Floor,
        },
        (EffectivePosture::Seated, Some(height), Some(reference)) => {
            let crouch = seated_crouch_drop(
                EffectivePosture::Seated,
                input.seated_crouch,
                input.seated_drop_m,
            );
            HeadHeight {
                cockpit_y: remap_head_translation(
                    Vec3::Y * (height - reference.height_m),
                    player,
                    character,
                )
                .y - crouch,
                source: HeadHeightSource::SeatedReference(reference.source),
            }
        }
        (posture, _, _) => {
            let crouch = seated_crouch_drop(posture, input.seated_crouch, input.seated_drop_m);
            HeadHeight {
                cockpit_y: remap_head_translation(Vec3::Y * input.cockpit_y, player, character).y
                    - crouch,
                source: HeadHeightSource::Baseline,
            }
        }
    }
}

/// How far the seated crouch lowers the eyes: `seated_drop_m` while crouched in the seated
/// posture, and nothing otherwise.
pub fn seated_crouch_drop(posture: EffectivePosture, crouched: bool, seated_drop_m: f32) -> f32 {
    if posture == EffectivePosture::Seated && crouched && seated_drop_m.is_finite() {
        seated_drop_m.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;
    const CHARACTER: BodyDimensions = BodyDimensions::CHARACTER_ESTIMATE;
    const PLAYER: BodyDimensions = BodyDimensions {
        height: 1.60,
        eye_height: 1.49,
        ..BodyDimensions::GENERIC_ADULT
    };
    const FRAME_S: f32 = 1.0 / 90.0;

    /// The eye heights of the session the classifier was fixed from: standing 1.70 m, calibrated
    /// seated 1.22 m.
    const LOGGED: ClassifierInput = ClassifierInput {
        head_height: None,
        standing_eye_m: 1.699,
        calibrated_seated_eye_m: Some(1.218),
    };

    fn input(posture: EffectivePosture) -> HeadHeightInput {
        HeadHeightInput {
            posture,
            cockpit_y: 0.0,
            stage_height: None,
            seated_reference: None,
            seated_crouch: false,
            seated_drop_m: 0.6,
        }
    }

    fn reference(height_m: f32, source: SeatedReferenceSource) -> Option<SeatedEyeReference> {
        Some(SeatedEyeReference { height_m, source })
    }

    fn at(head_height: f32) -> ClassifierInput {
        ClassifierInput {
            head_height: Some(head_height),
            ..LOGGED
        }
    }

    /// Run `seconds` of frames with the head at `head_height`.
    fn hold(
        classifier: &mut PostureClassifier,
        head_height: f32,
        seconds: f32,
    ) -> ClassifierStatus {
        let config = AutoPostureConfig::new();
        let mut status = classifier.update(&at(head_height), 0.0, &config);
        for _ in 0..(seconds / FRAME_S).round() as usize {
            status = classifier.update(&at(head_height), FRAME_S, &config);
        }
        status
    }

    #[test]
    fn auto_takes_the_detection() {
        assert_eq!(
            resolve(Posture::Auto, DetectedPosture::Standing),
            EffectivePosture::Standing
        );
        assert_eq!(
            resolve(Posture::Auto, DetectedPosture::Seated),
            EffectivePosture::Seated
        );
    }

    #[test]
    fn an_explicit_posture_ignores_the_detection() {
        assert_eq!(
            resolve(Posture::Seated, DetectedPosture::Standing),
            EffectivePosture::Seated
        );
        assert_eq!(
            resolve(Posture::Standing, DetectedPosture::Seated),
            EffectivePosture::Standing
        );
    }

    /// The logged startup: the head 1.05 m above the floor, a standing calibration of 1.70 m, and a
    /// seated one of 1.22 m. It was taken for a crouching stander; it is seated from the first
    /// frame.
    #[test]
    fn the_logged_seated_head_is_seated_from_the_first_frame() {
        let mut classifier = PostureClassifier::new();
        let status = classifier.update(&at(1.057), FRAME_S, &AutoPostureConfig::new());
        assert_eq!(status.detected, DetectedPosture::Seated);
        assert_eq!(status.reason, DetectionReason::FirstSample);
        for logged in [1.0496, 1.0094, 1.0892, 0.896, 1.0255, 1.1414] {
            assert_eq!(
                hold(&mut classifier, logged, 5.0).detected,
                DetectedPosture::Seated,
                "{logged}"
            );
        }
    }

    #[test]
    fn before_any_head_height_the_player_counts_as_seated() {
        let mut classifier = PostureClassifier::new();
        let status = classifier.update(&LOGGED, FRAME_S, &AutoPostureConfig::new());
        assert_eq!(status.detected, DetectedPosture::Seated);
        assert_eq!(status.reason, DetectionReason::NoEvidence);
        // The first located height still classifies at once.
        let status = classifier.update(&at(1.68), FRAME_S, &AutoPostureConfig::new());
        assert_eq!(status.detected, DetectedPosture::Standing);
        assert_eq!(status.reason, DetectionReason::FirstSample);
    }

    #[test]
    fn standing_up_is_detected_after_the_stand_dwell() {
        let mut classifier = PostureClassifier::new();
        hold(&mut classifier, 1.07, 1.0);
        let thresholds = Thresholds::new(LOGGED.standing_eye_m, LOGGED.calibrated_seated_eye_m);
        // A tall stretch in the chair stays seated.
        assert_eq!(
            hold(&mut classifier, thresholds.stand_above_m - 0.02, 10.0).detected,
            DetectedPosture::Seated
        );
        assert_eq!(
            hold(&mut classifier, 1.66, 0.5).detected,
            DetectedPosture::Seated
        );
        let status = hold(&mut classifier, 1.66, 1.0);
        assert_eq!(status.detected, DetectedPosture::Standing);
        assert_eq!(status.reason, DetectionReason::StoodUp);
    }

    #[test]
    fn a_standing_crouch_stays_standing_until_the_sit_dwell() {
        let mut classifier = PostureClassifier::new();
        hold(&mut classifier, 1.68, 1.0);
        // Crouching behind cover for ten seconds, then standing, then crouching again: the dwell
        // starts over, and the physical crouch is kept throughout.
        assert_eq!(
            hold(&mut classifier, 1.05, 10.0).detected,
            DetectedPosture::Standing
        );
        hold(&mut classifier, 1.68, 0.2);
        assert_eq!(
            hold(&mut classifier, 1.05, 15.0).detected,
            DetectedPosture::Standing
        );
        let status = hold(&mut classifier, 1.05, 6.0);
        assert_eq!(status.detected, DetectedPosture::Seated);
        assert_eq!(status.reason, DetectionReason::SatDown);
    }

    #[test]
    fn a_standing_player_sitting_on_the_floor_stays_standing() {
        let mut classifier = PostureClassifier::new();
        hold(&mut classifier, 1.68, 1.0);
        // Down to the floor of the play area and staying there, still.
        let status = hold(&mut classifier, 0.85, 120.0);
        assert_eq!(status.detected, DetectedPosture::Standing);
        assert!(status.floor_level);
        assert_eq!(status.dwell_s, 0.0);
        // Kneeling upright on the floor puts the head at a chair's height: a long steady kneel is
        // the classifier's known limit, as a long crouch is.
        let thresholds = Thresholds::new(LOGGED.standing_eye_m, LOGGED.calibrated_seated_eye_m);
        assert!(thresholds.floor_below_m < thresholds.seated_eye_m);
        assert!(thresholds.floor_below_m > 0.9);
        // Getting up from the floor onto a chair, and sitting there, is seated.
        let status = hold(&mut classifier, 1.15, 5.0);
        assert!(!status.floor_level);
        assert_eq!(status.detected, DetectedPosture::Standing);
        let status = hold(&mut classifier, 1.15, 20.0);
        assert_eq!(status.detected, DetectedPosture::Seated);
        assert_eq!(status.reason, DetectionReason::SatDown);
        // Switched off, the floor sits after the wait, as any low head does.
        let config = AutoPostureConfig {
            chair_height_only: false,
            ..AutoPostureConfig::new()
        };
        let mut classifier = PostureClassifier::new();
        classifier.update(&at(1.68), FRAME_S, &config);
        let mut status = classifier.update(&at(0.85), FRAME_S, &config);
        for _ in 0..(21.0 / FRAME_S) as usize {
            status = classifier.update(&at(0.85), FRAME_S, &config);
        }
        assert!(!status.floor_level);
        assert_eq!(status.detected, DetectedPosture::Seated);
    }

    #[test]
    fn a_head_bobbing_at_a_chairs_height_stays_standing() {
        let mut classifier = PostureClassifier::new();
        hold(&mut classifier, 1.68, 1.0);
        // Crouched behind cover for a minute, rising to peek and ducking again every few seconds.
        for _ in 0..12 {
            assert_eq!(
                hold(&mut classifier, 1.0 + STEADY_BAND_M * 2.0, 2.5).detected,
                DetectedPosture::Standing
            );
            assert_eq!(
                hold(&mut classifier, 1.03, 2.5).detected,
                DetectedPosture::Standing
            );
        }
        // Small fidgets within the band still count as steady.
        let mut status = hold(&mut classifier, 1.10, 1.0);
        for i in 0..25 {
            let fidget = if i % 2 == 0 { 0.03 } else { -0.03 };
            status = hold(&mut classifier, 1.10 + fidget, 1.0);
        }
        assert_eq!(status.detected, DetectedPosture::Seated);
    }

    #[test]
    fn a_recenter_classifies_at_once() {
        let mut classifier = PostureClassifier::new();
        hold(&mut classifier, 1.68, 1.0);
        assert_eq!(
            classifier.reclassify(&at(1.07)),
            Some(DetectedPosture::Seated)
        );
        assert_eq!(classifier.detected(), DetectedPosture::Seated);
        assert_eq!(classifier.reclassify(&LOGGED), None);
        assert_eq!(classifier.detected(), DetectedPosture::Seated);
    }

    #[test]
    fn a_stall_does_not_complete_a_dwell() {
        let mut classifier = PostureClassifier::new();
        let config = AutoPostureConfig::new();
        classifier.update(&at(1.68), FRAME_S, &config);
        let status = classifier.update(&at(1.05), 3600.0, &config);
        assert_eq!(status.detected, DetectedPosture::Standing);
    }

    #[test]
    fn without_a_seated_calibration_the_seated_eyes_are_estimated() {
        let t = Thresholds::new(1.70, None);
        assert!(t.seated_estimated);
        assert!((t.seated_eye_m - 1.70 * SEATED_EYE_FRACTION).abs() < EPS);
        assert!(t.sit_below_m < t.midpoint_m && t.midpoint_m < t.stand_above_m);
        assert!(t.stand_above_m < t.standing_eye_m);
        // A calibration too close to the standing eyes is replaced by the estimate.
        assert!(Thresholds::new(1.70, Some(1.60)).seated_estimated);
        assert!(!Thresholds::new(1.70, Some(1.22)).seated_estimated);
        // The logged session's midpoint.
        let logged = Thresholds::new(1.699, Some(1.218));
        assert!((logged.midpoint_m - 1.4585).abs() < 1e-3);
    }

    #[test]
    fn standing_straight_puts_the_eyes_at_the_anchor() {
        let h = head_height(
            &HeadHeightInput {
                stage_height: Some(PLAYER.eye_height),
                cockpit_y: 0.3,
                ..input(EffectivePosture::Standing)
            },
            &PLAYER,
            &CHARACTER,
        );
        assert_eq!(h.source, HeadHeightSource::Floor);
        assert!(h.cockpit_y.abs() < EPS, "the baseline height is ignored");
    }

    #[test]
    fn a_standing_duck_scales_by_the_height_ratio() {
        let h = head_height(
            &HeadHeightInput {
                stage_height: Some(PLAYER.eye_height - 0.4),
                ..input(EffectivePosture::Standing)
            },
            &PLAYER,
            &CHARACTER,
        );
        let expected = -0.4 * CHARACTER.height / PLAYER.height;
        assert!((h.cockpit_y - expected).abs() < EPS);
    }

    #[test]
    fn seated_without_a_reference_travel_scales_from_the_baseline_and_the_toggle_drops_it() {
        let seated = HeadHeightInput {
            cockpit_y: -0.1,
            stage_height: Some(1.1),
            ..input(EffectivePosture::Seated)
        };
        let ratio = CHARACTER.height / PLAYER.height;
        let h = head_height(&seated, &PLAYER, &CHARACTER);
        assert_eq!(h.source, HeadHeightSource::Baseline);
        assert!((h.cockpit_y - (-0.1 * ratio)).abs() < EPS);

        let crouched = head_height(
            &HeadHeightInput {
                seated_crouch: true,
                ..seated
            },
            &PLAYER,
            &CHARACTER,
        );
        assert!((crouched.cockpit_y - (-0.1 * ratio - 0.6)).abs() < EPS);
    }

    #[test]
    fn the_seated_reference_is_measured_from_the_floor() {
        let ratio = CHARACTER.height / PLAYER.height;
        let seated = HeadHeightInput {
            // The recenter baseline was taken leaning; the floor measurement ignores it.
            cockpit_y: 0.3,
            stage_height: Some(1.1),
            seated_reference: reference(1.2, SeatedReferenceSource::Calibrated),
            ..input(EffectivePosture::Seated)
        };
        let h = head_height(&seated, &PLAYER, &CHARACTER);
        assert_eq!(
            h.source,
            HeadHeightSource::SeatedReference(SeatedReferenceSource::Calibrated)
        );
        assert!((h.cockpit_y - (-0.1 * ratio)).abs() < EPS);

        // Sitting at the reference puts the eyes at the anchor.
        let upright = head_height(
            &HeadHeightInput {
                stage_height: Some(1.2),
                ..seated
            },
            &PLAYER,
            &CHARACTER,
        );
        assert!(upright.cockpit_y.abs() < EPS);

        let crouched = head_height(
            &HeadHeightInput {
                seated_crouch: true,
                ..seated
            },
            &PLAYER,
            &CHARACTER,
        );
        assert!((crouched.cockpit_y - (-0.1 * ratio - 0.6)).abs() < EPS);

        // Without a floor, the recenter baseline stands in.
        let no_floor = head_height(
            &HeadHeightInput {
                stage_height: None,
                ..seated
            },
            &PLAYER,
            &CHARACTER,
        );
        assert_eq!(no_floor.source, HeadHeightSource::Baseline);
        assert!((no_floor.cockpit_y - 0.3 * ratio).abs() < EPS);

        // The standing postures ignore it.
        let standing = head_height(
            &HeadHeightInput {
                stage_height: Some(PLAYER.eye_height),
                ..input(EffectivePosture::Standing)
            },
            &PLAYER,
            &CHARACTER,
        );
        assert!(standing.cockpit_y.abs() < EPS);
    }

    /// The logged session: the calibrated reference put the view 0.12 to 0.22 m below Rico's eyes
    /// while the player sat normally; a reference captured from how they sit puts it on them.
    #[test]
    fn a_reference_captured_seated_puts_the_logged_head_at_the_anchor() {
        let player = BodyDimensions {
            height: 1.821,
            eye_height: 1.699,
            seated_eye_height: Some(1.218),
            ..BodyDimensions::GENERIC_ADULT
        };
        let sitting = HeadHeightInput {
            stage_height: Some(1.07),
            seated_reference: reference(1.218, SeatedReferenceSource::Calibrated),
            ..input(EffectivePosture::Seated)
        };
        let calibrated = head_height(&sitting, &player, &CHARACTER);
        assert!(calibrated.cockpit_y < -0.13, "{}", calibrated.cockpit_y);
        let captured = head_height(
            &HeadHeightInput {
                seated_reference: reference(1.07, SeatedReferenceSource::Recenter),
                ..sitting
            },
            &player,
            &CHARACTER,
        );
        assert!(captured.cockpit_y.abs() < EPS);
    }

    #[test]
    fn only_the_crouched_seated_posture_drops_the_eyes() {
        assert_eq!(seated_crouch_drop(EffectivePosture::Seated, true, 0.6), 0.6);
        assert_eq!(
            seated_crouch_drop(EffectivePosture::Seated, false, 0.6),
            0.0
        );
        assert_eq!(
            seated_crouch_drop(EffectivePosture::Standing, true, 0.6),
            0.0
        );
        assert_eq!(
            seated_crouch_drop(EffectivePosture::Seated, true, -1.0),
            0.0
        );
        assert_eq!(
            seated_crouch_drop(EffectivePosture::Seated, true, f32::NAN),
            0.0
        );
    }

    #[test]
    fn standing_without_a_floor_falls_back_to_the_baseline_without_the_crouch_toggle() {
        let h = head_height(
            &HeadHeightInput {
                cockpit_y: 0.2,
                seated_crouch: true,
                ..input(EffectivePosture::Standing)
            },
            &PLAYER,
            &CHARACTER,
        );
        assert_eq!(h.source, HeadHeightSource::Baseline);
        assert!((h.cockpit_y - 0.2 * CHARACTER.height / PLAYER.height).abs() < EPS);
    }
}
