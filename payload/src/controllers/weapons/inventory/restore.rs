//! Carrying the weapons beyond the slots across the spawn system's resets and the game's saves and
//! loads. The format is [`crate::controllers::weapons::inventory::persist`]; the engine side is in
//! `docs/engine/gameplay/weapon-ownership-and-despawn.md`.
//!
//! A reset (a load, and possibly a death or a fast travel) releases every spawned object and
//! replaces the slots' weapons by type. The carried weapons are the mod's to replace:
//!
//! - at the reset's pre-reset event, the carried weapons are recorded (type, magazine, socket)
//!   before the carry set lets them go ([`on_pre_reset`]);
//! - each store of the player's save data writes the save sidecar, the carried weapons with the
//!   fingerprint of what the store writes ([`on_store`]), on a writer thread;
//! - each load of the player's save data reads the sidecar and keeps its weapons when its
//!   fingerprint matches what the load put into the inventory, and nothing extra otherwise
//!   ([`on_load`]);
//! - after the reset's post-reset event, the first input tick with the player spawns a weapon of
//!   each recorded type ([`SpawnSystem::SpawnTagIndex`]), from the load when there was one since
//!   the reset and from the reset's own record otherwise, and adopts each into the carry set on its
//!   socket with its magazine as it was when its spawn comes back ([`input_tick`]).
//!
//! The store, the load, and the reset run on whatever thread the game runs them on, and the spawn
//! callback on the spawn system's, so they take only this module's locks and the carry set's, one
//! at a time, and make no engine call but the reads the game's own store and load make.

use std::{
    ffi::c_void,
    fmt, fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
    game::GameObject,
    save::PlayerSaveData,
    spawn_system::SpawnSystem,
    types::{math::Matrix4, shared_ptr::SharedPtr, std_vector::Vector},
};
use parking_lot::Mutex;

use crate::{
    config::{self, paths},
    controllers::{
        hands::ik,
        loadout::{Restored, live},
        weapons::{
            dual_wield::single,
            hold::refs::Strong,
            inventory::{
                carry,
                config::InventoryConfig,
                engine::{self as inventory_engine, kind_of, weapon_of},
                persist::{Fingerprint, RESERVES, Record, SLOTS, Sidecar},
            },
        },
    },
};

/// The sidecar's file name, beside the settings file.
pub(crate) const SIDECAR_FILE_NAME: &str = "inventory.toml";

/// What the last load made of the sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// It matched the save: this many weapons will come back, and this many of types the game
    /// does not know now will not.
    Accepted { carried: usize, unknown: usize },
    /// There is no sidecar.
    Missing,
    /// It could not be read: why.
    Unreadable(String),
    /// It belongs to another save: how the save differs.
    Rejected(String),
    /// The load had no character to read.
    NoCharacter,
}

impl fmt::Display for LoadOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadOutcome::Accepted { carried, unknown } => {
                write!(
                    f,
                    "accepted, {carried} to restore, {unknown} of unknown types"
                )
            }
            LoadOutcome::Missing => write!(f, "no sidecar"),
            LoadOutcome::Unreadable(why) => write!(f, "unreadable: {why}"),
            LoadOutcome::Rejected(why) => write!(f, "another save's: {why}"),
            LoadOutcome::NoCharacter => write!(f, "no character to read"),
        }
    }
}

/// Where a respawn's record came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The sidecar a load accepted, or a load's empty set.
    Load,
    /// The carried weapons at the reset.
    Reset,
}

/// What the last respawn of the carried weapons did.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct RespawnOutcome {
    pub origin: Option<Origin>,
    /// Spawns asked for, weapons adopted onto sockets, and spawns that never came or found no
    /// place.
    pub requested: usize,
    pub restored: usize,
    pub failed: usize,
}

