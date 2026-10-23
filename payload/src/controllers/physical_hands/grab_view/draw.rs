//! The grab view's primitives for one input tick, within a budget: each hand's palm marker, the
//! refused grabs, the reach rays and the door handles near the palms, a line from each hand to the
//! grip targets near it, and the candidates' boxes, in that order, so that the budget leaves out
//! the candidates first.
//!
//! Everything is anchored at render time (`docs/mod/rendering/render-time-anchoring.md`): the palm
//! markers and the hands' ends of the lines to the hands' render-time poses, a target's end where
//! its grip marker is, the reach rays (placed by the palms' tick, which rides with the player) to
//! the render-time head at their offset from the tick's head, and only what lies in the world (the
//! door handles, the candidates, a refusal's point) to the world.

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    grab::{
        Category, GripTarget, Shown,
        markers::{self, MarkerFrame},
        target::TargetReach,
        tick::Tick,
    },
    overlay::primitive::{Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, SpriteSize},
    physical_hands::grab_view::{
        Recorded,
        config::GrabViewConfig,
        model::{
            Candidate, PalmState, REFUSAL_COLOUR, TargetState, approach, refusal_flash,
            target_colour, target_mark_size,
        },
    },
};

/// What one tick drew.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Drawn {
    pub target_lines: usize,
    pub rays: usize,
    pub handles: usize,
    pub candidates: usize,
    pub flashes: usize,
    /// The candidates and lines the budget left out.
    pub dropped: usize,
}

/// What the view is drawn from.
pub(crate) struct DrawInputs<'a> {
    pub config: &'a GrabViewConfig,
    pub frame: &'a MarkerFrame,
    /// The grip model's targets and resolution this tick.
    pub tick: &'a Tick,
    /// The palms' last tick, while it is fresh.
    pub recorded: Option<&'a Recorded>,
    /// Each hand's palm state, by [`Hand::index`].
    pub palms: [PalmState; 2],
    pub candidates: &'a [Candidate],
    /// The refused grabs still flashing: where, and how many seconds ago.
    pub flashes: &'a [(Anchor, f32)],
}

/// The most primitives the view submits in a tick.
pub const PRIMITIVE_BUDGET: usize = 1024;

