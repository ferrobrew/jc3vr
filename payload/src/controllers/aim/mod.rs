//! Per-hand aiming: the plan of record's "Aiming" section
//! (`docs/mod/input/controllers-and-roomscale.md`), with the engine side in
//! `docs/engine/gameplay/aim-pipeline.md` and `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! - [`ray`]: the pure ray math: the gun sight line, the aim-pose and grapple rays, the gun's aim
//!   point, and the camera matrix that stands in for the camera while the grapple is aimed;
//! - [`raycast`]: a scene raycast that ignores the player;
//! - [`gun`]: the guns aim along their hands' sight lines, through the per-weapon update;
//! - [`grapple`]: the grapple slot is filled from the off-hand's ray by the game's own targeting;
//! - [`grapple_weapons`]: inside that, the grapple's aim also sees the weapons lying in the world;
//! - [`targets`]: the game's own aim candidates, which the target selections below share;
//! - [`assist`]: per-hand aim assist over those candidates, and the grapple's, which turns the
//!   grapple ray onto the valid grapple target nearest it before the game's targeting runs;
//! - [`sights`]: each weapon's sight correction, looked up from the config and the embedded sight
//!   table, and the in-headset tool that tunes it.
//!
//! Everything runs from the player aim control's post-simulation update ([`update_post_sim`]), the
//! simulation-phase update where the game casts its own aim ray. Before the update, the hands are
//! composed into the world from the latest controller frame and the grapple ray is handed to the
//! grapple's hooks; after it, the hand rays are cast, the guns' aim points are published for the
//! per-weapon update that follows on the same thread, and the tick's results are published for the
//! world-space overlay ([`status`]). The same update hands the weapon handling what it needs from
//! the hands in the world: the grips for the grenade and C4 throws, and the dominant hand's aim
//! point for the game's own grenade throw.
//!
//! Both parts act only under the controller scheme, in gameplay, and for a tracked hand, each behind
//! its toggle in [`AimConfig`](crate::controllers::config::AimConfig); otherwise the game aims as it
//! always does.

pub mod assist;
pub mod grapple;
pub mod grapple_weapons;
pub mod gun;
pub mod ray;
pub mod raycast;
pub mod sights;
pub mod targets;

use std::num::NonZeroUsize;

use detours_macro::detour;
use glam::Vec3;
pub(crate) use grapple::camera_matrix_override;
use jc3gi::{
    aim::{aim::PlayerAimControl, target::TargetType},
    character::character::Character,
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        aim::{
            assist::{AssistCorrection, GrappleAssist},
            gun::HeldWeapon,
            ray::Ray,
            raycast::{CastOptions, RayHit, RaycastError},
        },
        cockpit::CockpitTransform,
        config::ControllersConfig,
        frame::{Hand, Pose},
        hands::{self, ik, late},
        mode::{self, Mode},
        physical_hands, scheme, vehicles,
        weapons::{
            c4, collide, grenade,
            hold::{armed, tick as hold_tick},
        },
    },
    frame_timing::{self, Slot},
    hooks,
};

/// The nearest a gun's aim point may be to the sight's origin, metres. A nearer hit is pushed out
/// along the ray, because the game fires along the barrel instead of at an aim point near the
/// muzzle, and skips the weapon's update entirely for one almost at the muzzle.
const MIN_TARGET_DISTANCE_M: f32 = 2.0;

/// The aiming hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_POST_SIM_BINDER)
        .with_hook_library(grapple::hook_library())
        .with_hook_library(grapple_weapons::hook_library())
        .with_hook_library(gun::hook_library())
}

/// How far the hand rays are cast, metres. A gun ray that hits nothing aims at this distance along
/// the ray.
pub const RAY_LENGTH_M: f32 = 500.0;

/// Where the grapple ray starts relative to the off-hand's wrist, in the wrist frame (-Z toward the
/// fingers, +Y out of the back of the hand), metres: at the wrist, where the gauntlet's line leaves
/// the forearm.
pub const GRAPPLE_ORIGIN_M: Vec3 = Vec3::ZERO;

/// Which ray a hand's aim follows.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandSource {
    /// The controller's aim pose: the hand holds no gun.
    AimPose,
    /// The sight line of the wielded weapon's gun `gun`.
    Gun { gun: usize },
}

