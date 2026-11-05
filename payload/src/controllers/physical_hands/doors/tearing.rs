//! Deliberate tear intent and the synchronous native damage boundary for a held door.

use std::{fmt, mem::MaybeUninit, num::NonZeroUsize};

use glam::{Mat4, Vec3};
use jc3gi::vehicle::{parts::ConstraintInfo, vehicle::Vehicle};

use crate::controllers::{
    diagnostics::note_diagnostic,
    physical_hands::{
        bodies::owner_of,
        doors::{DoorTarget, config::TearTuning, engine},
    },
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

/// Why a held door's identity could not be established.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum AcquireRefusal {
    /// The held body no longer belongs to the door's object.
    BodyReowned,
    /// Neither the captured vehicle body nor the part's own vehicle reference proves the vehicle.
    VehicleUnproven,
    /// The vehicle has no live body of its own.
    NoVehicleBody,
    /// The door's part no longer hangs by that body.
    ContinuityLost,
}

impl fmt::Display for AcquireRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("physical hands: doors: ")?;
        f.write_str(match self {
            Self::BodyReowned => "the held body no longer belongs to the door",
            Self::VehicleUnproven => "nothing proves the door's vehicle alive",
            Self::NoVehicleBody => "the door's vehicle has no live body",
            Self::ContinuityLost => "the door's part no longer hangs by the held body",
        })
    }
}

impl std::error::Error for AcquireRefusal {}

impl TearGrip {
    /// # Safety
    /// The game thread, outside the physics step. The queued target need not still be live.
    pub unsafe fn take(
        door: DoorTarget,
        object: NonZeroUsize,
        body: u32,
    ) -> Result<Acquisition, AcquireRefusal> {
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

    /// Step the grip's tear intent, returning the gesture that asks for a tear this tick.
    pub fn gesture(&mut self, input: Gesture, config: &TearTuning) -> Option<TearGesture> {
        self.intent.step(input, config)
    }
}

/// The gestures that tear a door off once its grip has settled.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TearGesture {
    /// The lag grew sharply above the settled lag with the hand pulling away.
    Yank,
    /// The lag held far above the settled lag: a pull the door would not follow.
    HardPull,
    /// A sustained, loaded push past the hinge's opening limit.
    LimitPush,
}

impl TearGesture {
    pub const fn label(self) -> &'static str {
        match self {
            TearGesture::Yank => "a yank",
            TearGesture::HardPull => "a hard pull",
            TearGesture::LimitPush => "a push past the opening limit",
        }
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
    /// Whether the lag runs mostly along a swing the hinge leaves free: the door would follow the
    /// hand there.
    pub free_swing: bool,
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
        log_refusal(grip, TearRefusal::Invalid);
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
    if let Some(refusal) = permit.refusal() {
        log_refusal(grip, refusal);
        return TearResult::Refused;
    }
    // Force ignores the healthy-part restriction. The selective checks retain the vehicle's and
    // part's actual protections; refusing parallel work prevents a stale forced request.
    unsafe { part.DamagePart_ToBreakOff(true, true) };
    let result = unsafe { continuity(grip, object, body) };
    tracing::info!(
        target: "controllers",
        slot = ?grip.door.slot,
        part = grip.door.part_index,
        ?result,
        "physical hands: doors: the game was asked to tear a door off"
    );
    result
}

/// Log that a tear of the door `grip` holds was refused, and why.
fn log_refusal(grip: &TearGrip, refusal: TearRefusal) {
    tracing::info!(
        target: "controllers",
        slot = ?grip.door.slot,
        part = grip.door.part_index,
        "physical hands: doors: a tear was refused: {}",
        refusal.label()
    );
    note_diagnostic("door", None, "a tear was refused", || {
        Some(format!(
            "{}; slot {:?}, part {}",
            refusal.label(),
            grip.door.slot,
            grip.door.part_index
        ))
    });
}

