//! The engine side of a snatched gun's arrival: the aligned landing that steers the flying body into
//! the pose the catching hand will hold it in ([`crate::controllers::snatch::landing`] has the
//! math), and the watch over the gun's first ticks in the hand.
//!
//! The landing reads the gun's body frame relative to the gun's own frame once at the hook's bite,
//! while the gun lies still, so each tick of the flight knows where the gun's grip is and what pose
//! its body must reach for the gun to sit on a hand's canonical grip
//! ([`crate::controllers::hands::grip::canonical_on_wrist_frame`]).
//!
//! The watch logs, for a few ticks after the catch, the weapon's state, whether its model is drawn,
//! whether the rigid grip pinned it, and the arrival slide's progress, and shows the weapon's model
//! again whenever the take left it hidden while it is in the catching hand.

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use jc3gi::{
    character::character::Character, equipment::weapon::WeaponBase,
    graphics_engine::model_instance::ModelInstanceFlags,
};

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    hands::{grip, ik, pose as hand_pose},
    loadout::live,
    physical_hands::rigid::BodyPose,
    snatch::{
        SNATCH,
        landing::{self, Turning},
    },
    weapons::{engine::gun_pose, hold::act},
};

/// A snatched gun's landing, from the bite to the catch.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Landing {
    /// The gun's body frame (its centre of mass and its rotation) in the gun's own frame.
    body_in_gun: Pose,
    /// How far from the line's hand the drive started, metres.
    start_m: Option<f32>,
    /// Ticks since the drive started.
    ticks: u32,
}

/// What a tick of the landing gives the flight.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct LandingTick {
    /// The gun's grip point now.
    pub grip: Vec3,
    /// Where each hand would hold the gun, by [`Hand::index`]: the gun's own pose on that hand's
    /// canonical grip.
    pub targets: [Option<Pose>; 2],
    /// The body's angular velocity the drive asks for, when it drives.
    pub angular: Option<Vec3>,
    /// How far the line's aim moves off the line's palm.
    pub shift: Vec3,
}

/// What the landing reads of the flight on a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct LandingInput {
    pub body: BodyPose,
    pub velocity: Vec3,
    pub angular: Vec3,
    /// The hand the line steers toward, and its palm.
    pub receiver: Hand,
    pub palm: Vec3,
    pub dt: f32,
}

impl Landing {
    /// The landing of `weapon`, whose body is at `body`: `None` when the gun's pose cannot be read.
    ///
    /// # Safety
    ///
    /// `weapon` must be live, on the game thread before the world update.
    pub(super) unsafe fn measure(weapon: &WeaponBase, body: BodyPose) -> Option<Self> {
        let gun = unsafe { gun_pose(weapon) }?;
        let body_in_gun = hand_pose::mul(
            hand_pose::inverse(gun),
            Pose {
                position: body.centre,
                orientation: body.orientation,
            },
        );
        (body_in_gun.position.is_finite() && body_in_gun.orientation.is_finite()).then_some(Self {
            body_in_gun,
            start_m: None,
            ticks: 0,
        })
    }

