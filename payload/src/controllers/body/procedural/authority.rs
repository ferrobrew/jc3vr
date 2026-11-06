//! Who poses the torso: the hand-over table between the procedural body and the game's animation,
//! and the spring that blends the torso from one to the other. Pure.
//!
//! The game's animation keeps the hips, the pelvis, and the legs in every case. The torso (the
//! spine, the neck, the head, and the clavicles) is posed on the animation's hips where the table
//! gives it to the player, the arms are the controllers' in every case, through the hand IK, and
//! the head is the headset's, through the head override. [`classify`] reads the character's state
//! and names the case, first match winning:
//!
//! | Case | Torso |
//! | --- | --- |
//! | not in the headset, not in gameplay (menus, cutscenes) | animation |
//! | in a scripted sequence | animation |
//! | dead | animation, at once |
//! | ragdoll | animation, at once |
//! | getting up from a ragdoll | animation |
//! | seated and riding in a vehicle | procedural on the seat's hips |
//! | standing in a vehicle's stunt area (a roof, a deck) | procedural on the stunt idle's hips |
//! | in or on a vehicle otherwise (entering, leaving, a mounted gun, a helicopter's hang, a stunt traversal) | animation |
//! | attached to a wall at the end of a grapple, a hang | procedural on the attach's hips |
//! | grapple traversal, hanging, stunt traversal, grapple struggle | animation, after a flight's hold |
//! | the grapple's ledge climb | animation |
//! | swimming | procedural on the swim's hips |
//! | the wingsuit (the arms follow the player's) | animation |
//! | the parachute | procedural on the harness's hips |
//! | a fall past the jump's grace | animation, after a flight's hold |
//! | a hit reaction (a stumble or a knock-back), for a hold after its act | animation |
//! | climbing with the hands | procedural |
//! | a jump, within the grace | procedural |
//! | an authored one-off (a locomotion one-off, a hard landing, a context action, a full-body pickup, cover) | animation |
//! | on foot without the movement task running | animation, after a flight's hold |
//! | on foot on the ground | procedural |
//!
//! [`TorsoBlend`] eases the torso's weight toward its case's with a critically damped spring,
//! quicker toward the animation than back, and at once where the case says so. A flight keeps the
//! torso for a while: a fall, a grapple traversal, or a takeoff not yet classified that follows a
//! jump or the parachute leaves it procedural for [`FLIGHT_HOLD_S`] of the simulation's time, which
//! each jump or spell under the parachute renews. A jump that outlasts its grace, and the canopy
//! closed and opened again around a grapple's reel, then never hand the torso to the animation and
//! back within a second; a flight that outlasts the hold hands it over once.

use body_rig::smooth::critically_damped;

use crate::controllers::body::procedural::config::ProceduralBodyConfig;

