//! The calibration flow's shared state: the running [`Calibration`], the stance choice, the
//! button and lifecycle handlers that drive it, and the rate-limited logging of its notices.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::{
    controllers::{
        body::{
            self,
            calibration::{Calibration, Stance, Step, Unavailable},
            menu::measure::{live_eye_height, live_grip_span},
        },
        menu::read_config,
        roomscale::{
            self,
            posture::{DetectedPosture, EffectivePosture},
        },
    },
    pause_menu::model::page::ButtonOutcome,
};

/// The calibration flow's state, shared by the page's rows.
pub(super) static CALIBRATION: Mutex<Calibration> = Mutex::new(Calibration::new());

/// The stance the player chose on the summary since the page opened, `None` for the posture's.
pub(super) static STANCE: Mutex<Option<Stance>> = Mutex::new(None);

pub(super) fn chosen_stance() -> Stance {
    STANCE.lock().unwrap_or_else(posture_stance)
}

/// The stance the posture setting implies, with the automatic posture's current detection.
pub(super) fn posture_stance() -> Stance {
    let detected = roomscale::frame_shared()
        .filter(|f| f.fresh(Instant::now()))
        .map(|f| match f.posture {
            EffectivePosture::Seated => DetectedPosture::Seated,
            EffectivePosture::Standing | EffectivePosture::Roomscale => DetectedPosture::Standing,
        });
    Stance::for_posture(read_config(|c| c.controllers.roomscale.posture), detected)
}

pub(super) fn stance_index(stance: Stance) -> usize {
    Stance::ALL
        .iter()
        .position(|s| *s == stance)
        .unwrap_or_default()
}

/// Start a capture with the live measurement. The status readout shows the countdown, or why the
/// capture was refused.
pub(super) fn measure(live: Result<f32, Unavailable>) -> ButtonOutcome {
    let mut calibration = CALIBRATION.lock();
    calibration.measure(Instant::now(), live);
    log_notice(&calibration);
    ButtonOutcome::Stay
}

/// Save the reviewed dimensions.
pub(super) fn save() -> ButtonOutcome {
    let result = CALIBRATION.lock().result();
    let Some(Ok(dimensions)) = result else {
        return ButtonOutcome::Stay;
    };
    match body::save(dimensions) {
        Ok(()) => {
            CALIBRATION.lock().saved();
            ButtonOutcome::Rebuild
        }
        Err(error) => {
            let mut calibration = CALIBRATION.lock();
            calibration.save_failed(format!("body dimensions: {error}"));
            log_notice(&calibration);
            ButtonOutcome::Stay
        }
    }
}

/// Feed the live measurement to a running capture; rebuild when it moved the flow on.
pub(super) fn on_frame() -> ButtonOutcome {
    let step = CALIBRATION.lock().step();
    let live = match step {
        Step::EyeHeight => live_eye_height(),
        Step::Span => live_grip_span(),
        Step::Summary | Step::Review => return ButtonOutcome::Stay,
    };
    let mut calibration = CALIBRATION.lock();
    let changed = calibration.tick(Instant::now(), live);
    log_notice(&calibration);
    rebuild_if(changed)
}

/// Leaving the page abandons a calibration in progress and returns the stance choice to the
/// posture's.
pub(super) fn on_close() {
    CALIBRATION.lock().cancel();
    *STANCE.lock() = None;
}

pub(super) fn rebuild_if(changed: bool) -> ButtonOutcome {
    if changed {
        ButtonOutcome::Rebuild
    } else {
        ButtonOutcome::Stay
    }
}

/// Log the flow's notice when it is a new one, at most once per [`NOTICE_LOG_INTERVAL`].
pub(super) fn log_notice(calibration: &Calibration) {
    let Some(notice) = calibration.notice() else {
        return;
    };
    let suppressed = NOTICE_LOG
        .lock()
        .take(calibration.notice_count(), Instant::now());
    match suppressed {
        Some(0) => tracing::warn!(target: "controllers", "{notice}"),
        Some(suppressed) => {
            tracing::warn!(target: "controllers", "{notice} ({suppressed} earlier not logged)")
        }
        None => {}
    }
}

/// The shortest time between two logged notices.
pub(super) const NOTICE_LOG_INTERVAL: Duration = Duration::from_secs(1);

pub(super) static NOTICE_LOG: Mutex<NoticeLog> = Mutex::new(NoticeLog::new());

/// Which notices have been logged, for [`log_notice`]'s rate limit.
#[derive(Debug)]
pub(super) struct NoticeLog {
    /// The notice count last seen.
    seen: u32,
    /// When a notice was last logged.
    logged_at: Option<Instant>,
    /// New notices not logged since then.
    suppressed: u32,
}

impl NoticeLog {
    pub(super) const fn new() -> Self {
        Self {
            seen: 0,
            logged_at: None,
            suppressed: 0,
        }
    }

    /// Given the flow's notice count at `now`, whether to log its notice: `Some` with the number
    /// of notices skipped since the last one logged, or `None` when there is no new notice or the
    /// last was logged too recently.
    pub(super) fn take(&mut self, count: u32, now: Instant) -> Option<u32> {
        let new = count.wrapping_sub(self.seen);
        self.seen = count;
        if new == 0 {
            return None;
        }
        let recent = self
            .logged_at
            .is_some_and(|at| now.saturating_duration_since(at) < NOTICE_LOG_INTERVAL);
        if recent {
            self.suppressed = self.suppressed.saturating_add(new);
            return None;
        }
        self.logged_at = Some(now);
        Some(std::mem::take(&mut self.suppressed).saturating_add(new - 1))
    }
}
