//! The grab view's primitives for one input tick, within a budget: the refused grabs, the reach
//! rays, and the door handles near the palms, which are always drawn, then a line from each hand to
//! the grip targets near it, a held gun's grab and knock zones, and the candidates' boxes, in that
//! order, so that the budget leaves out the candidates first.
//!
//! Everything is anchored at render time (`docs/mod/rendering/render-time-anchoring.md`): the hands'
//! ends of the lines to the hands' render-time poses, a target's end where its grip marker is, the
//! reach rays (placed by the palms' tick, which rides with the player) to the render-time head at
//! their offset from the tick's head, and only what lies in the world (the door handles, the
//! candidates, a refusal's point) to the world.

use glam::Vec3;
use jc3vr_body::controllers::{
    grab::{
        Category, GripTarget, MarkAt, Shown, markers, markers::MarkerFrame, target::TargetReach,
        tick::Tick,
    },
    held::zones::{GunZones, KnockZone},
};
use jc3vr_input::frame::Hand;
use jc3vr_view::controllers::overlay::primitive::{
    Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, SpriteSize,
};

use crate::controllers::physical_hands::{
    grab_view::{
        Recorded,
        model::{
            Candidate, REFUSAL_COLOUR, TargetState, approach, refusal_flash, target_colour,
            target_mark_size,
        },
    },
    views::{GrabDrawTuning, GrabViewTuning},
};

/// What one tick drew.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Drawn {
    pub target_lines: usize,
    pub rays: usize,
    pub handles: usize,
    pub candidates: usize,
    pub flashes: usize,
    /// The held guns whose grab zones were drawn.
    pub zones: usize,
    /// The target lines, gun zones, and candidates that the budget or the candidate cap left out.
    pub dropped: usize,
}

/// What the view is drawn from.
pub(crate) struct DrawInputs<'a> {
    pub frame: &'a MarkerFrame,
    /// The grip model's targets and resolution this tick.
    pub tick: &'a Tick,
    /// The palms' last tick, while it is fresh.
    pub recorded: Option<&'a Recorded>,
    pub candidates: &'a [Candidate],
    /// The refused grabs still flashing: where, and how many seconds ago.
    pub flashes: &'a [(Anchor, f32)],
    /// The grab zones of each gun held in one hand for the other, by the holding hand.
    pub zones: [Option<GunZones>; 2],
    /// The knock zone of the gun whose magazine a held pouch magazine can knock out.
    pub knock: Option<KnockZone>,
    pub tuning: &'a GrabViewTuning,
}

