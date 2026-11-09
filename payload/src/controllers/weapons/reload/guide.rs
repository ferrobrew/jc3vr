use std::time::Duration;

use glam::{Quat, Vec3};

use crate::controllers::{
    overlay::{
        primitive::{Anchor, DepthMode, HandPose, Primitive, Source},
        store,
    },
    weapons::reload::{
        State, Tick, guide::geometry::shape, insert::InsertStage, plan::WellFrame, tuning::Reload,
    },
};

mod geometry;

impl State {
    pub(super) fn insertion_guide(&self, tick: &Tick, enabled: bool, reload: &Reload) {
        let primitives = enabled
            .then(|| self.guide_primitives(tick, reload))
            .flatten();
        match primitives {
            Some(mut primitives) => {
                store::submit(source(), &mut primitives, Duration::from_millis(150));
            }
            None => clear(),
        }
    }

    fn guide_primitives(&self, tick: &Tick, reload: &Reload) -> Option<Vec<Primitive>> {
        let (_, holder) = self.status.insert_hands?;
        let (weapon, gun) = tick.gun(holder)?;
        if !self.status.insertion.holding()
            || self.status.insertion.float.is_some()
            || weapon.occupied()
            || !self.can_take(weapon)
        {
            return None;
        }
        let well = gun.well?;
        let capture = reload.for_plan(&weapon.plan);
        let (elapsed_s, caught) = match self.status.insertion.stage {
            InsertStage::Idle => return None,
            InsertStage::Holding { elapsed_s } => (elapsed_s, false),
            InsertStage::Captured { .. } => (0.0, true),
        };
        let sample = animation(
            elapsed_s,
            caught,
            capture.capture_radius_m,
            capture.seat_depth_m,
        )?;
        let shape = shape(weapon.reading.name_hash, &weapon.plan);
        let rotation = match (weapon.plan.well, gun.frame) {
            (WellFrame::Gun { .. }, Some(frame)) => frame.pose.orientation,
            _ => Quat::from_rotation_arc(shape.axis.try_normalize()?, well.axis.try_normalize()?),
        };
        let points = shape.segments;
        let mut primitives = Vec::new();
        let mut draw = |depth: f32, alpha: f32| {
            let top = well.point(Vec3::Y * depth)?;
            for &(a, b) in &points {
                let position = |p: Vec3| top + rotation * p / tick.world_scale;
                let anchor = |p: Vec3| Anchor::Hand {
                    hand: holder,
                    pose: HandPose::Wrist,
                    offset: gun.wrist.orientation.inverse()
                        * (p - gun.wrist.position)
                        * tick.world_scale,
                };
                primitives.push(Primitive::Line {
                    from: anchor(position(a)),
                    to: anchor(position(b)),
                    width: 0.0018 * tick.world_scale,
                    color: if caught {
                        [0.25, 1.0, 0.5, alpha]
                    } else {
                        [0.35, 0.85, 1.0, alpha]
                    },
                    depth: DepthMode::XRay {
                        occluded_alpha: 0.3,
                    },
                });
            }
            Some(())
        };
        if !(tick.world_scale.is_finite() && tick.world_scale > 0.0) {
            return None;
        }
        draw(capture.seat_depth_m, if caught { 0.55 } else { 0.18 })?;
        if !caught && sample.alpha > 0.0 {
            draw(sample.depth_m, sample.alpha * 0.65)?;
        }
        Some(primitives)
    }
}

pub(super) fn clear() {
    store::clear(source());
}

fn source() -> Source {
    Source::AmmoInsertionGuide
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Animation {
    depth_m: f32,
    alpha: f32,
}

fn animation(elapsed_s: f32, caught: bool, radius_m: f32, seat_m: f32) -> Option<Animation> {
    if ![elapsed_s, radius_m, seat_m]
        .into_iter()
        .all(f32::is_finite)
        || radius_m <= 0.0
        || seat_m < 0.0
    {
        return None;
    }
    if caught {
        return Some(Animation {
            depth_m: seat_m,
            alpha: 1.0,
        });
    }
    let phase = elapsed_s.max(0.0).rem_euclid(1.6) / 1.6;
    let travel = ((phase - 0.12) / 0.58).clamp(0.0, 1.0);
    let alpha = if phase < 0.12 {
        (phase / 0.12).min(1.0)
    } else {
        ((0.88 - phase) / 0.18).clamp(0.0, 1.0)
    };
    Some(Animation {
        depth_m: -radius_m + (radius_m + seat_m) * travel,
        alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::weapons::reload::insert::Well;

    #[test]
    fn the_preview_only_moves_inward_and_resets_while_invisible() {
        let mut previous = f32::NEG_INFINITY;
        for frame in 0..160 {
            let sample = animation(frame as f32 * 0.01, false, 0.16, 0.025).unwrap();
            assert!(sample.depth_m >= previous);
            previous = sample.depth_m;
        }
        assert_eq!(animation(1.59, false, 0.16, 0.025).unwrap().alpha, 0.0);
        assert_eq!(animation(1.6, false, 0.16, 0.025).unwrap().alpha, 0.0);
        assert_eq!(
            animation(0.0, true, 0.16, 0.025),
            animation(12.0, true, 0.16, 0.025)
        );
    }

    #[test]
    fn guide_and_capture_share_the_rotated_well_frame() {
        let well = Well {
            mouth: Vec3::new(2.0, 3.0, 4.0),
            axis: Vec3::new(1.0, 2.0, -1.0).normalize(),
        };
        for local in [Vec3::ZERO, Vec3::Y * 0.025, Vec3::new(0.04, -0.1, 0.03)] {
            let point = well.point(local).unwrap();
            assert!((well.local(point).unwrap() - local).length() < 1.0e-6);
        }
        assert!(
            (well.point(Vec3::Y * 0.025).unwrap() - well.mouth - well.axis * 0.025).length()
                < 1.0e-6
        );
    }
}
