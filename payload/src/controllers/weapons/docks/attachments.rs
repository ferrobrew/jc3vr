//! Copied weapon-socket observations shared by docking and the body mirror.
//!
//! Engine objects are observed only by the input-thread caller. These values neither keep an
//! object alive nor authorize a grab; the loadout and the existing grab rules retain that role.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    frame::{Hand, Pose},
    loadout::{ItemId, Socket},
    weapons::docks::{SocketPoint, SocketUse},
};

/// Whether the loadout has an item on the socket, independently of whether its grip was observed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Occupancy {
    Empty,
    Occupied { object: Option<usize> },
}

/// The observation that places a socket's marker. Object addresses are diagnostic identity only.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PoseSource {
    VisibleGrip { object: usize },
    RememberedGrip { object: usize },
    NativeBone { object: usize },
    OffsetBone { object: Option<usize> },
    UnavailableWeapon { object: Option<usize> },
}

/// Why an existing socket has no pose in this observation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Missing {
    Weapon,
    Grip,
    Bone,
    Transform,
}

/// A socket's pose in the world, optionally specialized for a hand's existing docking offer.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Attachment {
    pub socket: Socket,
    pub hand: Option<Hand>,
    pub usage: Option<SocketUse>,
    pub occupancy: Occupancy,
    /// The loadout's session identity of the socket occupant, when occupied.
    pub item: Option<ItemId>,
    pub source: PoseSource,
    pub pose: Result<Pose, Missing>,
}

/// One input-thread observation, with its owner, age, and source character transform.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sample {
    pub owner: usize,
    pub at: Instant,
    pub character: Mat4,
    pub attachments: Vec<Attachment>,
}

impl Sample {
    /// The actual docking points, without changing the resolver's hand or socket decisions.
    pub(crate) fn points(&self) -> Vec<SocketPoint> {
        self.attachments
            .iter()
            .filter_map(|attachment| {
                Some(SocketPoint {
                    socket: attachment.socket,
                    hand: attachment.hand?,
                    point: attachment.pose.ok()?.position,
                    usage: attachment.usage?,
                })
            })
            .collect()
    }

    /// Copy this observation onto the owner's current world transform, or reject an old or foreign
    /// observation. Bone-relative poses remain the input tick's observations, not fresh joint reads.
    pub(crate) fn in_world(&self, owner: usize, character: Mat4, now: Instant) -> Option<Self> {
        if self.owner != owner || now.checked_duration_since(self.at)? > MAX_AGE {
            return None;
        }
        if !usable_transform(self.character) || !usable_transform(character) {
            return None;
        }
        let change = character * self.character.inverse();
        let (_, rotation, _) = change.to_scale_rotation_translation();
        if !rotation.is_finite() {
            return None;
        }
        let mut sample = self.clone();
        sample.character = character;
        for attachment in &mut sample.attachments {
            attachment.pose = attachment.pose.and_then(|pose| {
                finite_pose(Pose {
                    position: change.transform_point3(pose.position),
                    orientation: (rotation * pose.orientation).normalize(),
                })
            });
        }
        Some(sample)
    }

    /// Show the actual hand-specific offers when present, otherwise the socket's observation.
    pub(crate) fn markers(&self) -> impl Iterator<Item = &Attachment> {
        self.attachments.iter().filter(|attachment| {
            attachment.hand.is_some()
                || !self
                    .attachments
                    .iter()
                    .any(|other| other.socket == attachment.socket && other.hand.is_some())
        })
    }
}

/// Inputs copied from the already-validated engine reads on the input thread.
#[derive(Debug, Copy, Clone)]
pub(crate) enum Placement {
    Visible {
        object: usize,
        grip: Option<Vec3>,
    },
    Free {
        object: Option<usize>,
        remembered: Option<Vec3>,
        native: bool,
        offset: Vec3,
    },
}

/// The production placement rule for both the docking points and the mirror's copied values.
pub(crate) fn socket_pose(
    character: Mat4,
    bone: Option<Mat4>,
    placement: Placement,
) -> (PoseSource, Result<Pose, Missing>) {
    match placement {
        Placement::Visible { object, grip } => {
            let source = PoseSource::VisibleGrip { object };
            let pose = grip.ok_or(Missing::Grip).and_then(|position| {
                let orientation = bone.map_or(Quat::IDENTITY, rotation_of);
                finite_pose(Pose {
                    position,
                    orientation,
                })
            });
            (source, pose)
        }
        Placement::Free {
            object,
            remembered,
            native,
            offset,
        } => {
            let source = match (object, remembered, native) {
                (Some(object), Some(_), _) => PoseSource::RememberedGrip { object },
                (Some(object), None, true) => PoseSource::NativeBone { object },
                _ => PoseSource::OffsetBone { object },
            };
            let pose = bone.ok_or(Missing::Bone).and_then(|bone| {
                let position = if let Some(local) = remembered {
                    bone.transform_point3(local)
                } else {
                    bone.w_axis.truncate()
                        + if native {
                            Vec3::ZERO
                        } else {
                            character.transform_vector3(offset)
                        }
                };
                finite_pose(Pose {
                    position,
                    orientation: rotation_of(bone),
                })
            });
            (source, pose)
        }
    }
}

/// The mirror must not keep showing attachment offers after input observations stop.
pub(crate) const MAX_AGE: Duration = Duration::from_millis(250);

fn usable_transform(matrix: Mat4) -> bool {
    matrix.is_finite() && matrix.determinant().abs() > 1.0e-6
}

fn rotation_of(matrix: Mat4) -> Quat {
    let (_, rotation, _) = matrix.to_scale_rotation_translation();
    if rotation.is_finite() && rotation.length_squared() > f32::EPSILON {
        rotation
    } else {
        Quat::IDENTITY
    }
}

fn finite_pose(pose: Pose) -> Result<Pose, Missing> {
    if pose.position.is_finite()
        && pose.orientation.is_finite()
        && pose.orientation.length_squared() > f32::EPSILON
    {
        Ok(pose)
    } else {
        Err(Missing::Transform)
    }
}

#[cfg(test)]
mod tests;
