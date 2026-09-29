//! Physical melee: the plan of record's "Melee" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! A fast strike of either hand away from its shoulder starts the game's melee attack toward the
//! character the hand moves toward or points at. The game's melee runs from its melee input
//! (`PUSH_GRAPPLE`), which it acts on only with the hammer ability, when the character can melee
//! ([`Character::CanUseMelee`]), and outside the grapple's reel; the strike is dropped otherwise.
//! The attack turns toward the aim control's melee target slot, which the game fills from the
//! body's facing. The strike fills it instead:
//!
//! 1. On the input tick where the strike is detected ([`input_tick`]), the strike's direction is
//!    noted.
//! 2. On every aim update while the strike is live ([`write_target`]), the game's own melee scoring
//!    ([`PlayerAimControl::SetTargetMeleeFitness`]) rates each character target along the strike's
//!    direction and along the hand's pointing, and the best visible one is written into the melee
//!    slot. The aim update clears the slot at its start and refills it from the body, so the write
//!    is repeated on each update.
//! 3. On the next input tick, one tick of the melee input is injected. The game's melee reads the
//!    slot the previous aim update wrote; under the gamepad's input rules, which the controller
//!    scheme holds, it acts on the input's release one tick later, which the strike still covers.
//!
//! The strike detector is pure ([`StrikeDetector`]). A hand holding a grenade does not strike, so
//! that a throw is not a punch.

use glam::{Quat, Vec3};
use jc3gi::{
    aim::{
        aim::PlayerAimControl,
        target::{Target, TargetType},
    },
    character::{
        character::Character,
        player::{AbilitiesHandler, Player, get_hash_ability_hammer},
    },
    input::input_action_map::Action,
    types::math::Vector3,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        aim::targets::{self, TargetSet},
        bindings::hold::{Desired, MapKind, Target as ActionTarget, Want},
        body,
        config::ControllersConfig,
        frame::Hand,
        hands::ActiveHands,
        mode::{Detection, Mode, OnFootVariant},
        motion,
        weapons::{
            config::MeleeStrikeConfig,
            haptics::{self, PulseSource},
        },
    },
    headpose,
};

/// A hand's motion over the tick, as the detector reads it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrikeMotion {
    /// The hand's peak speed over the tick, metres per second.
    pub speed: f32,
    /// How much of that velocity points away from the shoulder, `-1..=1`.
    pub outward: f32,
}

/// The motion of a hand at `hand` moving at `velocity`, relative to its `shoulder`.
pub fn strike_motion(velocity: Vec3, hand: Vec3, shoulder: Vec3) -> StrikeMotion {
    let speed = velocity.length();
    let away = (hand - shoulder).try_normalize();
    let outward = match (velocity.try_normalize(), away) {
        (Some(v), Some(a)) => v.dot(a),
        _ => 0.0,
    };
    StrikeMotion {
        speed: if speed.is_finite() { speed } else { 0.0 },
        outward,
    }
}

/// The per-hand strike detector: a strike is a hand moving at least the strike speed, mostly away
/// from its shoulder, once it has slowed below the rearm speed since its last strike and its cooldown
/// has passed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrikeDetector {
    armed: [bool; 2],
    cooldown_s: [f32; 2],
}

impl StrikeDetector {
    pub const fn new() -> Self {
        Self {
            armed: [true; 2],
            cooldown_s: [0.0; 2],
        }
    }

    /// Fold one tick in and return which hands struck. A hand with no motion counts as still. A
    /// `suppressed` hand (one throwing a grenade) cannot strike, and must slow down after the
    /// suppression ends before it can, so that a throw's follow-through is not a strike.
    pub fn step(
        &mut self,
        dt: f32,
        motions: [Option<StrikeMotion>; 2],
        suppressed: [bool; 2],
        config: &MeleeStrikeConfig,
    ) -> [bool; 2] {
        let mut struck = [false; 2];
        for i in 0..2 {
            self.cooldown_s[i] = (self.cooldown_s[i] - dt.max(0.0)).max(0.0);
            if suppressed[i] {
                self.armed[i] = false;
                continue;
            }
            let motion = motions[i].unwrap_or(StrikeMotion {
                speed: 0.0,
                outward: 0.0,
            });
            if !self.armed[i] {
                self.armed[i] = motion.speed < config.rearm_speed_mps;
                continue;
            }
            if self.cooldown_s[i] <= 0.0
                && motion.speed >= config.strike_speed_mps
                && motion.outward >= config.min_outward
            {
                struck[i] = true;
                self.armed[i] = false;
                self.cooldown_s[i] = config.cooldown_s.max(0.0);
            }
        }
        struck
    }
}

