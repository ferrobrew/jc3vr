//! The player's body dimensions and the Bonelab-style remap onto the character's proportions.
//!
//! The world stays at true scale, because vehicle sizes, building sizes, and the sense of speed
//! depend on it. Instead of scaling the world, the player's motion is remapped onto the character:
//!
//! - the eyes sit at the character's eye height, and vertical head travel (ducking, crouching)
//!   scales by the ratio of the two heights ([`remap_head_height`], [`remap_head_translation`]);
//! - horizontal head travel stays 1:1;
//! - the hands' offsets from the shoulders scale by the ratio of the arm spans
//!   ([`remap_hand_position`]).
//!
//! Keeping motion 1:1 against a fixed body instead (the Boneworks approach) reads to players as
//! permanently bent knees or a misplaced floor, which the remap avoids.
//!
//! # The body file
//!
//! The player's dimensions come from `body.local.json` beside the payload DLL, which is the
//! directory the injector runs from: `target/x86_64-pc-windows-msvc/<profile>/` for a local build
//! (the injector copies the payload to `jc3vr_payload_loaded.dll` in the same directory, and
//! [`crate::module::get_path`] names that copy). The file is git-ignored; it holds one player's
//! measurements. Every field is optional and defaults to [`BodyDimensions::GENERIC_ADULT`], all
//! values are metres, and an unknown field is an error so that a misspelt one is not silently
//! ignored:
//!
//! ```json
//! { "height": 1.80, "eye_height": 1.68, "shoulder_height": 1.47, "shoulder_width": 0.40,
//!   "arm_span": 1.80, "upper_arm": 0.34, "forearm": 0.26, "hand": 0.19 }
//! ```
//!
//! A missing file uses the defaults. A malformed file logs an error and uses the defaults; it
//! never stops the mod.
//!
//! The file can be written by hand, or by the calibration flow on the pause menu's `VR: BODY` page
//! ([`menu`]), which measures the player's eye height and arm span in the headset and derives the
//! other dimensions from them ([`calibration`]) before saving them with [`save`].

pub mod calibration;
pub(crate) mod menu;

use std::path::{Path, PathBuf};

use glam::{Quat, Vec3};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// A body's dimensions, in metres. Heights are measured from the floor.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BodyDimensions {
    /// Standing height.
    pub height: f32,
    /// Standing eye height.
    pub eye_height: f32,
    /// Standing height of the shoulder joints.
    pub shoulder_height: f32,
    /// The distance between the two shoulder joints.
    pub shoulder_width: f32,
    /// Fingertip to fingertip with the arms spread horizontally.
    pub arm_span: f32,
    /// Shoulder joint to elbow.
    pub upper_arm: f32,
    /// Elbow to wrist.
    pub forearm: f32,
    /// Wrist to the tip of the middle finger.
    pub hand: f32,
}

impl BodyDimensions {
    /// Generic adult proportions, used when no body file supplies a dimension.
    pub const GENERIC_ADULT: BodyDimensions = BodyDimensions {
        height: 1.75,
        eye_height: 1.63,
        shoulder_height: 1.44,
        shoulder_width: 0.39,
        arm_span: 1.75,
        upper_arm: 0.33,
        forearm: 0.26,
        hand: 0.19,
    };

    /// The character's proportions, estimated from his stated height of about 1.8 to 1.9 m and
    /// generic adult ratios. A stand-in until the dimensions are measured from the character's
    /// skeleton.
    pub const CHARACTER_ESTIMATE: BodyDimensions = BodyDimensions {
        height: 1.85,
        eye_height: 1.73,
        shoulder_height: 1.52,
        shoulder_width: 0.41,
        arm_span: 1.85,
        upper_arm: 0.35,
        forearm: 0.27,
        hand: 0.20,
    };

    /// Check that every dimension is a positive finite length and that the heights are ordered
    /// (shoulders below the eyes, eyes below the top of the head).
    pub fn validate(&self) -> Result<(), InvalidDimension> {
        for (field, value) in [
            ("height", self.height),
            ("eye_height", self.eye_height),
            ("shoulder_height", self.shoulder_height),
            ("shoulder_width", self.shoulder_width),
            ("arm_span", self.arm_span),
            ("upper_arm", self.upper_arm),
            ("forearm", self.forearm),
            ("hand", self.hand),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(InvalidDimension::NotPositive { field, value });
            }
        }
        if self.eye_height >= self.height {
            return Err(InvalidDimension::Misordered {
                lower: "eye_height",
                upper: "height",
            });
        }
        if self.shoulder_height >= self.eye_height {
            return Err(InvalidDimension::Misordered {
                lower: "shoulder_height",
                upper: "eye_height",
            });
        }
        Ok(())
    }
}

