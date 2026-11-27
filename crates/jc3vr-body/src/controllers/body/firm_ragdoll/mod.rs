//! The firm ragdoll: Rico's body hung from the physics for grapple swings, hangs, and the canopy.
//!
//! Swinging on the grapple lock, hanging from a hook, and riding the canopy, Rico's animated legs
//! ignore the swing: they stay in the clip's pose however the line throws the body, and under the
//! canopy they hang stiff. The game's ragdoll has presets that keep the upper body keyframed to
//! the animated pose and drive the hips and legs as dynamic bodies with constraint motors toward
//! it: the grapple hang's ([`RagdollParamIdSymbolTable::GRAPPLE_HANG`]) and the canopy's, which the
//! game already applies under the canopy. A preset with keyframed bones keeps the ragdoll under the
//! owner's control, so the character does not ragdoll and its animation graph never sees one.
//!
//! The firm mode wants the grapple hang's preset while the lock holds the line, and keeps the
//! canopy's own preset under the canopy. The character's preset id is written with its sync flag
//! (as the game's canopy update writes it), so the next pre-physics pass makes the ragdoll, and the
//! ragdoll's blend weight, which nothing in that sync raises, is written each tick just before the
//! post-physics pass blends the simulated pose into the animated one
//! ([`Character::PostPhysicsUpdateRagdoll`]). The weight ramps in over the body tuning's
//! `firm_ragdoll.blend_in_s` and out over its `blend_out_s`, and the saved preset id comes back
//! once it is out. The blend lands before the HumanIK solve, so the head, the hands, and the chest
//! stay the mod's; the procedural legs stand down while the mode is in, so the physics owns them.
//!
//! The ragdoll's pre-physics step reads its muscle target from the raw animation, before the mod's
//! solve. [`feed`] builds the producer to hand it instead ([`stand_in`]), whose current pose is the
//! last tick's final pose above the hips and the IK body's legs below ([`target`]), from the bones
//! and legs the IK body publishes at each gate while the mode is in ([`publish_source`]). A detour
//! on the local player's ragdoll's pre-physics step ([`RagdollInstance::PrePhysicsUpdate`]) hands
//! it over.
//!
//! [`RagdollInstance::PrePhysicsUpdate`]: jc3gi::animation::ragdoll::RagdollInstance::PrePhysicsUpdate
//! [`RagdollParamIdSymbolTable::GRAPPLE_HANG`]: jc3gi::animation::symbol_table::RagdollParamIdSymbolTable::GRAPPLE_HANG
//! [`Character::PostPhysicsUpdateRagdoll`]: jc3gi::character::character::Character::PostPhysicsUpdateRagdoll

mod stand_in;
pub(crate) mod target;

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::{
    animation::{
        pose_producer::PoseProducer, ragdoll::RagdollInstance,
        symbol_table::RagdollParamIdSymbolTable,
    },
    character::character::Character,
    types::math::Matrix4,
};
use jc3vr_core::{game_tick::GameTick, published::Stamp};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

pub(crate) use crate::controllers::body::firm_ragdoll::stand_in::TargetReport;
use crate::{
    controllers::{
        body::{
            firm_ragdoll::{
                stand_in::StandIn,
                target::{IkLegs, TargetBones},
            },
            tuning::BODY,
        },
        physics::local_player::LocalPlayer,
    },
    tuning::FRESHNESS,
};

pub fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&PRE_PHYSICS_UPDATE_BINDER)
        .with_static_binder(&POST_PHYSICS_UPDATE_RAGDOLL_BINDER)
}

/// What the firm mode is asked for on an input tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Want {
    /// Nothing: the animation owns the body.
    Off,
    /// The grapple lock holds the line: the grapple hang's preset.
    Line,
    /// Under the canopy: the canopy's own preset, held at full weight.
    Canopy,
}

