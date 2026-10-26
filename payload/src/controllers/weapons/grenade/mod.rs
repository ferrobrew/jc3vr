//! Independent safe grenades and native deliberate throws.
//!
//! A pouch draw creates only a model and a loadout item, independently of native ammunition.
//! Transfers, holster moves, and incidental releases keep that identity and stay actor-free.
//! Opening the grip during a tracked throwing motion requests the grenade weapon's existing
//! pool-backed creation and launch path. Native live grenades can still be caught and thrown on.

pub mod catch;
pub mod config;
mod locker;
mod native;
pub mod pin;
pub(crate) mod safe;
pub mod stage;

use std::{cell::Cell, time::Duration};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    equipment::{
        explosive::{Explosive, Grenade},
        weapon::{ThrownGrenadeWeapon, WeaponBase, WeaponInputs},
    },
    input::input_action_map::Action,
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        aim::targets::weak_target,
        bindings::{
            controls::ButtonControl,
            hold::{Desired, MapKind},
        },
        body::rig::frames::Frames,
        config::ControllersConfig,
        frame::{Hand, Pose},
        grab::{Candidate, GrabVolume},
        hands::{
            ActiveHands,
            held::{self, Held, Slot},
        },
        haptics::{self, Event},
        loadout::TakeSource,
        motion::{self, ReleaseEstimate},
        overlay::{
            primitive::{
                Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, Source, SpriteSize,
            },
            store,
        },
        physical_hands, scheme,
        weapons::{
            config::GrenadeConfig,
            docks, engine,
            grenade::{
                catch::{CatchCounts, Caught, Sighting},
                pin::{Pin, Puller},
                stage::{Outcome, Release, Stage},
            },
        },
    },
    headpose,
};

/// The grenade hooks, aggregated by the weapon controller.
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_FROM_USER_BINDER)
        .with_static_binder(&GRENADE_PROPEL_TOWARD_BINDER)
        .with_hook_library(native::hook_library())
        .with_hook_library(locker::hook_library())
}

/// Hook removal keeps the image mapped while a locker source callback remains active.
pub(crate) fn locker_active_entries() -> usize {
    locker::active_entries()
}

/// Observe a live world weapon during native placement or rendering for safe grenade sources.
///
/// # Safety
///
/// The caller runs the weapon's native pass and keeps it live for this call.
pub(crate) unsafe fn observe_world_weapon(weapon: &WeaponBase) {
    unsafe { locker::observe(weapon) };
}

/// The held live grenade's fuse.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Fuse {
    pub remaining_s: f32,
    pub total_s: f32,
}

/// One reported native throw step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrenadeEvent {
    pub hand: Option<Hand>,
    pub outcome: Outcome,
}

/// The grenade controller's readout. Safe held items have no live fuse.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrenadeStatus {
    pub stage: Stage,
    pub fuse: Option<Fuse>,
    pub last_release: Option<Vec3>,
    pub weapon_seen: bool,
    /// Whether another safe grenade may be drawn, independently of ammunition.
    pub in_stock: Option<bool>,
    pub primed: u64,
    pub thrown: u64,
    pub gone: u64,
    pub pin: Option<Pin>,
    pub pins_pulled: u64,
    pub last_puller: Option<Puller>,
    pub caught_hand: Option<Hand>,
    pub catch: CatchCounts,
    pub events: Vec<GrenadeEvent>,
}

/// The safe pouch does not depend on native grenade stock.
pub(crate) fn in_stock() -> bool {
    true
}

pub fn status() -> GrenadeStatus {
    let mut status = STATE.lock().status.clone();
    status.catch = catch::counts();
    status.caught_hand = catch::caught().map(|caught| caught.hand);
    status
}

/// Native caught grenades only. Actor-free items are already owned by the loadout table.
pub(crate) fn held() -> Option<(Hand, usize, TakeSource)> {
    catch::caught().map(|caught| (caught.hand, caught.explosive, TakeSource::Catch))
}

/// The hands whose native grenades launched since the previous read.
pub(crate) fn take_thrown() -> [bool; 2] {
    let pouch = std::mem::take(&mut STATE.lock().thrown);
    let caught = catch::take_thrown();
    Hand::BOTH.map(|hand| pouch[hand.index()] || caught[hand.index()])
}

/// Both hands can hold independent safe grenades.
pub(crate) fn holds(hand: Hand) -> bool {
    safe::in_hand(hand).is_some() || catch::caught().is_some_and(|caught| caught.hand == hand)
}

