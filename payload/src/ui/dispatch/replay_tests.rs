//! Renderer-free checks of the production Replay status and controls routes.

use std::path::PathBuf;

use super::*;
use crate::{
    config,
    testing::{
        replay::{self, Phase, ReplayOutcome, ReplayStatus},
        request,
        scenario::{self, ScenarioStatus},
        signal_status::{self, Readout, Snapshot},
        signals::{Signal, Value},
    },
};

#[test]
fn actual_replay_status_reads_replay_and_scenario_snapshots_without_a_renderer_or_actions() {
    assert_eq!(route(FeatureId::Replay), FeatureRoute::Replay);
    let status = replay::status();
    assert_eq!(status.phase, Phase::Idle);
    let before = replay_state();
    assert!(!before.active);
    assert!(!before.driving);
    let mut expected = status_labels(&status);
    expected.push("Scenarios".to_owned());
    expected.extend(scenario_labels(&scenario::status()));
    expected.push("Signals".to_owned());
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::Replay,
        section: FeatureSection::Status,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = replay_frame(&context, &mut selection, width, Vec::new());
        assert_status_labels(&labels, &expected);
        for (_, rect) in labels
            .into_iter()
            .filter(|(text, _)| text != "Signals")
            .take(12)
        {
            for pressed in [true, false] {
                let labels = replay_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert_status_labels(&labels, &expected);
                assert_eq!(selection, original_selection);
                assert_eq!(replay_state(), before);
                assert_eq!(config::status(), save_status);
            }
        }
        assert_eq!(selection, original_selection);
        assert_eq!(replay_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[test]
fn actual_replay_controls_keep_idle_forms_and_instructions_without_activation() {
    let status = replay::status();
    assert_eq!(status.phase, Phase::Idle);
    let before = replay_state();
    assert!(!before.active);
    assert!(!before.driving);
    let mut readouts = status_labels(&status);
    readouts.extend(scenario_labels(&scenario::status()));
    let instruction = format!(
        "Ctrl+Numpad- runs the last scenario again or stops it. A script can drop {} into the \
         configuration directory to start and stop the tools.",
        request::FILE_NAME
    );
    let mut selection = NavigationState {
        feature: FeatureId::Replay,
        section: FeatureSection::Controls,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = replay_frame(&context, &mut selection, width, Vec::new());
        for expected in [
            "Recording:",
            "Last",
            "Start:",
            "at the start marker",
            "at the bookmark",
            "in place",
            "Pacing:",
            "real time",
            "one per frame",
            "Plays the recording's head and controllers in place of the headset's, with or without a \
             session. Ctrl+Numpad* replays the last recording or stops.",
            "Scenarios",
            "Folder setting:",
            instruction.as_str(),
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == expected),
                "missing Replay control or instruction: {expected}"
            );
        }
        assert_eq!(
            labels.iter().filter(|(text, _)| text == "Replay").count(),
            2,
            "idle Replay Controls retains its header and Replay button"
        );
        for readout in &readouts {
            assert!(
                labels.iter().all(|(text, _)| text != readout),
                "unexpected Replay Controls status readout: {readout}"
            );
        }
        assert!(
            labels
                .iter()
                .all(|(text, _)| !text.starts_with("Running ") && !text.starts_with("Last: ")),
            "scenario reports belong in Replay Status"
        );
        assert!(
            Signal::ALL
                .iter()
                .all(|signal| labels.iter().all(|(text, _)| text != signal.name())),
            "Signal rows belong in Replay Status"
        );
        for forbidden in [
            "Signals",
            "Summary:",
            "Stop",
            "Start recording",
            "Stop recording",
            "Test tools available",
        ] {
            assert!(
                labels.iter().all(|(text, _)| text != forbidden),
                "unexpected idle Replay control: {forbidden}"
            );
        }
        assert!(
            labels
                .iter()
                .all(|(text, _)| !text.starts_with("Unavailable: the DX11 renderer"))
        );
        assert_eq!(selection, original_selection);
        assert_eq!(replay_state(), before);
    }
}

