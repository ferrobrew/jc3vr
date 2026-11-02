//! Where the cockpit frame sits in the world, estimated, because a recording does not hold the
//! view's world transform.
//!
//! The camera is the anchor the root carries, plus the cockpit position less the cockpit shift,
//! turned by the view's rotation and scaled by the world scale (`docs/mod/body/rig.md`, "Who moves
//! the root"). The anchor stands the eyes over the root, so the floor under the cockpit is at the
//! root's height. A recording holds the root's world position (the root tick), the cockpit shift,
//! the world scale, and the eyes' height over the floor (the STAGE space), but not the view's
//! heading.
//!
//! The heading is recovered from the hands. Each controller state holds the drawn wrists' world
//! positions, and each frame holds the grips in the cockpit; the heading is the yaw that best turns
//! the grips about the root's place in the cockpit onto the wrists about the root, pooled over a
//! short window and never across a recenter. A wrist sits a little off its grip, at an offset fixed
//! in the controller's frame; the fit estimates each hand's offset from a first pass and fits again
//! with it, a few times over. Without a root tick (in a vehicle, say) the wrists' centroid stands
//! in for the root, and with neither the last placement holds.

use glam::{Quat, Vec3};
use tracking_format::model::Pose;

/// What one frame offers the fit.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Correspondence {
    /// The root's world position, while the root's hook runs.
    pub root: Option<Vec3>,
    /// The root's place in the cockpit, horizontally: the cockpit shift.
    pub shift: Vec3,
    /// The floor's height in the cockpit.
    pub floor: f32,
    /// The grips in the cockpit, while tracked.
    pub grips: [Option<Pose>; 2],
    /// The drawn wrists in the world, while the late correction places them.
    pub wrists: [Option<Vec3>; 2],
    /// The world scale.
    pub scale: f32,
    /// The recenter segment: the heading is pooled only within one.
    pub segment: u32,
}

/// The cockpit frame's placement in the world: `world = translation + rotation * (scale * cockpit)`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Placement {
    pub translation: Vec3,
    /// The heading, radians about +Y.
    pub yaw: f32,
    pub scale: f32,
    pub anchor: Anchor,
}

impl Placement {
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }

    /// A cockpit point in the world.
    pub fn apply(&self, point: Vec3) -> Vec3 {
        self.translation + self.rotation() * (point * self.scale)
    }
}

/// What placed the cockpit on a frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// The root tick, with the heading from the wrists.
    Root,
    /// The wrists alone.
    Wrists,
    /// Nothing this frame: the last placement holds.
    Held,
}

impl Anchor {
    pub fn label(self) -> &'static str {
        match self {
            Anchor::Root => "root",
            Anchor::Wrists => "wrists",
            Anchor::Held => "held",
        }
    }
}

/// The fit over a stretch.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// Each frame's placement; absent until the first frame that can be placed.
    pub placements: Vec<Option<Placement>>,
    /// Each hand's wrist offset in its grip's frame, metres.
    pub offsets: [Vec3; 2],
    /// Each frame's mean distance between a registered grip's wrist and the recorded wrist.
    pub wrist_errors: Vec<Option<f32>>,
}

/// Place each of `samples`, pooling the heading over `window` frames either side.
pub fn place(samples: &[Correspondence], window: usize) -> Fit {
    let mut offsets = [Vec3::ZERO; 2];
    let mut placements = pass(samples, window, offsets);
    for _ in 0..OFFSET_PASSES {
        offsets = [0, 1].map(|hand| estimate_offset(samples, &placements, hand));
        placements = pass(samples, window, offsets);
    }
    let wrist_errors = samples
        .iter()
        .zip(&placements)
        .map(|(sample, placement)| wrist_error(sample, placement.as_ref()?, offsets))
        .collect();
    Fit {
        placements,
        offsets,
        wrist_errors,
    }
}

/// How many times the wrist offsets are estimated again from the placements they gave.
const OFFSET_PASSES: usize = 8;

/// The largest wrist offset the fit accepts, metres: a controller's wrist is a hand's breadth from
/// its grip at most.
const MAX_OFFSET_M: f32 = 0.2;

/// The least pooled weight a heading is taken from, square metres: two hands a few centimetres
/// from the anchor say little about the heading.
const MIN_WEIGHT: f32 = 0.01;

