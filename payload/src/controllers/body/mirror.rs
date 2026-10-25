//! The body mirror: a diagnostic that draws Rico's solved skeleton reflected in a mirror standing in
//! front of him, through the world-space overlay, for checking in the headset that his body lines
//! up with the player's. See `docs/mod/body/procedural-states.md`, "The body mirror".
//!
//! At the end of the local player's pose-finalization pass (the post-call `UpdatePropEffects`
//! detour, after the head override and the limbs' twist), [`after_solve`] reads the bones of a
//! stick figure from the finalized pose, carries them into the world, and reflects them across a
//! plane `body_mirror_distance_m` ahead of Rico's facing. The reflection is drawn as lines between
//! the bones, with the mirror's frame, a ring at the reflected head bone, and smaller rings at the
//! reflected targets the body was posed toward: the player's head, and each controller's wrist
//! target. A wrist ring away from the reflected hand, or the head ring away from the head, is a
//! misalignment. The primitives are submitted from the pass's thread to the overlay's retained
//! slot, anchored to the head at an offset, so the figure rides with a player moving fast (a
//! vehicle, the wingsuit) instead of trailing at the tick's rate.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use jc3gi::character::character::{AnimationController, Character, Joint};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::procedural::{
            joints::from_joint,
            skeleton::{self, SkeletonKey},
        },
        hands::ik::tick_records,
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        weapons::{
            docks::{
                self, SocketUse,
                attachments::{Occupancy, PoseSource, Sample},
            },
            pouch_models::attachments::{RenderSample as PouchRenderSample, SampleId},
        },
    },
    headpose,
};

/// What the mirror drew on its last pass.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct MirrorStatus {
    pub at: Option<Instant>,
    /// How many of the figure's bones the skeleton has; `None` before a skeleton is read.
    pub bones: Option<usize>,
    /// How many primitives the last pass submitted.
    pub primitives: usize,
}

/// Whether the mirror has current copied attachment observations on its last pass.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AttachmentStatus {
    pub at: Option<Instant>,
    pub sockets_current: bool,
    /// Socket observations without a pose, counted once per socket.
    pub sockets_missing: usize,
    pub socket_markers: usize,
    pub socket_offers: usize,
    pub occupied_sockets: usize,
    /// Availability from the separate render publication, not this solve pass.
    pub pouches_current: bool,
    /// Offered pouches whose model is missing or hidden.
    pub pouches_missing: usize,
}

pub fn attachment_status() -> AttachmentStatus {
    let mut status = *ATTACHMENT_STATUS.lock();
    let render = pouch_render_status();
    status.pouches_current = render.render.is_some();
    status.pouches_missing = render.missing;
    status
}

/// Identities for the last render-time comparison, separate from the skeleton's solve pass.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct PouchRenderStatus {
    pub render: Option<SampleId>,
    pub input: Option<SampleId>,
    pub displayed: usize,
    pub targets: usize,
    pub missing: usize,
}

