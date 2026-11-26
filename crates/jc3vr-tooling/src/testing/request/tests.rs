use std::sync::atomic::{AtomicU64, Ordering};

use jc3vr_view::stereo::config::SinglePassSwitch;
use pretty_assertions::assert_eq;

use super::*;

struct RequestDirectory(PathBuf);

impl RequestDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "jc3vr-request-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn request(&self) -> PathBuf {
        self.0.join(FILE_NAME)
    }
}

impl Drop for RequestDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn a_claimed_request_cannot_remove_the_replacement_restoration() {
    let directory = RequestDirectory::new();
    let path = directory.request();
    std::fs::write(&path, "command = 'record'").unwrap();
    let claimed = claim(&path).unwrap().unwrap();
    assert!(!path.exists());
    let restoration = "command = 'visual_end'\nid = 'suite_end'";
    std::fs::write(&path, restoration).unwrap();
    assert_eq!(claimed.read().unwrap(), "command = 'record'");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), restoration);
    assert_eq!(take(&path).unwrap().as_deref(), Some(restoration));
    assert!(take(&path).unwrap().is_none());
}

#[test]
fn cancellation_before_claim_preserves_the_restoration_request() {
    let directory = RequestDirectory::new();
    let path = directory.request();
    std::fs::write(&path, "command = 'record'").unwrap();
    std::fs::rename(&path, path.with_extension("cancelled")).unwrap();
    assert!(take(&path).unwrap().is_none());
    let restoration = "command = 'visual_end'\nid = 'suite_end'";
    std::fs::write(&path, restoration).unwrap();
    assert_eq!(take(&path).unwrap().as_deref(), Some(restoration));
}

#[test]
fn stale_claims_are_discarded_without_acting_and_do_not_accumulate() {
    let directory = RequestDirectory::new();
    let path = directory.request();
    let stale = path.with_extension("toml.claimed");
    std::fs::write(&stale, "command = 'record'").unwrap();
    assert!(take(&path).unwrap().is_none());
    assert!(!stale.exists());
    std::fs::write(&stale, "command = 'record'").unwrap();
    std::fs::write(&path, "command = 'stop_recording'").unwrap();
    assert_eq!(
        take(&path).unwrap().as_deref(),
        Some("command = 'stop_recording'")
    );
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn unreadable_claims_are_removed_without_touching_new_requests() {
    let directory = RequestDirectory::new();
    let path = directory.request();
    for bytes in [vec![0xff], vec![b'x'; text::REQUEST_BYTES + 1]] {
        std::fs::write(&path, bytes).unwrap();
        let claimed = claim(&path).unwrap().unwrap();
        std::fs::write(&path, "command = 'stop_recording'").unwrap();
        assert!(matches!(
            claimed.read().unwrap_err().kind,
            RequestErrorKind::Read(_)
        ));
        assert_eq!(
            take(&path).unwrap().as_deref(),
            Some("command = 'stop_recording'")
        );
    }
}

#[test]
fn blocked_stale_claim_cleanup_leaves_the_pending_request_intact() {
    let directory = RequestDirectory::new();
    let path = directory.request();
    std::fs::create_dir(path.with_extension("toml.claimed")).unwrap();
    std::fs::write(&path, "command = 'stop_recording'").unwrap();
    assert!(matches!(
        take(&path).unwrap_err().kind,
        RequestErrorKind::Remove(_)
    ));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "command = 'stop_recording'"
    );
}

fn read(text: &str) -> Result<Request, RequestErrorKind> {
    parse(Path::new("C:/config/test_request.toml"), text).map_err(|e| e.kind)
}

#[test]
fn screenshot_ids_are_required_and_path_safe() {
    assert!(matches!(
        read("command = 'screenshot'\nid = 'safe-ascii-id'"),
        Ok(Request::Screenshot { motion: None, .. })
    ));
    for id in [
        "",
        "../escape",
        "a/b",
        "a\\b",
        "é",
        "has space",
        &"x".repeat(65),
    ] {
        let request = format!("command = 'screenshot'\nid = '{id}'");
        assert!(matches!(
            read(&request),
            Err(RequestErrorKind::InvalidId(_))
        ));
    }
    assert!(matches!(
        read("command = 'screenshot'"),
        Err(RequestErrorKind::InvalidId(_))
    ));
}

