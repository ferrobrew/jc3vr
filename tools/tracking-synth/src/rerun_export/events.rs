//! The events other than the controller states, as text log entries under `events/`, state lanes
//! under `states/`, plots under `plots/timing/`, the settings under `settings`, and, for a grip's
//! target, a throw, and a snatch, marks in the 3D scene; and the frames' own moments (a recenter, a
//! change of session state, of focus, or of action set) as log entries and lanes.
//!
//! The poses, the recorded rig, and the bodies have their own modules; their events are skipped
//! here, but for a body's introduction and the rig's configuration, which are logged as text.

use glam::Vec3;
use rerun::{
    Arrows3D, Clear, Color, Points3D, RecordingStreamResult, Rgba32, Scalars, StateChange,
    TextDocument, TextLog, components::TextLogLevel,
};
use tracking_format::{
    event::{EventKind, FrameTiming, GripAcquisition, GripStage, GunRelease, Snatch, Throw},
    model::RecenterCause,
};

use crate::{
    dump,
    rerun_export::{
        Context,
        character::lane,
        path_part,
        scene::{ARROW_S, hand_name},
    },
};

/// Log the events within `(from, to)` and the frames' moments; returns how many entries were
/// logged.
pub(crate) fn log(context: &Context<'_>, (from, to): (u64, u64)) -> RecordingStreamResult<usize> {
    let mut log = Log {
        context,
        entries: 0,
        throws: 0,
        snatch_distance: [None; 2],
        grip_target: [false; 2],
    };
    if context.recording.truncated {
        context.set_time(from);
        log.text(
            "events/recording",
            TextLogLevel::WARN,
            "the recording was cut short".into(),
        )?;
    }
    log.frames()?;
    for event in &context.recording.events {
        if !(from..=to).contains(&event.time_ns) {
            continue;
        }
        context.set_time(event.time_ns);
        match &event.kind {
            EventKind::ControllerState(_) => {}
            EventKind::GripAcquisition(grip) => log.grip_acquisition(grip)?,
            EventKind::GunRelease(release) => log.gun_release(release)?,
            EventKind::TakeMarker(_) => {
                log.text("events/marker", TextLogLevel::INFO, body(&event.kind))?
            }
            EventKind::BodyState(state) => {
                let rule = match (&state.rule_state_name, state.rule_state) {
                    (Some(name), _) => name.to_string(),
                    (None, Some(hash)) => format!("{hash:#010x}"),
                    (None, None) => "no rule state".to_owned(),
                };
                context
                    .rec
                    .log("states/body_rule", &StateChange::single(rule.as_str()))?;
                context.rec.log(
                    "states/body_case",
                    &StateChange::single(state.case.as_ref()),
                )?;
                if !state.repeat {
                    log.text("events/body", TextLogLevel::DEBUG, body(&event.kind))?;
                }
            }
            EventKind::Throw(throw) => log.throw(event.time_ns, throw, &event.kind)?,
            EventKind::GripRelease(release) => {
                let level = if release.cause.contains("lost") {
                    TextLogLevel::WARN
                } else {
                    TextLogLevel::INFO
                };
                log.text("events/grip", level, body(&event.kind))?;
                if log.grip_target[release.hand.index()] {
                    log.grip_target[release.hand.index()] = false;
                    context.rec.log(
                        format!("world/grip_targets/{}", hand_name(release.hand)),
                        &Clear::flat(),
                    )?;
                }
            }
            EventKind::WingsuitGesture(gesture) => {
                let lane = match (&gesture.phase, &gesture.why) {
                    (_, Some(why)) => format!("unavailable: {why}"),
                    (Some(phase), None) => phase.to_string(),
                    (None, None) => "idle".to_owned(),
                };
                context
                    .rec
                    .log("states/wingsuit", &StateChange::single(lane.as_str()))?;
                log.text("events/wingsuit", TextLogLevel::DEBUG, body(&event.kind))?;
            }
            EventKind::Snatch(snatch) => log.snatch(snatch, &event.kind)?,
            EventKind::RigConfig(config) => {
                let level = if config.repeat {
                    TextLogLevel::DEBUG
                } else {
                    TextLogLevel::INFO
                };
                log.text("events/rig", level, body(&event.kind))?;
            }
            EventKind::BodyIntro(intro) if !intro.repeat => {
                log.text("events/bodies", TextLogLevel::DEBUG, body(&event.kind))?
            }
            EventKind::Injected(_) => {
                log.text("events/injected", TextLogLevel::DEBUG, body(&event.kind))?
            }
            EventKind::Diagnostic(diagnostic) => log.text(
                &format!("events/refusals/{}", path_part(&diagnostic.source)),
                TextLogLevel::WARN,
                body(&event.kind),
            )?,
            EventKind::Haptic(_) => {
                log.text("events/haptics", TextLogLevel::INFO, body(&event.kind))?
            }
            EventKind::Settings(settings) => {
                context.rec.log(
                    "settings",
                    &TextDocument::from_markdown(format!(
                        "# Settings at {:.2} s\n\n```toml\n{}\n```\n",
                        event.time_ns as f64 / 1.0e9,
                        settings.toml.trim_end()
                    )),
                )?;
                log.text("events/settings", TextLogLevel::INFO, body(&event.kind))?;
            }
            EventKind::FrameTiming(timing) => {
                log.frame_timing(timing)?;
                log.text("events/timing", TextLogLevel::DEBUG, body(&event.kind))?;
            }
            EventKind::Pose(_)
            | EventKind::Rig(_)
            | EventKind::Bodies(_)
            | EventKind::BodyIntro(_)
            | EventKind::Held(_)
            | EventKind::Force(_) => {}
        }
    }
    Ok(log.entries)
}

