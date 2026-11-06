//! The overlay's submission types: what a producer asks the overlay to draw.
//!
//! Positions are in the game's world space. A position may instead be anchored to a hand, so that
//! the renderer places it from the render-time hand pose ([`crate::controllers::hands::late`]) and
//! a hand-attached end tracks the controller at frame rate even when its producer submits at the
//! sim tick's rate.

use glam::Vec3;

use crate::controllers::frame::Hand;

/// Who submitted a set of primitives. Each source owns one retained slot, which a submission
/// replaces wholesale.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Source {
    /// The highlight on the grapple's target.
    GrappleHighlight,
    /// The gauntlet beam from the grapple wrist to the grapple's target.
    GrappleBeam,
    /// The guns' dot or laser.
    GunReadout,
    /// The grip targets' markers ([`crate::controllers::grab::markers`]).
    GrabMarkers,
    /// The ghosts of yielded hands at their controllers.
    GhostHands,
    /// Axes at the controllers' grip and aim poses.
    DebugGizmos,
    /// The wingsuit's arm-steering guide.
    WingGuide,
    /// The ring at a hand holding a primed grenade, warning of its fuse.
    GrenadeWarning,
    /// The armed grapple snatch's latch on its target ([`crate::controllers::snatch`]).
    SnatchLatch,
    /// The sight tuning tool's line-up marker.
    SightTuning,
    /// The laser of the hand pointing at the mod's debug panel.
    PanelPointer,
    /// The detonator button on the off-hand's wrist.
    WristDetonator,
    /// The physical hands' palm boxes and contacts.
    PhysicalHands,
    /// The body mirror's reflection of Rico's skeleton.
    BodyMirror,
    /// The body mirror's render-time pouch models and copied input targets.
    BodyMirrorPouches,
    /// The grounded body rig's neck and waist pivots, spine, and lean rings
    /// ([`crate::controllers::body::rig::visual`]).
    BodyRig,
    /// The grab view: the reach rays, the lines to the grip targets, the palms' state, the refused
    /// grabs, and the candidates near the hands
    /// ([`crate::controllers::physical_hands::grab_view`]).
    GrabView,
    /// The collider view's bodies, joints, and contacts. Last, so that it gives way first to the
    /// overlay's cap.
    ColliderView,
}

impl Source {
    pub const COUNT: usize = 18;

    /// Every source, in index order.
    pub const ALL: [Source; Source::COUNT] = [
        Source::GrappleHighlight,
        Source::GrappleBeam,
        Source::GunReadout,
        Source::GrabMarkers,
        Source::GhostHands,
        Source::DebugGizmos,
        Source::WingGuide,
        Source::GrenadeWarning,
        Source::SnatchLatch,
        Source::SightTuning,
        Source::PanelPointer,
        Source::WristDetonator,
        Source::PhysicalHands,
        Source::BodyMirror,
        Source::BodyMirrorPouches,
        Source::BodyRig,
        Source::GrabView,
        Source::ColliderView,
    ];

    pub const fn index(self) -> usize {
        match self {
            Source::GrappleHighlight => 0,
            Source::GrappleBeam => 1,
            Source::GunReadout => 2,
            Source::GrabMarkers => 3,
            Source::GhostHands => 4,
            Source::DebugGizmos => 5,
            Source::WingGuide => 6,
            Source::GrenadeWarning => 7,
            Source::SnatchLatch => 8,
            Source::SightTuning => 9,
            Source::PanelPointer => 10,
            Source::WristDetonator => 11,
            Source::PhysicalHands => 12,
            Source::BodyMirror => 13,
            Source::BodyMirrorPouches => 14,
            Source::BodyRig => 15,
            Source::GrabView => 16,
            Source::ColliderView => 17,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Source::GrappleHighlight => "grapple highlight",
            Source::GrappleBeam => "grapple beam",
            Source::GunReadout => "gun readout",
            Source::GrabMarkers => "grab markers",
            Source::GhostHands => "ghost hands",
            Source::DebugGizmos => "controller gizmos",
            Source::WingGuide => "wingsuit guide",
            Source::GrenadeWarning => "grenade fuse warning",
            Source::SnatchLatch => "snatch latch",
            Source::SightTuning => "sight tuning marker",
            Source::PanelPointer => "debug panel pointer",
            Source::WristDetonator => "wrist detonator",
            Source::PhysicalHands => "physical hands",
            Source::BodyMirror => "body mirror",
            Source::BodyMirrorPouches => "body mirror pouch render placement",
            Source::BodyRig => "body rig",
            Source::GrabView => "grab view",
            Source::ColliderView => "collider view",
        }
    }
}