#[test]
fn water_reflection_requires_water_inputs_and_screenshot() {
    assert!(matches!(
        read(
            "command = 'screenshot'\nid = 'shot'\nwater_inputs = true\nwater_reflection = 'zero_weight'"
        ),
        Ok(Request::Screenshot {
            water_reflection: Some(jc3vr_capture::screenshot::harness::WaterReflection::ZeroWeight),
            ..
        })
    ));
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'\nwater_reflection = 'zero_weight'"),
        Err(RequestErrorKind::WaterReflectionNeedsInputs)
    ));
    assert!(matches!(
        read("command = 'visual_end'\nid = 'end'\nwater_reflection = 'zero_weight'"),
        Err(RequestErrorKind::WaterReflectionOnCommand(_))
    ));
    assert!(matches!(
        read(
            "command = 'screenshot'\nid = 'shot'\nwater_inputs = true\nwater_reflection = 'other'"
        ),
        Err(RequestErrorKind::Parse(_))
    ));
}

#[test]
fn visual_overrides_are_typed_and_configure_only() {
    let base = "command = 'visual_configure'\nid = 'configure'\nmode = 'multipass'\n";
    assert!(matches!(
        read(base),
        Ok(Request::Visual(visual::Request::Configure {
            visual_overrides: None,
            ..
        }))
    ));
    let Ok(Request::Visual(visual::Request::Configure {
        visual_overrides: Some(overrides),
        ..
    })) = read(&format!(
        "{base}[visual_overrides]\nsun_shadows = 'disabled'\n"
    ))
    else {
        panic!("the sun shadows override did not parse");
    };
    assert_eq!(
        overrides.sun_shadows,
        Some(visual::overrides::SunShadowsOverride::Disabled)
    );
    assert!(overrides.single_pass.is_empty());
    for table in [
        "[visual_overrides]\nsun_shadows = 'enabled'\n",
        "[visual_overrides]\nsun_shadows = 'disabled'\nunknown = true\n",
        "[visual_overrides.single_pass]\nssr_per_eye = 'off'\n",
    ] {
        assert!(matches!(
            read(&format!("{base}{table}")),
            Err(RequestErrorKind::Parse(_))
        ));
    }
    assert!(matches!(
        read(&format!("{base}[visual_overrides]\n")),
        Err(RequestErrorKind::InvalidVisualOverrides(
            visual::OverrideError::Empty
        ))
    ));
    for command in ["visual_end", "screenshot", "record"] {
        assert!(matches!(
            read(&format!(
                "command = '{command}'\nid = 'safe'\n[visual_overrides]\nsun_shadows = 'disabled'\n"
            )),
            Err(RequestErrorKind::VisualOverridesOnCommand(_))
        ));
    }
}

#[test]
fn single_pass_switch_overrides_are_validated_against_the_switches_and_mode() {
    let single_pass = "command = 'visual_configure'\nid = 'configure'\nmode = 'single_pass'\n";
    let Ok(Request::Visual(visual::Request::Configure {
        visual_overrides: Some(overrides),
        ..
    })) = read(&format!(
        "{single_pass}[visual_overrides]\nscreen_space_reflections = 'enabled'\n\
         [visual_overrides.single_pass]\nssr_per_eye = false\npixel_camera_per_eye = false\n"
    ))
    else {
        panic!("the single-pass switch overrides did not parse");
    };
    assert_eq!(
        overrides.single_pass.iter().collect::<Vec<_>>(),
        [
            (SinglePassSwitch::PixelCameraPerEye, false),
            (SinglePassSwitch::SsrPerEye, false),
        ]
    );
    assert!(matches!(
        read(&format!(
            "{single_pass}[visual_overrides.single_pass]\nssr_per_ey = false\n"
        )),
        Err(RequestErrorKind::InvalidVisualOverrides(
            visual::OverrideError::UnknownSwitch(name)
        )) if name == "ssr_per_ey"
    ));
    assert!(matches!(
        read(&format!("{single_pass}[visual_overrides.single_pass]\n")),
        Err(RequestErrorKind::InvalidVisualOverrides(
            visual::OverrideError::EmptySinglePass
        ))
    ));
    for mode in ["left_only", "right_only", "multipass"] {
        assert!(matches!(
            read(&format!(
                "command = 'visual_configure'\nid = 'configure'\nmode = '{mode}'\n\
                 [visual_overrides.single_pass]\nssr_per_eye = false\n"
            )),
            Err(RequestErrorKind::InvalidVisualOverrides(
                visual::OverrideError::SinglePassOnMode(_)
            ))
        ));
    }
}

