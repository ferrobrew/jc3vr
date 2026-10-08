//! Who poses which part of the body: the hand-over table between the procedural body and the
//! game's animation, and the springs that blend each region from one to the other. Pure.
//!
//! The body is two regions: the legs (the pelvis and both legs) and the torso (the hips, the spine,
//! the neck, the head, and the clavicles). The arms are the controllers' in every case, through the
//! hand IK, and the head is the headset's, through the head override. [`classify`] reads the
//! character's state and names the case, first match winning:
//!
//! | Case | Legs | Torso |
//! | --- | --- | --- |
//! | not in the headset, not in gameplay (menus, cutscenes) | animation | animation |
//! | in a scripted sequence | animation | animation |
//! | dead | animation, at once | animation, at once |
//! | ragdoll | animation, at once | animation, at once |
//! | getting up from a ragdoll | animation | animation |
//! | seated and riding in a vehicle | animation | procedural on the seat's hips (animation with `seated_torso` off) |
//! | in or on a vehicle otherwise (entering, leaving, a mounted gun, a stunt position) | animation | animation |
//! | grapple traversal, hanging, stunt traversal, grapple struggle | animation | animation |
//! | the grapple's ledge climb | animation | animation |
//! | swimming | animation | animation |
//! | wingsuit, parachute, a fall past the jump's grace | animation | animation |
//! | a hit reaction (a stumble or a knock-back), for a hold after its act | animation | animation |
//! | climbing with the hands | procedural, hanging in the air (animation with `air_legs` off) | procedural |
//! | a jump, within the grace | procedural, hanging in the air (animation with `air_legs` off) | procedural |
//! | an authored one-off (a locomotion one-off, a hard landing, a context action, a full-body pickup, cover) | animation | animation |
//! | on foot without the movement task running | animation | animation |
//! | on foot on the ground | procedural | procedural |
//!
//! Where the torso is procedural over hips the animation keeps ([`Case::anchored`]), the solve
//! keeps the clip's hips and moves the spine from there (`solver::anchored`) rather than building
//! the body under the head.
//!
//! [`RegionBlend`] eases each region's weight toward its case's with a critically damped spring,
//! quicker toward the animation than back, and at once where the case says so.

use crate::controllers::body::procedural::config::ProceduralBodyConfig;

/// Why a region is posed the way it is: the case [`classify`] matched.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum Case {
    #[default]
    NotInHeadset,
    /// The procedural body is switched off and blends back to the animation.
    SwitchedOff,
    NotInGameplay,
    Sequence,
    Dead,
    Ragdoll,
    GettingUp,
    /// Seated and riding in a vehicle: not entering, leaving, or on a mounted gun.
    Seated,
    Vehicle,
    Traversal,
    LedgeClimb,
    Swimming,
    Flight,
    HitReaction,
    HandClimb,
    Jump,
    AuthoredMotion,
    NoMovementTask,
    OnFoot,
}

impl Case {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotInHeadset => "not in the headset",
            Self::SwitchedOff => "switched off",
            Self::NotInGameplay => "not in gameplay",
            Self::Sequence => "scripted sequence",
            Self::Dead => "dead",
            Self::Ragdoll => "ragdoll",
            Self::GettingUp => "getting up",
            Self::Seated => "seated in a vehicle",
            Self::Vehicle => "vehicle",
            Self::Traversal => "grapple or stunt traversal",
            Self::LedgeClimb => "grapple ledge climb",
            Self::Swimming => "swimming",
            Self::Flight => "wingsuit, parachute, or fall",
            Self::HitReaction => "hit reaction",
            Self::HandClimb => "climbing with the hands",
            Self::Jump => "jump",
            Self::AuthoredMotion => "authored motion",
            Self::NoMovementTask => "movement task not running",
            Self::OnFoot => "on foot",
        }
    }

    /// The weights the case asks for: the legs', then the torso's. In a jump and a hand climb the
    /// legs are the procedural body's, hanging in the air, when `air_legs`, and the animation's
    /// otherwise. Seated, the torso is the procedural body's when `seated_torso`, and the legs keep
    /// the seat's animation.
    pub const fn targets(self, config: &ProceduralBodyConfig) -> [f32; 2] {
        match self {
            Self::OnFoot => [1.0, 1.0],
            Self::Jump | Self::HandClimb if config.air_legs => [1.0, 1.0],
            Self::Jump | Self::HandClimb => [0.0, 1.0],
            Self::Seated if config.seated_torso => [0.0, 1.0],
            _ => [0.0, 0.0],
        }
    }

    /// Whether the animation keeps the hips while the torso is procedural: the solve moves the
    /// spine from the clip's hips (`solver::anchored`) instead of building the body under the head.
    pub const fn anchored(self) -> bool {
        matches!(self, Self::Seated)
    }

    /// Whether the animation takes over at once, with no blend: a ragdoll writes the pose
    /// itself, and a death's fall is a ragdoll or an authored clip from its first tick.
    pub const fn snaps(self) -> bool {
        matches!(self, Self::Dead | Self::Ragdoll)
    }
}