/// The firm mode's readout, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct FirmStatus {
    /// The blend weight written last.
    pub weight: f32,
    /// Whether the mode is in, ramping in or holding.
    pub active: bool,
    /// The preset id the mode is on, when it is in.
    pub preset: Option<u32>,
    /// How many ticks the muscle target was composed, and how many could not be.
    pub targets: u64,
    pub refused: u64,
    /// Whether the last target took the IK body's legs rather than the clip's.
    pub ik_legs: bool,
    /// How far the last target stood from the clip.
    pub report: TargetReport,
}

/// The firm mode's readout.
pub fn status() -> FirmStatus {
    STATE.lock().status
}

/// Whether the physics owns the legs this tick: the procedural legs stand down.
pub(crate) fn owns_legs() -> bool {
    OWNS_LEGS.load(Ordering::Relaxed)
}

/// Publish the bones the muscle target is composed on and the IK body's legs, from the solve gate
/// while the physics owns the legs.
pub(crate) fn publish_source(bones: TargetBones, legs: Option<IkLegs>, now: Stamp) {
    *SOURCE.lock() = Some(Source {
        bones,
        legs,
        at: now,
    });
}

/// The producer to hand the local player's ragdoll's pre-physics step in place of `producer`: the
/// stand-in holding the muscle target while the firm mode is in and the IK body has published its
/// source, and `producer` itself otherwise.
///
/// # Safety
///
/// `character` must be the live local player and `producer` its animation controller's producer,
/// between the producer's buffer exchange and the end of the ragdoll's pre-physics step, on the
/// thread running it. The returned producer is valid until the next call.
pub(crate) unsafe fn feed(character: &Character, producer: *mut PoseProducer) -> *mut PoseProducer {
    let in_mode = {
        let state = STATE.lock();
        state
            .mode
            .preset()
            .is_some_and(|preset| character.m_CurrentRagdollParamsID == preset)
    };
    let source = SOURCE.lock().filter(|source| {
        FRESHNESS
            .get()
            .body
            .firm_ragdoll_source_ms
            .window()
            .holds(source.at, Stamp::now())
    });
    let (Some(source), true, Some(live)) = (source, in_mode, unsafe { producer.as_ref() }) else {
        return producer;
    };
    let fed = unsafe { STAND_IN.lock().refresh(live, source.bones, source.legs) };
    let mut state = STATE.lock();
    match fed {
        Some((stand_in, report)) => {
            if state.status.targets == state.logged_targets {
                tracing::info!(
                    target: "controllers",
                    ik_legs = source.legs.is_some(),
                    upper_shift_m = report.upper_shift_m,
                    left_thigh_turn_deg = report.thigh_turn_deg[0],
                    right_thigh_turn_deg = report.thigh_turn_deg[1],
                    "firm ragdoll: the muscle target is the drawn body",
                );
            }
            state.status.targets += 1;
            state.status.ik_legs = source.legs.is_some();
            state.status.report = report;
            stand_in
        }
        None => {
            state.status.refused += 1;
            tracing::debug!(target: "controllers", "firm ragdoll: the muscle target could not be composed");
            producer
        }
    }
}

/// Step the firm mode for the local player on an input tick, asked for `want`, in a game tick
/// before the world update.
pub fn input_tick(game: &GameTick<'_>, want: Want) {
    if let Some(mut player) = LocalPlayer::get(game) {
        tick(&mut player, want);
    }
}

