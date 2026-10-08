//! The pure parts of a gun's regions: the gun's shape in its own frame, which region a point on it
//! is in, where a hand holding it there closes, and how a gun held at a point other than its grip
//! sits on the hand.
//!
//! Everything is in the gun's own frame, the frame of its world matrix and of its model: metres, +Y
//! up, -Z toward the muzzle, and the origin at the attach point, which sits in the palm of the hand
//! holding the pistol grip.

use glam::{Quat, Vec3};

use crate::controllers::{
    fingers::grip::catalogue::GripPose, frame::Pose, hands::pose as hand_pose,
};

/// A part of a gun a hand can hold it by.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum GunRegion {
    /// The pistol grip, the only place a hand fires the gun from.
    Grip,
    /// The stock behind the grip, on a long gun.
    Stock,
    /// The magazine below the receiver, on a long gun.
    Magazine,
    /// The slide, charging handle, bolt, or pump: where the action is worked.
    Action,
    /// The handguard or foregrip ahead of the receiver, and the receiver itself, on a long gun.
    Handguard,
    /// The barrel out to the muzzle.
    Barrel,
}

impl GunRegion {
    pub const fn label(self) -> &'static str {
        match self {
            GunRegion::Grip => "grip",
            GunRegion::Stock => "stock",
            GunRegion::Magazine => "magazine",
            GunRegion::Action => "action",
            GunRegion::Handguard => "handguard",
            GunRegion::Barrel => "barrel",
        }
    }

    /// Whether a hand holding the gun here fires it: only the pistol grip's.
    pub const fn fires(self) -> bool {
        matches!(self, GunRegion::Grip)
    }

    /// Whether a second hand holding the gun here steers it: a hand ahead of the grip sets the
    /// line the gun points along, and a hand on the stock or the grip only holds it.
    pub const fn steers(self) -> bool {
        matches!(
            self,
            GunRegion::Handguard | GunRegion::Barrel | GunRegion::Magazine | GunRegion::Action
        )
    }

    /// Whether a bracing hand slides along the gun in this region, following the hand along the
    /// line it wraps.
    pub const fn slides(self) -> bool {
        matches!(self, GunRegion::Handguard | GunRegion::Barrel)
    }

    /// The fingers' pose for a hand holding the gun here.
    pub const fn finger_pose(self) -> GripPose {
        match self {
            GunRegion::Grip => GripPose::PistolGrip,
            GunRegion::Handguard => GripPose::Foregrip,
            GunRegion::Barrel => GripPose::Cylinder {
                diameter_m: BARREL_DIAMETER_M,
            },
            GunRegion::Stock => GripPose::Cylinder {
                diameter_m: STOCK_DIAMETER_M,
            },
            GunRegion::Magazine => GripPose::MagazineCup,
            GunRegion::Action => GripPose::SlidePinch,
        }
    }
}

/// A magazine well: where a round enters the gun, and the direction it travels in.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MagazineWell {
    pub mouth: Vec3,
    /// Unit length, into the gun.
    pub axis: Vec3,
}

/// A gun's shape and its known points, in its own frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GunShape {
    /// The model's bounding box.
    pub min: Vec3,
    pub max: Vec3,
    /// The muzzle: the current fire point.
    pub muzzle: Vec3,
    /// The other hand's grip point, when the gun has one.
    pub foregrip: Option<Vec3>,
    /// The magazine well, when the wells table has one.
    pub magazine: Option<MagazineWell>,
    /// Where the action is worked, when the wells table has one.
    pub action: Option<Vec3>,
    /// A long gun (the two-handed, special, and heavy slots), with a stock and a handguard; a
    /// pistol has only its grip, its action, and its barrel.
    pub long: bool,
}

impl GunShape {
    /// Whether the shape can be classified against: a finite box with some length along the bore.
    pub fn is_usable(&self) -> bool {
        self.min.is_finite()
            && self.max.is_finite()
            && self.muzzle.is_finite()
            && self.max.cmpge(self.min).all()
            && self.max.z - self.min.z > MIN_LENGTH_M
    }

