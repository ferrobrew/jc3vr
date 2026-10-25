//! Copied weapon-socket observations shared by docking and the body mirror.
//!
//! Engine objects are observed only by the input-thread caller. These values neither keep an
//! object alive nor authorize a grab; the loadout and the existing grab rules retain that role.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    body::rig::frames::Frames,
    frame::{Hand, Pose},
    grab::GrabVolume,
    loadout::{ItemId, Socket},
    weapons::{
        config::HolstersConfig,
        docks::{SocketPoint, SocketUse},
        volumes::{self, BodyFrame},
    },
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
    VisibleGrip {
        object: usize,
    },
    RememberedGrip {
        object: usize,
    },
    NativeBone {
        object: usize,
    },
    OffsetBone {
        object: Option<usize>,
    },
    UnavailableWeapon {
        object: Option<usize>,
    },
    /// The configured interaction landmark, not the displayed native weapon pose.
    BodyLandmark,
}

/// The target policy captured by the input provider, including its native fallback.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TargetPolicy {
    Native,
    BodyLandmarks,
    NativeWithoutBody,
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
    /// The native observation retained when the target uses a body landmark.
    pub native: Option<NativePose>,
}

/// A copied native grip or socket pose, distinct from a configured interaction target.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct NativePose {
    pub source: PoseSource,
    pub pose: Result<Pose, Missing>,
}

/// One input-thread observation, with its owner, age, and source character transform.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sample {
    pub owner: usize,
    pub at: Instant,
    pub character: Mat4,
    pub policy: TargetPolicy,
    pub attachments: Vec<Attachment>,
}

impl Sample {
    /// Whether this sample was selected for the current holster target policy.
    pub(crate) fn matches_policy(&self, follow_docks: bool) -> bool {
        (self.policy == TargetPolicy::Native) == follow_docks
    }

    /// Select the existing interaction targets without changing native weapon placement or offers.
    pub(crate) fn targets(
        &self,
        frames: &Frames,
        body: Option<&BodyFrame>,
        holsters: &HolstersConfig,
    ) -> Self {
        let mut sample = self.clone();
        sample.policy = if holsters.follow_docks {
            TargetPolicy::Native
        } else if body.is_some() {
            TargetPolicy::BodyLandmarks
        } else {
            TargetPolicy::NativeWithoutBody
        };
        let Some(body) = body.filter(|_| !holsters.follow_docks) else {
            return sample;
        };
        let landmarks = volumes::socket_landmarks(body, holsters);
        for attachment in &mut sample.attachments {
            // A missing native offer must not become a new interaction at a landmark.
            if attachment.pose.is_err() {
                continue;
            }
            let Some(landmark) = landmarks
                .iter()
                .find(|candidate| candidate.volume == GrabVolume::Socket(attachment.socket))
            else {
                continue;
            };
            let basis = match attachment.socket {
                Socket::Hip(_) => body.pelvis,
                Socket::Shoulder(_) | Socket::Chest => body.chest,
                Socket::BackOuter => continue,
            };
            attachment.native = Some(NativePose {
                source: attachment.source,
                pose: attachment.pose,
            });
            attachment.source = PoseSource::BodyLandmark;
            attachment.pose = finite_pose(Pose {
                position: frames.cockpit_to_world(landmark.centre),
                orientation: (frames.view * basis).normalize(),
            });
        }
        sample
    }

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
        let rebase = |pose: Pose| {
            finite_pose(Pose {
                position: change.transform_point3(pose.position),
                orientation: (rotation * pose.orientation).normalize(),
            })
        };
        for attachment in &mut sample.attachments {
            attachment.pose = attachment.pose.and_then(rebase);
            if let Some(native) = &mut attachment.native {
                native.pose = native.pose.and_then(rebase);
            }
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