const THROW: Color = Color(Rgba32::from_rgb(255, 80, 160));
const SNATCH: Color = Color(Rgba32::from_rgb(80, 220, 255));
const GRIP_TARGET: Color = Color(Rgba32::from_rgb(120, 255, 120));

/// An event's text line as `dump` prints it, less the time and the kind.
pub(crate) fn body(kind: &EventKind) -> String {
    let mut text = Vec::new();
    // Writing to a vector cannot fail.
    let _ = dump::write_event_body(kind, &mut text);
    String::from_utf8_lossy(&text).trim_end().to_owned()
}

struct Log<'a, 'c> {
    context: &'c Context<'a>,
    entries: usize,
    throws: usize,
    /// Each hand's snatch distance, from its arming or firing, while the snatch is out.
    snatch_distance: [Option<f32>; 2],
    /// Whether each hand's grip target is drawn.
    grip_target: [bool; 2],
}

impl Log<'_, '_> {
    fn text(&mut self, path: &str, level: &'static str, text: String) -> RecordingStreamResult<()> {
        self.entries += 1;
        self.context.rec.log(
            path,
            &TextLog::new(text).with_level(TextLogLevel::from(level)),
        )
    }

    /// The frames' moments: recenters, and changes of session state, focus, and action set.
    fn frames(&mut self) -> RecordingStreamResult<()> {
        let context = self.context;
        let mut session: Option<String> = None;
        let mut action_set: Option<String> = None;
        let mut focused: Option<bool> = None;
        for frame in context.frames {
            context.set_time(frame.time_ns);
            for recenter in &frame.recenters {
                let turn = body_rig::angle::yaw_of(recenter.turn * Vec3::NEG_Z)
                    .unwrap_or(0.0)
                    .to_degrees();
                let cause = match recenter.cause {
                    RecenterCause::Manual => "manual",
                    RecenterCause::SeatTransition => "seat transition",
                    RecenterCause::FollowBank => "follow bank",
                };
                let level = match recenter.cause {
                    RecenterCause::FollowBank => TextLogLevel::DEBUG,
                    RecenterCause::Manual | RecenterCause::SeatTransition => TextLogLevel::INFO,
                };
                self.text(
                    "events/recenter",
                    level,
                    format!(
                        "{cause} recenter ({:?}), the cockpit frame turned {turn:+.1} degrees",
                        recenter.kind
                    ),
                )?;
            }
            let state = format!(
                "{}{}",
                frame
                    .session
                    .state
                    .map_or("no session".to_owned(), |s| format!("{s:?}")),
                if frame.session.should_render {
                    ""
                } else {
                    ", not rendering"
                }
            );
            if session.as_deref() != Some(state.as_str()) {
                self.text(
                    "events/session",
                    TextLogLevel::DEBUG,
                    format!("session: {state}"),
                )?;
            }
            lane(context, "states/session", &mut session, state)?;
            let set = frame
                .controllers
                .as_ref()
                .map_or("no controllers".to_owned(), |c| {
                    format!("{:?}", c.action_set)
                });
            lane(context, "states/action_set", &mut action_set, set)?;
            let now = frame.controllers.as_ref().map(|c| c.focused);
            if let Some(now) = now
                && focused.is_some_and(|before| before != now)
            {
                let text = if now {
                    "input focus gained"
                } else {
                    "input focus lost"
                };
                self.text("events/session", TextLogLevel::INFO, text.to_owned())?;
            }
            focused = now.or(focused);
        }
        Ok(())
    }

