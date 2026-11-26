use std::time::Duration;

use glam::{Quat, Vec3};
use jc3vr_view::controllers::overlay::{
    primitive::{Anchor, DepthMode, HandPose, Primitive, Source},
    store,
};

use crate::controllers::weapons::reload::{
    State, Tick,
    guide::geometry::shape,
    insert::InsertStage,
    plan::WellFrame,
    tuning::{GuideTuning, Reload},
};

mod geometry;

impl State {
    pub(super) fn insertion_guide(&self, tick: &Tick, reload: &Reload) {
        match self.guide_primitives(tick, reload) {
            Some(mut primitives) => {
                let ttl = Duration::from_millis(reload.guide.ttl_ms);
                store::submit(source(), &mut primitives, ttl);
            }
            None => clear(),
        }
    }

    fn guide_primitives(&self, tick: &Tick, reload: &Reload) -> Option<Vec<Primitive>> {
        let (_, holder) = self.status.insert_hands?;
        let (weapon, gun) = tick.gun(holder)?;
        if !self.status.insertion.holding()
            || self.status.insertion.float.is_some()
            || !self.can_take(weapon)
        {
            return None;
        }
        let well = gun.well?;
        let capture = reload.for_plan(&weapon.plan);
        let tuning = &reload.guide;
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
            tuning,
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
                    width: tuning.width_m * tick.world_scale,
                    color: if caught {
                        rgba(tuning.caught_rgb, alpha)
                    } else {
                        rgba(tuning.holding_rgb, alpha)
                    },
                    depth: DepthMode::XRay {
                        occluded_alpha: tuning.occluded_alpha,
                    },
                });
            }
            Some(())
        };
        if !(tick.world_scale.is_finite() && tick.world_scale > 0.0) {
            return None;
        }
        let seat_alpha = if caught {
            tuning.seat_caught_alpha
        } else {
            tuning.seat_holding_alpha
        };
        draw(capture.seat_depth_m, seat_alpha)?;
        if !caught && sample.alpha > 0.0 {
            draw(sample.depth_m, sample.alpha * tuning.preview_alpha)?;
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

fn rgba([r, g, b]: [f32; 3], alpha: f32) -> [f32; 4] {
    [r, g, b, alpha]
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Animation {
    depth_m: f32,
    alpha: f32,
}

fn animation(
    elapsed_s: f32,
    caught: bool,
    radius_m: f32,
    seat_m: f32,
    tuning: &GuideTuning,
) -> Option<Animation> {
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
    let period = tuning.preview_period_s;
    let fade_in = tuning.preview_fade_in_share;
    let phase = elapsed_s.max(0.0).rem_euclid(period) / period;
    let travel = ((phase - fade_in) / tuning.preview_travel_share).clamp(0.0, 1.0);
    let alpha = if phase < fade_in {
        (phase / fade_in).min(1.0)
    } else {
        ((tuning.preview_fade_out_end_share - phase) / tuning.preview_fade_out_share)
            .clamp(0.0, 1.0)
    };
    Some(Animation {
        depth_m: -radius_m + (radius_m + seat_m) * travel,
        alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::weapons::reload::{insert::Well, tuning::RELOAD};

    #[test]
    fn the_preview_only_moves_inward_and_resets_while_invisible() {
        let tuning = &RELOAD.get().guide;
        let period = tuning.preview_period_s;
        let mut previous = f32::NEG_INFINITY;
        for frame in 0..160 {
            let elapsed = period * frame as f32 / 160.0;
            let sample = animation(elapsed, false, 0.16, 0.025, tuning).unwrap();
            assert!(sample.depth_m >= previous);
            previous = sample.depth_m;
        }
        let last = period - 0.01;
        assert_eq!(
            animation(last, false, 0.16, 0.025, tuning).unwrap().alpha,
            0.0
        );
        assert_eq!(
            animation(period, false, 0.16, 0.025, tuning).unwrap().alpha,
            0.0
        );
        assert_eq!(
            animation(0.0, true, 0.16, 0.025, tuning),
            animation(12.0, true, 0.16, 0.025, tuning)
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
