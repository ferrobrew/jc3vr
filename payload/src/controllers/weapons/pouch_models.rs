//! The game's models of what the body's pouches hold, drawn on the pouches: a grenade on the grenade
//! pouch, a C4 charge on the C4 pouch, and the held gun's magazine on the magazine pouch, so the
//! player sees on Rico's body what a hand reaching there takes. The magazine a hand takes from the
//! pouch is drawn in that hand the same way ([`hand_tick`]).
//!
//! Each model is an instance the payload owns, made from a model the game already holds for the
//! item's lifetime ([`ModelInstanceManager::CreateInstanceFromModel`]): the grenade weapon's
//! projectile model ([`ThrownGrenadeWeapon::m_PhysicalProjectileModel`]), the C4 weapon's in-hand
//! model ([`PlantedExplosiveWeapon::m_InactiveModelInstance`]), and the gun's spare magazine prop's
//! model ([`WeaponData::m_MagazineObject`], [`RigidObject::m_Model`]). The instance keeps its own
//! reference to the model, and the engine's cull draws it once it is in the world, with no physics
//! and no game object behind it ([`owned`]). A gun's magazine on the belt and in the hand are both
//! such instances of its spare magazine's model, and either one keeps that model for the weapon
//! while the game respawns the spare after a magazine is dropped.
//!
//! The input tick ([`input_tick`]) places each model where its pouch is, in the character's model
//! space, and keeps the last two ticks' placements. The render frame ([`render_frame`]) draws it
//! between them, carried by the character's interpolated world transform at the frame's
//! interpolation fraction, so it rides the body as the body is drawn rather than a tick behind
//! it ([render-time-anchoring.md](../../../../docs/mod/rendering/render-time-anchoring.md)). The
//! magazine in the hand is drawn where the reload draws the round in the hand at the same fraction
//! ([`reload::held_round_matrix`]), on the hand as it is drawn.
//!
//! The instances are created, shown, hidden, and released on the game thread before the world
//! update, as the engine requires. A model whose pouch is not offered is hidden, as is the hand's
//! magazine once the hand no longer holds it; every instance is released outside gameplay, when
//! its source model changes (another gun's magazine), and when the payload unloads.

use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use glam::{EulerRot, Mat4, Quat, Vec3};
use jc3gi::{
    character::{
        character::{Character, SafeBoneIndex},
        inventory::WeaponSlot,
    },
    equipment::weapon::{PlantedExplosiveWeapon, ThrownGrenadeWeapon, WeaponBase},
    graphics_engine::model_instance::Model,
    types::shared_ptr::SharedPtr,
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::{mirror, rig::frames::Frames},
        frame::{Hand, Pose},
        hands::{grip, late::lerp_transform},
        weapons::{
            config::PouchModelsConfig,
            engine,
            pouch_models::{
                attachments::{RenderSample, Sample, SampleId, placement},
                owned::{OwnedModel, magazine_source, without_magazine_prop},
            },
            reload,
        },
    },
    hooks,
};

pub(crate) mod attachments;
pub(crate) mod owned;

/// What a pouch holds, drawn on it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Prop {
    Grenade,
    Charge,
    Magazine,
}

impl Prop {
    pub const ALL: [Prop; 3] = [Prop::Grenade, Prop::Charge, Prop::Magazine];

    pub const fn label(self) -> &'static str {
        match self {
            Prop::Grenade => "grenade",
            Prop::Charge => "C4 charge",
            Prop::Magazine => "magazine",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Prop::Grenade => 0,
            Prop::Charge => 1,
            Prop::Magazine => 2,
        }
    }

    /// Where the prop's model sits on its pouch.
    pub const fn placement(self) -> ModelPlacement {
        match self {
            Prop::Charge => CHARGE_PLACEMENT,
            Prop::Grenade | Prop::Magazine => MODEL_PLACEMENT,
        }
    }

    fn bone(self) -> SafeBoneIndex {
        match self {
            Self::Magazine => SafeBoneIndex::HIPS,
            Self::Grenade | Self::Charge => SafeBoneIndex::STERNUM,
        }
    }
}