/// Build the tick's primitives into `out` (cleared first).
pub(crate) fn build(inputs: &DrawInputs, out: &mut Vec<Primitive>) -> Drawn {
    out.clear();
    let mut drawn = Drawn::default();
    let config = inputs.config;
    if config.wrist {
        for hand in Hand::BOTH {
            palm_marker(hand, inputs.palms[hand.index()], out);
        }
    }
    if config.refusals {
        for &(at, age_s) in inputs.flashes {
            if let Some((alpha, share)) = refusal_flash(age_s) {
                let [r, g, b] = REFUSAL_COLOUR;
                out.push(sprite(
                    at,
                    REFUSAL_MARK_M * share * inputs.frame.xf.world_scale,
                    Shape::Ring { thickness: 0.35 },
                    [r, g, b, alpha],
                ));
                drawn.flashes += 1;
            }
        }
    }
    if let Some(recorded) = inputs.recorded {
        let rides = |p: Vec3| Anchor::Head(p - recorded.head);
        if config.rays {
            for ray in recorded.rays.iter().flatten() {
                let colour = ray.outcome.colour();
                let end = ray.hit.unwrap_or(ray.to);
                out.push(line(rides(ray.from), rides(end), RAY_WIDTH_M, colour));
                if let Some(hit) = ray.hit {
                    out.push(sprite(rides(hit), HIT_MARK_M, Shape::Disc, colour));
                }
                drawn.rays += 1;
            }
        }
        for handle in recorded.handles.iter().flatten() {
            out.push(sprite(
                Anchor::World(handle.at),
                HANDLE_MARK_M,
                Shape::Ring { thickness: 0.3 },
                handle.outcome.colour(),
            ));
            drawn.handles += 1;
        }
    }
    if config.targets {
        let lines = target_lines(inputs);
        for line_spec in lines {
            if out.len() + 2 > PRIMITIVE_BUDGET {
                drawn.dropped += 1;
                continue;
            }
            let colour = target_colour(line_spec.state, line_spec.approach);
            out.push(line(
                Anchor::hand(line_spec.hand, HandPose::Grip),
                line_spec.at,
                TARGET_LINE_WIDTH_M,
                colour,
            ));
            let shape = match line_spec.state {
                TargetState::OutOfReach | TargetState::InReach => Shape::Ring { thickness: 0.3 },
                TargetState::Chosen | TargetState::Held => Shape::Disc,
            };
            out.push(sprite(
                line_spec.at,
                target_mark_size(line_spec.approach) * inputs.frame.xf.world_scale,
                shape,
                colour,
            ));
            drawn.target_lines += 1;
        }
    }
    if config.candidates {
        let max = config.max_candidates as usize;
        for (k, candidate) in inputs.candidates.iter().enumerate() {
            if k >= max || out.len() + CANDIDATE_PRIMITIVES > PRIMITIVE_BUDGET {
                drawn.dropped += 1;
                continue;
            }
            let colour = candidate.kind.colour();
            for (a, b) in candidate.aabb.edges() {
                out.push(line(
                    Anchor::World(a),
                    Anchor::World(b),
                    BOX_WIDTH_M,
                    colour,
                ));
            }
            let [r, g, b, a] = colour;
            out.push(line(
                Anchor::World(candidate.aabb.centre()),
                Anchor::hand(candidate.hand, HandPose::Grip),
                BOX_WIDTH_M,
                [r, g, b, a * 0.5],
            ));
            drawn.candidates += 1;
        }
    }
    drawn
}

/// A line from a hand to a grip target.
#[derive(Debug, Copy, Clone, PartialEq)]
struct TargetLine {
    hand: Hand,
    at: Anchor,
    state: TargetState,
    approach: f32,
    distance: f32,
}

/// Each hand's lines to the targets near it, nearest first per hand, at most
/// [`MAX_TARGET_LINES`] per hand. A target out of reach farther than the view's radius gets none,
/// nor does a socket or pouch on the body out of reach, whose own marker already shows it.
fn target_lines(inputs: &DrawInputs) -> Vec<TargetLine> {
    let tick = inputs.tick;
    let radius = inputs.config.radius_m.max(0.0);
    let mut out = Vec::new();
    for hand in Hand::BOTH {
        let i = hand.index();
        let resolved = &tick.resolved[i];
        let grip = tick.grips[i];
        let mut lines: Vec<TargetLine> = tick
            .targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.for_hand(hand) && t.mark.shown != Shown::Never)
            .filter(|(_, t)| t.held_by.is_none_or(|h| h == hand))
            .filter_map(|(index, target)| {
                let reach = resolved.reaches.iter().find(|r| r.index == index);
                let chosen = resolved.chosen.is_some_and(|c| c.index == index);
                let state = TargetState::of(target.held_by == Some(hand), chosen, reach.is_some());
                let (distance, reach_m) = distance_to(target, grip, reach.map(|r| r.distance))?;
                if state == TargetState::OutOfReach
                    && (distance > radius || target.category() == Category::Body)
                {
                    return None;
                }
                let at = markers::anchor(target, inputs.frame)?;
                Some(TargetLine {
                    hand,
                    at,
                    state,
                    approach: approach(distance, reach_m, radius),
                    distance,
                })
            })
            .collect();
        lines.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        lines.truncate(MAX_TARGET_LINES);
        out.extend(lines);
    }
    out
}