    fn grip_acquisition(&mut self, grip: &GripAcquisition) -> RecordingStreamResult<()> {
        let hand = hand_name(grip.hand);
        let mut text = format!("{hand} grip {}:", grip.stage.name());
        let level = match grip.stage {
            GripStage::Input => {
                if grip.grip_pressed {
                    text.push_str(" pressed,");
                }
                if grip.grip_released {
                    text.push_str(" released,");
                }
                match &grip.chosen {
                    Some(chosen) => text.push_str(&format!(
                        " chose {} ({}) at {:.2} m of {} candidates,",
                        chosen.volume,
                        chosen.tier,
                        chosen.distance_m,
                        grip.candidates.len() + grip.candidates_omitted as usize
                    )),
                    None if grip.grip_pressed => text.push_str(" nothing in reach,"),
                    None => {}
                }
                text.push_str(&format!(" {}", grip.consumer));
                if grip.grab.is_some() || grip.grip_pressed {
                    TextLogLevel::INFO
                } else {
                    TextLogLevel::DEBUG
                }
            }
            GripStage::Request => {
                text.push_str(&format!(
                    " {} -> {}",
                    grip.target.as_deref().unwrap_or("no target"),
                    grip.result.as_deref().unwrap_or("no result")
                ));
                TextLogLevel::INFO
            }
            GripStage::Drive => {
                if let Some(drive) = &grip.drive {
                    text.push_str(&format!(
                        " body {} {}",
                        drive.body,
                        drive.result.as_deref().unwrap_or("driven")
                    ));
                }
                TextLogLevel::DEBUG
            }
        };
        if let Some(point) = grip.target_point {
            let label = match &grip.chosen {
                Some(chosen) => format!("{hand} grip target {}", chosen.volume),
                None => format!("{hand} grip target"),
            };
            self.context.rec.log(
                format!("world/grip_targets/{hand}"),
                &Points3D::new([self.context.world(point)])
                    .with_radii([0.03])
                    .with_colors([GRIP_TARGET])
                    .with_labels([label]),
            )?;
            self.grip_target[grip.hand.index()] = true;
        }
        self.text("events/grip", level, text)
    }

    /// The window's frame times and each slot's own time per frame, as plots.
    fn frame_timing(&mut self, timing: &FrameTiming) -> RecordingStreamResult<()> {
        let rec = self.context.rec;
        rec.log(
            "plots/timing/frame_ms",
            &Scalars::single(f64::from(timing.frame_ms)),
        )?;
        rec.log(
            "plots/timing/frame_max_ms",
            &Scalars::single(f64::from(timing.frame_max_ms)),
        )?;
        for slot in &timing.slots {
            if slot.self_ms.is_finite() {
                rec.log(
                    format!("plots/timing/slots/{}", path_part(&slot.name)),
                    &Scalars::single(f64::from(slot.self_ms)),
                )?;
            }
        }
        Ok(())
    }