pub(crate) fn pouch_render_status() -> PouchRenderStatus {
    let (at, status) = *POUCH_RENDER_STATUS.lock();
    if at
        .and_then(|at| Instant::now().checked_duration_since(at))
        .is_some_and(|age| age <= TTL)
    {
        status
    } else {
        PouchRenderStatus::default()
    }
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

/// Publish only copied values after the model producer applies this render's matrices.
pub(crate) fn pouches_rendered(sample: &PouchRenderSample) {
    clear_pouch_render();
    let (enabled, distance) = Config::lock_query(|c| {
        (
            c.controllers.overlay.body_mirror,
            c.controllers.overlay.body_mirror_distance_m,
        )
    });
    if !enabled {
        return;
    }
    let Some(context) = PouchRenderContext::new(sample, distance) else {
        return;
    };
    let mut primitives = Vec::new();
    let status = pouch_render_markers(sample, context, &mut primitives);
    store::submit(Source::BodyMirrorPouches, &mut primitives, TTL);
    *POUCH_RENDER_STATUS.lock() = (Some(Instant::now()), status);
}

/// Clear the render source without replacing the skeleton and socket submission.
pub(crate) fn clear_pouch_render() {
    store::clear(Source::BodyMirrorPouches);
    *POUCH_RENDER_STATUS.lock() = (None, PouchRenderStatus::default());
}

/// The last pass's status. A leaf lock, so any thread may read it.
pub fn status() -> MirrorStatus {
    *STATUS.lock()
}

/// Draw the local player's reflection, when the body mirror is on.
///
/// # Safety
///
/// `character` must be null or a live character, called from the post-call `UpdatePropEffects`
/// detour on the thread running its pose finalization.
pub(crate) unsafe fn after_solve(character: *mut Character) {
    let Some(character) = (unsafe { character.as_ref() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let (enabled, distance, show_attachments, follow_docks) = Config::lock_query(|c| {
        (
            c.controllers.overlay.body_mirror,
            c.controllers.overlay.body_mirror_distance_m,
            c.controllers.weapons.holsters.enabled,
            c.controllers.weapons.holsters.follow_docks,
        )
    });
    if !enabled {
        return;
    }
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() })
    else {
        return;
    };
    let world = Mat4::from(character.m_WorldMatrixT1);
    let Some(indices) = (unsafe { CACHE.lock().get(controller) }) else {
        return;
    };
    let found = indices.iter().filter(|i| i.is_some()).count();
    let bones =
        indices.map(|index| index.map(|i| unsafe { joint_in_world(controller, i, &world) }));
    let head = headpose::query().position;
    let targets = tick_records()
        .current
        .map(|t| t.map(|t| t.wrist.position).filter(|p| p.is_finite()));
    let Some(plane) = Plane::ahead_of(&world, distance) else {
        return;
    };
    let mut primitives = Vec::new();
    figure(
        &Figure {
            plane,
            bones,
            head_target: head,
            wrist_targets: targets,
            root_height: world.w_axis.y,
        },
        head,
        &mut primitives,
    );
    let now = Instant::now();
    let attachments = if show_attachments {
        docks::attachment_sample(character as *const Character as usize, world, now)
            .filter(|sample| sample.matches_policy(follow_docks))
    } else {
        None
    };
    if let Some(sample) = &attachments {
        attachment_markers(sample, plane, head, &mut primitives);
    }
    let attachments_missing = attachments.as_ref().map_or(0, |sample| {
        sample
            .attachments
            .iter()
            .filter(|attachment| {
                attachment.hand.is_none()
                    && (attachment.pose.is_err()
                        || sample
                            .attachments
                            .iter()
                            .any(|offer| offer.socket == attachment.socket && offer.pose.is_err()))
            })
            .count()
    });
    let submitted = primitives.len();
    store::submit(Source::BodyMirror, &mut primitives, TTL);
    *STATUS.lock() = MirrorStatus {
        at: Some(now),
        bones: Some(found),
        primitives: submitted,
    };
    *ATTACHMENT_STATUS.lock() = AttachmentStatus {
        at: Some(now),
        sockets_current: attachments.is_some(),
        sockets_missing: attachments_missing,
        socket_markers: attachments
            .as_ref()
            .map_or(0, |sample| sample.markers().count()),
        socket_offers: attachments.as_ref().map_or(0, |sample| {
            sample
                .attachments
                .iter()
                .filter(|attachment| attachment.hand.is_some())
                .count()
        }),
        occupied_sockets: attachments.as_ref().map_or(0, |sample| {
            sample
                .attachments
                .iter()
                .filter(|attachment| attachment.hand.is_none() && attachment.item.is_some())
                .count()
        }),
        pouches_current: false,
        pouches_missing: 0,
    };
}

/// A mirror plane: a point on it and its normal, which faces the body.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Plane {
    pub point: Vec3,
    pub normal: Vec3,
}

impl Plane {
    /// The mirror `distance` metres ahead of the root at `world`, upright, facing back at it. The
    /// facing is the root's forward laid flat, or its up laid flat where the forward is near
    /// vertical (a body lying prone in the wingsuit, whose root may pitch with it).
    pub fn ahead_of(world: &Mat4, distance: f32) -> Option<Plane> {
        let flat = |v: Vec3| {
            let v = v.with_y(0.0);
            (v.is_finite() && v.length() > MIN_FLAT).then(|| v.normalize())
        };
        let forward = flat(world.transform_vector3(Vec3::NEG_Z))
            .or_else(|| flat(world.transform_vector3(Vec3::Y)))?;
        let root = world.w_axis.truncate();
        (root.is_finite() && distance.is_finite() && distance > 0.0).then(|| Plane {
            point: root + forward * distance,
            normal: -forward,
        })
    }

