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
//! - An attracted dock ([`attract`]) is a dock a release made from outside the socket's reach: the
//!   gun travels onto the socket over a time set by the distance, toward where it sat at its last
//!   draw, or with no draw, toward where its grip goes there.
//! - A let-go ([`let_go`]) drops the weapon with the hand's motion and tells the table.
//!
//! Each hand has its own slide and its own grip watch, so the two hands draw, dock, and let go at
//! once. The rigid grip reads a hand's slide in the placement detour ([`snap_view`]) to place the
//! gun's attach bone between the socket and the hand.

#[cfg(test)]
mod release_tests;

use std::time::Instant;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::{Character, SafeBoneIndex},
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        bindings,
        clock::TimeScale,
        diagnostics,
        frame::{self, Hand, Pose},
        hands::grip,
        loadout::{ObjectAddress, Socket, grab, live},
        motion,
        roomscale::teleport,
        weapons::{
            docks,
            engine::{self, grip_points, slot_of},
            hold::{
                self, armed,
                dock::{ReleaseWatch, Snap, SnapFrom, SnapKind},
                equip::{self, DockRefusal, Thrown},
                layout,
                release_diagnostics::{
                    ControllerContext, MAX_RELEASE_AGE_S, ReleaseFrame, ReleaseInput,
                    ReleaseMotion, ReleaseRecord, ReleaseRoute, ReleaseStatus, ReleaseTransport,
                    RigMotion, SampleStage, estimate_release,
                },
            },
            inventory::{self, reconcile, sockets, traversal},
        },
    },
    headpose, hooks,
    recorder::tracking::events::{self, EventKind},
    vr::recenter_count,
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
/// The game thread, before the world update.
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
/// The game thread, before the world update.
pub(crate) unsafe fn dock(hand: Hand, socket: Socket, snap_s: f32) -> Result<(), DockRefusal> {
    let result = unsafe { start_dock(hand, socket) };
    let object = match result {
        Ok(object) => object,
        Err(refusal) => {
            hold::refused(refusal);
            return Err(refusal);
        }
    };
    let mut state = STATE.lock();
    state.watches[hand.index()].release.disarm();
    state.watches[hand.index()].since_s = None;
    if state.slide_back(object, socket, hand, snap_s) {
        return Ok(());
    }
    drop(state);
    unsafe { finish_dock(hand, socket, object) }
}

/// Dock the weapon `hand` let go of on `socket`, whose attraction caught it outside the socket's
/// reach ([`crate::controllers::weapons::docks::attract`]): the gun travels from the hand onto the
/// socket over `travel_s` seconds, and the table takes the dock when it arrives. A gun with no
/// pose there from a draw travels to `toward` (where its grip goes, in the world) at the
/// orientation it left the hand with, and takes the socket's own placement on arrival.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn attract(
    hand: Hand,
    socket: Socket,
    toward: Vec3,
    travel_s: f32,
) -> Result<(), DockRefusal> {
    let result = unsafe { start_dock(hand, socket) };
    let object = match result {
        Ok(object) => object,
        Err(refusal) => {
            hold::refused(refusal);
            return Err(refusal);
        }
    };
    // SAFETY: the table holds the weapon in the hand, and a slot or the carry set keeps it alive.
    let provisional = unsafe { provisional_offset(object, socket, toward) };
    let mut state = STATE.lock();
    state.watches[hand.index()].release.disarm();
    state.watches[hand.index()].since_s = None;
    if state.slide_back(object, socket, hand, travel_s)
        || provisional
            .is_some_and(|offset| state.slide_onto(object, socket, hand, offset, travel_s))
    {
        return Ok(());
    }
    drop(state);
    unsafe { finish_dock(hand, socket, object) }
}

