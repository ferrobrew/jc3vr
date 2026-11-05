//! The text lines of the events: each kind's fields after the time and the kind, one line each.

use std::io::{self, Write};

use glam::{EulerRot, Vec3};
use tracking_format::{
    event::{BodiesSample, EventKind, ForceKind, PoseSample},
    model::Hand,
};

/// An event's fields as its text line prints them, after the time and the kind, ending in a newline.
pub(crate) fn write_event_body(kind: &EventKind, out: &mut impl Write) -> io::Result<()> {
    match kind {
        EventKind::ControllerState(state) => writeln!(out, "{state:?}"),
        EventKind::GripAcquisition(grip) => writeln!(out, "{grip:?}"),
        EventKind::GunRelease(release) => writeln!(out, "{release:?}"),
        EventKind::TakeMarker(marker) => writeln!(
            out,
            "#{} at {} (unix ms {}){}",
            marker.index,
            marker.stamp,
            marker.unix_ms,
            if marker.label.is_empty() {
                String::new()
            } else {
                format!(" {:?}", marker.label)
            },
        ),
        EventKind::BodyState(state) => writeln!(
            out,
            "{} ({}){}",
            match (&state.rule_state_name, state.rule_state) {
                (Some(name), _) => name.to_string(),
                (None, Some(hash)) => format!("{hash:#010x}"),
                (None, None) => "no rule state".to_owned(),
            },
            state.case,
            if state.repeat { ", repeated" } else { "" },
        ),
        EventKind::Throw(throw) => writeln!(
            out,
            "{} {} {:.2} m/s {} spin {:.2} rad/s {} per game second, \
             {:.2} m/s per real second at time scale {}{}{}{}",
            hand_name(throw.hand),
            throw.item,
            throw.motion.linear.length(),
            vec3(throw.motion.linear),
            throw.motion.angular.length(),
            vec3(throw.motion.angular),
            throw.motion.linear.length() * throw.time_scale,
            throw.time_scale,
            throw
                .source
                .as_ref()
                .map_or(String::new(), |source| format!(", from {source}")),
            match throw.armed {
                Some(true) => ", armed",
                Some(false) => ", not armed",
                None => "",
            },
            throw.release.map_or(String::new(), |release| format!(
                ", left the hand at {}",
                vec3(release.position)
            )),
        ),
        EventKind::GripRelease(release) => writeln!(
            out,
            "{} let go of {}{}{} ({}){}",
            hand_name(release.hand),
            release.held,
            release
                .object
                .map_or(String::new(), |object| format!(" {object:#x}")),
            release
                .body
                .map_or(String::new(), |body| format!(" body {body}")),
            release.cause,
            if release.thrown { ", thrown" } else { "" },
        ),
        EventKind::WingsuitGesture(gesture) => writeln!(
            out,
            "{}{}",
            gesture
                .phase
                .as_deref()
                .unwrap_or("idle, neither falling nor in the suit"),
            gesture
                .why
                .as_ref()
                .map_or(String::new(), |why| format!(" ({why})")),
        ),
        EventKind::Snatch(snatch) => {
            write!(out, "{} -> {}", snatch.from, snatch.to)?;
            if let Some(hand) = snatch.hand {
                write!(out, " {}", hand_name(hand))?;
            }
            if let Some(outcome) = &snatch.outcome {
                write!(out, ", {outcome}")?;
            }
            if let Some(reason) = &snatch.reason {
                write!(out, ": {reason}")?;
            }
            if let Some(target) = &snatch.target {
                write!(out, ", target {target}")?;
            }
            if let Some(distance) = snatch.distance_m {
                write!(out, " at {distance:.2} m")?;
            }
            if let (Some(yank), Some(speed)) = (snatch.yank_m, snatch.yank_mps) {
                write!(out, ", yanked {yank:.2} m at {speed:.2} m/s")?;
            }
            writeln!(out)
        }
        EventKind::Pose(pose) => pose_line(pose, out),
        EventKind::Rig(rig) => {
            let o = &rig.output;
            writeln!(
                out,
                "{} body yaw {:+.1} deg toward {:+.1}, gap {:.3} m, demand {:.3} m, bend {:.1} deg, \
                 crouch {:.3} m, neck {}, pelvis floor ({:+.3}, {:+.3}), {} steps{}{}",
                o.phase,
                o.body_yaw.to_degrees(),
                o.yaw_target.to_degrees(),
                o.gap.length(),
                o.step_demand.length(),
                o.bend.length().to_degrees(),
                o.crouch_m,
                vec3(o.neck),
                rig.input.pelvis.x,
                rig.input.pelvis.y,
                o.steps,
                o.turn_cause
                    .as_ref()
                    .map_or(String::new(), |cause| format!(", turning ({cause})")),
                rig.snapped_to.map_or(String::new(), |at| format!(
                    ", snapped to ({:+.3}, {:+.3})",
                    at.x, at.y
                )),
            )
        }
        EventKind::RigConfig(config) => writeln!(
            out,
            "height {:.3} m, eyes {:.3} m; the rig's eyes {:.3} m and waist {:.3} m{}; tuning {}",
            config.height_m,
            config.eye_height_m,
            config.body_eye_height_m,
            config.body_hip_height_m,
            if config.repeat { ", repeated" } else { "" },
            config.tuning_json,
        ),
        EventKind::Bodies(bodies) => bodies_line(bodies, out),
        EventKind::BodyIntro(intro) => writeln!(
            out,
            "#{} {} {}{}, half extents {}{}, filter {:#010x}{}",
            intro.id,
            intro.kind,
            intro.shape,
            intro
                .mass_kg
                .map_or(String::new(), |mass| format!(", {mass:.1} kg")),
            vec3(intro.half_extents),
            intro
                .object
                .map_or(String::new(), |object| format!(", owner {object:#x}")),
            intro.filter,
            if intro.repeat { ", repeated" } else { "" },
        ),
        EventKind::Held(held) => {
            write!(
                out,
                "{} holds {} {:#x} body {}",
                hand_name(held.hand),
                held.kind,
                held.object,
                held.body
            )?;
            if let Some(motion) = &held.motion {
                write!(
                    out,
                    " at {} moving {:.2} m/s spinning {:.2} rad/s",
                    vec3(motion.pose.position),
                    motion.linear.length(),
                    motion.angular.length()
                )?;
            }
            if let Some(report) = &held.report {
                write!(
                    out,
                    ", {:.1} kg, lag {:.3} m, load {:.2}, twist {:.2}{}",
                    report.mass_kg,
                    report.lag_m,
                    report.load,
                    report.twist,
                    if report.joined { ", joined" } else { "" }
                )?;
            }
            writeln!(out)
        }
        EventKind::Force(force) => {
            match &force.kind {
                ForceKind::Velocity { linear, angular } => write!(
                    out,
                    "set body {} to {} m/s spin {} rad/s",
                    force.body,
                    vec3(*linear),
                    vec3(*angular)
                )?,
                ForceKind::Impulse { impulse, at } => write!(
                    out,
                    "impulse {} N s on body {} at {}",
                    vec3(*impulse),
                    force.body,
                    vec3(*at)
                )?,
            }
            if let Some(mass) = force.mass_kg {
                write!(out, ", {mass:.1} kg")?;
            }
            if let Some(before) = &force.before {
                write!(out, ", was {} m/s", vec3(before.linear))?;
            }
            writeln!(out, ", from {}:{}", force.source, force.line)
        }
        EventKind::Injected(injected) => {
            let writes: Vec<String> = injected
                .writes
                .iter()
                .map(|w| {
                    format!(
                        "{} {} {}{} {:.2} {}",
                        w.map,
                        w.action,
                        w.phase,
                        w.held_s.map_or(String::new(), |s| format!(" {s:.2} s")),
                        w.value,
                        w.outcome
                    )
                })
                .collect();
            writeln!(out, "{}", writes.join("; "))
        }
        EventKind::Diagnostic(diagnostic) => writeln!(
            out,
            "{}{}: {}{}",
            diagnostic.source,
            diagnostic
                .hand
                .map_or(String::new(), |hand| format!(" {}", hand_name(hand))),
            diagnostic.what,
            diagnostic
                .detail
                .as_ref()
                .map_or(String::new(), |detail| format!(" ({detail})")),
        ),
        EventKind::Haptic(haptic) => writeln!(
            out,
            "{} {} ({}) {:.2} for {:.0} ms{}",
            hand_name(haptic.hand),
            haptic.event,
            haptic.primitive,
            haptic.amplitude,
            haptic.duration_s * 1000.0,
            haptic
                .frequency_hz
                .map_or(String::new(), |hz| format!(" at {hz:.0} Hz")),
        ),
        EventKind::Settings(settings) => writeln!(
            out,
            "{} lines of settings (the JSON lines hold the text)",
            settings.toml.lines().count()
        ),
        EventKind::FrameTiming(timing) => {
            let mut slots: Vec<_> = timing.slots.iter().collect();
            slots.sort_by(|a, b| b.self_ms.total_cmp(&a.self_ms));
            let top: Vec<String> = slots
                .iter()
                .take(TIMING_SLOTS)
                .map(|slot| format!("{}={:.2}/{:.2}", slot.name, slot.self_ms, slot.max_ms))
                .collect();
            writeln!(
                out,
                "{} frames over {:.1} s, {:.2} ms mean, {:.2} ms longest, {} ticks; {}",
                timing.frames,
                timing.window_s,
                timing.frame_ms,
                timing.frame_max_ms,
                timing.sim_ticks,
                top.join(" ")
            )
        }
    }
}

