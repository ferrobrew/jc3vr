//! The steering wheel's and handlebars' geometry, fitted to the vehicle the player drives. Pure.
//!
//! Most cars and boats ship their steering wheel as a separate vehicle part
//! ([`jc3gi::vehicle::parts::Part`], named `<model>_steeringwheel` or `<model>_steering_wheel`)
//! whose mesh is authored in the vehicle's own space, so the part's model-space bounding box is the
//! wheel's bounds in the vehicle (`docs/engine/gameplay/vehicle-parts.md`). [`wheel_from_box`]
//! reads a hub, a radius, and a tilt from that box: the hub is the box's centre, the radius half
//! its width, and the tilt the angle a thin disc leans back by to cover the box's depth for its
//! height. A few meshes include the column or are not discs, and their box misleads;
//! [`WHEEL_OVERRIDES`] replaces the fit for those, keyed by the part's name.
//!
//! Motorcycles, the ATV, the jetski, and the duck turn handlebars instead: a hinged part
//! (`<model>_upper_steering`) whose mesh is authored about its hinge pivot. [`bars_from_box`] reads
//! the grips' middle and half width from its box.
//!
//! Everything here works in the part's own space (+X right, +Y up, +Z back, metres), which is the
//! vehicle's at rest. The caller carries the results through the part's root-space transform and
//! the vehicle's world transform into the cockpit frame ([`CockpitTransform`]).

use glam::{Mat4, Quat, Vec3};

use crate::controllers::vehicles::config::{HandlebarsSpec, WheelSpec};

/// A steering wheel in the cockpit frame: its hub, its in-plane axes, and its rim's radius.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WheelFrame {
    pub centre: Vec3,
    /// The wheel's right, in its plane, a unit vector.
    pub right: Vec3,
    /// The wheel's up along its face, a unit vector; with the face tilted back, its top leans away
    /// from the driver.
    pub up: Vec3,
    pub radius: f32,
}

impl WheelFrame {
    /// The wheel the spec describes, centred on the driver as the spec gives it. A car's wheel does
    /// not move with the driver's handedness, so the spec is never mirrored.
    pub fn from_spec(spec: &WheelSpec) -> Self {
        let (right, up) = tilted_axes(spec.tilt_deg.to_radians());
        Self {
            centre: spec.centre_m,
            right,
            up,
            radius: spec.radius_m,
        }
    }

    /// The angle of `point` around the hub in the wheel's plane, radians clockwise from the top as
    /// the driver sees it. `None` at the hub itself.
    pub fn hand_angle(&self, point: Vec3) -> Option<f32> {
        let d = point - self.centre;
        let (x, y) = (d.dot(self.right), d.dot(self.up));
        (x * x + y * y > 1e-6 && x.is_finite() && y.is_finite()).then(|| x.atan2(y))
    }

    /// The point on the rim at `angle` (clockwise from the top).
    pub fn rim_point(&self, angle: f32) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        self.centre + (self.right * sin + self.up * cos) * self.radius
    }

    /// The wheel's face normal, toward the driver.
    pub fn normal(&self) -> Vec3 {
        self.right.cross(self.up)
    }
}

/// Handlebars in the cockpit frame: the middle of the grips and the reach from it to each grip.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BarsFrame {
    pub centre: Vec3,
    pub half_width: f32,
}

impl BarsFrame {
    /// The handlebars the spec describes. They are centred on the driver, so the spec is never
    /// mirrored.
    pub fn from_spec(spec: &HandlebarsSpec) -> Self {
        Self {
            centre: spec.centre_m,
            half_width: spec.half_width_m,
        }
    }
}

/// A steering wheel in its part's own space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PartWheel {
    pub hub: Vec3,
    pub radius: f32,
    /// How far the face tilts up from vertical toward the driver, radians.
    pub tilt: f32,
}

impl PartWheel {
    /// The face normal, toward the driver: the axis the wheel turns about.
    pub fn axis(&self) -> Vec3 {
        let (right, up) = tilted_axes(self.tilt);
        right.cross(up)
    }

    /// The in-plane right and up axes.
    pub fn axes(&self) -> (Vec3, Vec3) {
        tilted_axes(self.tilt)
    }
}

