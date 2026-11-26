use std::num::NonZeroUsize;

use jc3gi::vehicle::vehicle::DoorSlot;
use jc3vr_body::controllers::{grab::world_reach::DoorTarget, physics::bodies::BodyError};

use crate::controllers::physical_hands::doors::{UNLATCH_PULL_M, engine::DoorError, slot_label};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Reason {
    NoContact,
    GlassRejected,
    NoPanel,
    ExteriorFace,
    InsufficientMovement,
    ContactLost,
    OpenPanel,
    Opened,
    InvalidSample,
    NativeFailure(DoorError),
}

impl Reason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoContact => "no contact",
            Self::GlassRejected => "glass rejected",
            Self::NoPanel => "no panel",
            Self::ExteriorFace => "exterior/inward face",
            Self::InsufficientMovement => "insufficient movement",
            Self::ContactLost => "contact lost",
            Self::OpenPanel => "open panel",
            Self::Opened => "opened",
            Self::InvalidSample => "invalid sample",
            Self::NativeFailure(_) => "native failure",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Readout {
    pub reason: Reason,
    pub slot: Option<DoorSlot>,
    pub displacement_m: f32,
    pub required_m: f32,
}

#[derive(Debug, Copy, Clone)]
pub(super) struct Diagnostics {
    pub(super) readout: Readout,
    target: Option<(NonZeroUsize, u32, DoorSlot)>,
    failure: Option<Failure>,
}

impl Diagnostics {
    pub(super) const fn new() -> Self {
        Self {
            readout: Readout {
                reason: Reason::NoContact,
                slot: None,
                displacement_m: 0.0,
                required_m: UNLATCH_PULL_M,
            },
            target: None,
            failure: None,
        }
    }

    pub(super) fn target(&mut self, target: DoorTarget) {
        let key = (target.vehicle, target.part_index, target.slot);
        if self.target != Some(key) {
            self.readout.displacement_m = 0.0;
        }
        self.target = Some(key);
        self.readout.slot = Some(target.slot);
    }

    pub(super) fn reason(&mut self, reason: Reason) {
        self.readout.reason = reason;
    }

    pub(super) fn idle(&mut self, reason: Reason) {
        self.reason(reason);
        if reason != Reason::ContactLost {
            self.target = None;
            self.readout.slot = None;
            self.readout.displacement_m = 0.0;
        }
    }

    pub(super) fn movement(&mut self, displacement: f32, scale: f32) {
        self.readout.displacement_m = displacement / scale;
    }

    pub(super) fn opened(&mut self) {
        self.reason(Reason::Opened);
        self.failure = None;
        if let Some((vehicle, part, slot)) = self.target {
            tracing::info!(target: "controllers", vehicle = vehicle.get(), part,
                slot = slot_label(slot), displacement_m = self.readout.displacement_m,
                required_m = self.readout.required_m, "door pressure: opened");
        }
    }

    pub(super) fn failed(&mut self, error: DoorError, body_error: Option<BodyError>) {
        if self.failure_changed(error, body_error) {
            tracing::warn!(target: "controllers",
                vehicle = self.target.map(|(vehicle, _, _)| vehicle.get()),
                part = self.target.map(|(_, part, _)| part),
                slot = self.readout.slot.map(slot_label), %error, ?body_error,
                displacement_m = self.readout.displacement_m,
                required_m = self.readout.required_m, "door pressure: native failure");
        }
    }

    fn failure_changed(&mut self, error: DoorError, body_error: Option<BodyError>) -> bool {
        self.reason(Reason::NativeFailure(error));
        let failure = Failure {
            target: self.target,
            error,
            body_error,
        };
        let changed = self.failure != Some(failure);
        self.failure = Some(failure);
        changed
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Failure {
    target: Option<(NonZeroUsize, u32, DoorSlot)>,
    error: DoorError,
    body_error: Option<BodyError>,
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    fn target() -> DoorTarget {
        DoorTarget {
            vehicle: NonZeroUsize::new(64).unwrap(),
            vehicle_body: Some(7),
            part_index: 2,
            slot: DoorSlot::FRONT_LEFT,
            latched: true,
            outward: Vec3::X,
        }
    }

    #[test]
    fn movement_is_in_metres_and_loss_retains_the_last_contact() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.target(target());
        diagnostics.movement(0.06, 2.0);
        diagnostics.idle(Reason::ContactLost);
        assert_eq!(diagnostics.readout.reason, Reason::ContactLost);
        assert_eq!(diagnostics.readout.slot, Some(DoorSlot::FRONT_LEFT));
        assert_eq!(diagnostics.readout.displacement_m, 0.03);
        assert_eq!(diagnostics.readout.required_m, UNLATCH_PULL_M);
        diagnostics.idle(Reason::NoContact);
        assert_eq!(diagnostics.readout.slot, None);
        assert_eq!(diagnostics.readout.displacement_m, 0.0);
    }

    #[test]
    fn failure_deduplication_preserves_native_body_context() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.target(target());
        assert!(diagnostics.failure_changed(DoorError::NoLooseBody, Some(BodyError::NoBody)));
        assert!(!diagnostics.failure_changed(DoorError::NoLooseBody, Some(BodyError::NoBody)));
        diagnostics.idle(Reason::NoContact);
        diagnostics.target(target());
        assert!(!diagnostics.failure_changed(DoorError::NoLooseBody, Some(BodyError::NoBody)));
        assert!(diagnostics.failure_changed(DoorError::NoLooseBody, Some(BodyError::Reowned)));
        let mut other = target();
        other.part_index += 1;
        diagnostics.target(other);
        assert!(diagnostics.failure_changed(DoorError::NoLooseBody, Some(BodyError::Reowned)));
        assert_eq!(
            diagnostics.readout.reason,
            Reason::NativeFailure(DoorError::NoLooseBody)
        );
    }
}
