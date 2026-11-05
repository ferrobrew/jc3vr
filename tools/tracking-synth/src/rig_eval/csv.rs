//! The evaluation's rows as CSV.

use std::io::{self, Write};

use body_rig::{StepPhase, angle};

use crate::rig_eval::{ComparedRow, EvalRow};

/// Write `rows` as CSV.
pub fn write_csv(rows: &[EvalRow], out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "{HEADER}")?;
    for row in rows {
        write_row(row, out)?;
        writeln!(out)?;
    }
    Ok(())
}

/// Write `rows` as CSV, each with the recorded rig frame's body yaw, phase, and neck offset beside
/// it and the differences, empty where a row has no recorded frame.
pub fn write_compared_csv(rows: &[ComparedRow], out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "{HEADER},{RECORDED_HEADER}")?;
    for compared in rows {
        let row = &compared.row;
        write_row(row, out)?;
        match compared.recorded.as_ref() {
            Some(recorded) => writeln!(
                out,
                ",{:.2},{},{:.4},{:.2},{},{:.4}",
                recorded.body_yaw.to_degrees(),
                recorded
                    .phase
                    .map_or(String::new(), |phase| phase_code(phase).to_string()),
                recorded.offset.length(),
                recorded.yaw_difference(&row.output).to_degrees(),
                recorded
                    .phase_matches(&row.output)
                    .map_or(String::new(), |agrees| u8::from(agrees).to_string()),
                recorded.offset_difference(&row.output),
            )?,
            None => writeln!(out, ",,,,,,")?,
        }
    }
    Ok(())
}

const HEADER: &str = "time_s,video_s,phase,steps,demand_m,offset_m,lean_reach_m,bend_deg,pitch_deg,\
                      roll_deg,crouch_m,neck_y_m,standing_neck_m,head_yaw_deg,body_yaw_deg,\
                      yaw_target_deg,yaw_error_deg,chest_yaw_deg,turning,pelvis_x,pelvis_z,neck_x,\
                      neck_z,speed_mps,moving,hands_reaching";

const RECORDED_HEADER: &str = "recorded_body_yaw_deg,recorded_phase,recorded_offset_m,\
                               body_yaw_diff_deg,phase_match,offset_diff_m";

/// Write one row's own columns, without the line's end.
fn write_row(row: &EvalRow, out: &mut impl Write) -> io::Result<()> {
    let o = &row.output;
    let s = &o.solve;
    write!(
        out,
        "{:.4},{},{},{},{:.4},{:.4},{:.4},{:.2},{:.2},{:.2},{:.4},{:.4},{:.4},{:.2},{:.2},{:.2},\
         {:.2},{:.2},{},{:.4},{:.4},{:.4},{:.4},{:.3},{},{}",
        row.time_s,
        row.video_s.map_or(String::new(), |v| format!("{v:.3}")),
        phase_code(o.phase),
        o.steps,
        s.demand_m(),
        s.offset.length(),
        s.lean_reach_m,
        s.bend.angle().to_degrees(),
        s.bend.pitch.to_degrees(),
        s.bend.roll.to_degrees(),
        s.crouch_m,
        s.neck.y,
        o.standing_neck_m,
        row.head_yaw.to_degrees(),
        o.body_yaw.to_degrees(),
        o.yaw_target.to_degrees(),
        angle::wrap(row.head_yaw - o.body_yaw).to_degrees(),
        o.chest_yaw.to_degrees(),
        u8::from(o.turning),
        row.pelvis.x,
        row.pelvis.y,
        s.neck.x,
        s.neck.z,
        row.speed_mps,
        u8::from(row.moving),
        u8::from(s.hands_reaching),
    )
}

fn phase_code(phase: StepPhase) -> u8 {
    match phase {
        StepPhase::Idle => 0,
        StepPhase::Stepping => 1,
        StepPhase::Settling => 2,
        StepPhase::Drifting => 3,
    }
}