/// Why the torso is posed the way it is: the case [`classify`] matched.
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
    /// Standing in a vehicle's stunt area: a car's roof, a boat's deck, a plane's wing.
    StuntStanding,
    Vehicle,
    /// Attached to a wall at the end of a grapple's reel: a hang the clip holds still.
    GrappleAttach,
    Traversal,
    LedgeClimb,
    Swimming,
    /// In the wingsuit, or going into it. The arms follow the player's apart from the torso.
    Wingsuit,
    Parachute,
    /// Falling past the jump's grace.
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
            Self::StuntStanding => "standing on a vehicle",
            Self::Vehicle => "vehicle",
            Self::GrappleAttach => "grapple wall attach",
            Self::Traversal => "grapple or stunt traversal",
            Self::LedgeClimb => "grapple ledge climb",
            Self::Swimming => "swimming",
            Self::Wingsuit => "wingsuit",
            Self::Parachute => "parachute",
            Self::Flight => "a fall past the jump's grace",
            Self::HitReaction => "hit reaction",
            Self::HandClimb => "climbing with the hands",
            Self::Jump => "jump",
            Self::AuthoredMotion => "authored motion",
            Self::NoMovementTask => "movement task not running",
            Self::OnFoot => "on foot",
        }
    }

    /// The torso's weight the case asks for.
    pub const fn target(self) -> f32 {
        match self {
            Self::OnFoot
            | Self::Jump
            | Self::HandClimb
            | Self::Parachute
            | Self::Seated
            | Self::StuntStanding
            | Self::GrappleAttach
            | Self::Swimming => 1.0,
            _ => 0.0,
        }
    }

    /// Whether a clip, a script, or physics poses Rico's whole body while he is out of a vehicle,
    /// and can carry his head anywhere against the root: a traversal or a hang, a ledge climb, the
    /// wingsuit, the parachute, a fall, a ragdoll and the get-up after it, a hit reaction, an
    /// authored motion such as a hard landing, or a takeoff the mode detection has not classified
    /// yet. The camera keeps its eyes out in front of the animated face in these.
    pub const fn game_poses_body(self) -> bool {
        matches!(
            self,
            Self::Dead
                | Self::Ragdoll
                | Self::GettingUp
                | Self::GrappleAttach
                | Self::Traversal
                | Self::LedgeClimb
                | Self::Wingsuit
                | Self::Parachute
                | Self::Flight
                | Self::HitReaction
                | Self::AuthoredMotion
                | Self::NoMovementTask
        )
    }

    /// Whether the animation takes over at once, with no blend: a ragdoll writes the pose
    /// itself, and a death's fall is a ragdoll or an authored clip from its first tick.
    pub const fn snaps(self) -> bool {
        matches!(self, Self::Dead | Self::Ragdoll)
    }

    /// Whether the case renews the torso's hold through a flight: the procedural cases in the air.
    const fn renews_flight_hold(self) -> bool {
        matches!(self, Self::Jump | Self::Parachute)
    }

    /// Whether the torso's hold through a flight keeps the torso procedural in the case: the
    /// animation's cases a flight passes through between a jump or the parachute and the next.
    const fn within_flight_hold(self) -> bool {
        matches!(self, Self::Flight | Self::Traversal | Self::NoMovementTask)
    }

    /// Whether the player's own movement carries the body on foot: the stick on the ground, a jump
    /// within its grace, or the hands climbing. In every other case a clip, a script, or physics
    /// moves the head against the root (a get-up, a stumble, a hard landing, a context action, a
    /// takeoff the mode detection has not classified yet), and the on-foot view follows the
    /// animated head rather than a standing head held over the root.
    pub const fn player_carries_body(self) -> bool {
        matches!(self, Self::OnFoot | Self::Jump | Self::HandClimb)
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
    /// Standing in a vehicle's stunt area, not traversing it: a narrower [`vehicle`](Self::vehicle).
    pub stunt_standing: bool,
    pub traversal: bool,
    /// Attached to a wall at the end of a grapple's reel.
    pub grapple_attach: bool,
    /// Climbing over a ledge at the end of a grapple.
    pub ledge_climb: bool,
    pub swimming: bool,
    /// In the wingsuit, or going into it.
    pub wingsuit: bool,
    /// Under the parachute.
    pub parachute: bool,
    /// Falling past the jump's grace.
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
    } else if state.vehicle && state.stunt_standing {
        Case::StuntStanding
    } else if state.vehicle {
        Case::Vehicle
    } else if state.grapple_attach {
        Case::GrappleAttach
    } else if state.traversal {
        Case::Traversal
    } else if state.ledge_climb {
        Case::LedgeClimb
    } else if state.swimming {
        Case::Swimming
    } else if state.wingsuit {
        Case::Wingsuit
    } else if state.parachute {
        Case::Parachute
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

/// How long a flight keeps the torso procedural after a jump or the parachute, seconds of the
/// simulation's time.
pub const FLIGHT_HOLD_S: f32 = 1.5;

/// The torso's weight, eased toward its case's.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TorsoBlend {
    spring: Spring,
    /// What is left of the torso's hold through a flight, seconds.
    hold_s: f32,
    /// The weight the blend last eased toward.
    target: f32,
}

impl TorsoBlend {
    pub const fn new() -> Self {
        Self {
            spring: Spring::new(),
            hold_s: 0.0,
            target: 0.0,
        }
    }

    /// Ease toward `case`'s weight over `dt` seconds, or toward the procedural pose while a flight
    /// holds the torso.
    pub fn step(&mut self, case: Case, dt: f32, config: &ProceduralBodyConfig) -> f32 {
        let target = self.hold(case, dt);
        self.target = target;
        if case.snaps() {
            self.spring = Spring {
                value: target,
                velocity: 0.0,
            };
        } else {
            let time = if target < self.spring.value {
                config.blend_out_s
            } else {
                config.blend_s
            };
            self.spring.step(target, dt, time);
        }
        self.weight()
    }

    pub fn weight(&self) -> f32 {
        self.spring.value
    }

