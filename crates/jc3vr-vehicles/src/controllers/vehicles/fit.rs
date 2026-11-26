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
//!
//! The fit also measures where the hub of a wheel fitted from the part sits from the character's
//! eyes, for the driving view's per-vehicle correction
//! ([`crate::controllers::vehicles::driving_view`]), and logs the driving position once per entry.

use std::{ffi::CStr, time::Instant};

use glam::{Mat4, Vec3};
use jc3gi::{
    types::math::Vector3,
    vehicle::{
        parts::{AdfHashString, PartInstance, PfxBreakable},
        vehicle::Vehicle,
    },
};
use jc3vr_body::controllers::{
    grab::cockpit::ControlKind,
    mode::{VehicleClass, VehicleSeat},
};
use jc3vr_view::controllers::cockpit::CockpitTransform;
use smol_str::SmolStr;

use crate::controllers::vehicles::{
    driving_view,
    driving_view::{Correction, Entry, Target},
    entry::VehicleEntry,
    physical::{
        calibration, native,
        native::{CockpitFit, NativeStick},
    },
    tuning::{PhysicalControlsTuning, VEHICLES},
    wheel,
    wheel::{BarsFrame, CockpitParts as _, PartWheel, WheelFrame, WheelPartView, WheelSource},
    wheel_render::RenderWheel,
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
    /// The driving view's correction for the seat, while the player drives a vehicle whose hub was
    /// measured.
    pub view: Option<Target>,
    pub readout: Option<FitReadout>,
    pub native_stick: Option<NativeStick>,
    /// The cockpit's handles measured within its body mesh, with a lever the mesh does not model
    /// placed from the tuning; never a render-animation target.
    pub cockpit: Option<CockpitFit>,
}

/// What the fit reads besides the vehicle.
#[derive(Debug, Copy, Clone)]
pub struct FitInputs<'a> {
    pub physical: &'a PhysicalControlsTuning,
    /// Whether the driving view is switched on
    /// ([`DrivingViewConfig::enabled`](crate::controllers::vehicles::config::DrivingViewConfig::enabled)).
    pub view_enabled: bool,
    pub seat: Option<VehicleSeat>,
    pub xf: &'a CockpitTransform,
    /// The character's eyes, world space: the head pose's anchor (his animated head) with the
    /// camera's offset to the eye point hung off it
    /// ([`jc3vr_view::hooks::camera::head_to_eye_point`]), where the view puts the eyes before the
    /// player's posture or the driving view's correction moves them. `None` until the anchor is
    /// published.
    pub eyes: Option<Vec3>,
    /// The hands' grips, world space, for the cockpit calibration
    /// ([`calibration`](crate::controllers::vehicles::physical::calibration)).
    pub hands: [Option<Vec3>; 2],
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
    /// Where the driving position puts the wheel's hub, for comparing seats.
    pub seat: SeatReadout,
}

/// Where the driving position puts the wheel's hub. Its place from the seated character's eyes is
/// the seat's: it differs from vehicle to vehicle with the driver's animation and the wheel's part.
/// The player's head, tracked about the cockpit origin, adds their own posture, and the driving
/// view's correction moves that origin.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SeatReadout {
    /// The hub from the character's eyes this tick, before the driving view's correction, cockpit
    /// frame (+X right, +Y up, +Z back), metres. `None` until the head pose's anchor is published.
    pub hub_from_eyes: Option<Vec3>,
    /// The hub from the player's tracked head, with the driving view's correction, cockpit frame,
    /// metres.
    pub hub_from_head: Vec3,
    /// The seat's correction, once the entry's measurement is taken; `None` for a fallback wheel.
    pub correction: Option<Correction>,
}

/// What the fit keeps for the vehicle the player drives, for as long as they drive it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct VehicleLatch {
    since: Instant,
    /// The fallback wheel, held in the vehicle's space.
    local: Option<WheelFrame>,
    /// The hub's measurement from the character's eyes for the driving view.
    view: Entry,
}

