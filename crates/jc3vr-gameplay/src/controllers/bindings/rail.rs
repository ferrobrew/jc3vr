//! Lateral movement leaves a native helicopter hang without changing the body pose policy.

use glam::{Quat, Vec3};
use jc3gi::input::input_action_map::Action;
use jc3vr_body::controllers::intent::hold::{Desired, MapKind, Target, Want};

use crate::controllers::tuning::GAMEPLAY;

#[derive(Debug, Default)]
pub(super) struct RailRelease {
    origin: Option<(u64, Vec3, Vec3)>,
    spent: bool,
}

impl RailRelease {
    pub(super) const fn new() -> Self {
        Self {
            origin: None,
            spent: false,
        }
    }

    pub(super) fn step(&mut self, input: RailInput, desired: &mut Vec<Desired>) -> bool {
        if !input.hanging {
            *self = Self::new();
            return false;
        }
        if self.spent {
            return false;
        }
        let tuning = &GAMEPLAY.get().bindings.rail;
        let lateral = desired.iter().any(|d| {
            d.target.map == MapKind::Player
                && matches!(d.target.action, Action::MOVE_LEFT | Action::MOVE_RIGHT)
                && match d.want {
                    Want::Analog(value) => value.is_finite() && value.abs() >= tuning.stick_push,
                    Want::Hold(value) => value.is_finite() && value.abs() >= tuning.stick_push,
                    Want::Click => true,
                }
        });
        let physical = input.head.is_some_and(|(position, orientation)| {
            if !position.is_finite() || !orientation.is_finite() {
                self.origin = None;
                return false;
            }
            let right = (orientation * Vec3::X).with_y(0.0).try_normalize();
            let Some(right) = right else {
                self.origin = None;
                return false;
            };
            let origin = self.origin.get_or_insert((input.epoch, position, right));
            if origin.0 != input.epoch {
                *origin = (input.epoch, position, right);
                return false;
            }
            (position - origin.1).dot(origin.2).abs() >= tuning.step_m
        });
        if input.head.is_none() {
            self.origin = None;
        }
        if !lateral && !physical {
            return false;
        }
        self.spent = true;
        // The native hang already uses JUMP to release; no parachute or suit is requested here.
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: Action::JUMP,
            },
            want: Want::Click,
        });
        true
    }
}

pub(super) struct RailInput {
    pub hanging: bool,
    pub epoch: u64,
    /// Raw tracking-space metres and heading, before the vehicle or body composition.
    pub head: Option<(Vec3, Quat)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(head: Vec3) -> RailInput {
        RailInput {
            hanging: true,
            epoch: 0,
            head: Some((head, Quat::IDENTITY)),
        }
    }

    #[test]
    fn a_lateral_step_releases_once_but_vertical_or_forward_motion_does_not() {
        let mut release = RailRelease::new();
        let mut desired = Vec::new();
        assert!(!release.step(tick(Vec3::ZERO), &mut desired));
        assert!(!release.step(tick(Vec3::new(0.1, 0.5, -0.5)), &mut desired));
        assert!(release.step(tick(Vec3::new(-0.21, 0.0, 0.0)), &mut desired));
        assert_eq!(desired.len(), 1);
        assert_eq!(desired[0].target.action, Action::JUMP);
        assert!(!release.step(tick(Vec3::X), &mut desired));
        assert_eq!(desired.len(), 1);
    }

    #[test]
    fn recenter_and_tracking_loss_rebase_instead_of_becoming_a_step() {
        let mut release = RailRelease::new();
        let mut desired = Vec::new();
        release.step(tick(Vec3::ZERO), &mut desired);
        assert!(!release.step(
            RailInput {
                epoch: 1,
                ..tick(Vec3::X)
            },
            &mut desired
        ));
        assert!(!release.step(
            RailInput {
                head: None,
                ..tick(Vec3::X)
            },
            &mut desired
        ));
        assert!(!release.step(tick(Vec3::ZERO), &mut desired));
        assert!(desired.is_empty());
    }

    #[test]
    fn the_entry_heading_defines_lateral_even_when_the_head_turns() {
        let mut release = RailRelease::new();
        let mut desired = Vec::new();
        release.step(
            RailInput {
                head: Some((
                    Vec3::ZERO,
                    Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                )),
                ..tick(Vec3::ZERO)
            },
            &mut desired,
        );
        assert!(release.step(tick(Vec3::Z * 0.21), &mut desired));
    }

    #[test]
    fn accepted_lateral_actions_release_without_tracking_but_not_outside_a_hang() {
        for hanging in [false, true] {
            let mut release = RailRelease::new();
            let mut desired = vec![Desired {
                target: Target {
                    map: MapKind::Player,
                    action: Action::MOVE_LEFT,
                },
                want: Want::Analog(0.8),
            }];
            assert_eq!(
                release.step(
                    RailInput {
                        hanging,
                        head: None,
                        ..tick(Vec3::ZERO)
                    },
                    &mut desired
                ),
                hanging
            );
            assert_eq!(
                desired.iter().any(|d| d.target.action == Action::JUMP),
                hanging
            );
        }
    }
}
