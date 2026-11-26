//! The mounted-gun half of the input tick: reading the gun and deriving its aim from the hands,
//! and which physical controls a seat offers.

use std::time::Instant;

use glam::Vec3;
use jc3vr_body::controllers::{
    aim::ray_length_m, grab::cockpit::ControlKind, hands::ActiveHands, mode::VehicleSeat,
    physics::local_player::LocalPlayer,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;
use jc3vr_view::controllers::cockpit::CockpitTransform;

use crate::controllers::vehicles::{
    mounted, mounted::MountedTarget, state::STATE, tuning::VEHICLES, types::MountedReadout,
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
    game: &GameTick<'_>,
    active: &ActiveHands,
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
    let xf = CockpitTransform::current(active.head.position, active.world_scale);
    let hands_world = Hand::BOTH
        .map(|hand| active.hands[hand.index()].map(|hand| xf.point_to_world(hand.grip.position)));
    let local_player = LocalPlayer::get(game);
    // SAFETY: the local player, in the game tick.
    let Some(gun) = (unsafe {
        local_player
            .as_ref()
            .and_then(|owner| mounted::read(owner.character()))
    })
    .or_else(|| unsafe { mounted::nearby(hands_world, VEHICLES.get().mounted.nearby_reach_m) }) else {
        let mut state = STATE.lock();
        state.mounted_reference = None;
        state.mounted_last = None;
        return none;
    };
    keep_gunner_visible(game);
    let to_cockpit = |world: Vec3| xf.point_to_cockpit(world);
    let body = xf.body;
    let handles = gun.handles.map(|h| h.map(to_cockpit));
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let grips = Hand::BOTH.map(|hand| grips[hand.index()].filter(|_| aiming[hand.index()]));

    let from_hands = mounted::hands_midpoint(grips).and_then(|(mid, tracked)| {
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
                VEHICLES.get().mounted.min_lever_m,
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
            None => state
                .mounted_last
                .filter(|(weapon, _)| *weapon == gun.address)
                .map(|(_, d)| body * d),
        }
    };
    let target = direction.map(|d| MountedTarget {
        weapon: gun.address,
        point: gun.pivot + d * ray_length_m(),
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
fn keep_gunner_visible(game: &GameTick<'_>) {
    let Some(mut local_player) = LocalPlayer::get(game) else {
        return;
    };
    let local = local_player.character_mut();
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

/// The physical controls `seat` offers: only the driver's seat has any. A vehicle with a steering
/// fork (`fork`) offers handlebars, and a plane whose cockpit has a control wheel (`yoke`) offers
/// the yoke.
pub(super) fn offered(seat: Option<VehicleSeat>, fork: bool, yoke: bool) -> &'static [ControlKind] {
    match seat.filter(|s| s.driver) {
        Some(seat) => ControlKind::for_seat(seat.class, fork, yoke),
        None => &[],
    }
}