/// The wheel a thin disc fitted to the part's model-space box `min..max` would be. `None` for a
/// box too small or malformed to hold a wheel.
pub fn wheel_from_box(min: Vec3, max: Vec3) -> Option<PartWheel> {
    let extent = max - min;
    if !(min.is_finite() && max.is_finite()) || extent.min_element() < 0.0 {
        return None;
    }
    let radius = extent.x * 0.5;
    if radius < MIN_WHEEL_RADIUS_M || extent.y < 1e-3 {
        return None;
    }
    // A disc tilted back by `t` spans `2r cos t` up and `2r sin t` deep, plus its thickness, so
    // the ratio of depth to height leans slightly high; the overrides catch the boxes it misleads.
    let tilt = extent
        .z
        .atan2(extent.y)
        .clamp(0.0, MAX_TILT_DEG.to_radians());
    Some(PartWheel {
        hub: (min + max) * 0.5,
        radius,
        tilt,
    })
}

/// The handlebars' grips' middle and half width, in the steering-fork part's own space, from its
/// model-space box `min..max`. The bars span the box's width; the grips sit at its rear (toward
/// the rider) and its mid-height, inset from its ends by a hand's half width.
pub fn bars_from_box(min: Vec3, max: Vec3) -> Option<(Vec3, f32)> {
    let extent = max - min;
    if !(min.is_finite() && max.is_finite()) || extent.min_element() < 0.0 {
        return None;
    }
    let half_width = extent.x * 0.5 - GRIP_INSET_M;
    if half_width < MIN_BARS_HALF_WIDTH_M {
        return None;
    }
    let centre = Vec3::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        max.z - GRIP_INSET_M,
    );
    Some((centre, half_width))
}

/// A wheel whose part's box misleads, given directly in the part's own space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WheelOverride {
    /// The steering wheel part's name.
    pub part: &'static str,
    pub hub: Vec3,
    pub radius_m: f32,
    pub tilt_deg: f32,
}

impl WheelOverride {
    pub fn wheel(&self) -> PartWheel {
        PartWheel {
            hub: self.hub,
            radius: self.radius_m,
            tilt: self.tilt_deg.to_radians(),
        }
    }
}

/// The wheels whose part's box misleads. Each was placed from the box and the seated driver's
/// point in the vehicle's shipped data, so they are a starting point to tune in the headset:
///
/// - the circuit racer's and the ladbil's meshes run the column far forward, so the wheel sits at
///   the box's rear, small and nearly upright;
/// - the old bus's mesh runs the column down to the floor, so the wheel sits at the box's top,
///   tilted nearly flat;
/// - the old jeep's mesh includes the column, which reads as a steep tilt;
/// - the transport boat's ship's wheel spans its handles, so its rim is inside the box.
pub const WHEEL_OVERRIDES: &[WheelOverride] = &[
    WheelOverride {
        part: "moderncircuitracer_steeringwheel",
        hub: Vec3::new(0.0, 0.54, -0.40),
        radius_m: 0.12,
        tilt_deg: 10.0,
    },
    WheelOverride {
        part: "ladbil_steering_wheel",
        hub: Vec3::new(0.0, 0.71, 0.11),
        radius_m: 0.11,
        tilt_deg: 20.0,
    },
    WheelOverride {
        part: "oldbus_steeringwheel",
        hub: Vec3::new(-0.75, 1.67, -3.64),
        radius_m: 0.19,
        tilt_deg: 60.0,
    },
    WheelOverride {
        part: "oldjeep_steeringwheel",
        hub: Vec3::new(-0.44, 1.35, -0.03),
        radius_m: 0.17,
        tilt_deg: 35.0,
    },
    WheelOverride {
        part: "transport_steering_wheel",
        hub: Vec3::new(2.88, 2.16, 7.18),
        radius_m: 0.36,
        tilt_deg: 0.0,
    },
];

/// The override for the steering wheel part named `part`, if it has one.
pub fn find_override(part: &str) -> Option<&'static WheelOverride> {
    WHEEL_OVERRIDES.iter().find(|o| o.part == part)
}