#[test]
fn actual_replay_status_expands_every_signal_row_without_commands_or_source_reads() {
    for width in [420.0, 1100.0] {
        signal_status::publish();
        let status = signal_status::snapshot();
        assert_eq!(
            status.get(Signal::ReplayDriving),
            Readout::Current(Value::Bool(replay::is_driving()))
        );
        assert_eq!(
            status.get(Signal::Recenters),
            Readout::Current(Value::Number(f64::from(crate::vr::recenter_count())))
        );
        let before = replay_state();
        let save_status = config::status();
        let context = egui::Context::default();
        context.style_mut_of(egui::Theme::Dark, |style| style.animation_time = 0.0);
        context.style_mut_of(egui::Theme::Light, |style| style.animation_time = 0.0);
        let mut selection = NavigationState {
            feature: FeatureId::Replay,
            section: FeatureSection::Status,
        };
        let original_selection = selection;
        let collapsed = replay_frame(&context, &mut selection, width, Vec::new());
        let header = collapsed
            .iter()
            .find(|(text, _)| text == "Signals")
            .expect("Replay Status retains the collapsed Signals header")
            .1;
        assert!(
            Signal::ALL
                .iter()
                .all(|signal| collapsed.iter().all(|(text, _)| text != signal.name()))
        );
        for pressed in [true, false] {
            replay_frame(
                &context,
                &mut selection,
                width,
                pointer_event(header.center(), pressed),
            );
        }
        let labels = replay_frame(&context, &mut selection, width, Vec::new());
        let first = assert_signal_rows(&labels, &status, width);
        if width == 420.0 {
            let font = egui::TextStyle::Body.resolve(&context.style_of(egui::Theme::Dark));
            let line_height = context.fonts_mut(|fonts| fonts.row_height(&font));
            assert!(
                labels[first..]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .any(|row| row[2].1.height() > line_height * 1.5),
                "narrow Signals descriptions wrap without hiding their text"
            );
        }
        for (_, rect) in labels[first..].iter().take(12) {
            for pressed in [true, false] {
                let after = replay_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert_eq!(assert_signal_rows(&after, &status, width), first);
                assert_eq!(
                    after[..first]
                        .iter()
                        .map(|(text, _)| text)
                        .collect::<Vec<_>>(),
                    labels[..first]
                        .iter()
                        .map(|(text, _)| text)
                        .collect::<Vec<_>>()
                );
                assert_eq!(replay_state(), before);
                assert_eq!(config::status(), save_status);
                assert_eq!(selection, original_selection);
            }
        }
        assert_eq!(replay_state(), before);
        assert_eq!(config::status(), save_status);
        assert_eq!(selection, original_selection);
    }
}

#[derive(Debug, PartialEq)]
struct ReplayState {
    saved_config: serde_json::Value,
    transient_director: (f32, bool, bool),
    status: String,
    active: bool,
    driving: bool,
    recenters: u32,
    scenario_status: String,
    scenario_running: bool,
    scenario_path: Option<PathBuf>,
}

fn replay_state() -> ReplayState {
    let cfg = config::get();
    ReplayState {
        saved_config: serde_json::to_value(&cfg).unwrap(),
        transient_director: (
            cfg.director.time_scale,
            cfg.director.paused,
            cfg.director.cinematic,
        ),
        status: format!("{:?}", replay::status()),
        active: replay::is_active(),
        driving: replay::is_driving(),
        recenters: crate::vr::recenter_count(),
        scenario_status: format!("{:?}", scenario::status()),
        scenario_running: scenario::is_running(),
        scenario_path: scenario::last_path(),
    }
}

fn status_labels(status: &ReplayStatus) -> Vec<String> {
    let mut labels = vec![match status.phase {
        Phase::Idle => "idle".to_owned(),
        Phase::Loading => "loading".to_owned(),
        Phase::Teleporting => "teleporting to the start".to_owned(),
        Phase::Playing => format!(
            "playing frame {} of {}, {:.1} s",
            status.frame,
            status.frames,
            status.elapsed.as_secs_f32()
        ),
    }];
    labels.extend(status.warnings.iter().cloned());
    match &status.last {
        Some(ReplayOutcome::Completed { frames, duration }) => labels.push(format!(
            "Last replay: {frames} frames in {:.1} s",
            duration.as_secs_f32()
        )),
        Some(ReplayOutcome::Stopped) => labels.push("Last replay: stopped".to_owned()),
        Some(ReplayOutcome::Failed(error)) => labels.push(error.to_string()),
        None => {}
    }
    labels
}