#[test]
fn visual_requests_validate_ids_modes_and_warmup() {
    for mode in [
        "left_only",
        "right_only",
        "multipass",
        "reverse_multipass",
        "single_pass",
    ] {
        let request = format!("command = 'visual_configure'\nid = 'configure-1'\nmode = '{mode}'");
        assert!(matches!(
            read(&request),
            Ok(Request::Visual(visual::Request::Configure {
                warmup_frames: 30,
                pose: None,
                ..
            }))
        ));
    }
    assert!(matches!(
        read("command = 'visual_end'\nid = 'end-1'"),
        Ok(Request::Visual(visual::Request::End { .. }))
    ));
    for command in ["visual_configure", "visual_end"] {
        assert!(matches!(
            read(&format!("command = '{command}'\nid = '../escape'")),
            Err(RequestErrorKind::InvalidId(_))
        ));
    }
    assert!(matches!(
        read("command = 'visual_configure'\nid = 'missing-mode'"),
        Err(RequestErrorKind::NoMode)
    ));
    assert!(matches!(
        read("command = 'visual_configure'\nid = 'native'\nmode = 'native'"),
        Err(RequestErrorKind::InvalidMode)
    ));
    for frames in [0, 30, 300] {
        assert!(matches!(
            read(&format!(
                "command = 'visual_configure'\nid = 'valid'\nmode = 'multipass'\nwarmup_frames = {frames}"
            )),
            Ok(Request::Visual(_))
        ));
    }
    assert!(matches!(
        read(
            "command = 'visual_configure'\nid = 'invalid'\nmode = 'multipass'\nwarmup_frames = 301"
        ),
        Err(RequestErrorKind::InvalidWarmup)
    ));
    assert!(matches!(
        read(
            "command = 'visual_configure'\nid = 'invalid'\nmode = 'multipass'\nwarmup_frames = -1"
        ),
        Err(RequestErrorKind::Parse(_))
    ));
}

#[test]
fn visual_environment_tables_validate_hours_weather_and_commands() {
    let header = "command = 'visual_configure'\nid = 'configure-env'\nmode = 'multipass'\n";
    for hour in ["0.0", "13.5", "23.999"] {
        let text = format!("{header}[environment]\nhour = {hour}\nweather = 'sunny'");
        assert!(matches!(
            read(&text),
            Ok(Request::Visual(visual::Request::Configure {
                environment: Some(_),
                ..
            }))
        ));
    }
    for hour in ["nan", "inf", "-inf"] {
        let text = format!("{header}[environment]\nhour = {hour}\nweather = 'sunny'");
        assert!(matches!(
            read(&text),
            Err(RequestErrorKind::InvalidEnvironment(
                visual::EnvironmentError::NonFiniteHour
            ))
        ));
    }
    for hour in ["-0.001", "24.0", "25.0"] {
        let text = format!("{header}[environment]\nhour = {hour}\nweather = 'sunny'");
        assert!(matches!(
            read(&text),
            Err(RequestErrorKind::InvalidEnvironment(
                visual::EnvironmentError::HourOutOfRange
            ))
        ));
    }
    for text in [
        format!("{header}[environment]\nweather = 'sunny'"),
        format!("{header}[environment]\nhour = 13.5"),
        format!("{header}[environment]\nhour = 13.5\nweather = 'storm'"),
        format!("{header}[environment]\nhour = 13.5\nweather = 'sunny'\nday_cycle_speed = 0.0"),
    ] {
        assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
    }
    for (command, extra, expected) in [
        ("visual_end", "\nid = 'end-env'", Command::VisualEnd),
        ("record", "", Command::Record),
        ("screenshot", "\nid = 'shot'", Command::Screenshot),
    ] {
        let text =
            format!("command = '{command}'{extra}\n[environment]\nhour = 13.5\nweather = 'sunny'");
        assert!(
            matches!(
                read(&text),
                Err(RequestErrorKind::EnvironmentOnCommand(command)) if command == expected
            ),
            "{command}"
        );
    }
}

