//! Drawing and holstering without the game's acts, and letting go of a held gun: the state across
//! input ticks that [`crate::controllers::weapons::hold::dock`] decides on and
//! [`crate::controllers::weapons::hold::equip`] carries out.
//!
//! A draw starts its snap as soon as the weapon is equipped; a holster starts its snap and
//! unequips the weapon when the snap ends, so the gun stays wielded (and held back from firing)
//! while it slides onto the dock. The gun's pose on its dock is measured at each draw and kept per
//! weapon, so that a holster can slide the gun to where it will sit. A weapon never drawn this way
//! holsters at once, without the slide.
//!
//! The rigid grip reads the snap in the placement detour ([`snap_view`]) to place the gun's attach
//! bone between the dock and the hand.

use jc3gi::character::{
    character::{Character, SafeBoneIndex},
    inventory::WeaponSlot,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    weapons::{
        engine,
        hold::{
            self,
            dock::{ReleaseWatch, Snap, SnapFrom, SnapKind},
            equip::{self, DockRefusal, Thrown},
            holder::Cause,
            layout,
        },
    },
};

/// A snap as the rigid grip sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SnapView {
    pub start: SnapStart,
    /// How far the gun is from the far end toward the hand, `0..=1`.
    pub toward_hand: f32,
}

/// The far end of a snap, as the rigid grip places it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SnapStart {
    /// The gun's pose `offset` on the dock bone `dock`, model space.
    Dock { dock: SafeBoneIndex, offset: Pose },
    /// A pose in the world.
    World(Pose),
}

/// The docking's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct DockStatus {
    /// The snap under way, and which way it moves the gun.
    pub snapping: Option<SnapKind>,
    /// The hand whose grip holds the gun, while the grip holds it.
    pub grip_holds: Option<Hand>,
    pub draws: u64,
    pub holsters: u64,
    pub drops: u64,
    /// The speed the last dropped weapon left with, metres per second, when the mod set it.
    pub last_throw_mps: Option<f32>,
    pub last_refusal: Option<DockRefusal>,
}

pub fn status() -> DockStatus {
    let state = STATE.lock();
    DockStatus {
        snapping: state.snap.map(|s| s.kind),
        grip_holds: state.release.armed(),
        ..state.status
    }
}

/// Draw `slot`'s weapon into `hand` without the game's act, sliding it out of its dock over
/// `snap_s` seconds.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn draw(hand: Hand, slot: WeaponSlot, snap_s: f32) -> Result<(), DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    hold::begin(hand, Cause::Draw, None);
    let result = unsafe { equip::draw(owner, slot) };
    let mut state = STATE.lock();
    let docked = match result {
        Ok(docked) => docked,
        Err(refusal) => {
            state.status.last_refusal = Some(refusal);
            drop(state);
            hold::refused(refusal);
            return Err(refusal);
        }
    };
    state.status.draws += 1;
    if let Some(offset) = docked.offset {
        state.remember(docked.weapon, docked.dock, offset);
        if snap_s > 0.0 {
            state.snap = Some(Snap::new(
                SnapKind::Draw,
                docked.weapon,
                hand,
                SnapFrom::Dock {
                    bone: docked.dock,
                    offset,
                },
                snap_s,
            ));
        }
    }
    drop(state);
    hold::adopt(docked.weapon, hand);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        "hold: drew the weapon into the {} hand without the game's draw",
        hand.label()
    );
    Ok(())
}

/// Holster the weapon `hand` holds without the game's act, sliding it onto its dock over `snap_s`
/// seconds first when its pose there is known.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn holster(hand: Hand, snap_s: f32) -> Result<(), DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    let target = match unsafe { equip::holster_target(owner) } {
        Ok(target) => target,
        Err(refusal) => {
            STATE.lock().status.last_refusal = Some(refusal);
            hold::refused(refusal);
            return Err(refusal);
        }
    };
    let mut state = STATE.lock();
    state.release.disarm();
    if let Some(offset) = state.offset_of(target.weapon, target.dock)
        && snap_s > 0.0
    {
        state.snap = Some(Snap::new(
            SnapKind::Holster,
            target.weapon,
            hand,
            SnapFrom::Dock {
                bone: target.dock,
                offset,
            },
            snap_s,
        ));
        return Ok(());
    }
    drop(state);
    unsafe { finish_holster(owner, target.weapon) }
}

/// Drop the weapon `hand` holds, with the hand's motion capped at `max_speed_mps`.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn drop_held(hand: Hand, max_speed_mps: f32) -> Result<Thrown, DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    let result = unsafe { equip::drop_held(owner, max_speed_mps) };
    let mut state = STATE.lock();
    match result {
        Ok(thrown) => {
            state.status.drops += 1;
            state.status.last_throw_mps = thrown.set.then(|| thrown.linear.length());
            drop(state);
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                speed_mps = thrown.linear.length(),
                set_by_the_mod = thrown.set,
                "hold: dropped the weapon from the {} hand",
                hand.label()
            );
            Ok(thrown)
        }
        Err(refusal) => {
            state.status.last_refusal = Some(refusal);
            drop(state);
            hold::refused(refusal);
            Err(refusal)
        }
    }
}