fn pass(samples: &[Correspondence], window: usize, offsets: [Vec3; 2]) -> Vec<Option<Placement>> {
    let terms: Vec<(f32, f32)> = samples.iter().map(|s| heading_terms(s, offsets)).collect();
    let mut placements = Vec::with_capacity(samples.len());
    let mut last: Option<Placement> = None;
    for (index, sample) in samples.iter().enumerate() {
        let lo = index.saturating_sub(window);
        let hi = (index + window + 1).min(samples.len());
        let (dot, cross) = (lo..hi)
            .filter(|&j| samples[j].segment == sample.segment)
            .fold((0.0, 0.0), |(d, c), j| (d + terms[j].0, c + terms[j].1));
        let yaw = if dot.hypot(cross) >= MIN_WEIGHT {
            Some(cross.atan2(dot))
        } else {
            last.map(|p| p.yaw)
        };
        let placement = yaw
            .and_then(|yaw| anchor(sample, yaw, offsets))
            .or(last.map(|p| Placement {
                anchor: Anchor::Held,
                ..p
            }));
        last = placement.or(last);
        placements.push(placement);
    }
    placements
}

/// A grip's wrist in the cockpit.
fn wrist_in_cockpit(grip: &Pose, offset: Vec3) -> Vec3 {
    grip.position + grip.orientation * offset
}

/// The pairs of horizontal vectors the heading turns one onto the other: the cockpit's about its
/// anchor, scaled, and the world's about its anchor.
fn pairs(sample: &Correspondence, offsets: [Vec3; 2]) -> Vec<(Vec3, Vec3)> {
    let hands: Vec<(Vec3, Vec3)> = (0..2)
        .filter_map(|hand| {
            Some((
                wrist_in_cockpit(sample.grips[hand].as_ref()?, offsets[hand]),
                sample.wrists[hand]?,
            ))
        })
        .collect();
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    match sample.root {
        Some(root) => hands
            .iter()
            .map(|(c, w)| (flat(*c - sample.shift) * sample.scale, flat(*w - root)))
            .collect(),
        None if hands.len() == 2 => {
            let c0 = (hands[0].0 + hands[1].0) * 0.5;
            let w0 = (hands[0].1 + hands[1].1) * 0.5;
            hands
                .iter()
                .map(|(c, w)| (flat(*c - c0) * sample.scale, flat(*w - w0)))
                .collect()
        }
        None => Vec::new(),
    }
}

/// The sums whose angle is the heading that best turns a frame's cockpit vectors onto its world
/// vectors: `atan2(cross, dot)` under [`Quat::from_rotation_y`].
fn heading_terms(sample: &Correspondence, offsets: [Vec3; 2]) -> (f32, f32) {
    pairs(sample, offsets)
        .iter()
        .fold((0.0, 0.0), |(dot, cross), (c, w)| {
            (dot + c.x * w.x + c.z * w.z, cross + c.z * w.x - c.x * w.z)
        })
}

/// The placement a heading gives a frame, from its root or its wrists.
fn anchor(sample: &Correspondence, yaw: f32, offsets: [Vec3; 2]) -> Option<Placement> {
    let rotation = Quat::from_rotation_y(yaw);
    let s = sample.scale;
    if let Some(root) = sample.root {
        let shift = Vec3::new(sample.shift.x, 0.0, sample.shift.z);
        let translation =
            Vec3::new(root.x, root.y - s * sample.floor, root.z) - rotation * (shift * s);
        return Some(Placement {
            translation,
            yaw,
            scale: s,
            anchor: Anchor::Root,
        });
    }
    let hands: Vec<(Vec3, Vec3)> = (0..2)
        .filter_map(|hand| {
            Some((
                wrist_in_cockpit(sample.grips[hand].as_ref()?, offsets[hand]),
                sample.wrists[hand]?,
            ))
        })
        .collect();
    let [left, right] = <[(Vec3, Vec3); 2]>::try_from(hands).ok()?;
    let cockpit = (left.0 + right.0) * 0.5;
    let world = (left.1 + right.1) * 0.5;
    Some(Placement {
        translation: world - rotation * (cockpit * s),
        yaw,
        scale: s,
        anchor: Anchor::Wrists,
    })
}

/// A hand's wrist offset in its grip's frame: the median, per axis, of where the recorded wrist sits
/// from the grip under the root-anchored placements.
fn estimate_offset(
    samples: &[Correspondence],
    placements: &[Option<Placement>],
    hand: usize,
) -> Vec3 {
    let mut axes: [Vec<f32>; 3] = Default::default();
    for (sample, placement) in samples.iter().zip(placements) {
        let (Some(placement), Some(grip), Some(wrist)) =
            (placement, sample.grips[hand], sample.wrists[hand])
        else {
            continue;
        };
        if placement.anchor != Anchor::Root || placement.scale <= 0.0 {
            continue;
        }
        let cockpit =
            placement.rotation().inverse() * (wrist - placement.translation) / placement.scale;
        let local = grip.orientation.inverse() * (cockpit - grip.position);
        if !local.is_finite() || local.length() > MAX_OFFSET_M * 2.0 {
            continue;
        }
        for (axis, value) in axes.iter_mut().zip(local.to_array()) {
            axis.push(value);
        }
    }
    let median = |values: &mut Vec<f32>| {
        values.sort_by(f32::total_cmp);
        values.get(values.len() / 2).copied().unwrap_or(0.0)
    };
    let [x, y, z] = &mut axes;
    Vec3::new(median(x), median(y), median(z)).clamp_length_max(MAX_OFFSET_M)
}

