//! The wheel and the handlebars fitted to the vehicle the player drives, on the input tick.
//!
//! The vehicle's parts are read from its breakable physics body
//! ([`Vehicle::m_PfxVehicle`](jc3gi::vehicle::vehicle::Vehicle::m_PfxVehicle)): the steering wheel
//! part's model-space box, through its root-space transform and the vehicle's world transform,
//! places the wheel ([`crate::controllers::vehicles::wheel`]), and a steering fork's places the
//! handlebars and offers them in place of the wheel. Both are carried into the cockpit frame each
//! tick, so they ride the vehicle rather than the character's animated head, which the cockpit
//! frame is pinned to.
//!
//! A vehicle without a usable wheel part falls back to the configured spec, which is given in the
//! cockpit frame. So that it too rides the vehicle, the spec's wheel is carried into the vehicle's
//! space once the driver has settled into the seat, and held there for as long as the driver stays
//! in that vehicle.

use std::{
    ffi::CStr,
    time::{Duration, Instant},
};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    types::math::Vector3,
    vehicle::{
        parts::{AdfHashString, PartInstance, PfxBreakable},
        vehicle::Vehicle,
    },
};
use smol_str::SmolStr;

use crate::controllers::{
    mode::VehicleSeat,
    vehicles::{
        config::PhysicalControlsConfig,
        wheel::{
            self, BarsFrame, CockpitTransform, PartWheel, WheelFrame, WheelPartView, WheelSource,
        },
        wheel_render::RenderWheel,
    },
};

/// The wheel and the handlebars as fitted this tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// The wheel in the cockpit frame.
    pub wheel: WheelFrame,
    /// The handlebars in the cockpit frame.
    pub bars: BarsFrame,
    /// Whether the vehicle steers by a steering fork, so its driver gets the handlebars.
    pub fork: bool,
    /// The vehicle's wheel part to turn with the steering, when it was found and is whole.
    pub render: Option<RenderWheel>,
    /// The vehicle's own steering, `None` outside a vehicle.
    pub steering_turn: Option<f32>,
    pub readout: Option<FitReadout>,
}

/// What the fit found, for the debug UI.
#[derive(Debug, Clone, PartialEq)]
pub struct FitReadout {
    pub vehicle_type_hash: u32,
    pub wheel_part: Option<SmolStr>,
    pub fork_part: Option<SmolStr>,
    pub source: WheelSource,
    /// The wheel in its part's own space, when it came from the part or an override.
    pub part_wheel: Option<PartWheel>,
    /// Whether the fallback wheel is held in the vehicle's space yet.
    pub fallback_latched: bool,
    /// Whether the handlebars came from the steering fork.
    pub bars_from_vehicle: bool,
}

/// The fallback wheel held in the vehicle's space, per vehicle.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FallbackLatch {
    vehicle: usize,
    since: Instant,
    local: Option<WheelFrame>,
}

/// Fit the wheel and the handlebars for `seat`: to the vehicle where the local player drives one,
/// otherwise to the specs. `latch` carries the fallback wheel between ticks.
pub fn fit(
    config: &PhysicalControlsConfig,
    seat: Option<VehicleSeat>,
    xf: &CockpitTransform,
    latch: &mut Option<FallbackLatch>,
) -> Fit {
    let specs = Fit {
        wheel: WheelFrame::from_spec(&config.wheel),
        bars: BarsFrame::from_spec(&config.handlebars),
        fork: false,
        render: None,
        steering_turn: None,
        readout: None,
    };
    if !seat.is_some_and(|s| s.driver) {
        *latch = None;
        return specs;
    }
    // SAFETY: the input tick runs on the main thread outside the world update, where the local
    // character and its vehicle are stable.
    let Some(vehicle) = (unsafe {
        Character::GetLocalPlayerCharacter()
            .as_ref()
            .and_then(|c| c.GetVehiclePtr().as_ref())
    }) else {
        *latch = None;
        return specs;
    };
    let address = vehicle as *const Vehicle as usize;
    if latch.is_none_or(|l| l.vehicle != address) {
        *latch = Some(FallbackLatch {
            vehicle: address,
            since: Instant::now(),
            local: None,
        });
    }
    // SAFETY: as above.
    unsafe { fit_vehicle(config, vehicle, xf, latch, specs) }
}

/// How long after entering a vehicle the fallback wheel is carried into its space: by then the
/// driver has settled into the seat.
const SETTLE: Duration = Duration::from_millis(1500);