#[test]
fn visual_pose_tables_validate_shape_ids_and_angles() {
    let header = "command = 'visual_configure'\nid = 'configure-pose'\nmode = 'multipass'\n";
    for (yaw, pitch, roll) in [
        (0.0, 0.0, 0.0),
        (180.0, -180.0, 180.0),
        (-180.0, 180.0, -180.0),
        (30.0, -12.5, 4.25),
    ] {
        let text = format!(
            "{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = {yaw}\npitch_deg = {pitch}\nroll_deg = {roll}"
        );
        assert!(matches!(
            read(&text),
            Ok(Request::Visual(visual::Request::Configure {
                pose: Some(_),
                ..
            }))
        ));
    }
    assert!(matches!(
        read(header),
        Ok(Request::Visual(visual::Request::Configure {
            pose: None,
            ..
        }))
    ));
    for id in ["", "../escape", "a/b", "é", &"x".repeat(65)] {
        let text = format!(
            "{header}[pose]\ncase_id = '{id}'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"
        );
        assert!(matches!(
            read(&text),
            Err(RequestErrorKind::InvalidPose(
                visual::PoseError::InvalidCaseId(_)
            ))
        ));
    }
    for (angle, expected) in [
        ("181.0", visual::PoseError::AngleOutOfRange("yaw")),
        ("-181.0", visual::PoseError::AngleOutOfRange("yaw")),
        ("nan", visual::PoseError::NonFiniteAngle("yaw")),
        ("inf", visual::PoseError::NonFiniteAngle("yaw")),
        ("-inf", visual::PoseError::NonFiniteAngle("yaw")),
    ] {
        let text = format!(
            "{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = {angle}\npitch_deg = 0.0\nroll_deg = 0.0"
        );
        assert!(
            matches!(read(&text), Err(RequestErrorKind::InvalidPose(error)) if error == expected)
        );
    }
    for text in [
        format!("{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0"),
        format!("{header}[pose]\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"),
        format!(
            "{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0\nunknown = 1"
        ),
    ] {
        assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
    }
}

#[test]
fn a_pose_table_is_only_accepted_by_visual_configure() {
    for (command, extra, expected) in [
        ("visual_end", "\nid = 'end-pose'", Command::VisualEnd),
        ("record", "", Command::Record),
        ("screenshot", "\nid = 'shot'", Command::Screenshot),
    ] {
        let text = format!(
            "command = '{command}'{extra}\n[pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"
        );
        assert!(
            matches!(
                read(&text),
                Err(RequestErrorKind::PoseOnCommand(command)) if command == expected
            ),
            "{command}"
        );
    }
}

#[test]
fn oversized_requests_never_reach_dispatch() {
    assert!(matches!(
        read(&"x".repeat(text::REQUEST_BYTES + 1)),
        Err(RequestErrorKind::Read(TextError::Limit { .. }))
    ));
}

#[test]
fn the_recording_commands_read() {
    for (command, request) in [
        ("record", Request::Record),
        ("stop_recording", Request::StopRecording),
    ] {
        assert_eq!(read(&format!("command = \"{command}\"")).unwrap(), request);
    }
}