/// Let the weapon `hand` holds fall, with the hand's motion capped at `max_speed_mps`.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn let_go(hand: Hand, max_speed_mps: f32) -> Result<Thrown, DockRefusal> {
    let mut diagnostic = ReleaseRecord::new(hand, max_speed_mps);
    let result = unsafe { drop_from(hand, max_speed_mps, &mut diagnostic) };
    diagnostic.refusal = result.as_ref().err().copied();
    let mut state = STATE.lock();
    state.status.releases.record(diagnostic);
    let release = state.status.releases.latest;
    if let Some(record) = release {
        events::note_with(|| EventKind::GunRelease(Box::new(record.event())));
        if let Some(thrown) = record.thrown() {
            diagnostics::note_throw(thrown);
        }
    }
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
            drop(state);
            hold::refused(refusal);
            Err(refusal)
        }
    }
}

/// Whether the weapon at `object` docks on a socket at all: any weapon with a slot, the heavy
/// weapons too, which the inventory keeps beyond the slots.
///
/// # Safety
///
/// `object` must be a live weapon, on the game thread before the world update.
pub(crate) unsafe fn dockable(object: usize) -> bool {
    // SAFETY: forwarded from the caller.
    unsafe { (object as *const WeaponBase).as_ref() }
        .and_then(|weapon| unsafe { slot_of(weapon) })
        .is_some()
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
///
/// On the first tick after the release stood down ([`cancel_releases`], [`clear`]), a hand still holding a gun
/// reads its controller's grip as it is ([`physical_grips`]), since `grips` stays open for a grip
/// held through the stand-down until the hand opens: closed, the hand keeps the gun until the grip
/// opens, and then lets go of it; open, it puts the gun back on a socket of the body ([`stow`]), or
/// lets it fall when none takes it. The hold ends with the grip, even when the grip opened while
/// the release stood down (the debug panel, the pause menu, a tracking loss).
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn release_tick(grips: [bool; 2], action_hands: [bool; 2]) -> [bool; 2] {
    let held = Hand::BOTH.map(live::in_hand);
    let now_s = motion::latest_time();
    let physical = physical_grips();
    let outcomes = {
        let mut state = STATE.lock();
        Hand::BOTH.map(|hand| {
            state.watches[hand.index()].observe(
                hand,
                held[hand.index()],
                GripReading { grips, physical },
                action_hands,
                now_s,
            )
        })
    };
    Hand::BOTH.map(|hand| match outcomes[hand.index()] {
        GripRelease::Kept => false,
        GripRelease::LetGo => true,
        GripRelease::Stow => {
            // SAFETY: forwarded from the caller.
            match unsafe { stow(hand) } {
                Ok(socket) => tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    socket = socket.label(),
                    "hold: the {} hand's grip opened while the release stood down; its weapon went back on the {}",
                    hand.label(),
                    socket.label()
                ),
                Err(refusal) => {
                    tracing::info!(
                        target: "controllers",
                        hand = hand.label(),
                        %refusal,
                        "hold: the {} hand's grip opened while the release stood down, and no socket takes its weapon; it falls",
                        hand.label()
                    );
                    // SAFETY: forwarded from the caller.
                    if let Err(refusal) = unsafe { let_go(hand, hold::MAX_THROW_MPS) } {
                        hold::refused(refusal);
                    }
                }
            }
            false
        }
    })
}

/// Cancel grip-release intent while its input route is inactive, without stopping snap motion.
/// The next [`release_tick`] reads each hand's grip afresh.
pub(crate) fn cancel_releases() {
    STATE.lock().watches.iter_mut().for_each(Watch::stand_down);
}

/// Whether each controller's grip holds its hand's gun on the latest frame, by [`Hand::index`],
/// unmasked by the bindings' wait for a returning hand's controls to rest
/// ([`bindings::grip_holds_now`]); `None` for a grip not known.
fn physical_grips() -> [Option<bool>; 2] {
    Hand::BOTH.map(bindings::grip_holds_now)
}

/// One tick's grips, by [`Hand::index`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct GripReading {
    /// The grips as a hold reads them, masked for a returning hand until its controls rest.
    grips: [bool; 2],
    /// The controllers' grips as they are, `None` where not known ([`physical_grips`]).
    physical: [Option<bool>; 2],
}

