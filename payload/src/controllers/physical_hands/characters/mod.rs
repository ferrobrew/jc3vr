//! Taking hold of characters with an empty hand. A grip on a character on foot throws them into
//! their ragdoll at once by the character's own hit reaction ([`topples`]), and the hand takes
//! hold of the bone of that ragdoll nearest the anatomical contact, which it drags and throws
//! through the hand body's joint; the game's own get-up takes them back once they lie still. With
//! the topple off, or for
//! a character whose state keeps it on its feet, the grip holds the hand on them instead: a yank
//! throws a character the player is hostile to off their feet, and letting go with a push shoves
//! them. A character the game protects, mission-critical or in a scripted sequence, is not
//! taken. A grip on an enclosed vehicle occupant requires an open door. A motorcycle occupant
//! needs no door; a yank requests the character's hit reaction and retains the anatomical
//! contact for the resulting ragdoll. A yank on a mounted gun's gunner takes them out of the seat
//! as a ragdoll where they sit ([`gunner_out`]), and the hand takes hold of it as of a fallen
//! character. Other enclosed-seat yanks use the vehicle-entry action, which hijacks an occupied
//! seat. The design is
//! in `docs/mod/input/character-grabs.md`, the engine side in
//! `docs/engine/gameplay/melee-and-damage.md` and `docs/engine/gameplay/vehicle-doors.md`.
//!
//! - [`config`]: the settings;
//! - [`engine`]: a character's capsule body, the player's hostility to it, and whether it is dead;
//! - [`ragdoll`]: a ragdolling character's bodies, the bone nearest a point, and the whole
//!   ragdoll's mass.
//!
//! A ragdolling character, fallen or dead, is taken by the dynamic body of its ragdoll nearest the
//! point the reach hit, whichever of the character's bodies the hit was on. The hand holds that
//! bone through the hand body's joint when it has one, the rest of the ragdoll hanging from it by
//! the ragdoll's own constraints, and carries it within the scoped ragdoll strength otherwise.
//!
//! The pickup's reach ([`crate::controllers::physical_hands::pickup::reach`]) asks [`reach`] about
//! every ray hit on a character. The pickup tick
//! ([`crate::controllers::physical_hands::pickup::tick`]) keeps a grip on a character
//! ([`Seize`]): the hand stays at its grab point on the character ([`SeizeStep::Pin`]) until it pulls
//! away past [`YANK_M`] ([`SeizeStep::Yank`]). The yank's and the shove's hit
//! reactions are dealt by the hand bodies' blows on the next frame tick
//! ([`crate::controllers::physical_hands::physics::queue_shove`]), outside the game's update.

mod acquisition;
pub mod config;
pub mod engine;
pub mod ragdoll;

use std::num::NonZeroUsize;

pub use config::CharactersConfig;
pub use engine::{GunnerOut, Protection};
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::{Character, SafeBoneIndex},
    vehicle::vehicle::DoorSlot,
};

use crate::controllers::{
    frame::Pose,
    physical_hands::{
        bodies::owner_of,
        characters::ragdoll::Ragdoll,
        contact::SurfaceHit,
        doors::engine as door_engine,
        grasp::{Reach, TargetKind},
        remote,
    },
};

/// A character a hand can take hold of.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CharacterTarget {
    pub character: NonZeroUsize,
    /// The body the reach found, which proves the character alive while the hand holds on.
    pub body: u32,
    /// The vehicle attachment and its access through an open door or an exposed seat.
    pub seated: Option<Seated>,
    /// Whether the body is one of the character's ragdoll's, which the hand holds as a loose
    /// object, rather than its capsule.
    pub ragdoll: bool,
    /// Whether the player's faction is hostile to the character.
    pub hostile: bool,
    /// The whole ragdoll's mass, kilograms, for a body of its ragdoll: what holding that one body
    /// drags. Zero for the capsule.
    pub mass_kg: f32,
}