/// Whether a part named `name` is a steering wheel. The mirrors of a few vehicles carry the
/// steering wheel's damage type, so the name decides.
pub fn is_wheel_part(name: &str) -> bool {
    name.ends_with("_steeringwheel") || name.ends_with("_steering_wheel")
}

/// Whether a part named `name` with behaviour `behavior` is a steering fork: the handlebars.
pub fn is_fork_part(name: &str, behavior: &str) -> bool {
    behavior == "SteeringFork" || name.ends_with("_upper_steering")
}

/// Where the wheel's geometry came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WheelSource {
    /// Fitted to the vehicle's steering wheel part.
    Part,
    /// The override for the vehicle's steering wheel part.
    Override,
    /// The configured spec, for the reason given.
    Fallback(Fallback),
}

impl WheelSource {
    pub const fn label(self) -> &'static str {
        match self {
            WheelSource::Part => "the vehicle's wheel part",
            WheelSource::Override => "the override for the vehicle's wheel part",
            WheelSource::Fallback(why) => why.label(),
        }
    }
}

/// Why the wheel falls back to the configured spec.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// Placing the wheel from the vehicle is switched off.
    Disabled,
    /// The vehicle has no steering wheel part, as the modern bus, the old truck, and the tanks do.
    NoPart,
    /// The steering wheel part, or a part it hangs from, has broken off.
    BrokenOff,
    /// The part has no model, or its box holds no wheel.
    NoModel,
}

impl Fallback {
    pub const fn label(self) -> &'static str {
        match self {
            Fallback::Disabled => "the configured wheel (placing from the vehicle is off)",
            Fallback::NoPart => "the configured wheel (the vehicle has no wheel part)",
            Fallback::BrokenOff => "the configured wheel (the wheel part broke off)",
            Fallback::NoModel => "the configured wheel (the wheel part has no usable model)",
        }
    }
}

/// What the vehicle's steering wheel part looks like, as read from the game.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WheelPartView<'a> {
    pub name: &'a str,
    /// The part's model-space box, `None` without a model.
    pub bounds: Option<(Vec3, Vec3)>,
    pub broken_off: bool,
}

/// The wheel to use for a vehicle whose steering wheel part is `part`: the override for the part,
/// else the fit to its box, else the configured spec.
pub fn choose_wheel(
    part: Option<WheelPartView>,
    enabled: bool,
) -> (Option<PartWheel>, WheelSource) {
    if !enabled {
        return (None, WheelSource::Fallback(Fallback::Disabled));
    }
    let Some(part) = part else {
        return (None, WheelSource::Fallback(Fallback::NoPart));
    };
    if part.broken_off {
        return (None, WheelSource::Fallback(Fallback::BrokenOff));
    }
    if let Some(o) = find_override(part.name) {
        return (Some(o.wheel()), WheelSource::Override);
    }
    match part.bounds.and_then(|(min, max)| wheel_from_box(min, max)) {
        Some(wheel) => (Some(wheel), WheelSource::Part),
        None => (None, WheelSource::Fallback(Fallback::NoModel)),
    }
}

/// The turn of the wheel by `angle` radians, clockwise as the driver sees it, about the axis
/// through `hub` along `axis` (the face normal, toward the driver), as a transform of the wheel's
/// own space.
pub fn turn_about_hub(hub: Vec3, axis: Vec3, angle: f32) -> Mat4 {
    let Some(axis) = axis.try_normalize() else {
        return Mat4::IDENTITY;
    };
    if !(angle.is_finite() && hub.is_finite()) {
        return Mat4::IDENTITY;
    }
    // Seen from the driver, along -axis, a positive rotation about the axis runs anticlockwise.
    Mat4::from_translation(hub)
        * Mat4::from_quat(Quat::from_axis_angle(axis, -angle))
        * Mat4::from_translation(-hub)
}

/// The map between the world and the cockpit frame, as the head pose composes them: the cockpit
/// frame's `head_cockpit` sits at the world's `head_world`, turned by the body frame and scaled by
/// the world scale.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CockpitTransform {
    pub head_world: Vec3,
    pub head_cockpit: Vec3,
    pub body: Quat,
    pub world_scale: f32,
}

impl CockpitTransform {
    fn scale(&self) -> f32 {
        self.world_scale.max(1e-3)
    }

