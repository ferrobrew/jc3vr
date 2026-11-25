//! Breaking glass by hand: a palm or a held gun that strikes a window damages it the way a bullet
//! does, and a hard enough strike, or enough damage, brings the whole pane down. The design is in
//! `docs/mod/input/breaking-glass.md`, the engine side in `docs/engine/rendering/window-glass.md`.
//!
//! Window glass has no collision with the hand bodies or a held gun, but its shapes answer the
//! engine's ray queries with the glass material, which is how a bullet finds the windows it
//! damages ([`Bullet::OnBulletHitGlass`](jc3gi::equipment::weapon::Bullet::OnBulletHitGlass)).
//! So each tick, every striker (an empty hand's palm, and the muzzle end of a held gun,
//! [`strike::Striker`]) is swept from where it was on the last tick to where it is now, as the
//! bullet's own ray is cast: on the query layer, passing what bullets pass. The sweep is composed
//! in the cockpit frame, so a hand inside a moving car sweeps against the car's own glass rather
//! than the road behind it. A swing at [`GlassTuning::min_speed_mps`] or faster whose sweep meets
//! glass strikes it:
//!
//! 1. The panes the hit names are looked up as the bullet looks them up, and each takes a damage
//!    point where the sweep met it, in its own space, through the bullet's own damage call; the
//!    damage grows with the swing's speed ([`strike::hit_damage`]). Several strikes build cracks
//!    up as several bullets do.
//! 2. A strike at [`GlassTuning::shatter_speed_mps`] or faster, or one that leaves the pane's
//!    damage at [`GlassTuning::shatter_total`], brings the whole pane down. The game has no state
//!    for a window that is gone (its shader only cracks glass over), so the pane's model instance
//!    is hidden, and only when its model draws nothing but glass ([`engine::hide`]).
//! 3. The striking hand gets a haptic strike, and the game's melee impact against glass plays
//!    where the strike landed.
//!
//! The tick runs on the game thread at the start of the frame's render update
//! ([`main_thread_tick`]): the same thread and phase as the ammunition manager's deferred list,
//! which applies the bullets' window damage from their worker threads. It acts in every mode, on
//! foot and in a vehicle, under the controller scheme.

#[cfg(test)]
mod tests;

pub mod bullets;
pub mod config;
mod engine;
pub mod strike;

use std::{num::NonZeroUsize, time::Instant};

pub use config::{GlassConfig, GlassTuning};
use glam::Vec3;
use jc3gi::physics::{GameMaterialId, PhysicsSystem};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions, RaycastError},
        },
        cockpit::CockpitTransform,
        frame::{Hand, Pose},
        glass::{
            engine::{HideRefusal, Pane},
            strike::{Limiter, Outcome, STRIKERS, Sample, Striker, StrikerKind},
        },
        hands::{self, ActiveHands, CockpitHand},
        haptics::{self, Event},
        loadout::{ItemKind, live},
        physical_hands::{
            self,
            palm::PalmBox,
            sounds::{self, MeleeImpact},
            tuning::PALMS,
        },
        scheme,
        weapons::{self, collide},
    },
    hooks,
    thread_role::{self, ThreadRole},
};

/// Why breaking glass stands down this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Idle {
    /// Its switch is off, the controller scheme is not in effect, or the game is not in play.
    Disabled,
    /// The tick ran off the game thread.
    OffThread,
    /// No hands are tracked.
    NoHands,
}

impl Idle {
    pub const fn label(self) -> &'static str {
        match self {
            Idle::Disabled => "off, or not in play under the controller scheme",
            Idle::OffThread => "the tick ran off the game thread",
            Idle::NoHands => "no tracked hands",
        }
    }
}

/// The last strike on glass.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LastStrike {
    pub striker: Striker,
    pub speed_mps: f32,
    pub damage: f32,
    /// The pane's damage after the strike ([`strike::Accumulated::total`]).
    pub total: f32,
    pub outcome: Outcome,
    /// Whether a shatter hid the pane.
    pub hidden: bool,
}

/// Breaking glass's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GlassStatus {
    /// Why it stands down, while it does.
    pub idle: Option<Idle>,
    /// Sweeps cast.
    pub sweeps: u64,
    /// Sweeps that met glass.
    pub glass_hits: u64,
    /// Glass hits that named no window pane.
    pub no_pane: u64,
    /// Damage points added.
    pub strikes: u64,
    /// Panes brought down.
    pub shatters: u64,
    /// Shatters refused, by why.
    pub shared_models: u64,
    pub last: Option<LastStrike>,
    pub last_error: Option<RaycastError>,
}

