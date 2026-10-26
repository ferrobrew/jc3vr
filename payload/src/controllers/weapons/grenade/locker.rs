//! Safe grenade draws from visible native grenade displays in nearby interaction graphs.
//!
//! Source discovery borrows the character's valid interaction entries. A placement rule's spawned
//! grenade weapon, or a grenade weapon's own pickup entry, supplies the displayed model and position.
//! Completion reads the same source again and never invokes native pickup or changes the display.

use std::fmt;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponData, WeaponState},
    graphics_engine::model_instance::ModelInstanceFlags,
    interaction::InteractionGraphEntry,
    weapon_placement_rule::WeaponPlacementRule,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
    weapons::{config::GrenadeConfig, engine, grenade::safe, hold::refs::Strong},
};

/// Why an offered locker grenade did not reach the hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    NotOffered,
    SourceChanged,
    OutOfReach,
    DrawUnavailable,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = match self {
            Self::NotOffered => "the hand has no displayed grenade source",
            Self::SourceChanged => "the displayed grenade source is no longer available",
            Self::OutOfReach => "the displayed grenade is outside the hand's reach",
            Self::DrawUnavailable => "the safe grenade model could not be created",
        };
        write!(f, "locker grenade: {cause}")
    }
}

impl std::error::Error for Refusal {}

/// Offer only the nearest visible native grenade display to each free hand.
///
/// # Safety
///
/// `owner` is the local character on the game thread, outside the world update.
pub(super) unsafe fn begin(
    owner: &Character,
    free: [bool; 2],
    grips: [Option<Vec3>; 2],
    to_cockpit: impl Fn(Vec3) -> Vec3,
) -> Vec<Candidate> {
    let sources = unsafe { sources(owner, &to_cockpit) };
    let offers = Hand::BOTH.map(|hand| {
        grips[hand.index()]
            .filter(|_| free[hand.index()])
            .and_then(|grip| nearest(&sources, grip))
    });
    let candidates = Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            let source = offers[hand.index()]?;
            Some(Candidate {
                volume: GrabVolume::GrenadeLocker,
                centre: source.centre,
                radius: REACH_M,
                hand: Some(hand),
            })
        })
        .collect();
    *OFFERS.lock() = offers;
    candidates
}

/// Revalidate the exact cached source before drawing the existing actor-free grenade item.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(super) unsafe fn take(
    hand: Hand,
    grip: Option<Vec3>,
    to_cockpit: impl Fn(Vec3) -> Vec3,
    tuning: &GrenadeConfig,
) -> Result<(), Refusal> {
    let offered = OFFERS.lock()[hand.index()].take();
    let owner = unsafe { engine::local_player() }.ok_or(Refusal::SourceChanged)?;
    let current = unsafe { sources(owner, &to_cockpit) };
    complete(offered, &current, grip, || unsafe {
        safe::draw(hand, tuning)
    })
}

/// Stop accepting cached sources after loss of gameplay or input.
pub(super) fn clear() {
    *OFFERS.lock() = [None; 2];
}