/// Why a character the reach hit is not one a hand takes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// The grab, or the hijack for a seated character, is off.
    Off,
    /// The character is seated in a vehicle with no door open beside the hand.
    NoOpenDoor,
    /// The character is dead but not yet a ragdoll: the death animation still drives it.
    Dying,
    /// The character ragdolls but none of its ragdoll's bodies could be read.
    NoBody,
    /// The game protects the character.
    Protected(Protection),
}

impl Unavailable {
    pub const fn label(self) -> &'static str {
        match self {
            Unavailable::Off => "the grab is off",
            Unavailable::NoOpenDoor => "seated with no door open beside the hand",
            Unavailable::Dying => "dying, not yet a ragdoll",
            Unavailable::NoBody => "no body of its ragdoll could be read",
            Unavailable::Protected(protection) => protection.label(),
        }
    }
}

/// A character attached to a vehicle, either exposed or reached through an open door.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seated {
    pub vehicle: NonZeroUsize,
    /// The open door, or none for an exposed motorcycle occupant.
    pub door: Option<DoorSlot>,
    /// How far the door stands open, degrees.
    pub open_deg: f32,
}

/// How far a hand gripping a character must pull away from its grab point to yank, metres of the
/// player's hand.
pub(crate) const YANK_M: f32 = 0.2;

/// How hard a yank throws a character the player is hostile to, as a share of the knockdown blow
/// ([`HandHitsTuning::knockdown_impulse_ns`]); one and above knocks the character down.
///
/// [`HandHitsTuning::knockdown_impulse_ns`]: crate::controllers::physical_hands::physics::HandHitsTuning::knockdown_impulse_ns
pub(crate) const YANK_STRENGTH: f32 = 1.3;

/// How long after a yank the hand looks for the fallen character's ragdoll to hold, seconds of
/// game time: the fall takes as long as the game's clock says, so slow motion gives it longer.
pub(crate) const REGRIP_S: f32 = 1.0;

/// How long a hand waits for the ragdoll after a topple before retrying the ragdoll act, game
/// seconds: the hit reaction's ragdoll is under way well before this when it comes.
const RAGDOLL_RETRY_AFTER_S: f32 = 0.3;

/// The slowest hand that shoves a character it lets go of, metres per second.
pub(crate) const SHOVE_MIN_SPEED_MPS: f32 = 1.5;

/// How hard a grip throws a standing character into their ragdoll, as a share of the knockdown
/// blow: just past it, so that the character falls where the hand holds them rather than flying
/// off.
pub(crate) const TOPPLE_STRENGTH: f32 = 1.1;

/// How much harder a hand pulls a character's ragdoll than a loose object, as a multiple of the
/// pickup's hand force. The hand drags the whole body by one of its bones, and the rest hangs from
/// that bone by the ragdoll's own constraints, so the pull must move the whole body's mass rather
/// than the bone's. Scoped to ragdolls: every other hold keeps the hand's own strength.
pub(crate) const RAGDOLL_CARRY_STRENGTH: f32 = 4.0;

/// Whether a grip that took `target` throws the character into their ragdoll at once
/// ([`CharactersConfig::topple`]): a character on foot, alive, and not one whose state keeps it out
/// of its ragdoll, so that the game's own hit reaction ragdolls it.
///
/// # Safety
///
/// The game thread, outside the physics step. The target need not still be live: its body's owner
/// is checked before the character is read.
pub(crate) unsafe fn topples(target: CharacterTarget, config: &CharactersConfig) -> bool {
    if !config.topple || target.seated.is_some() || target.ragdoll {
        return false;
    }
    if unsafe { owner_of(target.body) } != Some(target.character) {
        return false;
    }
    let character = unsafe { &*(target.character.get() as *const Character) };
    !unsafe { engine::keeps_footing(character) } && !unsafe { engine::dead(character) }
}

