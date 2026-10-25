//! The game's models of what the body's pouches hold, drawn on the pouches: a grenade on the grenade
//! pouch, a C4 charge on the C4 pouch, and the held gun's magazine on the magazine pouch, so the
//! player sees on Rico's body what a hand reaching there takes.
//!
//! Each model is an instance the payload owns, made from a model the game already holds for the
//! item's lifetime ([`ModelInstanceManager::CreateInstanceFromModel`]): the grenade weapon's
//! projectile model ([`ThrownGrenadeWeapon::m_PhysicalProjectileModel`]), the C4 weapon's in-hand
//! model ([`PlantedExplosiveWeapon::m_InactiveModelInstance`]), and the gun's spare magazine prop's
//! model ([`WeaponData::m_MagazineObject`], [`RigidObject::m_Model`]). The instance keeps its own
//! reference to the model, and the engine's cull draws it once it is in the world, with no physics
//! and no game object behind it.
//!
//! The input tick ([`input_tick`]) places each model where its pouch is, in the character's model
//! space, and keeps the last two ticks' placements. The render frame ([`render_frame`]) draws it
//! between them, carried by the character's interpolated world transform at the frame's
//! interpolation fraction, so it rides the body as the body is drawn rather than a tick behind
//! it ([render-time-anchoring.md](../../../../docs/mod/rendering/render-time-anchoring.md)).
//!
//! The instances are created, shown, hidden, and released on the game thread, outside the world
//! update, as the engine requires. A model whose pouch is not offered is hidden; every instance is
//! released outside gameplay, when its source model changes (another gun's magazine), and when the
//! payload unloads.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use glam::{EulerRot, Mat4, Quat, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{PlantedExplosiveWeapon, ThrownGrenadeWeapon, WeaponBase},
    graphics_engine::model_instance::{
        Model, ModelInstance, ModelInstanceFlags, ModelInstanceManager,
    },
    types::{math::Matrix4, shared_ptr::SharedPtr},
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        aim::gun,
        body::{mirror, rig::frames::Frames},
        frame::{Hand, Pose},
        hands::late::lerp_transform,
        weapons::{
            config::{ModelPlacement, PouchModelsConfig},
            engine,
            pouch_models::attachments::{RenderSample, Sample, SampleId, placement},
        },
    },
    hooks,
};

pub(crate) mod attachments;

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

    fn placement(self, config: &PouchModelsConfig) -> ModelPlacement {
        match self {
            Prop::Grenade => config.grenade,
            Prop::Charge => config.charge,
            Prop::Magazine => config.magazine,
        }
    }
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

/// The model's world matrix at the interpolation fraction `dtf`: its placement in the character's
/// model space between the last two ticks', carried by the character's world transform between its
/// last two ticks'.
pub fn render_matrix(character: [Mat4; 2], local: [Mat4; 2], dtf: f32) -> Option<Mat4> {
    let matrix = render_transform(character, dtf) * render_transform(local, dtf);
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
/// The game thread, outside the world update.
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
    for prop in Prop::ALL {
        let wanted = shown.iter().find(|s| s.prop == prop);
        let model = wanted.and_then(|s| unsafe { source_model(owner, prop, s.weapon) });
        // SAFETY: source_model returns a live borrowed model under this input tick's contract.
        let source = model.map(|model| unsafe { (*model).px } as usize);
        let mut attachment = placement(prop, wanted, source, &frames, &prop.placement(config));
        let local = attachment.local.ok();
        let slot = &mut state.slots[prop.index()];
        let previous_source = slot.model;
        let previously_visible = slot.visible;
        let created = match (model, local) {
            (Some(model), Some(local)) => unsafe {
                slot.show(model, local, Mat4::from(owner.m_WorldMatrixT1))
            },
            _ => {
                unsafe { slot.hide() };
                false
            }
        };
        let visible = slot.visible;
        if previously_visible != visible || previous_source != slot.model {
            mirror::clear_pouch_render();
        }
        state.status.created += u64::from(created);
        state.status.shown[prop.index()] = visible;
        attachment.visible = visible;
        attachments.attachments.push(attachment);
    }
    state.attachments = Some(attachments);
}