const MAX_ENTRIES: usize = 256;
const MAX_DISPLAYS: usize = 32;
const MAX_MODELS_PER_WEAPON: usize = 8;
const REACH_M: f32 = 0.18;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Identity {
    entry: usize,
    weapon: usize,
    control: usize,
    data: usize,
    model: usize,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Source {
    identity: Identity,
    centre: Vec3,
}

static OFFERS: Mutex<[Option<Source>; 2]> = Mutex::new([None; 2]);

unsafe fn sources(owner: &Character, to_cockpit: &impl Fn(Vec3) -> Vec3) -> Vec<Source> {
    let entries = &owner.m_InteractionUserProxy.m_ValidEntries;
    let begin = entries.m_Begin as usize;
    let end = entries.m_End as usize;
    let stride = std::mem::size_of::<*mut InteractionGraphEntry>();
    let Some(bytes) = end.checked_sub(begin).filter(|_| begin != 0) else {
        return Vec::new();
    };
    if !bytes.is_multiple_of(stride) || bytes / stride > MAX_ENTRIES {
        return Vec::new();
    }
    let mut out = Vec::new();
    for index in 0..bytes / stride {
        let entry = unsafe { *entries.m_Begin.add(index) };
        let Some(alive) = (unsafe { offered(entry) }) else {
            continue;
        };
        let Some(address) =
            (alive.object() as usize).checked_sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
        else {
            continue;
        };
        unsafe {
            weapon_displays(
                &*(address as *const WeaponBase),
                Identity {
                    entry: entry as usize,
                    weapon: address,
                    control: alive.control_block() as usize,
                    data: 0,
                    model: 0,
                },
                to_cockpit,
                &mut out,
            );
        }
        if out.len() >= MAX_DISPLAYS {
            break;
        }
    }
    out
}

unsafe fn offered(entry: *mut InteractionGraphEntry) -> Option<Strong> {
    let entry = unsafe { entry.as_ref() }?;
    let graph = unsafe { entry.m_Graph.as_mut() }?;
    if let Some(context) = unsafe { graph.m_Context.as_ref() }
        && let Some(parent) = unsafe { Strong::lock(&context.m_Parent) }
        && unsafe { (*parent.object()).IsType(WeaponBase::TYPE_ID()) }
    {
        return Some(parent);
    }
    let node = unsafe { graph.GetNodeByIndex(entry.m_EntryNodeIndex).as_ref() }?;
    let rule = unsafe { Strong::lock(&node.m_GameObject) }?;
    if !unsafe { (*rule.object()).IsType(WeaponPlacementRule::TYPE_ID()) } {
        return None;
    }
    let spawned = unsafe {
        Strong::filled(|out| {
            (*rule.object().cast::<WeaponPlacementRule>()).GetSpawnedObject(
                out,
                WeaponBase::TYPE_ID(),
                0,
            );
        })
    }?;
    unsafe { (*spawned.object()).IsType(WeaponBase::TYPE_ID()) }.then_some(spawned)
}

unsafe fn weapon_displays(
    weapon: &WeaponBase,
    identity: Identity,
    to_cockpit: &impl Fn(Vec3) -> Vec3,
    out: &mut Vec<Source>,
) {
    let slot = unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() };
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    if slot != WeaponSlot::E_SLOT_HAND_GRENADE as i32
        || !(state == WeaponState::E_WS_WEAPON_RACK as i32
            || state == WeaponState::E_WS_DROPPED as i32)
    {
        return;
    }
    let data = &weapon.m_lWeaponData;
    let stride = std::mem::size_of::<*mut WeaponData>();
    let Some(count) = (data.m_End as usize)
        .checked_sub(data.m_Begin as usize)
        .filter(|bytes| !data.m_Begin.is_null() && bytes.is_multiple_of(stride))
        .map(|bytes| bytes / stride)
        .filter(|count| *count <= MAX_MODELS_PER_WEAPON)
    else {
        return;
    };
    for index in 0..count {
        let data = unsafe { *data.m_Begin.add(index) };
        let Some(display) = (unsafe {
            display(
                data,
                Identity {
                    data: data as usize,
                    ..identity
                },
                to_cockpit,
            )
        }) else {
            continue;
        };
        if !out.iter().any(|source| {
            source.identity.weapon == display.identity.weapon
                && source.identity.model == display.identity.model
        }) {
            out.push(display);
        }
        if out.len() >= MAX_DISPLAYS {
            return;
        }
    }
}

unsafe fn display(
    data: *const WeaponData,
    mut identity: Identity,
    to_cockpit: &impl Fn(Vec3) -> Vec3,
) -> Option<Source> {
    let data = unsafe { data.as_ref() }?;
    let model = unsafe { data.m_BaseModelInstance.as_ref() }?;
    if !model.m_Flags.contains(ModelInstanceFlags::VISIBLE) {
        return None;
    }
    let world = Mat4::from(data.m_WorldMatrixT1);
    if !world.is_finite() {
        return None;
    }
    let centre = to_cockpit(world.w_axis.truncate());
    if !centre.is_finite() {
        return None;
    }
    identity.model = data.m_BaseModelInstance as usize;
    Some(Source { identity, centre })
}