/// One hand's aim for a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandAim {
    pub source: HandSource,
    pub ray: Ray,
    /// The closest hit along the ray, within [`RAY_LENGTH_M`], or `None` for a miss.
    pub hit: Option<RayHit>,
    /// The point a gun in this hand aims at: the hit, or the far point along the ray, never nearer
    /// than the configured minimum.
    pub target: Vec3,
}

/// The grapple's aim for a tick, read back from the grapple slot after the game's targeting.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrappleAim {
    /// The ray from the gauntlet the game's targeting ran along: the hand's, or the aim assist's
    /// turn of it ([`assist`](Self::assist)).
    pub ray: Ray,
    /// The grapple slot's aim point: the surface hit, a target's body position, or the point at
    /// the grapple's range.
    pub point: Vec3,
    /// Whether the ray hit something or a target was selected.
    pub hit: bool,
    /// Whether firing now would attach: the game holds a valid grapple target in range.
    pub valid: bool,
    /// Whether the target is an object rather than world geometry, so that a tether pulls it. A
    /// best guess at the plan's "targets that support tethering", not yet checked in play.
    pub tetherable: bool,
    /// Whether [`point`](Self::point) is current: the game's targeting ran (the game takes the
    /// grapple's fire), or the seat grapple cast its own ray. While it is not, the point may be
    /// stale, and nothing is drawn at it.
    pub current: bool,
    /// The aim is the seat grapple's ([`crate::grapple::seat`]), cast by the mod in a vehicle seat.
    pub seat: bool,
    /// The aim assist's turn of the ray onto a grapple target, when it made one.
    pub assist: Option<GrappleAssist>,
}

/// Why the aim did nothing on the last update.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AimIdle {
    /// The controllers are off.
    Disabled,
    /// The gamepad scheme is in effect.
    GamepadScheme,
    /// Outside gameplay, or no tracked hands.
    NoHands,
}

/// The aim's readout: the last update's per-hand results, for the debug UI and the world-space
/// overlay layer.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AimStatus {
    /// Why nothing was aimed, when nothing was.
    pub idle: Option<AimIdle>,
    /// Each hand's aim, indexed by [`Hand::index`]; `None` for an untracked hand or while idle.
    pub hands: [Option<HandAim>; 2],
    /// The grapple's aim, while the grapple follows the off-hand.
    pub grapple: Option<GrappleAim>,
    /// The wielded weapon the guns' aim points were published for.
    pub weapon: Option<HeldWeapon>,
    /// The weapon armed in each hand beside the wielded one, aimed along its own sight line, by
    /// [`Hand::index`].
    pub armed: [Option<HeldWeapon>; 2],
    /// The last raycast failure, if the last update had one.
    pub raycast_error: Option<RaycastError>,
    /// Each gun hand's aim-assist correction on the last update, indexed by [`Hand::index`].
    pub assist: [Option<AssistCorrection>; 2],
    /// Aim-assist corrections made, since the payload loaded.
    pub assist_corrections: u64,
    /// Aim updates that aimed, since the payload loaded.
    pub updates: u64,
}

/// The latest readout.
pub fn status() -> AimStatus {
    *STATUS.lock()
}

/// The character-level weapon aim point while the guns follow the hands: the dominant hand's aim
/// point, or the mounted gun's point from its handles (which the game draws the gun toward), or
/// `None` when neither aim is acting. The aim-state hook writes it after the game's own point is
/// set, so the point still counts as set this frame.
pub fn character_aim_point() -> Option<Vec3> {
    let dominant = Config::lock_query(|c| c.controllers.acting_hand());
    gun::character_aim_point(dominant).or_else(vehicles::mounted_aim_point)
}

/// A hand's world-space poses for a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct WorldHand {
    /// The character's wrist frame.
    wrist: Pose,
    /// The controller's grip pose.
    grip: Pose,
    /// The controller's aim pose.
    aim: Pose,
    /// The correction that carries the hand's gun from the last tick's placement to the controller,
    /// or `None` when the gun stays where the game placed it.
    correction: Option<Pose>,
}

