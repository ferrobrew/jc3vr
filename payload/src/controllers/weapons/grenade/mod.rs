//! The physical grenade throw: the plan of record's "Grenades"
//! (`docs/mod/input/controllers-and-roomscale.md`), with the engine side on
//! [`ThrownGrenadeWeapon`] and [`Explosive`].
//!
//! A grip press at the grenade pouch primes a grenade in whichever hand grabbed it, and opening the
//! grip throws it with that hand's velocity at the release. The game's own grenade weapon does the
//! work,
//! driven through its two requests instead of the throw animation that normally raises them: its
//! prime request ([`ThrownGrenadeWeapon::m_PrimeGrenade`]), which spawns the grenade live and starts
//! its fuse, and its fire request ([`WeaponBase::m_ForceFireProjectile`]), which launches it. The
//! game's fuse applies: a grenade held too long explodes in the hand, as a real one would, and the
//! overlay and the haptics warn as the fuse runs down. The stages are in [`stage`].
//!
//! - On the input tick ([`input_tick`]), the grab and the grip's release are folded into the stage.
//!   The release velocity is estimated from the hand's recent motion
//!   ([`crate::controllers::motion::release_velocity`]) and turned into world axes. The fuse
//!   warning is drawn and pulsed. While a throw is under way the game's own throw input is held
//!   back, so that its wind-up cannot take the held grenade.
//! - On the aim update ([`observe_hands`]), the controllers' grips in the world are noted for the
//!   weapon's update, which follows on the same thread.
//! - Before the weapon's update, the stage asks it to prime, to fire, or to put the grenade back.
//!   The fire request is set only once the grenade's physics body exists, one update after the
//!   prime.
//! - While the grenade is held, the weapon's attachment update places it at the hand instead of the
//!   grenade weapon's bone.
//! - After the game solves the launch ([`Grenade::PropelToward`]), the launch is moved to the hand
//!   and its velocity replaced by the hand's, plus the character's velocity the game recorded.
//!
//! The stick-click fallback keeps the game's own throw and arc; with
//! [`GrenadeConfig::hand_aimed_fallback`] its target is the point the dominant hand's ray reaches
//! ([`fallback_target`]) rather than the gaze's.
//!
//! [`WeaponBase::m_ForceFireProjectile`]: jc3gi::equipment::weapon::WeaponBase::m_ForceFireProjectile

pub mod stage;

use std::{cell::Cell, time::Duration};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
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
        config::ControllersConfig,
        frame::{Hand, Pose},
        motion::{self, ReleaseEstimate},
        overlay::{
            primitive::{
                Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, Source, SpriteSize,
            },
            store,
        },
        scheme,
        weapons::{
            config::GrenadeConfig,
            grenade::stage::{
                InputEvent, Outcome, Refusal, Release, Stage, WeaponAction, WeaponView,
            },
            haptics::{self, PulseSource},
        },
    },
    headpose,
};

/// The grenade throw's hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_FROM_USER_BINDER)
        .with_static_binder(&UPDATE_ATTACHMENT_TRANSFORMS_BINDER)
        .with_static_binder(&GRENADE_PROPEL_TOWARD_BINDER)
}

/// The held grenade's fuse, as the weapon's last update read it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Fuse {
    /// Seconds until it detonates.
    pub remaining_s: f32,
    /// The whole fuse, seconds.
    pub total_s: f32,
}

/// One reported step of a throw, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrenadeEvent {
    pub hand: Option<Hand>,
    pub outcome: Outcome,
}

/// The throw's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrenadeStatus {
    pub stage: Stage,
    /// The held grenade's fuse.
    pub fuse: Option<Fuse>,
    /// The last release's velocity relative to the character, world axes, metres per second.
    pub last_release: Option<Vec3>,
    /// Whether the grenade weapon's update ran for the local player since the payload loaded.
    pub weapon_seen: bool,
    pub primed: u64,
    pub thrown: u64,
    /// Grenades gone from the hand before their throw (almost always their fuse).
    pub gone: u64,
    /// The most recent steps, newest last.
    pub events: Vec<GrenadeEvent>,
}

