//! The sockets' grab volumes, seated from the loadout table's snapshot.
//!
//! A docked weapon sits where its socket's bone puts it on the character's skeleton (a rifle across
//! the back, a pistol on the thigh), wherever the animation and the procedural body have that bone,
//! so each socket's volume is seated where the player sees the weapon, and reached by the hand as
//! it is drawn:
//!
//! - a socket holding a weapon has a volume on the weapon's grip, for an empty hand to draw it;
//! - a free socket that takes the weapon a hand holds has a volume where that weapon would sit
//!   there, for that hand to dock it: where its grip was on the socket's bone when it was last
//!   seen there, or else on the socket's bone, moved by the socket's offset when the game does not
//!   dock the weapon's slot there itself;
//! - a socket of the body that does not take the weapon a hand holds (taken, or too small for it,
//!   or any socket for a weapon without a slot, which docks nowhere, [`act::dockable`]) is marked
//!   for that hand, never taken, so the hand knows before it opens.
//!
//! A free socket that takes a held weapon also attracts a release of it from beyond its volume
//! ([`attract`]), so a weapon let go of over the shoulder travels onto the back.
//!
//! What each grab does is the table's decision ([`crate::controllers::loadout::grab`]). The points
//! are read on the input tick from the guns' last placement, in the world, and carried into the
//! cockpit frame the volumes live in ([`cockpit_frames`]).

use std::time::Instant;

use body_rig::body::{PlayerBody, neck_pivot};
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::{Character, SafeBoneIndex},
    equipment::weapon::{WeaponBase, WeaponState},
    types::math::Matrix4,
};
use jc3vr_body::controllers::{
    body,
    body::rig::frames::Frames,
    hands,
    hands::{ActiveHands, grip, ik},
    held::native::{gun_count, gun_ptr},
    roomscale,
    roomscale::{SeatPhase, posture::EffectivePosture},
};
use jc3vr_core::published::Stamp;
use jc3vr_input::frame::Hand;
use jc3vr_view::{controllers::cockpit::CockpitTransform, vr::input::flight_pose};
use parking_lot::Mutex;

use crate::controllers::{
    loadout::{LoadoutSnapshot, Place, Socket, grab, grab::SocketMove, live},
    weapons::{
        docks::attachments::{Attachment, Missing, Occupancy, Placement, PoseSource, Sample},
        engine::{grip_points, slot_of},
        grenade,
        hold::{act, armed, tuning::HOLD},
        inventory::{carry, engine::ObjectKey, slots, sockets, sockets::Offsets},
    },
};

pub(crate) mod attachments;
pub(crate) mod attract;

/// The socket targets the input tick last published, carried onto `character`, the owner's
/// current model-to-world transform, for the body mirror. Reads no weapon or joint.
pub(crate) fn attachment_sample(owner: usize, character: Mat4, now: Instant) -> Option<Sample> {
    ATTACHMENTS.lock().as_ref()?.in_world(owner, character, now)
}

/// Publish the sample the socket candidates consume on this input tick, or `None` to withdraw it.
pub(crate) fn publish_targets(sample: Option<Sample>) {
    *ATTACHMENTS.lock() = sample;
}

/// The selected socket pose copied from this input tick, in world space.
pub(crate) fn socket_world_pose(socket: Socket) -> Option<Mat4> {
    let attachments = ATTACHMENTS.lock();
    let sample = attachments.as_ref()?;
    (Instant::now().checked_duration_since(sample.at)? <= attachments::max_age()).then_some(())?;
    sample
        .attachments
        .iter()
        .find(|attachment| attachment.socket == socket && attachment.hand.is_none())?
        .pose
        .ok()
        .map(|pose| pose.to_mat4())
}

/// What a socket's volume is for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SocketUse {
    /// The weapon on it, for an empty hand to draw.
    Draw,
    /// The free socket, for the hand holding a weapon that fits it to dock it.
    Dock,
    /// A socket that does not take what the hand holds: marked, never taken.
    NoFit,
}

/// One socket's volume for one hand, in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct SocketPoint {
    pub socket: Socket,
    pub hand: Hand,
    pub point: Vec3,
    pub usage: SocketUse,
}

