//! The hands' moves at the sockets, and letting go of a held weapon: the table takes each move,
//! the reconcile makes the game follow, and this keeps what the move looks like across input
//! ticks ([`crate::controllers::weapons::hold::dock`] has the pure parts).
//!
//! - A draw off a socket ([`draw`]) is a draw for the loadout table at once; the gun then slides
//!   from where it sat on the socket into the hand. Its pose on the socket's bone is measured at
//!   each draw and kept per weapon and socket, so that a dock can slide it back to where it sits.
//! - A dock onto a socket ([`dock`]) slides the gun from the hand onto the socket first, and the
//!   table takes the dock when the slide ends, so the gun stays in the hand (and held back from
//!   firing) while it slides. A gun never drawn off that socket docks at once, without the slide.
//! - A let-go ([`let_go`]) drops the weapon with the hand's motion and tells the table.
//!
//! Each hand has its own slide and its own grip watch, so the two hands draw, dock, and let go at
//! once. The rigid grip reads a hand's slide in the placement detour ([`snap_view`]) to place the
//! gun's attach bone between the socket and the hand.

use jc3gi::{
    character::{
        character::{Character, SafeBoneIndex},
        inventory::WeaponSlot,
    },
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    loadout::{ObjectAddress, Socket, grab, live},
    weapons::{
        docks, engine,
        hold::{
            self, armed,
            dock::{ReleaseWatch, Snap, SnapFrom, SnapKind},
            equip::{self, DockRefusal, Thrown},
            layout,
            release_diagnostics::{ReleaseRecord, ReleaseRoute, ReleaseStatus, SampleStage},
        },
        inventory::{
            self, carry,
            engine::{heavy_tracked, slot_of},
            reconcile, sockets,
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

/// The moves' readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct DockStatus {
    /// Each hand's snap under way, and which way it moves the gun, by [`Hand::index`].
    pub snapping: [Option<SnapKind>; 2],
    /// Whether each hand's grip holds its gun, by [`Hand::index`].
    pub grip_holds: [bool; 2],
    pub draws: u64,
    pub docks: u64,
    pub drops: u64,
    /// The last requested speed, metres per second, when the native setter returned success.
    pub last_throw_mps: Option<f32>,
    /// Only the latest release attempt is retained; all native identifiers are copied values.
    pub releases: ReleaseStatus,
    pub last_refusal: Option<DockRefusal>,
}

pub fn status() -> DockStatus {
    let state = STATE.lock();
    DockStatus {
        snapping: state.snaps.map(|s| s.map(|(snap, _)| snap.kind)),
        grip_holds: state.watches.map(|w| w.release.armed().is_some()),
        ..state.status
    }
}

/// Draw the weapon on `socket` into `hand`, sliding it off the socket over `snap_s` seconds.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn draw(hand: Hand, socket: Socket, snap_s: f32) -> Result<(), DockRefusal> {
    let result = unsafe { draw_off(hand, socket) };
    let mut state = STATE.lock();
    match result {
        Ok(seat) => {
            state.status.draws += 1;
            state.slide_out(seat, hand, snap_s);
            state.watches[hand.index()].pending = true;
            drop(state);
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                socket = socket.label(),
                "hold: drew the weapon off the {} into the {} hand",
                socket.label(),
                hand.label()
            );
            Ok(())
        }
        Err(refusal) => {
            state.status.last_refusal = Some(refusal);
            drop(state);
            hold::refused(refusal);
            Err(refusal)
        }
    }
}

/// Dock the weapon `hand` holds on `socket`, sliding it there over `snap_s` seconds first when its
/// pose there is known.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn dock(hand: Hand, socket: Socket, snap_s: f32) -> Result<(), DockRefusal> {
    let result = unsafe { start_dock(hand, socket) };
    let object = match result {
        Ok(object) => object,
        Err(refusal) => {
            STATE.lock().status.last_refusal = Some(refusal);
            hold::refused(refusal);
            return Err(refusal);
        }
    };
    let mut state = STATE.lock();
    state.watches[hand.index()].release.disarm();
    if state.slide_back(object, socket, hand, snap_s) {
        return Ok(());
    }
    drop(state);
    unsafe { finish_dock(hand, socket, object) }
}

/// Let the weapon `hand` holds fall, with the hand's motion capped at `max_speed_mps`.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn let_go(hand: Hand, max_speed_mps: f32) -> Result<Thrown, DockRefusal> {
    let mut diagnostic = ReleaseRecord::new(hand, max_speed_mps);
    let result = unsafe { drop_from(hand, max_speed_mps, &mut diagnostic) };
    diagnostic.refusal = result.as_ref().err().copied();
    let mut state = STATE.lock();
    state.status.releases.record(diagnostic);
    let release = state.status.releases.latest;
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
                release = ?release,
                "hold: let go of the weapon in the {} hand",
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

