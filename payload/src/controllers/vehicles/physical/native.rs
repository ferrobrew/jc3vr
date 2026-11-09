use glam::{Mat4, Quat, Vec2, Vec3};

use crate::controllers::vehicles::{
    physical::ControlKind, tuning::PhysicalControlsTuning, wheel_render::RenderWheel,
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct NativeStick {
    pub kind: ControlKind,
    pub target: RenderWheel,
    pub part_to_cockpit: Mat4,
    pub pivot: Vec3,
    pub grip: Vec3,
}

impl NativeStick {
    pub fn top(self) -> Vec3 {
        self.part_to_cockpit.transform_point3(self.grip)
    }

    pub fn handle(self, deflection: Vec2) -> Vec3 {
        if deflection == Vec2::ZERO {
            return self.top();
        }
        let pivot = self.part_to_cockpit.transform_point3(self.pivot);
        let shaft = self.top() - pivot;
        let flat =
            (Vec2::new(shaft.x, shaft.z) + deflection).clamp_length_max(shaft.length() * 0.95);
        let y = (shaft.length_squared() - flat.length_squared())
            .max(0.0)
            .sqrt();
        pivot + Vec3::new(flat.x, y.copysign(shaft.y), flat.y)
    }

    pub fn motion(self, deflection: Vec2) -> Mat4 {
        if deflection == Vec2::ZERO {
            return Mat4::IDENTITY;
        }
        let inverse = self.part_to_cockpit.inverse();
        let from = self.grip - self.pivot;
        let to = inverse.transform_point3(self.handle(deflection)) - self.pivot;
        let rotation = Quat::from_rotation_arc(from.normalize(), to.normalize());
        Mat4::from_translation(self.pivot)
            * Mat4::from_quat(rotation)
            * Mat4::from_translation(-self.pivot)
    }
}

/// The measured grips in one seat of a merged aircraft cockpit mesh.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CockpitAsset {
    pub seat: u8,
    pub stick_kind: ControlKind,
    pub stick_grip: Vec3,
    pub lever: Option<(ControlKind, Vec3)>,
}

/// The live part transform for a cockpit whose controls share its body mesh, and the lever the
/// mesh does not model. The part identifies the fit but is never submitted for joystick animation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CockpitFit {
    pub target: RenderWheel,
    pub part_to_cockpit: Mat4,
    pub asset: CockpitAsset,
    /// The lever the mesh does not model, placed from the tuning relative to the pilot's eyes and
    /// kept inside the part's bounds, in the part's own space.
    pub fallback_lever: Option<(ControlKind, Vec3)>,
}

impl CockpitFit {
    /// A fit of `asset` in `part_to_cockpit`. A lever the mesh does not model is placed from
    /// `physical` relative to the pilot's eyes and kept inside the part's `bounds`, in the part's
    /// own space.
    pub fn new(
        target: RenderWheel,
        part_to_cockpit: Mat4,
        asset: CockpitAsset,
        bounds: (Vec3, Vec3),
        physical: &PhysicalControlsTuning,
    ) -> Self {
        // A missing lever is placed from the tuning relative to the pilot's eyes, not from the
        // measured stick: the stick's height is the mesh's, while the spec's lever sits by the
        // seat. The part's bounds keep the placement inside the airframe.
        let missing = match (asset.lever, asset.stick_kind) {
            (None, ControlKind::Cyclic) => Some((ControlKind::Collective, physical.collective)),
            (None, ControlKind::FlightStick) => Some((ControlKind::Throttle, physical.throttle)),
            _ => None,
        };
        let fallback_lever = missing.map(|(kind, spec)| {
            let rest = part_to_cockpit.inverse().transform_point3(spec.rest_m);
            (kind, rest.clamp(bounds.0, bounds.1))
        });
        Self {
            target,
            part_to_cockpit,
            asset,
            fallback_lever,
        }
    }

    pub fn stick_top(self) -> Vec3 {
        self.part_to_cockpit.transform_point3(self.asset.stick_grip)
    }

    pub fn stick_handle(self, deflection: Vec2) -> Vec3 {
        self.stick_top()
            + self.direction(Vec3::X) * deflection.x
            + self.direction(Vec3::Z) * deflection.y
    }