fn take_with(
    door: DoorTarget,
    object: NonZeroUsize,
    body: u32,
    engine: &mut impl AcquisitionEngine,
) -> Result<Acquisition, AcquireRefusal> {
    if engine.owner(body) != Some(object) {
        return Err(AcquireRefusal::BodyReowned);
    }
    // The body a reach captured proves the vehicle alive while it still owns it. Undoing a latch
    // can leave the captured body unproven (the reach may have hit another of the vehicle's
    // bodies, or the vehicle's body may change with the loosened part), so the part's own weak
    // reference to its vehicle, read through the checked held body, proves it as well; a queued
    // part may survive its vehicle, and only that reference then shows the saved pointer readable.
    let captured = door
        .vehicle_body
        .is_some_and(|captured| engine.alive(door.vehicle, captured));
    if !captured && engine.vehicle_of(object) != Some(door.vehicle) {
        return Err(AcquireRefusal::VehicleUnproven);
    }
    let vehicle_body = engine
        .vehicle_body(door.vehicle)
        .ok_or(AcquireRefusal::NoVehicleBody)?;
    let grip = TearGrip::new(door, vehicle_body);
    match engine.continuity(&grip, object, body) {
        TearResult::Refused => Ok(Acquisition::Attached(grip)),
        TearResult::Detached => Ok(Acquisition::Detached),
        TearResult::Lost => Err(AcquireRefusal::ContinuityLost),
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

/// A grip's tear intent: whether its lag has settled, at what lag, and how long a hard pull or a
/// push at the limit has lasted.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Intent {
    /// How long the lag has held steady, seconds.
    steady_s: f32,
    previous_lag: Option<f32>,
    /// The lag the grip settled at, once it has: the gestures measure from it.
    settled_lag: Option<f32>,
    /// How long a hard pull has held its lag, seconds.
    wrench_s: f32,
    push_s: f32,
}

impl Intent {
    fn step(&mut self, input: Gesture, c: &TearTuning) -> Option<TearGesture> {
        let Gesture {
            dt,
            lag_m,
            separating_speed_mps,
            opening_push_m,
            load,
            at_limit,
            free_swing,
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
            return None;
        }
        let rate = self.previous_lag.replace(lag_m).map(|p| (lag_m - p) / dt);
        if rate.is_some_and(|r| r.abs() <= c.settle_speed_mps.max(0.01)) {
            self.steady_s += dt;
        } else {
            self.steady_s = 0.0;
        }
        // A grip that holds steady settles, or settles again, at its lag; one that eases off
        // settles lower at once, so a gesture is measured from the hand's resting lag.
        if self.steady_s >= c.settle_s.max(0.05) {
            self.settled_lag = Some(lag_m);
        }
        let settled = self.settled_lag.as_mut()?;
        *settled = settled.min(lag_m);
        let above = lag_m - *settled;
        let yank_speed = c.yank_speed_mps.max(0.3);
        let yank = !free_swing
            && above >= c.yank_lag_m.max(0.05)
            && separating_speed_mps >= yank_speed
            && rate.is_some_and(|r| r >= yank_speed);
        if !free_swing && above >= c.wrench_lag_m.max(c.yank_lag_m) {
            self.wrench_s += dt;
        } else {
            self.wrench_s = 0.0;
        }
        if at_limit
            && load >= c.push_load.clamp(0.1, 1.0)
            && opening_push_m >= c.push_lag_m.max(0.05)
        {
            self.push_s += dt;
        } else {
            self.push_s = 0.0;
        }
        let gesture = if yank {
            Some(TearGesture::Yank)
        } else if self.wrench_s >= c.wrench_s.max(0.0) && self.wrench_s > 0.0 {
            Some(TearGesture::HardPull)
        } else if self.push_s >= c.push_hold_s.max(0.05) {
            Some(TearGesture::LimitPush)
        } else {
            None
        };
        if gesture.is_some() {
            // Each attempt asks the grip to settle again.
            *self = Self::default();
        }
        gesture
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
    #[cfg(test)]
    fn allowed(self) -> bool {
        self.refusal().is_none()
    }

    /// The first reason the tear is refused, if any.
    fn refusal(self) -> Option<TearRefusal> {
        if self.vehicle_protected {
            Some(TearRefusal::VehicleProtected)
        } else if self.part_protected {
            Some(TearRefusal::PartProtected)
        } else if !self.authored {
            Some(TearRefusal::NotDetachable)
        } else if !self.body_survives {
            Some(TearRefusal::BodyWouldVanish)
        } else if !self.valid {
            Some(TearRefusal::Invalid)
        } else if !(0..i32::MAX).contains(&self.health) {
            Some(TearRefusal::Health)
        } else if self.parallel {
            Some(TearRefusal::Parallel)
        } else {
            None
        }
    }
}

/// Why a tear was refused.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TearRefusal {
    VehicleProtected,
    PartProtected,
    NotDetachable,
    BodyWouldVanish,
    Invalid,
    Health,
    Parallel,
}

impl TearRefusal {
    const fn label(self) -> &'static str {
        match self {
            TearRefusal::VehicleProtected => "the vehicle is invulnerable",
            TearRefusal::PartProtected => "the door is invulnerable",
            TearRefusal::NotDetachable => {
                "the door is not authored to break off, or not allowed to now"
            }
            TearRefusal::BodyWouldVanish => "the door would disappear rather than come off whole",
            TearRefusal::Invalid => "the door's part is not the one gripped, or not valid",
            TearRefusal::Health => "the door's health is out of range",
            TearRefusal::Parallel => "the vehicle is in its parallel update",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::physical_hands::doors::config::TEARING;

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
            assert_eq!(
                take_with(target(capture), object, 7, &mut engine),
                Err(AcquireRefusal::VehicleUnproven)
            );
            assert_eq!(engine.reads, 0);
        }
        let mut engine = QueuedPart {
            captured_alive: false,
            weak_vehicle: NonZeroUsize::new(32),
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(target(Some(3)), object, 7, &mut engine),
            Err(AcquireRefusal::VehicleUnproven)
        );
        assert_eq!(engine.reads, 0);
        let mut engine = QueuedPart {
            weak_vehicle: NonZeroUsize::new(32),
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(target(None), object, 7, &mut engine),
            Err(AcquireRefusal::VehicleUnproven)
        );
        assert_eq!(engine.reads, 0);
        let mut engine = QueuedPart {
            owner: None,
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(target(None), object, 7, &mut engine),
            Err(AcquireRefusal::BodyReowned)
        );
        assert_eq!(engine.reads, 0);
    }

    #[test]
    fn an_unlatched_door_keeps_its_grip_when_the_captured_vehicle_body_changes() {
        let object = NonZeroUsize::new(8).unwrap();
        let door = target(Some(3));
        // The captured body still names the vehicle, but the vehicle's own body is another.
        let mut engine = QueuedPart {
            current_body: Some(4),
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(door, object, 7, &mut engine),
            Ok(Acquisition::Attached(TearGrip::new(door, 4)))
        );
        // The captured body no longer names the vehicle; the part's weak reference proves it.
        let mut engine = QueuedPart {
            captured_alive: false,
            current_body: Some(4),
            ..QueuedPart::live()
        };
        assert_eq!(
            take_with(door, object, 7, &mut engine),
            Ok(Acquisition::Attached(TearGrip::new(door, 4)))
        );
    }

    #[test]
    fn queued_acquisition_classifies_external_detachment_and_lost_continuity() {
        let object = NonZeroUsize::new(8).unwrap();
        for capture in [Some(3), None] {
            let door = target(capture);
            let mut engine = QueuedPart::live();
            assert_eq!(
                take_with(door, object, 7, &mut engine),
                Ok(Acquisition::Attached(TearGrip::new(door, 3)))
            );
            let mut engine = QueuedPart {
                result: TearResult::Detached,
                ..QueuedPart::live()
            };
            assert_eq!(
                take_with(door, object, 7, &mut engine),
                Ok(Acquisition::Detached)
            );
            let mut engine = QueuedPart {
                result: TearResult::Lost,
                ..QueuedPart::live()
            };
            assert_eq!(
                take_with(door, object, 7, &mut engine),
                Err(AcquireRefusal::ContinuityLost)
            );
            let mut engine = QueuedPart {
                current_body: None,
                ..QueuedPart::live()
            };
            assert_eq!(
                take_with(door, object, 7, &mut engine),
                Err(AcquireRefusal::NoVehicleBody)
            );
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
            free_swing: false,
        }
    }

    fn tears(intent: &mut Intent, gesture: Gesture, c: &TearTuning) -> bool {
        intent.step(gesture, c).is_some()
    }

    fn armed() -> Intent {
        let mut intent = Intent::default();
        for _ in 0..15 {
            assert!(!tears(&mut intent, quiet(), &TEARING));
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
            assert!(!tears(
                &mut intent,
                Gesture {
                    lag_m: lag,
                    separating_speed_mps: 4.0,
                    ..quiet()
                },
                &TEARING
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
        assert!(tears(&mut intent, yank, &TEARING));
        assert!(!tears(&mut intent, yank, &TEARING));
        assert!(!tears(
            &mut armed(),
            Gesture {
                separating_speed_mps: 0.5,
                ..yank
            },
            &TEARING
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
        let c = TEARING;
        let mut intent = armed();
        for _ in 0..8 {
            assert!(!tears(&mut intent, push, &c));
        }
        assert!(tears(&mut intent, Gesture { dt: 0.03, ..push }, &c));
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
                assert!(!tears(&mut intent, bad, &c));
            }
        }
    }

    #[test]
    fn disabled_or_stale_tracking_resets_intent() {
        let mut intent = armed();
        assert!(!tears(
            &mut intent,
            Gesture { dt: 0.5, ..quiet() },
            &TEARING
        ));
        assert!(intent.settled_lag.is_none());
        let mut c = TEARING;
        c.enabled = false;
        let mut intent = armed();
        assert!(!tears(&mut intent, quiet(), &c));
        assert!(intent.settled_lag.is_none());
    }

    /// The session's attempt: a door at rest, held through a joint, pulled at under a metre per
    /// second. The pull grows the lag as fast as the hand moves, so it is a yank.
    #[test]
    fn a_firm_pull_on_a_door_that_does_not_give_tears_it() {
        let mut intent = armed();
        let mut lag = 0.01;
        let mut torn = None;
        for _ in 0..40 {
            lag += 0.8 * 0.02;
            let gesture = Gesture {
                lag_m: lag,
                separating_speed_mps: 0.8,
                ..quiet()
            };
            if let Some(kind) = intent.step(gesture, &TEARING) {
                torn = Some((kind, lag));
                break;
            }
        }
        let (kind, lag) = torn.expect("the pull tears the door");
        assert_eq!(kind, TearGesture::Yank);
        assert!(lag < 0.25, "{lag}");
    }

    /// A heavy door hanging well off the hand settles at that lag, and the gestures measure from it.
    #[test]
    fn a_grip_settled_far_off_the_hand_measures_from_its_settled_lag() {
        let mut intent = Intent::default();
        let hanging = Gesture {
            lag_m: 0.12,
            ..quiet()
        };
        for _ in 0..15 {
            assert!(!tears(&mut intent, hanging, &TEARING));
        }
        assert_eq!(intent.settled_lag, Some(0.12));
        // The old absolute threshold would read this as a yank already; above the settled lag it
        // is not.
        let small = Gesture {
            lag_m: 0.2,
            separating_speed_mps: 3.0,
            ..quiet()
        };
        assert!(!tears(&mut intent, small, &TEARING));
        let mut intent = Intent::default();
        for _ in 0..15 {
            tears(&mut intent, hanging, &TEARING);
        }
        assert!(tears(
            &mut intent,
            Gesture {
                lag_m: 0.32,
                ..small
            },
            &TEARING
        ));
    }

    /// A slow pull the door will not follow tears once its lag has held far enough above the
    /// settled lag, short of where the hand's body lets go of a joined door.
    #[test]
    fn a_slow_hard_pull_tears_and_a_slower_creep_settles_instead() {
        let pull = |speed: f32| {
            let mut intent = armed();
            let mut lag = 0.01;
            for tick in 0..200 {
                lag += speed * 0.02;
                let gesture = Gesture {
                    lag_m: lag,
                    separating_speed_mps: speed,
                    ..quiet()
                };
                if let Some(kind) = intent.step(gesture, &TEARING) {
                    return Some((kind, lag, tick));
                }
            }
            None
        };
        let (kind, lag, _) = pull(0.3).expect("a slow hard pull tears");
        assert_eq!(kind, TearGesture::HardPull);
        assert!(lag < 0.4, "{lag}");
        assert_eq!(
            pull(0.1),
            None,
            "a creep under the settling speed resettles"
        );
    }

    /// A pull the door would swing with does not tear it, unless the hinge holds it there.
    #[test]
    fn a_pull_along_a_free_swing_never_tears() {
        let mut intent = armed();
        let mut lag: f32 = 0.01;
        for _ in 0..60 {
            lag += 1.5 * 0.02;
            let gesture = Gesture {
                lag_m: lag.min(0.5),
                separating_speed_mps: 1.5,
                free_swing: true,
                ..quiet()
            };
            assert!(!tears(&mut intent, gesture, &TEARING));
        }
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
