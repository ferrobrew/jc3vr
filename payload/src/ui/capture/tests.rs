use super::*;

fn ready(neutral: bool, enabled: bool) -> InputObservation {
    InputObservation {
        focused: true,
        tracking_ready: true,
        controls_neutral: neutral,
        game_device_enabled: enabled,
    }
}

#[test]
fn read_only_overlay_has_no_edit_capture() {
    let mut policy = CapturePolicy::default();
    policy.update(Some(PanelRequest::Open), ready(true, true));
    assert!(
        policy
            .update(None, ready(true, false))
            .snapshot
            .edit_capture
            .any()
    );
    let update = policy.update(Some(PanelRequest::AllowGameplay), ready(false, false));
    assert_eq!(update.snapshot.mode, PanelMode::ReadOnly);
    assert!(update.snapshot.visible);
    assert_eq!(update.snapshot.edit_capture, EditCapture::default());
    assert!(!update.snapshot.gameplay_effective);
    assert_eq!(update.effects.game_device, DeviceEffect::SetEnabled(true));
    assert!(update.effects.clear_pending_events);
    assert!(update.effects.cancel_pointer_and_drag);
    let update = policy.update(None, ready(true, true));
    assert!(update.snapshot.gameplay_effective);
    assert_eq!(update.snapshot.edit_capture, EditCapture::default());
    assert_eq!(
        policy
            .update(Some(PanelRequest::Open), ready(true, true))
            .snapshot
            .mode,
        PanelMode::ReadOnly,
    );
}

#[test]
fn hide_reopen_returns_editing_without_held_input_leak() {
    let mut policy = CapturePolicy::default();
    policy.update(Some(PanelRequest::Open), ready(true, true));
    policy.update(None, ready(true, false));
    policy.update(Some(PanelRequest::AllowGameplay), ready(false, false));
    let hidden = policy.update(Some(PanelRequest::Hide), ready(false, true));
    assert_eq!(hidden.snapshot.mode, PanelMode::Hidden);
    assert!(!hidden.snapshot.gameplay_effective);
    assert!(hidden.effects.consume_until_neutral);
    let reopened = policy.update(Some(PanelRequest::Open), ready(false, true));
    assert_eq!(reopened.snapshot.mode, PanelMode::Editing);
    assert!(!reopened.snapshot.edit_capture.any());
    assert!(!reopened.snapshot.gameplay_effective);
    assert!(reopened.effects.cancel_pointer_and_drag);
    assert!(
        !policy
            .update(None, ready(false, false))
            .snapshot
            .edit_capture
            .any()
    );
    assert!(
        policy
            .update(None, ready(true, false))
            .snapshot
            .edit_capture
            .any()
    );
}

#[test]
fn prior_disabled_device_not_enabled() {
    for exit in [PanelRequest::Hide, PanelRequest::AllowGameplay] {
        let mut policy = CapturePolicy::default();
        let acquired = policy.update(Some(PanelRequest::Open), ready(true, false));
        assert_eq!(acquired.snapshot.prior_game_device_enabled, Some(false));
        let released = policy.update(Some(exit), ready(true, false));
        assert_eq!(
            released.effects.game_device,
            DeviceEffect::SetEnabled(false)
        );
        assert!(!released.snapshot.intended_game_device_enabled);
        let neutral = policy.update(None, ready(true, false));
        assert!(!neutral.snapshot.gameplay_effective);
        assert_eq!(
            neutral.snapshot.blocker,
            Some(OwnershipBlocker::GameDeviceDisabled)
        );
    }
}

#[test]
fn focus_and_tracking_recovery_require_real_neutral() {
    for (focused, tracking_ready, blocker) in [
        (false, true, OwnershipBlocker::Unfocused),
        (true, false, OwnershipBlocker::TrackingRecovery),
    ] {
        let mut policy = CapturePolicy::default();
        policy.update(Some(PanelRequest::Open), ready(true, true));
        policy.update(None, ready(true, false));
        let lost = policy.update(
            None,
            InputObservation {
                focused,
                tracking_ready,
                ..ready(false, false)
            },
        );
        assert!(!lost.snapshot.edit_capture.any());
        assert_eq!(lost.snapshot.blocker, Some(blocker));
        assert_eq!(lost.effects.game_device, DeviceEffect::SetEnabled(true));
        assert!(lost.effects.clear_pending_events);
        let recovered = policy.update(None, ready(false, true));
        assert!(!recovered.snapshot.edit_capture.any());
        assert_eq!(
            recovered.effects.game_device,
            DeviceEffect::SetEnabled(false)
        );
        assert!(
            policy
                .update(None, ready(true, false))
                .snapshot
                .edit_capture
                .any()
        );
    }
}

