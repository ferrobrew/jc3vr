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
//!   or any socket for a weapon that docks nowhere yet, [`act::dockable`]) is marked for that
//!   hand, never taken, so the hand knows before it opens.
//!
//! What each grab does is the table's decision ([`crate::controllers::loadout::grab`]). The points
//! are read on the input tick from the guns' last placement, in the world, and carried into the
//! cockpit frame the volumes live in ([`cockpit_frames`]).

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::rig::frames::Frames,
        frame::Hand,
        hands::{ActiveHands, grip},
        loadout::{
            LoadoutSnapshot, Place, Socket,
            grab::{self, GrabRules, SocketMove},
        },
        weapons::{
            hold::{act, armed, steal},
            inventory::{
                carry,
                engine::{ObjectKey, slot_of},
                sockets::{self, Offsets},
            },
        },
    },
    headpose,
};

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
    pub rules: GrabRules,
    /// Where a weapon sits on each socket relative to its bone, for a socket the game does not
    /// dock it on.
    pub offsets: &'a Offsets,
}

/// The sockets' volumes for each hand, in the world.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn socket_points(owner: &Character, inputs: &SocketInputs) -> Vec<SocketPoint> {
    let snapshot = inputs.snapshot;
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    let docked = unsafe { socket_weapons(owner, snapshot) };
    let mut grips = GRIPS.lock();
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for &(socket, weapon) in &docked {
        let object = weapon as *const WeaponBase as ObjectKey;
        let Some(point) = (unsafe { grip_point(weapon) }) else {
            continue;
        };
        if let Some(bone) = unsafe { bone_world(owner, to_world, socket) }
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
        for hand in Hand::BOTH {
            let draws = inputs.free[hand.index()]
                && grab::grab(snapshot, hand, socket, inputs.rules)
                    == Ok(SocketMove::Draw { socket, hand });
            if draws {
                out.push(SocketPoint {
                    socket,
                    hand,
                    point,
                    usage: SocketUse::Draw,
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
            let on_it = docked.iter().find(|(s, _)| *s == socket);
            let point = match on_it {
                Some(&(_, weapon)) => unsafe { grip_point(weapon) },
                None => unsafe {
                    dock_point(owner, to_world, socket, object, &grips, inputs.offsets)
                },
            };
            if let Some(point) = point {
                out.push(SocketPoint {
                    socket,
                    hand,
                    point,
                    usage,
                });
            }
        }
    }
    grips.retain(|g| seen.contains(&g.object));
    out
}

/// The weapons the table has on the body's sockets that are seen there: each one live (a slot or
/// the carry set keeps it), holstered, and not armed in a hand.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
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
            let in_slot = owner
                .m_Inventory
                .m_Weapons
                .iter()
                .any(|slot| slot.px as usize == object);
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
/// `None` for an offset longer than [`MAX_GRIP_OFFSET_M`], which no socket gives, or a bad read.
pub fn grip_on_bone(bone: Mat4, grip: Vec3) -> Option<Vec3> {
    let local = bone.inverse().transform_point3(grip);
    (local.is_finite() && local.length() <= MAX_GRIP_OFFSET_M).then_some(local)
}

/// The longest grip offset from a socket's bone, metres.
pub const MAX_GRIP_OFFSET_M: f32 = 0.6;

/// The transforms between the world and the cockpit frame on this input tick, with `character`
/// the character's model-to-world transform.
pub(crate) fn cockpit_frames(active: &ActiveHands, character: Mat4) -> Option<Frames> {
    let frames = Frames {
        head_world: headpose::query().position,
        head_cockpit: active.head.position,
        view: headpose::xr::body_rotation(),
        world_scale: active.world_scale,
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
    let (points, _) = unsafe { steal::gun_points(weapon) }?;
    points[0].is_finite().then_some(points[0])
}

/// The bone `socket` places a weapon on, bone-to-world.
unsafe fn bone_world(owner: &Character, to_world: Mat4, socket: Socket) -> Option<Mat4> {
    let pose = unsafe { grip::bone_model_pose(owner, sockets::bone(socket)) }?;
    let bone = to_world * Mat4::from_rotation_translation(pose.orientation, pose.position);
    bone.is_finite().then_some(bone)
}

/// Where the grip of the weapon at `object` would be on the free `socket`, in the world: where it
/// was on the socket's bone when last seen there, or else at the bone, moved by the socket's offset
/// when the game does not dock the weapon's slot there.
unsafe fn dock_point(
    owner: &Character,
    to_world: Mat4,
    socket: Socket,
    object: ObjectKey,
    grips: &[DockGrip],
    offsets: &Offsets,
) -> Option<Vec3> {
    let bone = unsafe { bone_world(owner, to_world, socket) }?;
    if let Some(grip) = grips
        .iter()
        .find(|g| g.object == object && g.socket == socket)
    {
        return Some(bone.transform_point3(grip.local)).filter(|p| p.is_finite());
    }
    // SAFETY: the table holds the weapon in a hand, and a slot or the carry set keeps it alive.
    let native = unsafe { (object as *const WeaponBase).as_ref() }
        .and_then(|weapon| unsafe { slot_of(weapon) })
        .is_some_and(|slot| socket.is_native(slot));
    let offset = if native {
        Vec3::ZERO
    } else {
        to_world.transform_vector3(offsets.of(socket))
    };
    let point = bone.w_axis.truncate() + offset;
    point.is_finite().then_some(point)
}

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