impl Default for BodyDimensions {
    fn default() -> Self {
        Self::GENERIC_ADULT
    }
}

/// A dimension that cannot describe a body.
#[derive(Debug, Clone, PartialEq)]
pub enum InvalidDimension {
    /// A length that is zero, negative, or not a number.
    NotPositive { field: &'static str, value: f32 },
    /// Two heights in the wrong order.
    Misordered {
        lower: &'static str,
        upper: &'static str,
    },
}

impl std::fmt::Display for InvalidDimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvalidDimension::NotPositive { field, value } => {
                write!(f, "`{field}` must be a positive length, not {value}")
            }
            InvalidDimension::Misordered { lower, upper } => {
                write!(f, "`{lower}` must be below `{upper}`")
            }
        }
    }
}

impl std::error::Error for InvalidDimension {}

/// Where the player's dimensions came from.
#[derive(Debug, Clone)]
pub enum BodySource {
    /// No body file exists; the generic adult defaults are in use.
    Defaults { path: Option<PathBuf> },
    /// The body file loaded, with any missing field defaulted.
    File { path: PathBuf },
    /// The body file exists but could not be used; the generic adult defaults are in use.
    Invalid { error: BodyFileError },
}

/// The player's body: the dimensions in use and where they came from.
#[derive(Debug, Clone)]
pub struct PlayerBody {
    pub dimensions: BodyDimensions,
    pub source: BodySource,
}

/// The player's body dimensions, loading the body file on first use.
pub fn player() -> BodyDimensions {
    with_player(|body| body.dimensions)
}

/// The player's body and where it came from, loading the body file on first use.
pub fn player_body() -> PlayerBody {
    with_player(Clone::clone)
}

/// Re-read the body file, for the debug UI after the file is edited.
pub fn reload() {
    *PLAYER.lock() = Some(load());
}

/// Validate `dimensions`, write them to the body file (replacing it whole), and use them from now
/// on. Returns the file's path. On failure, the file and the dimensions in use are unchanged.
pub fn save(dimensions: BodyDimensions) -> Result<PathBuf, BodyFileError> {
    let path = body_file_path().ok_or_else(|| BodyFileError {
        path: PathBuf::from(BODY_FILE_NAME),
        kind: BodyFileErrorKind::Write("the payload's path is unknown".to_owned()),
    })?;
    let error = |kind| BodyFileError {
        path: path.clone(),
        kind,
    };
    dimensions
        .validate()
        .map_err(|e| error(BodyFileErrorKind::Invalid(e)))?;
    let text = serde_json::to_string_pretty(&dimensions)
        .map_err(|e| error(BodyFileErrorKind::Write(e.to_string())))?;
    write_replacing(&path, text.as_bytes()).map_err(|e| error(BodyFileErrorKind::Write(e)))?;
    tracing::info!(target: "controllers", path = %path.display(), ?dimensions, "body dimensions saved");
    *PLAYER.lock() = Some(PlayerBody {
        dimensions,
        source: BodySource::File { path: path.clone() },
    });
    Ok(path)
}

/// The ratio of the character's height to the player's, which scales vertical head travel.
pub fn height_ratio(player: &BodyDimensions, character: &BodyDimensions) -> f32 {
    character.height / player.height
}

/// The ratio of the character's arm span to the player's, which scales the hands' offsets from
/// the shoulders.
pub fn arm_span_ratio(player: &BodyDimensions, character: &BodyDimensions) -> f32 {
    character.arm_span / player.arm_span
}

/// The character's eye height for a player whose eyes are `player_eye_height` above the floor.
/// Standing straight puts the eyes at the character's standing eye height, and every metre the
/// player ducks below their own standing eye height moves the character's eyes by the height
/// ratio.
pub fn remap_head_height(
    player_eye_height: f32,
    player: &BodyDimensions,
    character: &BodyDimensions,
) -> f32 {
    character.eye_height + (player_eye_height - player.eye_height) * height_ratio(player, character)
}

