use std::cell::Cell;

use super::*;

struct Harness {
    context: egui::Context,
    adapter: EguiCaptureAdapter,
    events: Vec<egui::Event>,
    time: f64,
    clicks: usize,
    text: String,
    raw_loss_generation: u64,
}

impl Harness {
    fn new() -> Self {
        let mut harness = Self {
            context: egui::Context::default(),
            adapter: EguiCaptureAdapter::default(),
            events: Vec::new(),
            time: 0.0,
            clicks: 0,
            text: String::new(),
            raw_loss_generation: 0,
        };
        harness.update(Some(PanelRequest::Open), observation(true, true));
        harness.update(None, observation(true, false));
        harness
    }

    fn update(&mut self, request: Option<PanelRequest>, input: InputObservation) -> CaptureUpdate {
        self.adapter
            .update(request, input, &self.context, &mut self.events)
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> (egui::Rect, egui::Id) {
        self.time += 1.0 / 60.0;
        self.events.extend(events);
        let _update = self.adapter.apply_raw_loss(
            self.raw_loss_generation,
            observation(false, false),
            &self.context,
            &mut self.events,
        );
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            time: Some(self.time),
            focused: true,
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.adapter.prepare_input(&mut input);
        if let Some(cancellation) = self.adapter.take_cancellation_input(&input) {
            let _output = self.context.run_ui(cancellation, |ui| {
                let _response = ui.button("Run fixture action");
                ui.text_edit_singleline(&mut self.text);
            });
            assert!(!self.context.egui_is_using_pointer());
        }
        let mut button = None;
        let mut text_id = None;
        let _output = self.context.run_ui(input, |ui| {
            let response = ui.button("Run fixture action");
            self.clicks += usize::from(response.clicked());
            button = Some(response.rect);
            let response = ui.text_edit_singleline(&mut self.text);
            text_id = Some(response.id);
        });
        (
            button.expect("button rectangle"),
            text_id.expect("text field"),
        )
    }

    fn press_button(&mut self) -> egui::Pos2 {
        let mut previous = None;
        let mut point = None;
        for _ in 0..16 {
            let rect = self.frame(Vec::new()).0;
            if previous == Some(rect) {
                point = Some(rect.center());
                break;
            }
            previous = Some(rect);
        }
        let point = point.expect("settled button rectangle");
        self.frame(vec![egui::Event::PointerMoved(point)]);
        self.frame(vec![button_event(point, true)]);
        assert!(self.context.input(|input| input.pointer.primary_down()));
        assert!(self.context.egui_is_using_pointer());
        point
    }
}

fn observation(neutral: bool, enabled: bool) -> InputObservation {
    InputObservation {
        focused: true,
        tracking_ready: true,
        controls_neutral: neutral,
        game_device_enabled: enabled,
    }
}

fn button_event(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

#[test]
fn adapter_clears_remembered_pointer_before_read_only_stale_release() {
    let mut harness = Harness::new();
    let point = harness.press_button();
    harness.events.push(button_event(point, false));
    let update = harness.update(Some(PanelRequest::AllowGameplay), observation(false, false));
    assert!(update.effects.cancel_pointer_and_drag);
    assert!(harness.events.is_empty());
    assert!(!harness.context.input(|input| input.pointer.any_down()));
    assert!(harness.context.dragged_id().is_none());
    assert!(harness.context.memory(|memory| memory.focused()).is_none());
    harness.update(None, observation(true, true));
    harness.frame(vec![button_event(point, false)]);
    assert_eq!(harness.clicks, 0);
}

#[test]
fn adapter_rapid_hide_reopen_requires_acknowledgement_then_neutral() {
    let mut harness = Harness::new();
    let point = harness.press_button();
    harness.update(Some(PanelRequest::Hide), observation(false, false));
    let reopened = harness.update(Some(PanelRequest::Open), observation(true, false));
    assert!(!reopened.snapshot.edit_capture.any());
    assert_eq!(reopened.snapshot.prior_game_device_enabled, Some(true));
    let held = harness.update(None, observation(false, false));
    assert!(!held.snapshot.edit_capture.any());
    harness.frame(vec![button_event(point, false)]);
    assert_eq!(harness.clicks, 0);
    let neutral = harness.update(None, observation(true, false));
    assert!(neutral.snapshot.edit_capture.pointer);
    harness.frame(vec![button_event(point, false)]);
    assert_eq!(harness.clicks, 0);
    let point = harness.press_button();
    harness.frame(vec![button_event(point, false)]);
    assert_eq!(harness.clicks, 1);
}

#[test]
fn adapter_focus_loss_while_held_clears_keys_and_defers_device_enable() {
    let mut harness = Harness::new();
    let point = harness.press_button();
    harness.frame(vec![egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    }]);
    assert!(harness.context.input(|input| input.key_down(egui::Key::A)));
    let update = harness.update(
        None,
        InputObservation {
            focused: false,
            ..observation(false, false)
        },
    );
    assert_eq!(
        update.snapshot.blocker,
        Some(crate::ui::capture::OwnershipBlocker::Unfocused)
    );
    assert_eq!(device_write(update.effects.game_device, false), None);
    assert!(!harness.context.input(|input| input.key_down(egui::Key::A)));
    assert!(!harness.context.input(|input| input.pointer.any_down()));
    assert!(
        !harness
            .update(None, observation(false, false))
            .snapshot
            .edit_capture
            .any()
    );
    harness.frame(vec![button_event(point, false)]);
    assert_eq!(harness.clicks, 0);
    assert!(
        harness
            .update(None, observation(true, false))
            .snapshot
            .edit_capture
            .any()
    );
}

#[test]
fn adapter_read_only_has_no_text_key_scroll_or_clipboard_input() {
    let mut harness = Harness::new();
    let text_id = harness.frame(Vec::new()).1;
    harness
        .context
        .memory_mut(|memory| memory.request_focus(text_id));
    harness.frame(vec![egui::Event::Text("editable".into())]);
    assert_eq!(harness.text, "editable");
    harness.update(Some(PanelRequest::AllowGameplay), observation(false, false));
    harness.update(None, observation(true, true));
    harness.frame(vec![
        egui::Event::Text("blocked".into()),
        egui::Event::Paste("blocked".into()),
        egui::Event::Cut,
        egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 100.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    assert_eq!(harness.text, "editable");
    harness.context.input(|input| {
        assert!(input.events.is_empty());
        assert!(input.keys_down.is_empty());
        assert_eq!(input.smooth_scroll_delta, egui::Vec2::ZERO);
        assert!(!input.is_scrolling());
        assert!(!input.focused);
    });
    assert!(harness.context.memory(|memory| memory.focused()).is_none());
}

#[test]
fn adapter_out_of_tick_open_hide_keeps_cancellation_until_input_owner_receives_it() {
    let context = egui::Context::default();
    let mut adapter = EguiCaptureAdapter::default();
    let mut events = Vec::new();
    adapter.update(None, observation(true, true), &context, &mut events);
    adapter.update(
        Some(PanelRequest::Open),
        observation(false, true),
        &context,
        &mut events,
    );
    adapter.update(
        Some(PanelRequest::Hide),
        observation(false, false),
        &context,
        &mut events,
    );
    let sample = adapter.input_owner_sample(DebugUiInput {
        focused: true,
        tracking_ready: true,
        controls_neutral: true,
    });
    assert!(!sample.controls_neutral);
    let update = adapter.update(
        None,
        observation(sample.controls_neutral, true),
        &context,
        &mut events,
    );
    let notification = adapter.notify_input_owner(update);
    assert!(notification.effects.cancel_pointer_and_drag);
    assert!(notification.effects.clear_pending_events);
    assert!(notification.effects.consume_until_neutral);
    assert!(!notification.snapshot.gameplay_effective);
    assert!(
        adapter
            .input_owner_sample(DebugUiInput {
                controls_neutral: true,
                ..sample
            })
            .controls_neutral
    );
    let update = adapter.update(None, observation(true, true), &context, &mut events);
    assert!(update.snapshot.gameplay_effective);
}

#[test]
fn adapter_last_surface_loss_hides_but_a_visible_desktop_keeps_editing() {
    let mut harness = Harness::new();
    assert_eq!(harness.adapter.surface_request(true), None);
    let hide = harness
        .adapter
        .surface_request(false)
        .expect("last surface disappears");
    let update = harness.update(Some(hide), observation(false, false));
    assert_eq!(update.snapshot.mode, PanelMode::Hidden);
    assert_eq!(update.effects.game_device, DeviceEffect::SetEnabled(true));
    assert!(update.effects.cancel_pointer_and_drag);
    assert!(!update.snapshot.gameplay_effective);
    assert!(
        harness
            .update(None, observation(true, true))
            .snapshot
            .gameplay_effective
    );
}

#[test]
fn adapter_two_buttons_reopen_between_frames_does_not_transfer_the_old_click_owner() {
    let context = egui::Context::default();
    let mut adapter = EguiCaptureAdapter::default();
    let mut pending = Vec::new();
    adapter.update(
        Some(PanelRequest::Open),
        observation(true, true),
        &context,
        &mut pending,
    );
    adapter.update(None, observation(true, false), &context, &mut pending);
    let mut clicks = [0, 0];
    let mut time = 0.0;
    let mut frame = |adapter: &mut EguiCaptureAdapter, events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let mut input = egui::RawInput {
            focused: true,
            time: Some(time),
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        };
        adapter.prepare_input(&mut input);
        let mut draw = |ui: &mut egui::Ui| {
            let a = ui.button("Fixture A");
            let b = ui.button("Fixture B");
            clicks[0] += usize::from(a.clicked());
            clicks[1] += usize::from(b.clicked());
            [a.rect.center(), b.rect.center()]
        };
        if let Some(cancellation) = adapter.take_cancellation_input(&input) {
            let _output = context.run_ui(cancellation, |ui| {
                draw(ui);
            });
            assert!(!context.egui_is_using_pointer());
        }
        let mut points = [egui::Pos2::ZERO; 2];
        let _output = context.run_ui(input, |ui| {
            points = draw(ui);
        });
        points
    };
    let mut points = frame(&mut adapter, Vec::new());
    for _ in 0..4 {
        points = frame(&mut adapter, Vec::new());
    }
    frame(&mut adapter, vec![egui::Event::PointerMoved(points[0])]);
    frame(&mut adapter, vec![button_event(points[0], true)]);
    assert!(context.egui_is_using_pointer());
    adapter.update(
        Some(PanelRequest::Hide),
        observation(false, false),
        &context,
        &mut pending,
    );
    adapter.update(
        Some(PanelRequest::Open),
        observation(false, true),
        &context,
        &mut pending,
    );
    adapter.update(None, observation(true, false), &context, &mut pending);
    frame(
        &mut adapter,
        vec![
            egui::Event::PointerMoved(points[1]),
            button_event(points[1], true),
        ],
    );
    frame(&mut adapter, vec![button_event(points[1], false)]);
    assert_eq!(clicks, [0, 1]);
}

#[test]
fn adapter_focus_loss_between_ticks_cancels_held_state_before_recovered_neutral() {
    let mut harness = Harness::new();
    let point = harness.press_button();
    harness.frame(vec![egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    }]);
    assert!(harness.context.input(|input| input.key_down(egui::Key::A)));
    harness.raw_loss_generation = 1;
    let lost = harness
        .adapter
        .apply_raw_loss(
            harness.raw_loss_generation,
            observation(false, false),
            &harness.context,
            &mut harness.events,
        )
        .expect("the input update observes the latched focus loss");
    assert_eq!(device_write(lost.effects.game_device, false), None);
    assert!(!harness.context.input(|input| input.pointer.any_down()));
    assert!(!harness.context.input(|input| input.key_down(egui::Key::A)));
    let recovered = harness.adapter.input_owner_sample(DebugUiInput {
        focused: true,
        tracking_ready: true,
        controls_neutral: true,
    });
    assert!(!recovered.controls_neutral);
    let update = harness.update(None, observation(recovered.controls_neutral, false));
    let update = harness.adapter.notify_input_owner(update);
    assert!(update.effects.clear_pending_events);
    assert!(update.effects.cancel_pointer_and_drag);
    assert!(update.effects.consume_until_neutral);
    assert!(!update.snapshot.observed_game_device_enabled);
    assert!(!update.snapshot.edit_capture.any());
    assert!(!update.snapshot.gameplay_effective);
    assert!(!harness.adapter.input_notification_pending);
    harness.frame(vec![button_event(point, false)]);
    assert_eq!(harness.clicks, 0);
    assert!(!harness.adapter.input_notification_pending);
    let held = harness.adapter.input_owner_sample(DebugUiInput {
        controls_neutral: false,
        ..recovered
    });
    assert!(
        !harness
            .update(None, observation(held.controls_neutral, false))
            .snapshot
            .edit_capture
            .any()
    );
    let neutral = harness.adapter.input_owner_sample(DebugUiInput {
        focused: true,
        tracking_ready: true,
        controls_neutral: true,
    });
    assert!(neutral.controls_neutral);
    assert!(
        harness
            .update(None, observation(neutral.controls_neutral, false))
            .snapshot
            .edit_capture
            .any()
    );
}

#[test]
fn raw_loss_regain_cancels_the_first_egui_frame_before_any_input_owner_update() {
    let mut harness = Harness::new();
    let point = harness.press_button();
    let text_id = harness
        .frame(vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }])
        .1;
    harness
        .context
        .memory_mut(|memory| memory.request_focus(text_id));
    assert!(harness.context.input(|input| input.key_down(egui::Key::A)));
    assert_eq!(
        harness.context.memory(|memory| memory.focused()),
        Some(text_id)
    );
    let active = harness.update(None, observation(false, false));
    harness.adapter.notify_input_owner(active);
    assert!(!harness.adapter.input_notification_pending);
    // Focus regains before this frame, but the raw observer retains the intervening loss.
    harness.raw_loss_generation = 1;
    harness.events.push(button_event(point, false));
    harness
        .events
        .push(egui::Event::Text("stale interval".into()));
    harness.frame(Vec::new());
    assert_eq!(harness.clicks, 0);
    assert!(harness.text.is_empty());
    assert!(!harness.context.input(|input| input.pointer.any_down()));
    assert!(!harness.context.input(|input| input.key_down(egui::Key::A)));
    assert!(harness.context.memory(|memory| memory.focused()).is_none());
    assert!(harness.adapter.input_notification_pending);
    let sample = harness.adapter.input_owner_sample(DebugUiInput {
        focused: true,
        tracking_ready: true,
        controls_neutral: true,
    });
    assert!(!sample.controls_neutral);
    let update = harness.update(None, observation(sample.controls_neutral, false));
    let notification = harness.adapter.notify_input_owner(update);
    assert!(notification.effects.clear_pending_events);
    assert!(notification.effects.cancel_pointer_and_drag);
    assert!(notification.effects.consume_until_neutral);
    assert!(!notification.snapshot.edit_capture.any());
    assert!(!notification.snapshot.gameplay_effective);
}