#[test]
fn transition_sample_never_delivers_input_even_when_neutral() {
    let mut policy = CapturePolicy::default();
    let opened = policy.update(Some(PanelRequest::Open), ready(true, true));
    assert!(!opened.snapshot.edit_capture.any());
    policy.update(None, ready(true, false));
    let closed = policy.update(Some(PanelRequest::Hide), ready(true, false));
    assert!(!closed.snapshot.gameplay_effective);
    assert!(
        policy
            .update(None, ready(true, true))
            .snapshot
            .gameplay_effective
    );
}

#[test]
fn device_effects_are_retried_and_effective_ownership_uses_observation() {
    let mut policy = CapturePolicy::default();
    policy.update(Some(PanelRequest::Open), ready(true, true));
    let unapplied_capture = policy.update(None, ready(true, true));
    assert!(!unapplied_capture.snapshot.edit_capture.any());
    assert_eq!(
        unapplied_capture.snapshot.blocker,
        Some(OwnershipBlocker::DeviceCapturePending)
    );
    assert_eq!(
        unapplied_capture.effects.game_device,
        DeviceEffect::SetEnabled(false)
    );
    policy.update(None, ready(true, false));
    policy.update(Some(PanelRequest::AllowGameplay), ready(true, false));
    let unapplied_restore = policy.update(None, ready(true, false));
    assert!(!unapplied_restore.snapshot.gameplay_effective);
    assert_eq!(
        unapplied_restore.effects.game_device,
        DeviceEffect::SetEnabled(true)
    );
    assert!(
        policy
            .update(None, ready(true, true))
            .snapshot
            .gameplay_effective
    );
}

#[test]
fn rapid_reopen_preserves_original_device_state_before_restore_is_observed() {
    let mut policy = CapturePolicy::default();
    policy.update(Some(PanelRequest::Open), ready(true, true));
    policy.update(Some(PanelRequest::Hide), ready(true, false));
    let reopened = policy.update(Some(PanelRequest::Open), ready(true, false));
    assert_eq!(reopened.snapshot.prior_game_device_enabled, Some(true));
    let hidden = policy.update(Some(PanelRequest::Hide), ready(true, false));
    assert_eq!(hidden.effects.game_device, DeviceEffect::SetEnabled(true));
}

#[test]
fn active_editing_device_loss_cancels_and_waits_for_new_neutral() {
    let mut policy = CapturePolicy::default();
    policy.update(Some(PanelRequest::Open), ready(true, true));
    assert!(
        policy
            .update(None, ready(true, false))
            .snapshot
            .edit_capture
            .any()
    );
    assert!(
        policy
            .update(None, ready(false, false))
            .snapshot
            .edit_capture
            .any()
    );

    let lost = policy.update(None, ready(false, true));
    assert_no_owner_during_handoff(lost);
    assert_eq!(lost.effects.game_device, DeviceEffect::SetEnabled(false));
    assert!(lost.effects.clear_pending_events);
    assert!(lost.effects.cancel_pointer_and_drag);
    assert_eq!(
        lost.snapshot.blocker,
        Some(OwnershipBlocker::DeviceCapturePending)
    );
    assert_no_owner_during_handoff(policy.update(None, ready(false, false)));
    assert_no_owner_during_handoff(policy.update(None, ready(false, false)));

    let neutral = policy.update(None, ready(true, false));
    assert!(neutral.snapshot.edit_capture.any());
    assert!(!neutral.effects.consume_until_neutral);
    let fresh_held = policy.update(None, ready(false, false));
    assert!(fresh_held.snapshot.edit_capture.any());
    assert!(!fresh_held.effects.clear_pending_events);
    assert!(!fresh_held.effects.cancel_pointer_and_drag);
    assert!(!fresh_held.effects.consume_until_neutral);
}

#[test]
fn delayed_acquisition_does_not_bank_neutral_before_device_acknowledgement() {
    let mut policy = CapturePolicy::default();
    policy.update(Some(PanelRequest::Open), ready(true, true));
    for _ in 0..3 {
        let pending = policy.update(None, ready(true, true));
        assert_no_owner_during_handoff(pending);
        assert_eq!(pending.effects.game_device, DeviceEffect::SetEnabled(false));
    }
    assert_no_owner_during_handoff(policy.update(None, ready(false, false)));
    assert_no_owner_during_handoff(policy.update(None, ready(false, false)));
    assert!(
        policy
            .update(None, ready(true, false))
            .snapshot
            .edit_capture
            .any()
    );
    assert!(
        policy
            .update(None, ready(false, false))
            .snapshot
            .edit_capture
            .any()
    );
}