pub(super) struct BeginInputs<'a> {
    pub active: &'a ActiveHands,
    pub free: [bool; 2],
    pub acting: bool,
}

/// Offer inert loose models and native live grenades to free hands.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(super) unsafe fn begin_tick(inputs: &BeginInputs, tuning: &GrenadeConfig) -> Vec<Candidate> {
    {
        let mut state = STATE.lock();
        state.sightings.clear();
        state.locker_frames = None;
    }
    locker::clear();
    if !inputs.acting {
        return Vec::new();
    }
    let Some(player) =
        (unsafe { engine::local_player() }).map(|player| Mat4::from(player.m_WorldMatrixT1))
    else {
        return Vec::new();
    };
    let Some(frames) = docks::cockpit_frames(inputs.active, player) else {
        return Vec::new();
    };
    STATE.lock().locker_frames = Some(frames);
    let mut candidates = Vec::new();
    if let Some(owner) = unsafe { engine::local_player() } {
        candidates.extend(unsafe {
            locker::begin(
                owner,
                inputs.free,
                inputs
                    .active
                    .hands
                    .map(|hand| hand.map(|hand| hand.grip.position)),
                |point| frames.world_to_cockpit(point),
            )
        });
    }
    for world in safe::loose() {
        for hand in Hand::BOTH {
            if inputs.free[hand.index()] && !holds(hand) {
                candidates.push(Candidate {
                    volume: GrabVolume::LiveGrenade,
                    centre: frames.world_to_cockpit(world),
                    radius: tuning.catch.radius_m,
                    hand: Some(hand),
                });
            }
        }
    }
    if tuning.catch.enabled && catch::caught().is_none() {
        let seen = unsafe {
            catch::sightings(player.w_axis.truncate(), &tuning.catch, |point| {
                frames.world_to_cockpit(point)
            })
        };
        let free = Hand::BOTH.map(|hand| inputs.free[hand.index()] && !holds(hand));
        candidates.extend(catch::candidates(&seen, tuning.catch.radius_m, free));
        STATE.lock().sightings = seen;
    }
    candidates
}

pub(super) struct TickInputs {
    pub acting: bool,
    pub pouch_grab: Option<Hand>,
    pub locker_grabs: [bool; 2],
    pub pin_grab: Option<Hand>,
    pub catch_grab: Option<Hand>,
    pub cockpit_grips: [Option<Vec3>; 2],
    pub triggers: [ButtonControl; 2],
    pub dominant: Hand,
    pub opened: [bool; 2],
    pub world_scale: f32,
    pub dt: f32,
}

/// Consume the resolved pouch grab and grip opening in the actual input tick.
pub(super) fn input_tick(
    inputs: &TickInputs,
    config: &ControllersConfig,
    desired: &mut Vec<Desired>,
) {
    if !inputs.acting {
        stand_down();
        return;
    }
    let tuning = &config.weapons.grenade;
    let _ = (inputs.pin_grab, inputs.triggers, inputs.dominant);
    for hand in Hand::BOTH {
        if inputs.locker_grabs[hand.index()] && !holds(hand) {
            let frames = STATE.lock().locker_frames;
            let result = frames
                .ok_or(locker::Refusal::SourceChanged)
                .and_then(|frames| {
                    // SAFETY: the weapon input tick runs on the game thread outside the world update.
                    unsafe {
                        locker::take(
                            hand,
                            inputs.cockpit_grips[hand.index()],
                            |point| frames.world_to_cockpit(point),
                            tuning,
                        )
                    }
                });
            match result {
                Ok(()) => haptics::play(hand, Event::GrenadePrime),
                Err(error) => {
                    tracing::debug!(target: "controllers", hand = hand.label(), %error, "grenade: locker draw refused");
                    haptics::play(hand, Event::PouchRefused);
                }
            }
        }
    }
    if let Some(hand) = inputs.pouch_grab
        && !holds(hand)
    {
        // SAFETY: the weapon input tick runs on the game thread outside the world update.
        if unsafe { safe::draw(hand, tuning) } {
            haptics::play(hand, Event::GrenadePrime);
        } else {
            haptics::play(hand, Event::PouchRefused);
        }
    }
    if let Some(hand) = inputs.catch_grab
        && !holds(hand)
    {
        let world = STATE.lock().grips[hand.index()].map(|grip| grip.position);
        let took_safe =
            world.is_some_and(|world| safe::take_loose(hand, world, tuning.catch.radius_m));
        if !took_safe
            && catch::caught().is_none()
            && let Some(grip) = inputs.cockpit_grips[hand.index()]
        {
            let seen = STATE.lock().sightings.clone();
            catch_one(hand, grip, &seen, tuning);
        }
    }
    for hand in Hand::BOTH {
        if inputs.opened[hand.index()] {
            safe::opened(hand, inputs.world_scale, tuning);
        }
    }
    if Hand::BOTH.into_iter().any(holds) {
        desired.retain(|desired| {
            !(desired.target.map == MapKind::Player
                && desired.target.action == Action::THROW_GRENADE)
        });
    }
    // SAFETY: the input tick runs on the game thread outside the world update.
    let caught = unsafe { carry_caught(inputs, tuning) };
    let mut state = STATE.lock();
    state.status.fuse = caught.map(|(_, fuse)| fuse);
    state.status.in_stock = Some(true);
    let (mut primitives, pulse) =
        warning(&mut state, caught, inputs.dt, config).unwrap_or_default();
    drop(state);
    if primitives.is_empty() {
        store::clear(Source::GrenadeWarning);
    } else {
        store::submit(Source::GrenadeWarning, &mut primitives, WARNING_TTL);
    }
    if let Some(hand) = pulse {
        haptics::play(hand, Event::GrenadeWarning);
    }
}