    /// The weight the blend last eased toward: its case's, or one while a flight holds the torso.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// The weight `case` asks for, through the hold: a jump or the parachute renews it, and the
    /// cases a flight passes through spend it on the procedural pose until it runs out. Any other
    /// case ends it.
    fn hold(&mut self, case: Case, dt: f32) -> f32 {
        if case.renews_flight_hold() {
            self.hold_s = FLIGHT_HOLD_S;
        } else if case.within_flight_hold() && self.hold_s > 0.0 {
            self.hold_s = (self.hold_s - dt).max(0.0);
            return 1.0;
        } else {
            self.hold_s = 0.0;
        }
        case.target()
    }

    /// Drop the torso to the animation at once.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// One weight in `0..=1` eased toward its target as the torso's is: over `in_s` toward one and the
/// quicker `out_s` back.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct WeightFollower {
    spring: Spring,
}

impl WeightFollower {
    pub const fn new() -> Self {
        Self {
            spring: Spring::new(),
        }
    }

    /// Ease toward `target` over `dt` seconds.
    pub fn step(&mut self, target: f32, dt: f32, in_s: f32, out_s: f32) -> f32 {
        let target = if target.is_finite() {
            target.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let time = if target < self.spring.value {
            out_s
        } else {
            in_s
        };
        self.spring.step(target, dt, time);
        self.spring.value
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
    /// way). A weight within [`SETTLE`] of its target lands on it, so a torso fully handed over is
    /// exactly zero or one.
    fn step(&mut self, target: f32, dt: f32, time: f32) {
        (self.value, self.velocity) =
            critically_damped(self.value, self.velocity, target, time, dt);
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
        assert_eq!(Case::HandClimb.target(), 1.0);
    }

    #[test]
    fn the_grapple_wall_attach_is_a_hang_with_the_torso() {
        let attached = CharacterState {
            grapple_attach: true,
            traversal: true,
            flight: true,
            ..on_foot()
        };
        assert_eq!(classify(&attached), Case::GrappleAttach);
        assert_eq!(Case::GrappleAttach.target(), 1.0);
        assert!(Case::GrappleAttach.game_poses_body());
        assert!(!Case::OnFoot.game_poses_body());
        assert!(!Case::Seated.game_poses_body());
    }

    #[test]
    fn seated_riding_takes_the_torso() {
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
        assert_eq!(Case::Seated.target(), 1.0);
        let roof = CharacterState {
            seated: false,
            stunt_standing: true,
            ..seated
        };
        assert_eq!(classify(&roof), Case::StuntStanding);
        assert_eq!(Case::StuntStanding.target(), 1.0);
        // A ragdoll thrown off the roof is the ragdoll.
        assert_eq!(
            classify(&CharacterState {
                ragdoll: true,
                ..roof
            }),
            Case::Ragdoll
        );
    }

    #[test]
    fn the_flying_states_are_told_apart() {
        let flying = |wingsuit, parachute, flight| CharacterState {
            wingsuit,
            parachute,
            flight,
            ..on_foot()
        };
        assert_eq!(classify(&flying(true, false, true)), Case::Wingsuit);
        assert_eq!(classify(&flying(false, true, true)), Case::Parachute);
        assert_eq!(classify(&flying(false, false, true)), Case::Flight);
        // The wingsuit's arms are not the torso: the body stays the flight pose.
        assert_eq!(Case::Wingsuit.target(), 0.0);
        // Under the parachute the torso is on the harness's hips.
        assert_eq!(Case::Parachute.target(), 1.0);
        // Swimming, the torso is on the swim's hips.
        assert_eq!(Case::Swimming.target(), 1.0);
        assert_eq!(
            classify(&CharacterState {
                swimming: true,
                ..flying(true, false, false)
            }),
            Case::Swimming
        );
    }

    #[test]
    fn a_weight_follower_eases_in_and_out_at_its_own_pace() {
        let mut follower = WeightFollower::new();
        let mut last = 0.0;
        for _ in 0..180 {
            let w = follower.step(1.0, 1.0 / 60.0, 0.35, 0.2);
            assert!(w >= last && w <= 1.0);
            last = w;
        }
        assert_eq!(last, 1.0);
        let out = (0..60)
            .position(|_| follower.step(0.0, 1.0 / 60.0, 0.35, 0.2) < 0.1)
            .unwrap();
        assert!(out < 30, "{out}");
        assert_eq!(follower.step(f32::NAN, 1.0, 0.35, 0.2), 0.0);
    }

    #[test]
    fn the_torso_eases_in_smoothly_and_reaches_its_target() {
        let config = ProceduralBodyConfig::new();
        let mut blend = TorsoBlend::new();
        let mut last = 0.0;
        for _ in 0..90 {
            let w = blend.step(Case::OnFoot, 1.0 / 30.0, &config);
            assert!(w >= last && w <= 1.0);
            last = w;
        }
        assert_eq!(blend.weight(), 1.0);
        assert_eq!(blend.step(Case::Jump, 1.0 / 30.0, &config), 1.0);
    }

    #[test]
    fn leaving_is_quicker_than_arriving() {
        let config = ProceduralBodyConfig::new();
        let ticks_to = |from: Case, to: Case, done: fn(f32) -> bool| {
            let mut blend = TorsoBlend::new();
            for _ in 0..300 {
                blend.step(from, 1.0 / 60.0, &config);
            }
            (0..300)
                .position(|_| done(blend.step(to, 1.0 / 60.0, &config)))
                .unwrap()
        };
        let arrive = ticks_to(Case::Vehicle, Case::OnFoot, |w| w > 0.9);
        let leave = ticks_to(Case::OnFoot, Case::Vehicle, |w| w < 0.1);
        assert!(leave < arrive, "{leave} >= {arrive}");
    }

    #[test]
    fn switching_off_blends_back_rather_than_snapping() {
        let config = ProceduralBodyConfig::new();
        let mut blend = TorsoBlend::new();
        for _ in 0..60 {
            blend.step(Case::OnFoot, 1.0 / 30.0, &config);
        }
        let w = blend.step(Case::SwitchedOff, 1.0 / 30.0, &config);
        assert!(w > 0.0 && w < 1.0);
        for _ in 0..60 {
            blend.step(Case::SwitchedOff, 1.0 / 30.0, &config);
        }
        assert_eq!(blend.weight(), 0.0);
    }

    /// The weights over `seconds` of `case` at 60 Hz.
    fn ease(blend: &mut TorsoBlend, case: Case, seconds: f32) -> Vec<f32> {
        let config = ProceduralBodyConfig::new();
        (0..(seconds * 60.0) as usize)
            .map(|_| blend.step(case, 1.0 / 60.0, &config))
            .collect()
    }

    #[test]
    fn a_jump_past_its_grace_keeps_the_torso_until_the_hold_runs_out() {
        let mut blend = TorsoBlend::new();
        ease(&mut blend, Case::OnFoot, 2.0);
        ease(&mut blend, Case::Jump, 1.0);
        let held = ease(&mut blend, Case::Flight, FLIGHT_HOLD_S * 0.9);
        assert!(held.iter().all(|&w| w == 1.0), "{held:?}");
        assert_eq!(ease(&mut blend, Case::OnFoot, 0.5).last(), Some(&1.0));

        ease(&mut blend, Case::Jump, 1.0);
        let fall = ease(&mut blend, Case::Flight, FLIGHT_HOLD_S + 1.0);
        assert_eq!(fall.last(), Some(&0.0));
        assert_eq!(blend.target(), 0.0);
    }

    #[test]
    fn toggling_the_canopy_around_a_reel_keeps_the_torso() {
        let mut blend = TorsoBlend::new();
        ease(&mut blend, Case::Parachute, 2.0);
        for _ in 0..4 {
            for (case, seconds) in [
                (Case::Traversal, 0.8),
                (Case::Parachute, 0.9),
                (Case::Flight, 0.5),
                (Case::Traversal, 0.4),
                (Case::Flight, 0.3),
                (Case::Parachute, 0.9),
            ] {
                let weights = ease(&mut blend, case, seconds);
                assert!(weights.iter().all(|&w| w == 1.0), "{case:?}: {weights:?}");
            }
        }
    }

    #[test]
    fn a_reel_from_the_ground_hands_over_at_once() {
        let mut blend = TorsoBlend::new();
        ease(&mut blend, Case::OnFoot, 2.0);
        ease(&mut blend, Case::Jump, 0.2);
        ease(&mut blend, Case::OnFoot, 0.5);
        ease(&mut blend, Case::NoMovementTask, 0.05);
        let reel = ease(&mut blend, Case::Traversal, 0.5);
        assert!(reel.last().is_some_and(|&w| w < 0.1), "{reel:?}");
    }

    #[test]
    fn a_ragdoll_snaps_to_the_animation() {
        let config = ProceduralBodyConfig::new();
        let mut blend = TorsoBlend::new();
        for _ in 0..60 {
            blend.step(Case::OnFoot, 1.0 / 30.0, &config);
        }
        assert_eq!(blend.step(Case::Ragdoll, 1.0 / 30.0, &config), 0.0);
    }
}
