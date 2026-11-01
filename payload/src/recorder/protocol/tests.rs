use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use super::*;

#[test]
fn the_start_line_names_the_protocol_take_folder_and_window() {
    let window = WindowHint {
        title: Some("Just Cause 3".to_owned()),
        client_size: Some([1920, 1080]),
    };
    let bytes = start_line("2026-11-01_10-00-00_000", Some("/home/user/s/r"), &window);
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value,
        json!({
            "protocol": "jc3vr-capture",
            "version": 1,
            "type": "start",
            "take": "2026-11-01_10-00-00_000",
            "folder": "/home/user/s/r",
            "window": {"title": "Just Cause 3", "client_size": [1920, 1080]},
        })
    );
}

#[test]
fn a_take_without_a_host_folder_sends_null() {
    let value: Value =
        serde_json::from_slice(&start_line("t", None, &WindowHint::default())).unwrap();
    assert_eq!(value["folder"], Value::Null);
    assert_eq!(value["window"], json!({"title": null, "client_size": null}));
}

#[test]
fn the_stop_line_is_one_line() {
    assert_eq!(stop_line(), b"{\"type\":\"stop\"}\n");
}

#[test]
fn host_messages_parse_by_type_and_unknown_types_are_tolerated() {
    let started = parse_host_line(
        br#"{"type":"started","file":"/s/video.mkv","video_start_unix_ms":1793503069411.417,
            "region":{"x":4,"y":31,"width":1141,"height":701},"output":"DP-1"}"#,
    )
    .unwrap();
    assert_eq!(
        started,
        HostMessage::Started {
            file: "/s/video.mkv".to_owned(),
            video_start_unix_ms: Some(1793503069411.417),
            region: Region {
                x: 4,
                y: 31,
                width: 1141,
                height: 701
            },
            output: Some("DP-1".to_owned()),
        }
    );
    assert_eq!(
        parse_host_line(br#"{"type":"error","message":"capture: no window"}"#).unwrap(),
        HostMessage::Error {
            message: "capture: no window".to_owned()
        }
    );
    assert_eq!(
        parse_host_line(br#"{"type":"stopped","exit_code":0}"#).unwrap(),
        HostMessage::Stopped { exit_code: Some(0) }
    );
    assert_eq!(
        parse_host_line(br#"{"type":"progress","seconds":3}"#).unwrap(),
        HostMessage::Unknown
    );
}

#[test]
fn malformed_host_lines_are_errors() {
    for line in [&b"not json"[..], b"{}", br#"{"type":"error"}"#] {
        let error = parse_host_line(line).unwrap_err();
        assert!(error.to_string().starts_with("capture protocol: "));
    }
}

#[test]
fn lines_split_across_reads_are_joined_and_an_overlong_line_is_refused() {
    let mut buffer = LineBuffer::default();
    assert!(buffer.feed(b"{\"type\":").unwrap().is_empty());
    assert_eq!(
        buffer.feed(b"\"stop\"}\n{\"a\"").unwrap(),
        [b"{\"type\":\"stop\"}".to_vec()]
    );
    assert_eq!(buffer.feed(b":1}\n").unwrap(), [b"{\"a\":1}".to_vec()]);

    let mut buffer = LineBuffer::default();
    assert!(buffer.feed(&vec![b'x'; LINE_LIMIT]).unwrap().is_empty());
    assert!(matches!(buffer.feed(b"x"), Err(ProtocolError::TooLong)));
}
