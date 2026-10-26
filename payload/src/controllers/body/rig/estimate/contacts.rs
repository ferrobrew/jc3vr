//! Virtual contacts on a named finite support patch, with support-local swing geometry.

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::controllers::body::rig::{
    estimate::{EstimateError, Stance, anatomy::Segments},
    legs::swing::{FootPose, arc},
    observation::StepStamp,
    torso::{wrap, yaw_forward},
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Support {
    pub identity: u64,
    pub generation: u64,
    pub previous_step: StepStamp,
    pub current_step: StepStamp,
    pub previous: Mat4,
    pub current: Mat4,
    pub half_extent: Vec2,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ReleaseReason {
    NoSupport,
    LostSupport,
    Unreachable,
    InvalidTouchdown,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum ContactState {
    Released(ReleaseReason),
    Planted {
        support: (u64, u64),
        local: FootPose,
    },
    Swinging {
        support: (u64, u64),
        progress: f32,
        endpoint: FootPose,
    },
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Contact {
    pub state: ContactState,
    /// The requested target until final realization, then the realized published foot.
    pub pose: FootPose,
    from_local: FootPose,
    target_local: FootPose,
}

impl Support {
    pub(crate) fn validate(
        self,
        previous: StepStamp,
        current: StepStamp,
    ) -> Result<(), EstimateError> {
        if self.previous_step != previous
            || self.current_step != current
            || !rigid_upright(self.previous)
            || !rigid_upright(self.current)
            || !self.half_extent.is_finite()
            || self.half_extent.min_element() <= 0.0
        {
            return Err(EstimateError::InvalidSupport);
        }
        Ok(())
    }

    pub(crate) fn key(self) -> (u64, u64) {
        (self.identity, self.generation)
    }

    pub(crate) fn contains(self, local: FootPose) -> bool {
        local.position.x.abs() <= self.half_extent.x && local.position.z.abs() <= self.half_extent.y
    }

    #[cfg(test)]
    pub(crate) fn foothold(self, world: Vec3, heading: f32, ankle_height: f32) -> Option<FootPose> {
        let mut local = self.local(FootPose {
            position: world,
            rotation: Quat::from_rotation_y(heading),
        });
        local.position.y = ankle_height;
        self.contains(local).then_some(local)
    }

    pub(crate) fn local(self, world: FootPose) -> FootPose {
        let rotation = self.current.to_scale_rotation_translation().1;
        FootPose {
            position: self.current.inverse().transform_point3(world.position),
            rotation: (rotation.inverse() * world.rotation).normalize(),
        }
    }

    pub(crate) fn world(self, local: FootPose) -> FootPose {
        let rotation = self.current.to_scale_rotation_translation().1;
        FootPose {
            position: self.current.transform_point3(local.position),
            rotation: (rotation * local.rotation).normalize(),
        }
    }

    pub(crate) fn carry_point(self, previous_world: Vec3) -> Vec3 {
        self.current
            .transform_point3(self.previous.inverse().transform_point3(previous_world))
    }
}

impl Contact {
    pub(crate) fn released(pose: FootPose, reason: ReleaseReason) -> Self {
        Self {
            state: ContactState::Released(reason),
            pose,
            from_local: pose,
            target_local: pose,
        }
    }

    #[cfg(test)]
    pub(crate) fn seed(pose: FootPose, support: Option<Support>, ankle_height: f32) -> Self {
        let Some(surface) = support else {
            return Self::released(pose, ReleaseReason::NoSupport);
        };
        let Some(local) = surface.foothold(pose.position, yaw(pose.rotation), ankle_height) else {
            return Self::released(pose, ReleaseReason::NoSupport);
        };
        Self::plant(surface, local)
    }

    pub(crate) fn plant(surface: Support, local: FootPose) -> Self {
        Self {
            state: ContactState::Planted {
                support: surface.key(),
                local,
            },
            pose: surface.world(local),
            from_local: local,
            target_local: local,
        }
    }

    pub(crate) fn lose_support(&mut self) {
        self.state = ContactState::Released(ReleaseReason::LostSupport);
    }

    /// Full contact identity includes private swing data and float component bits, not padding.
    pub(crate) fn same_bits(self, other: Self) -> bool {
        let same_state = match (self.state, other.state) {
            (ContactState::Released(left), ContactState::Released(right)) => left == right,
            (
                ContactState::Planted {
                    support: left,
                    local,
                },
                ContactState::Planted {
                    support: right,
                    local: other,
                },
            ) => left == right && same_pose_bits(local, other),
            (
                ContactState::Swinging {
                    support: left,
                    progress,
                    endpoint,
                },
                ContactState::Swinging {
                    support: right,
                    progress: other_progress,
                    endpoint: other,
                },
            ) => {
                left == right
                    && progress.to_bits() == other_progress.to_bits()
                    && same_pose_bits(endpoint, other)
            }
            _ => false,
        };
        same_state
            && same_pose_bits(self.pose, other.pose)
            && same_pose_bits(self.from_local, other.from_local)
            && same_pose_bits(self.target_local, other.target_local)
    }

    pub(crate) fn planted(self) -> bool {
        matches!(self.state, ContactState::Planted { .. })
    }
    pub(crate) fn active(self) -> bool {
        !matches!(self.state, ContactState::Released(_))
    }

    /// Advance the clock, but leave touchdown to the checked transition before final fitting.
    pub(crate) fn carry(&mut self, support: Option<Support>, dt: f32) {
        let key = match self.state {
            ContactState::Released(_) => return,
            ContactState::Planted { support, .. } | ContactState::Swinging { support, .. } => {
                support
            }
        };
        let Some(surface) = support.filter(|s| s.key() == key) else {
            self.state = ContactState::Released(ReleaseReason::LostSupport);
            return;
        };
        match self.state {
            ContactState::Planted { local, .. } => {
                if !surface.contains(local) {
                    self.state = ContactState::Released(ReleaseReason::LostSupport);
                } else {
                    self.pose = surface.world(local);
                }
            }
            ContactState::Swinging { progress, .. } => {
                self.set_swing(surface, (progress + dt / SWING_SECONDS).min(1.0));
            }
            ContactState::Released(_) => {}
        }
    }

    pub(crate) fn release(&mut self) {
        self.state = ContactState::Released(ReleaseReason::Unreachable);
    }

    pub(crate) fn lift(&mut self, support: Support, endpoint: FootPose) {
        self.from_local = support.local(self.pose);
        self.target_local = endpoint;
        self.set_swing(support, 0.0);
    }

    /// Bound translation and yaw in the support frame; late swing freezes the accepted endpoint.
    pub(crate) fn retarget(&mut self, support: Support, desired: FootPose, dt: f32) {
        let ContactState::Swinging { progress, .. } = self.state else {
            return;
        };
        if progress < ENDPOINT_COMMIT_PROGRESS {
            self.target_local.position += (desired.position - self.target_local.position)
                .clamp_length_max(ENDPOINT_SPEED * dt);
            let turn = wrap(yaw(desired.rotation) - yaw(self.target_local.rotation))
                .clamp(-ENDPOINT_TURN_SPEED * dt, ENDPOINT_TURN_SPEED * dt);
            self.target_local.rotation =
                (Quat::from_rotation_y(turn) * self.target_local.rotation).normalize();
        }
        self.set_swing(support, progress);
    }

    pub(crate) fn endpoint(self) -> FootPose {
        self.target_local
    }

    pub(crate) fn surface_endpoint(&mut self, support: Support, endpoint: FootPose) {
        if let ContactState::Swinging { progress, .. } = self.state {
            self.target_local = endpoint;
            self.set_swing(support, progress);
        }
    }

    pub(crate) fn touchdown(&mut self, support: Support, valid: bool) {
        if let ContactState::Swinging { progress, .. } = self.state
            && progress >= 1.0
        {
            if valid && support.contains(self.target_local) {
                self.state = ContactState::Planted {
                    support: support.key(),
                    local: self.target_local,
                };
                self.pose = support.world(self.target_local);
            } else {
                self.state = ContactState::Released(ReleaseReason::InvalidTouchdown);
            }
        }
    }

    fn set_swing(&mut self, support: Support, progress: f32) {
        // Both endpoints share the surface frame, including during support rotation.
        self.pose = support.world(arc(
            &self.from_local,
            &self.target_local,
            progress,
            SWING_HEIGHT,
        ));
        self.state = ContactState::Swinging {
            support: support.key(),
            progress,
            endpoint: support.world(self.target_local),
        };
    }
}

pub(crate) fn rest_feet(stance: Stance, segments: Segments, ankle_height: f32) -> [FootPose; 2] {
    let right = Quat::from_rotation_y(stance.yaw) * Vec3::X;
    [-1.0, 1.0].map(|sign| FootPose {
        position: stance.origin
            + right * (sign * segments.hip_width * 0.7)
            + Vec3::Y * ankle_height,
        rotation: Quat::from_rotation_y(stance.yaw),
    })
}

pub(crate) fn rigid_upright(transform: Mat4) -> bool {
    let x = transform.x_axis.truncate();
    let y = transform.y_axis.truncate();
    let z = transform.z_axis.truncate();
    transform.is_finite()
        && transform.w_axis.w == 1.0
        && transform.x_axis.w == 0.0
        && transform.y_axis.w == 0.0
        && transform.z_axis.w == 0.0
        && (x.length_squared() - 1.0).abs() < 1.0e-4
        && y.abs_diff_eq(Vec3::Y, 1.0e-4)
        && (z.length_squared() - 1.0).abs() < 1.0e-4
        && x.dot(z).abs() < 1.0e-4
        && x.cross(y).dot(z) > 0.999
}

pub(crate) fn yaw(rotation: Quat) -> f32 {
    crate::controllers::body::rig::torso::yaw_of(rotation * yaw_forward(0.0)).unwrap_or(0.0)
}

fn same_pose_bits(left: FootPose, right: FootPose) -> bool {
    left.position.to_array().map(f32::to_bits) == right.position.to_array().map(f32::to_bits)
        && left.rotation.to_array().map(f32::to_bits) == right.rotation.to_array().map(f32::to_bits)
}

pub(crate) const SWING_SECONDS: f32 = 0.32;
pub(crate) const ENDPOINT_COMMIT_PROGRESS: f32 = 0.8;
pub(crate) const ENDPOINT_SPEED: f32 = 0.8;
const ENDPOINT_TURN_SPEED: f32 = 2.0;
const SWING_HEIGHT: f32 = 0.06;