#[test]
fn delayed_restoration_does_not_bank_neutral_before_device_acknowledgement() {
    for exit in [PanelRequest::Hide, PanelRequest::AllowGameplay] {
        let mut policy = CapturePolicy::default();
        policy.update(Some(PanelRequest::Open), ready(true, true));
        policy.update(None, ready(true, false));
        policy.update(Some(exit), ready(true, false));
        for _ in 0..3 {
            let pending = policy.update(None, ready(true, false));
            assert_no_owner_during_handoff(pending);
            assert!(pending.snapshot.device_restore_pending);
            assert_eq!(pending.effects.game_device, DeviceEffect::SetEnabled(true));
        }
        let acknowledged_held = policy.update(None, ready(false, true));
        assert_no_owner_during_handoff(acknowledged_held);
        assert!(!acknowledged_held.snapshot.device_restore_pending);
        assert_eq!(
            acknowledged_held.snapshot.blocker,
            Some(OwnershipBlocker::AwaitingNeutral)
        );
        assert_no_owner_during_handoff(policy.update(None, ready(false, true)));
        assert!(
            policy
                .update(None, ready(true, true))
                .snapshot
                .gameplay_effective
        );
        assert!(
            policy
                .update(None, ready(false, true))
                .snapshot
                .gameplay_effective
        );
    }
}

#[test]
fn recovery_requires_neutral_with_the_device_handoff_complete() {
    for (focused, tracking_ready) in [(false, true), (true, false)] {
        let mut policy = CapturePolicy::default();
        policy.update(Some(PanelRequest::Open), ready(true, true));
        policy.update(None, ready(true, false));
        let lost = policy.update(
            None,
            InputObservation {
                focused,
                tracking_ready,
                ..ready(true, false)
            },
        );
        assert_no_owner_during_handoff(lost);
        assert!(lost.effects.clear_pending_events);
        assert!(lost.effects.cancel_pointer_and_drag);
        let recovered_gate = policy.update(None, ready(true, true));
        assert_no_owner_during_handoff(recovered_gate);
        assert_eq!(
            recovered_gate.effects.game_device,
            DeviceEffect::SetEnabled(false)
        );
        assert_no_owner_during_handoff(policy.update(None, ready(true, true)));
        assert_no_owner_during_handoff(policy.update(None, ready(false, false)));
        assert!(
            policy
                .update(None, ready(true, false))
                .snapshot
                .edit_capture
                .any()
        );
        assert!(
            policy
                .update(None, ready(false, false))
                .snapshot
                .edit_capture
                .any()
        );
    }
}

#[test]
fn active_gameplay_device_loss_requires_neutral_after_reenable() {
    let mut policy = CapturePolicy::default();
    assert!(
        policy
            .update(None, ready(true, true))
            .snapshot
            .gameplay_effective
    );
    assert!(
        policy
            .update(None, ready(false, true))
            .snapshot
            .gameplay_effective
    );
    let lost = policy.update(None, ready(false, false));
    assert_no_owner_during_handoff(lost);
    assert!(lost.effects.clear_pending_events);
    assert!(lost.effects.cancel_pointer_and_drag);
    assert_no_owner_during_handoff(policy.update(None, ready(true, false)));
    assert_no_owner_during_handoff(policy.update(None, ready(false, true)));
    assert!(
        policy
            .update(None, ready(true, true))
            .snapshot
            .gameplay_effective
    );
    assert!(
        policy
            .update(None, ready(false, true))
            .snapshot
            .gameplay_effective
    );
}

fn assert_no_owner_during_handoff(update: CaptureUpdate) {
    assert!(!update.snapshot.edit_capture.any());
    assert!(!update.snapshot.gameplay_effective);
    assert!(update.snapshot.awaiting_neutral);
    assert!(update.effects.consume_until_neutral);
}

#[test]
fn read_only_filters_pointer_keyboard_text_scroll_and_clipboard_events() {
    let mut policy = CapturePolicy::default();
    policy.update(Some(PanelRequest::Open), ready(true, true));
    let snapshot = policy
        .update(Some(PanelRequest::AllowGameplay), ready(true, false))
        .snapshot;
    let mut input = egui::RawInput {
        focused: true,
        events: vec![
            egui::Event::PointerMoved(egui::pos2(20.0, 20.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(20.0, 20.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::Text("sample".into()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, 1.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::Paste("sample".into()),
            egui::Event::Copy,
            egui::Event::Cut,
        ],
        ..Default::default()
    };
    snapshot.filter_egui_input(&mut input);
    assert!(input.events.is_empty());
    assert!(!input.focused);
    assert_eq!(snapshot.edit_capture, EditCapture::default());
}
