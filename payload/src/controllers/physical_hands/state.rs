//! The palms' state across ticks: each hand's probes, its pressed and held state, and its motion,
//! with the stand-down that forgets it all when the palms stop acting.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

use glam::{Vec2, Vec3};
use parking_lot::Mutex;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::controllers::{
    frame::Hand,
    haptics::{self, Event},
    motion,
    physical_hands::{
        climb,
        contact::PointProbe,
        draw,
        grasp::{AngularTracker, VelocityTracker},
        palm::PROBE_POINTS,
        physics,
        pickup::{self, PickupState, PickupStatus},
        shove::Give,
        status::{HandStatus, Idle, PhysicalHandsStatus},
    },
};

/// Note the input tick's thread, the main thread. Called from the input tick.
pub(crate) fn input_tick() {
    // SAFETY: reads the calling thread's own id.
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

/// Stop: forget the probes and the push, and say why.
pub(super) fn stand_down(idle: Idle) {
    let mut state = STATE.lock();
    if idle == Idle::OffThread {
        state.status.off_thread += 1;
    }
    state.reset_probes();
    state.pickup.drop_all();
    physics::publish_holds([None; 2]);
    physics::publish_pins([None; 2]);

    state.last_sample = [None; 2];
    state.clamp = [Vec3::ZERO; 2];
    state.at = None;
    state.status.idle = Some(idle);
    state.status.hands = [HandStatus::default(); 2];
    state.status.push = Vec2::ZERO;
    state.status.casts = 0;
    drop(state);
    climb::stand_down();
    draw::clear();
}

pub(super) struct State {
    pub(super) status: PhysicalHandsStatus,
    pub(super) probes: [[PointProbe; PROBE_POINTS]; 2],
    /// Whether each palm pressed into a surface on the last tick.
    pub(super) pressed: [bool; 2],
    /// Each hand's latest motion sample time read.
    pub(super) last_sample: [Option<f64>; 2],
    /// Each hand's clamp offset, cockpit frame.
    pub(super) clamp: [Vec3; 2],
    /// Each palm's motion across ticks, world space.
    pub(super) motion: [(VelocityTracker, AngularTracker); 2],
    /// How much each palm's pressed object gives.
    pub(super) gives: [Give; 2],
    /// When the last tick ran.
    pub(super) at: Option<Instant>,
    pub(super) epoch: u64,
    pub(super) pickup: PickupState,
    /// What each hand's reach found on the last tick, for the grip press's log.
    pub(super) reach_notes: [Vec<String>; 2],
}

impl State {
    pub(super) fn fresh(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    pub(super) fn reset_probes(&mut self) {
        for probes in &mut self.probes {
            probes.iter_mut().for_each(PointProbe::reset);
        }
        self.pressed = [false; 2];
        self.gives = [Give::new(); 2];
        self.motion = [(VelocityTracker::new(), AngularTracker::new()); 2];
    }

    /// Fold a pickup tick into the readout, and pulse the hands that took hold or let go.
    pub(super) fn record_pickup(
        &mut self,
        outcome: pickup::TickOutcome,
        hand_status: &mut [HandStatus; 2],
    ) {
        let status = &mut self.status.pickup;
        for hand in Hand::BOTH {
            let i = hand.index();
            hand_status[i].held = outcome.reports[i];
            if outcome.taken[i] {
                status.holds += 1;
                haptics::play(hand, Event::Take);
            }
            if outcome.released[i] {
                status.releases += 1;
                haptics::play(hand, Event::LetGo);
            }
        }
        for hand in Hand::BOTH {
            if outcome.unlatched[hand.index()] {
                status.unlatches += 1;
                haptics::play(hand, Event::Unlatch);
            }
        }
        status.door_grips += outcome.door_grips;
        status.tears += outcome.tears;
        status.tear_refusals += outcome.tear_refusals;
        status.character_grips += outcome.character_grips;
        status.hijacks += outcome.hijacks;
        status.yanks += outcome.yanks;
        status.articulated_grips += outcome.articulated_grips;
        status.regrips += outcome.regrips;
        if outcome.door.is_some() {
            status.last_door = outcome.door;
        }
        if let Some(speed) = outcome.throw {
            status.last_throw_mps = Some(speed);
        }
        status.lost += outcome.lost;
        if let Some(error) = outcome.error {
            status.last_error = Some(error);
        }
    }

    /// The hand's fastest velocity since the last tick, cockpit frame, metres per second.
    pub(super) fn hand_velocity(&mut self, hand: Hand) -> Vec3 {
        let samples = motion::history(hand);
        let since = self.last_sample[hand.index()];
        self.last_sample[hand.index()] = samples.last().map(|s| s.time_s).or(since);
        since
            .and_then(|since| motion::peak_velocity(&samples, since))
            .map_or(Vec3::ZERO, |(_, v)| v)
    }
}

/// How long a tick's results stay current: the aim update runs every simulation tick while the
/// palms act, so an older result means they have stopped.
pub(super) const STALE_AFTER: Duration = Duration::from_millis(200);

/// How long the palms' velocities are smoothed over, seconds: short, so that a throw keeps the
/// flick.
pub(super) const VELOCITY_SMOOTHING_S: f32 = 0.03;

/// The main thread's id, as the input tick last saw it; zero before the first.
pub(super) static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    status: PhysicalHandsStatus {
        human_hull: crate::controllers::physical_hands::human_hull::HumanHullStatus::UNAVAILABLE,
        idle: None,
        hands: [HandStatus {
            acting: false,
            palm: None,
            press: None,
            push: Vec2::ZERO,
            speed: 0.0,
            reach: None,
            held: None,
            shove: None,
            latched: false,
        }; 2],
        push: Vec2::ZERO,
        casts: 0,
        object_pushes: 0,
        shoves: 0,
        off_thread: 0,
        last_error: None,
        pickup: PickupStatus {
            holds: 0,
            weapons: 0,
            releases: 0,
            last_throw_mps: None,
            lost: 0,
            door_grips: 0,
            unlatches: 0,
            tears: 0,
            tear_refusals: 0,
            last_door: None,
            character_grips: 0,
            hijacks: 0,
            articulated_grips: 0,
            yanks: 0,
            regrips: 0,
            last_error: None,
        },
    },
    probes: [[PointProbe::new(); PROBE_POINTS]; 2],
    pressed: [false; 2],
    last_sample: [None; 2],
    clamp: [Vec3::ZERO; 2],
    motion: [(VelocityTracker::new(), AngularTracker::new()); 2],
    gives: [Give::new(); 2],
    at: None,
    epoch: 0,
    pickup: PickupState::new(),
    reach_notes: [Vec::new(), Vec::new()],
});
