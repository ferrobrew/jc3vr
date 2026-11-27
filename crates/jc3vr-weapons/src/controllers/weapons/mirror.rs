//! The weapon handling's part of the body mirror ([`jc3vr_body::controllers::body::mirror`]): the
//! sockets' copied interaction targets drawn on each mirror pass through the registered sampler
//! ([`sample_attachments`]), and the pouches' render placement compared with their input targets
//! after each render publication ([`pouches_rendered`]).

use glam::Vec3;
use jc3vr_body::controllers::{
    body::{
        mirror,
        mirror::{
            ATTACHMENT_AXIS_M, ATTACHMENT_RING_M, AttachmentPass, BONE_WIDTH, EMPTY, FRAME, NO_FIT,
            OCCUPIED, Plane, SampleId, TARGET, TARGET_RING_M, line, ring,
        },
    },
    overlay::config::{OVERLAY_CONFIG, body_mirror_distance_m},
};
use jc3vr_view::controllers::overlay::primitive::{Anchor, Primitive};

use crate::controllers::weapons::{
    docks,
    docks::{
        SocketUse,
        attachments::{Occupancy, PoseSource, Sample},
    },
    pouch_models::attachments::RenderSample as PouchRenderSample,
};

/// The body mirror's attachment sampler
/// ([`jc3vr_body::contributions::BodyContributions::attachment_sampler`]): the sockets' copied
/// interaction targets for this pass, reflected.
pub(crate) fn sample_attachments(pass: &AttachmentPass, out: &mut Vec<Primitive>) {
    if let Some(sample) = docks::attachment_sample(pass.owner, pass.character, pass.now) {
        attachment_markers(&sample, pass.plane, pass.head, out);
    }
}

/// Publish only copied values after the model producer applies this render's matrices.
pub(crate) fn pouches_rendered(sample: &PouchRenderSample) {
    mirror::clear_pouch_render();
    if !OVERLAY_CONFIG.query(|c| c.body_mirror) {
        return;
    }
    let Some(context) = PouchRenderContext::new(sample, body_mirror_distance_m()) else {
        return;
    };
    let mut primitives = Vec::new();
    pouch_render_markers(sample, context, &mut primitives);
    mirror::submit_pouch_render(&mut primitives);
}

/// A plane derived from the same render publication as the applied model matrices.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PouchRenderContext {
    pub id: SampleId,
    pub plane: Plane,
}

impl PouchRenderContext {
    pub(crate) fn new(sample: &PouchRenderSample, distance: f32) -> Option<Self> {
        Some(Self {
            id: sample.id,
            plane: Plane::ahead_of(&sample.character, distance)?,
        })
    }
}

/// Draw copied interaction targets; landmark rings do not represent native weapon placement.
pub(crate) fn attachment_markers(
    sample: &Sample,
    plane: Plane,
    head: Vec3,
    out: &mut Vec<Primitive>,
) {
    for attachment in sample.markers() {
        let Ok(pose) = attachment.pose else {
            continue;
        };
        let color = if attachment.usage == Some(SocketUse::NoFit) {
            NO_FIT
        } else if attachment.hand.is_none()
            && matches!(attachment.source, PoseSource::OffsetBone { object: None })
        {
            FRAME
        } else {
            match attachment.occupancy {
                Occupancy::Empty => EMPTY,
                Occupancy::Occupied { .. } => OCCUPIED,
            }
        };
        let at = |point| Anchor::Head(plane.reflect(point) - head);
        out.push(ring(at(pose.position), ATTACHMENT_RING_M, color));
        out.push(line(
            at(pose.position),
            at(pose.position + pose.orientation * Vec3::NEG_Z * ATTACHMENT_AXIS_M),
            BONE_WIDTH,
            color,
        ));
    }
}

/// Compare the applied render matrix with the copied input target, without native reads.
/// World anchors avoid joining the render plane to a head pose from another phase.
pub(crate) fn pouch_render_markers(
    sample: &PouchRenderSample,
    context: PouchRenderContext,
    out: &mut Vec<Primitive>,
) {
    if context.id != sample.id {
        return;
    }
    let at = |point| Anchor::World(context.plane.reflect(point));
    for attachment in &sample.input.attachments {
        let displayed = sample.displayed(attachment);
        let target = attachment
            .pouch
            .ok()
            .map(|pose| sample.input.character.transform_point3(pose.position))
            .filter(|point| point.is_finite());
        if let Some(target) = target {
            let color = if displayed.is_some() { EMPTY } else { NO_FIT };
            out.push(ring(at(target), ATTACHMENT_RING_M, color));
        }
        if let Some(matrix) = displayed {
            let origin = matrix.w_axis.truncate();
            out.push(ring(at(origin), TARGET_RING_M, TARGET));
            out.push(line(
                at(origin),
                at(matrix.transform_point3(Vec3::NEG_Z * ATTACHMENT_AXIS_M)),
                BONE_WIDTH,
                TARGET,
            ));
            if let Some(target) = target {
                out.push(line(at(target), at(origin), BONE_WIDTH, EMPTY));
            }
        }
    }
}
