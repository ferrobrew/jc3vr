//! The mounted-gun half of the input tick: reading the gun and deriving its aim from the hands,
//! and which physical controls a seat offers.

use std::time::Instant;

use glam::Vec3;
use jc3gi::character::character::Character;
use jc3vr_input::frame::Hand;
use jc3vr_render::controllers::cockpit::CockpitTransform;

use crate::controllers::{
    aim::RAY_LENGTH_M,
    hands::ActiveHands,
    mode::VehicleSeat,
    vehicles::{
        config::{PhysicalControlsConfig, VehicleSettings},
        mounted::{self, MountedTarget},
        physical::{ControlKind, control_enabled},
        state::STATE,
        types::MountedReadout,
    },
};

/// What the mounted-gun half of the input tick produced.
pub(super) struct MountedTick {
    pub(super) readout: Option<MountedReadout>,
    /// The gun the input tick selected, whether or not the hands are aiming it.
    pub(super) weapon: Option<usize>,
    pub(super) target: Option<MountedTarget>,
    pub(super) handles: Option<[Option<Vec3>; 2]>,
    /// The gun's handles in the cockpit frame, where it places them, for their grab volumes.
    pub(super) handle_points: [Option<Vec3>; 2],
    /// The barrel's direction in the cockpit frame, for the recoil's kick.
    pub(super) barrel: Option<Vec3>,
}

/// Read the mounted gun and derive its aim from the hands in `aiming`, indexed by
/// [`Hand::index`](jc3vr_input::frame::Hand::index). With the grip on the handles and no hand
/// aiming, the gun keeps the direction the hands last gave it, in the body frame.
pub(super) fn mounted_tick(
    active: &ActiveHands,
    config: &VehicleSettings,
    aiming: [bool; 2],
) -> MountedTick {
    let none = MountedTick {
        readout: None,
        weapon: None,
        target: None,
        handles: None,
        handle_points: [None; 2],
        barrel: None,
    };
    if !config.vehicles.mounted.aim_by_hands {
        let mut state = STATE.lock();
        state.mounted_reference = None;
        state.mounted_last = None;
        return none;
    }
    let xf = CockpitTransform::current(active.head.position, active.world_scale);
    let hands_world = Hand::BOTH
        .map(|hand| active.hands[hand.index()].map(|hand| xf.point_to_world(hand.grip.position)));
    // SAFETY: the game stage runs on the game thread before the world update.
    let Some(gun) = (unsafe {
        Character::GetLocalPlayerCharacter()
            .as_ref()
            .and_then(|owner| mounted::read(owner))
    })
    .or_else(|| unsafe { mounted::nearby(hands_world, NEARBY_REACH_M) }) else {
        let mut state = STATE.lock();
        state.mounted_reference = None;
        state.mounted_last = None;
        return none;
    };
    // SAFETY: as above.
    unsafe { keep_gunner_visible() };
    let to_cockpit = |world: Vec3| xf.point_to_cockpit(world);
    let body = xf.body;
    let handles = gun.handles.map(|h| h.map(to_cockpit));
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let grips = Hand::BOTH.map(|hand| grips[hand.index()].filter(|_| aiming[hand.index()]));

    let aim = config.vehicles.mounted.aim_by_hands;
    let from_hands = mounted::hands_midpoint(grips)
        .filter(|_| aim)
        .and_then(|(mid, tracked)| {
            let mut state = STATE.lock();
            let current = state
                .mounted_reference
                .filter(|r| r.weapon == gun.address && r.hands == tracked);
            let reference = current.or_else(|| {
                let handles_mid = match handles {
                    [Some(l), Some(r)] => Some((l + r) * 0.5),
                    [one, other] => one.or(other),
                };
                mounted::take_reference(
                    gun.address,
                    tracked,
                    mid,
                    to_cockpit(gun.pivot),
                    handles_mid,
                    body.inverse() * gun.barrel,
                    MIN_LEVER_M,
                )
            });
            state.mounted_reference = reference;
            drop(state);
            mounted::aim_direction(&reference?, mid).map(|d| body * d)
        });
    let direction = {
        let mut state = STATE.lock();
        match from_hands {
            Some(d) => {
                state.mounted_last = Some((gun.address, body.inverse() * d));
                Some(d)
            }
            None if aim && config.vehicles.mounted.grip_handles => state
                .mounted_last
                .filter(|(weapon, _)| *weapon == gun.address)
                .map(|(_, d)| body * d),
            None => None,
        }
    };
    let target = direction.map(|d| MountedTarget {
        weapon: gun.address,
        point: gun.pivot + d * RAY_LENGTH_M,
        at: Instant::now(),
    });
    MountedTick {
        readout: Some(MountedReadout {
            gun,
            direction,
            target: target.map(|t| t.point),
        }),
        weapon: Some(gun.address),
        target,
        handles: Some(handles),
        handle_points: handles,
        barrel: Some(body.inverse() * gun.barrel),
    }
}

/// Fade the local player back in while they man a mounted gun under the controllers. The gun's
/// start of use fades a local player user out and holds them transparent for as long as they man
/// it, for the game's turret camera; in the headset the view is at the head, so a gunner's body,
/// arms, and hands went undrawn (the gunner seat on the 14 November take, 536 to 550 s).
///
/// # Safety
///
/// The game stage, on the game thread before the world update.
unsafe fn keep_gunner_visible() {
    let Some(local) = (unsafe { Character::GetLocalPlayerCharacter().as_mut() }) else {
        return;
    };
    if local.m_FadingIn || !unsafe { local.IsInMountedGunState() } {
        return;
    }
    unsafe { local.FadeCharacterInOut(true, GUNNER_FADE_IN_S) };
    tracing::info!(
        target: "controllers",
        "mounted gun: faded the gunner back in, whom the gun's use had faded out"
    );
}

/// How long the gunner takes to fade back in, seconds: the game's own fade's length.
const GUNNER_FADE_IN_S: f32 = 0.25;

/// The physical controls `seat` offers with their toggles on: only the driver's seat has any. A
/// vehicle with a steering fork (`fork`) offers handlebars, and a plane whose cockpit has a control
/// wheel (`yoke`) offers the yoke.
pub(super) fn offered(
    seat: Option<VehicleSeat>,
    physical: &PhysicalControlsConfig,
    fork: bool,
    yoke: bool,
) -> &'static [ControlKind] {
    let Some(seat) = seat.filter(|s| s.driver && physical.any()) else {
        return &[];
    };
    let kinds = ControlKind::for_seat(seat.class, fork, yoke);
    match kinds {
        [one] if control_enabled(*one, physical) => kinds,
        [first, second] => match (
            control_enabled(*first, physical),
            control_enabled(*second, physical),
        ) {
            (true, true) => kinds,
            (true, false) => &kinds[..1],
            (false, true) => &kinds[1..],
            (false, false) => &[],
        },
        _ => &[],
    }
}

/// The shortest lever from the handles to the gun's pivot, metres. A gun whose handles the game
/// does not place, or places at the pivot, swings about a pivot this far ahead of them.
const MIN_LEVER_M: f32 = 0.4;

/// How near a hand must be to a mounted gun's handles or pivot to select it without the native
/// mounted state, metres.
const NEARBY_REACH_M: f32 = 2.0;