impl VehicleLatch {
    pub(super) fn new() -> Self {
        Self {
            since: Instant::now(),
            local: None,
            view: Entry::new(),
        }
    }
}

/// Fit the wheel and the handlebars for the seat: to the vehicle where the local player drives one,
/// otherwise to the specs. `latch` carries the fallback wheel and the driving view's measurement
/// between ticks.
pub(super) fn fit(
    inputs: &FitInputs,
    entry: Option<&VehicleEntry>,
    latch: &mut Option<VehicleLatch>,
) -> Fit {
    calibration::expire();
    let config = inputs.physical;
    let specs = Fit {
        wheel: WheelFrame::from_spec(&config.wheel),
        bars: BarsFrame::from_spec(&config.handlebars),
        fork: false,
        render: None,
        steering_turn: None,
        view: None,
        readout: None,
        native_stick: None,
        cockpit: None,
    };
    if !inputs.seat.is_some_and(|s| s.driver) {
        *latch = None;
        calibration::leave();
        return specs;
    }
    let Some(entry) = entry else {
        *latch = None;
        calibration::leave();
        return specs;
    };
    latch.get_or_insert_with(VehicleLatch::new);
    // SAFETY: the game stage retains the observed vehicle outside the world update.
    let fit = unsafe { fit_vehicle(inputs, entry.vehicle(), latch, specs) };
    if fit.cockpit.is_none() {
        calibration::leave();
    }
    fit
}