/// Whether the weapon at `object` docks on a socket at all. A heavy weapon does only while the
/// inventory keeps heavy weapons beyond the slots
/// ([`heavy_tracked`]): the game drops a heavy
/// weapon it unequips from its slot, so docked from the slot it would fall off the body.
///
/// # Safety
///
/// `object` must be a live weapon, on the main thread outside the world update.
pub(crate) unsafe fn dockable(object: usize) -> bool {
    // SAFETY: forwarded from the caller.
    unsafe { (object as *const WeaponBase).as_ref() }
        .and_then(|weapon| unsafe { slot_of(weapon) })
        .is_some_and(|slot| slot != WeaponSlot::E_SLOT_HEAVY || heavy_tracked())
}

/// Slide the gun of `weapon` in `hand` into the hand from `from`, its pose in the world before the
/// hand took it, over `snap_s` seconds. Nothing slides for a zero or bad time.
pub(crate) fn slide_in(weapon: usize, hand: Hand, from: Pose, snap_s: f32) {
    if !(snap_s.is_finite() && snap_s > 0.0) {
        return;
    }
    STATE.lock().snaps[hand.index()] = Some((
        Snap::new(SnapKind::Draw, weapon, hand, SnapFrom::World(from), snap_s),
        Finish::Nothing,
    ));
}

/// `hand` took the gun by a grab: with the grip holding guns, its grip holds this one once it is
/// seen held.
pub(crate) fn arm(hand: Hand) {
    STATE.lock().watches[hand.index()].pending = true;
}

/// Fold in one input tick of the grips, by [`Hand::index`]: returns, by hand, whether the hand's
/// grip opened on the weapon the table has in it: one it took by a grab, or one it closed on after
/// it came into the hand some other way ([`ReleaseWatch::latch_press`]).
pub(crate) fn release_tick(grips: [bool; 2]) -> [bool; 2] {
    let held = Hand::BOTH.map(live::in_hand);
    let mut state = STATE.lock();
    Hand::BOTH.map(|hand| {
        let watch = &mut state.watches[hand.index()];
        let weapon = held[hand.index()];
        let holder = weapon.map(|_| hand);
        if weapon != watch.weapon {
            watch.weapon = weapon;
            watch.release.unarm();
        }
        if std::mem::take(&mut watch.pending) && weapon.is_some() && grips[hand.index()] {
            watch.release.arm(hand);
        }
        if watch.release.latch_press(holder, grips).is_some() {
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                "hold: the grip closed on the gun in the hand, which it holds from now on"
            );
        }
        watch.release.observe(holder, grips) == Some(hand)
    })
}

/// Which hands' guns a dock's snap is moving onto their sockets, which holds them back from firing,
/// by [`Hand::index`].
pub(crate) fn docking() -> [bool; 2] {
    STATE.lock().snaps.map(|s| {
        s.is_some_and(|(snap, finish)| snap.kind == SnapKind::Holster && finish != Finish::Nothing)
    })
}

/// The snap of `weapon`'s gun in `hand`, for the rigid grip.
pub(crate) fn snap_view(weapon: usize, hand: Hand) -> Option<SnapView> {
    let (snap, _) = STATE.lock().snaps[hand.index()]?;
    if snap.weapon != weapon {
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

/// Advance each hand's snap by `dt` seconds: a finished dock's snap docks the weapon, and a snap
/// whose weapon left its hand is dropped.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn tick(dt: f32) {
    for hand in Hand::BOTH {
        let finished = {
            let mut state = STATE.lock();
            let Some((snap, finish)) = &mut state.snaps[hand.index()] else {
                continue;
            };
            if live::in_hand(hand) != Some(snap.weapon) {
                state.snaps[hand.index()] = None;
                continue;
            }
            snap.advance(dt);
            if !snap.done() {
                continue;
            }
            let finished = (*snap, *finish);
            state.snaps[hand.index()] = None;
            finished
        };
        if let (snap, Finish::Dock(socket)) = finished {
            let _ = unsafe { finish_dock(hand, socket, snap.weapon) };
        }
    }
}

/// Stop the snaps and the grip watches.
pub(crate) fn clear() {
    let mut state = STATE.lock();
    state.snaps = [None; 2];
    for watch in &mut state.watches {
        watch.release.disarm();
        watch.pending = false;
    }
}

/// The most weapons' poses on their sockets kept.
const MAX_OFFSETS: usize = 8;

/// What the end of a snap does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Finish {
    /// Dock the weapon on this socket.
    Dock(Socket),
    /// Nothing: a draw's or a take's snap.
    Nothing,
}

/// Where a drawn weapon sat on its socket.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Seat {
    weapon: usize,
    socket: Socket,
    /// The first gun's pose on the socket's bone, in the skeleton's model space.
    offset: Option<Pose>,
}

