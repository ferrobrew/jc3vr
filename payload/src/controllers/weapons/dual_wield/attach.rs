//! The pistols' attach bones, owned by the mod while the dual-wield pair is wielded: each pistol
//! rides its hand's attach bone while that hand has it drawn, and the hip holster bone on that
//! hand's side otherwise ([`draw::bones`]). The engine side is in
//! `docs/engine/character/hands-locomotion-and-state.md`.
//!
//! The game rewrites every gun's attach-bone table at the end of the character's
//! pose-finalization pass ([`WeaponBase::UpdatePostFinalizePose`]), and the character's state
//! tasks move single guns between bones earlier in the same pass, so a table written once does not
//! stay. The table is therefore written at three points, from the layout the input tick publishes
//! ([`input_tick`]):
//!
//! - before each placement of the inventory's weapons
//!   ([`Inventory::UpdateAttachmentTransforms`]), so the guns are placed on the mod's bones
//!   whatever the state tasks wrote; a pistol whose bone changed since its last placement skips
//!   its render interpolation for that placement
//!   ([`WeaponBase::SkipRenderInterpolationForOneFrame`]), so it snaps between hand and hip
//!   rather than streaking;
//! - after the end-of-pass rewrite ([`Inventory::UpdateInventoryPostFinalizePose`]), so every
//!   reader between two passes (the input tick, the gun aim, the shot's hand, and the render-time
//!   late update) sees the mod's bones;
//! - at the weapon's own per-tick update ([`crate::controllers::weapons::dual_wield::before_update`]),
//!   just before it fires.
//!
//! Both the wielding and the dual-wielding entries are written, so a state task that flips the
//! weapon between the two keeps the mod's layout. Nothing changes the weapon's state: the game
//! still has the pair dual-wielded, so its body animation, magazine, and reload are the pair's.
//!
//! The placement detour also runs the rigid grip around the original
//! ([`crate::controllers::hands::grip`]), for every weapon the local player holds.
//!
//! The detours run on the simulation's worker threads for every character, so they compare the
//! inventory's weapon against the owned weapon's address before taking any lock, and they never
//! touch the VR state.

use std::{
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Instant,
};

use detours_macro::detour;
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::grip,
    weapons::dual_wield::draw::{self, Holding},
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_ATTACHMENT_TRANSFORMS_BINDER)
        .with_static_binder(&UPDATE_INVENTORY_POST_FINALIZE_POSE_BINDER)
}

/// The settings the layout is built from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    pub dominant: Hand,
    /// Each hand draws its own pistol.
    pub independent: bool,
    /// The first pistol rides the dominant hand.
    pub by_handedness: bool,
}

/// The pair as the input tick left it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Pair {
    /// Whether the game has the pair in the hands.
    pub wielded: bool,
    /// The hands whose pistols are out, indexed by [`Hand::index`].
    pub drawn: [bool; 2],
    /// The hand each pistol belongs to, indexed by gun.
    pub gun_hands: [Hand; 2],
}

/// The attachment ownership's readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct AttachStatus {
    /// Whether the mod owns the pistols' attach bones.
    pub owned: bool,
    pub wielded: bool,
    pub drawn: [bool; 2],
    /// The hand each pistol belongs to, indexed by gun.
    pub gun_hands: Option<[Hand; 2]>,
    /// The hand of a hip grab whose draw waits on the game.
    pub pending: Option<Hand>,
    /// Table writes and snapped placements since the payload loaded.
    pub writes: u64,
    pub snaps: u64,
}

pub fn status() -> AttachStatus {
    let tick = TICK.lock();
    AttachStatus {
        owned: OWNED.load(Ordering::Relaxed) != 0,
        wielded: tick.holding.wielded(),
        drawn: tick.holding.drawn(),
        gun_hands: tick.pair.map(|p| p.gun_hands),
        pending: tick.holding.pending(),
        writes: WRITES.load(Ordering::Relaxed),
        snaps: SNAPS.load(Ordering::Relaxed),
    }
}

