//! Tearing a held door off its hinge: the gestures a settled open grip makes to ask for a tear
//! ([`TearGesture`]), and the game's own break-off damage, applied at once on the game thread when
//! the vehicle and the part allow it.

use std::{fmt, mem::MaybeUninit, num::NonZeroUsize};

use glam::{Mat4, Vec3};
use jc3gi::vehicle::{parts::ConstraintInfo, vehicle::Vehicle};
use jc3vr_body::controllers::{
    grab::world_reach::DoorTarget,
    physics::{bodies::owner_of, vehicles},
    telemetry::report,
};

use crate::controllers::physical_hands::doors::{engine, tuning::TearTuning};

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
    /// The tear grip of a hand that took `door` by its own body `body`, owned by `object`, or the
    /// news that the door has already come off.
    ///
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
    pub(crate) lag_direction: Vec3,
    pub(crate) separating_speed_mps: f32,
    pub(crate) opening_push_m: f32,
    pub load: f32,
    pub(crate) at_limit: bool,
    /// Whether the lag runs mostly along a swing the hinge leaves free: the door would follow the
    /// hand there.
    pub(crate) free_swing: bool,
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
    if !unsafe { vehicles::owns_body(grip.door.vehicle, grip.vehicle_body) } {
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
    if !unsafe { vehicles::owns_body(grip.door.vehicle, grip.vehicle_body) } {
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

/// Ask the game to break the held door off with forced damage
/// ([`PartInstance::DamagePart_ToBreakOff`](jc3gi::vehicle::parts::PartInstance::DamagePart_ToBreakOff)),
/// unless the vehicle or the part is protected, the part is not authored to break off whole, or
/// the vehicle is in its parallel update, when the request would be deferred past this tick.
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
        report_refusal(grip, TearRefusal::Invalid);
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
        report_refusal(grip, refusal);
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

/// Report that a tear of the door `grip` holds was refused, and why.
fn report_refusal(grip: &TearGrip, refusal: TearRefusal) {
    report("door", None, "a tear was refused", || {
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
        unsafe { vehicles::owns_body(vehicle, body) }
    }

    fn vehicle_of(&mut self, object: NonZeroUsize) -> Option<NonZeroUsize> {
        unsafe { vehicles::vehicle_of(object) }
    }

    fn vehicle_body(&mut self, vehicle: NonZeroUsize) -> Option<u32> {
        unsafe { vehicles::vehicle_body(vehicle) }
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
    previous: Option<(f32, Vec3)>,
    at_limit: Option<bool>,
    direction: Option<Vec3>,
    settled_push: f32,
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
            lag_direction,
            separating_speed_mps,
            opening_push_m,
            load,
            at_limit,
            free_swing,
        } = input;
        if !dt.is_finite()
            || dt <= 0.0
            || dt > c.max_dt_s
            || !lag_m.is_finite()
            || lag_m < 0.0
            || !lag_direction.is_finite()
            || !separating_speed_mps.is_finite()
            || !opening_push_m.is_finite()
            || !load.is_finite()
        {
            *self = Self::default();
            return None;
        }
        if free_swing {
            *self = Self::default();
            return None;
        }
        let direction = lag_direction.normalize_or_zero();
        let changed_direction = self.direction.is_some_and(|before| {
            direction != Vec3::ZERO && before.dot(direction) < c.direction_cos.clamp(0.0, 1.0)
        });
        if self.at_limit.is_some_and(|before| before != at_limit) || changed_direction {
            *self = Self::default();
        }
        self.at_limit = Some(at_limit);
        if self.direction.is_none() && direction != Vec3::ZERO {
            self.direction = Some(direction);
        }
        let previous = self.previous.replace((lag_m, direction));
        let rate = previous.map(|(lag, _)| (lag_m - lag) / dt);
        let settle_speed = c.settle_speed_mps.max(0.01);
        let steady = rate.is_some_and(|r| r.abs() <= settle_speed)
            && separating_speed_mps.abs() <= settle_speed
            && previous.is_some_and(|(_, before)| {
                (direction - before).length() * lag_m / dt <= settle_speed
            });
        if steady {
            self.steady_s += dt;
        } else {
            self.steady_s = 0.0;
        }
        let above = self.settled_lag.map_or(0.0, |settled| lag_m - settled);
        let opening_aligned =
            lag_m > 0.0 && opening_push_m >= lag_m * c.free_swing_share.clamp(0.0, 1.0);
        let pull = self.settled_lag.is_some() && !(at_limit && opening_aligned);
        let yank_speed = c.yank_speed_mps.max(0.3);
        let yank = pull
            && above >= c.yank_lag_m.max(0.05)
            && separating_speed_mps >= yank_speed
            && rate.is_some_and(|r| r >= yank_speed);
        if pull && above >= c.wrench_lag_m.max(c.yank_lag_m) {
            self.wrench_s += dt;
        } else {
            self.wrench_s = 0.0;
        }
        if at_limit
            && opening_aligned
            && self.settled_lag.is_some()
            && load >= c.push_load.clamp(0.1, 1.0)
            && opening_push_m - self.settled_push >= c.push_lag_m.max(0.05)
        {
            self.push_s += dt;
        } else {
            self.push_s = 0.0;
        }
        if self.wrench_s == 0.0 && self.push_s == 0.0 {
            if self.steady_s >= c.settle_s.max(0.05) {
                self.settled_lag = Some(lag_m);
                self.settled_push = opening_push_m;
                self.direction = (direction != Vec3::ZERO).then_some(direction);
            } else if let Some(settled) = self.settled_lag.as_mut() {
                *settled = settled.min(lag_m);
                self.settled_push = self.settled_push.min(opening_push_m);
            }
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
mod tests;