/// One hand's grip watch: the weapon the table had in it at the last tick, and whether a grab put
/// it there.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Watch {
    release: ReleaseWatch,
    weapon: Option<usize>,
    pending: bool,
}

impl Watch {
    const fn new() -> Self {
        Self {
            release: ReleaseWatch::new(),
            weapon: None,
            pending: false,
        }
    }
}

struct State {
    /// Each hand's snap and what its end does, by [`Hand::index`].
    snaps: [Option<(Snap, Finish)>; 2],
    /// Each weapon's gun pose on a socket's bone, from its last draw off it: weapon, socket, pose.
    offsets: Vec<(usize, Socket, Pose)>,
    watches: [Watch; 2],
    status: DockStatus,
}

impl State {
    /// Start a draw's snap of `seat`'s gun off its socket into `hand`, remembering how it sat there
    /// for its dock.
    fn slide_out(&mut self, seat: Seat, hand: Hand, snap_s: f32) {
        let Some(offset) = seat.offset else {
            return;
        };
        self.remember(seat.weapon, seat.socket, offset);
        if snap_s > 0.0 {
            self.snaps[hand.index()] = Some((
                Snap::new(
                    SnapKind::Draw,
                    seat.weapon,
                    hand,
                    SnapFrom::Dock {
                        bone: sockets::bone(seat.socket) as u32,
                        offset,
                    },
                    snap_s,
                ),
                Finish::Nothing,
            ));
        }
    }

    /// Start a dock's snap of `weapon`'s gun from `hand` onto `socket`, when the gun's pose there
    /// is known. Returns whether it started.
    fn slide_back(&mut self, weapon: usize, socket: Socket, hand: Hand, snap_s: f32) -> bool {
        let Some(offset) = self.offset_of(weapon, socket).filter(|_| snap_s > 0.0) else {
            return false;
        };
        self.snaps[hand.index()] = Some((
            Snap::new(
                SnapKind::Holster,
                weapon,
                hand,
                SnapFrom::Dock {
                    bone: sockets::bone(socket) as u32,
                    offset,
                },
                snap_s,
            ),
            Finish::Dock(socket),
        ));
        true
    }

    fn remember(&mut self, weapon: usize, socket: Socket, offset: Pose) {
        self.offsets
            .retain(|&(w, s, _)| !(w == weapon && s == socket));
        if self.offsets.len() >= MAX_OFFSETS {
            self.offsets.remove(0);
        }
        self.offsets.push((weapon, socket, offset));
    }

    fn offset_of(&self, weapon: usize, socket: Socket) -> Option<Pose> {
        self.offsets
            .iter()
            .find(|&&(w, s, _)| w == weapon && s == socket)
            .map(|&(_, _, offset)| offset)
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    snaps: [None; 2],
    offsets: Vec::new(),
    watches: [Watch::new(); 2],
    status: DockStatus {
        snapping: [None; 2],
        grip_holds: [false; 2],
        draws: 0,
        docks: 0,
        drops: 0,
        last_throw_mps: None,
        releases: ReleaseStatus::EMPTY,
        last_refusal: None,
    },
});

/// Draw the weapon on `socket` into `hand` through the loadout table, which the reconcile carries
/// out at once. Returns where it sat.
unsafe fn draw_off(hand: Hand, socket: Socket) -> Result<Seat, DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    if !unsafe { owner.m_Inventory.GetAnimationAllowsDrawing() } {
        return Err(DockRefusal::AnimationForbids);
    }
    let snapshot = live::snapshot_now().ok_or(DockRefusal::NoTable)?;
    let weapon = unsafe { docks::socket_weapons(owner, &snapshot) }
        .into_iter()
        .find(|(s, _)| *s == socket)
        .map(|(_, weapon)| weapon)
        .ok_or(DockRefusal::NothingThere)?;
    let object = weapon as *const WeaponBase as usize;
    let offset = unsafe { equip::pose_on_bone(owner, weapon, sockets::bone(socket)) };
    // SAFETY: a slot or the carry set keeps the weapon alive.
    let held = unsafe { inventory::held_of(object) }.ok_or(DockRefusal::Unsupported)?;
    if !live::arrive(hand, held) {
        return Err(DockRefusal::NotDone);
    }
    unsafe { reconcile::apply(owner) };
    Ok(Seat {
        weapon: object,
        socket,
        offset,
    })
}

