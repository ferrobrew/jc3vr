//! The hand bodies' shared state between the two tick halves: each hand's published palm and the
//! last step's results, the readout, and the small readers both halves and the rest of the
//! controllers take.

use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use glam::{Quat, Vec2, Vec3};
use parking_lot::Mutex;

use crate::{
    controllers::{
        frame::Hand,
        hands::{self, pose as hand_pose},
        mode,
        physical_hands::{
            PhysicalHandsConfig, STATE, free_hands,
            grasp::{AngularTracker, VelocityTracker},
            mode_acts,
            palm::PalmBox,
            physics::{
                callback,
                config::HandBodiesConfig,
                contact,
                drive::{DriveTuning, Goal},
                guns,
                render::RenderOffset,
                types::{BodyPhase, CallbackStatus, HandBodiesStatus, HandBodyStatus, HoldTarget},
            },
        },
        scheme,
    },
    headpose, hooks,
};

pub fn status() -> HandBodiesStatus {
    SHARED.lock().status.clone()
}

pub fn callback_status() -> CallbackStatus {
    CallbackStatus {
        registered: callback::registered(),
        fires: callback::fires(),
        game_thread: callback::registering_thread(),
        firing_thread: callback::firing_thread(),
        listening: Hand::BOTH.map(contact::listening),
    }
}

/// Whether the hand bodies are on: their switch, under the physical hands' master switch.
pub(crate) fn enabled(config: &PhysicalHandsConfig) -> bool {
    config.enabled && config.bodies.enabled
}

/// Publish what each hand's grip holds through a joint, for the next step to join. Called from the
/// player aim control's post-simulation update.
pub(crate) fn publish_holds(holds: [Option<HoldTarget>; 2]) {
    SHARED.lock().holds = holds;
}

/// Publish where each hand that grips something it cannot move yet is held, world space: a latched
/// door's grab point. The next step holds the hand's body there instead of driving it to its
/// controller. Called from the player aim control's post-simulation update.
pub(crate) fn publish_pins(pins: [Option<Vec3>; 2]) {
    SHARED.lock().pins = pins;
}

/// Take which hands lost the joint their grip held since the last call: the joint could not be
/// made, the object's body went away, or the hand's body stopped.
pub(crate) fn take_joint_losses() -> [bool; 2] {
    std::mem::take(&mut SHARED.lock().joint_lost)
}

/// Whether `hand` is driven by its body: the hand acts, and the last step drove its body. The
/// palm's probes leave such a hand alone, and take it back as soon as its body stops.
pub(crate) fn drives(hand: Hand) -> bool {
    let shared = SHARED.lock();
    let now = Instant::now();
    shared.fresh(now)
        && shared.stepped_lately(now)
        && shared.targets[hand.index()].is_some()
        && shared.status.hands[hand.index()].phase == BodyPhase::Acting
}

/// Where `hand`'s body was before the last step, game space, while it acts: the place its reach
/// is cast from, so that what the hand reaches is what its body touches.
pub(crate) fn body_pose(hand: Hand) -> Option<(Vec3, Quat)> {
    let shared = SHARED.lock();
    let now = Instant::now();
    (shared.fresh(now) && shared.stepped_lately(now))
        .then_some(shared.body_poses[hand.index()])
        .flatten()
}

/// Whether `hand`'s body presses into something that holds it back, as the last step found it.
pub(crate) fn pressing(hand: Hand) -> bool {
    let shared = SHARED.lock();
    let now = Instant::now();
    shared.fresh(now)
        && shared.stepped_lately(now)
        && shared.loads[hand.index()] >= MIN_PRESSURE_LOAD
}

/// The push the hand bodies give the player, world `(x, z)` metres per second: zero when no step
/// has run lately.
pub(crate) fn push() -> Vec2 {
    let shared = SHARED.lock();
    let now = Instant::now();
    if shared.stepped_lately(now) && shared.fresh(now) {
        shared.push
    } else {
        Vec2::ZERO
    }
}

/// How long a published pose stays current: the aim update runs every simulation tick while the
/// hands act, so an older one means they have stopped.
pub(super) const STALE_AFTER: Duration = Duration::from_millis(200);

/// How long a hand may stop acting before its body leaves the simulation, so that a moment's lost
/// tracking or a flicker of the mode does not churn the world. A hand that takes a gun or carries
/// what it grabbed leaves at once.
pub(super) const PARK_AFTER: Duration = Duration::from_millis(250);

/// The longest a published pose is carried forward by its velocity, seconds.
pub(super) const MAX_LEAD_S: f32 = 0.1;

/// How long the palms' velocities are smoothed over, seconds.
pub(super) const VELOCITY_SMOOTHING_S: f32 = 0.03;

/// The turn between a body and its controller's palm that is not drawn, radians: a free hand keeps
/// the controller's own rotation.
pub(super) const RENDER_TURN_DEADBAND_RAD: f32 = 0.05;