    pub fn stick_displacement(self, point: Vec3) -> Vec3 {
        let delta = point - self.stick_top();
        Vec3::new(
            delta.dot(self.direction(Vec3::X)),
            0.0,
            delta.dot(self.direction(Vec3::Z)),
        )
    }

    pub fn direction(self, axis: Vec3) -> Vec3 {
        self.part_to_cockpit
            .transform_vector3(axis)
            .normalize_or_zero()
    }

    pub fn lever_rest(self, kind: ControlKind) -> Option<Vec3> {
        let (lever_kind, grip) = self.asset.lever.or(self.fallback_lever)?;
        (kind == lever_kind).then(|| self.part_to_cockpit.transform_point3(grip))
    }

    pub fn contains(self, kind: ControlKind) -> bool {
        self.asset.stick_kind == kind
            || self.asset.lever.is_some_and(|(lever, _)| lever == kind)
            || self.fallback_lever.is_some_and(|(lever, _)| lever == kind)
    }
}

/// Measure the merged cockpit in its part's coordinates. The pilot selects a physical seat;
/// controller handedness does not change which seat's controls are offered.
pub fn cockpit_asset(name: &str, pilot_in_part: Vec3) -> Option<CockpitAsset> {
    if !pilot_in_part.is_finite() {
        return None;
    }
    match name {
        "utility_body" => {
            let right = pilot_in_part.x > 0.0;
            Some(CockpitAsset {
                seat: u8::from(right),
                stick_kind: ControlKind::Cyclic,
                // The grip shell's center comes from the connected components of the body mesh.
                stick_grip: Vec3::new(
                    if right { 0.383715 } else { -0.383725 },
                    1.472375,
                    -1.390_84,
                ),
                lever: None,
            })
        }
        "fighterjet_body" => Some(CockpitAsset {
            seat: 0,
            stick_kind: ControlKind::FlightStick,
            stick_grip: Vec3::new(0.0, 1.99, -2.927423),
            lever: Some((
                ControlKind::Throttle,
                Vec3::new(-0.235000, 1.910038, -2.872874),
            )),
        }),
        "fighterbomber_body" => Some(CockpitAsset {
            seat: 0,
            stick_kind: ControlKind::FlightStick,
            stick_grip: Vec3::new(0.005481, 2.607, -3.095483),
            lever: Some((
                ControlKind::Throttle,
                Vec3::new(-0.245000, 2.511149, -3.057425),
            )),
        }),
        _ => None,
    }
}

