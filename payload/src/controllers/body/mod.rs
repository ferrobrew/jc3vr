//! The player's body dimensions and the Bonelab-style remap onto the character's proportions.
//!
//! The world stays at true scale, because vehicle sizes, building sizes, and the sense of speed
//! depend on it. Instead of scaling the world, the player's motion is remapped onto the character:
//!
//! - the eyes sit at the character's eye height, and vertical head travel (ducking, crouching)
//!   scales by the ratio of the two heights ([`remap_head_height`], [`remap_head_translation`]);
//! - horizontal head travel stays 1:1;
//! - the hands' offsets from the shoulders scale by the ratio of the arm lengths
//!   ([`ArmRemap`](crate::controllers::hands::reach::ArmRemap)).
//!
//! Keeping motion 1:1 against a fixed body instead (the Boneworks approach) reads to players as
//! permanently bent knees or a misplaced floor, which the remap avoids.
//!
//! # The player's dimensions
//!
//! The player's dimensions are the `body` section of the runtime configuration
//! ([`Config::body`](crate::config::Config::body)), saved in the settings file with everything
//! else. Every field defaults to [`BodyDimensions::GENERIC_ADULT`], and all values are metres:
//!
//! ```toml
//! [body]
//! height = 1.80
//! eye_height = 1.68
//! shoulder_height = 1.47
//! shoulder_width = 0.40
//! arm_span = 1.80
//! upper_arm = 0.34
//! forearm = 0.26
//! hand = 0.19
//! seated_eye_height = 1.21
//! ```
//!
//! `seated_eye_height` is the one field without a default: it is the head's height above the STAGE
//! floor while the player sits upright in their chair, and only a seated calibration (or the
//! player) can supply it. While it is set, the seated eye reference starts from it and the
//! automatic posture's thresholds sit against it ([`crate::controllers::roomscale::seated`],
//! [`crate::controllers::roomscale::posture`]).
//!
//! The load checks the dimensions with [`BodyDimensions::validate`] and falls back to the generic
//! adult ones when they cannot describe a body; it never stops the mod.
//!
//! The section can be written by hand, or by the calibration flow on the pause menu's `VR: BODY`
//! page ([`menu`]), which measures the player's eye height (standing or seated) and arm span in
//! the headset and derives the other dimensions from them ([`calibration`]) before saving them
//! with [`save`].

pub mod alignment;
pub mod calibration;
pub(crate) mod menu;
pub mod mirror;
pub mod procedural;
pub mod rig;
pub mod torso;

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::config::{self, Config};

/// A body's dimensions, in metres. Heights are measured from the floor.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
    /// The eye height while sitting upright in the player's chair, when calibrated. It belongs to
    /// the chair as much as to the body, so there is no generic default.
    pub seated_eye_height: Option<f32>,
}

impl BodyDimensions {
    /// Generic adult proportions, used until the player's own are saved.
    pub const GENERIC_ADULT: BodyDimensions = BodyDimensions {
        height: 1.75,
        eye_height: 1.63,
        shoulder_height: 1.44,
        shoulder_width: 0.39,
        arm_span: 1.75,
        upper_arm: 0.33,
        forearm: 0.26,
        hand: 0.19,
        seated_eye_height: None,
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
        seated_eye_height: None,
    };

    /// Check that every dimension is a positive finite length and that the heights are ordered
    /// (shoulders below the eyes, eyes below the top of the head, and the seated eyes, when set,
    /// below the standing eyes).
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
        if let Some(seated) = self.seated_eye_height {
            if !seated.is_finite() || seated <= 0.0 {
                return Err(InvalidDimension::NotPositive {
                    field: "seated_eye_height",
                    value: seated,
                });
            }
            if seated >= self.eye_height {
                return Err(InvalidDimension::Misordered {
                    lower: "seated_eye_height",
                    upper: "eye_height",
                });
            }
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

/// The player's body dimensions.
pub fn player() -> BodyDimensions {
    Config::lock_query(|c| c.body)
}

/// Whether the player's dimensions are still the generic adult ones, for a readout.
pub fn is_generic() -> bool {
    player() == BodyDimensions::GENERIC_ADULT
}

/// Validate `dimensions` and use them from now on, saving them with the rest of the settings. On
/// failure, the dimensions in use are unchanged.
pub fn save(dimensions: BodyDimensions) -> Result<(), InvalidDimension> {
    dimensions.validate()?;
    config::edit().body = dimensions;
    tracing::info!(target: "controllers", ?dimensions, "body dimensions saved");
    Ok(())
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

/// How far the eyes sit in front of the shoulder joints, metres. A generic anthropometric figure;
/// the settings do not carry it because it varies little between adults.
pub const EYES_FORWARD_OF_SHOULDERS: f32 = 0.09;

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
        seated_eye_height: None,
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
        seated_eye_height: None,
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
        let h = remap_head_height(1.2, &PLAYER, &PLAYER);
        assert!((h - 1.2).abs() < EPS);
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
    fn the_seated_eye_height_is_optional_and_below_the_standing_eyes() {
        let seated = |height| BodyDimensions {
            seated_eye_height: Some(height),
            ..PLAYER
        };
        assert_eq!(seated(1.2).validate(), Ok(()));
        assert_eq!(PLAYER.validate(), Ok(()));
        assert!(matches!(
            seated(1.7).validate(),
            Err(InvalidDimension::Misordered {
                lower: "seated_eye_height",
                ..
            })
        ));
        assert!(matches!(
            seated(-1.0).validate(),
            Err(InvalidDimension::NotPositive { .. })
        ));
    }

    #[test]
    fn impossible_dimensions_are_errors() {
        let no_span = BodyDimensions {
            arm_span: 0.0,
            ..PLAYER
        };
        assert!(matches!(
            no_span.validate(),
            Err(InvalidDimension::NotPositive {
                field: "arm_span",
                ..
            })
        ));
        let eyes_above_head = BodyDimensions {
            eye_height: 1.9,
            ..PLAYER
        };
        assert!(matches!(
            eyes_above_head.validate(),
            Err(InvalidDimension::Misordered { .. })
        ));
    }
}