/// Remap a head translation (a change in the head position, in a frame with +Y up): the vertical
/// component scales by the height ratio, and the horizontal components stay 1:1 so that walking a
/// metre in the room walks a metre in the world.
pub fn remap_head_translation(
    delta: Vec3,
    player: &BodyDimensions,
    character: &BodyDimensions,
) -> Vec3 {
    Vec3::new(delta.x, delta.y * height_ratio(player, character), delta.z)
}

/// Estimate the shoulder joints `[left, right]` of a body from its eye position and torso heading.
/// The shoulders sit `eye_height - shoulder_height` below the eyes, spread by the shoulder width
/// along the torso's right axis, and [`EYES_FORWARD_OF_SHOULDERS`] behind the eyes. Only the yaw of
/// `torso` matters to a standing body, so the caller passes the torso's yaw rather than the head's
/// full orientation (the shoulders do not pitch when the head nods). Axes are OpenXR's: +X right,
/// +Y up, -Z forward.
pub fn shoulder_positions(eye_position: Vec3, torso: Quat, body: &BodyDimensions) -> [Vec3; 2] {
    let right = torso * Vec3::X;
    let back = torso * Vec3::Z;
    let centre = eye_position - Vec3::Y * (body.eye_height - body.shoulder_height)
        + back * EYES_FORWARD_OF_SHOULDERS;
    let half = right * (body.shoulder_width * 0.5);
    [centre - half, centre + half]
}

/// Remap a hand position from the player's body onto the character's: the hand's offset from the
/// player's shoulder, scaled by the arm-span ratio, applied at the character's shoulder. A hand at
/// full reach for the player is at full reach for the character.
pub fn remap_hand_position(
    hand: Vec3,
    player_shoulder: Vec3,
    character_shoulder: Vec3,
    player: &BodyDimensions,
    character: &BodyDimensions,
) -> Vec3 {
    character_shoulder + (hand - player_shoulder) * arm_span_ratio(player, character)
}

/// How far the eyes sit in front of the shoulder joints, metres. A generic anthropometric figure;
/// the body file does not carry it because it varies little between adults.
pub const EYES_FORWARD_OF_SHOULDERS: f32 = 0.09;

/// The body file's name, beside the payload DLL.
pub const BODY_FILE_NAME: &str = "body.local.json";

/// A body file that exists but could not be used.
#[derive(Debug, Clone)]
pub struct BodyFileError {
    pub path: PathBuf,
    pub kind: BodyFileErrorKind,
}

impl std::fmt::Display for BodyFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "body dimensions: {}: {}", self.path.display(), self.kind)
    }
}

impl std::error::Error for BodyFileError {}

/// Why a body file could not be used.
#[derive(Debug, Clone)]
pub enum BodyFileErrorKind {
    /// The file exists but could not be read.
    Read(String),
    /// The file is not a JSON object of known dimension fields.
    Parse(String),
    /// A dimension cannot describe a body.
    Invalid(InvalidDimension),
    /// The file could not be written.
    Write(String),
}

impl std::fmt::Display for BodyFileErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BodyFileErrorKind::Read(e) => write!(f, "could not read the file: {e}"),
            BodyFileErrorKind::Parse(e) => write!(f, "could not parse the file: {e}"),
            BodyFileErrorKind::Invalid(e) => write!(f, "invalid dimension: {e}"),
            BodyFileErrorKind::Write(e) => write!(f, "could not write the file: {e}"),
        }
    }
}

/// Parse a body file's text: every field optional, defaulted from
/// [`BodyDimensions::GENERIC_ADULT`], then validated.
pub fn parse_body_file(text: &str) -> Result<BodyDimensions, BodyFileErrorKind> {
    let dimensions: BodyDimensions =
        serde_json::from_str(text).map_err(|e| BodyFileErrorKind::Parse(e.to_string()))?;
    dimensions.validate().map_err(BodyFileErrorKind::Invalid)?;
    Ok(dimensions)
}

/// The loaded player body, `None` until first use.
static PLAYER: Mutex<Option<PlayerBody>> = Mutex::new(None);