/// The persistence's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RestoreStatus {
    /// The sidecar's path, when there is a configuration directory.
    pub path: Option<PathBuf>,
    /// Sidecar writes since the payload loaded, and the last one's error.
    pub writes: u64,
    pub last_write_error: Option<String>,
    /// The carried weapons the last write recorded.
    pub last_written: usize,
    pub last_load: Option<LoadOutcome>,
    pub last_respawn: Option<RespawnOutcome>,
    /// Carried weapons respawned onto their sockets since the payload loaded.
    pub restored: u64,
    /// Spawns still on their way.
    pub spawning: usize,
}

pub fn status() -> RestoreStatus {
    let mut status = STATUS.lock().clone();
    status.path = paths::file(SIDECAR_FILE_NAME);
    status.spawning = PLAN.lock().spawning.len();
    status
}

/// The spawn system is resetting: record the carried weapons, which the reset's release of the
/// carry set lets go, for the respawn, and whether the pistol slot held one pistol. Any spawn still
/// on its way belongs to the world before the reset and is forgotten.
///
/// A respawn the mission manager replays a checkpoint for records the carried weapons as they were
/// at that checkpoint instead ([`checkpoint_restoring`]). While a mission or a challenge holds the
/// player's weapons (`stashed`), the record is held back until the give-back ([`give_back`]),
/// rather than respawned onto the body during the hold.
pub(crate) fn on_pre_reset(records: Vec<Record>, slot_pistol_single: bool, stashed: bool) {
    let mut plan = PLAN.lock();
    plan.generation = plan.generation.wrapping_add(1);
    plan.spawning.clear();
    let (records, slot_pistol_single) = match plan.checkpoint_next.take() {
        Some(checkpoint) => checkpoint,
        None => (records, slot_pistol_single),
    };
    let pending = Pending {
        records,
        slot_pistol_single: Some(slot_pistol_single),
        at: Instant::now(),
        character: ik::local_character(),
    };
    if stashed {
        // A second reset during the hold keeps the first record: the weapons are not carried then.
        if plan.held.is_none() {
            plan.held = Some(pending);
        }
        plan.from_reset = None;
    } else {
        plan.from_reset = Some(pending);
    }
    plan.due = false;
    std::mem::drop(plan);
    SPAWNED.lock().clear();
    TWINS.lock().clear();
}

/// The spawn system has reset: the next input tick with the player respawns the carried weapons.
pub(crate) fn on_post_reset() {
    PLAN.lock().due = true;
}

/// The mission manager snapshots the slots at a checkpoint: keep the carried weapons as they are
/// now (`records`), and whether the pistol slot holds one pistol, for a restart from it.
pub(crate) fn checkpoint_cached(records: Vec<Record>, slot_pistol_single: bool) {
    PLAN.lock().checkpoint = Some((records, slot_pistol_single));
}

/// The mission manager replays its checkpoint for the player's respawn: the reset that follows
/// respawns the checkpoint's carried weapons rather than the ones carried now.
pub(crate) fn checkpoint_restoring() {
    let mut plan = PLAN.lock();
    plan.checkpoint_next = plan.checkpoint.clone();
}

/// A mission's or a challenge's give-back: a reset's record held back during its hold is due now.
/// Returns whether there was one.
pub(crate) fn give_back() -> bool {
    let mut plan = PLAN.lock();
    let Some(mut held) = plan.held.take() else {
        return false;
    };
    held.at = Instant::now();
    plan.from_reset = Some(held);
    plan.due = true;
    true
}

/// The frontend reset the game's systems: nothing recorded carries into what follows but a load's
/// accepted sidecar.
pub(crate) fn on_frontend() {
    let mut plan = PLAN.lock();
    plan.from_reset = None;
    plan.held = None;
    plan.checkpoint = None;
    plan.checkpoint_next = None;
}