/// # Safety
///
/// `vehicle` must be the live vehicle the local player drives, on the main thread outside the
/// world update.
unsafe fn fit_vehicle(
    config: &PhysicalControlsConfig,
    vehicle: &Vehicle,
    xf: &CockpitTransform,
    latch: &mut Option<FallbackLatch>,
    specs: Fit,
) -> Fit {
    let world = Mat4::from(vehicle.m_WorldMatrixT1);
    let parts = unsafe { vehicle.m_PfxVehicle.as_ref() }
        .map(|pfx| unsafe { find_parts(pfx) })
        .unwrap_or_default();

    let wheel_view = parts.wheel.as_ref().map(|p| WheelPartView {
        name: &p.name,
        bounds: p.bounds,
        broken_off: p.broken_off,
    });
    let (part_wheel, mut source) = wheel::choose_wheel(wheel_view, config.from_vehicle);
    let fitted = part_wheel.zip(parts.wheel.as_ref()).and_then(|(w, p)| {
        let frame = xf.wheel(world * p.root, &w)?;
        Some((frame, w, p))
    });
    let mut fallback_latched = false;
    let (wheel, render) = match fitted {
        Some((frame, w, p)) => (
            frame,
            Some(RenderWheel {
                vehicle: vehicle as *const Vehicle as usize,
                part_index: p.index,
                properties: p.properties,
                hub: w.hub,
                axis: w.axis(),
            }),
        ),
        None => {
            if part_wheel.is_some() {
                source = WheelSource::Fallback(wheel::Fallback::NoModel);
            }
            let frame = match latch.as_mut().filter(|_| config.from_vehicle) {
                Some(l) => {
                    if l.local.is_none() && l.since.elapsed() >= SETTLE {
                        l.local = xf.wheel_into(world, &specs.wheel);
                    }
                    fallback_latched = l.local.is_some();
                    l.local
                        .and_then(|local| xf.wheel_from(world, &local))
                        .unwrap_or(specs.wheel)
                }
                None => specs.wheel,
            };
            (frame, None)
        }
    };

    let fork_bars = parts
        .fork
        .as_ref()
        .filter(|p| config.from_vehicle && !p.broken_off)
        .and_then(|p| {
            let (centre, half_width) = wheel::bars_from_box(p.bounds?.0, p.bounds?.1)?;
            xf.bars(world * p.rest_root, centre, half_width)
        });

    Fit {
        wheel,
        bars: fork_bars.unwrap_or(specs.bars),
        fork: parts.fork.is_some(),
        render,
        steering_turn: Some(vehicle.m_SteeringTurn).filter(|t| t.is_finite()),
        readout: Some(FitReadout {
            vehicle_type_hash: vehicle.m_VehicleTypeNameHash,
            wheel_part: parts.wheel.as_ref().map(|p| p.name.clone()),
            fork_part: parts.fork.as_ref().map(|p| p.name.clone()),
            source,
            part_wheel: fitted.map(|(_, w, _)| w),
            fallback_latched,
            bars_from_vehicle: fork_bars.is_some(),
        }),
    }
}

/// A vehicle part as read for the fit.
#[derive(Debug, Clone)]
struct PartRead {
    index: u32,
    /// The part's [`PartInstance::m_PartProperties`], which identifies it across ticks.
    properties: usize,
    name: SmolStr,
    bounds: Option<(Vec3, Vec3)>,
    broken_off: bool,
    /// The part's root-space transform.
    root: Mat4,
    /// The part's root-space transform at rest: its parent space offset under its parent's
    /// root-space transform, without its hinge's turn.
    rest_root: Mat4,
}

#[derive(Debug, Clone, Default)]
struct Parts {
    wheel: Option<PartRead>,
    fork: Option<PartRead>,
}

/// The vehicle's steering wheel and steering fork parts, the first of each among its valid parts.
///
/// # Safety
///
/// `pfx` must be the live vehicle's physics body, on the main thread outside the world update.
unsafe fn find_parts(pfx: &PfxBreakable) -> Parts {
    let mut parts = Parts::default();
    if pfx.m_PartInstances.is_null() || pfx.m_ValidPartIndices.is_null() {
        return parts;
    }
    let indices = unsafe {
        std::slice::from_raw_parts(pfx.m_ValidPartIndices, pfx.m_NumberOfValidParts as usize)
    };
    for &index in indices {
        let instance = unsafe { &*pfx.m_PartInstances.add(index as usize) };
        let Some(properties) = (unsafe { instance.m_PartProperties.as_ref() }) else {
            continue;
        };
        let Some(name) = (unsafe { adf_str(&properties.m_name) }) else {
            continue;
        };
        let behavior = unsafe { adf_str(&properties.m_behavior) }.unwrap_or("");
        let slot = if wheel::is_wheel_part(name) {
            &mut parts.wheel
        } else if wheel::is_fork_part(name, behavior) {
            &mut parts.fork
        } else {
            continue;
        };
        if slot.is_none() {
            *slot =
                Some(unsafe { read_part(instance, index, properties as *const _ as usize, name) });
        }
        if parts.wheel.is_some() && parts.fork.is_some() {
            break;
        }
    }
    parts
}

/// # Safety
///
/// `instance` must be a live part instance of the vehicle's physics body.
unsafe fn read_part(
    instance: &PartInstance,
    index: u32,
    properties: usize,
    name: &str,
) -> PartRead {
    let bounds = unsafe { instance.m_PartModelInstance.as_ref() }.and_then(|model| {
        let mut min = Vector3 { data: [0.0; 3] };
        let mut max = Vector3 { data: [0.0; 3] };
        unsafe { model.GetModelSpaceBoundingBox(&mut min, &mut max) };
        let (min, max) = (Vec3::from_array(min.data), Vec3::from_array(max.data));
        (min.is_finite() && max.is_finite()).then_some((min, max))
    });
    let parent_root = unsafe { instance.m_ParentPart.as_ref() }
        .map_or(Mat4::IDENTITY, |p| Mat4::from(p.m_RootSpaceTransform));
    let offset = unsafe { instance.m_PartProperties.as_ref() }
        .map_or(Mat4::IDENTITY, |p| Mat4::from(p.m_parent_space_offset));
    PartRead {
        index,
        properties,
        name: SmolStr::new(name),
        bounds,
        broken_off: unsafe { instance.BrokeOffOrAncestorBrokeOff() },
        root: Mat4::from(instance.m_RootSpaceTransform),
        rest_root: parent_root * offset,
    }
}

/// The string of an ADF hash string, `None` when it is null or not UTF-8.
///
/// # Safety
///
/// `s` must belong to a live, loaded ADF resource.
unsafe fn adf_str(s: &AdfHashString) -> Option<&str> {
    if s.m_string.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(s.m_string.cast()) }.to_str().ok()
}