#[test]
fn an_unknown_command_is_refused() {
    assert!(matches!(
        read("command = \"launch\""),
        Err(RequestErrorKind::Parse(_))
    ));
}
#[test]
fn motion_tables_validate_period_amplitude_and_shape() {
    let header = "command = 'visual_configure'\nid = 'configure-motion'\nmode = 'multipass'\n";
    for period in [32, 300, 600] {
        let text = format!(
            "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = {period}\nyaw_deg = 10.0\npitch_deg = 5.0\nroll_deg = 2.5"
        );
        assert!(matches!(
            read(&text),
            Ok(Request::Visual(visual::Request::Configure {
                motion: Some(_),
                pose: None,
                ..
            }))
        ));
    }
    for period in [0, 1, 2, 31, 601, -1] {
        let text = format!(
            "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = {period}\nyaw_deg = 10.0\npitch_deg = 5.0\nroll_deg = 2.5"
        );
        assert!(
            matches!(
                read(&text),
                Err(RequestErrorKind::InvalidMotion(
                    visual::MotionError::PeriodOutOfRange(_)
                )) | Err(RequestErrorKind::Parse(_))
            ),
            "period {period}"
        );
    }
    for (angle, value) in [
        ("yaw", "45.1"),
        ("pitch", "-0.1"),
        ("roll", "90.0"),
        ("yaw", "nan"),
        ("pitch", "inf"),
    ] {
        let text = format!(
            "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = {}\npitch_deg = {}\nroll_deg = {}",
            if angle == "yaw" { value } else { "0.0" },
            if angle == "pitch" { value } else { "0.0" },
            if angle == "roll" { value } else { "0.0" },
        );
        assert!(
            matches!(
                read(&text),
                Err(RequestErrorKind::InvalidMotion(
                    visual::MotionError::AmplitudeOutOfRange(_)
                        | visual::MotionError::NonFiniteAmplitude(_)
                ))
            ),
            "{angle} {value}"
        );
    }
    for id in ["", "../escape", "a/b", "é", &"x".repeat(65)] {
        let text = format!(
            "{header}[motion]\ncase_id = '{id}'\nperiod_frames = 32\nyaw_deg = 1.0\npitch_deg = 0.0\nroll_deg = 0.0"
        );
        assert!(matches!(
            read(&text),
            Err(RequestErrorKind::InvalidMotion(
                visual::MotionError::InvalidCaseId(_)
            ))
        ));
    }
    for text in [
        format!("{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 1.0"),
        format!(
            "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 1.0\npitch_deg = 0.0\nroll_deg = 0.0\nunknown = 1"
        ),
    ] {
        assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
    }
}

#[test]
fn a_motion_table_and_a_pose_table_are_mutually_exclusive() {
    let text = "command = 'visual_configure'\nid = 'both'\nmode = 'multipass'\n\
                    [pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0\n\
                    [motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 1.0\npitch_deg = 0.0\nroll_deg = 0.0";
    assert!(matches!(read(text), Err(RequestErrorKind::MotionWithPose)));
}

#[test]
fn motion_tables_are_only_accepted_by_visual_configure() {
    for (command, extra, expected) in [
        ("visual_end", "\nid = 'end-motion'", Command::VisualEnd),
        ("record", "", Command::Record),
        ("screenshot", "\nid = 'shot'", Command::Screenshot),
    ] {
        let text = format!(
            "command = '{command}'{extra}\n[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"
        );
        assert!(
            matches!(
                read(&text),
                Err(RequestErrorKind::MotionOnCommand(command)) if command == expected
            ),
            "{command}"
        );
    }
}

#[test]
fn late_sky_stages_require_a_screenshot_without_motion() {
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'\nlate_sky_stages = true"),
        Ok(Request::Screenshot {
            late_sky_stages: true,
            ..
        })
    ));
    assert!(matches!(
        read("command = 'record'\nlate_sky_stages = true"),
        Err(RequestErrorKind::InvalidLateSkyStages)
    ));
    assert!(matches!(
        read(
            "command = 'screenshot'\nid = 'shot'\nlate_sky_stages = true\n[motion_capture]\nphase = 1"
        ),
        Err(RequestErrorKind::InvalidLateSkyStages)
    ));
}

#[test]
fn a_draw_census_requires_a_screenshot_without_motion() {
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'\ndraw_census = true"),
        Ok(Request::Screenshot {
            draw_census: true,
            ..
        })
    ));
    assert!(matches!(
        read("command = 'record'\ndraw_census = true"),
        Err(RequestErrorKind::InvalidDrawCensus)
    ));
    assert!(matches!(
        read(
            "command = 'screenshot'\nid = 'shot'\ndraw_census = true\n[motion_capture]\nphase = 1"
        ),
        Err(RequestErrorKind::InvalidDrawCensus)
    ));
}

