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

/// A producer-local identity, not an engine frame number or a host-time join.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct SampleId {
    pub owner: usize,
    pub epoch: u64,
    pub sequence: u64,
}

/// One bounded input-tick observation. Poses are in the character's model space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sample {
    pub id: SampleId,
    pub at: Instant,
    /// The interaction sample's world transform, without render interpolation.
    pub character: Mat4,
    pub attachments: Vec<Attachment>,
}

/// One render publication and the input observation it compares against.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderSample {
    pub id: SampleId,
    pub input: Sample,
    pub character: Mat4,
    pub models: [Option<RenderedModel>; 3],
}

/// A value copied immediately after applying this matrix to the owned instance.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RenderedModel {
    pub model: usize,
    pub matrix: Mat4,
}

impl RenderSample {
    pub(crate) fn new(id: SampleId, input: &Sample, character: Mat4, now: Instant) -> Option<Self> {
        (id.owner == input.id.owner
            && id.epoch == input.id.epoch
            && input.current(id.owner, now)
            && usable(character))
        .then(|| Self {
            id,
            input: input.clone(),
            character,
            models: [None; 3],
        })
    }

    pub(crate) fn applied(&mut self, prop: Prop, model: usize, matrix: Mat4) {
        self.models[prop.index()] = Some(RenderedModel { model, matrix });
    }

    pub(crate) fn displayed(&self, attachment: &Attachment) -> Option<Mat4> {
        let model = self.models[attachment.prop.index()]?;
        (attachment.visible
            && attachment.model == Some(model.model)
            && attachment.local.is_ok()
            && usable(model.matrix))
        .then_some(model.matrix)
    }
}

impl Sample {
    pub(crate) fn current(&self, owner: usize, now: Instant) -> bool {
        self.id.owner == owner
            && usable(self.character)
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

fn usable(matrix: Mat4) -> bool {
    matrix.is_finite() && matrix.determinant().abs() > 1.0e-6
}

fn valid_rotation(rotation: Quat) -> bool {
    rotation.is_finite() && rotation.length_squared() > f32::EPSILON
}

#[cfg(test)]
mod tests;
