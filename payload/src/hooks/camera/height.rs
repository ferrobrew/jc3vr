//! The camera height readout: where the camera puts the eyes against where Rico's animated eyes
//! are, both measured up from his root, with the head-height remap, the posture and what the
//! automatic posture decided it from, the seated eye reference and its source, and the crouch
//! state, and the driving view's correction that account for the difference. Read-only: it observes the camera placement and changes
//! nothing.
//!
//! The camera hook records a sample each gameplay frame the headpose places the camera
//! ([`record`]). Rico's eye midpoint is rebuilt from the published anchors exactly as the camera
//! hangs its eye point from them (the head anchor, the neck delta, and the eye arm in the body
//! frame), so with a zero vertical offset and no configured head or body offset the two heights
//! agree. The debug UI reads the latest sample through [`readout`], and while Rico is on foot or in
//! a vehicle seat the readout is logged at most every [`LOG_INTERVAL`].

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::{self, BodyDimensions, rig::collision::probe},
        roomscale::{self, RoomscaleStatus, SeatPhase, posture::HeadHeight},
        vehicles::driving_view,
    },
    headpose::{anchor, eye_arm, neck_delta},
};

/// One sample of the camera height and what explains it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeightReadout {
    /// Rico's animated eye midpoint above his root, world metres.
    pub rico_eyes_m: f32,
    /// The camera's eye point above Rico's root, world metres.
    pub camera_eyes_m: f32,
    /// The standing eye height the head remap assumes Rico's animated eyes are at, metres.
    pub character_eye_estimate_m: f32,
    /// The player's standing eye height from the body dimensions, metres.
    pub player_eye_height_m: f32,
    /// The player's head above the stage floor, metres, when the runtime reports a floor.
    pub floor_head_height_m: Option<f32>,
    /// The head height remap, while it is on.
    pub remap: Option<HeadHeight>,
    /// The vertical offset the cockpit composes the view with, world metres: the remap's offset
    /// scaled by the world scale.
    pub vertical_offset_m: f32,
    /// The driving view's correction the cockpit composed the view with, body frame, world
    /// metres: the correction scaled by the world scale.
    pub driving_view_m: Vec3,
    /// The roomscale state the offset comes from.
    pub roomscale: RoomscaleStatus,
    /// When the camera sample was taken.
    pub at: Instant,
}

impl HeightReadout {
    /// Whether Rico was on foot at the sample: the on-foot locomotion tick ran recently.
    pub fn on_foot(&self) -> bool {
        self.roomscale
            .root
            .sim
            .at
            .is_some_and(|at| self.at.saturating_duration_since(at) < probe::STALE_AFTER)
    }

    /// Whether Rico was in a vehicle seat at the sample.
    pub fn in_seat(&self) -> bool {
        matches!(
            self.roomscale.frame.seat,
            SeatPhase::Locked | SeatPhase::Shuffle
        )
    }

    /// Where Rico was at the sample, for the readouts.
    pub fn place(&self) -> &'static str {
        if self.on_foot() {
            "on foot"
        } else if self.in_seat() {
            "in a vehicle seat"
        } else {
            "neither on foot nor seated"
        }
    }
}

/// The latest camera height sample, or `None` until the headpose has placed the camera.
pub fn readout() -> Option<HeightReadout> {
    let sample = (*SAMPLE.lock())?;
    Some(sample.readout(roomscale::status()))
}

/// How often the readout is logged while Rico is on foot or in a vehicle seat.
pub const LOG_INTERVAL: Duration = Duration::from_secs(5);

