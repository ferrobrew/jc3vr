//! Owned pre-override geometry, advanced only by synchronous native pose publications.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};
use parking_lot::Mutex;

use crate::{
    controllers::body::{
        procedural::skeleton::SkeletonKey,
        rig::{live_observation, origin_provenance::OriginEvidence},
    },
    headpose::{Anchors, Source},
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
}

impl Geometry {
    /// A collapsed pair shares the current follow and view frame as well as its geometry.
    pub(crate) fn previous_endpoint<T>(self, previous: T, current: T) -> T {
        if self.collapsed { current } else { previous }
    }

    pub(crate) fn from_anchors(anchors: Anchors, root: Mat4) -> Self {
        Self {
            root,
            collapsed: false,
            head: anchors.head,
            neck_delta: anchors.neck - anchors.head,
            eye_arm: anchors.eye_arm,
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
        let tracking = live_observation::status()
            .raw
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
            gameplay: crate::hooks::in_gameplay(),
            teleport: crate::controllers::roomscale::teleport::epoch(),
            recenter: crate::vr::recenter_count(),
            world_scale: crate::config::Config::lock_query(|c| c.vr.world_scale),
            tracking,
        }
    }

    fn coherent(self, next: Self) -> bool {
        self.continuous(next)
    }

    fn continuous(self, next: Self) -> bool {
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
            && self.neck_orientation.is_finite()
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