/// Where the grenade's and the magazine's models sit on their pouches: at the pouch's centre,
/// square to the girdle.
pub const MODEL_PLACEMENT: ModelPlacement = ModelPlacement {
    offset_m: Vec3::ZERO,
    rotation_deg: Vec3::ZERO,
};

/// Where the C4 charge's model sits on its pouch: at the pouch's centre, pitched down a quarter
/// turn, so that the charge, whose in-hand model lies face up, lies flat on the chest with its face
/// forward rather than standing out of it.
pub const CHARGE_PLACEMENT: ModelPlacement = ModelPlacement {
    offset_m: Vec3::ZERO,
    rotation_deg: Vec3::new(-90.0, 0.0, 0.0),
};

/// Where a model sits relative to its pouch's centre, in the frame of the girdle the pouch rides
/// (+X right, +Y up the spine, +Z back), given for the right side and mirrored across the body's
/// midline for the left.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ModelPlacement {
    /// Metres.
    pub offset_m: Vec3,
    /// The model's turn from the girdle's axes: about X, then Y, then Z, degrees.
    pub rotation_deg: Vec3,
}

/// A pouch to draw a model on this tick: its centre and the orientation of the girdle it rides, in
/// the cockpit frame, the side of the body it is on, and for the magazine, the gun whose magazine
/// it holds.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ShownPouch {
    pub prop: Prop,
    pub pouch: Pose,
    pub side: Hand,
    /// The weapon whose magazine the magazine pouch holds (a `WeaponBase` address).
    pub weapon: Option<usize>,
}

/// The pouches' models' readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PouchModelsStatus {
    /// Whether each prop's model is drawn, by [`Prop::ALL`]'s order.
    pub shown: [bool; 3],
    /// Whether a magazine is drawn in a hand.
    pub hand: bool,
    /// Instances created since the payload loaded.
    pub created: u64,
    /// Render frames that drew at least one model.
    pub drawn: u64,
}

pub fn status() -> PouchModelsStatus {
    STATE.lock().status
}

/// The model's transform in the frame of the pouch it sits on: `placement`'s offset and turn,
/// mirrored across the body's midline for the left side.
pub fn on_pouch(placement: &ModelPlacement, side: Hand) -> Mat4 {
    let (offset, rotation) = match side {
        Hand::Right => (placement.offset_m, placement.rotation_deg),
        Hand::Left => (
            placement.offset_m * Vec3::new(-1.0, 1.0, 1.0),
            placement.rotation_deg * Vec3::new(1.0, -1.0, -1.0),
        ),
    };
    let r = rotation * (std::f32::consts::PI / 180.0);
    Mat4::from_rotation_translation(Quat::from_euler(EulerRot::XYZ, r.x, r.y, r.z), offset)
}

/// The displayed model origin in the cockpit frame, including its configured model-space offset.
pub(crate) fn display_centre(
    pouch: Pose,
    placement: &ModelPlacement,
    side: Hand,
    scale: f32,
) -> Vec3 {
    pouch.position + pouch.orientation * on_pouch(placement, side).w_axis.truncate() / scale
}

/// The model's world matrix at the interpolation fraction `dtf`: its placement in the character's
/// model space between the last two ticks', carried by the character's world transform between its
/// last two ticks'.
pub fn render_matrix(character: [Mat4; 2], local: [Mat4; 2], dtf: f32) -> Option<Mat4> {
    let matrix = render_transform(character, dtf) * render_transform(local, dtf);
    matrix.is_finite().then_some(matrix)
}

/// Carry a bone-relative pouch with the current solved posture and the rendered root transform.
pub(crate) fn bone_render_matrix(
    character: [Mat4; 2],
    bone: Mat4,
    local: Mat4,
    dtf: f32,
) -> Option<Mat4> {
    let matrix = render_character(character, dtf)? * bone * local;
    matrix.is_finite().then_some(matrix)
}