/// A loss of input releases safe items inertly and leaves a caught grenade's fuse unchanged.
pub(super) fn stand_down() {
    locker::clear();
    safe::stand_down();
    let mut state = STATE.lock();
    state.next_pulse_s = 0.0;
    state.sightings.clear();
    state.locker_frames = None;
    state.status.fuse = None;
    drop(state);
    if catch::caught().is_some() {
        catch::let_go(false);
    }
    store::clear(Source::GrenadeWarning);
}

/// Note tracked controller world poses for safe rendering and live catches.
pub(crate) fn observe_hands(grips: [Option<Pose>; 2]) {
    STATE.lock().grips = grips;
    safe::observe(
        grips,
        &Config::lock_query(|config| config.controllers.weapons.grenade),
    );
}

/// Draw actor-free grenade models before the engine culls its render instances.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(crate) unsafe fn render_frame(dtf: f32) {
    unsafe { safe::render_frame(dtf) };
}

pub(crate) fn fallback_target(
    config: &ControllersConfig,
    dominant_target: Option<Vec3>,
) -> Option<Vec3> {
    dominant_target
        .filter(|_| config.weapons.grenade_throw && config.weapons.grenade.hand_aimed_fallback)
}

pub(super) fn report_native_throw(hand: Hand, release: Release) {
    let mut state = STATE.lock();
    state.status.primed += 1;
    state.status.thrown += 1;
    state.status.last_release = Some(release.velocity);
    state.thrown[hand.index()] = true;
    state.status.events.push(GrenadeEvent {
        hand: Some(hand),
        outcome: Outcome::Thrown(release),
    });
    let excess = state.status.events.len().saturating_sub(MAX_EVENTS);
    state.status.events.drain(..excess);
}

const WARNING_TTL: Duration = Duration::from_millis(150);
const MAX_EVENTS: usize = 6;
const WARNING_CALM: Rgba = [0.4, 1.0, 0.4, 0.8];
const WARNING_URGENT: Rgba = [1.0, 0.15, 0.1, 0.95];

struct State {
    thrown: [bool; 2],
    sightings: Vec<Sighting>,
    locker_frames: Option<Frames>,
    status: GrenadeStatus,
    grips: [Option<Pose>; 2],
    next_pulse_s: f32,
}

static STATE: Mutex<State> = Mutex::new(State {
    thrown: [false; 2],
    sightings: Vec::new(),
    locker_frames: None,
    status: GrenadeStatus {
        stage: Stage::Idle,
        fuse: None,
        last_release: None,
        weapon_seen: false,
        in_stock: None,
        primed: 0,
        thrown: 0,
        gone: 0,
        pin: None,
        pins_pulled: 0,
        last_puller: None,
        caught_hand: None,
        catch: CatchCounts {
            tracked: 0,
            in_reach: 0,
            caught: 0,
            thrown: 0,
            lost: 0,
        },
        events: Vec::new(),
    },
    grips: [None; 2],
    next_pulse_s: 0.0,
});

#[derive(Copy, Clone)]
struct PlannedLaunch {
    explosive: usize,
    transform: Option<Matrix4>,
    velocity: Vec3,
}

thread_local! {
    static LAUNCH: Cell<Option<PlannedLaunch>> = const { Cell::new(None) };
}

