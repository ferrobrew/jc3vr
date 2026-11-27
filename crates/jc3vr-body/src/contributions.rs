//! What the body crate reads from the crates above it: one [`BodyContributions`] record, filled at
//! install by the crates that provide each part and registered once ([`register`]), so the body
//! names none of the systems above it (`docs/architecture.md`, "Crate layering").
//!
//! The composition starts from [`BodyContributions::default`], hands `&mut` to each crate's
//! registration to fill its own fields, then calls [`register`] with the install's
//! [`RegistrationCheck`]: every field still empty is reported to the check, which stops the startup
//! naming it, and the record is stored whole. The ordered tables (the hand stages, the world weapon
//! observers, the pre-weapon hooks, and the auto-attach refusals) run in the order of the slice
//! they are given, so their order is declared once, by the table that fills them.
//!
//! Every reader goes through [`get`]. Before the registration (and in a test that does not
//! register) it is `None`, and each reader answers as if the contribution did nothing.

use std::sync::OnceLock;

use glam::Vec2;
use jc3gi::{character::character::Character, game::GameObjectUpdateContext, physics::hknpWorld};
use jc3vr_core::registration::RegistrationCheck;
use jc3vr_input::frame::Hand;

use crate::{
    controllers::{
        aim::hooks::AimHooks,
        aim_state::PreWeaponHookFn,
        body::mirror::AttachmentSampler,
        facts::{climb::ClimbReaders, mounted::MountedSource},
        fingers::grip::{GripSource, MAX_SOURCES},
        hands::{anchor::HangPump, late::ObserveWeapon, stages::HandStageFn},
        held::{HeldSource, guns::TargetsFn, pickup::PickupSink},
    },
    hooks::character::{BlocksAutoAttach, PoseObserver},
};

/// Everything the body reads from the crates above it. Each field names the subsystem that fills
/// it today; the field's own type documents the calling context.
#[derive(Default)]
pub struct BodyContributions {
    /// The weapon handling's and the hand bodies' hooks into the aim update and the per-weapon
    /// update (gameplay's aim table).
    pub aim_hooks: Option<AimHooks>,
    /// The contributions around the local player's weapon handling, in the order they run; each
    /// runs around the ones after it and the call (gameplay's aim table).
    pub pre_weapon_hooks: &'static [PreWeaponHookFn],
    /// The mounted gun the hands aim (the vehicle handling).
    pub mounted: Option<MountedSource>,
    /// The hand composition's target stages, in the order they run: the poses the hands take on
    /// what they hold, which the hand bodies' drive applies too (gameplay's stage table).
    pub target_stages: &'static [HandStageFn],
    /// The rest of the hand composition's stages, in the order they run, after the target stages
    /// (gameplay's stage table).
    pub hand_stages: &'static [HandStageFn],
    /// The observers of each world weapon at its render update, in the order they run (gameplay's
    /// observer table).
    pub weapon_observers: &'static [ObserveWeapon],
    /// The hand bodies' sample of their post-step presentation offsets, taken once per rendered
    /// frame before the hands are composed, on the game thread after the simulation and outside
    /// its step (the physical hands).
    pub post_step_sample: Option<fn()>,
    /// The held-item source behind each hand's contents (the weapon handling).
    pub held: Option<HeldSource>,
    /// The held guns' support and weight targets (the weapon handling).
    pub gun_targets: Option<TargetsFn>,
    /// The pump of the hang swing (the grapple).
    pub hang_pump: Option<HangPump>,
    /// The climb's readers (the physical hands).
    pub climb: Option<ClimbReaders>,
    /// The palms' push off what they press into this tick, world `(x, z)`, or `None` for no push:
    /// the wanted velocity's [`Contributor::PalmPush`], added after the stick's and before the step
    /// drive reads it; called on the simulation thread from the locomotion task's post-call detour
    /// (the physical hands).
    ///
    /// [`Contributor::PalmPush`]: crate::controllers::physics::wanted::Contributor::PalmPush
    pub palm_push: Option<fn() -> Option<Vec2>>,
    /// The refusals of the automatic entry into a vehicle's stunt area below the player, in the
    /// order they are asked (gameplay's refusal table).
    pub auto_attach_blockers: &'static [BlocksAutoAttach],
    /// The observer of a body destruction, called with the world and the bodies' ids before the
    /// world destroys them, on the destroying thread; `ids` points at `count` ids, or is null or
    /// `count` is not positive (the physical hands).
    pub bodies_destroying: Option<unsafe fn(*mut hknpWorld, *const u32, i32)>,
    /// The summary of a hand's palm, logged with each grip press, on the input tick (the physical
    /// hands).
    pub palm_summary: Option<fn(Hand) -> String>,
    /// The wingsuit's take-off from the ground, which the roomscale's frame half ticks after the
    /// seat phase, on the game thread outside the world update (the vehicle handling).
    pub take_off: Option<unsafe fn()>,
    /// The sink a pickup grab hands a reached weapon to (the weapon handling).
    pub pickup_sink: Option<PickupSink>,
    /// The consumer of the deferred magazine detachments, called at the character's native serial
    /// post-physics entry with its native arguments, for every character (the weapon handling).
    pub serial_consumer: Option<unsafe fn(*mut Character, *mut GameObjectUpdateContext)>,
    /// The body mirror's sampler of the attachments on the body (the weapon handling).
    pub attachment_sampler: Option<AttachmentSampler>,
    /// Every source that may declare a grip, in the order their declarations win for one hand:
    /// what the hand holds in its fingers over what it holds on to; at most
    /// [`MAX_SOURCES`] (gameplay's grip table).
    pub grip_order: &'static [GripSource],
    /// The observer of the local player's pose-finalization pass, before the solves and after the
    /// pass (the tooling's diagnostics).
    pub pose_observer: Option<PoseObserver>,
}