/// # Safety
///
/// `vehicle` must be the live vehicle the local player drives, on the game thread before the
/// world update.
unsafe fn fit_vehicle(
    inputs: &FitInputs,
    vehicle: &Vehicle,
    latch: &mut Option<VehicleLatch>,
    specs: Fit,
) -> Fit {
    let xf = inputs.xf;
    let world = Mat4::from(vehicle.m_WorldMatrixT1);
    let parts = unsafe { vehicle.m_PfxVehicle.as_ref() }
        .map(|pfx| unsafe { find_parts(pfx) })
        .unwrap_or_default();

    let wheel_view = parts.wheel.as_ref().map(|p| WheelPartView {
        name: &p.name,
        bounds: p.bounds,
        broken_off: p.broken_off,
    });
    let (part_wheel, mut source) = wheel::choose_wheel(wheel_view);
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
            let frame = match latch.as_mut() {
                Some(l) => {
                    if l.local.is_none()
                        && l.since.elapsed() >= VEHICLES.get().driving_view.settle()
                    {
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

    let fork_bars = parts.fork.as_ref().filter(|p| !p.broken_off).and_then(|p| {
        let (centre, half_width) = wheel::bars_from_box(p.bounds?.0, p.bounds?.1)?;
        xf.bars(world * p.rest_root, centre, half_width)
    });

    let hub_from_eyes = inputs
        .eyes
        .map(|eyes| xf.offset_to_cockpit(xf.point_to_world(wheel.centre) - eyes))
        .filter(|h| h.is_finite());
    let measured = matches!(source, WheelSource::Part | WheelSource::Override);
    let class_corrected = inputs.seat.is_some_and(|s| s.class == VehicleClass::Car);
    let wheel_part = parts.wheel.as_ref().map(|p| p.name.as_str());
    let mut closed = false;
    let mut correction = None;
    if let Some(l) = latch.as_mut() {
        closed = l
            .view
            .observe(l.since.elapsed(), hub_from_eyes.filter(|_| measured));
        correction = l.view.mean().filter(|_| measured).map(|mean| {
            driving_view::derive(mean, wheel_part.and_then(driving_view::find_override))
        });
    }
    let seat = SeatReadout {
        hub_from_eyes,
        hub_from_head: wheel.centre - xf.head_cockpit,
        correction,
    };
    let view = correction
        .filter(|_| class_corrected)
        .map(|correction| Target {
            vehicle: vehicle as *const Vehicle as usize,
            correction,
        });
    if closed {
        log_driving_position(&DrivingPosition {
            vehicle_type_hash: vehicle.m_VehicleTypeNameHash,
            wheel_part,
            source,
            seat: &seat,
            corrected: view.is_some() && inputs.view_enabled,
            cockpit_part: parts.cockpit.as_ref().map(|part| part.name.as_str()),
            eyes_in_cockpit_part: parts.cockpit.as_ref().and_then(|part| {
                let part_to_world = world * part.root;
                let eyes = inputs.eyes?;
                (part_to_world.determinant().abs() > 1e-6)
                    .then(|| part_to_world.inverse().transform_point3(eyes))
                    .filter(|eyes| eyes.is_finite())
            }),
        });
    }

    let cockpit = parts
        .cockpit
        .as_ref()
        .and_then(|part| fit_cockpit(inputs, world, vehicle as *const Vehicle as usize, part));
    let native_stick = parts
        .stick
        .as_ref()
        .filter(|p| cockpit.is_none() && !p.broken_off && p.bounds.is_some())
        .and_then(|p| {
            let (kind, pivot, grip) = native::asset(&p.name)?;
            let expected = match inputs.seat?.class {
                VehicleClass::Helicopter => ControlKind::Cyclic,
                VehicleClass::Plane => ControlKind::FlightStick,
                _ => return None,
            };
            if kind != expected || !xf.is_usable() {
                return None;
            }
            let cockpit_to_world = Mat4::from_scale_rotation_translation(
                Vec3::splat(xf.scale()),
                xf.body,
                xf.point_to_world(Vec3::ZERO),
            );
            let part_to_cockpit = cockpit_to_world.inverse() * world * p.root;
            if !part_to_cockpit.is_finite() || part_to_cockpit.determinant().abs() < 1e-6 {
                return None;
            }
            Some(NativeStick {
                kind,
                target: RenderWheel {
                    vehicle: vehicle as *const Vehicle as usize,
                    part_index: p.index,
                    properties: p.properties,
                    hub: pivot,
                    axis: Vec3::Y,
                },
                part_to_cockpit,
                pivot,
                grip,
            })
        });

    Fit {
        native_stick,
        cockpit,
        wheel,
        bars: fork_bars.unwrap_or(specs.bars),
        fork: parts.fork.is_some(),
        render,
        steering_turn: Some(vehicle.m_SteeringTurn).filter(|t| t.is_finite()),
        view,
        readout: Some(FitReadout {
            vehicle_type_hash: vehicle.m_VehicleTypeNameHash,
            wheel_part: parts.wheel.as_ref().map(|p| p.name.clone()),
            fork_part: parts.fork.as_ref().map(|p| p.name.clone()),
            source,
            part_wheel: fitted.map(|(_, w, _)| w),
            fallback_latched,
            bars_from_vehicle: fork_bars.is_some(),
            seat,
        }),
    }
}

fn fit_cockpit(
    inputs: &FitInputs,
    world: Mat4,
    vehicle: usize,
    part: &PartRead,
) -> Option<CockpitFit> {
    let seat = inputs.seat.filter(|seat| seat.driver)?;
    if part.broken_off || !inputs.xf.is_usable() {
        return None;
    }
    let (min, max) = part.bounds?;
    if !min.is_finite() || !max.is_finite() || !min.cmplt(max).all() {
        return None;
    }
    let part_to_world = world * part.root;
    if !part_to_world.is_finite() || part_to_world.determinant().abs() < 1e-6 {
        return None;
    }
    // The uncorrected character eyes select the actual pilot seat, independent of roomscale.
    let pilot = part_to_world.inverse().transform_point3(inputs.eyes?);
    let mut asset = native::cockpit_asset(&part.name, pilot)?;
    let fits = match seat.class {
        VehicleClass::Helicopter => asset.stick_kind == ControlKind::Cyclic,
        VehicleClass::Plane => matches!(
            asset.stick_kind,
            ControlKind::FlightStick | ControlKind::Yoke
        ),
        _ => false,
    };
    if !fits {
        return None;
    }
    let to_part = part_to_world.inverse();
    let hands_in_part = inputs
        .hands
        .map(|hand| hand.map(|hand| to_part.transform_point3(hand)));
    // A calibrated cockpit's controls are where the player's hands put them, inside or not.
    let calibrated = calibration::fit(&part.name, &mut asset, hands_in_part);
    let inside = |point: Vec3| point.cmpge(min).all() && point.cmple(max).all();
    if !calibrated
        && (!inside(asset.stick_grip) || asset.lever.is_some_and(|(_, grip)| !inside(grip)))
    {
        return None;
    }
    let xf = inputs.xf;
    let cockpit_to_world = Mat4::from_scale_rotation_translation(
        Vec3::splat(xf.scale()),
        xf.body,
        xf.point_to_world(Vec3::ZERO),
    );
    if !cockpit_to_world.is_finite() || cockpit_to_world.determinant().abs() < 1e-6 {
        return None;
    }
    let part_to_cockpit = cockpit_to_world.inverse() * part_to_world;
    if !part_to_cockpit.is_finite() || part_to_cockpit.determinant().abs() < 1e-6 {
        return None;
    }
    Some(CockpitFit::new(
        RenderWheel {
            vehicle,
            part_index: part.index,
            properties: part.properties,
            hub: asset.stick_grip,
            axis: Vec3::Y,
        },
        part_to_cockpit,
        asset,
        (min, max),
        inputs.physical,
    ))
}

/// What the driving position line logs, once per entry.
struct DrivingPosition<'a> {
    vehicle_type_hash: u32,
    wheel_part: Option<&'a str>,
    source: WheelSource,
    seat: &'a SeatReadout,
    /// Whether the view is corrected in this seat: the setting is on and the class is corrected.
    corrected: bool,
    /// The cockpit's body part, which names the airframe for the cockpit controls' table.
    cockpit_part: Option<&'a str>,
    /// The character's eyes in the cockpit part's own space, where its controls are measured.
    eyes_in_cockpit_part: Option<Vec3>,
}

fn log_driving_position(p: &DrivingPosition) {
    let c = p.seat.correction;
    tracing::info!(
        target: "controllers",
        vehicle_type_hash = format_args!("{:08x}", p.vehicle_type_hash),
        wheel_part = p.wheel_part.unwrap_or("none"),
        source = p.source.label(),
        hub_from_eyes = ?c.map(|c| c.hub_from_eyes),
        hub_from_head = ?p.seat.hub_from_head,
        standard = ?VEHICLES.get().driving_view.hub_from_eyes_m,
        wanted = ?c.map(|c| c.wanted),
        applied = ?c.filter(|_| p.corrected).map(|c| c.applied),
        origin = c.map(|c| c.origin.label()),
        corrected = p.corrected,
        cockpit_part = p.cockpit_part.unwrap_or("none"),
        eyes_in_cockpit_part = ?p.eyes_in_cockpit_part,
        "driving position"
    );
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
    stick: Option<PartRead>,
    cockpit: Option<PartRead>,
}

/// The vehicle's steering wheel and steering fork parts, the first of each among its valid parts.
///
/// # Safety
///
/// `pfx` must be the live vehicle's physics body, on the game thread before the world update.
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
        } else if native::asset(name).is_some() {
            &mut parts.stick
        } else if native::cockpit_asset(name, Vec3::ZERO).is_some() {
            &mut parts.cockpit
        } else {
            continue;
        };
        if slot.is_none() {
            *slot =
                Some(unsafe { read_part(instance, index, properties as *const _ as usize, name) });
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

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use glam::Quat;

    use super::*;
    use crate::controllers::vehicles::tuning::PhysicalControlsTuning;

    static SHIPPED: LazyLock<PhysicalControlsTuning> =
        LazyLock::new(PhysicalControlsTuning::shipped);

    fn part(name: &str) -> PartRead {
        let (min, max) = match name {
            "utility_body" => (
                Vec3::new(-0.800297, 0.6425335, -3.0365307),
                Vec3::new(0.9101837, 2.977942, 2.4216115),
            ),
            "fighterjet_body" => (
                Vec3::new(-1.0071707, 0.8793905, -6.815199),
                Vec3::new(1.0071709, 2.65673, -0.3190375),
            ),
            "fighterbomber_body" => (
                Vec3::new(-1.8940631, 1.0584159, -7.160036),
                Vec3::new(1.8940631, 3.2672918, 0.31849),
            ),
            // A box around the cabin, enough for the fit's checks.
            "heavytroop_body" => (Vec3::new(-1.5, 0.5, -4.0), Vec3::new(1.5, 3.5, 3.0)),
            _ => unreachable!(),
        };
        PartRead {
            index: 0,
            properties: 7,
            name: SmolStr::new(name),
            bounds: Some((min, max)),
            broken_off: false,
            root: Mat4::IDENTITY,
            rest_root: Mat4::IDENTITY,
        }
    }

    fn inputs(xf: &CockpitTransform, class: VehicleClass, eyes: Vec3) -> FitInputs<'_> {
        FitInputs {
            physical: &SHIPPED,
            view_enabled: false,
            seat: Some(VehicleSeat {
                class,
                driver: true,
            }),
            xf,
            eyes: Some(eyes),
            hands: [None; 2],
        }
    }

    #[test]
    fn live_cockpit_handles_follow_the_model_part_and_world_transforms() {
        for (name, class, pilot) in [
            (
                "utility_body",
                VehicleClass::Helicopter,
                Vec3::new(-0.35, 1.5, -0.92),
            ),
            (
                "fighterjet_body",
                VehicleClass::Plane,
                Vec3::new(0.0, 2.3, -2.6),
            ),
            (
                "fighterbomber_body",
                VehicleClass::Plane,
                Vec3::new(0.0, 2.9, -2.762207),
            ),
        ] {
            let mut part = part(name);
            part.root = Mat4::from_rotation_translation(
                Quat::from_rotation_y(-0.3),
                Vec3::new(0.2, 0.1, -0.5),
            );
            let world = Mat4::from_rotation_translation(
                Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.4),
                Vec3::new(100.0, 250.0, -30.0),
            );
            let part_to_world = world * part.root;
            let eyes = part_to_world.transform_point3(pilot);
            let xf = CockpitTransform {
                head_world: eyes + Vec3::new(0.1, 0.02, -0.03),
                head_cockpit: Vec3::new(0.2, 1.6, 0.1),
                body: Quat::from_rotation_y(1.0),
                world_scale: 0.8,
            };
            let fit = fit_cockpit(&inputs(&xf, class, eyes), world, 11, &part).unwrap();
            assert_eq!(fit.target.vehicle, 11);
            assert_eq!(fit.target.properties, part.properties);
            let drawn = part_to_world.transform_point3(fit.asset.stick_grip);
            assert!(xf.point_to_world(fit.stick_top()).distance(drawn) < 1e-4);
            if let Some((kind, grip)) = fit.asset.lever {
                let drawn = part_to_world.transform_point3(grip);
                assert!(
                    xf.point_to_world(fit.lever_rest(kind).unwrap())
                        .distance(drawn)
                        < 1e-4
                );
            }
            assert!(native::asset(name).is_none());
        }
    }

    #[test]
    fn utility_fit_selects_the_character_seat_not_the_tracked_head_side() {
        let part = part("utility_body");
        for pilot_x in [-0.35, 0.35] {
            let xf =
                CockpitTransform::from_origin(Vec3::new(-pilot_x, 2.0, -0.92), Quat::IDENTITY, 1.0);
            let inputs = inputs(
                &xf,
                VehicleClass::Helicopter,
                Vec3::new(pilot_x, 1.6, -0.92),
            );
            let fit = fit_cockpit(&inputs, Mat4::IDENTITY, 1, &part).unwrap();
            assert_eq!(fit.asset.seat, u8::from(pilot_x > 0.0));
        }
    }

    #[test]
    fn a_missing_collective_is_placed_from_the_seats_cyclic() {
        let part = part("utility_body");
        for pilot_x in [-0.35, 0.35] {
            let eyes = Vec3::new(pilot_x, 1.5, -0.92);
            let xf = CockpitTransform::from_origin(eyes, Quat::IDENTITY, 1.0);
            let inputs = inputs(&xf, VehicleClass::Helicopter, eyes);
            let fit = fit_cockpit(&inputs, Mat4::IDENTITY, 1, &part).unwrap();
            let (kind, part_rest) = fit.fallback_lever.unwrap();
            assert_eq!(kind, ControlKind::Collective);
            let cyclic = fit.asset.stick_grip;
            assert!(
                part_rest.x < cyclic.x && part_rest.y < cyclic.y,
                "by the pilot's left thigh: {part_rest} against the cyclic at {cyclic}"
            );
            assert!(
                (fit.lever_rest(ControlKind::Collective).unwrap() - (part_rest - eyes)).length()
                    < 1e-5
            );
        }
    }

    #[test]
    fn the_heavy_troop_helicopters_console_collective_is_the_right_seats() {
        let part = part("heavytroop_body");
        let seat = |pilot_x: f32| {
            let eyes = Vec3::new(pilot_x, 2.1, -2.0);
            let xf = CockpitTransform::from_origin(eyes, Quat::IDENTITY, 1.0);
            let inputs = inputs(&xf, VehicleClass::Helicopter, eyes);
            fit_cockpit(&inputs, Mat4::IDENTITY, 1, &part).unwrap()
        };
        let right = seat(0.6);
        assert!(right.asset.lever.is_some() && right.fallback_lever.is_none());
        let left = seat(-0.6);
        assert!(left.asset.lever.is_none());
        assert_eq!(
            left.fallback_lever.map(|(kind, _)| kind),
            Some(ControlKind::Collective)
        );
    }

    #[test]
    fn cockpit_fit_rejects_unavailable_mismatched_and_degenerate_sources() {
        let xf = CockpitTransform::from_origin(Vec3::ZERO, Quat::IDENTITY, 1.0);
        let inputs = inputs(&xf, VehicleClass::Plane, Vec3::new(0.0, 2.3, -2.6));
        let part = part("fighterjet_body");
        assert!(fit_cockpit(&inputs, Mat4::IDENTITY, 1, &part).is_some());
        for eyes in [None, Some(Vec3::splat(f32::NAN))] {
            assert!(fit_cockpit(&FitInputs { eyes, ..inputs }, Mat4::IDENTITY, 1, &part).is_none());
        }
        for seat in [
            None,
            Some(VehicleSeat {
                class: VehicleClass::Plane,
                driver: false,
            }),
            Some(VehicleSeat {
                class: VehicleClass::Helicopter,
                driver: true,
            }),
            Some(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
        ] {
            assert!(fit_cockpit(&FitInputs { seat, ..inputs }, Mat4::IDENTITY, 1, &part).is_none());
        }
        for world in [
            Mat4::ZERO,
            Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0)),
            Mat4::NAN,
        ] {
            assert!(fit_cockpit(&inputs, world, 1, &part).is_none());
        }
        for changed in [
            PartRead {
                broken_off: true,
                ..part.clone()
            },
            PartRead {
                bounds: None,
                ..part.clone()
            },
            PartRead {
                bounds: Some((Vec3::ZERO, Vec3::ZERO)),
                ..part.clone()
            },
            PartRead {
                bounds: Some((Vec3::splat(-0.1), Vec3::splat(0.1))),
                ..part.clone()
            },
            PartRead {
                root: Mat4::ZERO,
                ..part.clone()
            },
            PartRead {
                name: SmolStr::new("lightassault_joystick"),
                ..part.clone()
            },
        ] {
            assert!(fit_cockpit(&inputs, Mat4::IDENTITY, 1, &changed).is_none());
        }
    }
}