/// The way a grip throws a character it takes into their ragdoll: along the palm's motion
/// `palm_velocity` when the hand moves at least [`SHOVE_MIN_SPEED_MPS`], or else toward the palm
/// along the touched surface's normal `normal`, held level so that the character folds toward the
/// hand rather than being lifted or driven into the ground. `None` when neither gives a way.
pub(crate) fn topple_direction(palm_velocity: Vec3, normal: Vec3) -> Option<Vec3> {
    if palm_velocity.is_finite() && palm_velocity.length() >= SHOVE_MIN_SPEED_MPS {
        return palm_velocity.try_normalize();
    }
    Vec3::new(normal.x, 0.0, normal.z)
        .try_normalize()
        .filter(|d| d.is_finite())
}

/// What a palm centred at `palm` reaches of the character `object` through the ray hit `hit` on
/// its body `body`: a character on foot, by its capsule, a ragdolling character, by the body of
/// its ragdoll nearest the hit, or a vehicle occupant accessible through an open door or an
/// exposed motorcycle seat; or why it reaches none of these. A character the game protects
/// ([`engine::protection`]) is never reached.
///
/// # Safety
///
/// The game thread, outside the physics step; `object` must be a live character from a cast this
/// tick.
pub(crate) unsafe fn reach(
    object: NonZeroUsize,
    body: u32,
    hit: &SurfaceHit,
    palm: Vec3,
    world_scale: f32,
    config: &CharactersConfig,
) -> Result<Reach, Unavailable> {
    let character = unsafe { &*(object.get() as *const Character) };
    if let Some(protection) = unsafe { engine::protection(character) } {
        return Err(Unavailable::Protected(protection));
    }
    let hostile = unsafe { engine::hostile(character) };
    let distance = hit.position.distance(palm);
    let reach =
        |reached: NonZeroUsize, body: u32, seated: Option<Seated>, mass_kg: Option<f32>| Reach {
            object: reached,
            body,
            point: hit.position,
            distance,
            normal: hit.normal,
            kind: TargetKind::Character(CharacterTarget {
                character: object,
                body,
                seated,
                ragdoll: mass_kg.is_some(),
                hostile,
                mass_kg: mass_kg.unwrap_or(0.0),
            }),
        };
    if unsafe { character.IsAttachedToVehicle() } {
        if !config.hijack {
            return Err(Unavailable::Off);
        }
        let vehicle = NonZeroUsize::new(unsafe { character.GetVehiclePtr() } as usize)
            .ok_or(Unavailable::NoOpenDoor)?;
        let seated = if unsafe { character.IsRidingMC() } {
            Seated {
                vehicle,
                door: None,
                open_deg: 0.0,
            }
        } else {
            let (door, open_deg) = unsafe { open_door_near(vehicle, palm, world_scale) }
                .ok_or(Unavailable::NoOpenDoor)?;
            Seated {
                vehicle,
                door: Some(door),
                open_deg,
            }
        };
        return Ok(reach(object, body, Some(seated), None));
    }
    if !config.grab {
        return Err(Unavailable::Off);
    }
    if let Some(ragdoll) = unsafe { Ragdoll::of(character) } {
        let bone = ragdoll
            .bone_at(body, hit.position)
            .ok_or(Unavailable::NoBody)?;
        return Ok(reach(bone.owner, bone.body, None, Some(ragdoll.mass_kg())));
    }
    if unsafe { engine::dead(character) } {
        return Err(Unavailable::Dying);
    }
    let identity = standing_body(
        body,
        unsafe { engine::proxy_body(character) },
        |body| unsafe { owner_of(body) == Some(object) },
    )
    .ok_or(Unavailable::NoBody)?;
    Ok(reach(object, identity, None, None))
}