impl BodyContributions {
    /// Report every field still empty to `check`, under the reader that reads it.
    pub fn report(&self, check: &mut RegistrationCheck) {
        let fields: [(&'static str, &'static str, bool); 21] = [
            ("aim", "the aim hooks", self.aim_hooks.is_some()),
            (
                "aim",
                "the pre-weapon hooks",
                !self.pre_weapon_hooks.is_empty(),
            ),
            ("aim", "the mounted-gun source", self.mounted.is_some()),
            ("hands", "the target stages", !self.target_stages.is_empty()),
            ("hands", "the hand stages", !self.hand_stages.is_empty()),
            (
                "hands",
                "the weapon observers",
                !self.weapon_observers.is_empty(),
            ),
            (
                "hands",
                "the hand bodies' post-step sample",
                self.post_step_sample.is_some(),
            ),
            ("hands", "the held-item source", self.held.is_some()),
            (
                "hands",
                "the held guns' support and weight targets",
                self.gun_targets.is_some(),
            ),
            ("hands", "the grapple's hang pump", self.hang_pump.is_some()),
            ("body", "the climb's readers", self.climb.is_some()),
            ("body", "the palms' push", self.palm_push.is_some()),
            (
                "character",
                "the auto-attach refusals",
                !self.auto_attach_blockers.is_empty(),
            ),
            (
                "character",
                "the body destruction observer",
                self.bodies_destroying.is_some(),
            ),
            ("grip", "the palm summary", self.palm_summary.is_some()),
            (
                "roomscale",
                "the wingsuit's take-off",
                self.take_off.is_some(),
            ),
            ("held", "the weapon pickup sink", self.pickup_sink.is_some()),
            (
                "character",
                "the magazine serial consumer",
                self.serial_consumer.is_some(),
            ),
            (
                "mirror",
                "the body mirror's attachment sampler",
                self.attachment_sampler.is_some(),
            ),
            (
                "fingers",
                "the grip sources' order, at most the slots the declarations have",
                !self.grip_order.is_empty() && self.grip_order.len() <= MAX_SOURCES,
            ),
            (
                "character",
                "the pose observer",
                self.pose_observer.is_some(),
            ),
        ];
        for (reader, contribution, registered) in fields {
            check.require(reader, contribution, registered);
        }
    }
}

/// Report every contribution `contributions` leaves empty to `check`, then store it for the body's
/// readers. One registration: a second is refused and logged, and the first stands.
pub fn register(contributions: BodyContributions, check: &mut RegistrationCheck) {
    contributions.report(check);
    if CONTRIBUTIONS.set(contributions).is_err() {
        tracing::error!("body: the contributions are already registered; keeping the first");
    }
}

/// The registered contributions; `None` before the registration, in a composition the install's
/// check refuses, or in a test that does not register them.
#[inline]
pub(crate) fn get() -> Option<&'static BodyContributions> {
    CONTRIBUTIONS.get()
}

static CONTRIBUTIONS: OnceLock<BodyContributions> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_record_reports_every_contribution() {
        let mut check = RegistrationCheck::new();
        BodyContributions::default().report(&mut check);
        assert_eq!(check.missing().len(), 21);
    }
}
