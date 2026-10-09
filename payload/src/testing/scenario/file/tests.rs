use pretty_assertions::assert_eq;

use super::*;

fn path() -> PathBuf {
    PathBuf::from("/scenarios/climb.toml")
}

fn problem(text: &str) -> Problem {
    match parse(&path(), text)
        .expect_err("the scenario is refused")
        .kind
    {
        ScenarioErrorKind::Expectation { problem, .. } => problem,
        other => panic!("not an expectation's problem: {other:?}"),
    }
}

#[test]
fn a_scenario_reads_with_its_defaults() {
    let scenario = parse(
        &path(),
        r#"
            recording = "recordings/climb.jc3trk"

            [[expect]]
            signal = "climb.climbing"

            [[expect]]
            signal = "climb.grips"

            [[expect]]
            signal = "procedural.pass_through_error_m"
            below = 0.001

            [[expect]]
            status = "callback"
            detail = "GAME THREAD"

            [[expect]]
            name = "no on-foot warning"
            log = "the game left on foot"
            at_most = 0
        "#,
    )
    .expect("the scenario reads");
    assert_eq!(scenario.name, "climb");
    assert_eq!(
        scenario.recording,
        PathBuf::from("/scenarios/recordings/climb.jc3trk")
    );
    assert_eq!(scenario.options, ReplayOptions::default());
    assert_eq!(scenario.settle, Duration::from_secs(2));
    let summary: Vec<_> = scenario
        .expectations
        .iter()
        .map(|e| (e.name.as_str(), e.check, e.condition))
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "climb.climbing is true",
                Check::Eventually,
                Condition::Is(true)
            ),
            (
                "climb.grips growth at least 1",
                Check::AtEnd,
                Condition::AtLeast(1.0)
            ),
            (
                "procedural.pass_through_error_m below 0.001",
                Check::Always,
                Condition::Below(0.001)
            ),
            (
                "STATUS CALLBACK shows \"GAME THREAD\"",
                Check::Eventually,
                Condition::Is(true)
            ),
            ("no on-foot warning", Check::AtEnd, Condition::AtMost(0.0)),
        ]
    );
    assert_eq!(
        scenario.expectations[1].probe,
        Probe::Signal {
            signal: Signal::ClimbGrips,
            delta: true
        }
    );
    assert_eq!(
        scenario.expectations[3].probe,
        Probe::Status {
            label: "CALLBACK",
            health: None,
            detail: Some("GAME THREAD".to_owned()),
        }
    );
}

#[test]
fn the_replay_options_and_an_absolute_recording_are_kept() {
    let scenario = parse(
        &path(),
        r#"
            name = "parachute flare"
            recording = "C:/elsewhere/flare.jc3trk"
            start = "in_place"
            pacing = "frame_locked"
            settle_s = 5.5

            [[expect]]
            signal = "controllers.airborne"
            check = "always"
            is = false
        "#,
    )
    .expect("the scenario reads");
    assert_eq!(scenario.name, "parachute flare");
    assert_eq!(
        scenario.recording,
        PathBuf::from("C:/elsewhere/flare.jc3trk")
    );
    assert_eq!(
        scenario.options,
        ReplayOptions {
            start: StartMode::InPlace,
            pacing: Pacing::FrameLocked,
        }
    );
    assert_eq!(scenario.settle, Duration::from_secs_f32(5.5));
    assert_eq!(scenario.expectations[0].check, Check::Always);
    assert_eq!(scenario.expectations[0].condition, Condition::Is(false));
}

#[test]
fn a_status_row_with_nothing_asked_must_read_ok() {
    let scenario = parse(
        &path(),
        "recording = \"a.jc3trk\"\n[[expect]]\nstatus = \"STANCE\"\n",
    )
    .expect("the scenario reads");
    assert_eq!(
        scenario.expectations[0].probe,
        Probe::Status {
            label: "STANCE",
            health: Some(Health::Ok),
            detail: None,
        }
    );
}

#[test]
fn a_log_probe_takes_a_minimum_level() {
    let scenario = parse(
        &path(),
        "recording = \"a.jc3trk\"\n[[expect]]\nlog = \"\"\nlevel = \"warn\"\nat_most = 0\n",
    )
    .expect("the scenario reads");
    assert_eq!(
        scenario.expectations[0].probe,
        Probe::Log {
            pattern: String::new(),
            level: Some(Level::WARN),
        }
    );
}