/// Step the firm mode for the local `player`.
fn tick(player: &mut LocalPlayer<'_>, want: Want) {
    let character = player.character_mut();
    let now = Stamp::now();
    let mut state = STATE.lock();
    let current = character.m_CurrentRagdollParamsID;
    match (state.mode, want) {
        (Mode::Off, Want::Off) => {
            state.logged_targets = state.status.targets;
        }
        (Mode::Off, Want::Line) => {
            let Some(hang) = (unsafe { preset_id(RagdollParamIdSymbolTable::GRAPPLE_HANG) }) else {
                return;
            };
            state.saved = Some(current);
            if current != hang {
                unsafe { request_preset(character, hang) };
            }
            state.mode = Mode::In {
                preset: hang,
                since: now,
            };
            tracing::info!(target: "controllers", "firm ragdoll: in for the grapple line");
        }
        (Mode::Off, Want::Canopy) => {
            // The canopy's preset is the game's; the mode only holds its weight.
            let Some(canopy) = (unsafe { preset_id(RagdollParamIdSymbolTable::PARACHUTING) })
            else {
                return;
            };
            if current == canopy {
                state.saved = None;
                state.mode = Mode::In {
                    preset: canopy,
                    since: now,
                };
                tracing::info!(target: "controllers", "firm ragdoll: in for the canopy");
            }
        }
        (Mode::In { preset, .. }, Want::Off) => {
            state.mode = Mode::Out {
                preset,
                since: now,
                from: state.status.weight,
            };
            tracing::info!(target: "controllers", "firm ragdoll: out");
        }
        (Mode::In { preset, .. }, _) => {
            // A graph state's own preset write (its exit clears the id, which destroys the
            // ragdoll) is taken back while the mode still wants its preset.
            if current != preset && state.saved.is_some() {
                unsafe { request_preset(character, preset) };
            }
        }
        (Mode::Out { since, .. }, Want::Off) => {
            if now.saturating_duration_since(since).as_secs_f32()
                >= BODY.get().firm_ragdoll.blend_out_s
            {
                if let Some(saved) = state.saved.take()
                    && character.m_CurrentRagdollParamsID != saved
                {
                    unsafe { request_preset(character, saved) };
                }
                state.mode = Mode::Off;
            }
        }
        (Mode::Out { preset, .. }, _) => {
            state.mode = Mode::In { preset, since: now };
        }
    }
    let weight = state.mode.weight(now);
    state.status.weight = weight;
    state.status.active = matches!(state.mode, Mode::In { .. });
    state.status.preset = state.mode.preset();
    OWNS_LEGS.store(weight > 0.0, Ordering::Relaxed);
}

#[detour(address = jc3gi::animation::ragdoll::RagdollInstance::PrePhysicsUpdate_ADDRESS)]
fn pre_physics_update(
    instance: *mut RagdollInstance,
    wanted_world_matrix: *const Matrix4,
    pose: *mut PoseProducer,
    dt: f32,
) {
    // SAFETY: the local player is read for identity only; the owner steps its own live instance
    // with its character's producer, between the producer's exchange and the physics.
    let pose = match unsafe { Character::GetLocalPlayerCharacter().as_ref() } {
        Some(player) if !instance.is_null() && std::ptr::eq(player.m_Ragdoll.px, instance) => unsafe {
            feed(player, pose)
        },
        _ => pose,
    };
    PRE_PHYSICS_UPDATE
        .get()
        .unwrap()
        .call(instance, wanted_world_matrix, pose, dt);
}

#[detour(address = jc3gi::character::character::Character::PostPhysicsUpdateRagdoll_ADDRESS)]
fn post_physics_update_ragdoll(character: *mut Character, dt: f32) {
    // SAFETY: the post-physics pass passes its live character.
    if let Some(character) = unsafe { character.as_mut() }.filter(|c| c.m_IsLocalCharacter) {
        let (weight, preset) = {
            let state = STATE.lock();
            (state.mode.weight(Stamp::now()), state.mode.preset())
        };
        if let Some(preset) = preset
            && character.m_CurrentRagdollParamsID == preset
            && let Some(instance) = unsafe { character.m_Ragdoll.px.as_mut() }
        {
            instance.m_BlendWeight = weight;
        }
    }
    POST_PHYSICS_UPDATE_RAGDOLL
        .get()
        .unwrap()
        .call(character, dt);
}

/// The runtime id of the ragdoll preset `name`.
///
/// # Safety
///
/// The game thread, once the character manager's ragdoll description has loaded.
unsafe fn preset_id(name: &std::ffi::CStr) -> Option<u32> {
    let table = unsafe { RagdollParamIdSymbolTable::get() }?;
    let id = unsafe { table.string_to_id(name.as_ptr().cast()) };
    u32::try_from(id).ok().filter(|id| *id != 0)
}

