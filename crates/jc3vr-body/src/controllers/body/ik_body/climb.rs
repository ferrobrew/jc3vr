//! The feet on the face while the hands climb: each foot's casts asked for, its brace held on the
//! face ([`body_rig::brace`]), and the targets the effectors place the ankle, the toe, and the knee
//! on, in Rico's model space. A foot with nothing in reach hangs in the air pose the gate gives it.

use std::time::Instant;

use body_rig::{
    angle::direction,
    brace::{BraceEvent, BraceFoot, BraceLeg, BraceTarget, BraceTuning},
    contact::Side,
};
use glam::{Mat4, Vec3};

use crate::controllers::{
    body::ik_body::{
        brace::{self, BraceCast},
        support,
        tuning::IK_BODY,
    },
    telemetry::report,
};

/// One tick's legs, in Rico's model space, with the character's world transform.
pub(crate) struct ClimbLegs<'a> {
    pub world: &'a Mat4,
    /// The character's heading in the world.
    pub world_yaw: f32,
    /// The hip joints, left then right.
    pub hips: [Vec3; 2],
    /// The clip's ankles, left then right.
    pub ankles: [Vec3; 2],
    pub leg_m: f32,
    pub thigh_m: f32,
    pub foot_m: f32,
    pub ankle_height_m: f32,
    pub toe_height_m: f32,
}

/// A braced foot's targets in model space, and the brace's share against the hanging leg.
pub(crate) type ModelBrace = (BraceTarget, f32);

/// The two feet's braces and the spell's counts.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ClimbFeet {
    feet: [BraceFoot; 2],
    spell: Option<Spell>,
    /// How many times a foot has braced or moved to a new brace.
    pub moves: u64,
}

/// One climb, for its summary in the log.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Spell {
    began: Instant,
    moves: u64,
    /// Seconds each foot held a brace.
    braced_s: [f32; 2],
}

impl ClimbFeet {
    pub(crate) const fn new() -> Self {
        Self {
            feet: [BraceFoot::new(); 2],
            spell: None,
            moves: 0,
        }
    }

    /// Whether a climb's braces are under way.
    pub(crate) fn active(&self) -> bool {
        self.spell.is_some()
    }

    /// Whether each foot holds a brace.
    pub(crate) fn braced(&self) -> [bool; 2] {
        self.feet.each_ref().map(BraceFoot::braced)
    }

    /// Advance the braces one tick for `legs`, asking for the casts the next tick answers.
    pub(crate) fn step(
        &mut self,
        legs: &ClimbLegs<'_>,
        tuning: &BraceTuning,
        dt: f32,
        now: Instant,
    ) -> [Option<ModelBrace>; 2] {
        let spell = self.spell.get_or_insert(Spell {
            began: now,
            moves: 0,
            braced_s: [0.0; 2],
        });
        let to_world = |p: Vec3| legs.world.transform_point3(p);
        let to_model = legs.world.inverse();
        let ahead = direction(legs.world_yaw);
        let forward_down = (ahead + Vec3::NEG_Y).normalize();
        let casts = &IK_BODY.get().climb;
        Side::BOTH.map(|side| {
            let i = side.index();
            let hip = to_world(legs.hips[i]);
            let ankle = to_world(legs.ankles[i]);
            let candidate = brace::ask(
                side,
                [
                    BraceCast {
                        from: hip,
                        direction: ahead,
                        length: legs.leg_m * casts.hip_cast_share,
                    },
                    BraceCast {
                        from: ankle,
                        direction: forward_down,
                        length: legs.leg_m * casts.ankle_cast_share,
                    },
                ],
                now,
            );
            let leg = BraceLeg {
                hip,
                facing: legs.world_yaw,
                leg_m: legs.leg_m,
                thigh_m: legs.thigh_m,
                foot_m: legs.foot_m,
                ankle_height_m: legs.ankle_height_m,
                toe_height_m: legs.toe_height_m,
            };
            let (out, event) =
                self.feet[i].update(&leg, candidate, |s| support::frame(s, now), dt, tuning);
            if self.feet[i].braced() {
                spell.braced_s[i] += dt;
            }
            if let Some(event) = event {
                if matches!(event, BraceEvent::Braced | BraceEvent::Moved) {
                    self.moves += 1;
                    spell.moves += 1;
                }
                let what = match event {
                    BraceEvent::Braced => "a foot braces on the face",
                    BraceEvent::Moved => "a foot moves to a new brace",
                    BraceEvent::LetGo => "a foot lets go of the face",
                };
                report("ik body", None, what, || {
                    Some(format!("{} foot", side.label()))
                });
            }
            out.map(|(target, weight)| {
                (
                    BraceTarget {
                        ankle: to_model.transform_point3(target.ankle),
                        toe: to_model.transform_point3(target.toe),
                        knee: to_model.transform_point3(target.knee),
                        normal: to_model.transform_vector3(target.normal),
                    },
                    weight,
                )
            })
        })
    }

    /// The climb is over, or the IK left it: let go of the face, and log the spell.
    pub(crate) fn stop(&mut self, now: Instant) {
        for foot in &mut self.feet {
            foot.reset();
        }
        brace::forget();
        let Some(spell) = self.spell.take() else {
            return;
        };
        let seconds = now.saturating_duration_since(spell.began).as_secs_f32();
        let share = |s: f32| if seconds > 0.0 { s / seconds } else { 0.0 };
        tracing::info!(
            target: "body",
            seconds,
            moves = spell.moves,
            left_braced = share(spell.braced_s[0]),
            right_braced = share(spell.braced_s[1]),
            "ik body: the climb's feet",
        );
    }
}