/// Follow the player's dual-wield pair for one input tick and publish its layout, or stop owning
/// the attach bones when the settings own nothing. Returns the pair while the mod owns its bones.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &Character, settings: Option<Settings>) -> Option<Pair> {
    let weapon: Option<&WeaponBase> = unsafe { dual_weapon(&owner.m_Inventory) }.map(|w| &*w);
    let pair = weapon.filter(|w| unsafe { gun::guns(w) }.count() == PISTOLS);
    let mut tick = TICK.lock();
    let now = Instant::now();
    let dt = tick
        .last
        .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
    tick.last = Some(now);
    let wielded = pair.is_some_and(|w| {
        is_wielded(w.m_State) && std::ptr::eq(w.m_User[0].cast::<Character>(), owner)
    });
    tick.holding
        .observe(pair.map(|w| w as *const WeaponBase as usize), wielded, dt);
    tick.settings = settings;
    let (Some(weapon), Some(_)) = (pair, settings) else {
        tick.pair = None;
        drop(tick);
        clear();
        return None;
    };
    let published = tick.publish(weapon as *const WeaponBase as usize);
    tick.pair = published;
    published
}

/// A hip grab by `hand` while the pair is not wielded: the draw brings out only `hand`'s pistol.
pub(crate) fn begin_draw(hand: Hand) {
    TICK.lock().change(|h| {
        h.begin_draw(hand);
        true
    });
}

/// Draw `hand`'s pistol while the pair is wielded. Returns whether it was drawn.
pub(crate) fn draw(hand: Hand) -> bool {
    TICK.lock().change(|h| h.draw(hand))
}

/// Holster `hand`'s pistol while the other hand holds its own. Returns whether it was holstered.
pub(crate) fn holster(hand: Hand) -> bool {
    TICK.lock().change(|h| h.holster(hand))
}

/// Stop owning the attach bones; the game's end-of-pass rewrite restores its own layout.
pub(crate) fn clear() {
    OWNED.store(0, Ordering::Relaxed);
    *LAYOUT.lock() = None;
}

/// The pistol each hand of `weapon` holds, while the mod owns its bones.
pub(crate) fn guns_by_hand(weapon: &WeaponBase) -> Option<[Option<usize>; 2]> {
    let address = weapon as *const WeaponBase as usize;
    if OWNED.load(Ordering::Relaxed) != address {
        return None;
    }
    LAYOUT
        .lock()
        .filter(|l| l.weapon == address)
        .map(|l| l.guns)
}