/// The largest turn a hand is drawn at against its controller, radians.
pub(super) const RENDER_MAX_TURN_RAD: f32 = 1.5;

/// The least share of a hand's strength that pulses it.
pub(super) const MIN_PRESSURE_LOAD: f32 = 0.1;

/// How often the drive is written to the debug log while a hand acts.
pub(super) const READOUT_PERIOD: Duration = Duration::from_secs(1);

pub(super) const TRACKERS_AT_REST: [(VelocityTracker, AngularTracker); 2] =
    [(VelocityTracker::new(), AngularTracker::new()); 2];

pub(super) static CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Where a hand's palm is, world space, and how it moves.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Target {
    pub(super) position: Vec3,
    pub(super) orientation: Quat,
    pub(super) linear: Vec3,
    pub(super) angular: Vec3,
}

impl Target {
    pub(super) fn goal(&self) -> Goal {
        Goal {
            position: self.position,
            orientation: self.orientation,
        }
    }
}

/// What the aim update and the step share.
pub(super) struct Shared {
    /// Each hand's palm, as last published; `None` for a hand that does not act.
    pub(super) targets: [Option<Target>; 2],
    /// Which hands stopped acting on purpose, and park at once, and when that was published.
    pub(super) park_now: Option<(Instant, [bool; 2])>,
    /// When the targets were published; `None` while no hand acts.
    pub(super) published: Option<Instant>,
    pub(super) world_scale: f32,
    pub(super) trackers: [(VelocityTracker, AngularTracker); 2],
    pub(super) epoch: u64,
    /// The body's facing and the recenter count of the last palms' tick.
    pub(super) rig: Option<(Quat, u32)>,
    /// Each palm's move with the rig since the last step, from and to, world space.
    pub(super) rig_moves: [Option<(Goal, Goal)>; 2],
    /// Each body against its controller's palm, world space.
    pub(super) world_offsets: [RenderOffset; 2],
    /// The same in the cockpit frame, for drawing the hands.
    pub(super) cockpit_offsets: [RenderOffset; 2],
    /// The share of each hand's strength the last step used.
    pub(super) loads: [f32; 2],
    /// Each acting body's pose before the last step, game space.
    pub(super) body_poses: [Option<(Vec3, Quat)>; 2],
    /// The smoothed push, world `(x, z)` metres per second.
    pub(super) push: Vec2,
    /// What each hand's grip holds through a joint, as last published.
    pub(super) holds: [Option<HoldTarget>; 2],
    /// Where each hand that grips what it cannot move yet is held, as last published.
    pub(super) pins: [Option<Vec3>; 2],
    /// Which hands lost their joint since the aim update last took it.
    pub(super) joint_lost: [bool; 2],
    /// Until when each hand follows through a hard blow with a softened drive.
    pub(super) follow_through: [Option<Instant>; 2],
    /// When the last step ran.
    pub(super) stepped: Option<Instant>,
    pub(super) status: HandBodiesStatus,
}

impl Shared {
    pub(super) fn fresh(&self, now: Instant) -> bool {
        self.published
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    pub(super) fn stepped_lately(&self, now: Instant) -> bool {
        self.stepped
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }
}

pub(super) static SHARED: Mutex<Shared> = Mutex::new(Shared {
    targets: [None; 2],
    park_now: None,
    published: None,
    world_scale: 1.0,
    trackers: TRACKERS_AT_REST,
    epoch: 0,
    rig: None,
    rig_moves: [None; 2],
    world_offsets: [RenderOffset::NONE; 2],
    cockpit_offsets: [RenderOffset::NONE; 2],
    loads: [0.0; 2],
    body_poses: [None; 2],
    push: Vec2::ZERO,
    holds: [None; 2],
    pins: [None; 2],
    joint_lost: [false; 2],

    follow_through: [None; 2],
    stepped: None,
    status: HandBodiesStatus {
        registered: false,
        steps: 0,
        system_group: None,
        hands: [HandBodyStatus {
            phase: BodyPhase::Absent,
            body_id: None,
            filter_info: None,
            lag_m: 0.0,
            speed_mps: 0.0,
            force_share: 0.0,
            torque_share: 0.0,
            resets: 0,
            joint: None,
        }; 2],
        push: Vec2::ZERO,
        created: 0,
        destroyed: 0,
        contacts: 0,
        dropped_contacts: 0,
        blows: 0,
        last_blow: None,
        shoves: 0,
        knockdowns: 0,
        last_shove: None,
        last_error: None,
    },
});

/// The hands' palms when they act, with the VR world scale: on foot on the ground or falling, for
/// a tracked hand the controllers drive that holds nothing, or that holds or braces a gun whose
/// body it holds ([`guns::acting_hands`]).
pub(super) fn acting_palms(config: &PhysicalHandsConfig) -> Option<ActingPalms> {
    if !scheme::is_controllers() || !hooks::in_gameplay() {
        return None;
    }
    if !mode_acts(mode::current()) {
        return None;
    }
    let active = hands::active_hands_raw()?;
    let head = headpose::query();
    let body_rotation = headpose::xr::body_rotation();
    let free = free_hands();
    // A hand that carries what it holds, rather than holding it through a joint, has no body, so
    // that it never fights the object.
    let carrying = {
        let state = STATE.lock();
        Hand::BOTH.map(|hand| state.pickup.carrying(hand))
    };
    let on_gun = guns::acting_hands(config);
    let palms = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let cockpit = active.hands[i].filter(|_| (free[i] && !carrying[i]) || on_gun[i])?;
        let wrist = hand_pose::compose_world(
            head.position,
            body_rotation,
            active.head.position,
            cockpit.wrist,
            active.world_scale,
        );
        Some(PalmBox::from_wrist(wrist, active.world_scale, config))
    });
    Some(ActingPalms {
        palms,
        world_scale: active.world_scale,
        // A tracked hand that takes a gun without a body or carries what it grabbed has stopped
        // acting on purpose: its body leaves at once rather than standing frozen in the world.
        park_now: Hand::BOTH.map(|hand| {
            let i = hand.index();
            active.hands[i].is_some() && !on_gun[i] && (!free[i] || carrying[i])
        }),
    })
}