impl Default for StrikeDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a strike did not start an attack.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// Not on the ground on foot, or in the grapple's reel or hang.
    Mode,
    /// The local player is not readable.
    NoCharacter,
    /// The game does not allow melee now (the character is off the ground, the animation forbids
    /// it, or a heavy weapon was just dropped).
    CannotMelee,
    /// The hammer ability is not unlocked.
    NoAbility,
    /// A strike is already under way.
    Busy,
}

impl Blocked {
    pub const fn label(self) -> &'static str {
        match self {
            Blocked::Mode => "not on the ground",
            Blocked::NoCharacter => "no character",
            Blocked::CannotMelee => "the game does not allow melee now",
            Blocked::NoAbility => "the hammer ability is locked",
            Blocked::Busy => "a strike is already under way",
        }
    }
}

/// The last strike, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrikeReport {
    pub hand: Hand,
    pub speed: f32,
    /// Why the strike did not attack, if it did not.
    pub blocked: Option<Blocked>,
}

/// Physical melee's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct MeleeStatus {
    /// Strikes detected.
    pub strikes: u64,
    /// Strikes that started an attack.
    pub attacks: u64,
    /// Attacks aimed at a character the strike chose.
    pub targeted: u64,
    pub last: Option<StrikeReport>,
    /// Whether a strike is live: its target is being written.
    pub live: bool,
    /// The last target's melee score (lower is better), when the last live strike chose one.
    pub last_score: Option<f32>,
}

pub fn status() -> MeleeStatus {
    STATE.lock().status
}

/// What the input tick hands physical melee.
pub(super) struct TickInputs<'a> {
    /// Whether melee may act: its toggle is on and the bindings inject.
    pub acting: bool,
    /// A hand that may not strike (the one holding a grenade).
    pub excluded: Option<Hand>,
    pub active: &'a ActiveHands,
    pub detection: Detection,
    pub dt: f32,
}

/// The input tick's half: detect strikes, start an attack for an allowed one, inject the melee
/// input the tick after, and keep the tethers' retract input off while a strike is live (the game's
/// gamepad melee rule ignores the melee input while it is held).
pub(super) fn input_tick(
    inputs: &TickInputs<'_>,
    config: &ControllersConfig,
    desired: &mut Vec<Desired>,
) {
    let tuning = &config.weapons.melee_strike;
    let mut state = STATE.lock();
    if !inputs.acting {
        state.stand_down();
        return;
    }
    let motions = Hand::BOTH.map(|hand| {
        let since = state.last_time[hand.index()];
        let samples = motion::history(hand);
        state.last_time[hand.index()] = samples.last().map(|s| s.time_s).or(since);
        // The first tick after melee starts acting only notes where the history ends, so that a
        // motion from before (in a menu, say) is not taken for a strike.
        let since = since?;
        let (time, velocity) = motion::peak_velocity(&samples, since)?;
        let at = samples.iter().find(|s| s.time_s == time)?.position;
        let shoulders = body::shoulder_positions(
            inputs.active.head.position,
            yaw_only(inputs.active.head.orientation),
            &body::player(),
        );
        Some((
            strike_motion(velocity, at, shoulders[hand.index()]),
            velocity,
        ))
    });
    let suppressed = Hand::BOTH.map(|hand| inputs.excluded == Some(hand));
    let struck = state.detector.step(
        inputs.dt,
        motions.map(|m| m.map(|(m, _)| m)),
        suppressed,
        tuning,
    );

    for hand in Hand::BOTH {
        let (true, Some((m, velocity))) = (struck[hand.index()], motions[hand.index()]) else {
            continue;
        };
        state.status.strikes += 1;
        let blocked = if state.live.is_some() {
            Some(Blocked::Busy)
        } else {
            // SAFETY: the input tick runs on the main thread outside the world update.
            unsafe { gate(&inputs.detection) }.err()
        };
        tracing::debug!(target: "controllers", ?hand, speed = m.speed, ?blocked, "melee strike");
        state.status.last = Some(StrikeReport {
            hand,
            speed: m.speed,
            blocked,
        });
        if blocked.is_none() {
            state.status.attacks += 1;
            state.live = Some(LiveStrike {
                hand,
                direction: headpose::xr::body_rotation() * velocity,
                age: 0,
                targeted: false,
            });
            haptics::pulse(hand, tuning.pulse, PulseSource::MeleeStrike);
        }
    }

    if let Some(live) = state.live.as_mut() {
        if live.age == 1 {
            desired.push(Desired {
                target: ActionTarget {
                    map: MapKind::Player,
                    action: Action::PUSH_GRAPPLE,
                },
                want: Want::Click,
            });
        }
        desired.retain(|d| {
            !(d.target.map == MapKind::Player && d.target.action == Action::RETRACT_GRAPPLE)
        });
        live.age += 1;
        if live.age > tuning.target_hold_ticks.max(2) {
            state.live = None;
        }
    }
    state.status.live = state.live.is_some();
}