/// Ask for the ragdoll preset `id`, at once, as the game's canopy update asks for its presets: the
/// next pre-physics pass makes, re-params, or (for zero) destroys the ragdoll.
///
/// # Safety
///
/// `character` is live, on the game thread outside the character's passes.
unsafe fn request_preset(character: &mut Character, id: u32) {
    character.m_CurrentRagdollParamsID = id;
    character.m_RagdollParameterBlendTime = 0.0;
    character.m_RagdollParameterReset = false;
    character.m_SyncRagdollState = true;
}

/// The firm mode's phase.
#[derive(Debug, Copy, Clone, PartialEq)]
enum Mode {
    Off,
    /// In on `preset`, ramping in since `since`.
    In {
        preset: u32,
        since: Stamp,
    },
    /// Ramping out of `preset` from the weight `from` since `since`.
    Out {
        preset: u32,
        since: Stamp,
        from: f32,
    },
}

impl Mode {
    fn weight(self, now: Stamp) -> f32 {
        match self {
            Mode::Off => 0.0,
            Mode::In { since, .. } => {
                let blend_in_s = BODY.get().firm_ragdoll.blend_in_s;
                (now.saturating_duration_since(since).as_secs_f32() / blend_in_s).clamp(0.0, 1.0)
            }
            Mode::Out { since, from, .. } => {
                let blend_out_s = BODY.get().firm_ragdoll.blend_out_s;
                let out = now.saturating_duration_since(since).as_secs_f32() / blend_out_s;
                (from * (1.0 - out)).clamp(0.0, 1.0)
            }
        }
    }

    fn preset(self) -> Option<u32> {
        match self {
            Mode::Off => None,
            Mode::In { preset, .. } | Mode::Out { preset, .. } => Some(preset),
        }
    }
}

struct State {
    mode: Mode,
    /// The preset id to restore once the mode is out, when the mode wrote one of its own.
    saved: Option<u32>,
    status: FirmStatus,
    /// The target count when the mode last came in, so the first target of each spell is logged.
    logged_targets: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    mode: Mode::Off,
    saved: None,
    status: FirmStatus {
        weight: 0.0,
        active: false,
        preset: None,
        targets: 0,
        refused: 0,
        ik_legs: false,
        report: TargetReport {
            upper_shift_m: 0.0,
            thigh_turn_deg: [0.0; 2],
        },
    },
    logged_targets: 0,
});

/// What the IK body publishes for the muscle target.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Source {
    bones: TargetBones,
    legs: Option<IkLegs>,
    at: Stamp,
}

static SOURCE: Mutex<Option<Source>> = Mutex::new(None);

static STAND_IN: Mutex<StandIn> = Mutex::new(StandIn::new());

static OWNS_LEGS: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::controllers::body::tuning::FirmRagdollTuning;

    #[test]
    fn the_weight_ramps_in_and_out_from_where_it_was() {
        let start = Stamp::now();
        let at = |s: f32| start + Duration::from_secs_f32(s);
        let FirmRagdollTuning {
            blend_in_s,
            blend_out_s,
        } = BODY.get().firm_ragdoll;
        let entering = Mode::In {
            preset: 7,
            since: start,
        };
        assert_eq!(entering.weight(start), 0.0);
        assert!((entering.weight(at(blend_in_s * 0.5)) - 0.5).abs() < 1e-3);
        assert_eq!(entering.weight(at(1.0)), 1.0);
        let leaving = Mode::Out {
            preset: 7,
            since: start,
            from: 0.6,
        };
        assert!((leaving.weight(start) - 0.6).abs() < 1e-6);
        assert_eq!(leaving.weight(at(blend_out_s)), 0.0);
        assert_eq!(Mode::Off.weight(start), 0.0);
        assert_eq!(leaving.preset(), Some(7));
    }
}