/// What the volumes are placed from.
pub(crate) struct SocketInputs<'a> {
    pub snapshot: &'a LoadoutSnapshot,
    /// The hands that hold nothing, by [`Hand::index`]: only they draw.
    pub free: [bool; 2],
    /// Where a weapon sits on each socket relative to its bone, for a socket the game does not
    /// dock it on.
    pub offsets: &'a Offsets,
}

/// Observe native socket poses and existing hand offers for target selection on this input tick.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn socket_points(owner: &Character, inputs: &SocketInputs) -> Sample {
    let snapshot = inputs.snapshot;
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    let docked = unsafe { socket_weapons(owner, snapshot) };
    let waist = seated_waist(owner);
    let offset = |socket| {
        if waist.is_some() && matches!(socket, Socket::Hip(_)) {
            Vec3::ZERO
        } else {
            inputs.offsets.of(socket)
        }
    };
    let bones = Socket::ALL.map(|socket| {
        if snapshot.body.has(socket) || snapshot.docked(socket).is_some() {
            waist
                .as_ref()
                .and_then(|waist| waist[socket.index()])
                .or_else(|| unsafe { bone_world(owner, to_world, socket) })
        } else {
            None
        }
    });
    let mut grips = GRIPS.lock();
    let mut sample = Sample {
        owner: owner as *const Character as usize,
        at: Instant::now(),
        character: to_world,
        attachments: Vec::new(),
    };
    let mut seen = Vec::new();
    let observed = docked.iter().map(|(socket, _)| *socket).chain(
        Socket::ALL
            .into_iter()
            .filter(|socket| !docked.iter().any(|(seen, _)| seen == socket)),
    );
    for socket in observed {
        let occupant = snapshot.docked(socket);
        if occupant.is_none() && !snapshot.body.has(socket) {
            continue;
        }
        let occupancy = occupant.map_or(Occupancy::Empty, |item| Occupancy::Occupied {
            object: item.binding.address().map(|address| address.get()),
        });
        let safe_occupant = occupant.is_some_and(|item| item.binding.safe_item().is_some());
        let (source, pose) = if let Some(&(_, weapon)) = docked.iter().find(|(s, _)| *s == socket) {
            let object = weapon as *const WeaponBase as ObjectKey;
            let point = unsafe { grip_point(weapon) };
            if let Some(point) = point {
                if let Some(bone) = bones[socket.index()]
                    && let Some(local) = grip_on_bone(bone, point)
                {
                    grips.retain(|g| !(g.object == object && g.socket == socket));
                    grips.push(DockGrip {
                        object,
                        socket,
                        local,
                    });
                }
                seen.push(object);
            }
            attachments::socket_pose(
                to_world,
                bones[socket.index()],
                Placement::Visible {
                    object,
                    grip: point,
                },
            )
        } else if safe_occupant && matches!(socket, Socket::Hip(_)) {
            attachments::socket_pose(
                to_world,
                bones[socket.index()],
                Placement::Free {
                    object: None,
                    remembered: None,
                    native: false,
                    offset: offset(socket),
                },
            )
        } else if let Occupancy::Occupied { object, .. } = occupancy {
            (
                PoseSource::UnavailableWeapon { object },
                Err(Missing::Weapon),
            )
        } else {
            attachments::socket_pose(
                to_world,
                bones[socket.index()],
                Placement::Free {
                    object: None,
                    remembered: None,
                    native: false,
                    offset: offset(socket),
                },
            )
        };
        let attachment = Attachment {
            socket,
            hand: None,
            usage: None,
            occupancy,
            item: occupant.map(|item| item.id),
            source,
            pose,
        };
        sample.attachments.push(attachment);
        if safe_occupant
            && matches!(socket, Socket::Hip(_))
            && let Ok(pose) = pose
        {
            grenade::safe::place_stowed(socket, pose.to_mat4());
        }
        if matches!(source, PoseSource::VisibleGrip { .. }) || safe_occupant {
            for hand in Hand::BOTH {
                let draws = inputs.free[hand.index()]
                    && if safe_occupant {
                        matches!(socket, Socket::Hip(_)) && snapshot.held(hand).is_none()
                    } else {
                        grab::grab(snapshot, hand, socket) == Ok(SocketMove::Draw { socket, hand })
                    };
                if draws {
                    sample.attachments.push(Attachment {
                        hand: Some(hand),
                        usage: Some(SocketUse::Draw),
                        ..attachment
                    });
                }
            }
        }
    }
    for hand in Hand::BOTH {
        if grenade::safe::in_hand(hand).is_none() {
            continue;
        }
        for socket in Hand::BOTH.map(Socket::Hip) {
            let Some(base) = sample
                .attachments
                .iter()
                .find(|attachment| attachment.socket == socket)
                .copied()
            else {
                continue;
            };
            if snapshot.body.has(socket) && base.occupancy == Occupancy::Empty && base.pose.is_ok()
            {
                sample.attachments.push(Attachment {
                    hand: Some(hand),
                    usage: Some(SocketUse::Dock),
                    ..base
                });
            }
        }
    }
    for hand in Hand::BOTH {
        let Some(object) = held_object(snapshot, hand) else {
            continue;
        };
        seen.push(object);
        // SAFETY: the table holds the weapon in a hand, and a slot or the carry set keeps it
        // alive.
        let dockable = unsafe { act::dockable(object) };
        for socket in Socket::ALL {
            let usage = if !dockable && snapshot.body.has(socket) {
                SocketUse::NoFit
            } else if grab::fits(snapshot, hand, socket) {
                SocketUse::Dock
            } else if grab::refusing(snapshot, hand).any(|s| s == socket) {
                SocketUse::NoFit
            } else {
                continue;
            };
            let Some(base) = sample
                .attachments
                .iter()
                .find(|a| a.socket == socket)
                .copied()
            else {
                continue;
            };
            let (source, pose) = if matches!(base.source, PoseSource::VisibleGrip { .. }) {
                (base.source, base.pose)
            } else {
                let remembered = (!(waist.is_some() && matches!(socket, Socket::Hip(_))))
                    .then(|| {
                        grips
                            .iter()
                            .find(|g| g.object == object && g.socket == socket)
                            .map(|g| g.local)
                    })
                    .flatten();
                // SAFETY: the table holds the weapon in a hand, and a slot or the carry set
                // keeps it alive, as for the existing dockability read.
                let native = unsafe { (object as *const WeaponBase).as_ref() }
                    .and_then(|weapon| unsafe { slot_of(weapon) })
                    .is_some_and(|slot| socket.is_native(slot));
                attachments::socket_pose(
                    to_world,
                    bones[socket.index()],
                    Placement::Free {
                        object: Some(object),
                        remembered,
                        native,
                        offset: offset(socket),
                    },
                )
            };
            sample.attachments.push(Attachment {
                hand: Some(hand),
                usage: Some(usage),
                source,
                pose,
                ..base
            });
        }
    }
    grips.retain(|g| seen.contains(&g.object));
    drop(grips);
    sample
}

