//! Synthetic fixtures exercise contracts, not native channel selection or runtime timing.

use glam::{Mat4, Quat, Vec3};

use crate::hooks::camera::authored::{
    composition::{
        Baseline, DeltaRequest, LocalSample, RootSample, RootTreatment, Sample, Stamp, Trs,
    },
    policy::{AcceptedCase, AcceptedRun, FinalizedBlend, Frame, GraphState},
};

mod composition;
mod temporal;

fn sample(finalized: u64, root: Trs, local: Trs) -> Sample {
    let stamp = Stamp {
        character: 10,
        continuity: 20,
        finalized,
    };
    Sample {
        root: RootSample {
            stamp,
            transform: root,
        },
        local: LocalSample {
            stamp,
            transform: local,
        },
    }
}

fn baseline() -> Baseline {
    Baseline {
        run: 30,
        sample: sample(1, Trs::IDENTITY, Trs::IDENTITY),
    }
}

fn frame(finalized: u64, state: GraphState, clip_time: f32, weight: f32) -> Frame {
    let sample = sample(finalized, Trs::IDENTITY, Trs::IDENTITY);
    Frame {
        accepted: AcceptedRun {
            case: AcceptedCase::BoostedGroundTakeoff,
            character: 10,
            continuity: 20,
            run: 30,
        },
        state,
        sample,
        baseline: baseline(),
        blend: FinalizedBlend {
            stamp: sample.root.stamp,
            run: 30,
            source: 40,
            weight,
        },
        source: 40,
        clip_time,
        request: DeltaRequest::RootCompensatedRelativeToBaseline,
        root_treatment: RootTreatment::ComposeWithAuthoredLocal,
    }
}

fn trs(translation: Vec3, rotation: Quat, scale: Vec3) -> Trs {
    Trs {
        translation,
        rotation,
        scale,
    }
}

fn assert_matrix(actual: Mat4, expected: Mat4) {
    for (actual, expected) in actual
        .to_cols_array()
        .into_iter()
        .zip(expected.to_cols_array())
    {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }
}