fn release(hand: Hand, world_scale: f32, tuning: &GrenadeConfig) -> Option<Release> {
    let samples = motion::history(hand);
    let now = motion::latest_time()?;
    let velocity = motion::release_velocity(
        &samples,
        now,
        hold_offset(hand, tuning),
        ReleaseEstimate {
            window_s: tuning.release_window_s,
            peak_span_s: tuning.release_peak_span_s,
        },
    )?;
    let world = headpose::xr::body_rotation() * velocity * world_scale * tuning.velocity_scale;
    Some(Release {
        velocity: world.clamp_length_max(tuning.max_speed_mps.max(0.0)),
    })
}

fn hold_offset(hand: Hand, tuning: &GrenadeConfig) -> Vec3 {
    let offset = tuning.hold_offset_m;
    match hand {
        Hand::Left => Vec3::new(-offset.x, offset.y, offset.z),
        Hand::Right => offset,
    }
}

fn warning(
    state: &mut State,
    caught: Option<(Hand, Fuse)>,
    dt: f32,
    config: &ControllersConfig,
) -> Option<(Vec<Primitive>, Option<Hand>)> {
    let tuning = &config.weapons.grenade;
    let Some((hand, fuse)) = caught else {
        state.next_pulse_s = 0.0;
        return None;
    };
    let urgency = fuse_urgency(fuse.remaining_s, tuning.warning_s);
    let mut primitives = Vec::new();
    if tuning.warning_overlay && config.overlay.enabled {
        primitives.push(Primitive::Sprite {
            at: Anchor::Hand {
                hand,
                pose: HandPose::Grip,
                offset: hold_offset(hand, tuning),
            },
            size: SpriteSize::World(0.07 + 0.03 * urgency),
            facing: Facing::Billboard,
            shape: Shape::Ring {
                thickness: 0.2 + 0.3 * urgency,
            },
            color: std::array::from_fn(|index| {
                WARNING_CALM[index] + (WARNING_URGENT[index] - WARNING_CALM[index]) * urgency
            }),
            depth: DepthMode::XRay {
                occluded_alpha: 0.6,
            },
        });
    }
    let mut pulse = None;
    if tuning.warning_haptics && urgency > 0.0 {
        state.next_pulse_s -= dt;
        if state.next_pulse_s <= 0.0 {
            let [slow, fast] = tuning.warning_interval_s;
            state.next_pulse_s = slow + (fast - slow) * urgency;
            pulse = Some(hand);
        }
    } else {
        state.next_pulse_s = 0.0;
    }
    Some((primitives, pulse))
}

pub fn fuse_urgency(remaining_s: f32, warning_s: f32) -> f32 {
    if warning_s.is_nan() || warning_s <= 0.0 || !remaining_s.is_finite() {
        return 0.0;
    }
    (1.0 - remaining_s / warning_s).clamp(0.0, 1.0)
}

#[detour(address = jc3gi::equipment::weapon::ThrownGrenadeWeapon::UpdateFromUserPostSim_Parallel_ADDRESS)]
fn update_from_user(
    weapon: *mut ThrownGrenadeWeapon,
    dt: f32,
    aim_position: *const Vector3,
    input: *const WeaponInputs,
) {
    UPDATE_FROM_USER
        .get()
        .unwrap()
        .call(weapon, dt, aim_position, input);
    // SAFETY: the grenade weapon is live for its own update.
    let Some(base) = (unsafe { weapon.cast::<WeaponBase>().as_ref() }) else {
        return;
    };
    if !base.m_IsUserLocalPlayer {
        return;
    }
    STATE.lock().status.weapon_seen = true;
    let acting = Config::lock_query(|config| {
        config.controllers.enabled && config.controllers.weapons.grenade_throw
    }) && scheme::is_controllers();
    if !acting {
        native::cancel();
    }
    // SAFETY: the local grenade weapon is live on its update thread.
    unsafe { native::update(&mut *weapon, dt) };
}

#[detour(address = jc3gi::equipment::explosive::Grenade::PropelToward_ADDRESS)]
fn grenade_propel_toward(
    grenade: *mut Grenade,
    target_position: *const Vector3,
    relative_velocity: *const Vector3,
    target_is_on_roof: bool,
    exact_target_aiming: bool,
) {
    GRENADE_PROPEL_TOWARD.get().unwrap().call(
        grenade,
        target_position,
        relative_velocity,
        target_is_on_roof,
        exact_target_aiming,
    );
    let launch = LAUNCH
        .get()
        .filter(|launch| launch.explosive == grenade as usize);
    // SAFETY: the grenade is live for its launch.
    unsafe { catch::register(grenade.cast::<Explosive>(), launch.is_some()) };
    let Some(launch) = launch else {
        return;
    };
    LAUNCH.set(None);
    // SAFETY: a grenade is an explosive at the same address.
    let Some(explosive) = (unsafe { grenade.cast::<Explosive>().as_mut() }) else {
        return;
    };
    if let Some(transform) = launch.transform {
        unsafe { explosive.SetPhysicsTransform(&transform) };
    }
    let carried = Vec3::from_array(explosive.m_RelativeVelocity.data);
    let velocity = launch.velocity
        + if carried.is_finite() {
            carried
        } else {
            Vec3::ZERO
        };
    explosive.m_DeferredInitialVelocity = Vector3 {
        data: velocity.to_array(),
    };
}

