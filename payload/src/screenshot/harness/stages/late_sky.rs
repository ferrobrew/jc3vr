use jc3gi::graphics_engine::render_engine::RenderPassId;

use crate::screenshot::harness::{
    late_sky_stages_requested,
    stages::{HdrStage, Stage, StageRecord, capture_hdr, empty_stage_record},
};

pub(crate) fn capture_before_fog_apply(pass: i16) {
    if pass == RenderPassId::RP_FOG_VOLUME_APPLY as i16 && late_sky_stages_requested() {
        capture_hdr(HdrStage::BeforeFogVolumeApply);
    }
}

pub(crate) fn capture_late_sky(pass: i16) {
    let stage = match pass {
        id if id == RenderPassId::RP_APPLY_CLOUDS as i16 => HdrStage::AfterApplyClouds,
        id if id == RenderPassId::RP_FOG_VOLUME_APPLY as i16 => HdrStage::AfterFogVolumeApply,
        _ => return,
    };
    if late_sky_stages_requested() {
        capture_hdr(stage);
    }
}

pub(crate) fn complete_late_sky(records: &mut Vec<StageRecord>, frame: i64, eyes: [bool; 2]) {
    for (eye, required) in eyes.into_iter().enumerate() {
        if !required {
            continue;
        }
        for stage in [
            Stage::AfterApplyClouds,
            Stage::BeforeFogVolumeApply,
            Stage::AfterFogVolumeApply,
        ] {
            if !records
                .iter()
                .any(|r| (r.frame, r.eye, r.stage) == (frame, eye, stage))
            {
                let mut record = empty_stage_record(frame, eye, stage);
                record.error = Some("late sky stages: missing requested frame/eye boundary".into());
                records.push(record);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_records_cannot_substitute_another_frame_or_eye() {
        let mut records = vec![
            empty_stage_record(1, 1, Stage::BeforeFogVolumeApply),
            empty_stage_record(2, 0, Stage::BeforeFogVolumeApply),
            empty_stage_record(2, 1, Stage::AfterFogVolumeApply),
        ];
        for _ in 0..2 {
            complete_late_sky(&mut records, 2, [false, true]);
        }
        assert_eq!(records.len(), 5);
        assert!(
            records[3..]
                .iter()
                .all(|r| r.frame == 2 && r.eye == 1 && r.error.is_some())
        );
    }
}