    /// How far along the bore from the grip the box ends at the back, metres (negative behind the
    /// grip).
    fn rear(&self) -> f32 {
        along(self.max)
    }

    /// How far along the bore the muzzle is, metres; the box's front without a muzzle ahead of
    /// the grip.
    fn muzzle_along(&self) -> f32 {
        let muzzle = along(self.muzzle);
        if muzzle > GRIP_REACH_M {
            muzzle
        } else {
            along(self.min)
        }
    }

    /// Where the handguard ends: past the foregrip, or most of the way to the muzzle without one.
    fn handguard_end(&self) -> f32 {
        let end = self.muzzle_along();
        self.foregrip
            .map_or(end * HANDGUARD_SHARE, |f| along(f) + PAST_FOREGRIP_M)
            .min(end)
    }
}

/// The bore's direction in the gun's frame.
pub const BORE: Vec3 = Vec3::NEG_Z;

/// Where a hand touched a gun.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RegionHit {
    pub region: GunRegion,
    /// Where the hand closes on the gun, in the gun's frame: the region's own point, or the point
    /// on the line the region runs along nearest the hand.
    pub anchor: Vec3,
    /// How far the touch was from the anchor, metres.
    pub distance: f32,
}

/// The region of `shape` the point `point` (in the gun's frame) touches, with a hand of grip radius
/// `grip_radius` that reaches `reach` beyond the gun's box. `None` for a point off the gun or an
/// unusable shape.
pub fn classify(shape: &GunShape, point: Vec3, grip_radius: f32, reach: f32) -> Option<RegionHit> {
    if !shape.is_usable() || !point.is_finite() {
        return None;
    }
    let outside = point - point.clamp(shape.min, shape.max);
    if outside.length() > reach.max(0.0) {
        return None;
    }
    let hit = |region, anchor: Vec3| RegionHit {
        region,
        anchor,
        distance: point.distance(anchor),
    };
    // The parts with a point of their own, nearest first by the share of their radius.
    let mut parts: Vec<(f32, RegionHit)> = Vec::new();
    let grip_segment = segment_point(point, Vec3::ZERO, GRIP_BOTTOM);
    let grip_radius = grip_radius.max(0.0);
    if grip_radius > 0.0 && point.distance(grip_segment) <= grip_radius {
        parts.push((
            point.distance(grip_segment) / grip_radius,
            hit(GunRegion::Grip, Vec3::ZERO),
        ));
    }
    if let Some(action) = shape.action.filter(|a| a.is_finite()) {
        let d = point.distance(action);
        if d <= ACTION_RADIUS_M {
            parts.push((d / ACTION_RADIUS_M, hit(GunRegion::Action, action)));
        }
    }
    // A pistol's magazine is in its grip.
    if shape.long
        && let Some(well) = shape
            .magazine
            .filter(|m| m.mouth.is_finite() && m.axis.is_finite())
        && well.mouth.length() > grip_radius
    {
        let end = well.mouth - well.axis.normalize_or_zero() * MAGAZINE_LENGTH_M;
        let on = segment_point(point, well.mouth, end);
        let d = point.distance(on);
        if d <= MAGAZINE_RADIUS_M {
            parts.push((d / MAGAZINE_RADIUS_M, hit(GunRegion::Magazine, on)));
        }
    }
    if let Some((_, nearest)) = parts.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
        return Some(nearest);
    }

    let along_bore = along(point);
    let muzzle_along = shape.muzzle_along();
    let on_line = |through: Vec3, along: f32| Vec3::new(through.x, through.y, -along);
    if !shape.long {
        return Some(if along_bore > GRIP_REACH_M {
            hit(
                GunRegion::Barrel,
                on_line(shape.muzzle, along_bore.clamp(GRIP_REACH_M, muzzle_along)),
            )
        } else {
            hit(GunRegion::Grip, Vec3::ZERO)
        });
    }
    if along_bore < -GRIP_REACH_M {
        let rear = shape.rear().min(-GRIP_REACH_M);
        return Some(hit(
            GunRegion::Stock,
            on_line(Vec3::ZERO, along_bore.clamp(rear, -GRIP_REACH_M)),
        ));
    }
    let handguard_end = shape.handguard_end().max(GRIP_REACH_M);
    if along_bore <= handguard_end {
        let through = shape
            .foregrip
            .filter(|f| f.is_finite())
            .unwrap_or(shape.muzzle);
        return Some(hit(
            GunRegion::Handguard,
            on_line(through, along_bore.clamp(-GRIP_REACH_M, handguard_end)),
        ));
    }
    Some(hit(
        GunRegion::Barrel,
        on_line(
            shape.muzzle,
            along_bore.clamp(handguard_end, muzzle_along.max(handguard_end)),
        ),
    ))
}