pub(crate) fn hook_library() -> HookLibrary {
    bullets::hook_library()
}

pub fn status() -> GlassStatus {
    STATE.lock().status
}

/// Run breaking glass's tick.
///
/// # Safety
///
/// The game thread, outside the world update and the physics step.
pub(crate) unsafe fn main_thread_tick() {
    let config = Config::lock_query(|c| c.controllers.glass);
    let active = config.enabled && scheme::is_controllers() && hooks::in_gameplay();
    let idle = if !active {
        Some(Idle::Disabled)
    } else if thread_role::current() != ThreadRole::Game {
        Some(Idle::OffThread)
    } else {
        None
    };
    if let Some(idle) = idle {
        STATE.lock().stand_down(idle);
        return;
    }
    let Some(hands) = hands::active_hands_raw() else {
        STATE.lock().stand_down(Idle::NoHands);
        return;
    };
    let points = strikers(&hands, &config);
    let frame = CockpitTransform::current(hands.head.position, hands.world_scale);
    let held = physical_hands::held_props();
    let swings = {
        let mut state = STATE.lock();
        state.status.idle = None;
        let now_s = state.now_s();
        state.swings(points, now_s, hands.world_scale, &config.tuning)
    };
    for (striker, swing, now_s) in swings.into_iter().flatten() {
        let lead_m = match striker.kind {
            StrikerKind::Palm => config.tuning.palm_lead_m,
            StrikerKind::Weapon => config.tuning.tip_lead_m,
        } * frame.scale();
        let from = frame.point_to_world(swing.from);
        let to = frame.point_to_world(swing.to);
        let Some(sweep) = strike::sweep(from, to, lead_m) else {
            continue;
        };
        let velocity = frame
            .direction_to_world(swing.to - swing.from)
            .normalize_or_zero()
            * swing.speed_mps;
        let ignore = held[striker.hand.index()].map_or(0, NonZeroUsize::get);
        // SAFETY: forwarded from the caller: the game thread outside the physics step; the ignored
        // object is a prop the hand holds this tick, or none.
        let hit = unsafe { cast_for_glass(&sweep, ignore) };
        let mut state = STATE.lock();
        state.status.sweeps += 1;
        let (at, panes) = match hit {
            Ok(Some(hit)) => hit,
            Ok(None) => continue,
            Err(e) => {
                state.status.last_error = Some(e);
                continue;
            }
        };
        state.status.glass_hits += 1;
        if panes.is_empty() {
            state.status.no_pane += 1;
            continue;
        }
        state.limiter.note(striker, now_s);
        let Some(damage) = strike::hit_damage(swing.speed_mps, &config.tuning) else {
            continue;
        };
        let mut shattered = false;
        for pane in panes {
            // SAFETY: forwarded from the caller; the pane was found this tick.
            let result = unsafe { state.strike(pane, at, swing.speed_mps, damage, &config) };
            shattered |= result.hidden;
            state.status.last = Some(LastStrike {
                striker,
                speed_mps: swing.speed_mps,
                damage: result.damage,
                total: result.total,
                outcome: result.outcome,
                hidden: result.hidden,
            });
        }
        drop(state);
        let scale = if shattered {
            1.0
        } else {
            damage / config.tuning.max_damage.max(f32::EPSILON)
        };
        haptics::play_scaled(striker.hand, Event::HandStrike, scale);
        if config.sound {
            // SAFETY: forwarded from the caller.
            let played = unsafe {
                sounds::melee_impact(
                    at,
                    velocity,
                    GameMaterialId::EGameMaterialId_Glass as u32,
                    true,
                )
            };
            if played == MeleeImpact::Silent {
                let mut state = STATE.lock();
                if !state.warned_silent {
                    state.warned_silent = true;
                    tracing::info!(
                        target: "controllers",
                        "glass: the player's melee impact has no sound for glass"
                    );
                }
            }
        }
    }
}

/// Each striker's point this tick, in the cockpit frame, by [`Striker::index`]: an empty hand's
/// palm, and the muzzle end of the gun a hand holds.
fn strikers(hands: &ActiveHands, config: &GlassConfig) -> [Option<Vec3>; STRIKERS] {
    let free = physical_hands::free_hands();
    let wielded = weapons::status()
        .wielded
        .map_or([false; 2], |w| w.weapon.wielded.hands);
    let mut points = [None; STRIKERS];
    for hand in Hand::BOTH {
        let Some(pose) = hands.hands[hand.index()] else {
            continue;
        };
        if free[hand.index()] {
            let palm = PalmBox::from_wrist(pose.wrist, 1.0, &PALMS);
            points[Striker::new(hand, StrikerKind::Palm).index()] = Some(palm.centre);
            continue;
        }
        let holds_gun = wielded[hand.index()]
            || live::held_kind(hand).is_some_and(|kind| matches!(kind, ItemKind::Weapon(_)));
        if config.weapons && holds_gun {
            points[Striker::new(hand, StrikerKind::Weapon).index()] =
                Some(muzzle(hand, pose, &config.tuning));
        }
    }
    points
}

