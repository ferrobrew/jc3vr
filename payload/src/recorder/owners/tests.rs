use std::{
    sync::mpsc::{self, SyncSender},
    time::{Duration, Instant},
};

use super::*;

#[test]
fn rapid_stop_start_retains_all_blocked_writers_through_shutdown() {
    let mut owners = VideoWriters::new();
    let first = blocked();
    owners.start("first".into(), || Ok(first.0)).1.unwrap();
    owners.stop();
    let second = blocked();
    owners.start("second".into(), || Ok(second.0)).1.unwrap();
    owners.stop();
    owners.stop();
    assert_eq!(owners.finishing.len(), MAX_WRITERS);
    assert!(owners.reap().is_empty());
    let error = owners
        .start("refused".into(), || panic!("saturation must precede spawn"))
        .1
        .unwrap_err();
    assert!(error.to_string().contains("still finishing"));

    second.1.send(()).unwrap();
    await_termination(&owners.finishing[1].writer);
    let third = blocked();
    let (completions, result) = owners.start("third".into(), || Ok(third.0));
    result.unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].folder, "second");
    assert!(completions[0].error.is_none());
    assert_eq!(owners.finishing.len(), 1);
    assert!(owners.reap().is_empty());

    let result = owners.shutdown(Duration::ZERO);
    assert!(!result.stopped);
    assert!(result.completions.is_empty());
    assert_eq!(owners.finishing.len(), MAX_WRITERS);
    assert!(owners.active().is_none());
    assert!(
        owners
            .start("closed".into(), || panic!("shutdown must precede spawn"))
            .1
            .is_err()
    );

    first.1.send(()).unwrap();
    let completions = await_completions(&mut owners, 1);
    assert_eq!(completions[0].folder, "first");
    assert!(!owners.shutdown(Duration::from_millis(1)).stopped);
    assert_eq!(owners.finishing.len(), 1);
    third.1.send(()).unwrap();
    let result = owners.shutdown(Duration::from_secs(5));
    assert!(result.stopped);
    assert_eq!(result.completions.len(), 1);
    assert_eq!(result.completions[0].folder, "third");
    assert!(owners.reap().is_empty());
    assert!(owners.shutdown(Duration::ZERO).completions.is_empty());
}

#[test]
fn failure_and_panic_are_reported_after_join_and_release_capacity() {
    let mut owners = VideoWriters::new();
    owners
        .start("failed".into(), || {
            Ok(Writer::spawn_controlled(|_| {
                Err("recorder: controlled write failure".into())
            }))
        })
        .1
        .unwrap();
    owners.stop();
    let (mut completions, result) = owners.start("panicked".into(), || {
        Ok(Writer::spawn_controlled(|_| {
            panic!("controlled writer panic")
        }))
    });
    result.unwrap();
    owners.stop();
    let remaining = 2 - completions.len();
    completions.extend(await_completions(&mut owners, remaining));
    completions.sort_by(|a, b| a.folder.cmp(&b.folder));
    assert_eq!(completions[0].folder, "failed");
    assert_eq!(
        completions[0].error.as_deref(),
        Some("recorder: controlled write failure")
    );
    assert_eq!(completions[1].folder, "panicked");
    assert!(completions[1].error.as_ref().unwrap().contains("panicked"));
    assert!(owners.reap().is_empty());
    assert!(owners.finishing.is_empty());
    owners
        .start("recovered".into(), || {
            Ok(Writer::spawn_controlled(|_| Ok(())))
        })
        .1
        .unwrap();
    assert!(owners.shutdown(Duration::from_secs(5)).stopped);
}

#[test]
fn spawn_failure_does_not_consume_capacity() {
    let mut owners = VideoWriters::new();
    let error = owners
        .start("failed".into(), || {
            anyhow::bail!("recorder: controlled spawn failure")
        })
        .1
        .unwrap_err();
    assert!(error.to_string().contains("spawn failure"));
    assert!(owners.active().is_none());
    assert!(owners.finishing.is_empty());
    assert!(owners.shutdown(Duration::ZERO).stopped);
}

fn blocked() -> (Writer, SyncSender<()>) {
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let writer = Writer::spawn_controlled(move |_| {
        entered.send(()).unwrap();
        let _ = wait.recv();
        Ok(())
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    (writer, release)
}

fn await_termination(writer: &Writer) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !writer.is_finished() {
        assert!(Instant::now() < deadline, "video writer did not exit");
        std::thread::yield_now();
    }
}

fn await_completions(owners: &mut VideoWriters, count: usize) -> Vec<Completion> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut completions = Vec::new();
    while completions.len() < count {
        completions.extend(owners.reap());
        assert!(Instant::now() < deadline, "video writers did not exit");
        std::thread::yield_now();
    }
    assert_eq!(completions.len(), count);
    completions
}