#[test]
fn enabling_rechecks_focus_and_raw_loss_after_cancellation_before_any_write() {
    for after_cancellation in [(false, 0), (true, 1)] {
        let context = egui::Context::default();
        let mut adapter = EguiCaptureAdapter::default();
        let mut events = Vec::new();
        adapter.update(
            Some(PanelRequest::Open),
            observation(false, true),
            &context,
            &mut events,
        );
        adapter.update(None, observation(true, false), &context, &mut events);
        let focus_source = Cell::new((true, 0));
        let mut update = adapter.update(
            Some(PanelRequest::AllowGameplay),
            observation(false, false),
            &context,
            &mut events,
        );
        assert_eq!(update.effects.game_device, DeviceEffect::SetEnabled(true));
        assert!(update.effects.cancel_pointer_and_drag);
        focus_source.set(after_cancellation);
        let write = adapter.validate_device_write(
            &mut update,
            observation(false, false),
            &context,
            &mut events,
            || focus_source.get(),
        );
        assert_eq!(write, None);
        assert!(!update.snapshot.focused);
        assert!(!update.snapshot.edit_capture.any());
        assert!(!update.snapshot.gameplay_effective);
        assert!(update.snapshot.device_restore_pending);
        assert!(update.snapshot.intended_game_device_enabled);
        assert!(adapter.input_notification_pending);
        let mut recovered = adapter.update(None, observation(false, false), &context, &mut events);
        assert!(!recovered.snapshot.gameplay_effective);
        assert_eq!(
            adapter.validate_device_write(
                &mut recovered,
                observation(false, false),
                &context,
                &mut events,
                || (true, after_cancellation.1),
            ),
            Some(true)
        );
        let sample = adapter.input_owner_sample(DebugUiInput {
            focused: true,
            tracking_ready: true,
            controls_neutral: true,
        });
        assert!(!sample.controls_neutral);
        let update = adapter.update(
            None,
            observation(sample.controls_neutral, true),
            &context,
            &mut events,
        );
        let notification = adapter.notify_input_owner(update);
        assert!(!notification.snapshot.gameplay_effective);
        assert!(notification.effects.consume_until_neutral);
        assert!(
            adapter
                .update(None, observation(true, true), &context, &mut events)
                .snapshot
                .gameplay_effective
        );
    }
}