pub(crate) unsafe fn place_waist(owner: &Character) {
    if ik::local_character() != owner as *const Character as usize {
        return;
    }
    let Some(waist) = seated_waist(owner) else {
        return;
    };
    let Some(snapshot) = live::snapshot_now() else {
        return;
    };
    let character = Mat4::from(owner.m_WorldMatrixT1);
    for (socket, weapon) in unsafe { socket_weapons(owner, &snapshot) } {
        let Socket::Hip(side) = socket else {
            continue;
        };
        let Some((grips, _)) = (unsafe { grip_points(weapon) }) else {
            continue;
        };
        for index in 0..unsafe { gun_count(weapon) } {
            let socket = Socket::Hip(if index == 0 { side } else { side.other() });
            let Some(target) = waist[socket.index()] else {
                continue;
            };
            let Some(native) = (unsafe { bone_world(owner, character, socket) }) else {
                continue;
            };
            let Some(mut gun) = (unsafe { gun_ptr(weapon, index) }) else {
                continue;
            };
            let gun = unsafe { gun.as_mut() };
            let current = Mat4::from(gun.m_WorldMatrixT1);
            let copied = Mat4::from(gun.m_WorldMatrixT0) == current;
            let grip = grips[index.min(grips.len() - 1)];
            let delta = attachments::waist_relocation(native, target, grip);
            let placed = delta * current;
            if placed.is_finite() {
                gun.m_WorldMatrixT1 = Matrix4::from(placed);
                if copied {
                    gun.m_WorldMatrixT0 = gun.m_WorldMatrixT1;
                }
            }
        }
    }
}