    /// `point` reflected across the plane.
    pub fn reflect(&self, point: Vec3) -> Vec3 {
        point - 2.0 * (point - self.point).dot(self.normal) * self.normal
    }
}

/// The figure's bones, by [`FIGURE_BONES`]' order.
pub const FIGURE_BONE_COUNT: usize = FIGURE_BONES.len();

/// What the reflection is drawn from, in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub plane: Plane,
    /// Each of [`FIGURE_BONES`] in the world, `None` for a bone the skeleton lacks.
    pub bones: [Option<Vec3>; FIGURE_BONE_COUNT],
    /// Where the body's head was posed toward: the player's head.
    pub head_target: Vec3,
    /// Each controller's wrist target, left then right.
    pub wrist_targets: [Option<Vec3>; 2],
    /// The root's height, where the mirror's frame stands.
    pub root_height: f32,
}

/// The reflection's primitives, anchored to the head at `head` (the world position the offsets
/// are taken from).
pub fn figure(figure: &Figure, head: Vec3, out: &mut Vec<Primitive>) {
    let plane = &figure.plane;
    let at = |p: Vec3| Anchor::Head(plane.reflect(p) - head);
    for (a, b) in LINKS {
        if let (Some(from), Some(to)) = (figure.bones[a], figure.bones[b]) {
            let color = match side_of(FIGURE_BONES[b]) {
                Some(BoneSide::Left) => LEFT,
                Some(BoneSide::Right) => RIGHT,
                None => SPINE,
            };
            out.push(line(at(from), at(to), BONE_WIDTH, color));
        }
    }
    if let Some(head_bone) = figure.bones[HEAD] {
        out.push(ring(at(head_bone), HEAD_RING_M, SPINE));
    }
    out.push(ring(at(figure.head_target), TARGET_RING_M, TARGET));
    for (target, color) in figure.wrist_targets.iter().zip([LEFT, RIGHT]) {
        if let Some(target) = target {
            out.push(ring(at(*target), TARGET_RING_M, color));
        }
    }
    // The frame: a doorway in the plane, standing on the root's height.
    let across = Vec3::Y.cross(plane.normal).normalize_or_zero() * (FRAME_WIDTH_M * 0.5);
    let base = plane.point.with_y(figure.root_height);
    let corners = [
        base - across,
        base + across,
        base + across + Vec3::Y * FRAME_HEIGHT_M,
        base - across + Vec3::Y * FRAME_HEIGHT_M,
    ];
    for i in 0..4 {
        out.push(line(
            Anchor::Head(corners[i] - head),
            Anchor::Head(corners[(i + 1) % 4] - head),
            FRAME_WIDTH,
            FRAME,
        ));
    }
}

/// How long a submission is drawn without another: a few ticks, so a paused or skipped pass
/// clears the mirror.
const TTL: Duration = Duration::from_millis(150);

/// How long a direction laid flat must be to have a heading of its own.
const MIN_FLAT: f32 = 0.2;

/// The figure's bones, by name.
const FIGURE_BONES: [&str; 21] = [
    "Hips",
    "Spine",
    "Spine1",
    "Spine2",
    "Sternum",
    "Neck",
    "Head",
    "LeftShoulder",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
    "RightShoulder",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
];

const HEAD: usize = 6;

/// The figure's lines, as pairs of [`FIGURE_BONES`] indices.
const LINKS: [(usize, usize); 20] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (4, 5),
    (5, 6),
    (4, 7),
    (7, 8),
    (8, 9),
    (9, 10),
    (4, 11),
    (11, 12),
    (12, 13),
    (13, 14),
    (0, 15),
    (15, 16),
    (16, 17),
    (0, 18),
    (18, 19),
    (19, 20),
];

enum BoneSide {
    Left,
    Right,
}