    pub fn point_to_cockpit(&self, world: Vec3) -> Vec3 {
        self.head_cockpit + self.body.inverse() * (world - self.head_world) / self.scale()
    }

    pub fn point_to_world(&self, cockpit: Vec3) -> Vec3 {
        self.head_world + self.body * ((cockpit - self.head_cockpit) * self.scale())
    }

    pub fn direction_to_cockpit(&self, world: Vec3) -> Vec3 {
        self.body.inverse() * world
    }

    pub fn direction_to_world(&self, cockpit: Vec3) -> Vec3 {
        self.body * cockpit
    }

    /// `wheel`, in the space `to_world` maps to the world, in the cockpit frame.
    pub fn wheel(&self, to_world: Mat4, wheel: &PartWheel) -> Option<WheelFrame> {
        let (right, up) = wheel.axes();
        let right_w = to_world.transform_vector3(right);
        let up_w = to_world.transform_vector3(up);
        let radius = wheel.radius * right_w.length() / self.scale();
        let frame = WheelFrame {
            centre: self.point_to_cockpit(to_world.transform_point3(wheel.hub)),
            right: self.direction_to_cockpit(right_w.try_normalize()?),
            up: self.direction_to_cockpit(up_w.try_normalize()?),
            radius,
        };
        (frame.centre.is_finite() && frame.radius.is_finite()).then_some(frame)
    }

    /// The handlebars whose middle is `centre` and half width `half_width`, in the space
    /// `to_world` maps to the world, in the cockpit frame.
    pub fn bars(&self, to_world: Mat4, centre: Vec3, half_width: f32) -> Option<BarsFrame> {
        let scale = to_world.transform_vector3(Vec3::X).length();
        let frame = BarsFrame {
            centre: self.point_to_cockpit(to_world.transform_point3(centre)),
            half_width: half_width * scale / self.scale(),
        };
        (frame.centre.is_finite() && frame.half_width.is_finite()).then_some(frame)
    }

    /// `frame` carried into the space `to_world` maps to the world: the inverse of
    /// [`wheel`](Self::wheel), for holding a cockpit-placed wheel fixed to the vehicle.
    pub fn wheel_into(&self, to_world: Mat4, frame: &WheelFrame) -> Option<WheelFrame> {
        let to_local = to_world.inverse();
        let local = WheelFrame {
            centre: to_local.transform_point3(self.point_to_world(frame.centre)),
            right: to_local
                .transform_vector3(self.direction_to_world(frame.right))
                .try_normalize()?,
            up: to_local
                .transform_vector3(self.direction_to_world(frame.up))
                .try_normalize()?,
            radius: frame.radius * self.scale(),
        };
        local.centre.is_finite().then_some(local)
    }

    /// A wheel held in the space `to_world` maps to the world (from
    /// [`wheel_into`](Self::wheel_into)), in the cockpit frame.
    pub fn wheel_from(&self, to_world: Mat4, local: &WheelFrame) -> Option<WheelFrame> {
        let frame = WheelFrame {
            centre: self.point_to_cockpit(to_world.transform_point3(local.centre)),
            right: self
                .direction_to_cockpit(to_world.transform_vector3(local.right).try_normalize()?),
            up: self.direction_to_cockpit(to_world.transform_vector3(local.up).try_normalize()?),
            radius: local.radius / self.scale(),
        };
        frame.centre.is_finite().then_some(frame)
    }
}

/// The steepest tilt the box fit gives, degrees: a bus's wheel lies about this flat.
const MAX_TILT_DEG: f32 = 60.0;

/// The smallest wheel the box fit accepts, metres.
const MIN_WHEEL_RADIUS_M: f32 = 0.05;

/// How far in from the bars' ends and rear the grips sit, metres.
const GRIP_INSET_M: f32 = 0.04;

/// The narrowest handlebars the box fit accepts, metres from the middle to a grip.
const MIN_BARS_HALF_WIDTH_M: f32 = 0.1;

