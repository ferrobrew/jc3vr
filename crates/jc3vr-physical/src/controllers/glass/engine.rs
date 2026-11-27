//! The engine side of breaking glass: the panes a glass hit names, the damage added to them the way
//! a bullet adds it, and a pane brought down by hiding its model instance.
//!
//! Everything here runs on the game thread, in the game's render update before the ammunition
//! manager's own: where the bullets' deferred window damage runs
//! ([`AmmunitionManagerHandleDeferredQueue`](jc3gi::equipment::explosive::AmmunitionManagerHandleDeferredQueue)).
//! A pane found from a hit is used within the same tick: the window metadata it carries is
//! destroyed only on a later frame, after its instance lets it go.

use std::{
    mem::MaybeUninit,
    ptr::{self, NonNull},
    slice,
};

use glam::Vec3;
use jc3gi::{
    graphics_engine::{
        model_instance::{
            ModelCollection, ModelInstance, ModelInstanceArray, ModelInstanceFlags,
            ModelInstanceManager,
        },
        render_block::RenderBlockWindow,
        render_pass::RenderBlock,
        window_damage::{MetaData, MetaDataType, MetaWindow, WindowDamageKind},
    },
    physics::{CastRayInfo, GetModelInstancesHit},
    types::math::{Matrix4, Vector3, Vector4},
};

use crate::controllers::glass::{bullets, strike};

/// A window pane a strike met: a model instance and the window metadata it draws its damage from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Pane {
    instance: NonNull<ModelInstance>,
    window: NonNull<MetaWindow>,
}

impl Pane {
    /// The instance's address, to name the pane in the log and to keep its strikes' excess.
    pub(super) fn address(self) -> usize {
        self.instance.as_ptr() as usize
    }

    /// The model's name hash, to name the pane in the log.
    ///
    /// # Safety
    ///
    /// As [`panes_hit`]: within the tick the pane was found in.
    pub(super) unsafe fn model_hash(self) -> u32 {
        // SAFETY: forwarded; the instance holds its model alive.
        unsafe {
            self.instance
                .as_ref()
                .m_spModel
                .px
                .as_ref()
                .map_or(0, |m| m.m_Id.m_Hash)
        }
    }
}

/// The window panes a glass hit names: through the hit object, as a bullet looks them up, or,
/// when that names none, the world's panes within [`WORLD_PANE_HALF_EXTENT_M`] of `at`, as the
/// bullet looks them up. Panes already hidden ([`VISIBLE`](ModelInstanceFlags::VISIBLE) clear) are
/// left out: there is no glass there to strike.
///
/// # Safety
///
/// The game thread outside the physics step, with `info` the complete cast info of the query that
/// hit, before anything else runs a query.
pub(super) unsafe fn panes_hit(info: &mut CastRayInfo, at: Vec3) -> Vec<Pane> {
    let mut found = [ptr::null_mut::<ModelInstance>(); MAX_PANES];
    let base = found.as_mut_ptr();
    // Every field is a pointer or an integer, so all-zero is a valid value to fill in.
    // SAFETY: as above.
    let mut array = unsafe { MaybeUninit::<ModelInstanceArray>::zeroed().assume_init() };
    array.m_Begin = base;
    array.m_End = base;
    // SAFETY: one past the end of the buffer.
    array.m_Last = unsafe { base.add(MAX_PANES) };
    array.m_Flags = ModelInstanceArray::USER_ALLOCATED;
    // SAFETY: the array points at the stack buffer, marked as storage it does not own; the info is
    // the query's own.
    unsafe { GetModelInstancesHit(&mut info.base, &mut array, ptr::null_mut()) };
    // SAFETY: the call appends within the buffer.
    let mut count = usize::try_from(unsafe { array.m_End.offset_from(array.m_Begin) }).unwrap_or(0);
    if count == 0 {
        let mut transform = Matrix4::IDENTITY;
        transform.data[12..15].copy_from_slice(&at.to_array());
        let half_extents = Vector4 {
            data: [WORLD_PANE_HALF_EXTENT_M; 4],
        };
        let mut world = 0_u32;
        // SAFETY: plain values on the stack; the call writes at most the capacity it is given.
        unsafe {
            ModelCollection::GetIntersectingDecalModels(
                &transform,
                &half_extents,
                base,
                &mut world,
                MAX_PANES as u32,
            );
        }
        count = (world as usize).min(MAX_PANES);
    }
    found[..count]
        .iter()
        .filter_map(|&instance| NonNull::new(instance))
        // SAFETY: the instances the engine just named are live this tick.
        .filter_map(|instance| unsafe { pane_of(instance) })
        .collect()
}

/// `world` in the pane's own space, where its damage points are kept.
///
/// # Safety
///
/// As [`Pane::model_hash`].
pub(super) unsafe fn local_point(pane: Pane, world: Vec3) -> Vec3 {
    let mut matrix = Matrix4::IDENTITY;
    // SAFETY: forwarded.
    unsafe { pane.instance.as_ref().GetMatrix(&mut matrix) };
    strike::world_to_local(&matrix.data, world)
}

/// The total strength of the pane's damage points, as [`strike::point_total`] counts it.
///
/// # Safety
///
/// As [`Pane::model_hash`].
pub(super) unsafe fn point_total(pane: Pane) -> f32 {
    // SAFETY: forwarded; the list is the window's own.
    let list = unsafe { &pane.window.as_ref().m_WindowDamage };
    let kind = WindowDamageKind::Point as usize;
    let count = (list.m_NumContacts[kind] as usize).min(list.m_Contacts[kind].len());
    strike::point_total(
        list.m_Contacts[kind][..count]
            .iter()
            .map(|c| c.m_DamageMeters),
    )
}