fn seated_waist(owner: &Character) -> Option<[Option<Mat4>; Socket::COUNT]> {
    let frame = roomscale::frame_shared()?;
    if !frame.fresh(Stamp::now())
        || !(frame.posture == EffectivePosture::Seated || frame.seat == SeatPhase::Locked)
    {
        *SEATED_NECK.lock() = None;
        return None;
    }
    let active = hands::active_hands_raw()?;
    let frames = cockpit_frames(&active, Mat4::from(owner.m_WorldMatrixT1))?;
    let player = body::player();
    let dimensions = PlayerBody::from_heights(player.height, player.eye_height);
    // The waist hangs from the neck as it was when the player sat, not as it is: a player leaning
    // to look at a thigh or reaching toward the knees does not move their hips, and on the
    // 14 November take a lean of half a metre carried a docked SMG out through the cabin wall.
    // The neck is taken only once the player's posture reads seated, since the seat locks before a
    // standing player sits down, and afresh at every change of the cockpit frame (a recenter, a
    // bank, the seat lock's yaw re-base), which would otherwise turn a neck kept in it away from
    // the player.
    let live_neck = neck_pivot(active.head.position, active.head.orientation);
    let neck = if frame.posture == EffectivePosture::Seated {
        let epoch = flight_pose::epoch();
        let mut latched = SEATED_NECK.lock();
        match *latched {
            Some((neck, at)) if at == epoch => neck,
            _ => {
                *latched = Some((live_neck, epoch));
                live_neck
            }
        }
    } else {
        *SEATED_NECK.lock() = None;
        live_neck
    };
    let waist = neck - Vec3::Y * dimensions.spine_length(dimensions.standing_neck());
    let remaps = hands::arm_remaps(
        &active.config,
        active.head,
        active.torso,
        active.world_scale,
    );
    let controller = unsafe { owner.m_AnimatedModel.m_AnimationController.as_ref() }?;
    let bind = |bone| {
        let index = unsafe { owner.GetSafeIndex(bone) };
        if index == 0 {
            return None;
        }
        let mut matrix = Matrix4::from(Mat4::IDENTITY);
        unsafe { controller.GetBindPoseBoneMatrix(index, &mut matrix) };
        let matrix = Mat4::from(matrix);
        (matrix.is_finite() && matrix.determinant().abs() > 1e-6).then_some(matrix)
    };
    let hips = bind(SafeBoneIndex::HIPS)?;
    let mut sockets = [None; Socket::COUNT];
    for hand in Hand::BOTH {
        let socket = Socket::Hip(hand);
        let local = bind(sockets::bone(socket))?;
        let (_, rotation, position) = local.to_scale_rotation_translation();
        let position = position - hips.w_axis.truncate();
        let cockpit = waist + active.torso * Vec3::new(position.x, 0.0, position.z);
        let cockpit = remaps[hand.index()].map_or(cockpit, |remap| remap.apply(cockpit));
        let orientation = frames.cockpit.rotation_to_world(active.torso * rotation);
        sockets[socket.index()] = Some(Mat4::from_rotation_translation(
            orientation,
            frames.cockpit.point_to_world(cockpit),
        ));
    }
    Some(sockets)
}

/// The weapons the table has on the body's sockets that are seen there: each one live (a slot or
/// the carry set keeps it), holstered, and not armed in a hand.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn socket_weapons(
    owner: &Character,
    snapshot: &LoadoutSnapshot,
) -> Vec<(Socket, &'static WeaponBase)> {
    snapshot
        .items
        .iter()
        .filter_map(|item| {
            let Place::Socket(socket) = item.place else {
                return None;
            };
            let object = item.binding.address()?.get();
            let in_slot = slots::contains(&owner.m_Inventory, object);
            if !(in_slot || carry::contains(object)) || armed::is(object) {
                return None;
            }
            // SAFETY: the slot, or the carry set, keeps the weapon alive.
            let weapon = unsafe { (object as *const WeaponBase).as_ref() }?;
            (weapon.m_State == WeaponState::E_WS_HOLSTERED).then_some((socket, weapon))
        })
        .collect()
}

