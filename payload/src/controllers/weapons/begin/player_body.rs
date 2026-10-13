//! The player's body the sockets and pouches ride, as the headset puts it or as the character's
//! skeleton does.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    body::{
        self, BodyDimensions,
        rig::{
            self, skeleton,
            torso::{self, EYES_TO_NECK_PIVOT},
        },
    },
    config::ControllersConfig,
    hands::ActiveHands,
    weapons::{config::HolsterAnchor, volumes::Anchor},
};

/// The player's body as the headset puts it, for the body's sockets and pouches: where the eyes
/// would be on the upright neck, which way the torso faces, and where the feet are.
pub(super) struct PlayerBody {
    /// The eyes over the neck pivot, `(x, z)` in the cockpit frame.
    pub(super) upright: Vec2,
    pub(super) yaw: f32,
    /// The middle of the feet, `(x, z)`, while the rig's frame half runs.
    pub(super) feet: Option<Vec2>,
}

impl PlayerBody {
    /// The body from the torso estimate ([`rig::estimate`]) while it is fresh, or from the
    /// head's pose alone: the neck pivot below and behind the eyes, facing where the head does.
    pub(super) fn of(active: &ActiveHands) -> Self {
        let head = active.head;
        let estimate = rig::estimate();
        let yaw = estimate
            .map(|e| e.yaw)
            .or_else(|| torso::head_heading(head.orientation))
            .unwrap_or(0.0);
        let neck = estimate.map_or(head.position + head.orientation * EYES_TO_NECK_PIVOT, |e| {
            e.neck
        });
        let eyes = neck - Quat::from_rotation_y(yaw) * EYES_TO_NECK_PIVOT;
        Self {
            upright: Vec2::new(eyes.x, eyes.z),
            yaw,
            feet: rig::feet(),
        }
    }
}

/// The body the volumes ride: the character's skeleton when the setting asks for it and the
/// skeleton is fresh and upright, and otherwise the player's body as the headset puts it
/// (`player`), turned to the estimated torso with its waist over the feet.
pub(super) fn volume_anchor(
    config: &ControllersConfig,
    active: &ActiveHands,
    player: &PlayerBody,
) -> Anchor {
    let skeleton = (config.weapons.holsters.anchor == HolsterAnchor::Rico)
        .then(|| skeleton::skeleton_in_cockpit(active.head.position, active.world_scale))
        .flatten();
    let anchor = match skeleton {
        Some(skeleton) => Anchor::Skeleton(skeleton),
        None => Anchor::Body {
            yaw: player.yaw,
            pelvis: player.feet,
        },
    };
    skeleton::record_volume_anchor(anchor.label());
    anchor
}

/// The dimensions the volumes are placed with: the player's on the player's body; on the
/// character's skeleton, the character's while the hands are remapped onto them, and the player's
/// otherwise.
pub(super) fn body_dimensions(config: &ControllersConfig, anchor: &Anchor) -> BodyDimensions {
    match anchor {
        Anchor::Skeleton(_) if config.body.remap_hands => BodyDimensions::CHARACTER_ESTIMATE,
        _ => body::player(),
    }
}

/// Each hand's point for the body's volumes: the player's own grip, before the body-scale remap,
/// on the player's body, so that a hand on the player's own belt finds the pouch there; the grip
/// of the hand the player sees on the character's skeleton, whose landmarks are the character's.
pub(super) fn body_points(active: &ActiveHands, anchor: &Anchor) -> [Option<Vec3>; 2] {
    match anchor {
        Anchor::Body { .. } => active.player_grips,
        Anchor::Skeleton(_) => active.hands.map(|h| h.map(|h| h.grip.position)),
    }
}
