use glam::{Mat4, Quat, Vec2, Vec3};

use crate::controllers::vehicles::{physical::ControlKind, wheel_render::RenderWheel};

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