/// The ragdoll body a hand that reached `target` at `point` takes when the character has fallen
/// since the reach: the body the reach hit when it is one of the ragdoll's, or else the bone
/// nearest `point`, as [`reach`] would have found it. `None` when the reached body no longer proves
/// the character live, the game protects it, it is not ragdolling, or none of its ragdoll's bodies
/// could be read.
///
/// # Safety
///
/// The game thread, outside the physics step; the reached body's owner is checked before the
/// character is read, which need not still be live.
pub(crate) unsafe fn fallen_reach(
    target: CharacterTarget,
    point: Vec3,
    palm: Vec3,
) -> Option<Reach> {
    if unsafe { owner_of(target.body) } != Some(target.character) {
        return None;
    }
    let character = unsafe { &*(target.character.get() as *const Character) };
    if unsafe { engine::protection(character) }.is_some() {
        return None;
    }
    let ragdoll = unsafe { Ragdoll::of(character) }?;
    let bone = ragdoll.bone_at(target.body, point)?;
    Some(Reach {
        object: bone.owner,
        body: bone.body,
        point,
        distance: point.distance(palm),
        normal: Vec3::ZERO,
        kind: TargetKind::Character(CharacterTarget {
            body: bone.body,
            seated: None,
            ragdoll: true,
            mass_kg: ragdoll.mass_kg(),
            ..target
        }),
    })
}

/// A hand holding on to a character.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seize {
    pub target: CharacterTarget,
    /// The grab point in the character's frame.
    pub grab: Vec3,
    pub bone: Option<SafeBoneIndex>,
    /// The original contact in the palm's frame, retained through toppling.
    pub in_palm: Vec3,
}

/// A hand that threw a character off their feet, by the grip's topple or a yank, waiting to take
/// hold of the ragdoll they fall as: it stays at the grab point on the character until its reach
/// finds one of the character's ragdoll's bodies, or until it gives up.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Regrip {
    pub seize: Seize,
    /// The game time left before the hand gives up, seconds.
    pub remaining_s: f32,
    /// Whether the ragdoll act was retried for a character the topple left standing.
    pub retried: bool,
}

impl Regrip {
    /// A regrip of `seize` that waits [`REGRIP_S`].
    pub fn new(seize: Seize) -> Self {
        Self {
            seize,
            remaining_s: REGRIP_S,
            retried: false,
        }
    }

    /// Whether the wait has run long enough, with no ragdoll, to retry the ragdoll act once.
    pub fn retry_due(&self) -> bool {
        !self.retried && REGRIP_S - self.remaining_s >= RAGDOLL_RETRY_AFTER_S
    }

    /// Count `dt` seconds of game time off the wait, and return whether it has run out.
    pub fn run_down(&mut self, dt: f32) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.remaining_s -= dt;
        self.expired()
    }

    /// Whether the wait has run out.
    pub fn expired(&self) -> bool {
        self.remaining_s <= 0.0
    }
}

/// What a grip on a character does on a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SeizeStep {
    /// The hand holds on at this world point.
    Pin(Vec3),
    /// The hand yanked from the grab point at `at` along `direction`, a unit vector.
    Yank { at: Vec3, direction: Vec3 },
    /// The character is gone.
    Lost,
}

impl Seize {
    pub(crate) unsafe fn point(&self) -> Option<Vec3> {
        if unsafe { owner_of(self.target.body) } != Some(self.target.character) {
            return None;
        }
        let character = unsafe { &*(self.target.character.get() as *const Character) };
        if unsafe { engine::protection(character) }.is_some() {
            return None;
        }
        unsafe { self.frame_point(character) }
    }

    /// The grab point in the world from the character's frames, once its body's owner has proved
    /// it live: the bone's frame while it is readable, or else the character's.
    unsafe fn frame_point(&self, character: &Character) -> Option<Vec3> {
        let matrix = match self.bone {
            Some(bone) => unsafe { bone_frame(character, bone) }?,
            None => unsafe { frame(self.target.character) },
        };
        let point = matrix.transform_point3(self.grab);
        point.is_finite().then_some(point)
    }