/// The palms' tick's view of the hands.
pub(super) struct ActingPalms {
    /// Each acting hand's palm, world space.
    pub(super) palms: [Option<PalmBox>; 2],
    pub(super) world_scale: f32,
    /// Which hands stopped acting on purpose, and park at once.
    pub(super) park_now: [bool; 2],
}

/// The drive of a hand that holds an object through a joint: a holding hand's pull and twist
/// ([`PickupConfig::hand_force_n`], [`PickupConfig::hand_torque_nm`]), no stronger than the hand's
/// own, and with both hands on the object, the twist at
/// [`PickupConfig::two_hand_angular_share`], so one hand lifts about what it carries and two hands
/// twice that, and the two grab points set the object's line.
///
/// [`PickupConfig::hand_force_n`]: crate::controllers::physical_hands::config::PickupConfig::hand_force_n
/// [`PickupConfig::hand_torque_nm`]: crate::controllers::physical_hands::config::PickupConfig::hand_torque_nm
/// [`PickupConfig::two_hand_angular_share`]: crate::controllers::physical_hands::config::PickupConfig::two_hand_angular_share
pub(super) fn holding_tuning(
    tuning: &DriveTuning,
    config: &PhysicalHandsConfig,
    shared: bool,
) -> DriveTuning {
    let pickup = &config.pickup;
    let twist_share = if shared {
        pickup.two_hand_angular_share.clamp(0.0, 1.0)
    } else {
        1.0
    };
    DriveTuning {
        max_force_n: tuning.max_force_n.min(pickup.hand_force_n.max(0.0)),
        max_torque_nm: tuning
            .max_torque_nm
            .min(pickup.hand_torque_nm.max(0.0) * twist_share),
        ..*tuning
    }
}

/// The drive of a hand on a gun: the hand's full strength, unlike a hand lifting a loose object,
/// against the hand's and the gun's mass together (`gun_kg`, the gun body's mass when the hand
/// holds one). The gun body moves with the hand's body as one rigid body, so the gun stays where
/// the hand holds it, and its mass shows only where the strength runs out: a slight lag in a fast
/// swing.
pub(super) fn gun_tuning(tuning: &DriveTuning, gun_kg: Option<f32>) -> DriveTuning {
    let gun_kg = gun_kg.filter(|m| m.is_finite() && *m > 0.0).unwrap_or(0.0);
    DriveTuning {
        mass_kg: tuning.mass_kg.max(0.0) + gun_kg,
        ..*tuning
    }
}

/// How hard a hand presses into what holds it back, `0..=1`: its lag past the push deadband, as a
/// share of the lag that presses fully.
pub(super) fn press_load(lag: Vec3, config: &HandBodiesConfig, scale: f32) -> f32 {
    let full = config.pressure_full_lag_m * scale;
    if full <= 0.0 || !lag.is_finite() {
        return 0.0;
    }
    ((lag.length() - config.push_deadband_m * scale) / full).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_on_a_gun_drives_the_hand_and_the_gun_with_its_full_strength() {
        let tuning = DriveTuning {
            mass_kg: 3.0,
            max_force_n: 5000.0,
            max_torque_nm: 400.0,
            linear_gain: 1.0,
            angular_gain: 0.8,
            max_speed_mps: 20.0,
            max_angular_speed_rps: 30.0,
        };
        let on_gun = gun_tuning(&tuning, Some(4.0));
        assert_eq!(on_gun.mass_kg, 7.0);
        assert_eq!(on_gun.max_force_n, tuning.max_force_n);
        assert_eq!(on_gun.max_torque_nm, tuning.max_torque_nm);
        assert_eq!(gun_tuning(&tuning, None), tuning);
        assert_eq!(gun_tuning(&tuning, Some(f32::NAN)), tuning);
    }
}