/// The game is storing the player's save data: write the sidecar with the carried weapons and the
/// fingerprint of what the store writes, on a writer thread.
///
/// # Safety
///
/// `save_data` must be the save data the store is given, live for the call.
pub(crate) unsafe fn on_store(save_data: *const PlayerSaveData, settings: &InventoryConfig) {
    if !(settings.enabled && settings.carry && settings.persist) {
        return;
    }
    let Some(character) = (unsafe { character_of(save_data) }) else {
        return;
    };
    let fingerprint = unsafe { stored_fingerprint(character) };
    let carried = carry::records();
    let Some(path) = paths::file(SIDECAR_FILE_NAME) else {
        return;
    };
    let count = carried.len();
    let slot_pistol_single = single::active().then(single::slot_single);
    let text = Sidecar {
        fingerprint,
        carried,
        slot_pistol_single,
    }
    .encode();
    let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let spawned = std::thread::Builder::new()
        .name("jc3vr-inventory-sidecar".to_owned())
        .spawn(move || write(path, text, sequence, count));
    if let Err(error) = spawned {
        tracing::warn!(target: "controllers", %error, "inventory: the sidecar writer did not start");
    }
}

/// The game loaded the player's save data: read the sidecar, and keep its carried weapons for the
/// respawn when its fingerprint matches what the load put into the inventory. Without a match, a
/// load carries nothing extra, whatever the reset recorded.
///
/// # Safety
///
/// `save_data` must be the save data the load was given, live for the call.
pub(crate) unsafe fn on_load(save_data: *const PlayerSaveData, settings: &InventoryConfig) {
    if !(settings.enabled && settings.carry && settings.persist) {
        return;
    }
    let (outcome, records, slot_pistol_single) = unsafe { read_for_load(save_data) };
    match &outcome {
        LoadOutcome::Accepted { carried, unknown } => tracing::info!(
            target: "controllers",
            carried,
            unknown,
            "inventory: the sidecar matches the save; its carried weapons come back"
        ),
        other => tracing::info!(
            target: "controllers",
            outcome = %other,
            "inventory: the load carries nothing extra"
        ),
    }
    PLAN.lock().from_load = Some(Pending {
        records,
        slot_pistol_single,
        at: Instant::now(),
        character: 0,
    });
    STATUS.lock().last_load = Some(outcome);
}

/// After a reset, respawn the carried weapons; and adopt each spawned one that has come back onto
/// its socket. Called every input tick.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &mut Character, settings: &InventoryConfig) {
    let enabled = settings.enabled && settings.carry;
    let plan = {
        let mut plan = PLAN.lock();
        if !plan.due {
            None
        } else {
            plan.due = false;
            let from_load = plan.from_load.take();
            let from_reset = plan.from_reset.take();
            Some(choose(
                from_load,
                from_reset,
                owner as *const Character as usize,
            ))
        }
    };
    if let Some(chosen) = plan.filter(|_| enabled) {
        single::cover_respawn(chosen.slot_pistol_single);
        unsafe { request(owner, chosen.origin, chosen.records) };
    }
    unsafe { adopt(owner) };
}

/// Ask the spawn system for the second pistol of a pair of the type `tag_hash`, at the player, for
/// the single pistols ([`crate::controllers::weapons::dual_wield::single`]), which take it from
/// [`take_twins`]. Returns whether it was asked for.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn request_twin(owner: &Character, tag_hash: u32) -> bool {
    let Some(system) = (unsafe { SpawnSystem::get() }) else {
        return false;
    };
    let index = unsafe { system.GetTagIndexFromTagHash(tag_hash) };
    if index <= 0 {
        return false;
    }
    let generation = PLAN.lock().generation;
    let transform = owner.m_WorldMatrixT1;
    let transform: *const Matrix4 = &transform;
    unsafe {
        system.SpawnTagIndex(
            index,
            transform,
            SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS,
            spawned as *const u8,
            (generation | TWIN_REQUEST) as usize as *mut u8,
            -1,
        )
    };
    true
}

