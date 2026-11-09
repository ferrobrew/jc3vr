use std::time::Duration;

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    equipment::weapon::WeaponBase,
    graphics_engine::model_instance::{ModelInstance, ModelInstanceManager},
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    overlay::{
        primitive::{Anchor, DepthMode, HandPose, Primitive, Source},
        store,
    },
    weapons::reload::{
        State, Tick, engine,
        insert::InsertStage,
        plan::{LoadKind, ReloadPlan},
        tuning::Reload,
    },
};

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
        if !self.status.insertion.holding() || weapon.occupied() || !self.can_take(weapon) {
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
        let shape = unsafe { shape(weapon.address, &weapon.plan) }?;
        let rotation = match gun.frame {
            Some(frame) => (frame.pose.orientation * shape.in_gun).normalize(),
            None => {
                Quat::from_rotation_arc(shape.axis.try_normalize()?, well.axis.try_normalize()?)
            }
        };
        let points = shape.segments();
        let mut primitives = Vec::new();
        let mut draw = |depth: f32, alpha: f32| {
            let top = well.point(Vec3::Y * depth)?;
            for &(a, b) in &points {
                let position = |p: Vec3| top + rotation * (p - shape.top) / tick.world_scale;
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
    CACHE.lock().take();
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
    let travel = (phase / 0.78).clamp(0.0, 1.0);
    let alpha = (phase / 0.12).min(1.0) * ((0.94 - phase) / 0.16).clamp(0.0, 1.0);
    Some(Animation {
        depth_m: -radius_m + (radius_m + seat_m) * travel,
        alpha,
    })
}

#[derive(Clone)]
struct Outline {
    min: Vec3,
    max: Vec3,
    top: Vec3,
    axis: Vec3,
    in_gun: Quat,
    offsets: Vec<Vec3>,
    round: Option<f32>,
}

impl Outline {
    fn segments(&self) -> Vec<(Vec3, Vec3)> {
        let mut segments = Vec::new();
        for offset in &self.offsets {
            if let Some(radius) = self.round {
                let axis = self.axis;
                let across = axis.any_orthonormal_vector();
                let along = |p: Vec3| p.dot(axis);
                let corners = box_corners(self.min, self.max);
                let back = corners.into_iter().map(along).fold(f32::INFINITY, f32::min);
                let front = self.top.dot(axis);
                let centre = self.top - axis * front + offset;
                for i in 0..12 {
                    let a = std::f32::consts::TAU * i as f32 / 12.0;
                    let b = std::f32::consts::TAU * (i + 1) as f32 / 12.0;
                    let radial = |angle| Quat::from_axis_angle(axis, angle) * across * radius;
                    segments.push((
                        centre + axis * front + radial(a),
                        centre + axis * front + radial(b),
                    ));
                    if back < front {
                        segments.push((
                            centre + axis * back + radial(a),
                            centre + axis * back + radial(b),
                        ));
                    }
                    if back < front && i % 3 == 0 {
                        segments.push((
                            centre + axis * back + radial(a),
                            centre + axis * front + radial(a),
                        ));
                    }
                }
            } else {
                let corners = box_corners(self.min, self.max);
                for (i, corner) in corners.iter().enumerate() {
                    for bit in [1, 2, 4] {
                        let other = i ^ bit;
                        if i < other {
                            segments.push((*corner + offset, corners[other] + offset));
                        }
                    }
                }
            }
        }
        segments
    }
}

fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    })
}

unsafe fn shape(weapon: usize, plan: &ReloadPlan) -> Option<Outline> {
    let mut cached = CACHE.lock();
    if let Some((address, prior, outline)) = cached.as_ref()
        && *address == weapon
        && prior == plan
    {
        return Some(outline.clone());
    }
    let native = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let (top, axis, in_gun, offsets, round, bounds) = if let Some(round) = plan.round {
        let bounds =
            unsafe { cached_round_bounds(round.name_hash) }.unwrap_or((round.top, round.top));
        (
            round.top,
            round.axis,
            Quat::from_rotation_arc(round.axis, plan.part_axis),
            round.offsets().collect(),
            Some(round.radius_m),
            bounds,
        )
    } else if let Some(top) = plan.spare_top_m {
        let first = unsafe { gun::guns(native) }.next().flatten()?;
        let magazine = unsafe { first.m_MagazineObject.px.as_ref() }?;
        let instance = unsafe { magazine.m_ModelInstance.as_ref() }?;
        (
            top,
            plan.part_axis,
            Quat::IDENTITY,
            vec![Vec3::ZERO],
            None,
            unsafe { bounds(instance) }?,
        )
    } else {
        let top = plan.part_axis * plan.part_top_m?;
        let part = unsafe { engine::magazine_part(native) }?;
        let instance = unsafe { part.m_BaseModelInstance.as_ref() }?;
        let bounds = unsafe { bounds(instance) }?;
        let radius = (plan.load == LoadKind::Rocket).then(|| {
            let axis = plan.part_axis;
            box_corners(bounds.0, bounds.1)
                .into_iter()
                .map(|point| {
                    let delta = point - top;
                    (delta - axis * delta.dot(axis)).length()
                })
                .fold(0.0_f32, f32::max)
        });
        (
            top,
            plan.part_axis,
            Quat::IDENTITY,
            vec![Vec3::ZERO],
            radius,
            bounds,
        )
    };
    let outline = Outline {
        min: bounds.0,
        max: bounds.1,
        top,
        axis,
        in_gun,
        offsets,
        round,
    };
    *cached = Some((weapon, *plan, outline.clone()));
    Some(outline)
}

unsafe fn bounds(instance: &ModelInstance) -> Option<(Vec3, Vec3)> {
    let mut min = Vector3 { data: [0.0; 3] };
    let mut max = Vector3 { data: [0.0; 3] };
    unsafe { instance.GetModelSpaceBoundingBox(&mut min, &mut max) };
    let (min, max) = (Vec3::from_array(min.data), Vec3::from_array(max.data));
    (min.is_finite() && max.is_finite() && max.cmpgt(min).all()).then_some((min, max))
}

unsafe fn cached_round_bounds(name_hash: u32) -> Option<(Vec3, Vec3)> {
    let manager = unsafe { ModelInstanceManager::get() }?;
    let instance = unsafe {
        manager.CreateInstanceFromHash(
            name_hash,
            &Matrix4::from(Mat4::IDENTITY),
            false,
            false,
            false,
        )
    };
    let instance = unsafe { instance.as_mut() }?;
    let result = unsafe { bounds(instance) };
    unsafe { manager.DeleteInstance(instance) };
    result
}

static CACHE: Mutex<Option<(usize, ReloadPlan, Outline)>> = Mutex::new(None);

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