/// The tick's inputs: the configuration, the head's world pose, and each tracked hand's world
/// poses.
pub(crate) struct TickHands {
    config: ControllersConfig,
    head: Pose,
    hands: [Option<WorldHand>; 2],
}

/// The aim control's post-simulation update: the grapple's view set around it, and the hand rays
/// cast and published after it.
#[detour(address = jc3gi::aim::aim::PlayerAimControl::UpdatePostSim_ADDRESS)]
fn update_post_sim(aim: *mut PlayerAimControl, dt: f32, character: *mut Character) {
    let original = || UPDATE_POST_SIM.get().unwrap().call(aim, dt, character);
    let owner = unsafe { aim.as_ref() }
        .and_then(|a| unsafe { a.m_Owner.as_ref() })
        .filter(|c| c.m_IsLocalCharacter);
    // The world markers' fallback raycasts run here whatever the control scheme: this is the
    // simulation update, outside the physics step, where the game casts its own aim ray.
    if let Some(owner) = owner {
        // SAFETY: the owner is the live local player character, on the simulation thread.
        unsafe { crate::hud::world_markers::resolve_rays(owner) };
        // SAFETY: as above. The head probe runs in every state and scheme, for the comfort fade.
        unsafe { crate::controllers::body::rig::collision::tick(owner) };
    }
    let tick = match owner.map(|_| begin()) {
        Some(Ok(tick)) => tick,
        Some(Err(idle)) => {
            idle_update(idle);
            grenade::observe_hands([None; 2]);
            c4::observe_hands([None; 2]);
            return original();
        }
        None => return original(),
    };
    let _timing = frame_timing::scope(Slot::Aim);
    grenade::observe_hands(tick.hands.map(|h| h.map(|h| h.grip)));
    c4::observe_hands(tick.hands.map(|h| h.map(|h| h.grip)));
    if owner.is_some() {
        // SAFETY: the owner is the live local player, in the simulation phase outside the physics
        // step.
        unsafe { c4::stick::probe(&tick.config) };
        // SAFETY: as above.
        unsafe { collide::probe() };
    }

    let off = tick.config.acting_hand().other();
    let grapple_ray = tick
        .config
        .aim
        .grapple
        .then(|| tick.hands[off.index()])
        .flatten()
        .and_then(|h| ray::grapple_ray(h.wrist, h.aim, GRAPPLE_ORIGIN_M));
    // The grapple's aim assist turns the ray before the game's own targeting runs along it.
    let grapple_assist = match (grapple_ray, owner) {
        (Some(ray), Some(owner)) => {
            // SAFETY: the owner is the live local player character, on the thread updating it.
            let options = unsafe { CastOptions::ignoring_rider(owner) }.seeing_through();
            // SAFETY: the live local player's aim control, before the original, with no grapple
            // view set; the options name the live owner and its vehicle.
            unsafe { assist::grapple_assist(aim, &ray, &options, &tick.config.aim) }
        }
        _ => None,
    };
    let grapple_ray = grapple_assist.map_or(grapple_ray, |(bent, _)| Some(bent));
    grapple::set_view(grapple_ray.map(|r| Matrix4::from(ray::view_matrix(&r))));
    original();
    grapple::set_view(None);

    let Some(owner) = owner.filter(|_| !aim.is_null()) else {
        return;
    };
    let grapple_assist = grapple_assist.map(|(_, assist)| assist);
    unsafe { finish(aim, owner, &tick, grapple_ray, grapple_assist) };
    physical_hands::after_aim(dt);
}