/// The character transform shared by the render placement and its diagnostic plane.
pub(crate) fn render_character(character: [Mat4; 2], dtf: f32) -> Option<Mat4> {
    let matrix = render_transform(character, dtf);
    matrix.is_finite().then_some(matrix)
}

/// Place the pouches' models for the tick: show each prop in `shown` on its pouch, carried into the
/// character's model space by `frames`, and hide the rest.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn input_tick(
    config: &PouchModelsConfig,
    frames: Option<Frames>,
    shown: &[ShownPouch],
) {
    let owner = if config.enabled && hooks::in_gameplay() {
        unsafe { engine::local_player() }
    } else {
        None
    };
    let mut state = STATE.lock();
    let (Some(owner), Some(frames)) = (owner, frames) else {
        // Outside gameplay the world the instances live in may be torn down, so they go with it.
        unsafe { state.release_all() };
        return;
    };
    ensure_cleanup();
    let owner_id = owner as *const Character as usize;
    if state.owner != Some(owner_id) {
        unsafe { state.release_all() };
        state.owner = Some(owner_id);
    }
    // The reload asks for the magazine in the hand on every tick it holds one, after this one; a
    // tick it did not ask on (the reload stood down) hides it.
    if state.hand_sequence != state.input_sequence {
        unsafe { state.hand.hide() };
        state.status.hand = false;
    }
    state.input_sequence = state.input_sequence.wrapping_add(1);
    let mut attachments = Sample {
        id: SampleId {
            owner: owner_id,
            epoch: state.epoch,
            sequence: state.input_sequence,
        },
        at: Instant::now(),
        character: frames.character,
        attachments: Vec::new(),
    };
    let hand = state.hand;
    for prop in Prop::ALL {
        let wanted = shown.iter().find(|s| s.prop == prop);
        let slot = &mut state.slots[prop.index()];
        if let Some(wanted) = wanted
            && slot.owned.weapon() != wanted.weapon
        {
            unsafe { slot.release() };
            unsafe { slot.owned.retarget(wanted.weapon) };
        }
        // The owned instances keep their model while the game respawns a dropped magazine's prop.
        let model =
            wanted.and_then(|s| unsafe { source_model(owner, prop, s.weapon, &slot.owned, &hand) });
        let source = model.map(|model| unsafe { (*model).px } as usize);
        let mut attachment = placement(prop, wanted, source, &frames, &prop.placement());
        let local = attachment.local.ok();
        let previous_source = slot.owned.model();
        let previously_visible = slot.owned.visible();
        let created = match (model, local) {
            (Some(model), Some(local)) => unsafe {
                slot.show(model, local, Mat4::from(owner.m_WorldMatrixT1))
            },
            _ => {
                unsafe { slot.hide() };
                false
            }
        };
        slot.bone_local = local.and_then(|local| {
            // SAFETY: the game stage owns this live character observation.
            let bone = unsafe { grip::bone_model_pose(owner, prop.bone()) }?;
            let bone = Mat4::from_rotation_translation(bone.orientation, bone.position);
            let local = bone.inverse() * local;
            local.is_finite().then_some(local)
        });
        let visible = slot.owned.visible();
        if previously_visible != visible || previous_source != slot.owned.model() {
            mirror::clear_pouch_render();
        }
        state.status.created += u64::from(created);
        state.status.shown[prop.index()] = visible;
        attachment.visible = visible;
        attachments.attachments.push(attachment);
    }
    state.attachments = Some(attachments);
}