pub fn status() -> GrenadeStatus {
    STATE.lock().status.clone()
}

/// The hand a throw holds, from the grab at the pouch until the grenade leaves it.
pub fn throwing_hand() -> Option<Hand> {
    STATE.lock().stage.hand()
}

/// What the input tick hands the throw.
pub(super) struct TickInputs {
    /// Whether the throw may act this tick: its toggle is on and the mode has free hands.
    pub acting: bool,
    /// The hand that grabbed the grenade pouch this tick.
    pub pouch_grab: Option<Hand>,
    /// Each hand's grip over the tick, indexed by [`Hand::index`].
    pub grips: [ButtonControl; 2],
    /// The VR world scale the hands compose with.
    pub world_scale: f32,
    pub dt: f32,
}

/// The input tick's half: fold the grab and the release into the stage, hold the game's own throw
/// back while a throw is under way, and warn of the fuse.
pub(super) fn input_tick(
    inputs: &TickInputs,
    config: &ControllersConfig,
    desired: &mut Vec<Desired>,
) {
    let tuning = &config.weapons.grenade;
    let mut state = STATE.lock();
    if !inputs.acting {
        drop(state);
        stand_down();
        return;
    }
    if let Some(hand) = inputs.pouch_grab {
        state.stage = stage::on_input(state.stage, InputEvent::Grab(hand));
    }
    if let Some(hand) = state.stage.hand()
        && inputs.grips[hand.index()].released
    {
        // Without a tracked sample of the throw (the controller was occluded), the grenade is let
        // go where it is rather than kept in a hand that has opened.
        let release = release(hand, inputs.world_scale, tuning).unwrap_or(Release {
            velocity: Vec3::ZERO,
        });
        state.stage = stage::on_input(state.stage, InputEvent::Release(hand, release));
        state.status.last_release = Some(release.velocity);
        tracing::debug!(target: "controllers", ?hand, velocity = ?release.velocity, "grenade release");
    }
    state.status.stage = state.stage;
    if state.stage != Stage::Idle {
        desired.retain(|d| {
            !(d.target.map == MapKind::Player && d.target.action == Action::THROW_GRENADE)
        });
    }
    let (mut prims, pulse) = warning(&mut state, inputs.dt, config).unwrap_or_default();
    drop(state);
    if prims.is_empty() {
        store::clear(Source::GrenadeWarning);
    } else {
        store::submit(Source::GrenadeWarning, &mut prims, WARNING_TTL);
    }
    if let Some(hand) = pulse {
        haptics::pulse(hand, tuning.warning_pulse, PulseSource::GrenadeFuse);
    }
}

/// Stop the throw: a held grenade goes back to the inventory at the weapon's next update, and a
/// grab not yet served is forgotten.
pub(super) fn stand_down() {
    let mut state = STATE.lock();
    state.stage = stage::on_input(state.stage, InputEvent::StandDown);
    state.status.stage = state.stage;
    state.next_pulse_s = 0.0;
    drop(state);
    store::clear(Source::GrenadeWarning);
}

/// Note the controllers' grips in the world for the weapon's update that follows on this thread:
/// where a held grenade sits and where a throw leaves from. `None` for a hand the aim update did not
/// compose (the aim is idle, or the hand is untracked).
pub(crate) fn observe_hands(grips: [Option<Pose>; 2]) {
    STATE.lock().grips = grips;
}

/// The point the game's own throw aims at when the hand aims it: the dominant hand's aim point,
/// with the throw's toggles on. The aim update writes it into the aim control's grenade slot.
pub(crate) fn fallback_target(
    config: &ControllersConfig,
    dominant_target: Option<Vec3>,
) -> Option<Vec3> {
    let on = config.weapons.grenade_throw && config.weapons.grenade.hand_aimed_fallback;
    dominant_target.filter(|_| on)
}

/// The overlay's warning slot lives a little past one input tick.
const WARNING_TTL: Duration = Duration::from_millis(150);

