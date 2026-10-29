//! Printing a recording's frames and events, in time order, as human-readable text or as JSON
//! lines.
//!
//! Events are stamped on the frames' clock and a writer stores them before the frame captured
//! after them, so an event prints before the first frame at or after its time. Input records are
//! counted in the header line, not printed.

use std::io::{self, Write};

use glam::Vec3;
use serde_json::{Value, json};
use tracking_format::{
    Frame, Recording,
    event::{Event, EventKind},
    model::{Button, Hand, HandFrame},
};

/// How [`dump`] prints.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DumpFormat {
    /// One line per record, for reading.
    Text,
    /// One JSON object per line: a `header`, then each `frame` and `event`.
    JsonLines,
}

/// Print `recording` to `out`.
pub fn dump(recording: &Recording, format: DumpFormat, out: &mut impl Write) -> io::Result<()> {
    match format {
        DumpFormat::Text => text_header(recording, out)?,
        DumpFormat::JsonLines => json_line(out, &json_header(recording))?,
    }
    for record in records(recording) {
        match (record, format) {
            (Record::Frame(frame), DumpFormat::Text) => text_frame(frame, out)?,
            (Record::Event(event), DumpFormat::Text) => text_event(event, out)?,
            (Record::Frame(frame), DumpFormat::JsonLines) => {
                let mut value = serde_json::to_value(frame).map_err(io::Error::other)?;
                value["type"] = json!("frame");
                json_line(out, &value)?;
            }
            (Record::Event(event), DumpFormat::JsonLines) => {
                json_line(out, &json_event(event).map_err(io::Error::other)?)?;
            }
        }
    }
    Ok(())
}

/// One printed record.
#[derive(Debug, Copy, Clone, PartialEq)]
enum Record<'a> {
    Frame(&'a Frame),
    Event(&'a Event),
}

/// The frames and events in time order: each event before the first frame at or after its time.
fn records(recording: &Recording) -> impl Iterator<Item = Record<'_>> {
    let mut events = recording.events.iter().peekable();
    let mut frames = recording.frames.iter().peekable();
    std::iter::from_fn(move || match (frames.peek(), events.peek()) {
        (Some(frame), Some(event)) if event.time_ns <= frame.time_ns => {
            events.next().map(Record::Event)
        }
        (Some(_), _) => frames.next().map(Record::Frame),
        (None, Some(_)) => events.next().map(Record::Event),
        (None, None) => None,
    })
}

fn json_line(out: &mut impl Write, value: &Value) -> io::Result<()> {
    // Serialized first, so a write error keeps its kind.
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    out.write_all(&line)
}

fn json_header(recording: &Recording) -> Value {
    json!({
        "type": "header",
        "version": recording.version,
        "truncated": recording.truncated,
        "skipped_records": recording.skipped_records,
        "frames": recording.frames.len(),
        "events": recording.events.len(),
        "input_records": recording.input.len(),
        "header": recording.header,
    })
}

fn json_event(event: &Event) -> serde_json::Result<Value> {
    let data = match &event.kind {
        EventKind::ControllerState(state) => serde_json::to_value(state)?,
        EventKind::GripAcquisition(grip) => serde_json::to_value(grip)?,
        EventKind::GunRelease(release) => serde_json::to_value(release)?,
    };
    Ok(json!({
        "type": "event",
        "time_ns": event.time_ns,
        "kind": event.kind.name(),
        "data": data,
    }))
}

fn text_header(recording: &Recording, out: &mut impl Write) -> io::Result<()> {
    let header = &recording.header;
    writeln!(
        out,
        "recording: version {}, {} frames, {} events, {} input records{}{}",
        recording.version,
        recording.frames.len(),
        recording.events.len(),
        recording.input.len(),
        if recording.truncated {
            ", cut short"
        } else {
            ""
        },
        if recording.skipped_records > 0 {
            format!(", {} records skipped", recording.skipped_records)
        } else {
            String::new()
        },
    )?;
    writeln!(
        out,
        "build {:?}, recorded {:?}, action schema {}, {:?} hand dominant, world scale {}, time scale {}",
        header.build,
        header.recorded_at,
        header.action_schema,
        header.dominant_hand,
        header.world_scale,
        header.time_scale,
    )?;
    writeln!(
        out,
        "start {}{}",
        vec3(header.start.position()),
        header
            .start
            .bookmark
            .as_ref()
            .map_or(String::new(), |name| format!(" at the bookmark {name:?}")),
    )
}

fn text_frame(frame: &Frame, out: &mut impl Write) -> io::Result<()> {
    write!(out, "{} frame", seconds(frame.time_ns))?;
    match &frame.head {
        Some(head) => write!(
            out,
            " head {}",
            vec3(
                head.eyes[0]
                    .pose
                    .position
                    .lerp(head.eyes[1].pose.position, 0.5)
            )
        )?,
        None => write!(out, " head -")?,
    }
    if !frame.session.head_tracked {
        write!(out, " (untracked)")?;
    }
    if let Some(controllers) = &frame.controllers {
        write!(out, " {:?}", controllers.action_set)?;
        for hand in Hand::BOTH {
            write!(out, " | ")?;
            text_hand(hand, &controllers.hands[hand.index()], out)?;
        }
    }
    for recenter in &frame.recenters {
        write!(
            out,
            " | recenter {:?} by {:?}",
            recenter.kind, recenter.cause
        )?;
    }
    writeln!(out)
}

