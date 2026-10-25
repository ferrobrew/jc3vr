//! Copied pouch and prop placements shared by model instances and the body mirror.

use std::time::Instant;

use glam::{Mat4, Quat};

use crate::controllers::{
    body::rig::frames::Frames,
    frame::{Hand, Pose},
    weapons::{
        config::ModelPlacement,
        docks::attachments::MAX_AGE,
        pouch_models::{Prop, ShownPouch, on_pouch},
    },
};

/// A pouch's missing value is distinct from one whose current source model is unavailable.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Missing {
    NotOffered,
    Model,
    Transform,
}

/// One current pouch and the exact local transform used to place its model instance.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Attachment {
    pub prop: Prop,
    pub side: Option<Hand>,
    pub weapon: Option<usize>,
    /// The borrowed model's identity, never dereferenced by a consumer.
    pub model: Option<usize>,
    pub pouch: Result<Pose, Missing>,
    pub local: Result<Mat4, Missing>,
    pub visible: bool,
}

/// One bounded input-tick observation. Poses are in the character's model space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sample {
    pub owner: usize,
    pub at: Instant,
    pub attachments: Vec<Attachment>,
}

impl Sample {
    pub(crate) fn current(&self, owner: usize, now: Instant) -> bool {
        self.owner == owner
            && now
                .checked_duration_since(self.at)
                .is_some_and(|age| age <= MAX_AGE)
    }
}

/// The production placement rule, used before showing an instance and copied for the mirror.
pub(crate) fn placement(
    prop: Prop,
    wanted: Option<&ShownPouch>,
    model: Option<usize>,
    frames: &Frames,
    config: &ModelPlacement,
) -> Attachment {
    let Some(wanted) = wanted else {
        return Attachment {
            prop,
            side: None,
            weapon: None,
            model: None,
            pouch: Err(Missing::NotOffered),
            local: Err(Missing::NotOffered),
            visible: false,
        };
    };
    let pose = Pose {
        position: frames.cockpit_to_model(wanted.pouch.position),
        orientation: frames.cockpit_rotation_to_model(wanted.pouch.orientation),
    };
    let pouch =
        if frames.is_usable() && pose.position.is_finite() && valid_rotation(pose.orientation) {
            Ok(pose)
        } else {
            Err(Missing::Transform)
        };
    let local = pouch.and_then(|pose| {
        let local = Mat4::from_rotation_translation(pose.orientation, pose.position)
            * on_pouch(config, wanted.side);
        if !local.is_finite() {
            Err(Missing::Transform)
        } else if model.is_none() {
            Err(Missing::Model)
        } else {
            Ok(local)
        }
    });
    Attachment {
        prop,
        side: Some(wanted.side),
        weapon: wanted.weapon,
        model,
        pouch,
        local,
        visible: false,
    }
}

fn valid_rotation(rotation: Quat) -> bool {
    rotation.is_finite() && rotation.length_squared() > f32::EPSILON
}

#[cfg(test)]
mod tests;
