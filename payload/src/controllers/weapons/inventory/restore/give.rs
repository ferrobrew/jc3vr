//! The debug UI's give: a weapon of any kind the game ships put straight onto a socket, a socket
//! cleared, the grenade stock set, and the placed C4 charges removed. The design is in
//! `docs/mod/input/two-handed-inventory.md`, "Giving weapons from the debug UI".
//!
//! A give takes the restore's path for a carried weapon: the spawn system spawns the weapon at the
//! player ([`SpawnSystem::Spawn`], with the shipped definition's full tag set, as the director's
//! spawner asks for one), and when its callback hands the weapon over, the next input tick carries
//! it onto the socket through the table's restore admission ([`adoption::carry_onto`]), which
//! falls back to the next free socket that fits it. A socket that holds a weapon is cleared first,
//! its weapon falling clear of the player as a swap's does ([`swap::eject`]), so the give writes
//! straight into it. The requests are queued from the UI and carried out on the game thread at
//! the restore's input tick ([`input_tick`]).

use std::{
    ffi::{CString, c_void},
    time::{Duration, Instant},
};

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
    game::GameObject,
    spawn_system::SpawnSystem,
    types::{math::Matrix4, shared_ptr::SharedPtr, std_vector::Vector},
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        loadout::{Restored, Socket},
        weapons::{
            c4,
            dual_wield::single,
            grenade::stock,
            hold::{refs::Strong, swap},
            inventory::{
                engine as inventory_engine,
                restore::{PLAN, adoption},
                spawn_safety::{self, RequestKind},
            },
        },
    },
    director::spawn::{self, SpawnDefinition},
};

/// What the debug UI asks of the inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GiveRequest {
    /// Spawn a weapon of the shipped definition named `name` onto `socket`.
    Weapon { socket: Socket, name: String },
    /// Let the weapon on `socket` fall clear of the player.
    Clear(Socket),
    /// Set the grenade stock to this many, within its capacity.
    Grenades(i32),
    /// Remove the player's placed C4 charges without detonating them.
    RemovePlacedC4,
}

/// Queue `request` for the next input tick. A request beyond the queue's bound is dropped.
pub fn request(request: GiveRequest) {
    let mut queue = QUEUE.lock();
    if queue.len() < MAX_QUEUED {
        queue.push(request);
    }
}

/// The shipped weapon definitions `socket` can be given, in table order: the one-handed weapons
/// for a hip or the chest, the rest of the hand weapons for the back.
pub fn weapons_for(socket: Socket) -> Vec<&'static SpawnDefinition> {
    let small = matches!(socket, Socket::Hip(_) | Socket::Chest);
    spawn::definitions()
        .iter()
        .filter(|def| giveable(def) == Some(small))
        .collect()
}

/// What the last give did, for the readout.
pub fn last_outcome() -> Option<String> {
    LAST.lock().clone()
}

/// Carry out the queued requests, and carry each given weapon that came onto its socket.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(super) unsafe fn input_tick(owner: &mut Character, ready: bool) {
    if !ready {
        return;
    }
    let queued = std::mem::take(&mut *QUEUE.lock());
    for request in queued {
        let outcome = match request {
            GiveRequest::Weapon { socket, name } => unsafe { give(owner, socket, &name) },
            GiveRequest::Clear(socket) => unsafe { clear(socket) },
            GiveRequest::Grenades(count) => match unsafe { stock::set(count) } {
                Some(count) => format!("give: the grenade stock is {count}"),
                None => "give: the player has no grenade weapon".to_owned(),
            },
            GiveRequest::RemovePlacedC4 => match unsafe { c4::remove_placed(owner) } {
                Some(1) => "give: removed the one placed C4 charge".to_owned(),
                Some(removed) => format!("give: removed {removed} placed C4 charges"),
                None => "give: the player has no C4 weapon".to_owned(),
            },
        };
        tracing::info!(target: "controllers", "{outcome}");
        *LAST.lock() = Some(outcome);
    }
    unsafe { adopt(owner) };
}

/// Forget the gives on their way. Called when the restore forgets its own.
pub(super) fn clear_pending() {
    PENDING.lock().clear();
    GIVEN.lock().clear();
}