/// The second pistols the spawn system has handed over since the last call, asked for in the
/// world as it is now.
pub(crate) fn take_twins() -> Vec<Strong> {
    let generation = PLAN.lock().generation;
    std::mem::take(&mut *TWINS.lock())
        .into_iter()
        .filter(|(asked_in, _)| *asked_in == generation)
        .map(|(_, strong)| strong)
        .collect()
}

/// Forget everything waiting: the inventory is off, or gameplay ended.
pub(crate) fn clear() {
    let mut plan = PLAN.lock();
    plan.generation = plan.generation.wrapping_add(1);
    plan.spawning.clear();
    plan.from_load = None;
    plan.from_reset = None;
    plan.held = None;
    plan.checkpoint = None;
    plan.checkpoint_next = None;
    plan.due = false;
    std::mem::drop(plan);
    SPAWNED.lock().clear();
    TWINS.lock().clear();
}

/// A set of carried weapons waiting for the respawn.
struct Pending {
    records: Vec<Record>,
    /// Whether the pistol slot held one pistol, when known.
    slot_pistol_single: Option<bool>,
    at: Instant,
    /// The local player at the reset, for a reset's record.
    character: usize,
}

/// A weapon asked of the spawn system.
struct Spawning {
    record: Record,
    at: Instant,
}

struct Plan {
    from_load: Option<Pending>,
    from_reset: Option<Pending>,
    /// A reset's record held back while a mission or a challenge holds the player's weapons.
    held: Option<Pending>,
    /// The carried weapons at the last mission checkpoint, and whether the pistol slot held one
    /// pistol, and the same taken for the reset of a restart from it.
    checkpoint: Option<(Vec<Record>, bool)>,
    checkpoint_next: Option<(Vec<Record>, bool)>,
    /// Whether a reset has finished and the respawn is waiting for the player.
    due: bool,
    spawning: Vec<Spawning>,
    /// Counts the resets, so a spawn of an earlier world is told apart.
    generation: u64,
    origin: Option<Origin>,
}

static PLAN: Mutex<Plan> = Mutex::new(Plan {
    from_load: None,
    from_reset: None,
    held: None,
    checkpoint: None,
    checkpoint_next: None,
    due: false,
    spawning: Vec::new(),
    generation: 0,
    origin: None,
});
/// The weapons the spawn callback handed over, with the generation they were asked in.
static SPAWNED: Mutex<Vec<(u64, Strong)>> = Mutex::new(Vec::new());
/// The second pistols the spawn callback handed over, with the generation they were asked in.
static TWINS: Mutex<Vec<(u64, Strong)>> = Mutex::new(Vec::new());
/// The bit of a spawn request's user data that marks it as a second pistol's, above the
/// generation.
const TWIN_REQUEST: u64 = 1 << 62;
static STATUS: Mutex<RestoreStatus> = Mutex::new(RestoreStatus {
    path: None,
    writes: 0,
    last_write_error: None,
    last_written: 0,
    last_load: None,
    last_respawn: None,
    restored: 0,
    spawning: 0,
});
/// The writes asked for, and the newest one done, so an older write never replaces a newer one.
static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WRITTEN_SEQUENCE: Mutex<u64> = Mutex::new(0);

/// How long a reset's record waits for the player: a load screen's length. A record older than
/// this is taken for a quit to the menu, and a new game does not get it.
const RESET_RECORD_FOR: Duration = Duration::from_secs(120);
/// How long a spawn is waited for.
const SPAWN_WAIT: Duration = Duration::from_secs(10);

/// The player's character, from the save data.
unsafe fn character_of(save_data: *const PlayerSaveData) -> Option<&'static Character> {
    let save_data = unsafe { save_data.as_ref() }?;
    let player = unsafe { save_data.m_Player.as_ref() }?;
    unsafe { player.m_Character.as_ref() }
}