#[test]
fn water_inputs_are_explicit_and_screenshot_only() {
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'\nwater_inputs = true"),
        Ok(Request::Screenshot {
            water_inputs: true,
            ..
        })
    ));
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'"),
        Ok(Request::Screenshot {
            water_inputs: false,
            ..
        })
    ));
    assert!(matches!(
        read("command = 'record'\nwater_inputs = true"),
        Err(RequestErrorKind::WaterInputsOnCommand(Command::Record))
    ));
}

#[test]
fn diagnostic_fields_are_typed_and_command_scoped() {
    let config = "command = 'visual_configure'\nid = 'cfg'\nmode = 'multipass'\n";
    assert!(matches!(
        read(&format!("{config}shader_identity = true")),
        Ok(Request::Visual(visual::Request::Configure {
            shader_identity: true,
            ..
        }))
    ));
    assert!(read("command = 'screenshot'\nid = 'shot'\nshader_identity = true").is_err());
    assert!(read("command = 'screenshot'\nid = 'shot'\ncascade_scale = 'shore_low'").is_err());
    assert!(matches!(
        read(
            "command = 'screenshot'\nid = 'shot'\nresolve_inputs = true\ncascade_scale = 'shore_high'"
        ),
        Ok(Request::Screenshot {
            cascade_scale: Some(jc3vr_capture::screenshot::harness::CascadeScale::ShoreHigh),
            ..
        })
    ));
    assert!(read("command = 'visual_end'\nid = 'end'\nresolve_inputs = true").is_err());
    assert!(matches!(
        read(
            "command = 'screenshot'\nid = 'shot'\nresolve_inputs = true\nsun_inputs = true\nsun_depth_clip = 'disabled_right'"
        ),
        Ok(Request::Screenshot {
            resolve_inputs: true,
            sun_depth_clip: Some(_),
            ..
        })
    ));
    assert!(
        read("command = 'screenshot'\nid = 'shot'\nsun_depth_clip = 'disabled_right'").is_err()
    );
    assert!(
        read("command = 'screenshot'\nid = 'shot'\nsun_inputs = true\nsun_depth_clip = 'other'")
            .is_err()
    );
}

#[test]
fn sun_inputs_are_explicit_and_screenshot_only() {
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'\nsun_inputs = true"),
        Ok(Request::Screenshot {
            sun_inputs: true,
            ..
        })
    ));
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'"),
        Ok(Request::Screenshot {
            sun_inputs: false,
            ..
        })
    ));
    assert!(matches!(
        read("command = 'record'\nsun_inputs = true"),
        Err(RequestErrorKind::SunInputsOnCommand(Command::Record))
    ));
}

#[test]
fn motion_capture_is_only_accepted_by_screenshot_and_carries_its_phase() {
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'\n[motion_capture]\nphase = 7"),
        Ok(Request::Screenshot {
            motion: Some(7),
            ..
        })
    ));
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'"),
        Ok(Request::Screenshot { motion: None, .. })
    ));
    for command in ["visual_configure", "visual_end", "record", "stop_recording"] {
        let extra = match command {
            "visual_configure" => "\nid = 'configure'\nmode = 'multipass'",
            "visual_end" => "\nid = 'end'",
            _ => "",
        };
        let text = format!("command = '{command}'{extra}\n[motion_capture]\nphase = 0");
        assert!(
            matches!(
                read(&text),
                Err(RequestErrorKind::MotionCaptureOnCommand(_))
            ),
            "{command}"
        );
    }
    for phase in ["-1", "4294967296", "'x'"] {
        let text =
            format!("command = 'screenshot'\nid = 'shot'\n[motion_capture]\nphase = {phase}");
        assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
    }
    assert!(matches!(
        read("command = 'screenshot'\nid = 'shot'\n[motion_capture]\nphase = 0\nunknown = 1"),
        Err(RequestErrorKind::Parse(_))
    ));
}

