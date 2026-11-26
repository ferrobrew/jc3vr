//! The IK body's offline replay (the design's first stage, `docs/mod/body/ik-body.md`, "The offline
//! replay"): the FK pelvis and the foot contact model run over a recording's learned estimate, so
//! the contact thresholds can be tuned against what a player really did before a headset session.
//!
//! The replay works in the cockpit frame with the floor at zero, as the rig does. It runs only
//! where the payload's IK body would own the standing body: the body grounded on foot, the move
//! stick at rest, and the estimate fresh. A frame outside that drops the contacts, as the IK body's
//! hand-over to the walk or the clip does, and a manual or seat recenter drops them as the
//! payload's recenter does. The ground is flat, every probe answers at once, and the root follows
//! the FK pelvis's floor point exactly, so the root rule never fires here; the replay measures the
//! player's own body against the reach, balance, twist, and tidy rules.
//!
//! The summary ([`IkSummary`]) gives the steps per standing minute and their reasons, how long a
//! planted foot stood out of its leg's straight reach, the distribution of the pelvis's twist over
//! the planted feet, and how many steps each glance of the head set off.

mod stats;
#[cfg(test)]
mod tests;

use body_estimator::smpl::{JOINT_COUNT, PELVIS, SPINE3};
use body_rig::{
    PlayerBody,
    body::neck_pivot,
    contact::{
        ContactEvent, ContactFileTuning, ContactFrame, ContactTuning, FootContacts, FootPose,
        FootTarget, Ground, Probe, Stance, SupportFrame, SupportId, Surroundings, stance_spot,
    },
    fk::{ANKLE_HEIGHT_M, BodyRotations, FkFollower, FkFrame, FkSkeleton, FkTuning, PelvisSource},
    math::{Side, angle},
};
use glam::{Quat, Vec2, Vec3};
use tracking_format::{
    Recording,
    model::{RecenterCause, RecenterKind},
};

pub use crate::rig_eval::ik::stats::{IkSummary, RELEASES};
use crate::rig_eval::{
    RigFile,
    estimate::EstimateTrack,
    frames::{body_cases, grounded, head_pose, stick_moving},
    ik::stats::IkAccumulator,
};

/// What the replay runs with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct IkEvalOptions {
    pub body: PlayerBody,
    pub contact: ContactTuning,
    /// The contact model's tuning from the rig's tuning file.
    pub contact_file: ContactFileTuning,
    pub fk: FkTuning,
    /// The stance a step lands in: the square stance's defaults.
    pub stance: Stance,
    /// The most the IK body lowers the pelvis to keep the planted feet in reach, metres.
    pub max_pelvis_drop_m: f32,
}

impl IkEvalOptions {
    /// The defaults for a player `height` tall with the eyes at `eye_height`, with the tuning
    /// file's `file`.
    pub fn new(height: f32, eye_height: f32, file: &RigFile) -> Self {
        Self::for_body(PlayerBody::from_heights(height, eye_height), file)
    }

    /// The defaults for the player `body`, with the tuning file's `file`.
    pub fn for_body(body: PlayerBody, file: &RigFile) -> Self {
        Self {
            body,
            contact: ContactTuning::new(),
            contact_file: file.core.contact,
            fk: FkTuning::new(),
            stance: DEFAULT_STANCE,
            max_pelvis_drop_m: DEFAULT_MAX_PELVIS_DROP_M,
        }
    }
}

/// The IK body's `MAX_PELVIS_DROP_M` (the payload's `ik_body::gate`).
pub const DEFAULT_MAX_PELVIS_DROP_M: f32 = 0.2;

/// The square stance's `stance.width_m` and `stance.toe_out_deg` (the body crate's
/// `tuning/procedural.toml`).
pub const DEFAULT_STANCE: Stance = Stance {
    width_m: 0.29,
    toe_out_deg: 10.0,
};