/// The fingerprint of what the store writes: each slot's weapon type as its tag hash (zero for the
/// heavy slot and an empty slot), and every reserve.
unsafe fn stored_fingerprint(character: &Character) -> Fingerprint {
    let mut slots = [0; SLOTS];
    for (index, slot) in slots.iter_mut().enumerate() {
        if index == WeaponSlot::E_SLOT_HEAVY as usize {
            continue;
        }
        let Some(weapon) = (unsafe { character.m_Inventory.m_Weapons[index].px.as_ref() }) else {
            continue;
        };
        *slot = unsafe { kind_of(weapon) }.map_or(0, |k| k.tag_hash);
    }
    let mut reserves = [0; RESERVES];
    reserves.copy_from_slice(&character.m_Inventory.m_Ammunition);
    Fingerprint { slots, reserves }
}

/// The fingerprint of what a load put into `character`'s inventory: the respawn table's types and
/// the reserves.
unsafe fn loaded_fingerprint(character: &Character, system: &SpawnSystem) -> Fingerprint {
    let mut slots = [0; SLOTS];
    for (slot, &index) in slots.iter_mut().zip(&character.m_Inventory.m_WeaponToSpawn) {
        if index > 0 {
            *slot = unsafe { system.GetTagHashFromTagIndex(index) };
        }
    }
    let mut reserves = [0; RESERVES];
    reserves.copy_from_slice(&character.m_Inventory.m_Ammunition);
    Fingerprint { slots, reserves }
}

/// Read the sidecar for a load: what it restores, and whether the pistol slot held one pistol.
unsafe fn read_for_load(
    save_data: *const PlayerSaveData,
) -> (LoadOutcome, Vec<Record>, Option<bool>) {
    let (Some(character), Some(system)) = (unsafe { character_of(save_data) }, unsafe {
        SpawnSystem::get()
    }) else {
        return (LoadOutcome::NoCharacter, Vec::new(), None);
    };
    let Some(path) = paths::file(SIDECAR_FILE_NAME) else {
        return (LoadOutcome::Missing, Vec::new(), None);
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (LoadOutcome::Missing, Vec::new(), None);
        }
        Err(error) => {
            let why = format!("could not read {}: {error}", path.display());
            tracing::warn!(target: "controllers", %why, "inventory: the sidecar is unreadable");
            return (LoadOutcome::Unreadable(why), Vec::new(), None);
        }
    };
    let sidecar = match Sidecar::decode(&text) {
        Ok(sidecar) => sidecar,
        Err(error) => {
            let why = format!("{}: {error}", path.display());
            tracing::warn!(target: "controllers", %why, "inventory: the sidecar is unreadable");
            return (LoadOutcome::Unreadable(why), Vec::new(), None);
        }
    };
    let known = |hash: u32| unsafe { system.GetTagIndexFromTagHash(hash) } > 0;
    let loaded = unsafe { loaded_fingerprint(character, system) };
    if let Err(mismatch) = sidecar.fingerprint.matches(&loaded, known) {
        return (
            LoadOutcome::Rejected(mismatch.to_string()),
            Vec::new(),
            None,
        );
    }
    let (records, unknown) = sidecar.restorable(known);
    (
        LoadOutcome::Accepted {
            carried: records.len(),
            unknown,
        },
        records,
        sidecar.slot_pistol_single,
    )
}

/// Write the sidecar's `text` to `path`, unless a newer write is done already.
fn write(path: PathBuf, text: String, sequence: u64, count: usize) {
    let mut written = WRITTEN_SEQUENCE.lock();
    if *written > sequence {
        return;
    }
    let result = config::write::replace(&path, text.as_bytes());
    *written = sequence;
    std::mem::drop(written);
    let mut status = STATUS.lock();
    status.writes += 1;
    status.last_written = count;
    match result {
        Ok(()) => {
            status.last_write_error = None;
            std::mem::drop(status);
            tracing::info!(
                target: "controllers",
                carried = count,
                path = %path.display(),
                "inventory: wrote the save sidecar"
            );
        }
        Err(error) => {
            let why = format!("inventory: sidecar: {error}");
            status.last_write_error = Some(why.clone());
            std::mem::drop(status);
            tracing::warn!(target: "controllers", error = %why, "inventory: the save sidecar was not written");
        }
    }
}