fn side_of(name: &str) -> Option<BoneSide> {
    if name.starts_with("Left") {
        Some(BoneSide::Left)
    } else if name.starts_with("Right") {
        Some(BoneSide::Right)
    } else {
        None
    }
}

const BONE_WIDTH: f32 = 0.025;
const FRAME_WIDTH: f32 = 0.01;
const HEAD_RING_M: f32 = 0.22;
const TARGET_RING_M: f32 = 0.08;
const FRAME_WIDTH_M: f32 = 1.2;
const FRAME_HEIGHT_M: f32 = 2.2;

const SPINE: Rgba = [0.95, 0.95, 0.95, 0.9];
const LEFT: Rgba = [0.3, 0.6, 1.0, 0.9];
const RIGHT: Rgba = [1.0, 0.55, 0.25, 0.9];
const TARGET: Rgba = [0.3, 1.0, 0.4, 0.9];
const FRAME: Rgba = [0.7, 0.7, 0.8, 0.5];
const OCCUPIED: Rgba = [0.9, 0.7, 0.2, 0.9];
const EMPTY: Rgba = [0.4, 0.8, 0.8, 0.7];
const NO_FIT: Rgba = [1.0, 0.2, 0.2, 0.9];
const ATTACHMENT_RING_M: f32 = 0.10;
const ATTACHMENT_AXIS_M: f32 = 0.08;

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
        let radius = if attachment.source == PoseSource::BodyLandmark {
            ATTACHMENT_RING_M * 0.7
        } else {
            ATTACHMENT_RING_M
        };
        out.push(ring(at(pose.position), radius, color));
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
) -> PouchRenderStatus {
    if context.id != sample.id {
        return PouchRenderStatus::default();
    }
    let mut status = PouchRenderStatus {
        render: Some(sample.id),
        input: Some(sample.input.id),
        ..PouchRenderStatus::default()
    };
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
            status.targets += 1;
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
            status.displayed += 1;
        } else if attachment.side.is_some() {
            status.missing += 1;
        }
    }
    status
}

fn line(from: Anchor, to: Anchor, width: f32, color: Rgba) -> Primitive {
    Primitive::Line {
        from,
        to,
        width,
        color,
        depth: DepthMode::Always,
    }
}

fn ring(at: Anchor, size: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at,
        size: SpriteSize::World(size),
        facing: Facing::Billboard,
        shape: Shape::Ring { thickness: 0.25 },
        color,
        depth: DepthMode::Always,
    }
}

/// `bone`'s model-space position carried into the world by `world`.
///
/// # Safety
///
/// `controller` must be live, and `bone` below its skeleton's bone count.
unsafe fn joint_in_world(controller: &AnimationController, bone: usize, world: &Mat4) -> Vec3 {
    let mut joint = Joint::default();
    unsafe { controller.GetJoint(bone as u32, &mut joint) };
    world.transform_point3(from_joint(&joint).translation)
}

/// The figure's bone indices for the skeleton a controller holds, found by name once per skeleton.
struct FigureCache {
    entry: Option<(SkeletonKey, Option<[Option<usize>; FIGURE_BONE_COUNT]>)>,
}

impl FigureCache {
    /// The indices of the figure's bones in `controller`'s skeleton, each below its bone count.
    ///
    /// # Safety
    ///
    /// `controller` must be a live controller, on the thread that owns its character.
    unsafe fn get(
        &mut self,
        controller: &AnimationController,
    ) -> Option<[Option<usize>; FIGURE_BONE_COUNT]> {
        let key = unsafe { SkeletonKey::of(controller) }?;
        if self.entry.as_ref().is_none_or(|(k, _)| *k != key) {
            let indices = match unsafe { skeleton::read(controller) } {
                Ok(snapshot) => Some(FIGURE_BONES.map(|name| {
                    snapshot
                        .bones
                        .iter()
                        .position(|b| b.name == name)
                        .filter(|&i| i < key.bones())
                })),
                Err(e) => {
                    tracing::warn!(target: "body", "body mirror: {e}");
                    None
                }
            };
            if let Some(indices) = &indices {
                let missing: Vec<&str> = FIGURE_BONES
                    .iter()
                    .zip(indices)
                    .filter(|(_, i)| i.is_none())
                    .map(|(name, _)| *name)
                    .collect();
                tracing::info!(
                    target: "body",
                    found = FIGURE_BONE_COUNT - missing.len(),
                    ?missing,
                    "body mirror: found the figure's bones",
                );
            }
            self.entry = Some((key, indices));
        }
        self.entry.as_ref().and_then(|(_, indices)| *indices)
    }
}

