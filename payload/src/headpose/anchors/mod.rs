//! Owned pre-override geometry, advanced only by synchronous native pose publications.

use std::time::{Duration, Instant};

use body_rig::rotation::twist_about_y;
use glam::{Mat4, Quat, Vec3};
use parking_lot::Mutex;

use crate::{
    controllers::body::rig::origin_provenance::OriginEvidence,
    headpose::{Anchors, Source, participants::head_participants, skeleton::SkeletonKey, teleport},
    vr::live_tracking,
};

#[cfg(test)]
pub(crate) mod tests;

/// One complete camera-anchor endpoint, in the captured simulation root's world frame.
#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub(crate) struct Geometry {
    pub root: Mat4,
    pub collapsed: bool,
    pub head: Vec3,
    pub neck_delta: Vec3,
    pub eye_arm: Vec3,
    /// The filtered head frame relative to the root, with the bind orientation removed.
    pub animation_rotation: Quat,
    /// Whether the camera hangs from the neutral head below rather than the animated one.
    pub camera_stable: bool,
    /// The head the camera hangs from while the root carries the body: the neutral head over the
    /// smoothed root, so a walking animation's bob does not reach the view.
    pub camera_head: Vec3,
    /// The world-space head-to-neck vector paired with [`camera_head`](Self::camera_head).
    pub camera_neck_delta: Vec3,
    /// The body-frame neck-to-eye arm paired with [`camera_head`](Self::camera_head).
    pub camera_eye_arm: Vec3,
    /// How far the camera has handed over to the root-held head, from zero to one: the view's
    /// frame turns toward the root's upright heading by this much (see
    /// [`view_frame`](Self::view_frame)).
    pub camera_weight: f32,
    /// The lowest the view's head may stand, world height: the ground under Rico plus a clearance,
    /// so a clip or a ragdoll that lays his head on or into the ground never takes the camera
    /// under it. `None` where the ground is not known.
    pub floor: Option<f32>,
}

impl Geometry {
    /// A collapsed pair shares the current follow and view frame as well as its geometry.
    pub(crate) fn previous_endpoint<T>(self, previous: T, current: T) -> T {
        if self.collapsed { current } else { previous }
    }

    /// The head the view hangs from: the neutral [`camera_head`](Self::camera_head) while it is
    /// [`camera_stable`](Self::camera_stable), else the animated [`head`](Self::head). The headset's
    /// pose is composed from it, so it is also the point a consumer subtracts from that pose to
    /// have the tracked offset.
    pub(crate) fn view_head(self) -> Vec3 {
        let head = if self.camera_stable {
            self.camera_head
        } else {
            self.head
        };
        match self.floor {
            Some(floor) if head.y < floor => Vec3::new(head.x, floor, head.z),
            _ => head,
        }
    }

    /// The frame the view composes the tracked head onto: the root, turned toward its upright
    /// heading by [`camera_weight`](Self::camera_weight). A clip's head frame never turns the view,
    /// so a deployment or an entry that pitches Rico's head leaves the player's head angle alone,
    /// and while the view hangs from the root-held head a root the physics tilts (a ragdoll's) does
    /// not turn it either.
    pub(crate) fn view_frame(self) -> Quat {
        let (_, root, _) = self.root.to_scale_rotation_translation();
        toward_heading(root, root, self.camera_weight)
    }

    /// The frame the camera's eye arm turns with: the root with the animated head frame, turned
    /// toward the root's upright heading as [`view_frame`](Self::view_frame) is, so the arm stays
    /// at rest under a root-held view.
    pub(crate) fn arm_frame(self) -> Quat {
        let (_, root, _) = self.root.to_scale_rotation_translation();
        toward_heading(root * self.animation_rotation, root, self.camera_weight)
    }