/// Where a grip at `grip` in the world is on a bone at `bone` (bone-to-world), in the bone's frame;
/// `None` for an offset longer than `docks.max_grip_offset_m`, which no socket gives, or a bad read.
pub(crate) fn grip_on_bone(bone: Mat4, grip: Vec3) -> Option<Vec3> {
    let local = bone.inverse().transform_point3(grip);
    (local.is_finite() && local.length() <= HOLD.get().docks.max_grip_offset_m).then_some(local)
}

/// The transforms between the world and the cockpit frame on this input tick, with `character`
/// the character's model-to-world transform.
pub(crate) fn cockpit_frames(active: &ActiveHands, character: Mat4) -> Option<Frames> {
    let frames = Frames {
        cockpit: CockpitTransform::current(active.head.position, active.world_scale),
        character,
    };
    frames.is_usable().then_some(frames)
}

/// A docked weapon's grip on its socket's bone, as last seen.
#[derive(Debug, Copy, Clone)]
struct DockGrip {
    object: ObjectKey,
    socket: Socket,
    local: Vec3,
}

/// The weapons' grips on their sockets' bones, for the weapons the table has on the body.
static GRIPS: Mutex<Vec<DockGrip>> = Mutex::new(Vec::new());

/// The neck pivot the seated waist hangs from, in the cockpit frame, as it was when the player sat,
/// with the cockpit frame's source epoch it was taken at.
static SEATED_NECK: Mutex<Option<(Vec3, Option<u64>)>> = Mutex::new(None);

/// The object of the weapon `hand` holds in `snapshot`.
fn held_object(snapshot: &LoadoutSnapshot, hand: Hand) -> Option<ObjectKey> {
    snapshot
        .held(hand)
        .filter(|item| item.kind.weapon().is_some())
        .and_then(|item| item.binding.address())
        .map(|address| address.get())
}

/// Where a hand closes on `weapon`, in the world: its grip, or its first gun's origin without one.
unsafe fn grip_point(weapon: &WeaponBase) -> Option<Vec3> {
    let (points, _) = unsafe { grip_points(weapon) }?;
    points[0].is_finite().then_some(points[0])
}

/// The bone `socket` places a weapon on, bone-to-world.
unsafe fn bone_world(owner: &Character, to_world: Mat4, socket: Socket) -> Option<Mat4> {
    let pose = unsafe { grip::bone_model_pose(owner, sockets::bone(socket)) }?;
    let bone = to_world * pose.to_mat4();
    bone.is_finite().then_some(bone)
}

/// Copied observations only: the body mirror never keeps or dereferences an engine object.
static ATTACHMENTS: Mutex<Option<Sample>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn a_grip_noted_on_its_bone_follows_the_bone() {
        let bone =
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(0.3, 1.2, -0.4));
        let grip = Vec3::new(0.45, 1.1, -0.3);
        let local = grip_on_bone(bone, grip).expect("a grip near its bone");
        assert!(bone.transform_point3(local).distance(grip) < 1e-5);
        // The bone moves and turns; the grip moves with it.
        let moved =
            Mat4::from_rotation_translation(Quat::from_rotation_y(-0.4), Vec3::new(1.0, 1.0, 1.0));
        let expected = moved.transform_point3(bone.inverse().transform_point3(grip));
        assert!(moved.transform_point3(local).distance(expected) < 1e-5);
    }

    #[test]
    fn a_far_grip_is_a_bad_read() {
        assert_eq!(grip_on_bone(Mat4::IDENTITY, Vec3::new(5.0, 0.0, 0.0)), None);
        assert_eq!(grip_on_bone(Mat4::IDENTITY, Vec3::NAN), None);
    }
}