fn with_player<R>(f: impl FnOnce(&PlayerBody) -> R) -> R {
    let mut slot = PLAYER.lock();
    f(slot.get_or_insert_with(load))
}

/// Load the body file beside the payload DLL, falling back to the defaults and logging why.
fn load() -> PlayerBody {
    let Some(path) = body_file_path() else {
        tracing::warn!(
            target: "controllers",
            "body dimensions: the payload's path is unknown; using the generic adult defaults",
        );
        return PlayerBody {
            dimensions: BodyDimensions::GENERIC_ADULT,
            source: BodySource::Defaults { path: None },
        };
    };
    match load_from(&path) {
        Ok(Some(dimensions)) => {
            tracing::info!(target: "controllers", path = %path.display(), ?dimensions, "body dimensions loaded");
            PlayerBody {
                dimensions,
                source: BodySource::File { path },
            }
        }
        Ok(None) => {
            tracing::info!(
                target: "controllers",
                path = %path.display(),
                "body dimensions: no body file; using the generic adult defaults",
            );
            PlayerBody {
                dimensions: BodyDimensions::GENERIC_ADULT,
                source: BodySource::Defaults { path: Some(path) },
            }
        }
        Err(error) => {
            tracing::error!(target: "controllers", "{error}; using the generic adult defaults");
            PlayerBody {
                dimensions: BodyDimensions::GENERIC_ADULT,
                source: BodySource::Invalid { error },
            }
        }
    }
}

fn body_file_path() -> Option<PathBuf> {
    Some(crate::module::get_path()?.parent()?.join(BODY_FILE_NAME))
}

/// Read and parse the body file at `path`. `Ok(None)` when the file does not exist.
fn load_from(path: &Path) -> Result<Option<BodyDimensions>, BodyFileError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(BodyFileError {
                path: path.to_owned(),
                kind: BodyFileErrorKind::Read(e.to_string()),
            });
        }
    };
    parse_body_file(&text)
        .map(Some)
        .map_err(|kind| BodyFileError {
            path: path.to_owned(),
            kind,
        })
}