fn text_hand(hand: Hand, frame: &HandFrame, out: &mut impl Write) -> io::Result<()> {
    let name = match hand {
        Hand::Left => "L",
        Hand::Right => "R",
    };
    if !frame.active {
        return write!(out, "{name} inactive");
    }
    let buttons: Vec<String> = Button::ALL
        .into_iter()
        .filter(|button| frame.buttons.contains(*button))
        .map(|button| format!("{button:?}"))
        .collect();
    write!(
        out,
        "{name} grip {}{} trigger {:.2} squeeze {:.2} stick ({:+.2}, {:+.2}) [{}]",
        vec3(frame.grip.pose.position),
        if frame.grip.position_tracked {
            ""
        } else {
            " (untracked)"
        },
        frame.inputs.trigger,
        frame.inputs.squeeze,
        frame.inputs.thumbstick.x,
        frame.inputs.thumbstick.y,
        buttons.join(" "),
    )
}

fn text_event(event: &Event, out: &mut impl Write) -> io::Result<()> {
    write!(
        out,
        "{} event {}: ",
        seconds(event.time_ns),
        event.kind.name()
    )?;
    match &event.kind {
        EventKind::ControllerState(state) => writeln!(out, "{state:?}"),
        EventKind::GripAcquisition(grip) => writeln!(out, "{grip:?}"),
        EventKind::GunRelease(release) => writeln!(out, "{release:?}"),
    }
}

fn seconds(time_ns: u64) -> String {
    format!("{:>12.6}", time_ns as f64 * 1e-9)
}

fn vec3(v: Vec3) -> String {
    format!("({:+.3}, {:+.3}, {:+.3})", v.x, v.y, v.z)
}

#[cfg(test)]
mod tests {
    use tracking_format::{
        Header, SessionSample, StartMarker,
        event::{GunRelease, Label},
        model::Stage,
    };

    use super::*;

    fn frame(time_ns: u64) -> Frame {
        Frame {
            time_ns,
            display_time_ns: 0,
            session: SessionSample {
                state: None,
                should_render: true,
                head_tracked: true,
            },
            head: None,
            stage: Stage::default(),
            controllers: None,
            semantic: None,
            recenters: Vec::new(),
        }
    }

    fn event(time_ns: u64) -> Event {
        Event {
            time_ns,
            kind: EventKind::GunRelease(Box::new(GunRelease {
                hand: Hand::Left,
                attempt: time_ns,
                weapon: None,
                route: Label::Borrowed("Wielded"),
                max_speed_mps: 1.0,
                refusal: None,
                before_disarm: true,
                disarm_returned: true,
                slot_clear_returned: true,
                reconcile_returned: true,
                controller: None,
                drop: None,
            })),
        }
    }

    fn recording() -> Recording {
        Recording {
            header: Header {
                build: "fixture".into(),
                recorded_at: "fixture".into(),
                action_schema: 1,
                start: StartMarker {
                    transform: [0.0; 16],
                    bookmark: None,
                },
                dominant_hand: Hand::Right,
                world_scale: 1.0,
                time_scale: 1.0,
            },
            frames: vec![frame(10), frame(20)],
            input: Vec::new(),
            events: vec![event(5), event(10), event(15), event(30)],
            version: tracking_format::VERSION,
            truncated: false,
            skipped_records: 0,
        }
    }

    #[test]
    fn events_print_before_the_first_frame_at_or_after_them() {
        let recording = recording();
        let order: Vec<(char, u64)> = records(&recording)
            .map(|record| match record {
                Record::Frame(frame) => ('f', frame.time_ns),
                Record::Event(event) => ('e', event.time_ns),
            })
            .collect();
        assert_eq!(
            order,
            [
                ('e', 5),
                ('e', 10),
                ('f', 10),
                ('e', 15),
                ('f', 20),
                ('e', 30)
            ]
        );
    }

    #[test]
    fn json_lines_are_one_object_each_with_a_type() {
        let mut out = Vec::new();
        dump(&recording(), DumpFormat::JsonLines, &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0]["type"], "header");
        assert_eq!(lines[0]["events"], 4);
        assert_eq!(lines[1]["type"], "event");
        assert_eq!(lines[1]["kind"], "gun_release");
        assert_eq!(lines[1]["data"]["route"], "Wielded");
        assert_eq!(lines[3]["type"], "frame");
        assert_eq!(lines[3]["time_ns"], 10);
    }

    #[test]
    fn text_prints_a_line_per_record_after_the_header() {
        let mut out = Vec::new();
        dump(&recording(), DumpFormat::Text, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 3 + 6);
        assert!(text.lines().nth(3).unwrap().contains("event gun_release"));
        assert!(text.lines().nth(5).unwrap().contains("frame head -"));
    }
}