/// Draw the pouches' models at the frame's interpolation fraction `dtf`. Called once per rendered
/// frame, before the engine's render update culls and draws the frame.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(crate) unsafe fn render_frame(dtf: f32) {
    mirror::clear_pouch_render();
    let mut state = STATE.lock();
    state.render_sequence = state.render_sequence.wrapping_add(1);
    if state.attachments.is_none() {
        return;
    }
    let Some((owner_id, character)) = (unsafe { engine::local_player() }).map(|owner| {
        (
            owner as *const Character as usize,
            [
                Mat4::from(owner.m_WorldMatrixT0),
                Mat4::from(owner.m_WorldMatrixT1),
            ],
        )
    }) else {
        unsafe { state.release_all() };
        return;
    };
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
        if !slot.visible {
            continue;
        }
        let (Some(instance), Some(local)) = (slot.instance, slot.local) else {
            continue;
        };
        let Some(matrix) = render_matrix(character, local, dtf) else {
            continue;
        };
        // SAFETY: the instance is the payload's own, live until it is released on this thread.
        unsafe { (*(instance as *mut ModelInstance)).UpdateMatrix(&Matrix4::from(matrix)) };
        if let (Some(sample), Some(model)) = (&mut sample, slot.model) {
            sample.applied(prop, model, matrix);
        }
        drew = true;
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
/// The game thread, outside the world update.
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
    /// The instance (a `ModelInstance` address), or `None` before one is made.
    instance: Option<usize>,
    /// The model the instance was made from.
    model: Option<usize>,
    visible: bool,
    /// The placement in the character's model space on the last two ticks, older first.
    local: Option<[Mat4; 2]>,
}

impl Slot {
    /// Show the instance of `model` at `local`, making it first (at `local` carried by
    /// `character`, the character's latest world transform) or remaking it when its model changed.
    /// Returns whether it made an instance.
    ///
    /// # Safety
    ///
    /// The game thread, outside the world update; `model` is a live shared model.
    unsafe fn show(
        &mut self,
        model: *const SharedPtr<Model>,
        local: Mat4,
        character: Mat4,
    ) -> bool {
        let source = unsafe { (*model).px } as usize;
        if self.instance.is_some() && self.model != Some(source) {
            unsafe { self.release() };
        }
        let Some(manager) = (unsafe { ModelInstanceManager::get() }) else {
            return false;
        };
        let mut created = false;
        let instance = match self.instance {
            Some(instance) => instance as *mut ModelInstance,
            None => {
                let world = Matrix4::from(character * local);
                let made =
                    unsafe { manager.CreateInstanceFromModel(model, &world, true, false, true) };
                let Some(made) = (unsafe { made.as_mut() }) else {
                    return false;
                };
                // The engine's own owners of a model held on a character keep it drawn whatever
                // the camera's distance, as the explosives do.
                made.m_Flags.remove(ModelInstanceFlags::FLAG_CLIPONDISTANCE);
                self.instance = Some(made as *mut ModelInstance as usize);
                self.model = Some(source);
                self.visible = false;
                created = true;
                made
            }
        };
        if !self.visible {
            unsafe { manager.ShowModelInstance(instance) };
            self.visible = true;
            // ShownPouch afresh, it is drawn at its new place rather than swept from its last.
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
    /// The game thread, outside the world update.
    unsafe fn hide(&mut self) {
        if let (Some(instance), true) = (self.instance, self.visible)
            && let Some(manager) = unsafe { ModelInstanceManager::get() }
        {
            unsafe { manager.HideModelInstance(instance as *mut ModelInstance) };
        }
        self.visible = false;
        self.local = None;
    }

    /// Release the instance.
    ///
    /// # Safety
    ///
    /// The game thread, outside the world update.
    unsafe fn release(&mut self) {
        if let Some(instance) = self.instance.take()
            && let Some(manager) = unsafe { ModelInstanceManager::get() }
        {
            unsafe { manager.AddToDeleteQueue(instance as *mut ModelInstance, false) };
        }
        *self = Slot::default();
    }
}

struct State {
    slots: [Slot; 3],
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
    /// The game thread, outside the world update.
    unsafe fn release_all(&mut self) {
        for slot in &mut self.slots {
            unsafe { slot.release() };
        }
        self.status.shown = [false; 3];
        self.attachments = None;
        self.owner = None;
        self.epoch = self.epoch.wrapping_add(1);
        mirror::clear_pouch_render();
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    slots: [Slot {
        instance: None,
        model: None,
        visible: false,
        local: None,
    }; 3],
    status: PouchModelsStatus {
        shown: [false; 3],
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
/// weapon's in-hand model, or the spare magazine's model of the gun at `weapon`; `None` while the
/// weapon or its model is missing (a gun's spare magazine is respawned a while after each one it
/// drops).
///
/// # Safety
///
/// `owner` must be the live local player character, and `weapon` null or a live weapon of its, on
/// the game thread outside the world update.
unsafe fn source_model(
    owner: &Character,
    prop: Prop,
    weapon: Option<usize>,
) -> Option<*const SharedPtr<Model>> {
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
        Prop::Magazine => {
            let weapon = unsafe { (weapon? as *const WeaponBase).as_ref() }?;
            let first = unsafe { gun::guns(weapon) }.next().flatten()?;
            let magazine = unsafe { first.m_MagazineObject.px.as_ref() }?;
            &magazine.m_Model
        }
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