/// Stop: forget any live strike.
pub(super) fn stand_down() {
    STATE.lock().stand_down();
}

/// Write the live strike's melee target into the aim control's melee slot, after the aim control's
/// own target pass. `pointing` is each hand's pointing direction in the world, when known.
///
/// # Safety
///
/// `aim` must be the local player's live aim control, called from its post-simulation update after
/// the original has run.
pub(crate) unsafe fn write_target(aim: *mut PlayerAimControl, pointing: [Option<Vec3>; 2]) {
    let use_pointing = Config::lock_query(|c| c.controllers.weapons.melee_strike.pointing_targets);
    let Some(live) = STATE.lock().live else {
        return;
    };
    let mut directions = vec![live.direction];
    if use_pointing && let Some(p) = pointing[live.hand.index()] {
        directions.push(p);
    }
    let directions = directions
        .into_iter()
        .filter_map(|d| Vec3::new(d.x, 0.0, d.z).try_normalize())
        .collect::<Vec<_>>();
    // SAFETY: forwarded from the caller.
    let choice = unsafe { choose(aim, &directions) };
    let Some((target, score)) = choice else {
        return;
    };
    // SAFETY: as above; the target was found live in this update.
    let Some(position) = (unsafe { targets::body_position(target) }) else {
        return;
    };
    let Some(aim) = (unsafe { aim.as_mut() }) else {
        return;
    };
    aim.m_Target[MELEE] = target;
    aim.m_AimPos[MELEE] = Vector3 {
        data: position.to_array(),
    };
    aim.m_IsInRange[MELEE] = true;
    let mut state = STATE.lock();
    state.status.last_score = Some(score);
    if let Some(l) = state.live.as_mut()
        && !l.targeted
    {
        l.targeted = true;
        state.status.targeted += 1;
    }
}

const MELEE: usize = TargetType::TARGET_MELEE as usize;

#[derive(Debug, Copy, Clone, PartialEq)]
struct LiveStrike {
    hand: Hand,
    /// The strike's velocity in world axes.
    direction: Vec3,
    /// Input ticks since the strike.
    age: u32,
    /// Whether an aim update has written a target for it.
    targeted: bool,
}

struct State {
    detector: StrikeDetector,
    /// Each hand's latest sample time the detector has read.
    last_time: [Option<f64>; 2],
    live: Option<LiveStrike>,
    status: MeleeStatus,
}

impl State {
    fn stand_down(&mut self) {
        self.detector = StrikeDetector::new();
        self.last_time = [None; 2];
        self.live = None;
        self.status.live = false;
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    detector: StrikeDetector::new(),
    last_time: [None; 2],
    live: None,
    status: MeleeStatus {
        strikes: 0,
        attacks: 0,
        targeted: 0,
        last: None,
        live: false,
        last_score: None,
    },
});

/// Whether a strike may start an attack now.
///
/// # Safety
///
/// Main thread, outside the world update.
unsafe fn gate(detection: &Detection) -> Result<(), Blocked> {
    if detection.mode != Mode::OnFoot(OnFootVariant::Ground) || detection.traversal.is_some() {
        return Err(Blocked::Mode);
    }
    let character =
        unsafe { Character::GetLocalPlayerCharacter().as_ref() }.ok_or(Blocked::NoCharacter)?;
    if !unsafe { character.CanUseMelee() } {
        return Err(Blocked::CannotMelee);
    }
    let avatar = character.m_Avatar.cast::<u8>();
    if avatar.is_null() || !character.m_IsPlayer {
        return Err(Blocked::NoCharacter);
    }
    let abilities = avatar
        .wrapping_sub(Player::AVATAR_BASE_OFFSET as usize)
        .wrapping_add(Player::ABILITIES_OFFSET as usize)
        .cast::<AbilitiesHandler>();
    let hammer = unsafe { *get_hash_ability_hammer() };
    if !unsafe { (*abilities).IsAbilityEnabled(&hammer) } {
        return Err(Blocked::NoAbility);
    }
    Ok(())
}

/// The best visible character target along any of `directions`, by the game's melee score, and
/// its score.
///
/// # Safety
///
/// As [`write_target`].
unsafe fn choose(aim: *mut PlayerAimControl, directions: &[Vec3]) -> Option<(*mut Target, f32)> {
    let aim = unsafe { aim.as_mut() }?;
    let mut scored = Vec::new();
    for target in unsafe { targets::targets(TargetSet::Characters) } {
        let Some(object) = (unsafe { targets::game_object(target) }) else {
            continue;
        };
        let best = directions
            .iter()
            .filter_map(|d| {
                let direction = Vector3 { data: d.to_array() };
                let fits = unsafe { aim.SetTargetMeleeFitness(object, target, &direction) };
                fits.then(|| unsafe { (*target).m_Fitness[MELEE] })
            })
            .filter(|s| s.is_finite())
            .min_by(f32::total_cmp);
        if let Some(score) = best {
            scored.push((target, score));
        }
    }
    scored.sort_by(|a, b| a.1.total_cmp(&b.1));
    scored
        .into_iter()
        .find(|(target, _)| unsafe { aim.IsTargetVisibleCast(*target, TargetType::TARGET_MELEE) })
}