    /// The ragdoll body this grip takes once the character ragdolls: the body the reach hit when
    /// it is one of the ragdoll's, or else the bone nearest the grab point, or the palm's point
    /// when the bone's frame is no longer readable, since the hand is where it took hold. `None`
    /// when the reached body no longer proves the character live, the game protects it, or none of
    /// its ragdoll's bodies could be read.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step. The reached body's owner is checked before the
    /// character is read.
    pub(crate) unsafe fn ragdoll_reach(&self, palm: Vec3) -> Option<Reach> {
        // A ragdoll's bones need not be owned by the character object; the ragdoll itself and the
        // bone read prove the character live. Every other target still needs the owner proof.
        if !self.target.ragdoll
            && unsafe { owner_of(self.target.body) } != Some(self.target.character)
        {
            return None;
        }
        let character = unsafe { &*(self.target.character.get() as *const Character) };
        if unsafe { engine::protection(character) }.is_some() {
            return None;
        }
        let ragdoll = unsafe { Ragdoll::of(character) }?;
        let at = unsafe { self.frame_point(character) }.unwrap_or(palm);
        let bone = ragdoll.bone_at(self.target.body, at)?;
        Some(Reach {
            object: bone.owner,
            body: bone.body,
            point: at,
            distance: at.distance(palm),
            normal: Vec3::ZERO,
            kind: TargetKind::Character(CharacterTarget {
                body: bone.body,
                seated: None,
                ragdoll: true,
                mass_kg: ragdoll.mass_kg(),
                ..self.target
            }),
        })
    }

    /// The grip of a hand that took `target` at the world point `point`.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step. The queued target need not still be live.
    pub unsafe fn take(
        target: CharacterTarget,
        point: Vec3,
        palm: Pose,
        world_scale: f32,
        config: &CharactersConfig,
    ) -> Option<Self> {
        // SAFETY: the adapter checks body ownership before any character access.
        let mut seize =
            unsafe { acquisition::take(target, point, palm.position, world_scale, config) }?;
        seize.in_palm = palm.orientation.inverse() * (point - palm.position);
        seize.in_palm.is_finite().then_some(seize)
    }

    /// What the grip does with the palm at `palm`, world space, yanking at `yank_m` from the grab
    /// point.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step.
    pub unsafe fn step(&self, palm: Vec3, yank_m: f32) -> SeizeStep {
        if unsafe { owner_of(self.target.body) } != Some(self.target.character) {
            return SeizeStep::Lost;
        }
        let Some(at) = (unsafe { self.point() }) else {
            return SeizeStep::Lost;
        };
        match yank(at, palm, yank_m) {
            Some(direction) => SeizeStep::Yank { at, direction },
            None => SeizeStep::Pin(at),
        }
    }
}

/// Take the character `target` out of its gunner seat as a ragdoll, if it mans a mounted gun in a
/// vehicle ([`engine::gunner_out`]).
///
/// # Safety
///
/// The game thread, outside the physics step and the game's update. The target's current owner is
/// checked first.
pub(crate) unsafe fn gunner_out(target: CharacterTarget) -> GunnerOut {
    if unsafe { owner_of(target.body) } != Some(target.character) {
        return GunnerOut::NotAGunner;
    }
    // SAFETY: the body's owner proves the character live.
    let character = unsafe { &mut *(target.character.get() as *mut Character) };
    unsafe { engine::gunner_out(character) }
}

/// Damp the ragdoll of `character`, held by its body `body`, within itself over a step of `dt`
/// seconds ([`Ragdoll::internal_damping`] at [`HELD_RAGDOLL_DAMPING_PER_S`]): a hand drives only
/// the bone it holds, and the rest of the body, hung off that bone on soft constraints, kept its
/// momentum and orbited and thrashed about the palm (on the 14 November take, at 2 to 4 Hz a few
/// centimetres from the camera).
///
/// # Safety
///
/// The main thread, outside the physics step. The body's current owner is checked first.
pub(crate) unsafe fn damp_held_ragdoll(character: NonZeroUsize, body: u32, dt: f32) {
    if unsafe { owner_of(body) } != Some(character) {
        return;
    }
    // SAFETY: the body's owner proves the character live.
    let Some(ragdoll) = (unsafe { Ragdoll::of(&*(character.get() as *const Character)) }) else {
        return;
    };
    for (bone, impulse) in ragdoll.internal_damping(HELD_RAGDOLL_DAMPING_PER_S, dt) {
        // A bone the game has freed since the read is passed over.
        let _ = unsafe { remote::push(bone.body, bone.owner, impulse, bone.centre) };
    }
}