pub(crate) fn hand_name(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "left",
        Hand::Right => "right",
    }
}

pub(super) fn vec3(v: Vec3) -> String {
    format!("({:+.3}, {:+.3}, {:+.3})", v.x, v.y, v.z)
}

/// The slots a frame-timing line names, the largest first.
const TIMING_SLOTS: usize = 6;

/// The bodies a bodies line lists.
const LISTED_BODIES: usize = 8;

fn pose_line(pose: &PoseSample, out: &mut impl Write) -> io::Result<()> {
    let (yaw, _, _) = pose.character.orientation.to_euler(EulerRot::YXZ);
    let present = pose.bones.iter().flatten().count();
    write!(
        out,
        "{} pass {}: Rico at {} heading {:+.1} deg, {present} of {} bones",
        pose.stage.name(),
        pose.pass,
        vec3(pose.character.position),
        yaw.to_degrees(),
        pose.bones.len(),
    )?;
    if let Some(hips) = pose.bones.first().copied().flatten() {
        write!(out, ", hips {}", vec3(hips.position))?;
    }
    if !pose.targets.is_empty() {
        let effectors: Vec<String> = pose
            .targets
            .iter()
            .map(|target| target.effector.to_string())
            .collect();
        write!(out, ", IK targets on effectors {}", effectors.join(" "))?;
    }
    writeln!(out)
}

