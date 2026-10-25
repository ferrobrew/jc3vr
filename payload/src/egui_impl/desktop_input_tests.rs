use super::*;
use crate::{
    egui_impl::EguiCaptureAdapter,
    ui::capture::{InputObservation, PanelRequest},
};

fn native(keys: &[usize]) -> Result<NativeSnapshot, DesktopInputUnavailable> {
    let mut held = Keys::default();
    for key in keys {
        held.insert(*key);
    }
    Ok(NativeSnapshot {
        held,
        composition_active: false,
    })
}

fn tracker() -> DesktopInputTracker {
    let mut tracker = DesktopInputTracker::new();
    tracker.message(1, WM_SETFOCUS, 0);
    assert_eq!(
        tracker.sample(native(&[])),
        DesktopInputObservation::Unavailable(DesktopInputUnavailable::BaselinePending)
    );
    tracker
}

fn evidence(tracker: &mut DesktopInputTracker, keys: &[usize]) -> DesktopInputEvidence {
    match tracker.sample(native(keys)) {
        DesktopInputObservation::Observed(evidence) => evidence,
        unavailable => panic!("expected observed native input, got {unavailable:?}"),
    }
}

#[test]
fn physical_keys_modifiers_and_every_mouse_button_block_without_egui_events() {
    for key in [
        0x41, 0x77, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0x5b, 0x5c, 0x01, 0x02, 0x04, 0x05, 0x06,
    ] {
        let mut tracker = tracker();
        assert!(!evidence(&mut tracker, &[key]).controls_neutral());
        assert!(evidence(&mut tracker, &[]).controls_neutral());
    }
    let mut tracker = tracker();
    let held = evidence(&mut tracker, &[0xa0, 0xa3, 0xa4, 0x5b]);
    assert_eq!(
        held.modifiers,
        DesktopModifiers {
            shift: true,
            control: true,
            alt: true,
            windows: true
        }
    );
}

