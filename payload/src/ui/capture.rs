//! Pure debug-panel ownership policy and egui event admission.
//!
//! The caller supplies focus, recovery, neutral input, and the observed game-device state.
//! Returned effects require an adapter at the existing input boundaries; this module never
//! reads engine globals, changes a device, or implements the panel shortcut.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PanelMode {
    #[default]
    Hidden,
    Editing,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelRequest {
    Open,
    Hide,
    AllowGameplay,
}

/// A valid neutral sample covers triggers, grips, buttons, sticks, pointer buttons, and keys.
/// The adapter must also finish scroll/text composition and any consumed shortcut episode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputObservation {
    pub focused: bool,
    pub tracking_ready: bool,
    pub controls_neutral: bool,
    pub game_device_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnershipBlocker {
    Editing,
    Unfocused,
    TrackingRecovery,
    AwaitingNeutral,
    DeviceCapturePending,
    GameDeviceDisabled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EditCapture {
    pub pointer: bool,
    pub grip_drag: bool,
    pub keyboard: bool,
    pub text: bool,
    pub scroll: bool,
}

impl EditCapture {
    pub(crate) fn any(self) -> bool {
        self.pointer || self.grip_drag || self.keyboard || self.text || self.scroll
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnershipSnapshot {
    pub mode: PanelMode,
    pub visible: bool,
    pub editing_requested: bool,
    pub gameplay_requested: bool,
    pub edit_capture: EditCapture,
    pub gameplay_effective: bool,
    pub blocker: Option<OwnershipBlocker>,
    pub awaiting_neutral: bool,
    pub focused: bool,
    pub tracking_ready: bool,
    pub prior_game_device_enabled: Option<bool>,
    pub observed_game_device_enabled: bool,
    pub intended_game_device_enabled: bool,
    pub device_restore_pending: bool,
}

impl OwnershipSnapshot {
    /// Admission is fail-closed for every egui event, including clipboard and IME events.
    /// Grip dragging is outside egui and must use `edit_capture.grip_drag` separately.
    pub(crate) fn filter_egui_input(self, input: &mut egui::RawInput) {
        if !self.edit_capture.any() {
            input.events.clear();
            input.modifiers = egui::Modifiers::default();
            input.focused = false;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DeviceEffect {
    #[default]
    Unchanged,
    SetEnabled(bool),
}

/// Effects describe required work, not completed engine or egui operations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CaptureEffects {
    pub game_device: DeviceEffect,
    pub clear_pending_events: bool,
    pub cancel_pointer_and_drag: bool,
    pub consume_until_neutral: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaptureUpdate {
    pub snapshot: OwnershipSnapshot,
    pub effects: CaptureEffects,
}

#[derive(Debug, Default)]
pub(crate) struct CapturePolicy {
    mode: PanelMode,
    ownership: OwnershipState,
    prior_game_device_enabled: Option<bool>,
    restore_pending: Option<bool>,
    gates_were_ready: bool,
}

impl CapturePolicy {
    /// Call once per input sample, before delivering any pointer or gameplay output.
    /// `Open` only opens a hidden panel; a visible ReadOnly panel cannot edit itself.
    pub(crate) fn update(
        &mut self,
        request: Option<PanelRequest>,
        input: InputObservation,
    ) -> CaptureUpdate {
        let next_mode = match (self.mode, request) {
            (_, Some(PanelRequest::Hide)) => PanelMode::Hidden,
            (PanelMode::Hidden, Some(PanelRequest::Open)) => PanelMode::Editing,
            (PanelMode::Editing, Some(PanelRequest::AllowGameplay)) => PanelMode::ReadOnly,
            _ => self.mode,
        };
        let transitioned = next_mode != self.mode;
        let gates_ready = input.focused && input.tracking_ready;
        let lost_gate = self.gates_were_ready && !gates_ready;
        self.mode = next_mode;
        self.gates_were_ready = gates_ready;

        let requested_owner = if self.mode == PanelMode::Editing {
            InputOwner::Editing
        } else {
            InputOwner::Gameplay
        };
        let wants_device_capture = requested_owner == InputOwner::Editing && gates_ready;
        let prior_for_snapshot = self.prior_game_device_enabled.or(self.restore_pending);
        let game_device = if wants_device_capture {
            let acquiring = self.prior_game_device_enabled.is_none();
            if acquiring {
                self.prior_game_device_enabled = Some(
                    self.restore_pending
                        .take()
                        .unwrap_or(input.game_device_enabled),
                );
            }
            if acquiring || input.game_device_enabled {
                DeviceEffect::SetEnabled(false)
            } else {
                DeviceEffect::Unchanged
            }
        } else if let Some(enabled) = self.prior_game_device_enabled.take() {
            self.restore_pending = Some(enabled);
            DeviceEffect::SetEnabled(enabled)
        } else if let Some(enabled) = self.restore_pending {
            if input.game_device_enabled == enabled {
                self.restore_pending = None;
                DeviceEffect::Unchanged
            } else {
                DeviceEffect::SetEnabled(enabled)
            }
        } else {
            DeviceEffect::Unchanged
        };
        let intended_device = match game_device {
            DeviceEffect::Unchanged => input.game_device_enabled,
            DeviceEffect::SetEnabled(enabled) => enabled,
        };
        let device_ready = match requested_owner {
            InputOwner::Editing => wants_device_capture && !input.game_device_enabled,
            InputOwner::Gameplay => self.restore_pending.is_none() && input.game_device_enabled,
        };
        let owner_ready = gates_ready && device_ready;
        let lost_ownership = matches!(self.ownership, OwnershipState::Active(_))
            && (!owner_ready || self.ownership != OwnershipState::Active(requested_owner));
        self.ownership = if transitioned || !owner_ready {
            OwnershipState::WaitingForDevice(requested_owner)
        } else if self.ownership == OwnershipState::Active(requested_owner) {
            self.ownership
        } else if input.controls_neutral {
            // Neutral samples count only after the destination's device state is observed.
            OwnershipState::Active(requested_owner)
        } else {
            OwnershipState::WaitingForNeutral(requested_owner)
        };
        let awaiting_neutral = !matches!(self.ownership, OwnershipState::Active(_));
        let can_edit = self.ownership == OwnershipState::Active(InputOwner::Editing);
        let edit_capture = EditCapture {
            pointer: can_edit,
            grip_drag: can_edit,
            keyboard: can_edit,
            text: can_edit,
            scroll: can_edit,
        };
        let blocker = if !input.focused {
            Some(OwnershipBlocker::Unfocused)
        } else if !input.tracking_ready {
            Some(OwnershipBlocker::TrackingRecovery)
        } else if requested_owner == InputOwner::Editing && !device_ready {
            Some(OwnershipBlocker::DeviceCapturePending)
        } else if requested_owner == InputOwner::Gameplay && !device_ready {
            Some(OwnershipBlocker::GameDeviceDisabled)
        } else if awaiting_neutral {
            Some(OwnershipBlocker::AwaitingNeutral)
        } else if requested_owner == InputOwner::Editing {
            Some(OwnershipBlocker::Editing)
        } else {
            None
        };
        CaptureUpdate {
            snapshot: OwnershipSnapshot {
                mode: self.mode,
                visible: self.mode != PanelMode::Hidden,
                editing_requested: self.mode == PanelMode::Editing,
                gameplay_requested: self.mode != PanelMode::Editing,
                edit_capture,
                gameplay_effective: blocker.is_none(),
                blocker,
                awaiting_neutral,
                focused: input.focused,
                tracking_ready: input.tracking_ready,
                prior_game_device_enabled: self.prior_game_device_enabled.or(prior_for_snapshot),
                observed_game_device_enabled: input.game_device_enabled,
                intended_game_device_enabled: intended_device,
                device_restore_pending: self.restore_pending.is_some(),
            },
            effects: CaptureEffects {
                game_device,
                clear_pending_events: transitioned || lost_gate || lost_ownership,
                cancel_pointer_and_drag: transitioned || lost_gate || lost_ownership,
                consume_until_neutral: awaiting_neutral,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputOwner {
    Editing,
    Gameplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnershipState {
    WaitingForDevice(InputOwner),
    WaitingForNeutral(InputOwner),
    Active(InputOwner),
}

impl Default for OwnershipState {
    fn default() -> Self {
        Self::WaitingForNeutral(InputOwner::Gameplay)
    }
}

#[cfg(test)]
#[path = "capture/tests.rs"]
mod tests;