/// Draw the magazine of the gun at `weapon` in the hand holding a magazine from its pouch, or with
/// `None`, in no hand. Returns whether the instance draws it; when it does not (the pouches'
/// models off, or no magazine model for the gun), the reload draws the round in the hand with the
/// gun's own part, if it has one. The reload places it each frame ([`reload::held_round_matrix`]).
///
/// # Safety
///
/// The game thread, before the world update; `weapon` is a live weapon of the local player.
pub(crate) unsafe fn hand_tick(weapon: Option<usize>) -> bool {
    let mut state = STATE.lock();
    let owner = state
        .owner
        .and_then(|_| unsafe { engine::local_player() })
        .filter(|owner| state.owner == Some(*owner as *const Character as usize));
    let (Some(owner), Some(weapon)) = (owner, weapon) else {
        unsafe { state.hand.hide() };
        state.status.hand = false;
        return false;
    };
    state.hand_sequence = state.input_sequence;
    unsafe { state.hand.retarget(Some(weapon)) };
    let belt = state.slots[Prop::Magazine.index()].owned;
    let Some(model) = (unsafe { magazine_source(weapon, [&state.hand, &belt]) }) else {
        unsafe { state.hand.hide() };
        state.status.hand = false;
        if let Some(name_hash) = unsafe { without_magazine_prop(weapon) }
            && state.without_prop.insert(name_hash)
        {
            tracing::info!(
                target: "controllers",
                weapon = format_args!("{name_hash:#010x}"),
                "pouch models: the held gun has no spare magazine model to draw in the hand"
            );
        }
        return false;
    };
    let created = unsafe { state.hand.show(model, Mat4::from(owner.m_WorldMatrixT1)) };
    state.status.created += u64::from(created == Some(true));
    state.status.hand = created.is_some();
    state.status.hand
}

/// Whether a magazine of the gun at `weapon` would be drawn in the hand by an instance of its
/// magazine model ([`hand_tick`]).
///
/// # Safety
///
/// As [`hand_tick`].
pub(crate) unsafe fn hand_draws(weapon: usize) -> bool {
    let state = STATE.lock();
    state.owner.is_some()
        && unsafe {
            magazine_source(
                weapon,
                [&state.hand, &state.slots[Prop::Magazine.index()].owned],
            )
        }
        .is_some()
}

/// Draw the pouches' models at the frame's interpolation fraction `dtf`. Called once per rendered
/// frame, before the engine's render update culls and draws the frame.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn render_frame(dtf: f32) {
    mirror::clear_pouch_render();
    let mut state = STATE.lock();
    state.render_sequence = state.render_sequence.wrapping_add(1);
    if state.attachments.is_none() {
        return;
    }
    let Some(owner) = (unsafe { engine::local_player() }) else {
        unsafe { state.release_all() };
        return;
    };
    let owner_id = owner as *const Character as usize;
    let character = [
        Mat4::from(owner.m_WorldMatrixT0),
        Mat4::from(owner.m_WorldMatrixT1),
    ];
    if state.owner != Some(owner_id) {
        unsafe { state.release_all() };
        return;
    }
    let id = SampleId {
        owner: owner_id,
        epoch: state.epoch,
        sequence: state.render_sequence,
    };
    let mut sample = state.attachments.as_ref().and_then(|input| {
        RenderSample::new(id, input, render_character(character, dtf)?, Instant::now())
    });
    let mut drew = false;
    for prop in Prop::ALL {
        let slot = &state.slots[prop.index()];
        if !slot.owned.visible() {
            continue;
        }
        let Some(local) = slot.local else {
            continue;
        };
        let bone = slot.bone_local.and_then(|local| {
            // SAFETY: this is the live local character, observed on the render thread.
            let bone = unsafe { grip::bone_model_pose(owner, prop.bone()) }?;
            let bone = Mat4::from_rotation_translation(bone.orientation, bone.position);
            bone_render_matrix(character, bone, local, dtf)
        });
        let Some(matrix) = bone.or_else(|| render_matrix(character, local, dtf)) else {
            continue;
        };
        // SAFETY: the game thread, outside the world update.
        if !unsafe { slot.owned.push(matrix) } {
            continue;
        }
        if let (Some(sample), Some(model)) = (&mut sample, slot.owned.model()) {
            sample.applied(prop, model, matrix);
        }
        drew = true;
    }
    // The magazine in the hand rides the hand as it is drawn, where the reload draws its round.
    if let Some(weapon) = state.hand.weapon().filter(|_| state.hand.visible())
        && let Some(matrix) = reload::held_round_matrix(weapon, dtf)
    {
        // SAFETY: the game thread, outside the world update.
        drew |= unsafe { state.hand.push(matrix) };
    }
    state.status.drawn += u64::from(drew);
    drop(state);
    if let Some(sample) = sample {
        mirror::pouches_rendered(&sample);
    }
}