fn nearest(sources: &[Source], grip: Vec3) -> Option<Source> {
    if !grip.is_finite() {
        return None;
    }
    sources
        .iter()
        .filter_map(|source| {
            let distance = source.centre.distance_squared(grip);
            (distance.is_finite() && distance <= REACH_M * REACH_M).then_some((*source, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(source, _)| source)
}

fn complete(
    offered: Option<Source>,
    current: &[Source],
    grip: Option<Vec3>,
    draw: impl FnOnce() -> bool,
) -> Result<(), Refusal> {
    let offered = offered.ok_or(Refusal::NotOffered)?;
    let source = current
        .iter()
        .find(|source| source.identity == offered.identity)
        .ok_or(Refusal::SourceChanged)?;
    nearest(
        std::slice::from_ref(source),
        grip.ok_or(Refusal::OutOfReach)?,
    )
    .ok_or(Refusal::OutOfReach)?;
    if draw() {
        Ok(())
    } else {
        Err(Refusal::DrawUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, mem::MaybeUninit, num::NonZeroU64};

    use jc3gi::{
        equipment::weapon::WeaponClass, graphics_engine::model_instance::ModelInstance,
        types::math::Matrix4,
    };

    use super::*;
    use crate::controllers::loadout::{BodyLayout, Loadout, safe_items::ModelLeaseId};

    fn identity() -> Identity {
        Identity {
            entry: 1,
            weapon: 2,
            control: 3,
            data: 4,
            model: 5,
        }
    }

    #[test]
    fn provider_uses_the_native_visible_display_and_current_world_transform() {
        // SAFETY: these mapped types contain only pointers, integers, and floating-point values.
        let mut model: Box<ModelInstance> = Box::new(unsafe { std::mem::zeroed() });
        let mut data: Box<WeaponData> = Box::new(unsafe { std::mem::zeroed() });
        model.m_Flags = ModelInstanceFlags::VISIBLE;
        data.m_BaseModelInstance = model.as_mut();
        data.m_WorldMatrixT1 = Matrix4::from(Mat4::from_translation(Vec3::new(8.0, 2.0, 3.0)));
        let transform = |world: Vec3| (world - Vec3::new(8.0, 2.0, 3.0)) / 2.0;
        let source = unsafe { display(data.as_ref(), identity(), &transform) }.unwrap();
        assert_eq!(source.centre, Vec3::ZERO);
        assert_eq!(source.identity.model, model.as_ref() as *const _ as usize);
        model.m_Flags = ModelInstanceFlags::empty();
        assert!(unsafe { display(data.as_ref(), identity(), &transform) }.is_none());
        model.m_Flags = ModelInstanceFlags::VISIBLE;
        data.m_WorldMatrixT1 = Matrix4::from(Mat4::from_translation(Vec3::NAN));
        assert!(unsafe { display(data.as_ref(), identity(), &transform) }.is_none());
    }

    #[test]
    fn native_display_provider_feeds_independent_safe_items_without_native_pickup() {
        let mut storage = Box::new(MaybeUninit::<WeaponBase>::zeroed());
        // SAFETY: every mapped value permits zero except WeaponClass, initialized before the borrow.
        let weapon = unsafe {
            let weapon = storage.as_mut_ptr();
            (&raw mut (*weapon).m_Class).write(WeaponClass::E_WEAPON_CLASS_NONE);
            &mut *weapon
        };
        let mut model: Box<ModelInstance> = Box::new(unsafe { std::mem::zeroed() });
        let mut data: Box<WeaponData> = Box::new(unsafe { std::mem::zeroed() });
        model.m_Flags = ModelInstanceFlags::VISIBLE;
        data.m_BaseModelInstance = model.as_mut();
        data.m_WorldMatrixT1 = Matrix4::from(Mat4::IDENTITY);
        let mut entries = [data.as_mut() as *mut WeaponData];
        weapon.m_lWeaponData.m_Begin = entries.as_mut_ptr();
        weapon.m_lWeaponData.m_End = unsafe { entries.as_mut_ptr().add(entries.len()) };
        weapon.m_EquipSlot = WeaponSlot::E_SLOT_HAND_GRENADE;
        weapon.m_State = WeaponState::E_WS_WEAPON_RACK;
        let mut sources = Vec::new();
        unsafe { weapon_displays(weapon, identity(), &|point| point, &mut sources) };
        assert_eq!(sources.len(), 1);
        unsafe { weapon_displays(weapon, identity(), &|point| point, &mut sources) };
        assert_eq!(sources.len(), 1);
        let offer = nearest(&sources, Vec3::ZERO);
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        for hand in Hand::BOTH {
            assert_eq!(
                complete(offer, &sources, Some(Vec3::ZERO), || {
                    table
                        .admit_safe_item(
                            ModelLeaseId::new(NonZeroU64::new(hand.index() as u64 + 1).unwrap()),
                            hand,
                        )
                        .is_ok()
                }),
                Ok(())
            );
        }
        let items: Vec<_> = table.safe_items().collect();
        assert_eq!(items.len(), 2);
        assert_ne!(items[0].item, items[1].item);
        assert_ne!(items[0].binding.model, items[1].binding.model);
        assert_eq!(weapon.m_State, WeaponState::E_WS_WEAPON_RACK);
        assert_eq!(model.m_Flags, ModelInstanceFlags::VISIBLE);
        sources.clear();
        weapon.m_State = WeaponState::E_WS_WIELDING;
        unsafe { weapon_displays(weapon, identity(), &|point| point, &mut sources) };
        assert!(sources.is_empty());
        weapon.m_State = WeaponState::E_WS_DROPPED;
        weapon.m_EquipSlot = WeaponSlot::E_SLOT_DUAL_WIELD;
        unsafe { weapon_displays(weapon, identity(), &|point| point, &mut sources) };
        assert!(sources.is_empty());
        weapon.m_EquipSlot = WeaponSlot::E_SLOT_HAND_GRENADE;
        unsafe { weapon_displays(weapon, identity(), &|point| point, &mut sources) };
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn exact_source_revalidation_precedes_the_actual_draw_consumer() {
        let source = Source {
            identity: identity(),
            centre: Vec3::ZERO,
        };
        let draws = Cell::new(0);
        let draw = || {
            draws.set(draws.get() + 1);
            true
        };
        assert_eq!(
            complete(Some(source), &[source], Some(Vec3::ZERO), draw),
            Ok(())
        );
        assert_eq!(draws.get(), 1);
        let changed = Source {
            identity: Identity {
                control: 99,
                ..source.identity
            },
            ..source
        };
        assert_eq!(
            complete(Some(source), &[changed], Some(Vec3::ZERO), draw),
            Err(Refusal::SourceChanged)
        );
        assert_eq!(
            complete(Some(source), &[source], Some(Vec3::X), draw),
            Err(Refusal::OutOfReach)
        );
        assert_eq!(
            complete(None, &[source], Some(Vec3::ZERO), draw),
            Err(Refusal::NotOffered)
        );
        assert_eq!(draws.get(), 1);
    }

    #[test]
    fn repeated_draws_from_one_display_are_independent_and_refusal_does_not_retarget() {
        let source = Source {
            identity: identity(),
            centre: Vec3::ZERO,
        };
        let other = Source {
            identity: Identity {
                model: 99,
                ..source.identity
            },
            ..source
        };
        let draws = Cell::new(0);
        for _ in 0..32 {
            assert_eq!(
                complete(Some(source), &[source], Some(Vec3::ZERO), || {
                    draws.set(draws.get() + 1);
                    true
                }),
                Ok(())
            );
        }
        assert_eq!(draws.get(), 32);
        assert_eq!(
            complete(Some(source), &[other], Some(Vec3::ZERO), || {
                draws.set(draws.get() + 1);
                true
            }),
            Err(Refusal::SourceChanged)
        );
        assert_eq!(draws.get(), 32);
        assert_eq!(
            complete(Some(source), &[source], Some(Vec3::ZERO), || false),
            Err(Refusal::DrawUnavailable)
        );
    }
}
