//! The physics bodies around Rico (from version 8): the near set after each physics step, what
//! each hand holds with its hold's report, and the forces the mod applied, in the world and, for
//! the forces, on the text log.
//!
//! A body sample leaves out a body at rest since an earlier step, so the near set is kept across
//! samples: a body absent from a sample stays while its last sample had it at rest, and has left
//! the set otherwise.

use std::collections::{BTreeMap, HashMap};

use glam::{Quat, Vec3};
use rerun::{
    Arrows3D, Boxes3D, Clear, Color, LineStrips3D, Points3D, RecordingStreamResult, Rgba32,
    Scalars, TextLog, components::TextLogLevel,
};
use tracking_format::event::{BodiesSample, BodyIntro, EventKind, Force, ForceKind, HeldSample};

use crate::rerun_export::{Context, events::body, scene::ARROW_S};

/// Log the bodies, holds, and forces within `(from, to)`; returns how many text log entries were
/// logged.
pub(crate) fn log(context: &Context<'_>, (from, to): (u64, u64)) -> RecordingStreamResult<usize> {
    let mut log = Log {
        context,
        near: NearSet::default(),
        intros: HashMap::new(),
        positions: HashMap::new(),
        forces: Vec::new(),
        forces_at: None,
        holding: [false; 2],
        entries: 0,
    };
    for event in &context.recording.events {
        if !(from..=to).contains(&event.time_ns) {
            continue;
        }
        if log.forces_at.is_some_and(|at| at != event.time_ns) && !log.forces.is_empty() {
            log.flush_forces()?;
        }
        match &event.kind {
            EventKind::BodyIntro(intro) => {
                log.intros.insert(intro.id, intro.as_ref().clone());
            }
            EventKind::Bodies(sample) => {
                context.set_time(event.time_ns);
                log.bodies(event.time_ns, sample)?;
            }
            EventKind::Held(held) => {
                context.set_time(event.time_ns);
                log.held(held)?;
            }
            EventKind::GripRelease(release) if log.holding[release.hand.index()] => {
                context.set_time(event.time_ns);
                log.holding[release.hand.index()] = false;
                context.rec.log(
                    format!("world/held/{}", release.hand.label()),
                    &Clear::recursive(),
                )?;
            }
            EventKind::Force(force) => {
                context.set_time(event.time_ns);
                log.forces_at = Some(event.time_ns);
                log.forces.push(force.as_ref());
                log.entries += 1;
                let level = match force.kind {
                    ForceKind::Impulse { .. } => TextLogLevel::INFO,
                    ForceKind::Velocity { .. } => TextLogLevel::DEBUG,
                };
                context.rec.log(
                    "events/forces",
                    &TextLog::new(body(&event.kind)).with_level(level),
                )?;
            }
            _ => {}
        }
    }
    if !log.forces.is_empty() {
        log.flush_forces()?;
    }
    Ok(log.entries)
}

/// The near set as the samples so far leave it: each body's latest state, by id.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct NearSet {
    pub bodies: BTreeMap<u32, NearBody>,
}

/// A body of the near set.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct NearBody {
    /// World position.
    pub position: Vec3,
    pub orientation: Quat,
    pub linear: Vec3,
    pub resting: bool,
    pub held: [bool; 2],
}

impl NearSet {
    /// Take a step's sample: its bodies replace their states, and a body it leaves out stays only
    /// while it was at rest.
    pub fn apply(&mut self, sample: &BodiesSample) {
        let present: HashMap<u32, NearBody> = sample
            .bodies
            .iter()
            .map(|body| {
                (
                    body.id,
                    NearBody {
                        position: sample.origin + body.offset,
                        orientation: body.orientation,
                        linear: body.linear,
                        resting: body.resting,
                        held: body.held,
                    },
                )
            })
            .collect();
        self.bodies
            .retain(|id, body| body.resting || present.contains_key(id));
        self.bodies.extend(present);
    }
}

const HELD: Color = Color(Rgba32::from_rgb(255, 255, 120));
const HELD_BODY: Color = Color(Rgba32::from_rgb(255, 230, 0));
const ANCHOR: Color = Color(Rgba32::from_rgb(255, 120, 60));
const TARGET: Color = Color(Rgba32::from_rgb(90, 200, 120));
const CENTRE: Color = Color(Rgba32::from_rgb(200, 200, 200));
const IMPULSE: Color = Color(Rgba32::from_rgb(255, 60, 200));
const VELOCITY_WRITE: Color = Color(Rgba32::from_rgb(60, 200, 255));
const MOTION: Color = Color(Rgba32::from_rgb(255, 200, 80));