/// The in-plane right and up axes of a wheel whose face tilts back by `tilt` radians.
fn tilted_axes(tilt: f32) -> (Vec3, Vec3) {
    let (sin, cos) = tilt.sin_cos();
    (Vec3::X, Vec3::new(0.0, cos, -sin))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::controllers::vehicles::config::PhysicalControlsConfig;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn angles_around_the_wheel_run_clockwise_from_the_top() {
        let frame = WheelFrame::from_spec(&PhysicalControlsConfig::new().wheel);
        assert!(frame.hand_angle(frame.rim_point(0.0)).unwrap().abs() < 1e-5);
        let right = frame.rim_point(FRAC_PI_2);
        assert!(right.x > frame.centre.x);
        assert!((frame.hand_angle(right).unwrap() - FRAC_PI_2).abs() < 1e-5);
        assert_eq!(frame.hand_angle(frame.centre), None);
        assert!(frame.normal().z > 0.0 && frame.normal().y > 0.0);
    }

    #[test]
    fn a_box_fits_a_wheel_at_its_centre_half_its_width_wide() {
        // The modern SUV's wheel part's box.
        let min = Vec3::new(-0.565, 0.89, -0.53);
        let max = Vec3::new(-0.235, 1.23, -0.35);
        let wheel = wheel_from_box(min, max).unwrap();
        assert!(close(wheel.hub, Vec3::new(-0.4, 1.06, -0.44)));
        assert!((wheel.radius - 0.165).abs() < 1e-4);
        let tilt = wheel.tilt.to_degrees();
        assert!((25.0..30.0).contains(&tilt), "{tilt}");
        let axis = wheel.axis();
        assert!(axis.z > 0.0 && axis.y > 0.0 && axis.x.abs() < 1e-6);
    }

    #[test]
    fn an_upright_box_fits_an_upright_wheel_and_a_deep_one_stops_at_the_limit() {
        let upright = wheel_from_box(Vec3::new(-0.2, -0.2, -0.01), Vec3::new(0.2, 0.2, 0.01));
        assert!(upright.unwrap().tilt.to_degrees() < 3.0);
        let deep = wheel_from_box(Vec3::new(-0.2, -0.1, -1.0), Vec3::new(0.2, 0.1, 1.0));
        assert!((deep.unwrap().tilt.to_degrees() - MAX_TILT_DEG).abs() < 1e-3);
    }

    #[test]
    fn a_degenerate_box_fits_nothing() {
        assert_eq!(wheel_from_box(Vec3::ZERO, Vec3::ZERO), None);
        assert_eq!(wheel_from_box(Vec3::ONE, Vec3::ZERO), None);
        assert_eq!(wheel_from_box(Vec3::splat(f32::NAN), Vec3::ONE), None);
    }

    #[test]
    fn the_bars_sit_at_the_rear_middle_inset_from_the_ends() {
        // The ATV's steering fork's box, about its hinge pivot.
        let (centre, half) =
            bars_from_box(Vec3::new(-0.41, -0.06, -0.33), Vec3::new(0.41, 0.1, 0.0)).unwrap();
        assert!(close(centre, Vec3::new(0.0, 0.02, -GRIP_INSET_M)));
        assert!((half - (0.41 - GRIP_INSET_M)).abs() < 1e-5);
        assert_eq!(bars_from_box(Vec3::ZERO, Vec3::new(0.1, 0.1, 0.1)), None);
    }

    #[test]
    fn an_override_replaces_the_fit_and_a_missing_or_broken_part_falls_back() {
        let bounds = Some((Vec3::new(-0.11, 0.41, -1.33), Vec3::new(0.11, 0.67, -0.34)));
        let view = |name, broken_off| WheelPartView {
            name,
            bounds,
            broken_off,
        };
        let (wheel, source) =
            choose_wheel(Some(view("moderncircuitracer_steeringwheel", false)), true);
        assert_eq!(source, WheelSource::Override);
        assert_eq!(
            wheel.unwrap().hub,
            find_override("moderncircuitracer_steeringwheel")
                .unwrap()
                .hub
        );
        let (wheel, source) = choose_wheel(Some(view("modernsuv_steeringwheel", false)), true);
        assert_eq!(source, WheelSource::Part);
        assert!(close(wheel.unwrap().hub, Vec3::new(0.0, 0.54, -0.835)));
        assert_eq!(
            choose_wheel(Some(view("modernsuv_steeringwheel", true)), true).1,
            WheelSource::Fallback(Fallback::BrokenOff)
        );
        assert_eq!(
            choose_wheel(None, true).1,
            WheelSource::Fallback(Fallback::NoPart)
        );
        assert_eq!(
            choose_wheel(Some(view("modernsuv_steeringwheel", false)), false).1,
            WheelSource::Fallback(Fallback::Disabled)
        );
        let no_model = WheelPartView {
            bounds: None,
            ..view("modernsuv_steeringwheel", false)
        };
        assert_eq!(
            choose_wheel(Some(no_model), true).1,
            WheelSource::Fallback(Fallback::NoModel)
        );
    }

    #[test]
    fn every_override_names_a_wheel_part() {
        for o in WHEEL_OVERRIDES {
            assert!(is_wheel_part(o.part), "{}", o.part);
            assert!(o.radius_m > 0.0 && (0.0..=90.0).contains(&o.tilt_deg));
        }
    }

    #[test]
    fn mirrors_and_forks_are_not_wheels() {
        assert!(is_wheel_part("modernsuv_steeringwheel"));
        assert!(is_wheel_part("oldtractor_steering_wheel"));
        assert!(!is_wheel_part("modernmini_mirror_l"));
        assert!(!is_wheel_part("atv_upper_steering"));
        assert!(is_fork_part("atv_upper_steering", "SteeringFork"));
        assert!(is_fork_part("snowmobile_upper_steering", "None"));
        assert!(!is_fork_part("modernsuv_steeringwheel", "SteeringWheel"));
    }

    #[test]
    fn turning_clockwise_moves_the_top_of_the_wheel_right() {
        let wheel = PartWheel {
            hub: Vec3::new(-0.4, 1.0, -0.4),
            radius: 0.17,
            tilt: 25f32.to_radians(),
        };
        let (_, up) = wheel.axes();
        let top = wheel.hub + up * wheel.radius;
        let turn = turn_about_hub(wheel.hub, wheel.axis(), FRAC_PI_2);
        assert!(close(turn.transform_point3(wheel.hub), wheel.hub));
        let moved = turn.transform_point3(top);
        assert!(close(moved, wheel.hub + Vec3::X * wheel.radius), "{moved}");
        assert_eq!(turn_about_hub(wheel.hub, Vec3::ZERO, 1.0), Mat4::IDENTITY);
    }

    #[test]
    fn the_cockpit_transform_round_trips_and_carries_a_wheel_with_the_vehicle() {
        let xf = CockpitTransform {
            head_world: Vec3::new(100.0, 20.0, -50.0),
            head_cockpit: Vec3::new(0.0, 0.05, 0.0),
            body: Quat::from_rotation_y(0.7),
            world_scale: 1.25,
        };
        let p = Vec3::new(0.1, -0.4, -0.3);
        assert!(close(xf.point_to_cockpit(xf.point_to_world(p)), p));

        // A vehicle turned like the body and sitting so its wheel is where the head expects it.
        let vehicle = Mat4::from_rotation_translation(xf.body, Vec3::new(100.5, 19.0, -49.0));
        let wheel = PartWheel {
            hub: Vec3::new(-0.4, 1.0, -0.4),
            radius: 0.17,
            tilt: 0.0,
        };
        let frame = xf.wheel(vehicle, &wheel).unwrap();
        assert!((frame.radius - 0.17 / 1.25).abs() < 1e-5);
        assert!(close(frame.right, Vec3::X) && close(frame.up, Vec3::Y));
        let world_hub = vehicle.transform_point3(wheel.hub);
        assert!(close(xf.point_to_world(frame.centre), world_hub));

        let local = xf.wheel_into(vehicle, &frame).unwrap();
        assert!(close(local.centre, wheel.hub));
        assert!((local.radius - 0.17).abs() < 1e-5);
        let back = xf.wheel_from(vehicle, &local).unwrap();
        assert!(close(back.centre, frame.centre) && close(back.up, frame.up));

        let bars = xf.bars(vehicle, Vec3::new(0.0, 0.9, -0.5), 0.3).unwrap();
        assert!((bars.half_width - 0.3 / 1.25).abs() < 1e-5);
    }
}