/// Decide whether the aim acts this update and compose the hands into the world.
fn begin() -> Result<TickHands, AimIdle> {
    let config = Config::lock_query(|c| c.controllers);
    // The hands are composed even with both the gun and the grapple aim off: the grenade and C4
    // throws follow them too.
    if !config.enabled {
        return Err(AimIdle::Disabled);
    }
    if !scheme::is_controllers() {
        return Err(AimIdle::GamepadScheme);
    }
    if !hooks::in_gameplay() {
        return Err(AimIdle::NoHands);
    }
    let active = hands::active_hands().ok_or(AimIdle::NoHands)?;
    let xf = CockpitTransform::current(active.head.position, active.world_scale);
    let compose = |pose: Pose| xf.pose_to_world(pose);
    let records = ik::tick_records();
    let late_update = config.hands.late_update;
    let hands = Hand::BOTH.map(|hand| {
        let cockpit = active.hands[hand.index()]?;
        let wrist = compose(cockpit.wrist);
        let correction = records.current[hand.index()]
            .filter(|_| late_update)
            .and_then(|target| late::correction(wrist, target.wrist, target.weight));
        Some(WorldHand {
            wrist,
            grip: compose(cockpit.grip),
            aim: compose(cockpit.aim),
            correction,
        })
    });
    if hands.iter().all(Option::is_none) {
        return Err(AimIdle::NoHands);
    }
    let head = xf.pose_to_world(active.head);
    Ok(TickHands {
        config,
        head,
        hands,
    })
}

/// Cast the hand rays, assist the guns' aim points and publish them, hand the weapon handling its
/// targets, and read back the grapple slot.
///
/// # Safety
///
/// `aim` and `owner` must be the live player aim control and its local player character, called
/// from the aim control's update after the original.
unsafe fn finish(
    aim: *mut PlayerAimControl,
    owner: &Character,
    tick: &TickHands,
    grapple_ray: Option<Ray>,
    grapple_assist: Option<GrappleAssist>,
) {
    let cfg = &tick.config.aim;
    let weapon = cfg
        .gun
        .then(|| unsafe { gun::held_weapon(owner) })
        .flatten();
    let (held, mut gun_rays) = match weapon {
        Some(weapon) => {
            let (held, rays) = unsafe { gun::sight_rays(weapon, tick, cfg) };
            (Some(held), rays)
        }
        None => (None, [None; 2]),
    };
    // A gun armed in a hand beside the wielded weapon aims along its own sight line too.
    let mut armed_weapons = [None; 2];
    if cfg.gun {
        for hand in Hand::BOTH {
            if gun_rays[hand.index()].is_some() {
                continue;
            }
            // SAFETY: the owner is the live local player character, on the thread updating it.
            let Some(weapon) = (unsafe { armed::weapon_of(owner, hand) }) else {
                continue;
            };
            let (armed, rays) = unsafe { gun::sight_rays(weapon, tick, cfg) };
            gun_rays[hand.index()] = rays[hand.index()];
            armed_weapons[hand.index()] = Some(armed);
        }
    }

    // SAFETY: the owner is the live local player character, on the thread updating it.
    let options = unsafe { CastOptions::ignoring_rider(owner) }.seeing_through();
    let mut raycast_error = None;
    let hands = Hand::BOTH.map(|hand| {
        let world = tick.hands[hand.index()]?;
        let (source, ray) = match gun_rays[hand.index()] {
            Some((gun, ray)) => (HandSource::Gun { gun }, ray),
            None => (HandSource::AimPose, ray::pose_ray(world.aim)?),
        };
        let hit = unsafe { raycast::cast(&ray, RAY_LENGTH_M, &options) }.unwrap_or_else(|error| {
            raycast_error = Some(error);
            None
        });
        let (target, _) = ray::aim_point(
            &ray,
            hit.map(|h| h.distance),
            RAY_LENGTH_M,
            MIN_TARGET_DISTANCE_M,
        );
        Some(HandAim {
            source,
            ray,
            hit,
            target,
        })
    });

    let mut assist = [None; 2];
    let gun_targets = Hand::BOTH.map(|hand| {
        let h = hands[hand.index()].filter(|h| matches!(h.source, HandSource::Gun { .. }))?;
        // SAFETY: forwarded from the caller; the options name the live owner and its vehicle.
        match unsafe { assist::assist(aim, &h.ray, h.target, &options, cfg) } {
            Some((point, correction)) => {
                assist[hand.index()] = Some(correction);
                Some(point)
            }
            None => Some(h.target),
        }
    });
    gun::publish(held, gun_targets);
    hold_tick::publish_targets(
        owner as *const Character as usize,
        Hand::BOTH.map(|hand| {
            Some((
                armed_weapons[hand.index()]?.address,
                gun_targets[hand.index()]?,
            ))
        }),
    );
    let seat = match mode::current().mode {
        Mode::Vehicle(seat) => Some(seat),
        _ => None,
    };
    // SAFETY: the owner is the live local player character, on the simulation thread.
    let mounted = unsafe { owner.IsInMountedGunState() };
    let body_point = vehicles::passenger::body_aim_point(&tick.config, seat, mounted, gun_targets);
    vehicles::passenger::note(body_point.is_some());
    // SAFETY: the aim control is live for its own update, and the original has run.
    if let Some(aim) = unsafe { aim.as_mut() }
        && let Some(point) = body_point
    {
        aim.m_AimPos[TargetType::TARGET_WEAPONS as usize] = Vector3 {
            data: point.to_array(),
        };
    }
    sights::tuning::service(
        &cfg.sight_tuning,
        tick.head,
        held.as_ref(),
        tick.config.acting_hand(),
    );

    let dominant = tick.config.acting_hand();
    // SAFETY: the aim control is live for its own update, and the original has run.
    if let Some(aim) = unsafe { aim.as_mut() }
        && let Some(point) =
            grenade::fallback_target(&tick.config, hands[dominant.index()].map(|h| h.target))
    {
        aim.m_AimPos[TargetType::TARGET_GRENADE as usize] = Vector3 {
            data: point.to_array(),
        };
    }

    // SAFETY: the live local player's hook, on the thread updating it; the calls only read.
    let hook = unsafe { crate::grapple::hook::of(owner) };
    let fire_enabled = hook.is_some_and(|hook| unsafe { hook.GetFireInputEnabled() });
    // SAFETY: as above.
    let seat_range_m = hook
        .filter(|_| !fire_enabled && tick.config.grapple.seat && unsafe { seated(owner) })
        .and_then(|hook| unsafe { hook.GetTuning().as_ref() })
        .map(|tuning| tuning.m_GrappleDistance)
        .filter(|range| range.is_finite() && *range > 0.0);
    let grapple = grapple_ray.and_then(|ray| match seat_range_m {
        Some(range_m) => {
            // SAFETY: the simulation phase outside the physics step, as for the hands' casts.
            let hit = unsafe { raycast::cast(&ray, range_m, &options) }.unwrap_or_else(|error| {
                raycast_error = Some(error);
                None
            });
            Some(seat_readout(ray, hit, range_m))
        }
        // SAFETY: as above.
        None => unsafe { aim.as_ref() }
            .map(|aim| grapple_readout(aim, ray, grapple_assist, fire_enabled)),
    });
    let mut status = STATUS.lock();
    let corrections = assist.iter().flatten().count() as u64;
    *status = AimStatus {
        idle: None,
        hands,
        grapple,
        weapon: held,
        armed: armed_weapons,
        raycast_error,
        assist,
        assist_corrections: status.assist_corrections + corrections,
        updates: status.updates + 1,
    };
}