/// Record the camera's eye point `camera` for the character world matrix `character` (the matrix
/// the camera placement composed with), and log the readout if one is due.
pub(super) fn record(character: Mat4, camera: Vec3) {
    let Some(head) = anchor() else {
        return;
    };
    let (_, rotation, root) = character.to_scale_rotation_translation();
    let rico_eyes = head + neck_delta() + rotation * eye_arm();
    let now = Instant::now();
    let sample = Sample {
        root,
        rico_eyes,
        camera,
        at: now,
    };
    if !sample.is_finite() {
        return;
    }
    *SAMPLE.lock() = Some(sample);

    let mut last_log = LAST_LOG.lock();
    if last_log.is_some_and(|at| now.saturating_duration_since(at) < LOG_INTERVAL) {
        return;
    }
    let readout = sample.readout(roomscale::status());
    if !(readout.on_foot() || readout.in_seat()) {
        return;
    }
    *last_log = Some(now);
    drop(last_log);
    log(&readout);
}

/// The positions one camera placement measured, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Sample {
    /// Rico's root, the character world matrix's translation.
    root: Vec3,
    rico_eyes: Vec3,
    camera: Vec3,
    at: Instant,
}

impl Sample {
    fn is_finite(&self) -> bool {
        self.root.is_finite() && self.rico_eyes.is_finite() && self.camera.is_finite()
    }

    fn readout(&self, roomscale: RoomscaleStatus) -> HeightReadout {
        let frame = &roomscale.frame;
        HeightReadout {
            rico_eyes_m: self.rico_eyes.y - self.root.y,
            camera_eyes_m: self.camera.y - self.root.y,
            character_eye_estimate_m: BodyDimensions::CHARACTER_ESTIMATE.eye_height,
            player_eye_height_m: body::player().eye_height,
            floor_head_height_m: frame.stage.head_height,
            remap: frame.head_height,
            vertical_offset_m: frame.cockpit_y * frame.world_scale,
            driving_view_m: driving_view::status().offset * frame.world_scale,
            roomscale,
            at: self.at,
        }
    }
}

static SAMPLE: Mutex<Option<Sample>> = Mutex::new(None);

static LAST_LOG: Mutex<Option<Instant>> = Mutex::new(None);

fn log(r: &HeightReadout) {
    let frame = &r.roomscale.frame;
    let classifier = frame.classifier;
    let thresholds = classifier.map(|c| c.thresholds);
    let reference = frame.seated_reference.reference;
    tracing::info!(
        target: "vr",
        place = r.place(),
        seat = frame.seat.label(),
        rico_eyes_m = r.rico_eyes_m,
        camera_eyes_m = r.camera_eyes_m,
        camera_minus_rico_m = r.camera_eyes_m - r.rico_eyes_m,
        vertical_offset_m = r.vertical_offset_m,
        driving_view_m = ?r.driving_view_m,
        remap = r.remap.is_some(),
        remap_source = r.remap.map(|h| h.source.label()),
        floor_head_height_m = ?r.floor_head_height_m,
        player_eye_height_m = r.player_eye_height_m,
        character_eye_estimate_m = r.character_eye_estimate_m,
        world_scale = frame.world_scale,
        posture_setting = ?frame.setting,
        posture = frame.posture.label(),
        auto_detected = ?classifier.map(|c| c.detected),
        auto_reason = classifier.map(|c| c.reason.label()),
        auto_dwell_s = classifier.map(|c| c.dwell_s),
        auto_seated_eye_m = thresholds.map(|t| t.seated_eye_m),
        auto_seated_eye_estimated = thresholds.map(|t| t.seated_estimated),
        auto_sit_below_m = thresholds.map(|t| t.sit_below_m),
        auto_floor_below_m = thresholds.map(|t| t.floor_below_m),
        auto_floor_level = classifier.map(|c| c.floor_level),
        auto_stand_above_m = thresholds.map(|t| t.stand_above_m),
        seated_reference_m = reference.map(|r| r.height_m),
        seated_reference_source = reference.map(|r| r.source.label()),
        seated_median_m = frame.seated_reference.median_m,
        seated_samples = frame.seated_reference.samples,
        crouched = frame.crouched,
        seated_crouch = frame.seated_crouch,
        capsule = r.roomscale.root.sim.capsule_owned,
        no_headroom = r.roomscale.root.sim.capsule_blocked,
        "camera height above Rico's root"
    );
}