/// Release every instance.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn stand_down() {
    unsafe { STATE.lock().release_all() };
}

fn render_transform(transforms: [Mat4; 2], dtf: f32) -> Mat4 {
    let t = if dtf.is_finite() {
        dtf.clamp(0.0, 1.0)
    } else {
        1.0
    };
    lerp_transform(transforms[0], transforms[1], t)
}

/// One prop's instance and its placements.
#[derive(Debug, Copy, Clone, Default)]
struct Slot {
    owned: OwnedModel,
    bone_local: Option<Mat4>,
    /// The placement in the character's model space on the last two ticks, older first.
    local: Option<[Mat4; 2]>,
}

impl Slot {
    const EMPTY: Slot = Slot {
        owned: OwnedModel::EMPTY,
        bone_local: None,
        local: None,
    };

    /// Show the instance of `model` at `local`, making it first (at `local` carried by
    /// `character`, the character's latest world transform) or remaking it when its model changed.
    /// Returns whether it made an instance.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update; `model` is a live shared model.
    unsafe fn show(
        &mut self,
        model: *const SharedPtr<Model>,
        local: Mat4,
        character: Mat4,
    ) -> bool {
        let was_visible = self.owned.visible();
        let Some(created) = (unsafe { self.owned.show(model, character * local) }) else {
            return false;
        };
        // A model shown afresh starts at its new place rather than sweeping from its last.
        if !was_visible || created {
            self.local = None;
        }
        self.local = Some(match self.local {
            Some([_, last]) => [last, local],
            None => [local, local],
        });
        created
    }

    /// Hide the instance, keeping it for when its pouch comes back.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    unsafe fn hide(&mut self) {
        unsafe { self.owned.hide() };
        self.local = None;
        self.bone_local = None;
    }

    /// Release the instance.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    unsafe fn release(&mut self) {
        unsafe { self.owned.release() };
        *self = Slot::EMPTY;
    }
}

struct State {
    slots: [Slot; 3],
    /// The magazine in the hand ([`hand_tick`]).
    hand: OwnedModel,
    /// The input tick [`hand_tick`] last showed the magazine in the hand on.
    hand_sequence: u64,
    /// The weapons, by name hash, already logged as having no spare magazine model.
    without_prop: BTreeSet<u32>,
    status: PouchModelsStatus,
    attachments: Option<Sample>,
    owner: Option<usize>,
    epoch: u64,
    input_sequence: u64,
    render_sequence: u64,
}

impl State {
    /// # Safety
    ///
    /// The game thread, before the world update.
    unsafe fn release_all(&mut self) {
        for slot in &mut self.slots {
            unsafe { slot.release() };
        }
        unsafe { self.hand.release() };
        self.status.shown = [false; 3];
        self.status.hand = false;
        self.attachments = None;
        self.owner = None;
        self.epoch = self.epoch.wrapping_add(1);
        mirror::clear_pouch_render();
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    slots: [Slot::EMPTY; 3],
    hand: OwnedModel::EMPTY,
    hand_sequence: 0,
    without_prop: BTreeSet::new(),
    status: PouchModelsStatus {
        shown: [false; 3],
        hand: false,
        created: 0,
        drawn: 0,
    },
    attachments: None,
    owner: None,
    epoch: 0,
    input_sequence: 0,
    render_sequence: 0,
});

static CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Release the instances when the payload unloads, once one may exist.
fn ensure_cleanup() {
    if !CLEANUP_REGISTERED.swap(true, Ordering::AcqRel) {
        crate::lifecycle::on_cleanup(|_renderer| {
            // SAFETY: the shutdown cleanups run on the game thread, outside the game's update.
            unsafe { STATE.lock().release_all() };
        });
    }
}

/// The model `prop` is drawn with for `owner`: the grenade weapon's projectile model, the C4
/// weapon's in-hand model, or a magazine model of the gun at `weapon` ([`magazine_source`]). While
/// the source is missing (a gun's spare magazine is respawned a while after each one it drops),
/// the model `own` (the prop's instance) or, for a magazine, `hand` (the hand's) already holds for
/// the same weapon; `None` without either.
///
/// # Safety
///
/// `owner` must be the live local player character, and `weapon` null or a live weapon of its, on
/// the game thread before the world update.
unsafe fn source_model(
    owner: &Character,
    prop: Prop,
    weapon: Option<usize>,
    own: &OwnedModel,
    hand: &OwnedModel,
) -> Option<*const SharedPtr<Model>> {
    if prop == Prop::Magazine {
        return unsafe { magazine_source(weapon?, [own, hand]) };
    }
    unsafe { game_model(owner, prop) }.or_else(|| own.kept_for(weapon))
}

/// The grenade weapon's projectile model, or the C4 weapon's in-hand model, for `owner`.
///
/// # Safety
///
/// As [`source_model`].
unsafe fn game_model(owner: &Character, prop: Prop) -> Option<*const SharedPtr<Model>> {
    let model: *const SharedPtr<Model> = match prop {
        Prop::Grenade => {
            let weapon = unsafe { slot_weapon(owner, WeaponSlot::E_SLOT_HAND_GRENADE) }?;
            let grenade = unsafe { &*(weapon as *const WeaponBase).cast::<ThrownGrenadeWeapon>() };
            &grenade.m_PhysicalProjectileModel
        }
        Prop::Charge => {
            let weapon = unsafe { slot_weapon(owner, WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE) }?;
            let c4 = unsafe { &*(weapon as *const WeaponBase).cast::<PlantedExplosiveWeapon>() };
            &c4.m_InactiveModelInstance
        }
        Prop::Magazine => return None,
    };
    (!unsafe { (*model).px }.is_null()).then_some(model)
}