/// The grapple slot after the game's targeting, as the grapple's readout. While the game takes no
/// fire (`fire_enabled` false: a vehicle seat, a fire under way, and the other states
/// [`GrapplingHook::GetFireInputEnabled`] refuses), nothing is hit or valid and the point is not
/// current: the game may leave the slot as it was when the fire was last allowed, which in a seat
/// is the boarding point.
///
/// [`GrapplingHook::GetFireInputEnabled`]: jc3gi::equipment::grappling_hook::GrapplingHook::GetFireInputEnabled
fn grapple_readout(
    aim: &PlayerAimControl,
    ray: Ray,
    assist: Option<GrappleAssist>,
    fire_enabled: bool,
) -> GrappleAim {
    const GRAPPLE: usize = TargetType::TARGET_GRAPPLE as usize;
    let (hit, valid, tetherable) = slot_flags(
        SlotFlags {
            hit: aim.m_DirectHits[GRAPPLE] || !aim.m_Target[GRAPPLE].is_null(),
            valid: aim.m_HasLastValidGrapple && aim.m_IsInRange[GRAPPLE],
            object: NonZeroUsize::new(aim.m_LastHitGrappleTargetGO[0] as usize).is_some(),
        },
        fire_enabled,
    );
    GrappleAim {
        ray,
        point: Vec3::from_array(aim.m_AimPos[GRAPPLE].data),
        hit,
        valid,
        tetherable,
        current: fire_enabled,
        seat: false,
        assist,
    }
}