    pub(crate) fn from_anchors(anchors: Anchors, root: Mat4) -> Self {
        Self {
            root,
            collapsed: false,
            head: anchors.head,
            neck_delta: anchors.neck - anchors.head,
            eye_arm: anchors.eye_arm,
            animation_rotation: Quat::IDENTITY,
            camera_stable: false,
            camera_head: anchors.head,
            camera_neck_delta: anchors.neck - anchors.head,
            camera_eye_arm: anchors.eye_arm,
            camera_weight: 0.0,
            floor: None,
        }
    }

    pub(crate) fn valid(self) -> bool {
        self.root.is_finite()
            && self.root.determinant().abs() > f32::EPSILON
            && self.head.is_finite()
            && self.head.length_squared() <= 100_000.0_f32.powi(2)
            && self.neck_delta.is_finite()
            && self.neck_delta.length_squared() < 1.0
            && self.eye_arm.is_finite()
            && self.eye_arm.length_squared() < 1.0
            && self.animation_rotation.is_finite()
            && (self.animation_rotation.length_squared() - 1.0).abs() < 0.01
    }
}

/// `frame` turned toward the upright heading of `root` by `weight`, clamped to one.
fn toward_heading(frame: Quat, root: Quat, weight: f32) -> Quat {
    if weight > 0.0 {
        frame.slerp(twist_about_y(root), weight.min(1.0))
    } else {
        frame
    }
}

/// The allocations and bone selection checked at the pre-finalize seam.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub character: usize,
    pub skeleton: SkeletonKey,
    pub producer: usize,
    pub bones: [u32; 4],
}

/// Source context whose changes invalidate interpolation, not a body-authority decision.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Context {
    pub source: Source,
    pub gameplay: bool,
    pub teleport: u64,
    pub recenter: u32,
    pub world_scale: f32,
    pub tracking: Option<Publication>,
}

impl Context {
    pub(crate) fn current() -> Self {
        let now = Instant::now();
        let tracking = live_tracking::status()
            .packet
            .filter(|packet| {
                packet.focused
                    && packet.head.is_valid()
                    && now
                        .checked_duration_since(packet.published_at)
                        .is_some_and(|age| age <= Duration::from_millis(250))
                    && matches!(packet.origin.finish(), OriginEvidence::Known(_))
            })
            .map(|packet| Publication {
                sequence: packet.sequence,
                display_time_ns: packet.display_time_ns,
                published_at: packet.published_at,
                origin: packet.origin,
            });
        Self {
            source: crate::headpose::source(),
            gameplay: crate::hooks::in_gameplay() && !(head_participants().ui_mode)(),
            teleport: teleport::epoch(),
            recenter: crate::vr::recenter_count(),
            world_scale: crate::config::Config::lock_query(|c| c.vr.world_scale),
            tracking,
        }
    }

    fn coherent(self, next: Self) -> bool {
        self.continuous(next)
    }