/// The weapon in `owner`'s `slot`.
unsafe fn slot_weapon(owner: &Character, slot: WeaponSlot) -> Option<&WeaponBase> {
    unsafe { owner.m_Inventory.m_Weapons.get(slot as usize)?.px.as_ref() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::cockpit::CockpitTransform;

    const PLACEMENT: ModelPlacement = ModelPlacement {
        offset_m: Vec3::new(0.03, 0.01, -0.02),
        rotation_deg: Vec3::new(10.0, 20.0, 30.0),
    };

    #[test]
    fn the_left_pouch_mirrors_the_right() {
        let right = on_pouch(&PLACEMENT, Hand::Right);
        let left = on_pouch(&PLACEMENT, Hand::Left);
        let mirror = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
        // Reflecting the right side's placement across the midline gives the left side's.
        let reflected = mirror * right * mirror;
        for (a, b) in reflected.to_cols_array().iter().zip(left.to_cols_array()) {
            assert!((a - b).abs() < 1e-5, "{reflected} vs {left}");
        }
    }

    #[test]
    fn the_charge_lies_flat_on_the_chest_facing_forward_on_either_side() {
        for side in Hand::BOTH {
            let on = on_pouch(&Prop::Charge.placement(), side);
            // The model's up, its face, points forward out of the chest (the girdle's -Z).
            let face = on.transform_vector3(Vec3::Y);
            assert!(face.distance(Vec3::NEG_Z) < 1e-5, "{side:?}: {face}");
            assert_eq!(on.w_axis.truncate(), Vec3::ZERO);
        }
    }

    #[test]
    fn a_zero_placement_sits_on_the_pouch() {
        let zero = ModelPlacement {
            offset_m: Vec3::ZERO,
            rotation_deg: Vec3::ZERO,
        };
        for side in Hand::BOTH {
            assert_eq!(on_pouch(&zero, side), Mat4::IDENTITY);
        }
    }

    #[test]
    fn the_interaction_center_matches_the_displayed_origin_at_each_world_scale() {
        let pouch = Pose {
            position: Vec3::new(0.2, 0.9, -0.1),
            orientation: Quat::from_rotation_x(0.6),
        };
        for scale in [0.7, 1.0, 1.5] {
            let frames = Frames {
                cockpit: CockpitTransform {
                    head_world: Vec3::new(20.0, 4.0, -8.0),
                    head_cockpit: Vec3::Y * 1.7,
                    body: Quat::from_rotation_y(0.8),
                    world_scale: scale,
                },
                character: Mat4::IDENTITY,
            };
            for side in Hand::BOTH {
                let shown = ShownPouch {
                    prop: Prop::Magazine,
                    pouch,
                    side,
                    weapon: Some(1),
                };
                let placed = placement(Prop::Magazine, Some(&shown), Some(7), &frames, &PLACEMENT);
                let visible_origin = placed.local.unwrap().w_axis.truncate();
                let target = display_centre(pouch, &PLACEMENT, side, scale);
                assert!(
                    frames
                        .cockpit
                        .point_to_world(target)
                        .abs_diff_eq(visible_origin, 1.0e-5)
                );
            }
        }
    }

    #[test]
    fn a_pouch_follows_a_bending_bone_without_interpolating_the_posture_twice() {
        let input_bone = Mat4::from_translation(Vec3::Y);
        let pouch = Mat4::from_translation(Vec3::new(0.15, 1.0, -0.12));
        let local = input_bone.inverse() * pouch;
        let bent =
            Mat4::from_rotation_translation(Quat::from_rotation_x(0.7), Vec3::new(0.0, 0.8, -0.2));
        let character = [Mat4::IDENTITY, Mat4::from_translation(Vec3::X * 3.0)];
        for fraction in [0.0, 0.5, 1.0] {
            let shown = bone_render_matrix(character, bent, local, fraction).unwrap();
            let expected = render_character(character, fraction).unwrap() * bent * local;
            assert!(shown.abs_diff_eq(expected, 1.0e-5));
            assert!(!shown.abs_diff_eq(
                render_character(character, fraction).unwrap() * pouch,
                1.0e-4
            ));
        }
    }

    #[test]
    fn the_model_rides_the_interpolated_body() {
        let character = [
            Mat4::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.4), Vec3::new(2.0, 0.0, 0.0)),
        ];
        let local = [
            Mat4::from_translation(Vec3::new(0.1, 1.2, -0.1)),
            Mat4::from_translation(Vec3::new(0.1, 1.0, -0.1)),
        ];
        let start = render_matrix(character, local, 0.0).unwrap();
        assert!((start.w_axis.truncate() - Vec3::new(0.1, 1.2, -0.1)).length() < 1e-5);
        let end = render_matrix(character, local, 1.0).unwrap();
        let expected = character[1].transform_point3(Vec3::new(0.1, 1.0, -0.1));
        assert!((end.w_axis.truncate() - expected).length() < 1e-5);
        // Halfway, the model is between the two, with the body.
        let half = render_matrix(character, local, 0.5).unwrap();
        let body = lerp_transform(character[0], character[1], 0.5);
        let expected = body.transform_point3(Vec3::new(0.1, 1.1, -0.1));
        assert!((half.w_axis.truncate() - expected).length() < 1e-5);
        // A fraction out of range or unreadable is clamped to a tick.
        assert_eq!(render_matrix(character, local, 3.0), Some(end));
        assert_eq!(render_matrix(character, local, f32::NAN), Some(end));
    }
}