/// Put the gun `hand` holds back on a socket of the body that takes it: the one it was last drawn
/// off when that one is free ([`stow_socket`]), sliding there as a dock does. Returns the socket.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn stow(hand: Hand) -> Result<Socket, DockRefusal> {
    let snapshot = live::snapshot_now().ok_or(DockRefusal::NoTable)?;
    let object = live::in_hand(hand).ok_or(DockRefusal::NothingHeld)?;
    let remembered = STATE
        .lock()
        .offsets
        .iter()
        .rev()
        .find(|(weapon, _, _)| *weapon == object)
        .map(|&(_, socket, _)| socket);
    let socket = stow_socket(remembered, |socket| grab::fits(&snapshot, hand, socket))
        .ok_or(DockRefusal::DoesNotFit)?;
    // SAFETY: forwarded from the caller.
    unsafe { dock(hand, socket, hold::SNAP_S) }?;
    Ok(socket)
}

/// The socket a stowed gun goes back on: `remembered`, the one it was last drawn off, when it
/// `fits`, else the first socket that does.
fn stow_socket(remembered: Option<Socket>, fits: impl Fn(Socket) -> bool) -> Option<Socket> {
    remembered
        .filter(|&socket| fits(socket))
        .or_else(|| Socket::ALL.into_iter().find(|&socket| fits(socket)))
}