/// Build the tick's primitives into `out` (cleared first).
pub(crate) fn build(inputs: &DrawInputs, out: &mut Vec<Primitive>) -> Drawn {
    out.clear();
    let tuning = &inputs.tuning.draw;
    let budget = tuning.primitive_budget;
    let mut drawn = Drawn::default();
    for &(at, age_s) in inputs.flashes {
        let flash = refusal_flash(
            age_s,
            inputs.tuning.refusal_flash_s,
            tuning.refusal_start_share,
        );
        if let Some((alpha, share)) = flash {
            let [r, g, b] = REFUSAL_COLOUR;
            out.push(sprite(
                at,
                tuning.refusal_mark_m * share * inputs.frame.xf.world_scale,
                Shape::Ring {
                    thickness: tuning.refusal_ring_thickness,
                },
                [r, g, b, alpha],
            ));
            drawn.flashes += 1;
        }
    }
    if let Some(recorded) = inputs.recorded {
        let rides = |p: Vec3| Anchor::Head(p - recorded.head);
        for ray in recorded.rays.iter().flatten() {
            let colour = ray.outcome.colour();
            let end = ray.hit.unwrap_or(ray.to);
            out.push(line(
                rides(ray.from),
                rides(end),
                tuning.ray_width_m,
                colour,
            ));
            if let Some(hit) = ray.hit {
                out.push(sprite(rides(hit), tuning.hit_mark_m, Shape::Disc, colour));
            }
            drawn.rays += 1;
        }
        for handle in recorded.handles.iter().flatten() {
            out.push(sprite(
                Anchor::World(handle.at),
                tuning.handle_mark_m,
                Shape::Ring {
                    thickness: tuning.handle_ring_thickness,
                },
                handle.outcome.colour(),
            ));
            drawn.handles += 1;
        }
    }
    let lines = target_lines(inputs);
    for line_spec in lines {
        if out.len() + 2 > budget {
            drawn.dropped += 1;
            continue;
        }
        let colour = target_colour(line_spec.state, line_spec.approach);
        out.push(line(
            Anchor::hand(line_spec.hand, HandPose::Grip),
            line_spec.at,
            tuning.target_line_width_m,
            colour,
        ));
        let shape = match line_spec.state {
            TargetState::OutOfReach | TargetState::InReach => Shape::Ring {
                thickness: tuning.target_ring_thickness,
            },
            TargetState::Chosen | TargetState::Held => Shape::Disc,
        };
        out.push(sprite(
            line_spec.at,
            target_mark_size(line_spec.approach, tuning) * inputs.frame.xf.world_scale,
            shape,
            colour,
        ));
        drawn.target_lines += 1;
    }
    for view in inputs.zones.iter().flatten() {
        if out.len() + ZONE_PRIMITIVES > budget {
            drawn.dropped += 1;
            continue;
        }
        zone_primitives(view, inputs.frame, tuning, out);
        drawn.zones += 1;
    }
    if let Some(view) = inputs.knock {
        if out.len() + KNOCK_PRIMITIVES > budget {
            drawn.dropped += 1;
        } else {
            knock_primitives(&view, inputs.frame, tuning, out);
            drawn.zones += 1;
        }
    }
    for (k, candidate) in inputs.candidates.iter().enumerate() {
        if k >= inputs.tuning.max_candidates || out.len() + CANDIDATE_PRIMITIVES > budget {
            drawn.dropped += 1;
            continue;
        }
        let colour = candidate.kind.colour();
        for (a, b) in candidate.aabb.edges() {
            out.push(line(
                Anchor::World(a),
                Anchor::World(b),
                tuning.box_width_m,
                colour,
            ));
        }
        let [r, g, b, a] = colour;
        out.push(line(
            Anchor::World(candidate.aabb.centre()),
            Anchor::hand(candidate.hand, HandPose::Grip),
            tuning.box_width_m,
            [r, g, b, a * 0.5],
        ));
        drawn.candidates += 1;
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

/// Each hand's lines to the targets near it, nearest first per hand, at most the tuning's
/// `max_target_lines` per hand. A target out of reach farther than the tuning's `target_radius_m`
/// gets none, nor does a socket or pouch on the body out of reach, whose own marker already shows
/// it.
fn target_lines(inputs: &DrawInputs) -> Vec<TargetLine> {
    let tick = inputs.tick;
    let tuning = &inputs.tuning.draw;
    let radius = tuning.target_radius_m;
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
        lines.truncate(tuning.max_target_lines);
        out.extend(lines);
    }
    out
}

/// How far a hand with its drawn grip at `grip` is from `target`'s grab point, and how near it
/// must come, metres at the player's scale; `in_reach` is the resolver's distance when the hand is
/// within reach. A target the provider measured is only offered within reach.
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

/// A held gun's grab zones for the free hand, on the gun as it rides its hand: the magazine's
/// zone under the well as its axis and a ring at each end, and the handover's reach around the
/// pistol grip as a ring of its radius. The zone the free hand is in is drawn brighter, and the
/// handover dimmed while the magazine's zone holds the hand.
fn zone_primitives(
    view: &GunZones,
    frame: &MarkerFrame,
    tuning: &GrabDrawTuning,
    out: &mut Vec<Primitive>,
) {
    let on_gun = |point: Vec3| {
        markers::anchor_at(
            MarkAt::OnHand {
                hand: view.holder,
                point,
            },
            frame,
        )
        .or_else(|| markers::anchor_at(MarkAt::Cockpit(point), frame))
    };
    let scale = frame.xf.world_scale;
    if let Some(zone) = view.magazine {
        let colour = shade(MAGAZINE_ZONE_COLOUR, view.at_magazine);
        if let (Some(top), Some(bottom)) = (on_gun(zone.top), on_gun(zone.bottom)) {
            out.push(line(top, bottom, tuning.zone_line_width_m, colour));
            for end in [top, bottom] {
                out.push(sprite(
                    end,
                    2.0 * zone.radius * scale,
                    Shape::Ring {
                        thickness: tuning.zone_ring_thickness,
                    },
                    colour,
                ));
            }
        }
    }
    if let Some((centre, radius)) = view.handover
        && let Some(at) = on_gun(centre)
    {
        let colour = if view.at_magazine {
            [
                HANDOVER_ZONE_COLOUR[0],
                HANDOVER_ZONE_COLOUR[1],
                HANDOVER_ZONE_COLOUR[2],
                SUPPRESSED_ALPHA,
            ]
        } else {
            HANDOVER_ZONE_COLOUR
        };
        out.push(sprite(
            at,
            2.0 * radius * scale,
            Shape::Ring {
                thickness: tuning.handover_ring_thickness,
            },
            colour,
        ));
    }
}

/// The knock zone on the held gun: the inserted magazine as its axis with a ring at the base and
/// one at the top, and the held magazine's tip, green while it moves fast enough to knock. The
/// zone flashes white when the magazine is knocked out.
fn knock_primitives(
    view: &KnockZone,
    frame: &MarkerFrame,
    tuning: &GrabDrawTuning,
    out: &mut Vec<Primitive>,
) {
    let on_gun = |point: Vec3| {
        markers::anchor_at(
            MarkAt::OnHand {
                hand: view.holder,
                point,
            },
            frame,
        )
        .or_else(|| markers::anchor_at(MarkAt::Cockpit(point), frame))
    };
    let scale = frame.xf.world_scale;
    let colour = if view.flashing {
        KNOCK_FLASH_COLOUR
    } else {
        KNOCK_ZONE_COLOUR
    };
    if let (Some(top), Some(base)) = (on_gun(view.zone.top), on_gun(view.zone.bottom)) {
        out.push(line(top, base, tuning.zone_line_width_m, colour));
        let rings = [
            (top, tuning.knock_top_ring_thickness),
            (base, tuning.knock_base_ring_thickness),
        ];
        for (end, thickness) in rings {
            out.push(sprite(
                end,
                2.0 * view.zone.radius * scale,
                Shape::Ring { thickness },
                colour,
            ));
        }
    }
    if let Some(tip) = view
        .tip
        .and_then(|tip| markers::anchor_at(MarkAt::Cockpit(tip), frame))
    {
        let colour = if view.fast {
            KNOCK_FAST_COLOUR
        } else {
            KNOCK_ZONE_COLOUR
        };
        out.push(sprite(tip, tuning.knock_tip_m * scale, Shape::Disc, colour));
    }
}

/// `colour` brightened when the free hand is in its zone.
fn shade(colour: Rgba, inside: bool) -> Rgba {
    let [r, g, b, a] = colour;
    if inside { [r, g, b, 1.0] } else { [r, g, b, a] }
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

/// The most primitives one held gun's zones take.
const ZONE_PRIMITIVES: usize = 4;
/// The magazine's zone: amber, as the well's gold ring.
const MAGAZINE_ZONE_COLOUR: Rgba = [1.0, 0.7, 0.15, 0.55];
/// The handover's reach: cyan.
const HANDOVER_ZONE_COLOUR: Rgba = [0.3, 0.85, 1.0, 0.55];
/// The handover's alpha while the magazine's zone holds the free hand.
const SUPPRESSED_ALPHA: f32 = 0.15;

/// The most primitives the knock zone takes.
const KNOCK_PRIMITIVES: usize = 4;
/// The knock zone: red, apart from the pull's amber zone it lies inside.
const KNOCK_ZONE_COLOUR: Rgba = [1.0, 0.3, 0.25, 0.6];
/// The knock zone as the magazine is knocked out.
const KNOCK_FLASH_COLOUR: Rgba = [1.0, 1.0, 1.0, 1.0];
/// The held tip while it moves fast enough to knock.
const KNOCK_FAST_COLOUR: Rgba = [0.3, 1.0, 0.35, 0.9];

/// A candidate's primitives: its box's twelve edges and its line to the hand.
const CANDIDATE_PRIMITIVES: usize = 13;

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use jc3vr_body::controllers::{
        grab::{
            GrabVolume, Mark, MarkAt, Reach, Resolved, Tier, markers::test_frame, socket::Socket,
        },
        physics::scan::Aabb,
    };

    use super::*;
    use crate::controllers::physical_hands::{
        grab_view::model::{CandidateKind, RayOutcome, ReachRay},
        views::VIEWS,
    };

    fn config() -> &'static GrabViewTuning {
        &VIEWS.get().grab_view
    }

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
        frame: &'a MarkerFrame,
        tick: &'a Tick,
        candidates: &'a [Candidate],
    ) -> DrawInputs<'a> {
        DrawInputs {
            frame,
            tick,
            recorded: None,
            candidates,
            flashes: &[],
            zones: [None; 2],
            knock: None,
            tuning: config(),
        }
    }

    #[test]
    fn targets_get_lines_by_state_and_far_or_body_ones_get_none() {
        let near = sphere(GrabVolume::WorldWeapon, Vec3::new(0.0, 1.0, -0.5), 0.1);
        let chosen = sphere(GrabVolume::Foregrip, Vec3::new(0.0, 1.0, -0.05), 0.1);
        let far = sphere(GrabVolume::WorldMagazine, Vec3::new(0.0, 1.0, -5.0), 0.1);
        let pouch = sphere(
            GrabVolume::Socket(Socket::Hip(Hand::Left)),
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
        let frame = test_frame();
        let lines = target_lines(&inputs(&frame, &tick, &[]));
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
        let frame = test_frame();
        let lines = target_lines(&inputs(&frame, &tick, &[]));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].hand, Hand::Right);
        assert_eq!(lines[0].state, TargetState::Held);
    }

    #[test]
    fn the_rays_follow_the_head_and_the_candidates_are_capped() {
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
        let max_candidates = config().max_candidates;
        let candidates = vec![candidate; max_candidates + 1];
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
        let mut drawn_inputs = inputs(&frame, &tick, &candidates);
        drawn_inputs.recorded = Some(&recorded);
        let mut out = Vec::new();
        let drawn = build(&drawn_inputs, &mut out);
        assert_eq!(drawn.rays, 1);
        assert_eq!(drawn.candidates, max_candidates);
        assert_eq!(drawn.dropped, 1);
        assert!(out.contains(&line(
            Anchor::Head(Vec3::X),
            Anchor::Head(Vec3::new(1.0, 0.0, -0.5)),
            config().draw.ray_width_m,
            RayOutcome::InReach.colour(),
        )));
        // The ray and its hit, and the candidates.
        assert_eq!(out.len(), 2 + max_candidates * CANDIDATE_PRIMITIVES);
    }
}