#[test]
fn visual_stage_requests_need_a_stage_table_only_they_accept() {
    let stage = "[stage]\nsettle_seconds = 5.0\n[stage.location]\nposition = [1.0, 2.0, 3.0]\n";
    assert!(matches!(
        read(&format!(
            "command = 'visual_stage'\nid = 'stage-1'\n{stage}"
        )),
        Ok(Request::Stage(_))
    ));
    assert!(matches!(
        read("command = 'visual_stage'\nid = 'stage-1'\n"),
        Err(RequestErrorKind::NoStage)
    ));
    assert!(matches!(
        read(&format!("command = 'visual_stage'\nid = 'bad id'\n{stage}")),
        Err(RequestErrorKind::InvalidId(_))
    ));
    assert!(matches!(
        read("command = 'visual_stage'\nid = 'stage-1'\n[stage]\nsettle_seconds = 5.0\n"),
        Err(RequestErrorKind::InvalidStage(
            crate::testing::stage::StageError::NothingToStage
        ))
    ));
    assert!(matches!(
        read(&format!(
            "command = 'visual_configure'\nid = 'c'\nmode = 'multipass'\n{stage}"
        )),
        Err(RequestErrorKind::StageOnCommand(Command::VisualConfigure))
    ));
}

#[test]
fn a_benchmark_request_needs_its_table_and_only_it_takes_one() {
    let request = read(
        "command = 'benchmark'\nid = 'b1'\n[benchmark]\nlabel = 'town'\nmode = 'single_pass'\n\
         record_seconds = 10.0\n[benchmark.single_pass]\nfsr_per_eye = false\n",
    )
    .unwrap();
    let Request::Benchmark(benchmark) = request else {
        panic!("expected a benchmark request");
    };
    assert_eq!(benchmark.label, "town");
    assert_eq!(benchmark.switches.iter().count(), 1);
    assert!(matches!(
        read("command = 'benchmark'\nid = 'b1'\n"),
        Err(RequestErrorKind::NoBenchmark)
    ));
    assert!(matches!(
        read(
            "command = 'snapshot_release'\nid = 'b1'\n[benchmark]\nlabel = 'a'\n\
             mode = 'multipass'\nrecord_seconds = 1.0\n"
        ),
        Err(RequestErrorKind::BenchmarkOnCommand(_))
    ));
    assert!(matches!(
        read(
            "command = 'benchmark'\nid = 'b1'\n[benchmark]\nlabel = 'a'\nmode = 'multipass'\n\
             record_seconds = 500.0\n"
        ),
        Err(RequestErrorKind::InvalidBenchmark(_))
    ));
}

#[test]
fn an_anchor_is_validated_and_configure_only() {
    let base = "command = 'visual_configure'\nid = 'configure'\nmode = 'multipass'\n";
    let Ok(Request::Visual(visual::Request::Configure {
        anchor: Some(anchor),
        ..
    })) = read(&format!(
        "{base}[anchor]\nposition = [2090.9, 1040.3, 5004.9]\norientation = [0.0, 0.0, 0.0, 1.0]\n"
    ))
    else {
        panic!("the anchor did not parse");
    };
    let values = anchor.values();
    assert_eq!(values.position.to_array(), [2090.9, 1040.3, 5004.9]);
    assert_eq!(
        [values.yaw_deg, values.pitch_deg, values.roll_deg],
        [0.0, 0.0, 0.0]
    );
    for table in [
        "[anchor]\nposition = [0.0, 0.0, nan]\norientation = [0.0, 0.0, 0.0, 1.0]\n",
        "[anchor]\nposition = [0.0, 0.0, 0.0]\norientation = [0.0, 0.0, 0.0, 0.5]\n",
    ] {
        assert!(matches!(
            read(&format!("{base}{table}")),
            Err(RequestErrorKind::InvalidPose(_))
        ));
    }
    assert!(matches!(
        read(
            "command = 'visual_end'\nid = 'safe'\n[anchor]\nposition = [0.0, 0.0, 0.0]\n\
             orientation = [0.0, 0.0, 0.0, 1.0]\n"
        ),
        Err(RequestErrorKind::AnchorOnCommand(_))
    ));
}