fn wrist_error(sample: &Correspondence, placement: &Placement, offsets: [Vec3; 2]) -> Option<f32> {
    let errors: Vec<f32> = (0..2)
        .filter_map(|hand| {
            let grip = sample.grips[hand]?;
            let wrist = sample.wrists[hand]?;
            Some(
                placement
                    .apply(wrist_in_cockpit(&grip, offsets[hand]))
                    .distance(wrist),
            )
        })
        .collect();
    (!errors.is_empty()).then(|| errors.iter().sum::<f32>() / errors.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    /// A frame of a cockpit placed at `truth`, with the wrists `offset` off the grips.
    fn sample(truth: &Placement, root: Vec3, grips: [Pose; 2], offset: Vec3) -> Correspondence {
        // The root's place in the cockpit, from the placement's own definition.
        let shift_world = root - truth.translation;
        let shift = truth.rotation().inverse() * shift_world / truth.scale;
        Correspondence {
            root: Some(root),
            shift: Vec3::new(shift.x, 0.0, shift.z),
            floor: shift.y,
            grips: grips.map(Some),
            wrists: grips.map(|g| Some(truth.apply(wrist_in_cockpit(&g, offset)))),
            scale: truth.scale,
            segment: 0,
        }
    }

    #[test]
    fn the_fit_recovers_the_heading_and_the_wrist_offset() {
        let truth = Placement {
            translation: Vec3::new(10.0, 2.0, -5.0),
            yaw: 2.3,
            scale: 1.0,
            anchor: Anchor::Root,
        };
        let offset = Vec3::new(0.0, 0.02, 0.06);
        let samples: Vec<Correspondence> = (0..60)
            .map(|i| {
                let t = i as f32 * 0.1;
                let grips = [
                    pose(
                        Vec3::new(-0.3 + 0.1 * t.sin(), -0.5, -0.4),
                        Quat::from_rotation_x(0.3 * t) * Quat::from_rotation_y(0.2 * t),
                    ),
                    pose(
                        Vec3::new(0.35, -0.3 + 0.1 * t.cos(), -0.2),
                        Quat::from_rotation_z(0.4 * t.sin()),
                    ),
                ];
                let root = truth.apply(Vec3::new(0.1, -1.6, 0.05));
                sample(&truth, root, grips, offset)
            })
            .collect();
        let fit = place(&samples, 3);
        // The offset and the heading trade off against each other, so the passes close in on both
        // rather than reaching them: within about half a degree and a few millimetres.
        for placement in fit.placements.iter().flatten() {
            assert!((placement.yaw - truth.yaw).abs() < 1e-2, "{placement:?}");
            assert!(placement.translation.distance(truth.translation) < 5e-3);
        }
        for estimated in fit.offsets {
            assert!(estimated.distance(offset) < 5e-3, "{estimated:?}");
        }
        assert!(fit.wrist_errors.iter().flatten().all(|e| *e < 5e-3));
    }

    #[test]
    fn without_a_root_the_wrists_place_the_cockpit_and_nothing_holds_the_last() {
        let truth = Placement {
            translation: Vec3::new(-3.0, 0.5, 7.0),
            yaw: -0.8,
            scale: 1.0,
            anchor: Anchor::Wrists,
        };
        let grips = [
            pose(Vec3::new(-0.4, -0.4, -0.3), Quat::IDENTITY),
            pose(Vec3::new(0.4, -0.4, -0.3), Quat::IDENTITY),
        ];
        let mut placed = sample(&truth, Vec3::ZERO, grips, Vec3::ZERO);
        placed.root = None;
        let mut empty = placed;
        empty.wrists = [None, None];
        let fit = place(&[placed, empty], 0);
        let first = fit.placements[0].unwrap();
        assert_eq!(first.anchor, Anchor::Wrists);
        assert!((first.yaw - truth.yaw).abs() < 1e-4);
        assert!(first.translation.distance(truth.translation) < 1e-4);
        let held = fit.placements[1].unwrap();
        assert_eq!(held.anchor, Anchor::Held);
        assert_eq!(held.translation, first.translation);
    }
}
