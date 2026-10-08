//! The mounted-gun half of the input tick: reading the gun and deriving its aim from the hands,
//! and which physical controls a seat offers.

use std::time::Instant;

use glam::Vec3;
use jc3gi::character::character::Character;

use crate::{
    controllers::{
        config::ControllersConfig,
        hands::ActiveHands,
        mode::VehicleSeat,
        vehicles::{
            MountedReadout, STATE,
            mounted::{self, MountedTarget},
            physical::ControlKind,
        },
    },
    headpose,
};

/// What the mounted-gun half of the input tick produced.
pub(super) struct MountedTick {
    pub(super) readout: Option<MountedReadout>,
    pub(super) target: Option<MountedTarget>,
    pub(super) handles: Option<[Option<Vec3>; 2]>,
}

/// Read the mounted gun and derive its aim from the hands.
pub(super) fn mounted_tick(active: &ActiveHands) -> MountedTick {
    let config = &active.config;
    let none = MountedTick {
        readout: None,
        target: None,
        handles: None,
    };
    if !(config.vehicles.mounted.aim_by_hands || config.vehicles.mounted.anchor_hands) {
        STATE.lock().mounted_reference = None;
        return none;
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    let Some(gun) = (unsafe {
        Character::GetLocalPlayerCharacter()
            .as_ref()
            .and_then(|owner| mounted::read(owner))
    }) else {
        STATE.lock().mounted_reference = None;
        return none;
    };
    let head = headpose::query();
    let body = headpose::xr::body_rotation();
    let to_cockpit = |world: Vec3| {
        active.head.position
            + body.inverse() * (world - head.position) / active.world_scale.max(1e-3)
    };
    let handles = gun.handles.map(|h| h.map(to_cockpit));
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));

    let aim = config.vehicles.mounted.aim_by_hands;
    let direction = mounted::hands_midpoint(grips)
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
                    config.vehicles.mounted.min_lever_m,
                )
            });
            state.mounted_reference = reference;
            drop(state);
            mounted::aim_direction(&reference?, mid).map(|d| body * d)
        });
    let target = direction.map(|d| MountedTarget {
        weapon: gun.address,
        point: gun.pivot + d * config.aim.max_distance_m.max(1.0),
        at: Instant::now(),
    });
    MountedTick {
        readout: Some(MountedReadout {
            gun,
            direction,
            target: target.map(|t| t.point),
        }),
        target,
        handles: config.vehicles.mounted.anchor_hands.then_some(handles),
    }
}

/// The physical controls `seat` offers with their toggles on: only the driver's seat has any. A
/// vehicle with a steering fork (`fork`) offers handlebars.
pub(super) fn offered(
    seat: Option<VehicleSeat>,
    config: &ControllersConfig,
    fork: bool,
) -> &'static [ControlKind] {
    let physical = &config.vehicles.physical;
    let Some(seat) = seat.filter(|s| s.driver && physical.any()) else {
        return &[];
    };
    let kinds = ControlKind::for_seat(seat.class, fork);
    match kinds {
        [one] if one.enabled(physical) => kinds,
        [first, second] => match (first.enabled(physical), second.enabled(physical)) {
            (true, true) => kinds,
            (true, false) => &kinds[..1],
            (false, true) => &kinds[1..],
            (false, false) => &[],
        },
        _ => &[],
    }
}