/// The seat grapple's readout ([`crate::grapple::seat`]): the off-hand's ray cast by the mod, the
/// game's targeting not running in a seat, valid while it hits something within the grapple's
/// range `range_m`.
fn seat_readout(ray: Ray, hit: Option<RayHit>, range_m: f32) -> GrappleAim {
    let in_range = hit.filter(|hit| hit.distance <= range_m);
    GrappleAim {
        ray,
        point: in_range.map_or_else(|| ray.at(range_m), |hit| hit.position),
        hit: hit.is_some(),
        valid: in_range.is_some(),
        tetherable: in_range.is_some_and(|hit| hit.object.is_some()),
        current: true,
        seat: true,
        assist: None,
    }
}

/// Whether `character` rides in a vehicle seat, as the game's own vehicle grapple tests it.
///
/// # Safety
///
/// `character` must be live, on the game thread.
unsafe fn seated(character: &Character) -> bool {
    !unsafe { character.GetVehiclePtr() }.is_null()
        && unsafe { character.IsInRidingInVehicleState() }
}

/// What the grapple slot says of its target, before the fire gate.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct SlotFlags {
    /// The ray hit something, or a target is selected.
    hit: bool,
    /// The game holds a valid grapple target in range.
    valid: bool,
    /// The cached grapple target is an object.
    object: bool,
}

/// The readout's hit, valid, and tetherable flags from the slot's, none of them while the game
/// takes no fire.
const fn slot_flags(slot: SlotFlags, fire_enabled: bool) -> (bool, bool, bool) {
    let valid = fire_enabled && slot.valid;
    (fire_enabled && slot.hit, valid, valid && slot.object)
}

/// Record an update that aimed nothing, and withdraw the guns' aim points.
fn idle_update(idle: AimIdle) {
    gun::publish(None, [None; 2]);
    hold_tick::publish_targets(0, [None; 2]);
    let mut status = STATUS.lock();
    *status = AimStatus {
        idle: Some(idle),
        updates: status.updates,
        assist_corrections: status.assist_corrections,
        ..AimStatus::default()
    };
}

static STATUS: Mutex<AimStatus> = Mutex::new(AimStatus {
    idle: None,
    hands: [None; 2],
    grapple: None,
    weapon: None,
    armed: [None; 2],
    raycast_error: None,
    assist: [None; 2],
    assist_corrections: 0,
    updates: 0,
});

#[cfg(test)]
mod tests {
    use super::*;

    const SLOT: SlotFlags = SlotFlags {
        hit: true,
        valid: true,
        object: true,
    };

    #[test]
    fn the_slot_reads_through_while_the_game_takes_the_fire() {
        assert_eq!(slot_flags(SLOT, true), (true, true, true));
        let wall = SlotFlags {
            object: false,
            ..SLOT
        };
        assert_eq!(slot_flags(wall, true), (true, true, false));
    }

    fn hit_at(distance: f32, object: bool) -> RayHit {
        RayHit {
            position: Vec3::new(0.0, 0.0, -distance),
            normal: Vec3::Z,
            distance,
            object: object.then(|| NonZeroUsize::new(0x1000).unwrap()),
            body: 0,
            material: None,
        }
    }

    #[test]
    fn the_seat_aim_is_its_own_casts_hit_within_the_grapples_range() {
        let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
        let near = seat_readout(ray, Some(hit_at(20.0, true)), 80.0);
        assert!(near.current && near.seat && near.valid && near.tetherable);
        assert_eq!(near.point, Vec3::new(0.0, 0.0, -20.0));
        let far = seat_readout(ray, Some(hit_at(120.0, false)), 80.0);
        assert!(far.current && far.hit && !far.valid);
        assert_eq!(far.point, Vec3::new(0.0, 0.0, -80.0));
        let miss = seat_readout(ray, None, 80.0);
        assert!(miss.current && !miss.hit && !miss.valid);
    }

    #[test]
    fn nothing_is_hit_or_valid_while_the_game_takes_no_fire() {
        // In a seat the slot keeps the boarding point's target; the readout must not report it.
        assert_eq!(slot_flags(SLOT, false), (false, false, false));
    }
}