/// The most requests queued at once.
const MAX_QUEUED: usize = 16;
/// How long a give's spawn is waited for.
const GIVE_WAIT: Duration = Duration::from_secs(10);
/// The shift of a give's socket index in its spawn request's user data, above the generation.
const SOCKET_SHIFT: u32 = 56;

/// A give whose weapon is on its way.
struct Pending {
    token: u64,
    socket: Socket,
    name: String,
    at: Instant,
}

static QUEUE: Mutex<Vec<GiveRequest>> = Mutex::new(Vec::new());
static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
/// The weapons the spawn callback handed over, with their request's user data.
static GIVEN: Mutex<Vec<(u64, Strong)>> = Mutex::new(Vec::new());
static LAST: Mutex<Option<String>> = Mutex::new(None);

/// Whether `def` is a hand weapon a socket can be given, and if so whether it is one-handed
/// (`Some(true)`, for a hip or the chest) or not (`Some(false)`, for the back).
fn giveable(def: &SpawnDefinition) -> Option<bool> {
    let tags: Vec<&str> = def.tags.split(',').map(str::trim).collect();
    let excluded = [
        "do_not_spawn",
        "mounted",
        "turret",
        "fow",
        "thrown",
        "planted",
    ];
    if !def.category.starts_with("Weapon")
        || def.category.contains("Stationary")
        || tags.iter().any(|tag| excluded.contains(tag))
    {
        return None;
    }
    let one_handed = tags.contains(&"one_handed")
        || ["_pistol_", "_handgun_", "_smg_"]
            .iter()
            .any(|kind| def.name.contains(kind));
    Some(one_handed)
}

/// Spawn a weapon of the definition `name` for `socket`, clearing the socket first.
unsafe fn give(owner: &Character, socket: Socket, name: &str) -> String {
    let Some(def) = spawn::definitions().iter().find(|def| def.name == name) else {
        return format!("give: no shipped definition is named {name}");
    };
    let Ok(tags) = CString::new(def.tags.as_str()) else {
        return format!("give: the tags of {name} contain a NUL");
    };
    let Some(system) = (unsafe { SpawnSystem::get() }) else {
        return "give: there is no spawn system".to_owned();
    };
    // A refusal here only means the socket was empty.
    let _ = unsafe { swap::eject(socket) };
    let generation = PLAN.lock().generation;
    let token = generation | ((socket.index() as u64) << SOCKET_SHIFT);
    let transform = owner.m_WorldMatrixT1;
    let transform: *const Matrix4 = &transform;
    let submitted = spawn_safety::submit(RequestKind::Give, || unsafe {
        system.Spawn(
            tags.as_ptr().cast(),
            transform,
            SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS,
            given as *const u8,
            token as usize as *mut u8,
        )
    });
    if let Err(refusal) = submitted {
        return format!("give: {refusal}");
    }
    PENDING.lock().push(Pending {
        token,
        socket,
        name: name.to_owned(),
        at: Instant::now(),
    });
    format!(
        "give: asked the spawn system for {name} for the {}",
        socket.label()
    )
}

/// Let the weapon on `socket` fall clear of the player.
unsafe fn clear(socket: Socket) -> String {
    match unsafe { swap::eject(socket) } {
        Ok(()) => format!("give: cleared the {}", socket.label()),
        Err(refusal) => format!("give: the {} was not cleared: {refusal}", socket.label()),
    }
}