/// Write `contents` to a temporary file beside `path` and move it over `path`, so a failed write
/// never leaves a truncated body file behind.
fn write_replacing(path: &Path, contents: &[u8]) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, contents).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        format!("{}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    const PLAYER: BodyDimensions = BodyDimensions {
        height: 1.80,
        eye_height: 1.68,
        shoulder_height: 1.47,
        shoulder_width: 0.40,
        arm_span: 1.80,
        upper_arm: 0.34,
        forearm: 0.26,
        hand: 0.19,
    };

    const SHORT_PLAYER: BodyDimensions = BodyDimensions {
        height: 1.50,
        eye_height: 1.39,
        shoulder_height: 1.22,
        shoulder_width: 0.35,
        arm_span: 1.48,
        upper_arm: 0.28,
        forearm: 0.22,
        hand: 0.17,
    };

    const CHARACTER: BodyDimensions = BodyDimensions::CHARACTER_ESTIMATE;

    #[test]
    fn the_defaults_and_the_character_estimate_are_valid() {
        BodyDimensions::GENERIC_ADULT.validate().unwrap();
        BodyDimensions::CHARACTER_ESTIMATE.validate().unwrap();
        PLAYER.validate().unwrap();
    }

    #[test]
    fn standing_straight_puts_the_eyes_at_the_character_eye_height() {
        let h = remap_head_height(PLAYER.eye_height, &PLAYER, &CHARACTER);
        assert!((h - CHARACTER.eye_height).abs() < EPS);
    }

    #[test]
    fn ducking_scales_by_the_height_ratio() {
        let duck = 0.5;
        let h = remap_head_height(SHORT_PLAYER.eye_height - duck, &SHORT_PLAYER, &CHARACTER);
        let expected = CHARACTER.eye_height - duck * CHARACTER.height / SHORT_PLAYER.height;
        assert!((h - expected).abs() < EPS);
        assert!(
            CHARACTER.eye_height - h > duck,
            "a shorter player's duck is amplified"
        );
    }

    #[test]
    fn horizontal_head_travel_is_one_to_one() {
        let delta = Vec3::new(0.3, -0.2, -0.7);
        let remapped = remap_head_translation(delta, &SHORT_PLAYER, &CHARACTER);
        assert_eq!(remapped.x, delta.x);
        assert_eq!(remapped.z, delta.z);
        assert!((remapped.y - delta.y * CHARACTER.height / SHORT_PLAYER.height).abs() < EPS);
    }

    #[test]
    fn the_same_body_remaps_to_itself() {
        let hand = Vec3::new(0.4, 1.1, -0.5);
        let shoulder = Vec3::new(0.2, 1.47, 0.0);
        let remapped = remap_hand_position(hand, shoulder, shoulder, &PLAYER, &PLAYER);
        assert!((remapped - hand).length() < EPS);
        let h = remap_head_height(1.2, &PLAYER, &PLAYER);
        assert!((h - 1.2).abs() < EPS);
    }

    #[test]
    fn full_reach_maps_to_full_reach() {
        let [_, player_shoulder] = shoulder_positions(Vec3::ZERO, Quat::IDENTITY, &SHORT_PLAYER);
        let [_, character_shoulder] = shoulder_positions(Vec3::ZERO, Quat::IDENTITY, &CHARACTER);
        let player_reach = SHORT_PLAYER.arm_span * 0.5;
        let hand = player_shoulder + Vec3::X * player_reach;
        let remapped = remap_hand_position(
            hand,
            player_shoulder,
            character_shoulder,
            &SHORT_PLAYER,
            &CHARACTER,
        );
        let reach = (remapped - character_shoulder).length();
        assert!((reach - CHARACTER.arm_span * 0.5).abs() < EPS);
    }

    #[test]
    fn shoulders_sit_below_and_behind_the_eyes_and_follow_the_torso_yaw() {
        let eyes = Vec3::new(1.0, PLAYER.eye_height, 2.0);
        let [left, right] = shoulder_positions(eyes, Quat::IDENTITY, &PLAYER);
        assert!((left.y - PLAYER.shoulder_height).abs() < EPS);
        assert!(((right - left).length() - PLAYER.shoulder_width).abs() < EPS);
        assert!(right.x > left.x);
        assert!(
            left.z > eyes.z,
            "the shoulders are behind the eyes (+Z is back)"
        );

        // Turned to face +X (a quarter turn clockwise seen from above), the right shoulder is
        // toward +Z.
        let turned = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        let [left, right] = shoulder_positions(eyes, turned, &PLAYER);
        assert!(right.z > left.z);
        assert!((right.x - left.x).abs() < EPS);
    }

    #[test]
    fn a_partial_body_file_defaults_the_missing_fields() {
        let body = parse_body_file(r#"{ "height": 1.9, "arm_span": 1.95 }"#).unwrap();
        assert_eq!(body.height, 1.9);
        assert_eq!(body.arm_span, 1.95);
        assert_eq!(body.forearm, BodyDimensions::GENERIC_ADULT.forearm);
    }

    #[test]
    fn a_full_body_file_parses() {
        let body = parse_body_file(
            r#"{ "height": 1.80, "eye_height": 1.68, "shoulder_height": 1.47,
                 "shoulder_width": 0.40, "arm_span": 1.80, "upper_arm": 0.34,
                 "forearm": 0.26, "hand": 0.19 }"#,
        )
        .unwrap();
        assert_eq!(body, PLAYER);
    }

    #[test]
    fn a_misspelt_field_is_an_error() {
        let err = parse_body_file(r#"{ "hieght": 1.9 }"#).unwrap_err();
        assert!(matches!(err, BodyFileErrorKind::Parse(_)));
    }

    #[test]
    fn malformed_json_is_an_error() {
        let err = parse_body_file("{ height: ").unwrap_err();
        assert!(matches!(err, BodyFileErrorKind::Parse(_)));
    }

    #[test]
    fn impossible_dimensions_are_errors() {
        let err = parse_body_file(r#"{ "arm_span": 0 }"#).unwrap_err();
        assert!(matches!(
            err,
            BodyFileErrorKind::Invalid(InvalidDimension::NotPositive {
                field: "arm_span",
                ..
            })
        ));
        let err = parse_body_file(r#"{ "eye_height": 1.9, "height": 1.8 }"#).unwrap_err();
        assert!(matches!(
            err,
            BodyFileErrorKind::Invalid(InvalidDimension::Misordered { .. })
        ));
    }
}