/// The set a respawn restores.
struct Chosen {
    origin: Origin,
    records: Vec<Record>,
    /// Whether the pistol slot held one pistol, when the set says.
    slot_pistol_single: Option<bool>,
}

/// The set to respawn: a load's, when there was one since the reset; otherwise the reset's own,
/// when it is recent and for the same character.
fn choose(from_load: Option<Pending>, from_reset: Option<Pending>, character: usize) -> Chosen {
    if let Some(load) = from_load {
        return Chosen {
            origin: Origin::Load,
            records: load.records,
            slot_pistol_single: load.slot_pistol_single,
        };
    }
    let nothing = Chosen {
        origin: Origin::Reset,
        records: Vec::new(),
        slot_pistol_single: None,
    };
    match from_reset {
        Some(reset) if reset.at.elapsed() <= RESET_RECORD_FOR && reset.character == character => {
            Chosen {
                origin: Origin::Reset,
                records: reset.records,
                slot_pistol_single: reset.slot_pistol_single,
            }
        }
        Some(reset) => {
            if !reset.records.is_empty() {
                tracing::info!(
                    target: "controllers",
                    carried = reset.records.len(),
                    "inventory: the carried weapons at the reset are not restored: the reset was too long ago or for another character"
                );
            }
            nothing
        }
        None => nothing,
    }
}

/// Ask the spawn system for a weapon of each record's type, at the player.
unsafe fn request(owner: &Character, origin: Origin, records: Vec<Record>) {
    let mut outcome = RespawnOutcome {
        origin: Some(origin),
        ..RespawnOutcome::default()
    };
    let Some(system) = (unsafe { SpawnSystem::get() }) else {
        STATUS.lock().last_respawn = Some(outcome);
        return;
    };
    let transform = owner.m_WorldMatrixT1;
    let mut plan = PLAN.lock();
    plan.origin = Some(origin);
    let generation = plan.generation;
    for record in records {
        let index = unsafe { system.GetTagIndexFromTagHash(record.tag_hash) };
        if index <= 0 {
            outcome.failed += 1;
            continue;
        }
        let transform: *const Matrix4 = &transform;
        unsafe {
            system.SpawnTagIndex(
                index,
                transform,
                SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS,
                spawned as *const u8,
                generation as usize as *mut u8,
                -1,
            )
        };
        plan.spawning.push(Spawning {
            record,
            at: Instant::now(),
        });
        outcome.requested += 1;
    }
    std::mem::drop(plan);
    tracing::info!(
        target: "controllers",
        origin = ?origin,
        requested = outcome.requested,
        failed = outcome.failed,
        "inventory: respawning the carried weapons"
    );
    STATUS.lock().last_respawn = Some(outcome);
}