/// What the character is doing, as [`classify`] needs it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CharacterState {
    pub in_headset: bool,
    pub in_gameplay: bool,
    /// Driven by a scripted sequence.
    pub sequence: bool,
    pub dead: bool,
    pub ragdoll: bool,
    pub getting_up: bool,
    pub vehicle: bool,
    /// Seated and riding in the vehicle, not in a stunt position: a narrower [`vehicle`](Self::vehicle).
    pub seated: bool,
    pub traversal: bool,
    /// Climbing over a ledge at the end of a grapple.
    pub ledge_climb: bool,
    pub swimming: bool,
    /// In the wingsuit, under the parachute, or falling past the jump's grace.
    pub flight: bool,
    /// Within the hold of a stumble's or a knock-back's act.
    pub hit_reaction: bool,
    /// Hanging from a hand that grips the world.
    pub hand_climb: bool,
    /// In the air within the jump's grace: a jump, a fall, or the proxy off its support. It wins
    /// over a movement task that ran this tick, which it can on the tick the body leaves the ground.
    pub jump: bool,
    pub authored_motion: bool,
    /// Whether the on-foot movement task ran since the last gate.
    pub movement_task: bool,
}

/// The case `state` falls in.
pub fn classify(state: &CharacterState) -> Case {
    if !state.in_headset {
        Case::NotInHeadset
    } else if !state.in_gameplay {
        Case::NotInGameplay
    } else if state.sequence {
        Case::Sequence
    } else if state.dead {
        Case::Dead
    } else if state.ragdoll {
        Case::Ragdoll
    } else if state.getting_up {
        Case::GettingUp
    } else if state.vehicle && state.seated {
        Case::Seated
    } else if state.vehicle {
        Case::Vehicle
    } else if state.traversal {
        Case::Traversal
    } else if state.ledge_climb {
        Case::LedgeClimb
    } else if state.swimming {
        Case::Swimming
    } else if state.flight {
        Case::Flight
    } else if state.hit_reaction {
        Case::HitReaction
    } else if state.hand_climb {
        Case::HandClimb
    } else if state.jump {
        Case::Jump
    } else if state.authored_motion {
        Case::AuthoredMotion
    } else if !state.movement_task {
        Case::NoMovementTask
    } else {
        Case::OnFoot
    }
}

/// The two regions' weights, eased toward their targets.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RegionBlend {
    springs: [Spring; 2],
}

/// The regions' weights: the legs', then the torso's.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Weights {
    pub legs: f32,
    pub torso: f32,
}

impl Weights {
    pub fn any(&self) -> bool {
        self.legs > 0.0 || self.torso > 0.0
    }
}

impl RegionBlend {
    pub const fn new() -> Self {
        Self {
            springs: [Spring::new(); 2],
        }
    }

    /// Ease toward `case`'s weights over `dt` seconds.
    pub fn step(&mut self, case: Case, dt: f32, config: &ProceduralBodyConfig) -> Weights {
        let targets = case.targets(config);
        for (spring, target) in self.springs.iter_mut().zip(targets) {
            if case.snaps() {
                *spring = Spring {
                    value: target,
                    velocity: 0.0,
                };
            } else {
                let time = if target < spring.value {
                    config.blend_out_s
                } else {
                    config.blend_s
                };
                spring.step(target, dt, time);
            }
        }
        self.weights()
    }