/// Slide the gun of `weapon` in `hand` into the hand from `from`, its pose in the world before the
/// hand took it, over `snap_s` seconds. Nothing slides for a zero or bad time.
pub(crate) fn slide_in(weapon: usize, hand: Hand, from: Pose, snap_s: f32) {
    if !(snap_s.is_finite() && snap_s > 0.0) {
        return;
    }
    STATE.lock().snap = Some(Snap::new(
        SnapKind::Draw,
        weapon,
        hand,
        SnapFrom::World(from),
        snap_s,
    ));
}

/// `hand` took the gun by a grab: with the grip holding guns, its grip holds this one once it is
/// seen held.
pub(crate) fn arm(hand: Hand) {
    STATE.lock().arm_pending = Some(hand);
}

/// Fold in one input tick of the grips, by [`Hand::index`]: returns the holding hand when its grip
/// opened on the gun it took by a grab.
pub(crate) fn release_tick(holder: Option<Hand>, grips: [bool; 2]) -> Option<Hand> {
    let mut state = STATE.lock();
    if let Some(hand) = state.arm_pending.take()
        && holder == Some(hand)
        && grips[hand.index()]
    {
        state.release.arm(hand);
    }
    state.release.observe(holder, grips)
}

/// Whether a snap is moving a gun: a holster's holds the weapon back from firing.
pub(crate) fn holstering() -> bool {
    STATE
        .lock()
        .snap
        .is_some_and(|s| s.kind == SnapKind::Holster)
}

/// The snap of `weapon`'s gun in `hand`, for the rigid grip.
pub(crate) fn snap_view(weapon: usize, hand: Hand) -> Option<SnapView> {
    let snap = STATE.lock().snap?;
    if snap.weapon != weapon || snap.hand != hand {
        return None;
    }
    let start = match snap.from {
        SnapFrom::Dock { bone, offset } => SnapStart::Dock {
            dock: layout::dock_bone(bone)?,
            offset,
        },
        SnapFrom::World(pose) => SnapStart::World(pose),
    };
    Some(SnapView {
        start,
        toward_hand: snap.toward_hand(),
    })
}

/// Advance the snap by `dt` seconds, with `wielded` the weapon in the hands: a finished holster
/// unequips the weapon, and a snap whose weapon left the hands is dropped.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn tick(owner: &Character, wielded: Option<usize>, dt: f32) {
    let finished = {
        let mut state = STATE.lock();
        let Some(snap) = &mut state.snap else {
            return;
        };
        if wielded != Some(snap.weapon) {
            state.snap = None;
            return;
        }
        snap.advance(dt);
        if !snap.done() {
            return;
        }
        let snap = *snap;
        state.snap = None;
        snap
    };
    if finished.kind == SnapKind::Holster {
        let _ = unsafe { finish_holster(owner, finished.weapon) };
    }
}

/// Stop the snap and the grip watch.
pub(crate) fn clear() {
    let mut state = STATE.lock();
    state.snap = None;
    state.release.disarm();
    state.arm_pending = None;
}

/// The most weapons whose dock pose is kept.
const MAX_OFFSETS: usize = 8;

struct State {
    snap: Option<Snap>,
    /// Each weapon's gun pose on its dock bone, from its last draw: weapon, dock bone, pose.
    offsets: Vec<(usize, u32, Pose)>,
    release: ReleaseWatch,
    arm_pending: Option<Hand>,
    status: DockStatus,
}

impl State {
    fn remember(&mut self, weapon: usize, dock: u32, offset: Pose) {
        self.offsets.retain(|&(w, _, _)| w != weapon);
        if self.offsets.len() >= MAX_OFFSETS {
            self.offsets.remove(0);
        }
        self.offsets.push((weapon, dock, offset));
    }

    fn offset_of(&self, weapon: usize, dock: u32) -> Option<Pose> {
        self.offsets
            .iter()
            .find(|&&(w, d, _)| w == weapon && d == dock)
            .map(|&(_, _, offset)| offset)
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    snap: None,
    offsets: Vec::new(),
    release: ReleaseWatch::new(),
    arm_pending: None,
    status: DockStatus {
        snapping: None,
        grip_holds: None,
        draws: 0,
        holsters: 0,
        drops: 0,
        last_throw_mps: None,
        last_refusal: None,
    },
});

/// Unequip `weapon` now, counting and logging the holster.
unsafe fn finish_holster(owner: &Character, weapon: usize) -> Result<(), DockRefusal> {
    let result = unsafe { equip::holster(owner, weapon) };
    let mut state = STATE.lock();
    match result {
        Ok(()) => {
            state.status.holsters += 1;
            drop(state);
            tracing::info!(target: "controllers", "hold: holstered the weapon without the game's holster");
        }
        Err(refusal) => {
            state.status.last_refusal = Some(refusal);
            drop(state);
            hold::refused(refusal);
        }
    }
    result
}