/// How many throw steps the readout keeps.
const MAX_EVENTS: usize = 6;

const WARNING_CALM: Rgba = [0.4, 1.0, 0.4, 0.8];
const WARNING_URGENT: Rgba = [1.0, 0.15, 0.1, 0.95];

struct State {
    stage: Stage,
    status: GrenadeStatus,
    /// The controllers' grips in the world, from the last aim update.
    grips: [Option<Pose>; 2],
    /// Seconds until the next warning pulse.
    next_pulse_s: f32,
}

static STATE: Mutex<State> = Mutex::new(State {
    stage: Stage::Idle,
    status: GrenadeStatus {
        stage: Stage::Idle,
        fuse: None,
        last_release: None,
        weapon_seen: false,
        primed: 0,
        thrown: 0,
        gone: 0,
        events: Vec::new(),
    },
    grips: [None; 2],
    next_pulse_s: 0.0,
});

/// The launch planned for the weapon update in progress on this thread, for the launch hook.
#[derive(Copy, Clone)]
struct PlannedLaunch {
    /// The grenade's address.
    explosive: usize,
    /// Where it leaves from: the hand, or `None` to keep the game's.
    transform: Option<Matrix4>,
    /// The hand's release velocity relative to the character.
    velocity: Vec3,
}

thread_local! {
    static LAUNCH: Cell<Option<PlannedLaunch>> = const { Cell::new(None) };
}

/// The release of a throw by `hand`: the velocity of the held grenade's point over the samples just
/// before now, turned into world axes and scaled. `None` without samples.
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

/// The held grenade's offset in `hand`'s grip frame: the configured right-hand offset, mirrored
/// for the left.
fn hold_offset(hand: Hand, tuning: &GrenadeConfig) -> Vec3 {
    let o = tuning.hold_offset_m;
    match hand {
        Hand::Left => Vec3::new(-o.x, o.y, o.z),
        Hand::Right => o,
    }
}