/// Add a damage point of `damage` at `local`, in the pane's own space, as a bullet's hit adds one.
///
/// # Safety
///
/// As [`Pane::model_hash`], on the game thread.
pub(super) unsafe fn add_damage(pane: Pane, local: Vec3, damage: f32) {
    let local = Vector3 {
        data: local.to_array(),
    };
    // SAFETY: forwarded; the main thread adds points while the render thread draws them, as the
    // bullets' deferred calls do.
    unsafe { (*pane.window.as_ptr()).AddWindowDamagePoint(&local, damage) };
}

/// Why a pane was not brought down.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum HideRefusal {
    /// The model draws more than glass: hiding it would hide the rest with it.
    SharedModel,
    /// The model instance manager does not exist.
    NoManager,
}

impl HideRefusal {
    pub(super) const fn label(self) -> &'static str {
        match self {
            HideRefusal::SharedModel => "its model draws more than glass",
            HideRefusal::NoManager => "the model instance manager does not exist",
        }
    }
}

/// Bring the pane down: hide its model instance, when every render block of its model is a window,
/// so that nothing but the glass goes with it.
///
/// # Safety
///
/// As [`Pane::model_hash`], on the game thread.
pub(super) unsafe fn hide(pane: Pane) -> Result<(), HideRefusal> {
    // SAFETY: forwarded.
    if !unsafe { glass_only(pane.instance) } {
        return Err(HideRefusal::SharedModel);
    }
    // SAFETY: the singleton, on the game thread.
    let manager = unsafe { ModelInstanceManager::get() }.ok_or(HideRefusal::NoManager)?;
    // SAFETY: a live instance, hidden from the game thread as the engine's owners hide theirs.
    unsafe { manager.HideModelInstance(pane.instance.as_ptr()) };
    bullets::note_hidden(pane.instance.as_ptr());
    Ok(())
}

/// The most panes one hit is looked up for, as many as a bullet looks up.
const MAX_PANES: usize = 20;

/// The half extent of the box the world's panes are looked up in, metres: the bullet's.
const WORLD_PANE_HALF_EXTENT_M: f32 = 0.01;

/// The pane `instance` is, when it is shown and carries window metadata.
///
/// # Safety
///
/// `instance` must be live.
unsafe fn pane_of(instance: NonNull<ModelInstance>) -> Option<Pane> {
    // SAFETY: forwarded.
    let model = unsafe { instance.as_ref() };
    if model.m_Flags.contains(ModelInstanceFlags::FLAG_DELETED)
        || !model.m_Flags.contains(ModelInstanceFlags::VISIBLE)
    {
        return None;
    }
    // SAFETY: forwarded.
    let window = unsafe { window_of(model) }?;
    Some(Pane { instance, window })
}

/// The window metadata of `instance`, resolved from its tagged metadata word as the window render
/// block resolves it.
///
/// # Safety
///
/// `instance` must be live.
unsafe fn window_of(instance: &ModelInstance) -> Option<NonNull<MetaWindow>> {
    let word = instance.m_MetaFlags;
    let address = word & !ModelInstance::META_TAG_MASK;
    if address == 0 {
        return None;
    }
    let meta = if word & ModelInstance::META_INDIRECT != 0 {
        // SAFETY: an indirect word points at a record holding the block's address.
        unsafe { ((address + ModelInstance::META_INDIRECT_OFFSET) as *const *mut MetaData).read() }
    } else {
        address as *mut MetaData
    };
    let meta = NonNull::new(meta)?;
    // The kind word is read raw: the block may be of a kind the enum does not list.
    // SAFETY: a live block starts with its kind word.
    let kind = unsafe { meta.cast::<u16>().read() };
    (kind == MetaDataType::Window as u16).then(|| meta.cast::<MetaWindow>())
}

/// Whether every render block of every LOD of `instance`'s model is a window.
///
/// # Safety
///
/// `instance` must be live.
unsafe fn glass_only(instance: NonNull<ModelInstance>) -> bool {
    // SAFETY: forwarded; the instance holds its model alive.
    let Some(model) = (unsafe { instance.as_ref().m_spModel.px.as_ref() }) else {
        return false;
    };
    // SAFETY: the model's own virtual functions.
    let lods = unsafe { model.CountLods() };
    let mut blocks_seen = 0_usize;
    for lod in 0..lods {
        let mut blocks: *mut *mut RenderBlock = ptr::null_mut();
        // SAFETY: as above; the call points `blocks` at the model's own array.
        let count = unsafe { model.GetRenderBlocks(lod as i32, &mut blocks) } as usize;
        if count == 0 || blocks.is_null() {
            continue;
        }
        // SAFETY: the model's array of `count` blocks.
        let blocks = unsafe { slice::from_raw_parts(blocks, count) };
        for &block in blocks {
            if block.is_null() {
                continue;
            }
            // SAFETY: a live render block starts with its virtual table.
            let vftable = unsafe { block.cast::<u64>().read() };
            if vftable != RenderBlockWindow::VFTABLE {
                return false;
            }
            blocks_seen += 1;
        }
    }
    blocks_seen > 0
}
