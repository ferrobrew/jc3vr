//! The default layout: the 3D scene and the plots on the left, the video, the event log, the state
//! lanes, and the export's notes on the right, and the coarse movement lane as a thin row across
//! the bottom, always in view.

use rerun::{
    RecordingStream, RecordingStreamResult,
    blueprint::{
        Blueprint, BlueprintActivation, ContainerLike, Horizontal, Spatial2DView, Spatial3DView,
        StateTimelineView, Tabs, TextDocumentView, TextLogView, TimePanel, TimeSeriesView,
        Vertical,
    },
};

use crate::rerun_export::{TIMELINE, character::MOVE_LANE};

/// Send the layout, with a video view when the export has a video.
pub(crate) fn send(rec: &RecordingStream, video: bool) -> RecordingStreamResult<()> {
    let scene = Tabs::new([
        Spatial3DView::new("world").with_origin("/world").into(),
        Spatial3DView::new("cockpit")
            .with_origin("/world/cockpit")
            .with_contents(["+ /world/cockpit/**"])
            .into(),
    ]);
    let plots = Tabs::new(PLOTS.iter().map(|(name, queries)| {
        ContainerLike::from(TimeSeriesView::new(*name).with_contents(queries.iter().copied()))
    }));
    let left = Vertical::new([scene.into(), plots.into()]).with_row_shares([0.62, 0.38]);
    let log = Tabs::new([
        TextLogView::new("events").with_origin("/events").into(),
        StateTimelineView::new("states")
            .with_origin("/states")
            .into(),
        TextDocumentView::new("about").with_origin("/about").into(),
        TextDocumentView::new("settings")
            .with_origin("/settings")
            .into(),
    ]);
    let right = if video {
        Vertical::new([
            Spatial2DView::new("video").with_origin("/video").into(),
            log.into(),
        ])
        .with_row_shares([0.45, 0.55])
    } else {
        Vertical::new([log.into()])
    };
    let main = Horizontal::new([left.into(), right.into()]).with_column_shares([0.58, 0.42]);
    let motion = StateTimelineView::new("move").with_origin(format!("/{MOVE_LANE}"));
    Blueprint::new(
        Vertical::new([main.into(), motion.into()])
            .with_row_shares([1.0 - MOVE_ROW_SHARE, MOVE_ROW_SHARE]),
    )
    .with_auto_views(false)
    .with_time_panel(TimePanel::new().with_timeline(TIMELINE))
    .send(rec, BlueprintActivation::default())
}

/// The share of the layout's height the movement lane's row takes: a thin strip.
const MOVE_ROW_SHARE: f32 = 0.06;

/// The plot tabs and the entities each shows.
const PLOTS: [(&str, &[&str]); 10] = [
    ("Rico", &["+ /plots/rico/**", "+ /plots/height/**"]),
    ("rig", &["+ /plots/rig/**"]),
    ("hands", &["+ /plots/hands/**"]),
    ("roomscale", &["+ /plots/roomscale/**"]),
    ("arms", &["+ /plots/ik/**", "+ /plots/late/**"]),
    ("frame", &["+ /plots/frame/**", "+ /plots/registration/**"]),
    ("camera", &["+ /plots/camera/**"]),
    ("timing", &["+ /plots/timing/**"]),
    ("held", &["+ /plots/held/**", "+ /plots/grapple/**"]),
    ("vehicle", &["+ /plots/vehicle/**"]),
];
