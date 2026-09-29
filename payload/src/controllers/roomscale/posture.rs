//! The player's posture and the head height it implies. Pure.
//!
//! The posture decides where the eyes sit and whether the body chases the head:
//!
//! - seated: the eye height at the last recenter (the calibrated seated eye height) maps to the
//!   character's standing eye height, and vertical travel from it scales by the height ratio; the
//!   crouch toggle lowers the eyes by a preset;
//! - standing: the head's true height above the STAGE floor is remapped onto the character
//!   ([`remap_head_height`]), so that ducking and crouching are physical;
//! - roomscale: standing, with the chase loop on.
//!
//! In the automatic posture the play area picks standing or roomscale; seated is never chosen
//! automatically, because OpenXR has no signal for it.

use glam::{Vec2, Vec3};

use crate::controllers::{
    body::{BodyDimensions, remap_head_height, remap_head_translation},
    config::Posture,
};

/// The posture in effect after the automatic setting is resolved.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EffectivePosture {
    Seated,
    Standing,
    Roomscale,
}

impl EffectivePosture {
    pub fn label(self) -> &'static str {
        match self {
            EffectivePosture::Seated => "seated",
            EffectivePosture::Standing => "standing",
            EffectivePosture::Roomscale => "roomscale",
        }
    }
}

/// Resolve the posture setting against the play area's `bounds` (width and depth, metres; `None`
/// when the runtime reports none). The automatic setting takes roomscale when both sides reach
/// `min_extent_m`, and standing otherwise.
pub fn resolve(setting: Posture, bounds: Option<Vec2>, min_extent_m: f32) -> EffectivePosture {
    match setting {
        Posture::Seated => EffectivePosture::Seated,
        Posture::Standing => EffectivePosture::Standing,
        Posture::Roomscale => EffectivePosture::Roomscale,
        Posture::Auto => match bounds {
            Some(extent) if extent.min_element() >= min_extent_m => EffectivePosture::Roomscale,
            _ => EffectivePosture::Standing,
        },
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
    /// Whether the seated crouch toggle is down.
    pub seated_crouch: bool,
    /// How far the seated crouch lowers the eyes, metres at the character's scale.
    pub seated_drop_m: f32,
}

/// Where the head height came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeadHeightSource {
    /// The STAGE floor.
    Floor,
    /// The recenter baseline, as the seated eye height (the seated posture, or a standing posture
    /// on a runtime without a STAGE space).
    Baseline,
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
/// Otherwise the baseline is the calibrated eye height, and travel from it scales by the height
/// ratio, less the seated crouch.
pub fn head_height(
    input: &HeadHeightInput,
    player: &BodyDimensions,
    character: &BodyDimensions,
) -> HeadHeight {
    match (input.posture, input.stage_height) {
        (EffectivePosture::Standing | EffectivePosture::Roomscale, Some(height)) => HeadHeight {
            cockpit_y: remap_head_height(height, player, character) - character.eye_height,
            source: HeadHeightSource::Floor,
        },
        (posture, _) => {
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

    fn input(posture: EffectivePosture) -> HeadHeightInput {
        HeadHeightInput {
            posture,
            cockpit_y: 0.0,
            stage_height: None,
            seated_crouch: false,
            seated_drop_m: 0.6,
        }
    }

    #[test]
    fn auto_takes_roomscale_only_with_a_large_enough_play_area() {
        let resolve_auto = |bounds| resolve(Posture::Auto, bounds, 1.0);
        assert_eq!(resolve_auto(None), EffectivePosture::Standing);
        assert_eq!(
            resolve_auto(Some(Vec2::new(0.5, 3.0))),
            EffectivePosture::Standing
        );
        assert_eq!(
            resolve_auto(Some(Vec2::new(2.0, 1.5))),
            EffectivePosture::Roomscale
        );
    }

    #[test]
    fn an_explicit_posture_ignores_the_play_area() {
        let big = Some(Vec2::splat(4.0));
        assert_eq!(resolve(Posture::Seated, big, 1.0), EffectivePosture::Seated);
        assert_eq!(
            resolve(Posture::Standing, big, 1.0),
            EffectivePosture::Standing
        );
        assert_eq!(
            resolve(Posture::Roomscale, None, 1.0),
            EffectivePosture::Roomscale
        );
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
                ..input(EffectivePosture::Roomscale)
            },
            &PLAYER,
            &CHARACTER,
        );
        let expected = -0.4 * CHARACTER.height / PLAYER.height;
        assert!((h.cockpit_y - expected).abs() < EPS);
    }

    #[test]
    fn seated_travel_scales_from_the_baseline_and_the_toggle_drops_it() {
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