/// The yaw of `q` alone: the rotation about +Y that turns -Z to `q`'s horizontal forward.
fn yaw_only(q: Quat) -> Quat {
    let forward = q * Vec3::NEG_Z;
    if forward.x.abs() < 1e-6 && forward.z.abs() < 1e-6 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_y((-forward.x).atan2(-forward.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: MeleeStrikeConfig = MeleeStrikeConfig::new();
    const DT: f32 = 1.0 / 30.0;
    const NONE: [bool; 2] = [false; 2];

    fn motion(speed: f32, outward: f32) -> Option<StrikeMotion> {
        Some(StrikeMotion { speed, outward })
    }

    #[test]
    fn a_punch_strikes_once() {
        let mut d = StrikeDetector::new();
        assert_eq!(
            d.step(DT, [None, motion(1.0, 0.9)], NONE, &CONFIG),
            [false; 2]
        );
        assert_eq!(
            d.step(DT, [None, motion(4.5, 0.9)], NONE, &CONFIG),
            [false, true]
        );
        // Still fast on the next tick: the same punch.
        assert_eq!(
            d.step(DT, [None, motion(4.0, 0.9)], NONE, &CONFIG),
            [false; 2]
        );
    }

    #[test]
    fn pulling_back_toward_the_body_is_not_a_strike() {
        let mut d = StrikeDetector::new();
        assert_eq!(
            d.step(DT, [motion(6.0, -0.8), None], NONE, &CONFIG),
            [false; 2]
        );
    }

    #[test]
    fn a_second_strike_waits_for_the_hand_to_slow_and_the_cooldown() {
        let mut d = StrikeDetector::new();
        assert_eq!(
            d.step(DT, [motion(5.0, 1.0), None], NONE, &CONFIG),
            [true, false]
        );
        // Slowed, but inside the cooldown.
        assert_eq!(
            d.step(DT, [motion(0.2, 1.0), None], NONE, &CONFIG),
            [false; 2]
        );
        assert_eq!(
            d.step(DT, [motion(5.0, 1.0), None], NONE, &CONFIG),
            [false; 2]
        );
        let mut t = 0.0;
        while t < CONFIG.cooldown_s {
            d.step(DT, [None, None], NONE, &CONFIG);
            t += DT;
        }
        assert_eq!(
            d.step(DT, [motion(5.0, 1.0), None], NONE, &CONFIG),
            [true, false]
        );
    }

    #[test]
    fn a_suppressed_hand_must_slow_before_it_strikes() {
        let mut d = StrikeDetector::new();
        assert_eq!(
            d.step(DT, [None, motion(6.0, 1.0)], [false, true], &CONFIG),
            [false; 2]
        );
        // The throw's follow-through, just after the suppression ends.
        assert_eq!(
            d.step(DT, [None, motion(6.0, 1.0)], NONE, &CONFIG),
            [false; 2]
        );
        assert_eq!(
            d.step(DT, [None, motion(0.5, 1.0)], NONE, &CONFIG),
            [false; 2]
        );
        assert_eq!(
            d.step(DT, [None, motion(6.0, 1.0)], NONE, &CONFIG),
            [false, true]
        );
    }

    #[test]
    fn the_hands_strike_independently() {
        let mut d = StrikeDetector::new();
        assert_eq!(
            d.step(DT, [motion(5.0, 1.0), motion(5.0, 1.0)], NONE, &CONFIG),
            [true, true]
        );
    }

    #[test]
    fn the_outward_share_is_measured_from_the_shoulder() {
        let shoulder = Vec3::new(0.2, -0.2, 0.0);
        let hand = Vec3::new(0.25, -0.3, -0.4);
        let away = strike_motion(Vec3::new(0.0, 0.0, -5.0), hand, shoulder);
        assert!(away.outward > 0.9 && (away.speed - 5.0).abs() < 1e-5);
        let back = strike_motion(Vec3::new(0.0, 0.0, 5.0), hand, shoulder);
        assert!(back.outward < -0.9);
        let still = strike_motion(Vec3::ZERO, hand, shoulder);
        assert_eq!(still.outward, 0.0);
    }

    #[test]
    fn the_yaw_drops_pitch_and_roll() {
        let q = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.5);
        let yaw = yaw_only(q);
        let f = yaw * Vec3::NEG_Z;
        assert!(f.y.abs() < 1e-6);
        assert!((yaw.angle_between(Quat::from_rotation_y(0.7))).abs() < 1e-5);
    }
}