    fn gun_release(&mut self, release: &GunRelease) -> RecordingStreamResult<()> {
        let hand = hand_name(release.hand);
        let (level, outcome) = match &release.refusal {
            Some(refusal) => (TextLogLevel::WARN, format!("refused: {refusal}")),
            None => (
                TextLogLevel::INFO,
                release
                    .drop
                    .as_ref()
                    .and_then(|d| d.requested_motion)
                    .map_or("dropped".to_owned(), |m| {
                        format!("dropped at {:.2} m/s", m.linear.length())
                    }),
            ),
        };
        self.text(
            "events/grip",
            level,
            format!(
                "{hand} gun release #{} through {}: {outcome}",
                release.attempt, release.route
            ),
        )
    }

    /// A throw's log entry, and its release point with its velocity in the world: where the throw
    /// says the item left, else the hand's drawn wrist at the throw, else the placed grip.
    fn throw(
        &mut self,
        time_ns: u64,
        throw: &Throw,
        kind: &EventKind,
    ) -> RecordingStreamResult<()> {
        self.text("events/throw", TextLogLevel::INFO, body(kind))?;
        let context = self.context;
        let release = throw
            .release
            .map(|release| release.position)
            .or_else(|| {
                context
                    .states
                    .at(time_ns)
                    .and_then(|state| state.late.hands[throw.hand.index()].latest_wrist)
            })
            .or_else(|| {
                let index = context.frame_at(time_ns)?;
                let placement = context.fit.placements[index]?;
                let hand = &context.frames[index].controllers.as_ref()?.hands[throw.hand.index()];
                Some(placement.apply(hand.grip.pose.position))
            });
        let Some(release) = release else {
            return Ok(());
        };
        self.throws += 1;
        let path = format!("world/throws/{:03}", self.throws);
        let label = format!("{} {}", hand_name(throw.hand), throw.item);
        let velocity = throw.motion.linear * throw.time_scale;
        context.rec.log(
            format!("{path}/release"),
            &Points3D::new([context.world(release)])
                .with_radii([0.04])
                .with_colors([THROW])
                .with_labels([label]),
        )?;
        context.rec.log(
            format!("{path}/velocity"),
            &Arrows3D::from_vectors([(velocity * ARROW_S).to_array()])
                .with_origins([context.world(release)])
                .with_colors([THROW]),
        )
    }

    /// A snatch's log entry and lane, and, while it is out, its line along the hand's aim to the
    /// target's distance; from version 8 the grapple's own line is drawn instead
    /// (`world/grapple_line`).
    fn snatch(&mut self, snatch: &Snatch, kind: &EventKind) -> RecordingStreamResult<()> {
        let level = match snatch.outcome.as_deref() {
            Some("failed") => TextLogLevel::WARN,
            _ => TextLogLevel::INFO,
        };
        self.text("events/snatch", level, body(kind))?;
        let Some(hand) = snatch.hand else {
            return Ok(());
        };
        let name = hand_name(hand);
        self.context.rec.log(
            format!("states/snatch/{name}"),
            &StateChange::single(snatch.to.as_ref()),
        )?;
        if self.context.recording.version >= GRAPPLE_LINE_VERSION {
            return Ok(());
        }
        let path = format!("world/cockpit/{name}/aim/snatch");
        let distance = &mut self.snatch_distance[hand.index()];
        if snatch.to == IDLE {
            *distance = None;
            return self.context.rec.log(path, &Clear::flat());
        }
        *distance = snatch.distance_m.or(*distance);
        match *distance {
            Some(distance) => self.context.rec.log(
                path,
                &Arrows3D::from_vectors([[0.0, 0.0, -distance]])
                    .with_colors([SNATCH])
                    .with_radii([0.008])
                    .with_labels([format!("snatch {}", snatch.to)]),
            ),
            None => Ok(()),
        }
    }
}

/// The snatch's resting phase.
const IDLE: &str = "IDLE";

/// The format version whose controller states record the grapple's line.
const GRAPPLE_LINE_VERSION: u16 = 8;