#[test]
fn malformed_expectations_name_their_problem() {
    let with = |entry: &str| format!("recording = \"a.jc3trk\"\n[[expect]]\n{entry}\n");
    assert_eq!(problem(&with("name = \"x\"")), Problem::NoProbe);
    assert_eq!(
        problem(&with("signal = \"climb.climbing\"\nlog = \"x\"")),
        Problem::SeveralProbes
    );
    assert_eq!(
        problem(&with("signal = \"climb.climbin\"")),
        Problem::UnknownSignal("climb.climbin".to_owned())
    );
    assert_eq!(
        problem(&with("status = \"NOPE\"")),
        Problem::UnknownStatusRow("NOPE".to_owned())
    );
    assert_eq!(
        problem(&with("status = \"STANCE\"\nhealth = \"fine\"")),
        Problem::UnknownHealth("fine".to_owned())
    );
    assert_eq!(
        problem(&with("log = \"x\"\nlevel = \"debug\"")),
        Problem::UnknownLevel("debug".to_owned())
    );
    assert_eq!(
        problem(&with("signal = \"climb.grips\"\nat_least = 1\nat_most = 3")),
        Problem::SeveralConditions
    );
    assert_eq!(
        problem(&with("signal = \"locomotion.stick_speed_mps\"")),
        Problem::NeedsCondition
    );
    assert!(matches!(
        problem(&with("signal = \"climb.climbing\"\nabove = 1")),
        Problem::ConditionDoesNotFit(_)
    ));
    assert!(matches!(
        problem(&with("signal = \"climb.grips\"\nis = true")),
        Problem::ConditionDoesNotFit(_)
    ));
    assert_eq!(
        problem(&with("signal = \"climb.climbing\"\ndelta = true")),
        Problem::DeltaNeedsCounter
    );
    assert_eq!(
        problem(&with("signal = \"climb.climbing\"\nhealth = \"ok\"")),
        Problem::StatusFieldsNeedStatus
    );
    assert_eq!(
        problem(&with("signal = \"climb.climbing\"\nlevel = \"warn\"")),
        Problem::LevelNeedsLog
    );
    assert_eq!(problem(&with("log = \"\"")), Problem::EmptyLog);
}

#[test]
fn a_scenario_without_expectations_or_a_recording_is_refused() {
    let error = parse(&path(), "recording = \"a.jc3trk\"\n").expect_err("refused");
    assert!(matches!(error.kind, ScenarioErrorKind::NoExpectations));
    let error =
        parse(&path(), "recording = \" \"\n[[expect]]\nlog = \"x\"\n").expect_err("refused");
    assert!(matches!(error.kind, ScenarioErrorKind::NoRecording));
    let error = parse(&path(), "recordin = \"a\"\n").expect_err("refused");
    assert!(matches!(error.kind, ScenarioErrorKind::Parse(_)));
    assert!(
        error
            .to_string()
            .starts_with("scenario /scenarios/climb.toml: ")
    );
}

#[test]
fn the_shipped_templates_read() {
    macro_rules! templates {
        ($($name:literal),* $(,)?) => {
            [$(($name, include_str!(concat!("../../../../../testing/scenarios/", $name)))),*]
        };
    }
    let templates = templates![
        "articulated.toml",
        "character_grab.toml",
        "climb_moving.toml",
        "cockpit_buttons.toml",
        "door_by_handle.toml",
        "flight_controls.toml",
        "grip_targets.toml",
        "hand_bodies.toml",
        "hand_climbing.toml",
        "hand_strike.toml",
        "hijack.toml",
        "input_basics.toml",
        "mounted_gun.toml",
        "parachute_body.toml",
        "parachute_handles.toml",
        "passenger_aim.toml",
        "physical_guns.toml",
        "procedural_full_body.toml",
        "procedural_pass_through.toml",
        "push_up.toml",
        "recenter.toml",
        "seated_torso.toml",
        "second_long_gun.toml",
        "steering_wheel.toml",
        "stunt_positions.toml",
        "swim_torso.toml",
        "stick_locomotion.toml",
        "vehicle_doors.toml",
        "weapon_left_hand.toml",
        "weapon_pickup.toml",
        "wingsuit_arms.toml",
    ];
    for (name, text) in templates {
        let path = Path::new("C:/scenarios").join(name);
        let scenario = parse(&path, text).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            scenario.description.is_some(),
            "{name} says how to record it"
        );
    }
}

#[test]
fn an_error_names_the_expectation() {
    let error = parse(
        &path(),
        "recording = \"a.jc3trk\"\n[[expect]]\nlog = \"x\"\n[[expect]]\nname = \"second\"\n",
    )
    .expect_err("refused");
    assert_eq!(
        error.to_string(),
        "scenario /scenarios/climb.toml: expectation 2 (second): it names none of signal, status, \
         and log"
    );
}