fn bodies_line(bodies: &BodiesSample, out: &mut impl Write) -> io::Result<()> {
    write!(
        out,
        "step {} around {}: {} bodies, walked {} of {} slots, {} us",
        bodies.step,
        vec3(bodies.origin),
        bodies.bodies.len(),
        bodies.walked,
        bodies.peak.saturating_add(1),
        bodies.cost_us,
    )?;
    for body in bodies.bodies.iter().take(LISTED_BODIES) {
        write!(
            out,
            " | #{} {} {:.2} m/s{}{}{}",
            body.id,
            vec3(body.offset),
            body.linear.length(),
            if body.held[0] { " held left" } else { "" },
            if body.held[1] { " held right" } else { "" },
            if body.resting { " at rest" } else { "" },
        )?;
    }
    if bodies.bodies.len() > LISTED_BODIES {
        write!(out, " | {} more", bodies.bodies.len() - LISTED_BODIES)?;
    }
    writeln!(out)
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec2};
    use tracking_format::{
        event::{
            BODY_BONES, BodySample, BoneSet, Diagnostic, Force, FrameTiming, Haptic, Injected,
            InjectedWrite, Label, PoseStage, RigConfig, RigInput, RigResult, RigSample, Settings,
            SlotTiming, WorldMotion,
        },
        model::Pose,
    };

    use super::*;

    fn line(kind: EventKind) -> String {
        let mut out = Vec::new();
        write_event_body(&kind, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_version_8_kinds_print_one_line_each() {
        let pose = PoseSample {
            pass: 12,
            stage: PoseStage::PreIk,
            character: Pose {
                position: Vec3::new(10.0, 2.0, -5.0),
                orientation: Quat::from_rotation_y(0.5),
            },
            set: BoneSet::Body,
            bones: (0..BODY_BONES.len())
                .map(|i| {
                    (i != 3).then_some(Pose {
                        position: Vec3::new(0.0, 1.0, 0.0),
                        orientation: Quat::IDENTITY,
                    })
                })
                .collect(),
            targets: Vec::new(),
        };
        let bodies = BodiesSample {
            step: 4,
            origin: Vec3::ZERO,
            walked: 256,
            peak: 1023,
            cost_us: 21,
            bodies: vec![BodySample {
                id: 77,
                offset: Vec3::X,
                orientation: Quat::IDENTITY,
                linear: Vec3::new(0.0, 3.0, 4.0),
                angular: Vec3::ZERO,
                resting: false,
                held: [true, false],
            }],
        };
        let cases = [
            (
                EventKind::Pose(Box::new(pose)),
                "pre_ik pass 12: Rico at (+10.000, +2.000, -5.000) heading +28.6 deg, 22 of 23 bones",
            ),
            (
                EventKind::Bodies(Box::new(bodies)),
                "step 4 around (+0.000, +0.000, +0.000): 1 bodies, walked 256 of 1024 slots, 21 us \
                 | #77 (+1.000, +0.000, +0.000) 5.00 m/s held left",
            ),
            (
                EventKind::Force(Box::new(Force {
                    source: "payload/src/a.rs".into(),
                    line: 9,
                    kind: ForceKind::Impulse {
                        impulse: Vec3::Y,
                        at: Vec3::ZERO,
                    },
                    body: 77,
                    object: None,
                    mass_kg: Some(20.0),
                    before: Some(WorldMotion {
                        linear: Vec3::ZERO,
                        angular: Vec3::ZERO,
                    }),
                })),
                "impulse (+0.000, +1.000, +0.000) N s on body 77 at (+0.000, +0.000, +0.000), \
                 20.0 kg, was (+0.000, +0.000, +0.000) m/s, from payload/src/a.rs:9",
            ),
            (
                EventKind::Injected(Box::new(Injected {
                    writes: vec![InjectedWrite {
                        map: "Player".into(),
                        action: "FIRE_GRAPPLE".into(),
                        value: 1.0,
                        phase: "held".into(),
                        held_s: Some(0.5),
                        outcome: "applied".into(),
                    }],
                })),
                "Player FIRE_GRAPPLE held 0.50 s 1.00 applied",
            ),
            (
                EventKind::Diagnostic(Box::new(Diagnostic {
                    source: "pouch".into(),
                    hand: Some(Hand::Left),
                    what: "not placed".into(),
                    detail: Some(Label::Borrowed("no skeleton")),
                })),
                "pouch left: not placed (no skeleton)",
            ),
            (
                EventKind::Haptic(Box::new(Haptic {
                    hand: Hand::Right,
                    event: "grab".into(),
                    primitive: "click".into(),
                    amplitude: 0.5,
                    duration_s: 0.02,
                    frequency_hz: None,
                })),
                "right grab (click) 0.50 for 20 ms",
            ),
            (
                EventKind::Settings(Box::new(Settings {
                    toml: "[a]\nb = 1\n".into(),
                })),
                "2 lines of settings",
            ),
            (
                EventKind::FrameTiming(Box::new(FrameTiming {
                    window_s: 1.0,
                    frames: 90,
                    sim_ticks: 30,
                    frame_ms: 11.1,
                    frame_max_ms: 15.0,
                    slots: vec![SlotTiming {
                        name: "recorder".into(),
                        self_ms: 0.02,
                        max_ms: 0.1,
                        calls: 120,
                    }],
                })),
                "90 frames over 1.0 s, 11.10 ms mean, 15.00 ms longest, 30 ticks; recorder=0.02/0.10",
            ),
            (
                EventKind::RigConfig(Box::new(RigConfig {
                    height_m: 1.8,
                    eye_height_m: 1.68,
                    body_eye_height_m: 1.68,
                    body_hip_height_m: 0.95,
                    tuning_json: "{}".into(),
                    repeat: true,
                })),
                "height 1.800 m, eyes 1.680 m; the rig's eyes 1.680 m and waist 0.950 m, repeated; tuning {}",
            ),
            (
                EventKind::Rig(Box::new(RigSample {
                    input: RigInput {
                        eyes: Vec3::Y,
                        head: Quat::IDENTITY,
                        hands: [None, None],
                        pelvis: Vec2::new(0.1, 0.2),
                        moving: false,
                        sample_height: true,
                        dt_s: 0.011,
                    },
                    output: RigResult {
                        neck: Vec3::Y,
                        waist: Vec3::ZERO,
                        spine_m: 0.5,
                        offset: Vec2::ZERO,
                        bend: Vec2::ZERO,
                        lean_reach_m: 0.0,
                        step_demand: Vec2::ZERO,
                        crouch_m: 0.0,
                        hands_reaching: false,
                        phase: "idle".into(),
                        gap: Vec2::ZERO,
                        body_yaw: 0.0,
                        chest_yaw: 0.0,
                        yaw_target: 0.0,
                        turning: false,
                        turn_cause: None,
                        standing_neck_m: 1.5,
                        steps: 3,
                    },
                    floor_m: 0.0,
                    snapped_to: None,
                })),
                "idle body yaw +0.0 deg toward +0.0",
            ),
        ];
        for (kind, expected) in cases {
            let name = kind.name();
            let text = line(kind);
            assert!(text.contains(expected), "{name}: {expected:?} in {text:?}");
            assert_eq!(text.lines().count(), 1, "{name}: {text:?}");
        }
    }
}
