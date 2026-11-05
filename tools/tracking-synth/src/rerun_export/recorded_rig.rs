//! The grounded rig the game ran (from version 8), drawn beside the offline rig: the same pivots,
//! spine, lean ring, step demand, and heading under `world/cockpit/room/rig_recorded`, its scalars
//! under `plots/rig/recorded_*`, and its step phase on `states/rig_step_recorded`.
//!
//! The recorded rig works in the room frame its sample's floor gives; the export's room frame is
//! the frame's own floor estimate, so the recorded rig's entity carries the difference.

use body_rig::angle;
use glam::Vec3;
use rerun::{Clear, Color, RecordingStreamResult, Rgba32, StateChange, Transform3D};
use tracking_format::event::{EventKind, RigSample};

use crate::rerun_export::{
    Context,
    scene::{RigDraw, floor, log_rig_draw},
};

/// Log the rig samples within `(from, to)`.
pub(crate) fn log(context: &Context<'_>, (from, to): (u64, u64)) -> RecordingStreamResult<()> {
    let samples: Vec<(u64, &RigSample)> = context
        .recording
        .events
        .iter()
        .filter(|event| (from..=to).contains(&event.time_ns))
        .filter_map(|event| match &event.kind {
            EventKind::Rig(sample) => Some((event.time_ns, sample.as_ref())),
            _ => None,
        })
        .collect();
    let mut phase: Option<&str> = None;
    for (index, &(time_ns, sample)) in samples.iter().enumerate() {
        context.set_time(time_ns);
        log_sample(context, time_ns, sample)?;
        if phase != Some(sample.output.phase.as_ref()) {
            context.rec.log(
                "states/rig_step_recorded",
                &StateChange::single(sample.output.phase.as_ref()),
            )?;
            phase = Some(sample.output.phase.as_ref());
        }
        // The rig runs every grounded frame; a gap is a stretch it did not run, cleared a frame
        // after its last sample.
        let next = samples.get(index + 1).map_or(to, |(t, _)| *t);
        if next.saturating_sub(time_ns) > GAP_NS {
            context.set_time(time_ns + CLEAR_AFTER_NS);
            context.rec.log(BASE, &Clear::recursive())?;
            context.rec.log(
                "states/rig_step_recorded",
                &StateChange::single("not grounded"),
            )?;
            phase = None;
        }
    }
    Ok(())
}

const BASE: &str = "world/cockpit/room/rig_recorded";

const RIG_RECORDED: Color = Color(Rgba32::from_rgb(60, 220, 220));

/// A gap between samples longer than this is a stretch the rig did not run, nanoseconds.
const GAP_NS: u64 = 200_000_000;

/// How long after its last sample a stopped rig is cleared, nanoseconds: about a frame.
const CLEAR_AFTER_NS: u64 = 20_000_000;

fn log_sample(
    context: &Context<'_>,
    time_ns: u64,
    sample: &RigSample,
) -> RecordingStreamResult<()> {
    let room_floor = context
        .frame_at(time_ns)
        .and_then(|index| floor(&context.frames[index], context.eye_height));
    let correction = room_floor.map_or(0.0, |room| sample.floor_m - room);
    context
        .rec
        .log(BASE, &Transform3D::from_translation([0.0, correction, 0.0]))?;
    let o = &sample.output;
    let head_yaw = angle::yaw_of(sample.input.head * Vec3::NEG_Z);
    let mut scalars = vec![
        ("bend_deg", f64::from(o.bend.length().to_degrees())),
        ("demand_m", f64::from(o.step_demand.length())),
        ("offset_m", f64::from(o.offset.length())),
        ("lean_reach_m", f64::from(o.lean_reach_m)),
        ("crouch_m", f64::from(o.crouch_m)),
        ("neck_height_m", f64::from(o.neck.y)),
        ("standing_neck_m", f64::from(o.standing_neck_m)),
        ("gap_m", f64::from(o.gap.length())),
    ];
    if let Some(head_yaw) = head_yaw {
        scalars.push((
            "yaw_error_deg",
            f64::from(angle::wrap(head_yaw - o.body_yaw).to_degrees()),
        ));
    }
    let rig = RigDraw {
        neck: o.neck,
        waist: o.waist,
        pelvis: sample.input.pelvis,
        lean_reach_m: o.lean_reach_m,
        step_demand: o.step_demand,
        body_yaw: o.body_yaw,
        scalars,
    };
    log_rig_draw(context, BASE, "plots/rig/recorded_", RIG_RECORDED, &rig)
}