/// How fast a held ragdoll's motion within itself dies down, per second.
const HELD_RAGDOLL_DAMPING_PER_S: f32 = 4.0;

/// Ragdoll the character `target` that a topple left standing ([`engine::retry_ragdoll`]).
/// Returns whether an act was queued.
///
/// # Safety
///
/// The game thread, outside the physics step and the game's update. The target's current owner is
/// checked first.
pub(crate) unsafe fn retry_ragdoll(target: CharacterTarget) -> bool {
    if unsafe { owner_of(target.body) } != Some(target.character) {
        return false;
    }
    // SAFETY: the body's owner proves the character live.
    let character = unsafe { &mut *(target.character.get() as *mut Character) };
    unsafe { engine::retry_ragdoll(character) }
}

/// Whether the hand still has access to the character's current vehicle seat.
/// Enclosed seats require an open door; motorcycle occupants are exposed.
///
/// # Safety
///
/// The game thread, outside the physics step. The target's current owner is checked first.
pub(crate) unsafe fn occupant_access(
    target: CharacterTarget,
    palm: Vec3,
    world_scale: f32,
    config: &CharactersConfig,
) -> Option<Seated> {
    unsafe { acquisition::seated(target, palm, world_scale, config) }
}

/// The open door of `vehicle` nearest the palm at `palm`, within the hijack's reach and open at
/// least its angle, and how far it stands open, degrees.
///
/// # Safety
///
/// The game thread, outside the physics step; `vehicle` must be live.
unsafe fn open_door_near(
    vehicle: NonZeroUsize,
    palm: Vec3,
    world_scale: f32,
) -> Option<(DoorSlot, f32)> {
    let reach_m = HIJACK_DOOR_REACH_M * world_scale;
    unsafe { door_engine::doors(vehicle) }
        .into_iter()
        .filter(|d| !d.latched)
        .filter_map(|d| {
            let distance = d.door_box?.centre().distance(palm);
            let open_deg = unsafe { door_engine::open_angle(vehicle, d.part_index) }.to_degrees();
            (distance <= reach_m && open_deg >= HIJACK_MIN_OPEN_DEG)
                .then_some((d.slot, open_deg, distance))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(slot, open_deg, _)| (slot, open_deg))
}

/// Prefer the persistent proxy identity while retaining the hit's anatomical point.
fn standing_body(hit: u32, proxy: Option<u32>, owned: impl Fn(u32) -> bool) -> Option<u32> {
    proxy
        .filter(|&body| owned(body))
        .or_else(|| owned(hit).then_some(hit))
}

/// The character's frame: its world transform at the end of the current simulation step.
///
/// # Safety
///
/// `character` must be a live character.
unsafe fn bone_frame(character: &Character, bone: SafeBoneIndex) -> Option<Mat4> {
    let controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() }?;
    if !controller.m_Initialized
        || controller.m_SkeletonResourceHandle.is_null()
        || controller.m_Skeleton.is_null()
        || controller.m_PoseProducer.is_null()
        || unsafe { character.GetSafeIndex(bone) } == 0
    {
        return None;
    }
    let mut matrix = Mat4::NAN.into();
    unsafe { character.GetWorldMatrixForBone(bone, &mut matrix) };
    let matrix = Mat4::from(matrix);
    (matrix.is_finite() && matrix.determinant().abs() > 1e-6).then_some(matrix)
}

unsafe fn frame(character: NonZeroUsize) -> Mat4 {
    let character = unsafe { &*(character.get() as *const Character) };
    Mat4::from(character.m_WorldMatrixT1)
}