#[test]
fn write_revalidation_preserves_a_disabled_device_restoration_while_unfocused() {
    let context = egui::Context::default();
    let mut adapter = EguiCaptureAdapter::default();
    let mut events = Vec::new();
    adapter.update(
        Some(PanelRequest::Open),
        observation(false, false),
        &context,
        &mut events,
    );
    let mut update = adapter.update(
        Some(PanelRequest::AllowGameplay),
        observation(false, false),
        &context,
        &mut events,
    );
    assert_eq!(update.effects.game_device, DeviceEffect::SetEnabled(false));
    assert_eq!(
        adapter.validate_device_write(
            &mut update,
            observation(false, false),
            &context,
            &mut events,
            || (false, 0),
        ),
        Some(false)
    );
    assert!(!update.snapshot.intended_game_device_enabled);
    assert!(!update.snapshot.gameplay_effective);
}

#[test]
fn adapter_preserves_disabled_device_and_does_not_assume_write_acknowledgement() {
    let context = egui::Context::default();
    let mut adapter = EguiCaptureAdapter::default();
    let mut events = Vec::new();
    adapter.update(
        Some(PanelRequest::Open),
        observation(true, false),
        &context,
        &mut events,
    );
    adapter.update(None, observation(true, false), &context, &mut events);
    let update = adapter.update(
        Some(PanelRequest::AllowGameplay),
        observation(true, false),
        &context,
        &mut events,
    );
    assert_eq!(device_write(update.effects.game_device, true), Some(false));
    assert!(!update.snapshot.gameplay_effective);
    let update = adapter.update(None, observation(true, false), &context, &mut events);
    assert!(!update.snapshot.gameplay_effective);
    assert!(!update.snapshot.intended_game_device_enabled);
    assert_eq!(device_write(DeviceEffect::SetEnabled(true), false), None);
    assert_eq!(
        device_write(DeviceEffect::SetEnabled(true), true),
        Some(true)
    );
}