/// Replay the FK pelvis and the contact model over `recording` with the estimate `estimate`.
pub fn evaluate_ik(
    recording: &Recording,
    options: &IkEvalOptions,
    estimate: &EstimateTrack,
) -> IkSummary {
    let cases = body_cases(recording);
    let skeleton = FkSkeleton::for_body(&options.body, options.body.standing_neck());
    let mut follower = FkFollower::new();
    let mut contacts = FootContacts::new();
    let mut stats = IkAccumulator::new(options.contact.twist_deg, options.max_pelvis_drop_m);
    let mut turned = 0.0f32;
    let mut last_time: Option<u64> = None;
    let mut standing = false;
    for frame in &recording.frames {
        let dt = last_time.map_or(0.0, |last| {
            frame.time_ns.saturating_sub(last) as f32 / 1.0e9
        });
        last_time = Some(frame.time_ns);
        let dt = dt.clamp(0.0, 0.1);
        let mut snapped = false;
        for recenter in &frame.recenters {
            turned =
                angle::wrap(turned + angle::yaw_of(recenter.turn * Vec3::NEG_Z).unwrap_or(0.0));
            snapped |= recenter.kind == RecenterKind::Full
                || matches!(
                    recenter.cause,
                    RecenterCause::Manual | RecenterCause::SeatTransition
                );
        }
        let Some(head) = head_pose(frame, options.body.eye_height) else {
            continue;
        };
        let time_s = frame.time_ns as f64 / 1.0e9;
        let head_yaw = angle::head_heading(head.orientation);
        let rotations = estimate
            .joints_at(frame.time_ns, turned)
            .map(|joints| rotations(&joints));
        let neck = neck_pivot(head.eyes, head.orientation);
        let fk = follower.update(
            &FkFrame {
                rotations,
                fallback_pelvis: None,
                fallback_yaw: head_yaw.unwrap_or(0.0),
                skeleton,
                dt,
            },
            &options.fk,
        );
        let owns = grounded(frame, &cases)
            && !stick_moving(frame)
            && fk.source == PelvisSource::Estimate
            && !snapped;
        stats.observe_head(time_s, head_yaw, dt, owns);
        if !owns {
            if standing {
                contacts.reset();
                stats.end_spell();
            }
            standing = false;
            continue;
        }
        standing = true;
        let body = fk.body.map(|p| neck + p);
        let pelvis_yaw = angle::line_yaw(body.hips[0], body.hips[1]).unwrap_or(0.0);
        let mut contact_frame = ContactFrame {
            pelvis: body.pelvis,
            pelvis_yaw,
            neck,
            hips: body.hips,
            leg_m: skeleton.leg_length(),
            ankle_height_m: ANKLE_HEIGHT_M,
            root: Vec3::new(body.pelvis.x, 0.0, body.pelvis.z),
            stance: options.stance,
            animated: [FootPose {
                ankle: Vec3::ZERO,
                yaw: 0.0,
            }; 2],
            dt,
        };
        contact_frame.animated = Side::BOTH.map(|side| {
            let (spot, yaw) = stance_spot(&contact_frame, side);
            FootPose {
                ankle: Vec3::new(spot.x, ANKLE_HEIGHT_M, spot.y),
                yaw,
            }
        });
        let out = contacts.update(
            &contact_frame,
            &mut FlatFloor,
            &options.contact,
            &options.contact_file,
        );
        for event in &out.events {
            if let ContactEvent::Released { reason, .. } = event {
                stats.step(time_s, *reason);
            }
        }
        if out.handover.is_some() {
            stats.handover();
        }
        let planted = out.feet.map(|foot| match foot {
            FootTarget::Planted(pose) => Some(pose),
            _ => None,
        });
        stats.observe_standing(&contact_frame, planted, dt);
    }
    stats.finish()
}

/// Log `summary`, the replay with the `index`th contact tuning `contact`.
pub fn log_summary(index: usize, contact: &ContactTuning, summary: &IkSummary) {
    let reasons: Vec<String> = RELEASES
        .iter()
        .zip(summary.reasons)
        .filter(|(_, count)| *count > 0)
        .map(|(reason, count)| format!("{} {count}", reason.label()))
        .collect();
    tracing::info!(
        tuning = index,
        contact = %serde_json::to_string(contact).unwrap_or_default(),
        standing_s = %format_args!("{:.0}", summary.standing_s),
        spells = summary.spells,
        steps = summary.steps,
        steps_per_minute = %format_args!("{:.2}", summary.steps_per_minute),
        reasons = %reasons.join(", "),
        handovers = summary.handovers,
        reach_over_s = %format_args!("{:.1}", summary.reach_over_s),
        reach_violations = summary.reach_violations,
        drop_p90_m = %format_args!("{:.3}", summary.drop_p90_m),
        max_drop_m = %format_args!("{:.3}", summary.max_drop_m),
        twist_p50_deg = %format_args!("{:.1}", summary.twist_p50_deg),
        twist_p90_deg = %format_args!("{:.1}", summary.twist_p90_deg),
        twist_p99_deg = %format_args!("{:.1}", summary.twist_p99_deg),
        twist_over_share = %format_args!("{:.3}", summary.twist_over_share),
        glances = summary.glances,
        glance_steps = summary.glance_steps,
        steps_per_glance = %format_args!("{:.3}", summary.steps_per_glance),
        "rig-eval: the IK body's replay"
    );
}

/// The FK's rotations from the estimate's joints, in SMPL's joint order.
fn rotations(joints: &[Quat; JOINT_COUNT]) -> BodyRotations {
    BodyRotations {
        pelvis: joints[PELVIS],
        spine1: joints[SPINE1],
        spine2: joints[SPINE2],
        spine3: joints[SPINE3],
        neck: joints[NECK],
        hips: [joints[LEFT_HIP], joints[RIGHT_HIP]],
        knees: [joints[LEFT_KNEE], joints[RIGHT_KNEE]],
    }
}

/// The SMPL joints the FK reads, by their place in the format's joint order.
const LEFT_HIP: usize = 1;
const RIGHT_HIP: usize = 2;
const SPINE1: usize = 3;
const LEFT_KNEE: usize = 4;
const RIGHT_KNEE: usize = 5;
const SPINE2: usize = 6;
const NECK: usize = 12;

/// Flat ground at zero, answered at once, in the static world.
struct FlatFloor;

impl Surroundings for FlatFloor {
    fn frame(&self, support: SupportId) -> Option<SupportFrame> {
        (support == SupportId::WORLD).then_some(SupportFrame::IDENTITY)
    }

    fn probe(&mut self, _side: Side, at: Vec2, _near: f32) -> Probe {
        if at.is_finite() {
            Probe::Hit(Ground {
                height: 0.0,
                support: SupportId::WORLD,
            })
        } else {
            Probe::Miss
        }
    }
}