static CACHE: Mutex<FigureCache> = Mutex::new(FigureCache { entry: None });

static STATUS: Mutex<MirrorStatus> = Mutex::new(MirrorStatus {
    at: None,
    bones: None,
    primitives: 0,
});

static POUCH_RENDER_STATUS: Mutex<(Option<Instant>, PouchRenderStatus)> = Mutex::new((
    None,
    PouchRenderStatus {
        render: None,
        input: None,
        displayed: 0,
        targets: 0,
        missing: 0,
    },
));

static ATTACHMENT_STATUS: Mutex<AttachmentStatus> = Mutex::new(AttachmentStatus {
    at: None,
    sockets_current: false,
    sockets_missing: 0,
    socket_markers: 0,
    socket_offers: 0,
    occupied_sockets: 0,
    pouches_current: false,
    pouches_missing: 0,
});

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn the_mirror_stands_ahead_of_the_root_facing_back() {
        let world = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(5.0, 2.0, 0.0),
        );
        let plane = Plane::ahead_of(&world, 1.5).unwrap();
        // Turned a quarter left, -Z becomes -X.
        assert!(plane.point.abs_diff_eq(Vec3::new(3.5, 2.0, 0.0), 1e-5));
        assert!(plane.normal.abs_diff_eq(Vec3::X, 1e-5));
        // Pitched prone, the forward points down and the up is the heading.
        let prone = Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        let plane = Plane::ahead_of(&prone, 1.0).unwrap();
        assert!(
            plane.normal.abs_diff_eq(Vec3::Z, 1e-5),
            "{:?}",
            plane.normal
        );
        assert!(Plane::ahead_of(&Mat4::IDENTITY, f32::NAN).is_none());
    }

    #[test]
    fn a_reflection_mirrors_across_the_plane_and_swaps_the_sides_as_seen() {
        let plane = Plane::ahead_of(&Mat4::IDENTITY, 1.0).unwrap();
        // A point at the root reflects two metres ahead; a point on the plane stays.
        assert!(
            plane
                .reflect(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(0.0, 0.0, -2.0), 1e-5)
        );
        let on = Vec3::new(0.3, 1.0, -1.0);
        assert!(plane.reflect(on).abs_diff_eq(on, 1e-5));
        // Left stays left in the world: as seen facing the mirror, it is on the viewer's left.
        let left = Vec3::new(-0.3, 1.0, 0.0);
        assert!((plane.reflect(left).x - left.x).abs() < 1e-5);
    }

    #[test]
    fn the_figure_draws_the_found_bones_and_the_targets() {
        let plane = Plane::ahead_of(&Mat4::IDENTITY, 1.0).unwrap();
        let mut bones = [None; FIGURE_BONE_COUNT];
        bones[0] = Some(Vec3::new(0.0, 1.0, 0.0));
        bones[1] = Some(Vec3::new(0.0, 1.1, 0.0));
        bones[HEAD] = Some(Vec3::new(0.0, 1.7, 0.0));
        let head = Vec3::new(0.0, 1.75, 0.0);
        let mut out = Vec::new();
        figure(
            &Figure {
                plane,
                bones,
                head_target: head,
                wrist_targets: [Some(Vec3::new(-0.3, 1.2, -0.3)), None],
                root_height: 0.0,
            },
            head,
            &mut out,
        );
        // One link (hips to spine), the head ring, the head target, one wrist target, the frame.
        assert_eq!(out.len(), 1 + 1 + 1 + 1 + 4);
        let Primitive::Line {
            from: Anchor::Head(from),
            ..
        } = out[0]
        else {
            panic!("{:?}", out[0]);
        };
        assert!((head + from).abs_diff_eq(Vec3::new(0.0, 1.0, -2.0), 1e-5));
    }
}