/// Write the owned layout into `weapon`'s attach-bone table, when `weapon` is the owned pair and
/// wielded. With `snap`, a pistol whose bone changed since its last placement skips its render
/// interpolation.
///
/// # Safety
///
/// `weapon` must be live, on the thread updating its holder.
pub(crate) unsafe fn apply(weapon: &mut WeaponBase, snap: bool) {
    let address = weapon as *mut WeaponBase as usize;
    if OWNED.load(Ordering::Relaxed) != address || !is_wielded(weapon.m_State) {
        return;
    }
    let mut snaps = [false; PISTOLS];
    let bones = {
        let mut layout = LAYOUT.lock();
        let Some(layout) = layout.as_mut().filter(|l| l.weapon == address) else {
            return;
        };
        if snap {
            for (gun, bone) in layout.bones.into_iter().enumerate() {
                snaps[gun] = layout.placed[gun].is_some_and(|placed| placed != bone);
                layout.placed[gun] = Some(bone);
            }
        }
        layout.bones
    };
    let list = &weapon.m_lWeaponData;
    for (gun, bone) in bones.into_iter().enumerate() {
        if list.m_Begin.is_null() || unsafe { list.m_Begin.add(gun) } >= list.m_End {
            return;
        }
        // SAFETY: the entry is inside the weapon's live gun list.
        let Some(data) = (unsafe { (*list.m_Begin.add(gun)).as_mut() }) else {
            continue;
        };
        for state in [WeaponState::E_WS_WIELDING, WeaponState::E_WS_DUAL_WIELDING] {
            if let Some(entry) = data
                .m_lBoneAttachementPerWeaponState
                .get_mut(state as usize)
            {
                *entry = bone;
            }
        }
    }
    WRITES.fetch_add(1, Ordering::Relaxed);
    for (gun, _) in snaps.into_iter().enumerate().filter(|&(_, s)| s) {
        if let Ok(index) = i32::try_from(gun) {
            unsafe { weapon.SkipRenderInterpolationForOneFrame(index) };
            SNAPS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The number of guns of a dual-wield pair the layout handles.
const PISTOLS: usize = 2;

/// The layout the detours write.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Layout {
    weapon: usize,
    /// The attach bone of each pistol, indexed by gun.
    bones: [u32; PISTOLS],
    /// The pistol each hand holds, indexed by [`Hand::index`].
    guns: [Option<usize>; 2],
    /// The bone each pistol was last placed on.
    placed: [Option<u32>; PISTOLS],
}

/// The input tick's side: the holding state and the settings it was last published with.
struct Tick {
    holding: Holding,
    settings: Option<Settings>,
    pair: Option<Pair>,
    last: Option<Instant>,
}

impl Tick {
    /// Publish the layout of `weapon` from the holding state and the settings.
    fn publish(&mut self, weapon: usize) -> Option<Pair> {
        let settings = self.settings?;
        let gun_hands = draw::gun_hands(settings.dominant, settings.by_handedness);
        let drawn = if settings.independent {
            self.holding.drawn()
        } else {
            [true; 2]
        };
        let bones = draw::bones(gun_hands, drawn);
        let guns = draw::guns_by_hand(gun_hands, drawn);
        let wielded = self.holding.wielded();
        {
            let mut layout = LAYOUT.lock();
            let placed = (*layout)
                .filter(|l| l.weapon == weapon && wielded)
                .map_or([None; PISTOLS], |l| l.placed);
            *layout = Some(Layout {
                weapon,
                bones,
                guns,
                placed,
            });
        }
        OWNED.store(weapon, Ordering::Relaxed);
        Some(Pair {
            wielded,
            drawn,
            gun_hands,
        })
    }

    /// Apply `change` to the holding state and republish the layout at once, so the next
    /// placement already has it.
    fn change(&mut self, change: impl FnOnce(&mut Holding) -> bool) -> bool {
        let changed = change(&mut self.holding);
        let layout = *LAYOUT.lock();
        if changed && let Some(layout) = layout {
            self.pair = self.publish(layout.weapon);
        }
        changed
    }
}

static TICK: Mutex<Tick> = Mutex::new(Tick {
    holding: Holding::new(),
    settings: None,
    pair: None,
    last: None,
});
static LAYOUT: Mutex<Option<Layout>> = Mutex::new(None);
/// The owned pair's address, or zero: the detours' check before any lock.
static OWNED: AtomicUsize = AtomicUsize::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);
static SNAPS: AtomicU64 = AtomicU64::new(0);

const fn is_wielded(state: WeaponState) -> bool {
    matches!(
        state,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    )
}

/// The inventory's dual-wield slot weapon.
unsafe fn dual_weapon(inventory: &Inventory) -> Option<&'static mut WeaponBase> {
    let weapon = inventory
        .m_Weapons
        .get(WeaponSlot::E_SLOT_DUAL_WIELD as usize)?
        .px;
    unsafe { weapon.as_mut() }
}

/// The owned pair of `inventory`, when it holds it.
unsafe fn owned_in(inventory: *mut Inventory) -> Option<&'static mut WeaponBase> {
    let owned = OWNED.load(Ordering::Relaxed);
    if owned == 0 {
        return None;
    }
    let weapon = unsafe { dual_weapon(inventory.as_ref()?) }?;
    (weapon as *mut WeaponBase as usize == owned).then_some(weapon)
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateAttachmentTransforms_ADDRESS)]
fn update_attachment_transforms(
    inventory: *mut Inventory,
    character: *const Character,
    p3: f32,
    p4: f32,
    transform_type: u32,
) {
    if let Some(weapon) = unsafe { owned_in(inventory) } {
        unsafe { apply(weapon, true) };
    }
    // The rigid grip pins the attach bones after the layout above has chosen them.
    let placement = unsafe { grip::before_placement(inventory, character) };
    UPDATE_ATTACHMENT_TRANSFORMS
        .get()
        .unwrap()
        .call(inventory, character, p3, p4, transform_type);
    if let Some(placement) = placement {
        unsafe { grip::after_placement(character, placement) };
    }
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateInventoryPostFinalizePose_ADDRESS)]
fn update_inventory_post_finalize_pose(inventory: *mut Inventory) {
    UPDATE_INVENTORY_POST_FINALIZE_POSE
        .get()
        .unwrap()
        .call(inventory);
    if let Some(weapon) = unsafe { owned_in(inventory) } {
        unsafe { apply(weapon, false) };
    }
}