/// What one tick of a hand's grip does to the gun the hand holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum GripRelease {
    /// The hand keeps it.
    Kept,
    /// The grip opened on it: it is let go of, docking on a socket under the hand.
    LetGo,
    /// The grip opened while the release stood down: it goes back on a socket of the body.
    Stow,
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
/// The game thread, before the world update.
pub(crate) unsafe fn tick(dt: f32) {
    sample_release_transport(dt);
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

/// Stop the snaps and the grip watches. The next [`release_tick`] reads each hand's grip afresh,
/// as after [`cancel_releases`].
pub(crate) fn clear() {
    let mut state = STATE.lock();
    state.snaps = [None; 2];
    state.transport = ReleaseTransport::new();
    state.rig = None;
    state.watches.iter_mut().for_each(Watch::stand_down);
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
#[derive(Debug, Copy, Clone, PartialEq)]
struct Watch {
    release: ReleaseWatch,
    weapon: Option<usize>,
    /// A grab put the weapon in the hand, and the grip has not yet been seen holding it. It waits
    /// for the weapon to reach the table's hand.
    pending: bool,
    since_s: Option<f64>,
    /// The release stood down since the last tick ([`Self::stand_down`]).
    resumed: bool,
    /// The weapon a grip held through a stand-down keeps, until the controller's grip opens.
    held_through: Option<usize>,
}

impl Watch {
    const fn new() -> Self {
        Self {
            release: ReleaseWatch::new(),
            weapon: None,
            pending: false,
            since_s: None,
            resumed: false,
            held_through: None,
        }
    }

    /// Stand the release down: forget the intent and the weapon, and read the controller's grip
    /// afresh on the next tick ([`Self::observe`]).
    fn stand_down(&mut self) {
        self.release.disarm();
        self.pending = false;
        self.weapon = None;
        self.since_s = None;
        self.resumed = true;
        self.held_through = None;
    }

    /// Fold in one input tick ([`release_tick`]): [`Self::observe_release`], after the first tick
    /// back from a stand-down has read the controller's grip as it is: a gun under a closed grip, or
    /// one not known, is kept until that grip reads open, and one under an open grip is stowed.
    fn observe(
        &mut self,
        hand: Hand,
        weapon: Option<usize>,
        reading: GripReading,
        action_hands: [bool; 2],
        now_s: Option<f64>,
    ) -> GripRelease {
        let GripReading { grips, physical } = reading;
        let i = hand.index();
        if std::mem::take(&mut self.resumed) && weapon.is_some() {
            // A grip not known keeps the gun until it reads open.
            if physical[i] != Some(false) {
                self.held_through = weapon;
            } else {
                self.weapon = weapon;
                self.since_s = now_s;
                self.release.unarm();
                self.release.latch_press(None, grips);
                return GripRelease::Stow;
            }
        }
        if let Some(through) = self.held_through {
            if weapon != Some(through) {
                self.held_through = None;
            } else if grips[i] {
                // The masked grip reads the hand again: the ordinary watch takes over.
                self.held_through = None;
                self.pending = true;
            } else if physical[i] == Some(false) {
                self.held_through = None;
                self.weapon = weapon;
                self.release.unarm();
                self.release.latch_press(None, grips);
                return GripRelease::LetGo;
            }
        }
        if self.observe_release(hand, weapon, grips, action_hands, now_s) {
            GripRelease::LetGo
        } else {
            GripRelease::Kept
        }
    }

    fn observe_release(
        &mut self,
        hand: Hand,
        weapon: Option<usize>,
        grips: [bool; 2],
        action_hands: [bool; 2],
        now_s: Option<f64>,
    ) -> bool {
        let holder = weapon.map(|_| hand);
        if weapon != self.weapon {
            self.weapon = weapon;
            self.since_s = now_s;
            self.release.unarm();
        }
        // A grab whose weapon has not reached the table's hand yet keeps waiting for it.
        if self.pending && weapon.is_some() {
            self.pending = false;
            if grips[hand.index()] {
                self.release.arm(hand);
            }
        }
        if self.release.latch_press(holder, grips).is_some() {
            self.since_s = now_s;
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                "hold: the grip closed on the gun in the hand, which it holds from now on"
            );
        }
        self.release.observe_action(holder, grips, action_hands) == Some(hand)
    }
}

struct State {
    /// Each hand's snap and what its end does, by [`Hand::index`].
    snaps: [Option<(Snap, Finish)>; 2],
    /// Each weapon's gun pose on a socket's bone, from its last draw off it: weapon, socket, pose.
    offsets: Vec<(usize, Socket, Pose)>,
    watches: [Watch; 2],
    transport: ReleaseTransport,
    rig: Option<(Instant, RigMotion)>,
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

    /// Start a dock's snap of `weapon`'s gun from `hand` onto `socket` toward `offset` on the
    /// socket's bone, a pose not measured from a draw and so not remembered. Returns whether it
    /// started.
    fn slide_onto(
        &mut self,
        weapon: usize,
        socket: Socket,
        hand: Hand,
        offset: Pose,
        snap_s: f32,
    ) -> bool {
        if snap_s.is_nan() || snap_s <= 0.0 {
            return false;
        }
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
    transport: ReleaseTransport::new(),
    rig: None,
    status: DockStatus {
        snapping: [None; 2],
        grip_holds: [false; 2],
        draws: 0,
        docks: 0,
        drops: 0,
        last_throw_mps: None,
        releases: ReleaseStatus::EMPTY,
    },
});

fn sample_release_transport(dt: f32) {
    let scale = Config::lock_query(|config| config.vr.world_scale);
    let frame = headpose::xr::cockpit_pose().map(|head_cockpit| {
        let head_world = hooks::camera::eye_point();
        ReleaseFrame {
            origin: head_world,
            rotation: headpose::xr::body_rotation(),
            scale,
            teleport_epoch: teleport::epoch(),
            recenters: recenter_count(),
        }
        .with_composed_head(head_world, head_cockpit.position)
    });
    let time_s = motion::latest_time();
    let mut state = STATE.lock();
    state.rig = state
        .transport
        .update(frame, dt, time_s)
        .map(|rig| (Instant::now(), rig));
}

/// Copy the estimate before disarm changes the gun's attach pose or cached native motion.
unsafe fn release_motion(
    hand: Hand,
    weapon: &WeaponBase,
    diagnostic: &ReleaseRecord,
) -> ReleaseMotion {
    let Some(gun) = (unsafe { gun::guns(weapon) }).next().flatten() else {
        return ReleaseMotion::REST;
    };
    let point_world = Mat4::from(gun.m_WorldMatrixT1).w_axis.truncate();
    let since_s = {
        let watch = STATE.lock().watches[hand.index()];
        (watch.weapon == diagnostic.weapon)
            .then_some(watch.since_s)
            .flatten()
            .unwrap_or(f64::INFINITY)
    };
    estimate(hand, point_world, since_s, diagnostic.controller.as_ref())
}

/// The world motion of the point at `point_world` that `hand` lets go of, for an item the hand has
/// held since `since_s` (display time; earlier samples belong to whatever the hand did before).
///
/// This is the estimate a gun's let-go takes, so everything a hand throws leaves it the same way:
/// the hand's timestamped history read at its fastest moment, the rotation's lever at the point,
/// and the player's own movement through the rig, each once. A grenade, a caught grenade, and a
/// charge use it as well.
pub(crate) fn release_at(hand: Hand, point_world: Vec3, since_s: f64) -> ReleaseMotion {
    let controller = frame::with_latest(|frame| ControllerContext::from_frame(frame, hand));
    estimate(hand, point_world, since_s, controller.as_ref())
}

fn estimate(
    hand: Hand,
    point_world: Vec3,
    since_s: f64,
    controller: Option<&ControllerContext>,
) -> ReleaseMotion {
    let (rig, history_after_s) = {
        let state = STATE.lock();
        let rig = state
            .rig
            .filter(|(at, _)| at.elapsed().as_secs_f32() <= MAX_RELEASE_AGE_S);
        (rig.map(|(_, rig)| rig), state.transport.history_after_s())
    };
    let samples = motion::history(hand);
    estimate_release(ReleaseInput {
        rig,
        controller,
        samples: &samples,
        since_s,
        history_after_s,
        point_world,
    })
    // SAFETY: a release happens while the game runs.
    .in_game_time(unsafe { TimeScale::current() })
}

/// Draw the weapon on `socket` into `hand` through the loadout table, which the reconcile carries
/// out at once. Returns where it sat.
unsafe fn draw_off(hand: Hand, socket: Socket) -> Result<Seat, DockRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(DockRefusal::NoCharacter)?;
    let gate = DrawGate {
        // SAFETY: the game thread, on the live local player.
        wield_skipped: unsafe { traversal::wield_skipped(owner) },
        animation_allows: unsafe { owner.m_Inventory.GetAnimationAllowsDrawing() },
    };
    if !gate.allows() {
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

/// What decides whether a draw off a socket may start.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct DrawGate {
    /// Whether the game's weapon updates skip the wielded weapon here
    /// ([`traversal::wield_skipped`]): a car's seat, or the boost wingsuit.
    wield_skipped: bool,
    /// [`Inventory::GetAnimationAllowsDrawing`](jc3gi::character::inventory::Inventory::GetAnimationAllowsDrawing).
    animation_allows: bool,
}

impl DrawGate {
    /// Where the game wields the drawn weapon, its animation must allow a draw. Where its updates
    /// skip the wielded weapon, the reconcile arms the weapon in the hand instead and the game never
    /// wields it, so the seat's animation, which forbids the game's own draw, has no say.
    fn allows(self) -> bool {
        self.wield_skipped || self.animation_allows
    }
}

/// Where the gun of `weapon` would sit on `socket`'s bone with its grip at `toward` (world) and the
/// orientation it has now, in the skeleton's model space: the end of an attracted gun's travel
/// when no draw has measured where it sits there.
///
/// # Safety
///
/// `weapon` must be a weapon a slot or the carry set keeps alive, on the game thread before the
/// world update.
unsafe fn provisional_offset(weapon: usize, socket: Socket, toward: Vec3) -> Option<Pose> {
    let owner = unsafe { engine::local_player() }?;
    // SAFETY: forwarded from the caller.
    let weapon = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let bone = sockets::bone(socket);
    let on_bone = unsafe { equip::pose_on_bone(owner, weapon, bone) }?;
    let bone_pose = unsafe { grip::bone_model_pose(owner, bone) }?;
    let ([grip_world, _], _) = unsafe { grip_points(weapon) }?;
    let character = Mat4::from(owner.m_WorldMatrixT1);
    let moved = shift_on_bone(on_bone, bone_pose, character, toward - grip_world)?;
    Some(moved)
}

/// `on_bone`, a pose on a bone posed at `bone` in the model space of a character whose
/// model-to-world transform is `character`, moved by `world` in the world.
fn shift_on_bone(on_bone: Pose, bone: Pose, character: Mat4, world: Vec3) -> Option<Pose> {
    if character.determinant().abs() <= f32::EPSILON {
        return None;
    }
    let model = character.inverse().transform_vector3(world);
    let local = bone.orientation.inverse() * model;
    let position = on_bone.position + local;
    position.is_finite().then_some(Pose {
        position,
        ..on_bone
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
        let weapon = unsafe { gun::held_weapon(owner) }.ok_or(DockRefusal::NotDone)?;
        let motion = unsafe { release_motion(hand, weapon, diagnostic) };
        unsafe { equip::drop_held(owner, max_speed_mps, motion) }?
    } else if armed::in_hand(hand) == Some(object) {
        diagnostic.route = ReleaseRoute::ArmedInHand;
        let mut handle =
            unsafe { armed::weapon_handle(owner, hand) }.ok_or(DockRefusal::NotDone)?;
        let motion = unsafe { release_motion(hand, handle.as_ref(), diagnostic) };
        diagnostic.stages.before_disarm = true;
        unsafe { armed::disarm(owner, hand, Some(handle), true) };
        diagnostic.stages.disarm_returned = true;
        // SAFETY: the slot or carry set retains the source until release finishes, and the game
        // stage runs before the world update.
        let weapon = unsafe { handle.as_mut() };
        if weapon.m_State != WeaponState::E_WS_HOLSTERED {
            return Err(DockRefusal::NotHolstered);
        }
        let mut thrown = unsafe { equip::drop_carried(owner, weapon, max_speed_mps, motion) };
        thrown.diagnostic.sample_stage = SampleStage::AfterDisarm;
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
    // SAFETY: the caller holds the live local player on the game thread before the world update.
    unsafe { gun::held_weapon(owner) }.is_some_and(|w| w as *const WeaponBase as usize == object)
}

#[cfg(test)]
mod attract_tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn a_pose_on_a_bone_moves_by_a_world_shift() {
        let character = Mat4::from_rotation_translation(
            Quat::from_rotation_y(0.9),
            Vec3::new(100.0, 5.0, -40.0),
        );
        let bone = Pose {
            position: Vec3::new(0.1, 1.3, 0.12),
            orientation: Quat::from_rotation_z(0.4) * Quat::from_rotation_x(-0.3),
        };
        let on_bone = Pose {
            position: Vec3::new(0.05, -0.2, 0.1),
            orientation: Quat::from_rotation_y(0.2),
        };
        let world_of = |pose: Pose| {
            character.transform_point3(bone.position + bone.orientation * pose.position)
        };
        let shift = Vec3::new(0.2, -0.1, 0.3);
        let moved = shift_on_bone(on_bone, bone, character, shift).unwrap();
        assert!(world_of(moved).distance(world_of(on_bone) + shift) < 1e-5);
        assert_eq!(moved.orientation, on_bone.orientation);
        assert_eq!(shift_on_bone(on_bone, bone, Mat4::ZERO, shift), None);
        assert_eq!(shift_on_bone(on_bone, bone, character, Vec3::NAN), None);
    }
}

#[cfg(test)]
mod draw_gate_tests {
    use test_case::test_case;

    use super::*;

    #[test_case(false, true, true; "on foot the animation allows it")]
    #[test_case(false, false, false; "on foot the animation forbids it")]
    #[test_case(true, false, true; "a car's seat arms the gun whatever its animation says")]
    #[test_case(true, true, true; "a seat whose animation allows it")]
    fn a_draw_starts_where_the_game_would_wield_it_or_never_wields(
        wield_skipped: bool,
        animation_allows: bool,
        allowed: bool,
    ) {
        let gate = DrawGate {
            wield_skipped,
            animation_allows,
        };
        assert_eq!(gate.allows(), allowed);
    }
}