#[test]
fn raw_release_and_consumed_shortcut_require_a_later_physical_release_sample() {
    let mut tracker = tracker();
    tracker.message(1, WM_KEYDOWN, 0x77);
    tracker.consume_shortcut(0x77);
    assert!(!evidence(&mut tracker, &[0x77]).controls_neutral());
    assert!(tracker.message(1, WM_KEYUP, 0x77));
    let released = evidence(&mut tracker, &[]);
    assert!(released.key_activity);
    assert_eq!(released.consumed_shortcuts, 1);
    assert!(!released.controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
}

#[test]
fn consumed_shortcut_release_remains_consumed_after_native_neutral_precedes_its_message() {
    let mut tracker = tracker();
    tracker.message(1, WM_KEYDOWN, 0x77);
    tracker.consume_shortcut(0x77);
    assert!(!evidence(&mut tracker, &[0x77]).controls_neutral());
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
    assert!(tracker.message(1, WM_KEYUP, 0x77));
    assert!(!tracker.message(1, WM_KEYUP, 0x77));
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
}

#[test]
fn consumed_control_multiply_missing_release_does_not_own_a_later_plain_episode() {
    let mut tracker = tracker();
    let control = 0xa2;
    let multiply = 0x6a;
    assert!(!tracker.message(1, WM_KEYDOWN, control));
    assert!(!tracker.message_with_lparam(1, WM_KEYDOWN, multiply, 0));
    tracker.consume_shortcut(multiply);
    assert!(!evidence(&mut tracker, &[control, multiply]).controls_neutral());
    tracker.message(1, WM_ACTIVATEAPP, 0);
    tracker.message(1, WM_KILLFOCUS, 0);
    tracker.message(1, WM_ACTIVATEAPP, 1);
    tracker.message(1, WM_SETFOCUS, 0);
    assert_eq!(tracker.sample(native(&[])).controls_neutral(), None);
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
    assert!(tracker.release_to_consume.contains(multiply));
    assert!(!tracker.message_with_lparam(1, WM_KEYDOWN, multiply, 0));
    assert!(!tracker.release_to_consume.contains(multiply));
    assert!(!tracker.message_with_lparam(1, WM_KEYUP, multiply, 0xc000_0000));
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
    assert!(!tracker.message_with_lparam(1, WM_KEYDOWN, multiply, 0));
    tracker.consume_shortcut(multiply);
    assert!(tracker.message_with_lparam(1, WM_KEYUP, multiply, 0xc000_0000));
}

#[test]
fn repeat_without_a_new_first_press_does_not_supersede_a_consumed_release() {
    let mut tracker = tracker();
    tracker.message(1, WM_KEYDOWN, 0x6a);
    tracker.consume_shortcut(0x6a);
    evidence(&mut tracker, &[]);
    assert!(!tracker.message_with_lparam(1, WM_KEYDOWN, 0x6a, 0x4000_0000));
    assert!(tracker.message_with_lparam(1, WM_KEYUP, 0x6a, 0xc000_0000));
}

#[test]
fn wheel_text_clipboard_and_ime_are_not_an_empty_egui_queue_proof() {
    for message in [
        WM_MOUSEWHEEL,
        WM_MOUSEHWHEEL,
        WM_CHAR,
        WM_UNICHAR,
        WM_PASTE,
        WM_CUT,
        WM_DEADCHAR,
    ] {
        let mut tracker = tracker();
        tracker.message(1, message, 0);
        let sample = evidence(&mut tracker, &[]);
        assert!(sample.scroll_activity || sample.text_activity);
        assert!(!sample.controls_neutral());
        assert!(evidence(&mut tracker, &[]).controls_neutral());
    }
    let mut tracker = tracker();
    tracker.message(1, WM_IME_STARTCOMPOSITION, 0);
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    tracker.message(1, WM_IME_ENDCOMPOSITION, 0);
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
}

#[test]
fn focus_loss_regain_between_samples_invalidates_baseline_and_retains_loss_generation() {
    let mut tracker = tracker();
    let generation = tracker.focus_loss_generation;
    tracker.message(1, WM_ACTIVATEAPP, 0);
    tracker.message(1, WM_ACTIVATEAPP, 1);
    assert_ne!(tracker.focus_loss_generation, generation);
    assert_eq!(
        tracker.sample(native(&[0x41])),
        DesktopInputObservation::Unavailable(DesktopInputUnavailable::BaselinePending)
    );
    assert!(!evidence(&mut tracker, &[0x41]).controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
    assert_eq!(
        tracker
            .sample(Err(DesktopInputUnavailable::Unfocused))
            .focused(),
        Some(false)
    );
    assert_eq!(tracker.sample(native(&[])).controls_neutral(), None);
}

#[test]
fn missing_keyboard_ime_or_thread_evidence_cannot_report_neutral() {
    for reason in [
        DesktopInputUnavailable::KeyboardStateUnavailable,
        DesktopInputUnavailable::ImeStateUnavailable,
        DesktopInputUnavailable::WrongThread,
        DesktopInputUnavailable::WindowNotObserved,
    ] {
        let mut tracker = tracker();
        assert_eq!(tracker.sample(Err(reason)).controls_neutral(), None);
        assert_eq!(tracker.sample(native(&[])).controls_neutral(), None);
        assert!(evidence(&mut tracker, &[]).controls_neutral());
    }
    let mut tracker = tracker();
    let composing = Ok(NativeSnapshot {
        held: Keys::default(),
        composition_active: true,
    });
    assert!(
        !tracker
            .sample(composing)
            .controls_neutral()
            .expect("observed composition")
    );
}

#[test]
fn composition_start_before_baseline_cannot_be_erased_by_an_empty_native_string() {
    let mut tracker = DesktopInputTracker::new();
    tracker.message(1, WM_SETFOCUS, 0);
    tracker.message(1, WM_IME_STARTCOMPOSITION, 0);
    assert_eq!(tracker.sample(native(&[])).controls_neutral(), None);
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    tracker.message(1, WM_IME_ENDCOMPOSITION, 0);
    assert!(!evidence(&mut tracker, &[]).controls_neutral());
    assert!(evidence(&mut tracker, &[]).controls_neutral());
}

#[test]
fn native_composition_distinguishes_no_data_from_query_or_release_failure() {
    assert_eq!(composition_from_native(IMM_ERROR_NODATA, true), Ok(false));
    assert_eq!(composition_from_native(0, true), Ok(false));
    assert_eq!(composition_from_native(2, true), Ok(true));
    assert_eq!(
        composition_from_native(-2, true),
        Err(DesktopInputUnavailable::ImeStateUnavailable)
    );
    assert_eq!(
        composition_from_native(0, false),
        Err(DesktopInputUnavailable::ImeStateUnavailable)
    );
}

#[test]
fn raw_mouse_and_key_activity_blocks_even_when_the_native_sample_has_already_released() {
    for (down, up, key, wparam) in [
        (WM_KEYDOWN, WM_KEYUP, 0x41, 0x41),
        (WM_LBUTTONDOWN, WM_LBUTTONUP, 0x01, 0),
        (WM_RBUTTONDOWN, WM_RBUTTONUP, 0x02, 0),
        (WM_MBUTTONDOWN, WM_MBUTTONUP, 0x04, 0),
        (WM_XBUTTONDOWN, WM_XBUTTONUP, 0x05, 1 << 16),
        (WM_XBUTTONDOWN, WM_XBUTTONUP, 0x06, 2 << 16),
    ] {
        let mut tracker = tracker();
        tracker.message(1, down, wparam);
        assert!(!evidence(&mut tracker, &[key]).controls_neutral());
        tracker.message(1, up, wparam);
        assert!(!evidence(&mut tracker, &[]).controls_neutral());
        assert!(evidence(&mut tracker, &[]).controls_neutral());
    }
}

#[test]
fn physical_hold_survives_egui_hide_reopen_and_device_acknowledgement() {
    let context = egui::Context::default();
    let mut adapter = EguiCaptureAdapter::default();
    let mut events = Vec::new();
    let mut desktop = tracker();
    let held = !evidence(&mut desktop, &[0x41, 0x01]).controls_neutral();
    assert!(held);
    let input = InputObservation {
        focused: true,
        tracking_ready: true,
        controls_neutral: false,
        game_device_enabled: true,
    };
    adapter.update(Some(PanelRequest::Open), input, &context, &mut events);
    adapter.update(
        Some(PanelRequest::Hide),
        InputObservation {
            game_device_enabled: false,
            ..input
        },
        &context,
        &mut events,
    );
    adapter.update(Some(PanelRequest::Open), input, &context, &mut events);
    let update = adapter.update(
        None,
        InputObservation {
            controls_neutral: evidence(&mut desktop, &[0x41, 0x01]).controls_neutral(),
            game_device_enabled: false,
            ..input
        },
        &context,
        &mut events,
    );
    assert!(!update.snapshot.edit_capture.any());
    assert!(!context.input(|input| input.pointer.any_down()));
    assert!(!evidence(&mut desktop, &[0x41, 0x01]).controls_neutral());
    let update = adapter.update(
        None,
        InputObservation {
            controls_neutral: evidence(&mut desktop, &[]).controls_neutral(),
            game_device_enabled: false,
            ..input
        },
        &context,
        &mut events,
    );
    assert!(update.snapshot.edit_capture.any());
}