    pub fn weights(&self) -> Weights {
        Weights {
            legs: self.springs[0].value,
            torso: self.springs[1].value,
        }
    }

    /// Drop both regions to the animation at once.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// A critically damped spring on a weight in `0..=1`.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    const fn new() -> Self {
        Self {
            value: 0.0,
            velocity: 0.0,
        }
    }

    /// Move toward `target` with the smoothing time `time` (about the time to cover most of the
    /// way). A weight within [`SETTLE`] of its target lands on it, so a region fully handed over is
    /// exactly zero or one.
    fn step(&mut self, target: f32, dt: f32, time: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if time <= 0.0 {
            self.value = target;
            self.velocity = 0.0;
            return;
        }
        let omega = 2.0 / time;
        let x = omega * dt;
        let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
        let change = self.value - target;
        let temp = (self.velocity + omega * change) * dt;
        self.velocity = (self.velocity - omega * temp) * decay;
        self.value = target + (change + temp) * decay;
        if (self.value - target).abs() < SETTLE {
            self.value = target;
            self.velocity = 0.0;
        }
        self.value = self.value.clamp(0.0, 1.0);
    }
}

/// How close a weight comes to its target before it lands on it.
const SETTLE: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use super::*;

    fn on_foot() -> CharacterState {
        CharacterState {
            in_headset: true,
            in_gameplay: true,
            movement_task: true,
            ..CharacterState::default()
        }
    }

    #[test]
    fn the_table_picks_the_first_matching_case() {
        assert_eq!(classify(&on_foot()), Case::OnFoot);
        assert_eq!(
            classify(&CharacterState {
                jump: true,
                movement_task: false,
                ..on_foot()
            }),
            Case::Jump
        );
        assert_eq!(
            classify(&CharacterState {
                ragdoll: true,
                vehicle: true,
                ..on_foot()
            }),
            Case::Ragdoll
        );
        assert_eq!(
            classify(&CharacterState {
                dead: true,
                ragdoll: true,
                ..on_foot()
            }),
            Case::Dead
        );
        assert_eq!(
            classify(&CharacterState {
                in_headset: false,
                ..on_foot()
            }),
            Case::NotInHeadset
        );
        assert_eq!(
            classify(&CharacterState {
                movement_task: false,
                ..on_foot()
            }),
            Case::NoMovementTask
        );
        assert_eq!(
            classify(&CharacterState {
                authored_motion: true,
                ..on_foot()
            }),
            Case::AuthoredMotion
        );
        // Leaving the ground wins over the movement task that ran on the same tick.
        assert_eq!(
            classify(&CharacterState {
                jump: true,
                ..on_foot()
            }),
            Case::Jump
        );
        // The climb carries the body itself, and wins over the jump its start looks like.
        assert_eq!(
            classify(&CharacterState {
                ledge_climb: true,
                jump: true,
                ..on_foot()
            }),
            Case::LedgeClimb
        );
        assert_eq!(
            classify(&CharacterState {
                sequence: true,
                dead: true,
                ..on_foot()
            }),
            Case::Sequence
        );
        // A reaction wins over the jump a knock-back looks like, and loses to a ragdoll.
        assert_eq!(
            classify(&CharacterState {
                hit_reaction: true,
                jump: true,
                ..on_foot()
            }),
            Case::HitReaction
        );
        assert_eq!(
            classify(&CharacterState {
                hit_reaction: true,
                ragdoll: true,
                ..on_foot()
            }),
            Case::Ragdoll
        );
        // Hanging from the hands is on foot to the game; a reaction or a vehicle still wins.
        assert_eq!(
            classify(&CharacterState {
                hand_climb: true,
                ..on_foot()
            }),
            Case::HandClimb
        );
        assert_eq!(
            classify(&CharacterState {
                hand_climb: true,
                hit_reaction: true,
                ..on_foot()
            }),
            Case::HitReaction
        );
        let config = ProceduralBodyConfig::new();
        let animated_legs = ProceduralBodyConfig {
            air_legs: false,
            ..config
        };
        assert_eq!(Case::HandClimb.targets(&config), [1.0, 1.0]);
        assert_eq!(Case::HandClimb.targets(&animated_legs), [0.0, 1.0]);
    }

    #[test]
    fn seated_riding_takes_the_torso_and_leaves_the_legs_to_the_seat() {
        let seated = CharacterState {
            vehicle: true,
            seated: true,
            ..on_foot()
        };
        assert_eq!(classify(&seated), Case::Seated);
        // Entering, leaving, or a stunt position is the vehicle's animation.
        assert_eq!(
            classify(&CharacterState {
                seated: false,
                ..seated
            }),
            Case::Vehicle
        );
        // A stale seat outside a vehicle is nothing.
        assert_eq!(
            classify(&CharacterState {
                vehicle: false,
                ..seated
            }),
            Case::OnFoot
        );
        assert_eq!(
            classify(&CharacterState {
                ragdoll: true,
                ..seated
            }),
            Case::Ragdoll
        );
        let config = ProceduralBodyConfig::new();
        assert_eq!(Case::Seated.targets(&config), [0.0, 1.0]);
        assert!(Case::Seated.anchored() && !Case::OnFoot.anchored());
        let off = ProceduralBodyConfig {
            seated_torso: false,
            ..config
        };
        assert_eq!(Case::Seated.targets(&off), [0.0, 0.0]);
    }

    #[test]
    fn regions_ease_in_smoothly_and_reach_their_targets() {
        let config = ProceduralBodyConfig::new();
        let mut blend = RegionBlend::new();
        let mut last = 0.0;
        for _ in 0..90 {
            let w = blend.step(Case::OnFoot, 1.0 / 30.0, &config);
            assert!(w.torso >= last && w.torso <= 1.0);
            last = w.torso;
        }
        assert_eq!(
            blend.weights(),
            Weights {
                legs: 1.0,
                torso: 1.0
            }
        );
        let w = blend.step(Case::Jump, 1.0 / 30.0, &config);
        assert!(w.legs == 1.0 && w.torso == 1.0);
        let animated_legs = ProceduralBodyConfig {
            air_legs: false,
            ..config
        };
        let w = blend.step(Case::Jump, 1.0 / 30.0, &animated_legs);
        assert!(w.legs < 1.0 && w.torso == 1.0);
    }

    #[test]
    fn leaving_is_quicker_than_arriving() {
        let config = ProceduralBodyConfig::new();
        let ticks_to = |from: Case, to: Case, done: fn(&Weights) -> bool| {
            let mut blend = RegionBlend::new();
            for _ in 0..300 {
                blend.step(from, 1.0 / 60.0, &config);
            }
            (0..300)
                .position(|_| done(&blend.step(to, 1.0 / 60.0, &config)))
                .unwrap()
        };
        let arrive = ticks_to(Case::Vehicle, Case::OnFoot, |w| w.torso > 0.9);
        let leave = ticks_to(Case::OnFoot, Case::Vehicle, |w| w.torso < 0.1);
        assert!(leave < arrive, "{leave} >= {arrive}");
    }

    #[test]
    fn switching_off_blends_back_rather_than_snapping() {
        let config = ProceduralBodyConfig::new();
        let mut blend = RegionBlend::new();
        for _ in 0..60 {
            blend.step(Case::OnFoot, 1.0 / 30.0, &config);
        }
        let w = blend.step(Case::SwitchedOff, 1.0 / 30.0, &config);
        assert!(w.torso > 0.0 && w.torso < 1.0);
        for _ in 0..60 {
            blend.step(Case::SwitchedOff, 1.0 / 30.0, &config);
        }
        assert!(!blend.weights().any());
    }

    #[test]
    fn a_ragdoll_snaps_to_the_animation() {
        let config = ProceduralBodyConfig::new();
        let mut blend = RegionBlend::new();
        for _ in 0..60 {
            blend.step(Case::OnFoot, 1.0 / 30.0, &config);
        }
        let w = blend.step(Case::Ragdoll, 1.0 / 30.0, &config);
        assert!(!w.any());
    }
}