pub fn asset(name: &str) -> Option<(ControlKind, Vec3, Vec3)> {
    match name {
        "lightassault_joystick" => Some((
            ControlKind::Cyclic,
            Vec3::new(-0.57419, 0.96796, -1.28541),
            Vec3::new(-0.57313, 1.86677, -1.14495),
        )),
        "smallfloatprop_controlwheel" => Some((
            ControlKind::FlightStick,
            Vec3::new(0.0, 1.64364, -0.768),
            Vec3::new(0.00983, 2.13808, -0.64207),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{
        frame::Hand,
        vehicles::{
            physical::{ControlState, Hold, Placement, anchor_point},
            tuning::PHYSICAL,
        },
    };

    fn stick(name: &str, part_to_cockpit: Mat4) -> NativeStick {
        let (kind, pivot, grip) = asset(name).unwrap();
        NativeStick {
            kind,
            target: RenderWheel {
                vehicle: 1,
                part_index: 2,
                properties: 3,
                hub: pivot,
                axis: Vec3::Y,
            },
            part_to_cockpit,
            pivot,
            grip,
        }
    }

    #[test]
    fn native_rotation_preserves_the_pivot_and_matches_the_grab() {
        for name in ["lightassault_joystick", "smallfloatprop_controlwheel"] {
            for transform in [
                Mat4::IDENTITY,
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(0.7),
                    Quat::from_rotation_y(0.8),
                    Vec3::new(2.0, -1.0, 4.0),
                ),
            ] {
                let stick = stick(name, transform);
                for deflection in [Vec2::ZERO, Vec2::new(0.12, -0.08), Vec2::splat(20.0)] {
                    let motion = stick.motion(deflection);
                    assert!(motion.is_finite());
                    assert!(motion.transform_point3(stick.pivot).distance(stick.pivot) < 1e-5);
                    let drawn = transform.transform_point3(motion.transform_point3(stick.grip));
                    assert!(drawn.distance(stick.handle(deflection)) < 1e-5);
                }
                assert!(stick.handle(Vec2::ZERO).distance(stick.top()) < 1e-5);
            }
        }
    }

    #[test]
    fn native_grabs_and_hand_anchors_follow_the_drawn_mesh_without_mirroring() {
        for name in ["lightassault_joystick", "smallfloatprop_controlwheel"] {
            for dominant in Hand::BOTH {
                let native = stick(name, Mat4::IDENTITY);
                let mut placement = Placement::new(PHYSICAL, dominant);
                placement.native_stick = Some(native);
                let state = ControlState {
                    stick: Vec2::new(0.04, -0.03),
                    ..ControlState::default()
                };
                let handle = placement
                    .grab_point(&state, native.kind, dominant, Vec3::ZERO)
                    .unwrap();
                let drawn = native.motion(state.stick).transform_point3(native.grip);
                assert!(handle.distance(drawn) < 1e-5);
                let hold = Hold {
                    kind: native.kind,
                    offset: Vec3::ZERO,
                    orientation: None,
                };
                let mut grips = [None; 2];
                grips[dominant.index()] = Some(handle);
                let anchor = anchor_point(&state, &placement, hold, dominant, grips).unwrap();
                assert!(anchor.distance(drawn) < 1e-5);
            }
        }
    }

    #[test]
    fn merged_cockpit_axes_and_grips_follow_the_part_without_handedness_mirroring() {
        let asset = CockpitAsset {
            seat: 0,
            stick_kind: ControlKind::FlightStick,
            stick_grip: Vec3::new(0.0, 1.5, -3.0),
            lever: Some((ControlKind::Throttle, Vec3::new(-0.4, 1.3, -2.8))),
        };
        for dominant in Hand::BOTH {
            for rotation in [
                Quat::IDENTITY,
                Quat::from_rotation_y(1.1) * Quat::from_rotation_x(0.4),
            ] {
                let fit = CockpitFit::new(
                    stick("lightassault_joystick", Mat4::IDENTITY).target,
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.7),
                        rotation,
                        Vec3::new(2.0, -1.0, 4.0),
                    ),
                    asset,
                    (Vec3::splat(-10.0), Vec3::splat(10.0)),
                    &PHYSICAL,
                );
                let mut placement = Placement::new(PHYSICAL, dominant);
                placement.cockpit = Some(fit);
                let state = ControlState::default();
                for kind in [ControlKind::FlightStick, ControlKind::Throttle] {
                    let model = if kind == ControlKind::FlightStick {
                        asset.stick_grip
                    } else {
                        asset.lever.unwrap().1
                    };
                    let actual = fit.part_to_cockpit.transform_point3(model);
                    let grab = placement
                        .grab_point(&state, kind, dominant, actual)
                        .unwrap();
                    assert!(grab.distance(actual) < 1e-5);
                    let offset = Vec3::new(0.01, -0.01, 0.02);
                    let hold = Hold {
                        kind,
                        offset,
                        orientation: None,
                    };
                    let moved = if kind == ControlKind::FlightStick {
                        fit.stick_handle(Vec2::new(0.04, -0.03))
                    } else {
                        let (_, spec) = placement.lever(kind).unwrap();
                        let moved = placement.lever_handle(kind, &spec, 0.5);
                        assert!((placement.lever_value(kind, &spec, moved) - 0.5).abs() < 1e-5);
                        moved
                    };
                    let mut grips = [None; 2];
                    grips[dominant.index()] = Some(moved + offset);
                    let anchor = anchor_point(&state, &placement, hold, dominant, grips).unwrap();
                    assert!(anchor.distance(moved + offset) < 1e-5);
                }
            }
        }
    }

    #[test]
    fn merged_cockpit_refresh_tracks_the_part_and_seat_but_not_vehicle_motion() {
        let fit = CockpitFit::new(
            stick("lightassault_joystick", Mat4::IDENTITY).target,
            Mat4::IDENTITY,
            CockpitAsset {
                seat: 0,
                stick_kind: ControlKind::Cyclic,
                stick_grip: Vec3::ZERO,
                lever: Some((ControlKind::Collective, Vec3::NEG_X)),
            },
            (Vec3::splat(-10.0), Vec3::splat(10.0)),
            &PHYSICAL,
        );
        let mut state = ControlState {
            holds: [ControlKind::Collective, ControlKind::Cyclic].map(|kind| {
                Some(Hold {
                    kind,
                    offset: Vec3::ZERO,
                    orientation: None,
                })
            }),
            levers: [0.6, 0.0],
            ..Default::default()
        };
        let mut events = Vec::new();
        let moved = CockpitFit {
            part_to_cockpit: Mat4::from_translation(Vec3::ONE),
            ..fit
        };
        crate::controllers::vehicles::physical::refresh_cockpit(
            &mut state,
            Some(fit),
            Some(moved),
            &mut events,
        );
        assert!(events.is_empty());
        assert!(state.holds.iter().all(Option::is_some));
        let changed = CockpitFit {
            asset: CockpitAsset {
                seat: 1,
                ..fit.asset
            },
            ..fit
        };
        crate::controllers::vehicles::physical::refresh_cockpit(
            &mut state,
            Some(fit),
            Some(changed),
            &mut events,
        );
        assert_eq!(state.holds, [None; 2]);
        assert_eq!(state.levers, [0.0; 2]);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn utility_uses_the_authored_left_driver_seat_and_only_falls_back_for_the_collective() {
        let driver = cockpit_asset("utility_body", Vec3::new(-0.35, 0.74, -0.92)).unwrap();
        let passenger = cockpit_asset("utility_body", Vec3::new(0.35, 0.74, -0.92)).unwrap();
        assert_eq!(driver.seat, 0);
        assert_eq!(passenger.seat, 1);
        assert!(driver.lever.is_none());
        assert!(
            driver
                .stick_grip
                .cmpge(Vec3::new(-0.40782, 1.37331, -1.42878))
                .all()
        );
        assert!(
            driver
                .stick_grip
                .cmple(Vec3::new(-0.35963, 1.57144, -1.35290))
                .all()
        );
        assert!(passenger.stick_grip.x > 0.35);
        let bounds = (
            Vec3::new(-0.800297, 0.6425335, -3.0365307),
            Vec3::new(0.9101837, 2.977942, 2.4216115),
        );
        let pilot = Vec3::new(-0.35, 1.5, -0.92);
        for dominant in Hand::BOTH {
            let mut placement = Placement::new(PHYSICAL, dominant);
            let fit = CockpitFit::new(
                stick("lightassault_joystick", Mat4::IDENTITY).target,
                Mat4::from_translation(-pilot),
                driver,
                bounds,
                &PHYSICAL,
            );
            placement.cockpit = Some(fit);
            let collective = placement
                .grab_point(
                    &ControlState::default(),
                    ControlKind::Collective,
                    dominant,
                    Vec3::ZERO,
                )
                .unwrap();
            // The missing lever is placed from the tuning relative to the pilot, not from the
            // measured cyclic, and it stays inside the part.
            assert!((collective - PHYSICAL.collective.rest_m).length() < 1e-5);
            let (_, part_rest) = fit.fallback_lever.unwrap();
            assert!(part_rest.cmpge(bounds.0).all() && part_rest.cmple(bounds.1).all());
            assert!(collective.x < fit.stick_top().x);
            assert!(part_rest.x > bounds.0.x + 0.1);
            assert!(collective.y < fit.stick_top().y - 0.5);
            assert!(fit.contains(ControlKind::Collective));
            assert!(!fit.contains(ControlKind::Throttle));
        }
        assert!(cockpit_asset("utility_body", Vec3::splat(f32::NAN)).is_none());
        assert!(cockpit_asset("unknown_body", Vec3::ZERO).is_none());
    }

    #[test]
    fn a_missing_throttle_uses_the_same_placement_rule() {
        let fit = CockpitFit::new(
            stick("lightassault_joystick", Mat4::IDENTITY).target,
            Mat4::IDENTITY,
            CockpitAsset {
                seat: 0,
                stick_kind: ControlKind::FlightStick,
                stick_grip: Vec3::ZERO,
                lever: None,
            },
            (Vec3::splat(-1.0), Vec3::splat(1.0)),
            &PHYSICAL,
        );
        let (kind, rest) = fit.fallback_lever.unwrap();
        assert_eq!(kind, ControlKind::Throttle);
        assert!((rest - PHYSICAL.throttle.rest_m).length() < 1e-5);
        assert!(fit.lever_rest(ControlKind::Throttle).is_some());
        assert!(fit.lever_rest(ControlKind::Collective).is_none());
    }

    #[test]
    fn both_jets_use_the_grip_components_and_the_left_throttle() {
        let fixtures = [
            (
                "fighterjet_body",
                Vec3::new(0.0, 1.6, -2.6),
                (
                    Vec3::new(-0.044064, 1.929756, -2.997034),
                    Vec3::new(0.044064, 2.084456, -2.906799),
                ),
                (
                    Vec3::new(-0.325838, 1.879053, -2.954679),
                    Vec3::new(-0.131491, 1.967957, -2.857686),
                ),
            ),
            (
                "fighterbomber_body",
                Vec3::new(0.0, 2.199951, -2.762207),
                (
                    Vec3::new(-0.020820, 2.547414, -3.131091),
                    Vec3::new(0.046177, 2.656412, -3.061911),
                ),
                (
                    Vec3::new(-0.299796, 2.478388, -3.106),
                    Vec3::new(-0.153286, 2.545039, -3.028942),
                ),
            ),
        ];
        for (name, pilot, stick_bounds, throttle_bounds) in fixtures {
            let asset = cockpit_asset(name, pilot).unwrap();
            let (kind, throttle) = asset.lever.unwrap();
            assert_eq!(kind, ControlKind::Throttle);
            assert!(asset.stick_grip.cmpge(stick_bounds.0).all());
            assert!(asset.stick_grip.cmple(stick_bounds.1).all());
            assert!(throttle.cmpge(throttle_bounds.0).all());
            assert!(throttle.cmple(throttle_bounds.1).all());
            assert!(throttle.x < pilot.x - 0.2);
            assert!((asset.stick_grip.x - pilot.x).abs() < 0.01);
            assert!((0.3..0.4).contains(&(pilot.z - asset.stick_grip.z)));
            assert!((0.3..0.5).contains(&(asset.stick_grip.y - pilot.y)));
            for dominant in Hand::BOTH {
                let mut placement = Placement::new(PHYSICAL, dominant);
                placement.cockpit = Some(CockpitFit::new(
                    stick("lightassault_joystick", Mat4::IDENTITY).target,
                    Mat4::from_translation(-pilot),
                    asset,
                    (Vec3::splat(-10.0), Vec3::splat(10.0)),
                    &PHYSICAL,
                ));
                for (kind, measured) in [
                    (ControlKind::FlightStick, asset.stick_grip),
                    (ControlKind::Throttle, throttle),
                ] {
                    let handle = placement
                        .grab_point(&ControlState::default(), kind, dominant, Vec3::ZERO)
                        .unwrap();
                    assert!(handle.distance(measured - pilot) < 1e-5);
                    let mut grips = [None; 2];
                    grips[dominant.index()] = Some(handle);
                    let candidates = crate::controllers::vehicles::physical::candidates(
                        &placement,
                        &ControlState::default(),
                        &[kind],
                        grips,
                    );
                    assert_eq!(candidates.len(), 1);
                    assert!(candidates[0].centre.distance(handle) < 1e-5);
                }
            }
        }
    }

    #[test]
    fn unsupported_assemblies_do_not_match() {
        for name in [
            "assault_body",
            "heavytroop_joystick_l",
            "privatepropeller_body",
        ] {
            assert!(asset(name).is_none());
        }
    }
}