    pub(crate) fn continuous(self, next: Self) -> bool {
        if self.source != next.source
            || self.gameplay != next.gameplay
            || self.teleport != next.teleport
            || self.recenter != next.recenter
            || self.world_scale != next.world_scale
        {
            return false;
        }
        if next.source == Source::Sim {
            return true;
        }
        match (self.tracking, next.tracking) {
            (Some(a), Some(b)) => {
                matches!(a.origin, OriginEvidence::Known(_))
                    && a.origin == b.origin
                    && (b == a
                        || (b.sequence > a.sequence
                            && b.display_time_ns >= a.display_time_ns
                            && b.published_at >= a.published_at))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Publication {
    pub sequence: u64,
    pub display_time_ns: i64,
    pub published_at: Instant,
    pub origin: OriginEvidence,
}

/// Current joints and root copied before any procedural or HumanIK override.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Sample {
    pub root: Mat4,
    pub head_model: Vec3,
    pub neck_model: Vec3,
    pub eyes_model: Vec3,
    pub head_orientation: Quat,
    pub neck_orientation: Quat,
    pub geometry: Geometry,
}

impl Sample {
    pub(crate) fn animated(root: Mat4, points: [Vec3; 3], rotations: [Quat; 2]) -> Self {
        let [head_model, neck_model, eyes_model] = points;
        let (_, rotation, _) = root.to_scale_rotation_translation();
        let head = root.transform_point3(head_model);
        let neck = root.transform_point3(neck_model);
        Self {
            root,
            head_model,
            neck_model,
            eyes_model,
            head_orientation: rotations[0],
            neck_orientation: rotations[1],
            geometry: Geometry {
                root,
                collapsed: false,
                head,
                neck_delta: neck - head,
                eye_arm: rotation.inverse() * (root.transform_point3(eyes_model) - neck),
                animation_rotation: Quat::IDENTITY,
                camera_stable: false,
                camera_head: head,
                camera_neck_delta: neck - head,
                camera_eye_arm: rotation.inverse() * (root.transform_point3(eyes_model) - neck),
                camera_weight: 0.0,
                floor: None,
            },
        }
    }

    pub(crate) fn valid(self) -> bool {
        self.root.is_finite()
            && self.root.determinant().abs() > f32::EPSILON
            && self.head_model.is_finite()
            && self.neck_model.is_finite()
            && self.eyes_model.is_finite()
            && self.head_orientation.is_finite()
            && (self.head_orientation.length_squared() - 1.0).abs() < 0.01
            && self.neck_orientation.is_finite()
            && (self.neck_orientation.length_squared() - 1.0).abs() < 0.01
            && self.geometry.valid()
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Pair {
    pub collapsed: bool,
    pub previous: Sample,
    pub current: Sample,
}

impl Pair {
    pub(crate) fn geometry(self) -> [Geometry; 2] {
        [self.previous.geometry, self.current.geometry].map(|mut geometry| {
            geometry.collapsed = self.collapsed;
            geometry
        })
    }
}

pub(crate) struct Capture {
    pub identity: Identity,
    pub tick: u64,
    pub serial: Option<u64>,
    pub context: Context,
    pub previous_root: Mat4,
    pub current_root: Mat4,
}

#[derive(Default)]
pub(crate) struct History {
    entry: Option<Entry>,
    consumed: u64,
}

impl History {
    pub(crate) const fn new() -> Self {
        Self {
            entry: None,
            consumed: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entry = None;
    }

    /// A synchronous publication is consumed at most once, independent of dispatch cadence.
    pub(crate) fn capture(
        &mut self,
        request: Capture,
        read: impl FnOnce() -> Option<Sample>,
    ) -> Option<Pair> {
        let Capture {
            identity,
            tick,
            serial,
            context,
            previous_root,
            current_root,
        } = request;
        let Some(serial) = serial else {
            self.clear();
            return None;
        };
        if serial <= self.consumed {
            let entry = self.entry.as_ref()?;
            if entry.serial != serial
                || entry.identity != identity
                || !entry.context.coherent(context)
                || entry.pair.current.root != current_root
                || entry.previous_root != previous_root
            {
                self.clear();
                return None;
            }
            return Some(entry.pair);
        }
        self.consumed = serial;
        let Some(current) = read().filter(|sample| sample.valid() && sample.root == current_root)
        else {
            self.clear();
            return None;
        };
        let retained = self.entry.as_ref().filter(|entry| {
            entry.identity == identity
                && entry.tick.checked_add(1) == Some(tick)
                && entry.context.continuous(context)
                && entry.pair.current.root == previous_root
                && entry
                    .pair
                    .current
                    .root
                    .w_axis
                    .truncate()
                    .distance(current.root.w_axis.truncate())
                    < 10.0
        });
        let pair = Pair {
            collapsed: retained.is_none(),
            previous: retained.map_or(current, |entry| entry.pair.current),
            current,
        };
        self.entry = Some(Entry {
            identity,
            tick,
            serial,
            context,
            previous_root,
            pair,
        });
        Some(pair)
    }
}

pub(crate) static HISTORY: Mutex<History> = Mutex::new(History::new());

struct Entry {
    identity: Identity,
    tick: u64,
    serial: u64,
    previous_root: Mat4,
    context: Context,
    pair: Pair,
}