/// The muzzle end of the gun in `hand`: its measured muzzle for a long gun, or a pistol's reach
/// ahead of the aim pose.
fn muzzle(hand: Hand, pose: CockpitHand, tuning: &GlassTuning) -> Vec3 {
    match collide::measured(hand) {
        Some(gun) => pose.wrist.position + pose.wrist.orientation * gun.muzzle,
        None => ahead(pose.aim, tuning.pistol_reach_m),
    }
}

/// The point `distance` metres along `pose`'s forward.
fn ahead(pose: Pose, distance: f32) -> Vec3 {
    pose.position + pose.orientation * (Vec3::NEG_Z * distance)
}

/// Cast `sweep` as a bullet's ray is cast, and, when it meets glass, return where with the panes
/// the hit names.
///
/// # Safety
///
/// The game thread outside the physics step; `ignore` must be zero or a live game object.
unsafe fn cast_for_glass(
    sweep: &strike::Sweep,
    ignore: usize,
) -> Result<Option<(Vec3, Vec<Pane>)>, RaycastError> {
    let options = CastOptions {
        material: true,
        ..unsafe { CastOptions::ignoring_player(ignore) }.seeing_through()
    };
    visible_hit(sweep, |ray, length| unsafe {
        raycast::cast_on_with(
            ray,
            length,
            &options,
            PhysicsSystem::QUERY_LAYER,
            |hit, info| {
                let glass = hit.material == Some(GameMaterialId::EGameMaterialId_Glass as u32);
                let hidden = glass && bullets::names_hidden_pane(&mut info.base);
                let panes = (glass && !hidden).then(|| engine::panes_hit(info, hit.position));
                (hit.position, hidden, panes)
            },
        )
    })
}

fn visible_hit<T>(
    sweep: &strike::Sweep,
    mut cast: impl FnMut(&Ray, f32) -> Result<Option<(Vec3, bool, Option<T>)>, RaycastError>,
) -> Result<Option<(Vec3, T)>, RaycastError> {
    let Some(ray) = Ray::new(sweep.origin, sweep.direction) else {
        return Ok(None);
    };
    if !sweep.length.is_finite() || sweep.length <= 0.0 {
        return Err(RaycastError::InvalidDistance);
    }
    let mut travelled = 0.0;
    for _ in 0..16 {
        let remaining = sweep.length - travelled;
        let current = Ray {
            origin: ray.at(travelled),
            ..ray
        };
        let Some((point, hidden, value)) = cast(&current, remaining)? else {
            return Ok(None);
        };
        if !hidden {
            return Ok(value.map(|value| (point, value)));
        }
        let along = (point - current.origin).dot(ray.direction);
        if !point.is_finite() || !(0.0..remaining).contains(&along) {
            return Err(RaycastError::InvalidDistance);
        }
        let next = travelled + along + 0.001;
        if next >= sweep.length || next <= travelled || ray.at(next) == current.origin {
            return Ok(None);
        }
        travelled = next;
    }
    Ok(None)
}

/// What a strike did to one pane.
struct PaneStrike {
    damage: f32,
    total: f32,
    outcome: Outcome,
    hidden: bool,
}

/// One pane's strikes so far: the damage its point list did not take.
#[derive(Debug, Copy, Clone, PartialEq)]
struct PaneRecord {
    address: usize,
    excess: f32,
    last_s: f64,
}

struct State {
    /// The tick clock's start, set on the first tick.
    epoch: Option<Instant>,
    samples: [Option<Sample>; STRIKERS],
    limiter: Limiter,
    panes: Vec<PaneRecord>,
    status: GlassStatus,
    warned_silent: bool,
}

impl State {
    /// Seconds on the tick clock.
    fn now_s(&mut self) -> f64 {
        self.epoch
            .get_or_insert_with(Instant::now)
            .elapsed()
            .as_secs_f64()
    }

    fn stand_down(&mut self, idle: Idle) {
        self.samples = [None; STRIKERS];
        self.status.idle = Some(idle);
    }