/// Which of a hand's render-time poses an anchor follows.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandPose {
    /// The character's wrist frame.
    Wrist,
    /// The controller's grip pose.
    Grip,
    /// The controller's aim pose.
    Aim,
}

/// Where a point is.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Anchor {
    /// A fixed world position.
    World(Vec3),
    /// `offset`, in the local frame of one of a hand's poses, placed at render time from that
    /// pose. A primitive with a hand anchor is not drawn while the hand has no render-time pose.
    Hand {
        hand: Hand,
        pose: HandPose,
        offset: Vec3,
    },
    /// An offset in world axes from the eyes, placed at render time from the render camera's
    /// position. The tick measures it from the eye midpoint the camera renders from
    /// ([`crate::hooks::camera::eye_point`]), the cockpit frame's origin in the world. For a point
    /// that rides with the player, such as one placed in the cockpit frame, which a world anchor
    /// placed at the tick rate trails while the player moves fast.
    Head(Vec3),
}

impl Anchor {
    /// The origin of one of a hand's poses.
    pub const fn hand(hand: Hand, pose: HandPose) -> Anchor {
        Anchor::Hand {
            hand,
            pose,
            offset: Vec3::ZERO,
        }
    }
}

/// A sprite's diameter.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SpriteSize {
    /// Metres.
    World(f32),
    /// Radians, seen from the head, so that the sprite keeps its apparent size at any distance.
    Angular(f32),
}

/// Which way a sprite faces.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Facing {
    /// Toward the head, the same for both eyes.
    Billboard,
    /// Lying on a surface with this normal.
    Surface { normal: Vec3 },
}

/// A sprite's shape within its square.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Shape {
    /// A filled disc.
    Disc,
    /// A ring, `thickness` as a fraction of the radius.
    Ring { thickness: f32 },
}

/// How a primitive meets the scene's depth.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum DepthMode {
    /// Hidden where the scene is in front of it.
    Tested,
    /// Drawn with its alpha scaled by `occluded_alpha` where the scene is in front of it.
    XRay { occluded_alpha: f32 },
    /// Drawn over the scene regardless of depth.
    Always,
}

impl DepthMode {
    /// The alpha scale of the primitive's occluded pixels: zero hides them and one ignores depth.
    pub fn occluded_alpha(self) -> f32 {
        match self {
            DepthMode::Tested => 0.0,
            DepthMode::XRay { occluded_alpha } => occluded_alpha.clamp(0.0, 1.0),
            DepthMode::Always => 1.0,
        }
    }
}

/// A straight-alpha colour.
pub type Rgba = [f32; 4];

/// `colour` with its alpha replaced by `alpha`.
pub fn with_alpha([r, g, b, _]: Rgba, alpha: f32) -> Rgba {
    [r, g, b, alpha]
}

/// `colour` with its alpha scaled by `scale`, clamped to 0 to 1.
pub fn scale_alpha([r, g, b, a]: Rgba, scale: f32) -> Rgba {
    [r, g, b, a * scale.clamp(0.0, 1.0)]
}

/// The colour `t` of the way from `a` to `b`, with `t` clamped to 0 to 1.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// A primitive to draw.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Primitive {
    /// A disc or ring.
    Sprite {
        at: Anchor,
        size: SpriteSize,
        facing: Facing,
        shape: Shape,
        color: Rgba,
        depth: DepthMode,
    },
    /// A segment `width` metres thick, never thinner than the configured minimum on screen.
    Line {
        from: Anchor,
        to: Anchor,
        width: f32,
        color: Rgba,
        depth: DepthMode,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_alpha_replaces_and_scale_alpha_scales() {
        let colour = [0.2, 0.4, 0.6, 0.5];
        assert_eq!(with_alpha(colour, 0.8), [0.2, 0.4, 0.6, 0.8]);
        assert_eq!(scale_alpha(colour, 0.5), [0.2, 0.4, 0.6, 0.25]);
        assert_eq!(scale_alpha(colour, 3.0), colour, "the scale clamps to one");
        assert_eq!(scale_alpha(colour, -1.0)[3], 0.0);
    }

    #[test]
    fn mix_runs_from_one_colour_to_the_other() {
        let (a, b) = ([0.0, 0.0, 0.0, 0.0], [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), [0.5, 0.25, 0.125, 0.5]);
        assert_eq!(mix(a, b, 2.0), b);
    }
}