/// The fuse warning for the tick: the overlay's ring at the throwing hand, and the hand to pulse
/// this tick, if a pulse is due.
fn warning(
    state: &mut State,
    dt: f32,
    config: &ControllersConfig,
) -> Option<(Vec<Primitive>, Option<Hand>)> {
    let tuning = &config.weapons.grenade;
    let (Stage::Held { hand, .. }, Some(fuse)) = (state.stage, state.status.fuse) else {
        state.next_pulse_s = 0.0;
        return None;
    };
    let urgency = fuse_urgency(fuse.remaining_s, tuning.warning_s);
    let mut prims = Vec::new();
    if tuning.warning_overlay && config.overlay.enabled {
        prims.push(Primitive::Sprite {
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
            color: mix(WARNING_CALM, WARNING_URGENT, urgency),
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
    Some((prims, pulse))
}

/// How urgent the fuse is, `0..=1`: zero until `warning_s` remain, one when it runs out.
pub fn fuse_urgency(remaining_s: f32, warning_s: f32) -> f32 {
    if warning_s.is_nan() || warning_s <= 0.0 || !remaining_s.is_finite() {
        return 0.0;
    }
    (1.0 - remaining_s / warning_s).clamp(0.0, 1.0)
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Record a step's outcome.
fn report(state: &mut State, hand: Option<Hand>, outcome: Outcome) {
    tracing::debug!(target: "controllers", ?hand, outcome = %outcome.label(), "grenade");
    match outcome {
        Outcome::Primed => state.status.primed += 1,
        Outcome::Thrown(_) => state.status.thrown += 1,
        Outcome::Gone => state.status.gone += 1,
        _ => {}
    }
    state.status.events.push(GrenadeEvent { hand, outcome });
    let excess = state.status.events.len().saturating_sub(MAX_EVENTS);
    state.status.events.drain(..excess);
}

/// The grenade weapon's update from its user: the stage's request set before it, and the stage
/// settled after it.
#[detour(address = jc3gi::equipment::weapon::ThrownGrenadeWeapon::UpdateFromUserPostSim_Parallel_ADDRESS)]
fn update_from_user(
    weapon: *mut ThrownGrenadeWeapon,
    dt: f32,
    aim_position: *const Vector3,
    input: *const WeaponInputs,
) {
    let original = || {
        UPDATE_FROM_USER
            .get()
            .unwrap()
            .call(weapon, dt, aim_position, input)
    };
    // SAFETY: the weapon is live for its own update.
    let Some(w) = (unsafe { local_weapon(weapon) }) else {
        return original();
    };
    let (enabled, tuning) =
        Config::lock_query(|c| (c.controllers.enabled, c.controllers.weapons.grenade));
    let acting = enabled && scheme::is_controllers();

    let mut state = STATE.lock();
    state.status.weapon_seen = true;
    if !acting && state.stage == Stage::Idle {
        drop(state);
        return original();
    }
    // SAFETY: as above, on the thread that updates the weapon.
    let view = unsafe { weapon_view(w) };
    let hand = state.stage.hand();
    let (next, action, outcome) = stage::before_update(state.stage, view);
    state.stage = next;
    if let Some(outcome) = outcome {
        report(&mut state, hand, outcome);
    }
    match action {
        WeaponAction::None => {}
        WeaponAction::Prime => w.m_PrimeGrenade = true,
        WeaponAction::Fire(release) => {
            // SAFETY: the primed grenade was found live just above.
            if let Some(explosive) = unsafe { primed_explosive(w) } {
                let transform = hand.and_then(|h| {
                    state.grips[h.index()].map(|grip| held_matrix(grip, hold_offset(h, &tuning)))
                });
                LAUNCH.set(Some(PlannedLaunch {
                    explosive: explosive as usize,
                    transform,
                    velocity: release.velocity,
                }));
            }
            // SAFETY: the thrown grenade weapon is a weapon at the same address.
            unsafe { (*weapon.cast::<WeaponBase>()).m_ForceFireProjectile = true };
        }
        // SAFETY: the weapon is live, on the thread that updates it.
        WeaponAction::Cancel => unsafe { w.CancelPrimedGrenade() },
    }
    drop(state);

    original();
    LAUNCH.set(None);

    // SAFETY: as above.
    let w = unsafe { &mut *weapon };
    let primed = unsafe { primed_explosive(w) };
    let mut state = STATE.lock();
    let hand = state.stage.hand();
    let (next, outcome) = stage::after_update(state.stage, primed.is_some());
    state.stage = next;
    state.status.stage = next;
    if let Some(outcome) = outcome {
        report(&mut state, hand, outcome);
        if outcome == Outcome::Primed
            && let Some(hand) = hand
        {
            haptics::pulse(hand, tuning.prime_pulse, PulseSource::GrenadeFuse);
        }
    }
    // SAFETY: the grenade was found live just above, on this thread.
    state.status.fuse = primed
        .filter(|_| next.grenade_in_hand())
        .and_then(|explosive| unsafe { explosive.as_ref() })
        .map(|e| Fuse {
            remaining_s: e.m_DetonationTimer,
            total_s: e.m_StartDetonationTimer,
        });
}

/// The weapon's attachment update: a grenade held by the throw rides the hand rather than the
/// grenade weapon's bone.
#[detour(address = jc3gi::equipment::weapon::ThrownGrenadeWeapon::UpdateAttachmentTransforms_ADDRESS)]
fn update_attachment_transforms(
    weapon: *mut ThrownGrenadeWeapon,
    p2: f32,
    p3: f32,
    transform_type: u32,
) {
    UPDATE_ATTACHMENT_TRANSFORMS
        .get()
        .unwrap()
        .call(weapon, p2, p3, transform_type);
    // SAFETY: the weapon is live for its own update.
    let Some(w) = (unsafe { local_weapon(weapon) }) else {
        return;
    };
    let (stage, grips) = {
        let state = STATE.lock();
        (state.stage, state.grips)
    };
    let Some(hand) = stage.hand().filter(|_| stage.grenade_in_hand()) else {
        return;
    };
    let Some(grip) = grips[hand.index()] else {
        return;
    };
    let offset = hold_offset(hand, &Config::lock_query(|c| c.controllers.weapons.grenade));
    // SAFETY: the weapon's primed grenade is live while the weak reference locks, on this thread.
    if let Some(explosive) = unsafe { primed_explosive(w).and_then(|e| e.as_mut()) } {
        explosive.m_WorldMatrixT1 = held_matrix(grip, offset);
    }
}

/// The grenade's launch: moved to the hand, with the hand's velocity, when it is the throw's.
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
    let Some(launch) = LAUNCH.get().filter(|l| l.explosive == grenade as usize) else {
        return;
    };
    LAUNCH.set(None);
    // SAFETY: a grenade is an explosive at the same address, live for its own launch.
    let Some(explosive) = (unsafe { grenade.cast::<Explosive>().as_mut() }) else {
        return;
    };
    if let Some(transform) = launch.transform {
        // SAFETY: as above; the call only queues the transform for the physics body.
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

/// The weapon, when it is the local player's.
///
/// # Safety
///
/// `weapon` must be null or a live thrown grenade weapon.
unsafe fn local_weapon<'a>(
    weapon: *mut ThrownGrenadeWeapon,
) -> Option<&'a mut ThrownGrenadeWeapon> {
    let base = unsafe { weapon.cast::<WeaponBase>().as_ref() }?;
    if !base.m_IsUserLocalPlayer {
        return None;
    }
    unsafe { weapon.as_mut() }
}

/// The weapon's state as its update begins.
///
/// # Safety
///
/// `weapon` must be the local player's live thrown grenade weapon, on the thread that updates it.
unsafe fn weapon_view(weapon: &mut ThrownGrenadeWeapon) -> WeaponView {
    let primed = unsafe { primed_explosive(weapon) };
    let physics_ready = primed
        .and_then(|e| unsafe { e.as_ref() })
        .is_some_and(|e| !e.m_PfxInstance.is_null());
    let base = unsafe { &*(weapon as *const ThrownGrenadeWeapon).cast::<WeaponBase>() };
    let user = unsafe { base.m_User[0].cast::<Character>().as_ref() };
    let refusal = if !user.is_some_and(|c| c.m_Inventory.m_DoesAnimationAllowGrenades) {
        Some(Refusal::NotAllowed)
    } else if unsafe { base.GetTotalAmmo() } <= 0 {
        Some(Refusal::NoGrenades)
    } else {
        None
    };
    WeaponView {
        primed: primed.is_some(),
        physics_ready,
        refusal,
    }
}

/// The weapon's primed grenade, while its weak reference is live.
///
/// # Safety
///
/// `weapon` must be a live thrown grenade weapon, on the thread that updates it.
unsafe fn primed_explosive(weapon: &ThrownGrenadeWeapon) -> Option<*mut Explosive> {
    let object = unsafe { weak_target(&weapon.m_PrimedGrenade) }?;
    Some(
        object
            .cast::<u8>()
            .wrapping_sub(Explosive::GAME_OBJECT_BASE_OFFSET as usize)
            .cast::<Explosive>(),
    )
}

/// The held grenade's world matrix: the grip's world pose moved to the grenade's point.
fn held_matrix(grip: Pose, offset: Vec3) -> Matrix4 {
    let position = grip.position + grip.orientation * offset;
    Matrix4::from(Mat4::from_rotation_translation(
        grip.orientation.normalize(),
        position,
    ))
}

#[cfg(test)]
mod tests {
    use glam::Quat;

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

    #[test]
    fn the_held_matrix_puts_the_grenade_at_the_offset() {
        let grip = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let m = Mat4::from(held_matrix(grip, Vec3::new(0.0, 0.0, -0.1)));
        let at = m.w_axis.truncate();
        // A quarter turn about +Y carries -Z to -X.
        assert!(at.distance(Vec3::new(0.9, 2.0, 3.0)) < 1e-5, "{at}");
    }
}
