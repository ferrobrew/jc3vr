//! A tap on the log for the scenario runner: while a scenario runs, every event at info level and
//! above is formatted as the log file shows its message and fields and queued for the runner's
//! log probes to match. Off, the tap costs one atomic load per event.
//!
//! The tap is a layer of the payload's subscriber (see `crate::logging`), filtered to info like the
//! file. Events from the test tools themselves are left out, so that the runner's own report of a
//! log probe does not match it.

use std::{
    collections::VecDeque,
    fmt::Write as _,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use parking_lot::Mutex;
use tracing::{Event, Level, Subscriber, field::Field};
use tracing_subscriber::{Layer, layer::Context};

/// One tapped event.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LogLine {
    pub(crate) level: Level,
    pub(crate) target: String,
    /// The message, then each field as ` name=value`.
    pub(crate) text: String,
}

/// The tap's subscriber layer.
pub(crate) struct TapLayer;

impl<S: Subscriber> Layer<S> for TapLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if !ACTIVE.load(Ordering::Relaxed) {
            return;
        }
        let metadata = event.metadata();
        if metadata.target().starts_with(OWN_TARGET) {
            return;
        }
        let mut visitor = Visitor::default();
        event.record(&mut visitor);
        let mut text = visitor.message;
        text.push_str(&visitor.fields);
        let line = LogLine {
            level: *metadata.level(),
            target: metadata.target().to_owned(),
            text,
        };
        let mut queue = QUEUE.lock();
        if queue.len() >= CAPACITY {
            queue.pop_front();
            OVERFLOWED.fetch_add(1, Ordering::Relaxed);
        }
        queue.push_back(line);
    }
}

/// Start tapping, with an empty queue.
pub(crate) fn start() {
    QUEUE.lock().clear();
    OVERFLOWED.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Stop tapping and drop what is queued.
pub(crate) fn stop() {
    ACTIVE.store(false, Ordering::Relaxed);
    QUEUE.lock().clear();
}

/// Take what was tapped since the last take, and how many lines the queue lost meanwhile.
pub(crate) fn drain() -> (Vec<LogLine>, u64) {
    let lines = QUEUE.lock().drain(..).collect();
    (lines, OVERFLOWED.swap(0, Ordering::Relaxed))
}

/// The target prefix of the test tools' own events.
const OWN_TARGET: &str = concat!(env!("CARGO_CRATE_NAME"), "::testing");

/// How many lines the queue holds between two takes. The runner takes every frame, so this is
/// only reached by a burst.
const CAPACITY: usize = 8192;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static OVERFLOWED: AtomicU64 = AtomicU64::new(0);
static QUEUE: Mutex<VecDeque<LogLine>> = Mutex::new(VecDeque::new());

/// Collects an event's message and its other fields.
#[derive(Default)]
struct Visitor {
    message: String,
    fields: String,
}

impl tracing::field::Visit for Visitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;

    #[test]
    fn an_event_is_tapped_with_its_fields_while_active() {
        let subscriber = tracing_subscriber::registry().with(TapLayer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "probe", "not yet");
            start();
            tracing::warn!(target: "probe", count = 3, "the game left on foot");
            tracing::info!(target: "jc3vr_payload::testing::scenario", "scenario: PASS");
            let (lines, lost) = drain();
            stop();
            tracing::info!(target: "probe", "after");
            assert_eq!(lost, 0);
            assert_eq!(
                lines,
                vec![LogLine {
                    level: Level::WARN,
                    target: "probe".to_owned(),
                    text: "the game left on foot count=3".to_owned(),
                }]
            );
            assert!(drain().0.is_empty());
        });
    }
}