    /// One tick of the landing.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(super) unsafe fn tick(&mut self, input: &LandingInput) -> LandingTick {
        let tuning = &SNATCH.landing;
        let body = Pose {
            position: input.body.centre,
            orientation: input.body.orientation,
        };
        let gun = hand_pose::mul(body, hand_pose::inverse(self.body_in_gun));
        let grip = gun.position;
        let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() };
        let records = ik::tick_records();
        let wrists = Hand::BOTH.map(|hand| {
            let current = records.current[hand.index()]?.wrist;
            let canonical = unsafe { grip::canonical_on_wrist_frame(character?, hand) }?;
            let previous = records.previous[hand.index()].map(|t| t.wrist);
            Some((current, previous, canonical))
        });
        let catch_reach = SNATCH.catch_zone_m;
        let targets = Hand::BOTH.map(|hand| {
            let (current, previous, canonical) = wrists[hand.index()]?;
            let now = hand_pose::mul(current, canonical);
            let t_go = landing::time_to_go(grip, input.velocity, now.position, catch_reach, tuning);
            let (position, orientation, angular) = landing::extrapolate(
                previous.map(|p| (p.position, p.orientation)),
                (current.position, current.orientation),
                input.dt,
                t_go,
                tuning.max_extrapolate_s,
            );
            let wrist = Pose {
                position,
                orientation,
            };
            Some((hand_pose::mul(wrist, canonical), angular))
        });
        let distance = input.body.centre.distance(input.palm);
        let mut tick = LandingTick {
            grip,
            targets: targets.map(|t| t.map(|(pose, _)| pose)),
            angular: None,
            shift: Vec3::ZERO,
        };
        if distance > SNATCH.approach_m && self.start_m.is_none() {
            return tick;
        }
        let start = *self.start_m.get_or_insert(distance);
        self.ticks = self.ticks.saturating_add(1);
        let weight = landing::drive_weight(start, distance, self.ticks, tuning);
        let Some((target, hand_angular)) = targets[input.receiver.index()] else {
            return tick;
        };
        let target_body = hand_pose::mul(target, self.body_in_gun);
        let t_go = landing::time_to_go(grip, input.velocity, target.position, catch_reach, tuning);
        let drive = landing::turn_drive(
            Turning {
                orientation: input.body.orientation,
                angular: input.angular,
            },
            Turning {
                orientation: target_body.orientation,
                angular: hand_angular,
            },
            t_go,
            input.dt,
            tuning,
        );
        tick.angular = Some(input.angular.lerp(drive, weight));
        let line = (input.palm - input.body.centre)
            .try_normalize()
            .unwrap_or(Vec3::ZERO);
        tick.shift = landing::aim_shift(input.palm, target_body.position, line, weight, tuning);
        tick
    }
}

/// How far the gun at `gun` is from where `target` holds it: the angle between them, radians, and
/// the distance between their grips, metres.
pub(super) fn residual(gun_rotation: Quat, grip: Vec3, target: Pose) -> (f32, f32) {
    (
        gun_rotation.angle_between(target.orientation),
        grip.distance(target.position),
    )
}

/// The gun's rotation for a body at `body` with the landing's body frame.
pub(super) fn gun_rotation(landing: &Landing, body: BodyPose) -> Quat {
    (body.orientation * landing.body_in_gun.orientation.inverse()).normalize()
}

/// The watch over a snatched gun's first ticks in the catching hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct ArrivalWatch {
    weapon: usize,
    hand: Hand,
    since: Instant,
    /// Ticks the weapon's model was found hidden in the hand and shown again.
    shown: u32,
}

impl ArrivalWatch {
    pub(super) fn new(weapon: usize, hand: Hand) -> Self {
        Self {
            weapon,
            hand,
            since: Instant::now(),
            shown: 0,
        }
    }

    /// One tick of the watch. Returns whether it keeps watching.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(super) unsafe fn tick(&mut self) -> bool {
        let elapsed = self.since.elapsed();
        let in_hand = live::in_hand(self.hand) == Some(self.weapon);
        if elapsed > WATCH || !in_hand {
            if self.shown > 0 {
                tracing::info!(
                    target: "controllers",
                    hand = self.hand.label(),
                    ticks = self.shown,
                    "snatch: the caught weapon's model was hidden in the hand and shown again"
                );
            }
            return false;
        }
        // SAFETY: the table holds the weapon in the hand, which keeps it alive.
        let Some(weapon) = (unsafe { (self.weapon as *mut WeaponBase).as_mut() }) else {
            return false;
        };
        let drawn = unsafe { gun::guns(weapon) }.next().flatten().map(|first| {
            unsafe { first.m_BaseModelInstance.as_ref() }
                .is_some_and(|model| model.m_Flags.contains(ModelInstanceFlags::VISIBLE))
        });
        let placed = grip::status().hands[self.hand.index()]
            .filter(|g| g.weapon == self.weapon)
            .map(|g| g.pinned);
        let slide = act::snap_view(self.weapon, self.hand).map(|s| s.toward_hand);
        // Read as the raw value: a state the enum does not name is logged as it is.
        let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
        tracing::info!(
            target: "controllers",
            hand = self.hand.label(),
            elapsed_ms = elapsed.as_millis() as u64,
            state,
            drawn = ?drawn,
            pinned = ?placed,
            slide = ?slide,
            "snatch: the caught weapon in the hand"
        );
        if drawn == Some(false) {
            unsafe { weapon.SetVisible(true, false, true) };
            self.shown += 1;
        }
        true
    }
}

/// How long the watch follows a caught weapon.
const WATCH: Duration = Duration::from_millis(300);