    /// Note this tick's striker `points` at `now_s`, and return the swings fast enough to strike
    /// that the cooldown allows.
    fn swings(
        &mut self,
        points: [Option<Vec3>; STRIKERS],
        now_s: f64,
        world_scale: f32,
        tuning: &GlassTuning,
    ) -> [Option<(Striker, strike::Swing, f64)>; STRIKERS] {
        Striker::ALL.map(|striker| {
            let i = striker.index();
            let cur = points[i].map(|at| Sample { at, time_s: now_s });
            let prev = std::mem::replace(&mut self.samples[i], cur);
            let swing = strike::swing(prev?, cur?, world_scale, tuning.max_gap_s)?;
            strike::hit_damage(swing.speed_mps, tuning)?;
            self.limiter
                .ready(striker, now_s, tuning.cooldown_s)
                .then_some((striker, swing, now_s))
        })
    }

    /// Strike `pane` at `at`, world space, with `damage` from a swing at `speed_mps`.
    ///
    /// # Safety
    ///
    /// The game thread, within the tick the pane was found in.
    unsafe fn strike(
        &mut self,
        pane: Pane,
        at: Vec3,
        speed_mps: f32,
        damage: f32,
        config: &GlassConfig,
    ) -> PaneStrike {
        let now_s = self.now_s();
        // SAFETY: forwarded from the caller.
        let (before, after) = unsafe {
            let local = engine::local_point(pane, at);
            let before = engine::point_total(pane);
            engine::add_damage(pane, local, damage);
            (before, engine::point_total(pane))
        };
        self.status.strikes += 1;
        self.panes.retain(|p| now_s - p.last_s <= PANE_MEMORY_S);
        let record = match self.panes.iter_mut().find(|p| p.address == pane.address()) {
            Some(record) => record,
            None => {
                if self.panes.len() >= MAX_PANE_RECORDS {
                    self.panes.remove(0);
                }
                self.panes.push(PaneRecord {
                    address: pane.address(),
                    excess: 0.0,
                    last_s: now_s,
                });
                self.panes.last_mut().expect("just pushed")
            }
        };
        let accumulated = strike::accumulate(before, after, damage, record.excess);
        record.excess = accumulated.excess;
        record.last_s = now_s;
        let outcome = if config.shatter {
            strike::outcome(speed_mps, accumulated.total, &config.tuning)
        } else {
            Outcome::Damage
        };
        // SAFETY: forwarded from the caller.
        let model = unsafe { pane.model_hash() };
        let mut hidden = false;
        match outcome {
            Outcome::Damage => {
                tracing::info!(
                    target: "controllers",
                    pane = format_args!("{:#x}", pane.address()),
                    model = format_args!("{model:#010x}"),
                    speed_mps,
                    damage,
                    total = accumulated.total,
                    "glass: a strike cracked a pane"
                );
            }
            Outcome::Shatter(cause) => {
                // SAFETY: forwarded from the caller.
                match unsafe { engine::hide(pane) } {
                    Ok(()) => {
                        hidden = true;
                        self.status.shatters += 1;
                        self.panes.retain(|p| p.address != pane.address());
                        tracing::info!(
                            target: "controllers",
                            pane = format_args!("{:#x}", pane.address()),
                            model = format_args!("{model:#010x}"),
                            speed_mps,
                            damage,
                            total = accumulated.total,
                            cause = cause.label(),
                            "glass: a strike brought a pane down"
                        );
                    }
                    Err(refusal) => {
                        if refusal == HideRefusal::SharedModel {
                            self.status.shared_models += 1;
                        }
                        tracing::info!(
                            target: "controllers",
                            pane = format_args!("{:#x}", pane.address()),
                            model = format_args!("{model:#010x}"),
                            cause = cause.label(),
                            reason = refusal.label(),
                            "glass: a pane would shatter but stays"
                        );
                    }
                }
            }
        }
        PaneStrike {
            damage,
            total: accumulated.total,
            outcome,
            hidden,
        }
    }
}

/// How long a pane's strikes are remembered after its last, seconds.
const PANE_MEMORY_S: f64 = 120.0;

/// The most panes whose strikes are remembered at once.
const MAX_PANE_RECORDS: usize = 32;

static STATE: Mutex<State> = Mutex::new(State {
    epoch: None,
    samples: [None; STRIKERS],
    limiter: Limiter::new(),
    panes: Vec::new(),
    status: GlassStatus {
        idle: None,
        sweeps: 0,
        glass_hits: 0,
        no_pane: 0,
        strikes: 0,
        shatters: 0,
        shared_models: 0,
        last: None,
        last_error: None,
    },
    warned_silent: false,
});