/// The way a palm at `palm` yanked from a grab point at `at`, once it is `yank_m` or more away.
fn yank(at: Vec3, palm: Vec3, yank_m: f32) -> Option<Vec3> {
    let pulled = palm - at;
    (pulled.is_finite() && pulled.length() >= yank_m.max(1e-3))
        .then(|| pulled.normalize_or_zero())
        .filter(|d| *d != Vec3::ZERO)
}

/// How far open a door beside the hand must stand for a hand to reach in, degrees.
const HIJACK_MIN_OPEN_DEG: f32 = 25.0;

/// How far from the palm an open door may be for the hand to reach in through it, metres of the
/// player's hand.
const HIJACK_DOOR_REACH_M: f32 = 1.2;

#[cfg(test)]
mod tests {
    use super::*;

    fn target(character: usize, ragdoll: bool) -> CharacterTarget {
        CharacterTarget {
            character: NonZeroUsize::new(character).unwrap(),
            body: 7,
            seated: None,
            ragdoll,
            hostile: true,
            mass_kg: if ragdoll { 70.0 } else { 0.0 },
        }
    }

    #[test]
    fn an_animated_limb_hit_uses_the_live_proxy_without_losing_the_hit_point() {
        assert_eq!(
            standing_body(9, Some(7), |body| [7, 9].contains(&body)),
            Some(7)
        );
        assert_eq!(standing_body(9, Some(7), |body| body == 9), Some(9));
        assert_eq!(standing_body(9, None, |body| body == 9), Some(9));
        assert_eq!(standing_body(9, Some(7), |_| false), None);
    }

    #[test]
    fn a_regrip_retains_the_acquired_anatomical_anchor() {
        let seize = Seize {
            target: target(32, false),
            grab: Vec3::new(0.02, -0.03, 0.01),
            bone: Some(SafeBoneIndex::LEFT_HAND),
            in_palm: Vec3::new(0.01, 0.02, -0.03),
        };
        let mut regrip = Regrip::new(seize);
        assert!(!regrip.run_down(REGRIP_S * 0.5));
        assert_eq!(regrip.seize, seize);
    }

    #[test]
    fn a_regrip_waits_on_the_game_clock() {
        let mut regrip = Regrip::new(Seize {
            target: target(32, false),
            grab: Vec3::Y,
            bone: None,
            in_palm: Vec3::ZERO,
        });
        // A second of real time at a quarter speed is a quarter of the wait.
        for _ in 0..60 {
            assert!(!regrip.run_down(0.25 / 60.0));
        }
        assert!((regrip.remaining_s - (REGRIP_S - 0.25)).abs() < 1e-4);
        // A paused clock, or a broken time step, waits on.
        assert!(!regrip.run_down(0.0));
        assert!(!regrip.run_down(f32::NAN));
        assert!(!regrip.run_down(-1.0));
        assert!(regrip.run_down(REGRIP_S));
    }

    #[test]
    fn a_topple_follows_a_moving_hand_and_otherwise_folds_toward_it() {
        let moving = topple_direction(Vec3::new(0.0, 0.0, -2.0), Vec3::X).unwrap();
        assert!((moving - Vec3::NEG_Z).length() < 1e-5);
        // A still hand pulls toward itself, level, whatever the surface's slope.
        let still = topple_direction(Vec3::new(0.1, 0.0, 0.0), Vec3::new(0.6, 0.8, 0.0)).unwrap();
        assert!((still - Vec3::X).length() < 1e-5);
        assert_eq!(topple_direction(Vec3::ZERO, Vec3::Y), None);
        assert_eq!(topple_direction(Vec3::NAN, Vec3::NAN), None);
    }

    #[test]
    fn a_hand_yanks_once_it_pulls_far_enough() {
        let at = Vec3::new(1.0, 1.4, 0.0);
        assert_eq!(yank(at, at + Vec3::X * 0.1, 0.2), None);
        let direction = yank(at, at + Vec3::new(-0.3, 0.1, 0.0), 0.2).unwrap();
        assert!(direction.x < -0.9 && (direction.length() - 1.0).abs() < 1e-5);
        assert_eq!(yank(at, Vec3::NAN, 0.2), None);
    }
}