/// How a hand holds a gun at a point other than its grip.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Carry {
    pub region: GunRegion,
    /// Where the hand closes on the gun, in the gun's frame.
    pub anchor: Vec3,
    /// The gun's rotation on the hand's wrist, kept from when the hand took it there.
    pub turn: Quat,
}

/// The gun's rotation on a wrist of rotation `wrist`, for a gun of rotation `gun` in the same
/// frame.
pub fn turn_on_wrist(wrist: Quat, gun: Quat) -> Quat {
    (wrist.inverse() * gun).normalize()
}

/// The pose that carries a gun held at its grip to the same gun held by `carry`, in the attach
/// frame, for a hand whose grip puts the attach frame at `grip` on its wrist: the gun turned as the
/// hand took it, and moved so that the anchor sits where the grip's attach point sits, in the palm.
/// The rigid grip composes it after the grip's pose on the wrist.
pub fn carry_offset(carry: &Carry, grip: Pose) -> Pose {
    let orientation = (grip.orientation.inverse() * carry.turn).normalize();
    Pose {
        position: -(orientation * carry.anchor),
        orientation,
    }
}

/// `point` in the frame `gun` gives, in a frame of `scale` gun metres per unit: the gun's pose is
/// in that frame's units, and the result in the gun's metres.
pub fn into_gun(gun: Pose, point: Vec3, scale: f32) -> Vec3 {
    hand_pose::mul(
        hand_pose::inverse(gun),
        Pose {
            position: point,
            orientation: Quat::IDENTITY,
        },
    )
    .position
        * scale
}

/// The point `point` in the gun's frame, in the frame `gun` is given in, of `scale` gun metres per
/// unit.
pub fn out_of_gun(gun: Pose, point: Vec3, scale: f32) -> Vec3 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    gun.position + gun.orientation * (point / scale)
}

/// A gun's length below which its box is not trusted, metres.
const MIN_LENGTH_M: f32 = 0.05;
/// The bottom of the pistol grip, from the attach point: down and a little back.
const GRIP_BOTTOM: Vec3 = Vec3::new(0.0, -0.1, 0.02);
/// How far along the bore either side of the grip the grip's hand reaches, metres: the receiver's
/// length the grip hand covers.
const GRIP_REACH_M: f32 = 0.06;
/// The radius of the action's handle, metres.
const ACTION_RADIUS_M: f32 = 0.045;
/// How far a magazine hangs from its well, and how thick it is, metres.
const MAGAZINE_LENGTH_M: f32 = 0.16;
const MAGAZINE_RADIUS_M: f32 = 0.05;
/// How far past the foregrip the handguard reaches, metres.
const PAST_FOREGRIP_M: f32 = 0.12;
/// The share of the way to the muzzle the handguard covers on a gun without a foregrip.
const HANDGUARD_SHARE: f32 = 0.6;
/// The diameters the fingers wrap on a barrel and on a stock, metres.
const BARREL_DIAMETER_M: f32 = 0.03;
const STOCK_DIAMETER_M: f32 = 0.045;

/// How far along the bore from the grip `point` is.
fn along(point: Vec3) -> f32 {
    point.dot(BORE)
}

/// The point on the segment from `a` to `b` nearest `point`.
fn segment_point(point: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let length_sq = ab.length_squared();
    if length_sq <= f32::EPSILON {
        return a;
    }
    a + ab * ((point - a).dot(ab) / length_sq).clamp(0.0, 1.0)
}