unsafe fn primed_explosive(weapon: &ThrownGrenadeWeapon) -> Option<*mut Explosive> {
    let object = unsafe { weak_target(&weapon.m_PrimedGrenade) }?;
    Some(
        object
            .cast::<u8>()
            .wrapping_sub(Explosive::GAME_OBJECT_BASE_OFFSET as usize)
            .cast::<Explosive>(),
    )
}

fn catch_one(hand: Hand, grip: Vec3, seen: &[Sighting], tuning: &GrenadeConfig) {
    if let Some(sighting) = catch::nearest(seen, grip, tuning.catch.radius_m) {
        catch::set_caught(Some(Caught {
            hand,
            explosive: sighting.explosive,
            body: sighting.body,
            owner: sighting.owner,
        }));
        haptics::play(hand, Event::GrenadeCatch);
    }
}

unsafe fn carry_caught(inputs: &TickInputs, tuning: &GrenadeConfig) -> Option<(Hand, Fuse)> {
    let caught = catch::caught()?;
    let hand = caught.hand;
    let Ok((centre, _)) = (unsafe { physical_hands::body_pose(caught.body, caught.owner) }) else {
        catch::let_go(false);
        return None;
    };
    if inputs.opened[hand.index()] {
        let relative = release(hand, inputs.world_scale, tuning)
            .map_or(Vec3::ZERO, |release| release.velocity);
        let carried = unsafe { engine::local_player() }
            .map(|player| Vec3::from_array(player.m_MeasuredWorldVelocity.data))
            .filter(|velocity| velocity.is_finite())
            .unwrap_or(Vec3::ZERO);
        let _ = unsafe {
            physical_hands::set_body_velocities(
                caught.body,
                caught.owner,
                relative + carried,
                Vec3::ZERO,
            )
        };
        catch::let_go(true);
        STATE.lock().status.last_release = Some(relative);
        return None;
    }
    let world_grip = STATE.lock().grips[hand.index()];
    held::hold(
        Slot::CaughtGrenade,
        Some(Held {
            explosive: caught.explosive,
            hand,
            offset: hold_offset(hand, tuning),
        }),
    );
    if let Some(grip) = world_grip {
        let target = grip.position + grip.orientation * hold_offset(hand, tuning);
        let velocity = catch::carry_velocity(centre, target, inputs.dt, tuning.catch.max_carry_mps);
        let _ = unsafe {
            physical_hands::set_body_velocities(caught.body, caught.owner, velocity, Vec3::ZERO)
        };
    }
    // SAFETY: body_pose proved the native grenade live outside the world update.
    let explosive = unsafe { &*(caught.explosive as *const Explosive) };
    Some((
        hand,
        Fuse {
            remaining_s: explosive.m_DetonationTimer,
            total_s: explosive.m_StartDetonationTimer,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fuse_warns_only_in_its_last_seconds() {
        assert_eq!(fuse_urgency(4.0, 2.0), 0.0);
        assert_eq!(fuse_urgency(2.0, 2.0), 0.0);
        assert!((fuse_urgency(1.0, 2.0) - 0.5).abs() < 1e-6);
        assert_eq!(fuse_urgency(-0.1, 2.0), 1.0);
        assert_eq!(fuse_urgency(1.0, 0.0), 0.0);
        assert_eq!(fuse_urgency(f32::NAN, 2.0), 0.0);
    }

    #[test]
    fn the_hold_offset_mirrors_for_the_left_hand() {
        let tuning = GrenadeConfig {
            hold_offset_m: Vec3::new(0.02, -0.01, -0.03),
            ..GrenadeConfig::new()
        };
        assert_eq!(hold_offset(Hand::Right, &tuning), tuning.hold_offset_m);
        assert_eq!(
            hold_offset(Hand::Left, &tuning),
            Vec3::new(-0.02, -0.01, -0.03)
        );
    }
}