fn scenario_labels(status: &ScenarioStatus) -> Vec<String> {
    let mut labels = Vec::new();
    if let Some((name, results)) = &status.running {
        labels.push(format!("Running {name}"));
        for result in results {
            labels.push(
                if result.verdict.passed {
                    "PASS"
                } else {
                    "...."
                }
                .to_owned(),
            );
            labels.push(result.name.clone());
            labels.push(result.verdict.detail.clone());
        }
    }
    labels.extend(status.error.iter().cloned());
    if let Some(report) = &status.last {
        let total = report.results.len();
        let passed = report.passed();
        labels.push(match &report.aborted {
            Some(reason) => format!("Last: {}: did not finish: {reason}", report.name),
            None => format!(
                "Last: {}: {passed} of {total} passed over {} frames",
                report.name, report.frames
            ),
        });
        for result in &report.results {
            labels.push(
                if result.verdict.passed {
                    "PASS"
                } else {
                    "FAIL"
                }
                .to_owned(),
            );
            labels.push(result.name.clone());
            labels.push(result.verdict.detail.clone());
        }
        if let Some(summary) = &report.summary {
            labels.push("Summary:".to_owned());
            labels.push(summary.display().to_string());
        }
    }
    labels
}

fn assert_signal_rows(labels: &[(String, egui::Rect)], status: &Snapshot, width: f32) -> usize {
    let first = labels
        .iter()
        .position(|(text, _)| text == Signal::ALL[0].name())
        .expect("expanding Signals renders its table");
    assert_eq!(labels.len() - first, Signal::ALL.len() * 3);
    for (&signal, row) in Signal::ALL.iter().zip(labels[first..].as_chunks::<3>().0) {
        assert_eq!(row[0].0, signal.name());
        assert_eq!(row[2].0, signal.description());
        for (text, rect) in row {
            assert!(
                rect.left() >= 0.0 && rect.right() <= width + 1.0,
                "signal text extends beyond the {width}-point viewport: {text}: {rect:?}"
            );
        }
        assert!(row[0].1.right() <= row[1].1.left() + 1.0);
        assert!(row[1].1.right() <= row[2].1.left() + 1.0);
        match status.get(signal) {
            Readout::Current(value) => assert!(
                row[1].0 == value.to_string()
                    || (row[1].0.starts_with("stale (")
                        && row[1].0.ends_with(&format!("): {value}"))),
                "unexpected value for {}: {}",
                signal.name(),
                row[1].0
            ),
            Readout::Stale { value, .. } => {
                assert!(row[1].0.starts_with("stale ("));
                assert!(row[1].0.ends_with(&format!("): {value}")));
            }
            readout => assert_eq!(row[1].0, readout.to_string()),
        }
    }
    let unsupported = Readout::Unsupported.to_string();
    assert_eq!(
        labels
            .iter()
            .filter(|(text, _)| text == &unsupported)
            .count(),
        Signal::ALL.len() - 8,
        "only the reviewed atomic and root rows have UI snapshot sources"
    );
    first
}

fn assert_status_labels(labels: &[(String, egui::Rect)], expected: &[String]) {
    assert_eq!(
        labels.len(),
        expected.len(),
        "Replay Status must contain only replay and scenario readouts and collapsed Signals"
    );
    for ((text, _), expected) in labels.iter().zip(expected) {
        assert_eq!(text, expected);
    }
    for forbidden in [
        "Recording:",
        "Last",
        "Start:",
        "Pacing:",
        "Replay",
        "Run",
        "Stop",
        "Folder setting:",
    ] {
        assert!(
            labels.iter().all(|(text, _)| text != forbidden),
            "unexpected Replay Status control or form: {forbidden}"
        );
    }
}

fn replay_frame(
    context: &egui::Context,
    selection: &mut NavigationState,
    width: f32,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 100_000.0),
            )),
            events,
            ..Default::default()
        },
        |ui| feature(ui, selection, None),
    );
    let mut labels = Vec::new();
    for clipped in &output.shapes {
        collect_text(&clipped.shape, &mut labels);
    }
    labels
}

fn collect_text(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => labels.push((
            text.galley.text().to_owned(),
            egui::Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, labels);
            }
        }
        _ => {}
    }
}

fn pointer_event(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        },
    ]
}