/// Carry each weapon the spawn callback handed over onto its socket, and forget the gives that
/// never came.
unsafe fn adopt(owner: &mut Character) {
    let given = std::mem::take(&mut *GIVEN.lock());
    for (token, strong) in given {
        let pending = {
            let mut pending = PENDING.lock();
            let index = pending.iter().position(|p| p.token == token);
            index.map(|i| pending.remove(i))
        };
        let Some(pending) = pending else {
            continue;
        };
        // SAFETY: the callback's strong reference keeps the weapon alive.
        let weapon = unsafe { &mut *inventory_engine::weapon_of(&strong) };
        let Some(restored) = inventory_engine::object_address(weapon)
            .zip(unsafe { inventory_engine::weapon_type(weapon) })
            .map(|(object, weapon)| Restored {
                object,
                weapon,
                socket: pending.socket,
            })
        else {
            continue;
        };
        let pistol = restored.weapon.slot() == WeaponSlot::E_SLOT_DUAL_WIELD;
        let (outcome, carried) = adoption::carry_onto(owner, strong, restored);
        let line = match carried {
            Some(object) => {
                if pistol && single::active() {
                    // SAFETY: the carry set keeps the weapon alive.
                    unsafe {
                        single::designate(&mut *(object as *mut WeaponBase), "a given pistol")
                    };
                }
                format!(
                    "give: {} is on the {}",
                    pending.name,
                    pending.socket.label()
                )
            }
            None => format!("give: {} was not carried: {outcome:?}", pending.name),
        };
        tracing::info!(target: "controllers", "{line}");
        *LAST.lock() = Some(line);
    }
    let expired: Vec<String> = {
        let mut pending = PENDING.lock();
        let (waiting, expired) = std::mem::take(&mut *pending)
            .into_iter()
            .partition(|p| p.at.elapsed() <= GIVE_WAIT);
        *pending = waiting;
        expired.into_iter().map(|p| p.name).collect()
    };
    for name in expired {
        let line = format!("give: {name} never came");
        tracing::info!(target: "controllers", "{line}");
        *LAST.lock() = Some(line);
    }
}

/// The spawn system's callback for a give: take a strong reference to each spawned weapon and hand
/// it to the next input tick. Runs inline on the caller of `SpawnSystem::PreUpdate`.
unsafe extern "system" fn given(
    objects: *const Vector<SharedPtr<GameObject>>,
    userdata: *mut c_void,
) {
    let _entry = spawn_safety::callback_entry();
    let Some(objects) = (unsafe { objects.as_ref() }) else {
        return;
    };
    let token = userdata as usize as u64;
    let mut out = GIVEN.lock();
    for object in unsafe { objects.as_slice() } {
        if object.px.is_null() || !unsafe { (*object.px).IsType(WeaponBase::TYPE_ID()) } {
            continue;
        }
        let weapon = (object.px as usize).wrapping_sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
            as *const WeaponBase;
        // SAFETY: the vector's reference keeps the object alive for the call.
        if let Some(strong) = unsafe { inventory_engine::strong_to(&*weapon) } {
            out.push((token, strong));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, category: &str, tags: &str) -> SpawnDefinition {
        SpawnDefinition {
            name: name.to_owned(),
            category: category.to_owned(),
            tags: tags.to_owned(),
            pool: "weapon".to_owned(),
        }
    }

    #[test]
    fn hand_weapons_are_sorted_by_whether_a_hip_takes_them() {
        let pistol = def("w011_pistol_a", "Weapon", "weapon,bullet,one_handed,pistol");
        let handgun = def("w014_handgun_b", "Weapon", "weapon,golden");
        let rifle = def("w111_rifle_c", "Weapon", "weapon,two_handed,bullet,rifle");
        let minigun = def("w201_minigun", "Weapon", "weapon,unmounted,heavy,minigun");
        assert_eq!(giveable(&pistol), Some(true));
        assert_eq!(giveable(&handgun), Some(true));
        assert_eq!(giveable(&rifle), Some(false));
        assert_eq!(giveable(&minigun), Some(false));
    }

    #[test]
    fn mounted_thrown_and_vehicle_weapons_are_not_given() {
        for d in [
            def("w201_mounted_minigun", "", "weapon,mounted,heavy"),
            def(
                "w301_frag_grenade",
                "Weapon",
                "weapon,thrown,explosive,grenade",
            ),
            def(
                "w401_explosives_c4",
                "",
                "weapon,thrown,planted_explosive,planted",
            ),
            def("w701_missile_strike", "Weapon", "weapon,fow,w701"),
            def("w501_turret_sam", "Weapon|Stationary", "weapon,turret,sam"),
            def("w601_helicopter_missile", "", "do_not_spawn"),
        ] {
            assert_eq!(giveable(&d), None, "{}", d.name);
        }
    }

    #[test]
    fn every_shipped_socket_list_is_nonempty_and_disjoint() {
        let small = weapons_for(Socket::Chest);
        let large = weapons_for(Socket::BackOuter);
        assert!(!small.is_empty() && !large.is_empty());
        assert!(
            small
                .iter()
                .all(|s| !large.iter().any(|l| l.name == s.name))
        );
    }
}
