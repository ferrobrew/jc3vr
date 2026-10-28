//! Deliberate tear intent and the synchronous native damage boundary for a held door.

use std::{mem::MaybeUninit, num::NonZeroUsize};

use glam::{Mat4, Vec3};
use jc3gi::vehicle::{parts::ConstraintInfo, vehicle::Vehicle};

use crate::controllers::physical_hands::{
    bodies::owner_of,
    doors::{DoorTarget, config::TearConfig, engine},
};

/// The native identity and gesture history of an open door grip.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct TearGrip {
    pub door: DoorTarget,
    pub vehicle_body: u32,
    intent: Intent,
    motion: OpeningMotion,
}

/// Acquisition either retains a checked vehicle identity or confirms that the held body detached.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Acquisition {
    Attached(TearGrip),
    Detached,
}

impl TearGrip {
    /// # Safety
    /// The game thread, outside the physics step. The queued target need not still be live.
    pub unsafe fn take(door: DoorTarget, object: NonZeroUsize, body: u32) -> Option<Acquisition> {
        take_with(door, object, body, &mut NativeAcquisition)
    }

    fn new(door: DoorTarget, vehicle_body: u32) -> Self {
        Self {
            door,
            vehicle_body,
            intent: Intent::default(),
            motion: OpeningMotion::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture(door: DoorTarget, vehicle_body: u32) -> Self {
        Self::new(door, vehicle_body)
    }

    pub fn reset(&mut self) {
        self.intent = Intent::default();
    }

    pub fn gesture(&mut self, input: Gesture, config: &TearConfig) -> bool {
        self.intent.step(input, config)
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Gesture {
    pub dt: f32,
    pub lag_m: f32,
    pub separating_speed_mps: f32,
    pub opening_push_m: f32,
    pub load: f32,
    pub at_limit: bool,
}

/// A native request's outcome, without retaining any native pointer across a tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TearResult {
    Detached,
    Refused,
    Lost,
}

/// The current hinge's opening range and world-space opening tangent at the grip.
///
/// # Safety
/// The held body must prove the door's object alive this tick, outside the physics step.
pub(crate) unsafe fn hinge(grip: &mut TearGrip, anchor: Vec3) -> Option<(f32, f32, Vec3)> {
    if !unsafe { engine::alive(grip.door.vehicle, grip.vehicle_body) } {
        return None;
    }
    let read = unsafe { engine::door(grip.door.vehicle, grip.door.part_index) }?;
    let vehicle = unsafe { &mut *(grip.door.vehicle.get() as *mut Vehicle) };
    let pfx = unsafe { vehicle.m_PfxVehicle.as_mut() }?;
    let mut info = MaybeUninit::<ConstraintInfo>::uninit();
    if !unsafe { pfx.GetConstraintInfoFromPrototype(read.part_hash?, info.as_mut_ptr()) } {
        return None;
    }
    let info = unsafe { info.assume_init() };
    let range = info.m_MaxAngle.abs().max(info.m_MinAngle.abs())
        - info.m_MaxAngle.abs().min(info.m_MinAngle.abs());
    let world = Mat4::from(vehicle.m_WorldMatrixT1);
    let angle = unsafe { engine::open_angle(grip.door.vehicle, grip.door.part_index) };
    let local_anchor = world.inverse().transform_point3(anchor);
    let tangent = world.transform_vector3(grip.motion.observe(angle, local_anchor));
    (range.is_finite() && range > 0.0 && tangent.is_finite() && tangent != Vec3::ZERO)
        .then_some((angle, range, tangent))
}

/// Validate the held native identity and distinguish an externally detached door from a closed,
/// missing, or replaced door body.
///
/// # Safety
/// The held object's body ownership must have been checked this tick.
pub(crate) unsafe fn continuity(grip: &TearGrip, object: NonZeroUsize, body: u32) -> TearResult {
    if !unsafe { engine::alive(grip.door.vehicle, grip.vehicle_body) } {
        return TearResult::Lost;
    }
    let vehicle = unsafe { &mut *(grip.door.vehicle.get() as *mut Vehicle) };
    let Some(pfx) = (unsafe { vehicle.m_PfxVehicle.as_mut() }) else {
        return TearResult::Lost;
    };
    let Some(part) = (unsafe { pfx.GetPartByIndex(grip.door.part_index).as_mut() }) else {
        return TearResult::Lost;
    };
    let native_body = unsafe { part.GetPfxVehiclePartIfAny().as_ref() }.map(|p| p.m_BodyId);
    let same = native_body == Some(body) && unsafe { owner_of(body) } == Some(object);
    let broken = part.m_BrokeOff;
    let open = !broken
        && unsafe { engine::door(grip.door.vehicle, grip.door.part_index) }
            .is_some_and(|d| !d.latched && d.body == Some((object, body)));
    body_continuity(same, broken, open)
}

fn body_continuity(same: bool, broken: bool, open: bool) -> TearResult {
    match (same, broken, open) {
        (true, true, _) => TearResult::Detached,
        (true, false, true) => TearResult::Refused,
        _ => TearResult::Lost,
    }
}

/// Apply one native break-damage request after selective protection checks. No forced work is
/// queued across a parallel phase.
///
/// # Safety
/// The game thread, outside the physics step; the held body's owner was checked this tick.
pub(crate) unsafe fn tear(grip: &TearGrip, object: NonZeroUsize, body: u32) -> TearResult {
    let current = unsafe { continuity(grip, object, body) };
    if current != TearResult::Refused {
        return current;
    }
    let vehicle = unsafe { &mut *(grip.door.vehicle.get() as *mut Vehicle) };
    let Some(pfx) = (unsafe { vehicle.m_PfxVehicle.as_mut() }) else {
        return TearResult::Lost;
    };
    let parallel = pfx.m_InParallelUpdatePhaseCounter > 0;
    let Some(part) = (unsafe { pfx.GetPartByIndex(grip.door.part_index).as_mut() }) else {
        return TearResult::Lost;
    };
    let Some(properties) = (unsafe { part.m_PartProperties.as_ref() }) else {
        return TearResult::Refused;
    };
    let permit = Permit {
        vehicle_protected: vehicle.m_Invulnerable || vehicle.m_InJesusMode,
        part_protected: part.m_Invulnerable,
        authored: properties.m_should_break != 0 && unsafe { part.IsAllowedToBreakOffNow(true) },
        body_survives: properties.m_disappears_when_broken == 0
            && properties.m_has_world_collision_shape != 0,
        valid: part.m_IsValid
            && !part.m_Deleted
            && !part.m_DamagePartOwner.is_null()
            && !part.m_PartDamageType.is_null()
            && properties.m_part_index > 0
            && properties.m_part_index as u32 == grip.door.part_index
            && part.m_Breakable == vehicle.m_PfxVehicle
            && part.m_PartInstanceIndex == grip.door.part_index
            && grip.door.part_index != 0,
        health: part.m_Health,
        parallel,
    };
    if !permit.allowed() {
        return TearResult::Refused;
    }
    // Force ignores the healthy-part restriction. The selective checks retain the vehicle's and
    // part's actual protections; refusing parallel work prevents a stale forced request.
    unsafe { part.DamagePart_ToBreakOff(true, true) };
    unsafe { continuity(grip, object, body) }
}

fn take_with(
    door: DoorTarget,
    object: NonZeroUsize,
    body: u32,
    engine: &mut impl AcquisitionEngine,
) -> Option<Acquisition> {
    if engine.owner(body) != Some(object) {
        return None;
    }
    let vehicle_body = if let Some(captured) = door.vehicle_body {
        if !engine.alive(door.vehicle, captured) {
            return None;
        }
        (engine.vehicle_body(door.vehicle) == Some(captured)).then_some(captured)?
    } else {
        // A queued part may survive its vehicle. Only its current weak reference proves that
        // the saved vehicle pointer can be read; the held body's ownership was checked above.
        if engine.vehicle_of(object) != Some(door.vehicle) {
            return None;
        }
        engine.vehicle_body(door.vehicle)?
    };
    let grip = TearGrip::new(door, vehicle_body);
    match engine.continuity(&grip, object, body) {
        TearResult::Refused => Some(Acquisition::Attached(grip)),
        TearResult::Detached => Some(Acquisition::Detached),
        TearResult::Lost => None,
    }
}

trait AcquisitionEngine {
    fn owner(&mut self, body: u32) -> Option<NonZeroUsize>;
    fn alive(&mut self, vehicle: NonZeroUsize, body: u32) -> bool;
    fn vehicle_of(&mut self, object: NonZeroUsize) -> Option<NonZeroUsize>;
    fn vehicle_body(&mut self, vehicle: NonZeroUsize) -> Option<u32>;
    fn continuity(&mut self, grip: &TearGrip, object: NonZeroUsize, body: u32) -> TearResult;
}

struct NativeAcquisition;

impl AcquisitionEngine for NativeAcquisition {
    fn owner(&mut self, body: u32) -> Option<NonZeroUsize> {
        unsafe { owner_of(body) }
    }

    fn alive(&mut self, vehicle: NonZeroUsize, body: u32) -> bool {
        unsafe { engine::alive(vehicle, body) }
    }

    fn vehicle_of(&mut self, object: NonZeroUsize) -> Option<NonZeroUsize> {
        unsafe { engine::vehicle_of(object) }
    }

    fn vehicle_body(&mut self, vehicle: NonZeroUsize) -> Option<u32> {
        unsafe { engine::vehicle_body(vehicle) }
    }

    fn continuity(&mut self, grip: &TearGrip, object: NonZeroUsize, body: u32) -> TearResult {
        unsafe { continuity(grip, object, body) }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct OpeningMotion {
    previous: Option<(f32, Vec3)>,
    direction: Vec3,
}

impl OpeningMotion {
    fn observe(&mut self, angle: f32, anchor: Vec3) -> Vec3 {
        if !angle.is_finite() || !anchor.is_finite() {
            *self = Self::default();
            return Vec3::ZERO;
        }
        if let Some((before, point)) = self.previous.replace((angle, anchor)) {
            let delta = angle - before;
            let movement = anchor - point;
            if delta.abs() > 0.001 && movement.length() > 0.001 {
                self.direction = (movement * delta.signum()).normalize_or_zero();
            }
        }
        self.direction
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Intent {
    settled_s: f32,
    push_s: f32,
    previous_lag: Option<f32>,
    armed: bool,
}

impl Intent {
    fn step(&mut self, input: Gesture, c: &TearConfig) -> bool {
        let Gesture {
            dt,
            lag_m,
            separating_speed_mps,
            opening_push_m,
            load,
            at_limit,
        } = input;
        if !c.enabled
            || !dt.is_finite()
            || dt <= 0.0
            || dt > 0.1
            || !lag_m.is_finite()
            || !separating_speed_mps.is_finite()
            || !opening_push_m.is_finite()
            || !load.is_finite()
        {
            *self = Self::default();
            return false;
        }
        let previous = self.previous_lag.replace(lag_m);
        if lag_m <= c.reset_lag_m.max(0.0) {
            self.settled_s += dt;
            self.armed |= self.settled_s >= c.settle_s.max(0.05);
        } else if !self.armed {
            self.settled_s = 0.0;
        }
        if !self.armed {
            return false;
        }
        let yank = lag_m >= c.yank_lag_m.max(c.reset_lag_m + 0.05)
            && separating_speed_mps >= c.yank_speed_mps.max(0.5)
            && previous.is_some_and(|p| (lag_m - p) / dt >= c.yank_speed_mps.max(0.5));
        if at_limit
            && load >= c.push_load.clamp(0.1, 1.0)
            && opening_push_m >= c.push_lag_m.max(c.reset_lag_m + 0.05)
        {
            self.push_s += dt;
        } else {
            self.push_s = 0.0;
        }
        if yank || self.push_s >= c.push_hold_s.max(0.05) {
            self.armed = false;
            self.settled_s = 0.0;
            self.push_s = 0.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct Permit {
    vehicle_protected: bool,
    part_protected: bool,
    authored: bool,
    body_survives: bool,
    valid: bool,
    health: i32,
    parallel: bool,
}

impl Permit {
    fn allowed(self) -> bool {
        !self.vehicle_protected
            && !self.part_protected
            && self.authored
            && self.body_survives
            && self.valid
            && (0..i32::MAX).contains(&self.health)
            && !self.parallel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(vehicle_body: Option<u32>) -> DoorTarget {
        DoorTarget {
            vehicle: NonZeroUsize::new(16).unwrap(),
            vehicle_body,
            part_index: 2,
            slot: jc3gi::vehicle::vehicle::DoorSlot::FRONT_LEFT,
            latched: false,
            outward: Vec3::X,
        }
    }

    struct QueuedPart {
        owner: Option<NonZeroUsize>,
        captured_alive: bool,
        weak_vehicle: Option<NonZeroUsize>,
        current_body: Option<u32>,
        result: TearResult,
        proof: bool,
        reads: usize,
    }

    impl QueuedPart {
        fn live() -> Self {
            Self {
                owner: NonZeroUsize::new(8),
                captured_alive: true,
                weak_vehicle: Some(target(None).vehicle),
                current_body: Some(3),
                result: TearResult::Refused,
                proof: false,
                reads: 0,
            }
        }
    }

    impl AcquisitionEngine for QueuedPart {
        fn owner(&mut self, _: u32) -> Option<NonZeroUsize> {
            self.owner
        }
        fn alive(&mut self, _: NonZeroUsize, _: u32) -> bool {
            self.proof = self.captured_alive;
            self.captured_alive
        }
        fn vehicle_of(&mut self, object: NonZeroUsize) -> Option<NonZeroUsize> {
            assert_eq!(self.owner, Some(object));
            self.proof = self.weak_vehicle == Some(target(None).vehicle);
            self.weak_vehicle
        }
        fn vehicle_body(&mut self, _: NonZeroUsize) -> Option<u32> {
            assert!(
                self.proof,
                "vehicle dereferenced before current liveness proof"
            );
            self.reads += 1;
            self.current_body
        }
        fn continuity(&mut self, _: &TearGrip, _: NonZeroUsize, _: u32) -> TearResult {
            assert!(self.proof);
            self.result
        }
    }

    #[test]
    fn queued_surviving_part_never_dereferences_a_destroyed_or_different_vehicle() {
        let object = NonZeroUsize::new(8).unwrap();
        for capture in [Some(3), None] {
            let mut engine = QueuedPart {
                captured_alive: false,
                weak_vehicle: None,
                ..QueuedPart::live()
            };
            assert_eq!(take_with(target(capture), object, 7, &mut engine), None);
            assert_eq!(engine.reads, 0);
        }
        let mut engine = QueuedPart {
            weak_vehicle: NonZeroUsize::new(32),
            ..QueuedPart::live()
        };
        assert_eq!(take_with(target(None), object, 7, &mut engine), None);
        assert_eq!(engine.reads, 0);
        let mut engine = QueuedPart {
            owner: None,
            ..QueuedPart::live()
        };
        assert_eq!(take_with(target(None), object, 7, &mut engine), None);
        assert_eq!(engine.reads, 0);
    }

    #[test]
    fn queued_acquisition_rejects_changed_vehicle_body_and_classifies_external_detachment() {
        let object = NonZeroUsize::new(8).unwrap();
        let mut engine = QueuedPart {
            current_body: Some(4),
            ..QueuedPart::live()
        };
        assert_eq!(take_with(target(Some(3)), object, 7, &mut engine), None);
        for capture in [Some(3), None] {
            let door = target(capture);
            let mut engine = QueuedPart::live();
            assert_eq!(
                take_with(door, object, 7, &mut engine),
                Some(Acquisition::Attached(TearGrip::new(door, 3)))
            );
            let mut engine = QueuedPart {
                result: TearResult::Detached,
                ..QueuedPart::live()
            };
            assert_eq!(
                take_with(door, object, 7, &mut engine),
                Some(Acquisition::Detached)
            );
            let mut engine = QueuedPart {
                result: TearResult::Lost,
                ..QueuedPart::live()
            };
            assert_eq!(take_with(door, object, 7, &mut engine), None);
        }
    }

    fn quiet() -> Gesture {
        Gesture {
            dt: 0.02,
            lag_m: 0.01,
            separating_speed_mps: 0.0,
            opening_push_m: 0.0,
            load: 0.0,
            at_limit: false,
        }
    }

    fn armed() -> Intent {
        let mut intent = Intent::default();
        for _ in 0..15 {
            assert!(!intent.step(quiet(), &TearConfig::new()));
        }
        intent
    }

    #[test]
    fn native_body_loss_change_and_relatch_never_count_as_detachment() {
        assert_eq!(body_continuity(true, false, true), TearResult::Refused);
        assert_eq!(body_continuity(true, true, false), TearResult::Detached);
        assert_eq!(body_continuity(false, true, false), TearResult::Lost);
        assert_eq!(body_continuity(false, false, true), TearResult::Lost);
        assert_eq!(body_continuity(true, false, false), TearResult::Lost);
    }

    #[test]
    fn push_direction_requires_observed_swing_and_tracks_opening_not_closing() {
        let mut motion = OpeningMotion::default();
        assert_eq!(motion.observe(1.0, Vec3::ZERO), Vec3::ZERO);
        assert_eq!(motion.observe(1.0, Vec3::ZERO), Vec3::ZERO);
        assert_eq!(motion.observe(1.1, Vec3::X * 0.1), Vec3::X);
        assert_eq!(motion.observe(1.0, Vec3::ZERO), Vec3::X);
        assert_eq!(motion.observe(f32::NAN, Vec3::ZERO), Vec3::ZERO);
    }

    #[test]
    fn opening_pull_and_unsettled_handoff_never_tear() {
        let mut intent = Intent::default();
        for lag in [0.06, 0.10, 0.18, 0.25] {
            assert!(!intent.step(
                Gesture {
                    lag_m: lag,
                    separating_speed_mps: 4.0,
                    ..quiet()
                },
                &TearConfig::new()
            ));
        }
    }

    #[test]
    fn sharp_yank_requires_new_lag_and_does_not_repeat() {
        let mut intent = armed();
        let yank = Gesture {
            lag_m: 0.22,
            separating_speed_mps: 3.0,
            ..quiet()
        };
        assert!(intent.step(yank, &TearConfig::new()));
        assert!(!intent.step(yank, &TearConfig::new()));
        assert!(!armed().step(
            Gesture {
                separating_speed_mps: 0.5,
                ..yank
            },
            &TearConfig::new()
        ));
    }

    #[test]
    fn push_requires_limit_direction_and_duration() {
        let push = Gesture {
            lag_m: 0.16,
            opening_push_m: 0.16,
            load: 0.9,
            at_limit: true,
            ..quiet()
        };
        let c = TearConfig::new();
        let mut intent = armed();
        for _ in 0..8 {
            assert!(!intent.step(push, &c));
        }
        assert!(intent.step(Gesture { dt: 0.03, ..push }, &c));
        for bad in [
            Gesture { load: 0.2, ..push },
            Gesture {
                at_limit: false,
                ..push
            },
            Gesture {
                opening_push_m: -0.16,
                ..push
            },
            Gesture {
                opening_push_m: 0.05,
                ..push
            },
        ] {
            let mut intent = armed();
            for _ in 0..30 {
                assert!(!intent.step(bad, &c));
            }
        }
    }

    #[test]
    fn disabled_or_stale_tracking_resets_intent() {
        let mut intent = armed();
        assert!(!intent.step(Gesture { dt: 0.5, ..quiet() }, &TearConfig::new()));
        assert!(!intent.armed);
        let mut c = TearConfig::new();
        c.enabled = false;
        let mut intent = armed();
        assert!(!intent.step(quiet(), &c));
        assert!(!intent.armed);
    }

    #[test]
    fn healthy_authored_parts_are_allowed_but_protections_and_invalid_parts_are_not() {
        let allowed = Permit {
            vehicle_protected: false,
            part_protected: false,
            authored: true,
            body_survives: true,
            valid: true,
            health: 100,
            parallel: false,
        };
        assert!(allowed.allowed());
        for refused in [
            Permit {
                vehicle_protected: true,
                ..allowed
            },
            Permit {
                part_protected: true,
                ..allowed
            },
            Permit {
                authored: false,
                ..allowed
            },
            Permit {
                body_survives: false,
                ..allowed
            },
            Permit {
                valid: false,
                ..allowed
            },
            Permit {
                health: -1,
                ..allowed
            },
            Permit {
                health: i32::MAX,
                ..allowed
            },
            Permit {
                parallel: true,
                ..allowed
            },
        ] {
            assert!(!refused.allowed());
        }
    }
}
