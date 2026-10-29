use super::*;

#[test]
fn write_status_updates_only_matching_path_and_preserves_success_on_failure() {
    let mut status = RestoreStatus {
        path: Some(PathBuf::from("current.toml")),
        writes: 2,
        last_written: 3,
        ..RestoreStatus::default()
    };
    apply_write_status(
        &mut status,
        sidecar::WriteStatus {
            sequence: 4,
            path: Some(PathBuf::from("other.toml")),
            writes: 9,
            last_written: 9,
            error: Some("inventory: sidecar: older path failure".into()),
        },
    );
    assert_eq!(status.writes, 2);
    assert_eq!(status.last_written, 3);
    assert!(status.last_write_error.is_none());
    apply_write_status(
        &mut status,
        sidecar::WriteStatus {
            sequence: 5,
            path: Some(PathBuf::from("current.toml")),
            writes: 2,
            last_written: 3,
            error: Some("inventory: sidecar: newest attempt failed".into()),
        },
    );
    assert_eq!(status.writes, 2);
    assert_eq!(status.last_written, 3);
    assert!(
        status
            .last_write_error
            .unwrap()
            .contains("newest attempt failed")
    );
}