/// How far a hand with its drawn grip at `grip` is from `target`'s grab point, and how near it must come, metres at
/// the player's scale; `in_reach` is the resolver's distance when the hand is within reach. A
/// target the provider measured is only offered within reach.
fn distance_to(
    target: &GripTarget,
    grip: Option<Vec3>,
    in_reach: Option<f32>,
) -> Option<(f32, f32)> {
    match target.reach {
        TargetReach::Sphere { centre, radius } => {
            let distance =
                in_reach.unwrap_or_else(|| grip.map_or(f32::INFINITY, |p| p.distance(centre)));
            distance.is_finite().then_some((distance, radius))
        }
        TargetReach::Measured { distance } => {
            let distance = in_reach.unwrap_or(distance);
            distance.is_finite().then_some((distance, distance))
        }
    }
}

/// The palm marker on `hand`'s wrist: a disc in the state's colour, ringed while the palm acts.
fn palm_marker(hand: Hand, state: PalmState, out: &mut Vec<Primitive>) {
    let at = Anchor::Hand {
        hand,
        pose: HandPose::Wrist,
        offset: WRIST_OFFSET,
    };
    let colour = state.colour();
    out.push(Primitive::Sprite {
        at,
        size: SpriteSize::World(WRIST_MARK_M),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color: colour,
        depth: DepthMode::Always,
    });
    if matches!(
        state,
        PalmState::Acting | PalmState::Driven | PalmState::Holding
    ) {
        out.push(Primitive::Sprite {
            at,
            size: SpriteSize::World(WRIST_MARK_M * 1.6),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.2 },
            color: colour,
            depth: DepthMode::Always,
        });
    }
}

fn line(from: Anchor, to: Anchor, width: f32, color: Rgba) -> Primitive {
    Primitive::Line {
        from,
        to,
        width,
        color,
        depth: XRAY,
    }
}

fn sprite(at: Anchor, size: f32, shape: Shape, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at,
        size: SpriteSize::World(size),
        facing: Facing::Billboard,
        shape,
        color,
        depth: XRAY,
    }
}

const XRAY: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.4,
};