/// Adopt each weapon the spawn system has handed over onto its socket, and give up on spawns that
/// never came.
unsafe fn adopt(owner: &mut Character) {
    let generation = PLAN.lock().generation;
    let spawned = std::mem::take(&mut *SPAWNED.lock());
    for (asked_in, strong) in spawned {
        if asked_in != generation {
            continue;
        }
        // SAFETY: the strong reference keeps the weapon alive.
        let weapon = unsafe { &mut *weapon_of(&strong) };
        let Some(kind) = (unsafe { kind_of(weapon) }) else {
            continue;
        };
        let record = {
            let mut plan = PLAN.lock();
            let index = plan
                .spawning
                .iter()
                .position(|s| s.record.tag_hash == kind.tag_hash);
            index.map(|i| plan.spawning.remove(i).record)
        };
        let Some(record) = record else {
            continue;
        };
        // The recorded socket when it is free, otherwise where the table puts a weapon of its type.
        let socket = unsafe { inventory_engine::weapon_type(weapon) }
            .and_then(|weapon| live::free_socket(weapon, Some(record.socket)));
        let restored = socket.is_some_and(|socket| {
            match unsafe { carry::carry_loose(owner, strong, socket) } {
                Ok(object) => {
                    // SAFETY: the carry set keeps the weapon alive.
                    let weapon = unsafe { &mut *(object as *mut WeaponBase) };
                    if record.single {
                        unsafe { single::designate(weapon, "a carried pistol restored") };
                    }
                    if record.magazine >= 0 {
                        unsafe { weapon.SetMagazineAmmo(record.magazine) };
                    }
                    if let (Some(object), Some(weapon)) = (
                        inventory_engine::object_address(weapon),
                        unsafe { inventory_engine::weapon_type(weapon) },
                    ) {
                        live::restored(Restored {
                            object,
                            weapon,
                            socket,
                        });
                    }
                    tracing::info!(
                        target: "controllers",
                        socket = socket.label(),
                        magazine = record.magazine,
                        "inventory: restored a carried weapon on the {}",
                        socket.label()
                    );
                    true
                }
                Err(refusal) => {
                    tracing::info!(target: "controllers", %refusal, "inventory: a respawned weapon was not carried");
                    false
                }
            }
        });
        if !restored && socket.is_none() {
            tracing::info!(target: "controllers", "inventory: no socket is free for a respawned weapon; it lies at the player's feet");
        }
        let mut status = STATUS.lock();
        if restored {
            status.restored += 1;
        }
        if let Some(outcome) = &mut status.last_respawn {
            if restored {
                outcome.restored += 1;
            } else {
                outcome.failed += 1;
            }
        }
    }
    let expired: Vec<Record> = {
        let mut plan = PLAN.lock();
        let (waiting, expired) = std::mem::take(&mut plan.spawning)
            .into_iter()
            .partition(|s| s.at.elapsed() <= SPAWN_WAIT);
        plan.spawning = waiting;
        expired.into_iter().map(|s| s.record).collect()
    };
    if !expired.is_empty() {
        tracing::info!(target: "controllers", expired = expired.len(), "inventory: respawns of carried weapons never came");
        if let Some(outcome) = &mut STATUS.lock().last_respawn {
            outcome.failed += expired.len();
        }
    }
    // A single pistol that never came back leaves the slot's pistol the pair it was split from.
    for record in expired.iter().filter(|r| r.single) {
        unsafe { single::pistol_not_restored(owner, record.tag_hash) };
    }
}

/// The spawn system's callback for a respawn: take a strong reference to each spawned weapon and
/// hand it to the next input tick. Runs on the spawn system's thread.
unsafe extern "system" fn spawned(
    objects: *const Vector<SharedPtr<GameObject>>,
    userdata: *mut c_void,
) {
    let Some(objects) = (unsafe { objects.as_ref() }) else {
        return;
    };
    let userdata = userdata as usize as u64;
    let generation = userdata & !TWIN_REQUEST;
    let mut out = if userdata & TWIN_REQUEST != 0 {
        TWINS.lock()
    } else {
        SPAWNED.lock()
    };
    for object in unsafe { objects.as_slice() } {
        if object.px.is_null() {
            continue;
        }
        // The spawned object's own weak reference to itself, through which the inventory holds it.
        let weapon = (object.px as usize).wrapping_sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
            as *const WeaponBase;
        // SAFETY: the vector's reference keeps the object alive for the call.
        if !unsafe { (*object.px).IsType(WeaponBase::TYPE_ID()) } {
            continue;
        }
        if let Some(strong) = unsafe { inventory_engine::strong_to(&*weapon) } {
            out.push((generation, strong));
        }
    }
}