/// Check that `hand` can dock its weapon on `socket` now. Returns the weapon.
unsafe fn start_dock(hand: Hand, socket: Socket) -> Result<usize, DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    let snapshot = live::snapshot_now().ok_or(DockRefusal::NoTable)?;
    if !grab::fits(&snapshot, hand, socket) {
        return Err(DockRefusal::DoesNotFit);
    }
    let object = live::in_hand(hand).ok_or(DockRefusal::NothingHeld)?;
    // SAFETY: the table holds the weapon in a hand, and a slot or the carry set keeps it alive.
    if !unsafe { dockable(object) } {
        return Err(DockRefusal::Unsupported);
    }
    if is_wielded(owner, object) && !unsafe { owner.m_Inventory.GetAnimationAllowsHolstering() } {
        return Err(DockRefusal::AnimationForbids);
    }
    Ok(object)
}

/// The table takes `hand`'s dock of `weapon` on `socket`, the game unequips it when it wields it,
/// and the reconcile does the rest: the other hand's weapon becomes the wielded one when that hand
/// is the primary now, and a weapon armed in `hand` is disarmed.
unsafe fn finish_dock(hand: Hand, socket: Socket, weapon: usize) -> Result<(), DockRefusal> {
    let refused = |refusal: DockRefusal| {
        STATE.lock().status.last_refusal = Some(refusal);
        hold::refused(refusal);
        Err(refusal)
    };
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return refused(DockRefusal::NoCharacter);
    };
    let Some(object) = ObjectAddress::from_address(weapon) else {
        return refused(DockRefusal::NothingHeld);
    };
    if !live::dock(hand, object, socket) {
        return refused(DockRefusal::DoesNotFit);
    }
    // Unequipped here rather than by the reconcile, which waits for the animation to allow it: a
    // weapon the game still wields on a socket of the table's would be landed in a hand again.
    if is_wielded(owner, weapon)
        && let Err(refusal) = unsafe { equip::holster(owner, weapon) }
    {
        tracing::debug!(target: "controllers", %refusal, "hold: the docked weapon was not unequipped");
    }
    unsafe { reconcile::apply(owner) };
    STATE.lock().status.docks += 1;
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        socket = socket.label(),
        "hold: docked the weapon from the {} hand on the {}",
        hand.label(),
        socket.label()
    );
    Ok(())
}

/// Drop the weapon `hand` holds: the wielded weapon through its slot, a weapon armed in the hand
/// once it is disarmed. Then the table takes the let-go.
unsafe fn drop_from(
    hand: Hand,
    max_speed_mps: f32,
    diagnostic: &mut ReleaseRecord,
) -> Result<Thrown, DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    let object = live::in_hand(hand).ok_or(DockRefusal::NothingHeld)?;
    diagnostic.weapon = Some(object);
    let thrown = if is_wielded(owner, object) {
        diagnostic.route = ReleaseRoute::Wielded;
        unsafe { equip::drop_held(owner, max_speed_mps) }?
    } else if armed::in_hand(hand) == Some(object) {
        diagnostic.route = ReleaseRoute::ArmedInHand;
        let weapon = unsafe { armed::weapon_of(owner, hand) }.ok_or(DockRefusal::NotDone)?;
        diagnostic.stages.before_disarm = true;
        unsafe { armed::disarm(owner, hand, Some(&mut *weapon), true) };
        diagnostic.stages.disarm_returned = true;
        if weapon.m_State != WeaponState::E_WS_HOLSTERED {
            return Err(DockRefusal::NotHolstered);
        }
        // A carried weapon's strong reference is kept until the drop is done.
        let strong = carry::release(object);
        let mut thrown = unsafe { equip::drop_loose(weapon, max_speed_mps) };
        thrown.diagnostic.sample_stage = SampleStage::AfterDisarm;
        let slot = unsafe { slot_of(weapon) };
        thrown.diagnostic.slot = slot;
        let in_slot = slot.is_some_and(|slot| {
            owner
                .m_Inventory
                .m_Weapons
                .get(slot as usize)
                .is_some_and(|s| s.px as usize == object)
        });
        if let Some(slot) = slot.filter(|_| in_slot) {
            unsafe { owner.m_Inventory.DropWeapon(slot, false) };
            thrown.diagnostic.slot_clear_returned = true;
        }
        drop(strong);
        thrown
    } else {
        return Err(DockRefusal::NotDone);
    };
    diagnostic.stages.slot_clear_returned = thrown.diagnostic.slot_clear_returned;
    diagnostic.drop = Some(thrown.diagnostic);
    if let Some(address) = ObjectAddress::from_address(object) {
        live::depart(hand, address, live::Whereabouts::Away);
    }
    unsafe { reconcile::apply(owner) };
    diagnostic.stages.reconcile_returned = true;
    Ok(thrown)
}

/// Whether `owner` wields the weapon at `object`.
fn is_wielded(owner: &Character, object: usize) -> bool {
    // SAFETY: the caller holds the live local player on the main thread outside the world update.
    unsafe { gun::held_weapon(owner) }.is_some_and(|w| w as *const WeaponBase as usize == object)
}