/// The palm marker's place in the wrist frame: over the back of the hand (+Y), toward the forearm.
const WRIST_OFFSET: Vec3 = Vec3::new(0.0, 0.04, 0.03);
/// The palm marker's diameter, metres.
const WRIST_MARK_M: f32 = 0.022;
/// A refusal's flash at its largest, metres.
const REFUSAL_MARK_M: f32 = 0.16;
const RAY_WIDTH_M: f32 = 0.002;
const HIT_MARK_M: f32 = 0.012;
const HANDLE_MARK_M: f32 = 0.04;
const TARGET_LINE_WIDTH_M: f32 = 0.0025;
const BOX_WIDTH_M: f32 = 0.002;
/// The most target lines per hand.
const MAX_TARGET_LINES: usize = 24;
/// A candidate's primitives: its box's twelve edges and its line to the hand.
const CANDIDATE_PRIMITIVES: usize = 13;

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::controllers::{
        grab::{GrabVolume, Mark, MarkAt, Reach, Resolved, Tier, markers::test_frame},
        loadout::Socket,
        physical_hands::{
            collider_view::model::Aabb,
            grab_view::model::{CandidateKind, RayOutcome, ReachRay},
        },
    };

    fn sphere(volume: GrabVolume, centre: Vec3, radius: f32) -> GripTarget {
        GripTarget::sphere(
            volume,
            None,
            centre,
            radius,
            Mark::ring(MarkAt::Cockpit(centre), 0.05),
        )
    }

    fn inputs<'a>(
        config: &'a GrabViewConfig,
        frame: &'a MarkerFrame,
        tick: &'a Tick,
        candidates: &'a [Candidate],
    ) -> DrawInputs<'a> {
        DrawInputs {
            config,
            frame,
            tick,
            recorded: None,
            palms: [PalmState::Acting; 2],
            candidates,
            flashes: &[],
        }
    }

    #[test]
    fn targets_get_lines_by_state_and_far_or_body_ones_get_none() {
        let near = sphere(GrabVolume::WorldWeapon, Vec3::new(0.0, 1.0, -0.5), 0.1);
        let chosen = sphere(GrabVolume::Foregrip, Vec3::new(0.0, 1.0, -0.05), 0.1);
        let far = sphere(GrabVolume::WorldMagazine, Vec3::new(0.0, 1.0, -5.0), 0.1);
        let pouch = sphere(
            GrabVolume::Holster(Socket::Chest),
            Vec3::new(0.0, 1.0, 0.5),
            0.1,
        );
        let reach = Reach {
            index: 1,
            volume: GrabVolume::Foregrip,
            distance: 0.05,
            tier: Tier::Target,
        };
        let mut tick = Tick {
            targets: vec![near, chosen, far, pouch],
            ..Tick::default()
        };
        tick.grips = [Some(Vec3::new(0.0, 1.0, 0.0)), None];
        tick.resolved[0] = Resolved {
            reaches: vec![reach],
            chosen: Some(reach),
        };
        let config = GrabViewConfig::new();
        let frame = test_frame();
        let lines = target_lines(&inputs(&config, &frame, &tick, &[]));
        let states: Vec<TargetState> = lines.iter().map(|l| l.state).collect();
        assert_eq!(states, vec![TargetState::Chosen, TargetState::OutOfReach]);
        assert!(lines.iter().all(|l| l.hand == Hand::Left));
        assert!(lines[0].approach > lines[1].approach);
    }

    #[test]
    fn a_held_target_is_drawn_only_for_its_holder() {
        let held = sphere(GrabVolume::Handle, Vec3::new(0.0, 1.0, 0.0), 0.1).held(Hand::Right);
        let mut tick = Tick {
            targets: vec![held],
            ..Tick::default()
        };
        tick.grips = [Some(Vec3::new(0.0, 1.0, 0.0)); 2];
        let config = GrabViewConfig::new();
        let frame = test_frame();
        let lines = target_lines(&inputs(&config, &frame, &tick, &[]));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].hand, Hand::Right);
        assert_eq!(lines[0].state, TargetState::Held);
    }

    #[test]
    fn the_rays_follow_the_head_and_the_candidates_are_capped() {
        let config = GrabViewConfig {
            max_candidates: 2,
            ..GrabViewConfig::new()
        };
        let frame = test_frame();
        let tick = Tick::default();
        let candidate = Candidate {
            aabb: Aabb {
                min: Vec3::ZERO,
                max: Vec3::ONE,
            },
            kind: CandidateKind::Weapon,
            hand: Hand::Left,
            distance_m: 1.0,
        };
        let candidates = [candidate; 3];
        let head = Vec3::new(10.0, 2.0, -5.0);
        let recorded = Recorded {
            head,
            rays: [
                vec![ReachRay {
                    from: head + Vec3::X,
                    to: head + Vec3::new(1.0, 0.0, -1.0),
                    hit: Some(head + Vec3::new(1.0, 0.0, -0.5)),
                    outcome: RayOutcome::InReach,
                }],
                Vec::new(),
            ],
            handles: Default::default(),
        };
        let mut drawn_inputs = inputs(&config, &frame, &tick, &candidates);
        drawn_inputs.recorded = Some(&recorded);
        let mut out = Vec::new();
        let drawn = build(&drawn_inputs, &mut out);
        assert_eq!(drawn.rays, 1);
        assert_eq!(drawn.candidates, 2);
        assert_eq!(drawn.dropped, 1);
        assert!(out.contains(&line(
            Anchor::Head(Vec3::X),
            Anchor::Head(Vec3::new(1.0, 0.0, -0.5)),
            RAY_WIDTH_M,
            RayOutcome::InReach.colour(),
        )));
        // Two palm markers with their rings, the ray and its hit, and the candidates.
        assert_eq!(out.len(), 4 + 2 + 2 * CANDIDATE_PRIMITIVES);
    }
}