/// The colours a body's kind picks from.
const KINDS: [Color; 6] = [
    Color(Rgba32::from_rgb(120, 170, 255)),
    Color(Rgba32::from_rgb(190, 130, 255)),
    Color(Rgba32::from_rgb(120, 220, 160)),
    Color(Rgba32::from_rgb(240, 150, 110)),
    Color(Rgba32::from_rgb(230, 120, 170)),
    Color(Rgba32::from_rgb(170, 200, 90)),
];

/// A body's marker radius when its shape is not known, metres.
const POINT_M: f32 = 0.05;

/// How long a force's arrow stays after it was applied, nanoseconds.
const FORCE_HOLD_NS: u64 = 250_000_000;

struct Log<'a, 'c> {
    context: &'c Context<'a>,
    near: NearSet,
    /// The latest introduction of each body.
    intros: HashMap<u32, BodyIntro>,
    /// Each body's latest known world position, from the near set or a hold.
    positions: HashMap<u32, Vec3>,
    /// The forces applied at one time, drawn together.
    forces: Vec<&'a Force>,
    forces_at: Option<u64>,
    holding: [bool; 2],
    entries: usize,
}

impl<'a> Log<'a, '_> {
    fn bodies(&mut self, time_ns: u64, sample: &BodiesSample) -> RecordingStreamResult<()> {
        let context = self.context;
        self.near.apply(sample);
        let mut boxes = Drawn::default();
        let mut points = Drawn::default();
        let mut arrows: (Vec<[f32; 3]>, Vec<[f32; 3]>) = (Vec::new(), Vec::new());
        for (id, near) in &self.near.bodies {
            self.positions.insert(*id, near.position);
            let intro = self.intros.get(id);
            let held = near.held.iter().any(|h| *h);
            let color = if held {
                HELD
            } else {
                intro.map_or(KINDS[0], |intro| kind_color(&intro.kind))
            };
            let label = match intro {
                Some(intro) => format!("#{id} {}", intro.kind),
                None => format!("#{id}"),
            };
            let drawn = match intro {
                Some(intro) if intro.half_extents.max_element() > 0.0 => {
                    boxes.half_sizes.push(intro.half_extents.to_array());
                    &mut boxes
                }
                _ => &mut points,
            };
            drawn.centres.push(context.world(near.position));
            drawn.colors.push(color);
            drawn.labels.push(label);
            if !near.resting {
                arrows.0.push(context.world(near.position));
                arrows.1.push((near.linear * ARROW_S).to_array());
            }
        }
        context.rec.log(
            "world/bodies/boxes",
            &Boxes3D::from_centers_and_half_sizes(boxes.centres, boxes.half_sizes)
                .with_colors(boxes.colors)
                .with_labels(boxes.labels)
                .with_show_labels(false),
        )?;
        context.rec.log(
            "world/bodies/points",
            &Points3D::new(points.centres)
                .with_radii([POINT_M])
                .with_colors(points.colors)
                .with_labels(points.labels)
                .with_show_labels(false),
        )?;
        context.rec.log(
            "world/bodies/velocity",
            &Arrows3D::from_vectors(arrows.1)
                .with_origins(arrows.0)
                .with_colors([MOTION]),
        )?;
        if self
            .forces_at
            .is_some_and(|at| time_ns.saturating_sub(at) > FORCE_HOLD_NS)
        {
            context.rec.log("world/forces", &Clear::flat())?;
            self.forces_at = None;
        }
        Ok(())
    }

    fn held(&mut self, held: &HeldSample) -> RecordingStreamResult<()> {
        let context = self.context;
        let rec = context.rec;
        let name = held.hand.label();
        let base = format!("world/held/{name}");
        self.holding[held.hand.index()] = true;
        if let Some(motion) = &held.motion {
            let position = motion.pose.position;
            self.positions.insert(held.body, position);
            rec.log(
                format!("{base}/body"),
                &Points3D::new([context.world(position)])
                    .with_radii([0.05])
                    .with_colors([HELD_BODY])
                    .with_labels([format!("{name} holds {} #{}", held.kind, held.body)]),
            )?;
            rec.log(
                format!("{base}/velocity"),
                &Arrows3D::from_vectors([(motion.linear * ARROW_S).to_array()])
                    .with_origins([context.world(position)])
                    .with_colors([MOTION]),
            )?;
        }
        if let Some(report) = &held.report {
            rec.log(
                format!("{base}/report"),
                &Points3D::new([
                    context.world(report.anchor),
                    context.world(report.target),
                    context.world(report.centre),
                ])
                .with_radii([0.025, 0.025, 0.02])
                .with_colors([ANCHOR, TARGET, CENTRE])
                .with_labels(["anchor", "target", "centre"])
                .with_show_labels(false),
            )?;
            rec.log(
                format!("{base}/lag"),
                &LineStrips3D::new([[context.world(report.anchor), context.world(report.target)]])
                    .with_colors([ANCHOR])
                    .with_radii([0.006]),
            )?;
            let scalars = [
                ("lag_m", report.lag_m),
                ("load", report.load),
                ("twist", report.twist),
            ];
            for (what, value) in scalars {
                if value.is_finite() {
                    rec.log(
                        format!("plots/held/{what}/{name}"),
                        &Scalars::single(f64::from(value)),
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Draw the forces applied at one time as arrows of the velocity change each asked for: an
    /// impulse at its point, and a velocity write at the body's latest known position.
    fn flush_forces(&mut self) -> RecordingStreamResult<()> {
        let context = self.context;
        if let Some(at) = self.forces_at {
            context.set_time(at);
        }
        let mut origins: Vec<[f32; 3]> = Vec::new();
        let mut vectors: Vec<[f32; 3]> = Vec::new();
        let mut colors: Vec<Color> = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        for force in self.forces.drain(..) {
            let (origin, change, color) = match force.kind {
                ForceKind::Impulse { impulse, at } => {
                    let change = match force.mass_kg {
                        Some(mass) if mass > 0.0 => impulse / mass,
                        _ => impulse,
                    };
                    (Some(at), change, IMPULSE)
                }
                ForceKind::Velocity { linear, .. } => {
                    let before = force.before.map_or(Vec3::ZERO, |before| before.linear);
                    (
                        self.positions.get(&force.body).copied(),
                        linear - before,
                        VELOCITY_WRITE,
                    )
                }
            };
            let Some(origin) = origin else { continue };
            if !change.is_finite() {
                continue;
            }
            origins.push(context.world(origin));
            vectors.push((change * ARROW_S).to_array());
            colors.push(color);
            labels.push(format!("{}:{}", force.source, force.line));
        }
        context.rec.log(
            "world/forces",
            &Arrows3D::from_vectors(vectors)
                .with_origins(origins)
                .with_colors(colors)
                .with_labels(labels)
                .with_show_labels(false),
        )
    }
}

/// The bodies drawn one way, with their colours and labels.
#[derive(Default)]
struct Drawn {
    centres: Vec<[f32; 3]>,
    half_sizes: Vec<[f32; 3]>,
    colors: Vec<Color>,
    labels: Vec<String>,
}

/// A kind's colour: the same kind always picks the same one.
fn kind_color(kind: &str) -> Color {
    // FNV-1a, for a hash that does not change between runs.
    let hash = kind.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    KINDS[hash as usize % KINDS.len()]
}

#[cfg(test)]
mod tests {
    use tracking_format::event::BodySample;

    use super::*;

    fn body(id: u32, offset: Vec3, resting: bool) -> BodySample {
        BodySample {
            id,
            offset,
            orientation: Quat::IDENTITY,
            linear: if resting { Vec3::ZERO } else { Vec3::X },
            angular: Vec3::ZERO,
            resting,
            held: [false; 2],
        }
    }

    fn sample(origin: Vec3, bodies: Vec<BodySample>) -> BodiesSample {
        BodiesSample {
            step: 0,
            origin,
            walked: 0,
            peak: 0,
            cost_us: 0,
            bodies,
        }
    }

    #[test]
    fn a_resting_body_stays_when_left_out_and_a_moving_one_leaves() {
        let mut near = NearSet::default();
        near.apply(&sample(
            Vec3::new(10.0, 0.0, 0.0),
            vec![body(1, Vec3::Y, true), body(2, Vec3::Z, false)],
        ));
        assert_eq!(near.bodies[&1].position, Vec3::new(10.0, 1.0, 0.0));
        near.apply(&sample(Vec3::ZERO, vec![]));
        assert!(near.bodies.contains_key(&1));
        assert!(!near.bodies.contains_key(&2));
        // A resting body that moves again is written again, and its new state replaces the old.
        near.apply(&sample(Vec3::ZERO, vec![body(1, Vec3::X, false)]));
        assert_eq!(near.bodies[&1].position, Vec3::X);
        assert!(!near.bodies[&1].resting);
        near.apply(&sample(Vec3::ZERO, vec![]));
        assert!(near.bodies.is_empty());
    }

    #[test]
    fn a_kind_always_picks_the_same_colour() {
        assert_eq!(kind_color("prop"), kind_color("prop"));
    }
}
