//! The debug UI's give: a weapon of any kind the game ships put straight onto a socket or into a
//! hand, a socket cleared or a hand's weapon let go of, the grenade stock set, and the placed C4
//! charges removed. The design is in `docs/mod/input/two-handed-inventory.md`, "Giving weapons from
//! the debug UI".
//!
//! A give takes the restore's path for a carried weapon: the shipped definition's tag set names
//! its definition's unique tag ([`SpawnSystem::GetUniqueTag`]), the spawn system spawns that one
//! definition at the player ([`SpawnSystem::SpawnTagIndex`]), and when its callback hands the
//! weapon over, the next input tick carries it onto the socket through the table's restore
//! admission ([`adoption::carry_onto`]), which falls back to the next free socket that fits it. A
//! socket that holds a weapon is cleared first, its weapon falling clear of the player as a swap's
//! does ([`swap::eject`]), so the give writes straight into it. The requests are queued from the UI
//! and carried out on the game thread at the restore's input tick ([`input_tick`]).
//!
//! A give into a hand comes onto a socket that fits it ([`hand_socket`]) the same way, and on the
//! next tick the hand draws it off through the ordinary draw, as a grab at that socket would, but
//! only while the hand's grip is closed and the hand is empty. A hand whose grip is open holds
//! nothing, so the weapon then stays on the socket.
//!
//! Nothing in the spawn path locks a definition: the game's own completion reward gives its golden
//! pistol through the same unique tag. What can stop a give is the definition's exported entity:
//! one that is not loaded streams in first, and one whose load hits a file error is dropped with
//! the request, which then never calls back ([`SpawnResourceDefStatus::LOAD_ERROR`]). A give waits
//! while the entity is still streaming, withdraws its request when it gives up
//! ([`SpawnSystem::CancelSpawn`]) so that a late spawn does not leave a stray weapon at the player,
//! and says which of these it was. A definition that failed is remembered, and the list greys it
//! with the reason ([`refusal`]).

use std::{
    ffi::CString,
    time::{Duration, Instant},
};

use jc3gi::{
    character::character::Character,
    equipment::weapon::WeaponBase,
    spawn_system::{SpawnResourceDefStatus, SpawnSystem},
    types::math::Matrix4,
    ui::locator::Locale,
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        bindings,
        frame::Hand,
        loadout::{Socket, live},
        summary::localise,
        weapons::{
            c4,
            dual_wield::single,
            grenade::stock,
            hold::{self, act, refs::Strong, swap},
            inventory::{
                engine as inventory_engine,
                restore::{
                    PLAN, SpawnToken,
                    adoption::{self, Outcome},
                    spawned,
                },
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
    /// Spawn a weapon of the shipped definition named `name` for `hand`: onto a socket that fits
    /// it, then drawn into the hand if its grip is closed.
    Hand { hand: Hand, name: String },
    /// Let the weapon on `socket` fall clear of the player.
    Clear(Socket),
    /// Let go of the weapon `hand` holds.
    LetGo(Hand),
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

/// The shipped weapon definitions a hand can be given: every hand weapon, in table order.
pub fn weapons_for_hand() -> Vec<&'static SpawnDefinition> {
    spawn::definitions()
        .iter()
        .filter(|def| giveable(def).is_some())
        .collect()
}

/// The socket a give of a weapon for `hand` comes onto, by whether the weapon is one-handed
/// (`small`) and which sockets are `occupied`: the first empty one of the sockets that fit it, the
/// hand's own side first, else the first of them, which the give clears.
pub fn hand_socket(hand: Hand, small: bool, occupied: impl Fn(Socket) -> bool) -> Socket {
    let candidates = if small {
        [Socket::Hip(hand), Socket::Hip(hand.other()), Socket::Chest]
    } else {
        [
            Socket::Shoulder(hand),
            Socket::Shoulder(hand.other()),
            Socket::BackOuter,
        ]
    };
    candidates
        .into_iter()
        .find(|&socket| !occupied(socket))
        .unwrap_or(candidates[0])
}

/// The display name of the shipped definition `def`, as the game names a weapon of it, in the
/// game's language: looked up on the game thread ([`input_tick`]) once per language, by the
/// definition's name hash ([`SpawnDefinition::name_hash`]). Falls back to the definition's name
/// before the lookup, for a definition the localisation store has no name for, and in a language
/// the debug UI's fonts cannot draw.
pub fn display_name(def: &SpawnDefinition) -> String {
    NAMES
        .lock()
        .names
        .iter()
        .find(|(name, _)| *name == def.name)
        .and_then(|(_, display)| display.clone())
        .unwrap_or_else(|| def.name.clone())
}

/// The definition name of each give on its way to `target`.
pub fn pending(target: GiveTarget) -> Vec<&'static str> {
    let target_of = |p: &Pending| match p.hand {
        Some(hand) => GiveTarget::Hand(hand),
        None => GiveTarget::Socket(p.socket),
    };
    PENDING
        .lock()
        .iter()
        .filter(|p| target_of(p) == target)
        .map(|p| p.name)
        .collect()
}

/// Where a give goes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GiveTarget {
    Hand(Hand),
    Socket(Socket),
}

/// What the last give did, for the readout.
pub fn last_outcome() -> Option<String> {
    LAST.lock().clone()
}

/// Why the shipped definition named `name` cannot be given, when a give of it has shown that it
/// cannot: the spawn system has no definition for its tags, or its entity failed to load.
pub fn refusal(name: &str) -> Option<&'static str> {
    REFUSED
        .lock()
        .iter()
        .find(|(refused, _)| *refused == name)
        .map(|(_, why)| why.label())
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
    unsafe { refresh_names() };
    unsafe { draw_given() };
    let queued = std::mem::take(&mut *QUEUE.lock());
    for request in queued {
        let outcome = match request {
            GiveRequest::Weapon { socket, name } => unsafe { give(owner, socket, None, &name) },
            GiveRequest::Hand { hand, name } => match live::snapshot_now() {
                Some(table) => {
                    let small = spawn::definitions()
                        .iter()
                        .find(|def| def.name == name)
                        .and_then(giveable)
                        .unwrap_or(true);
                    let socket = hand_socket(hand, small, |socket| table.docked(socket).is_some());
                    unsafe { give(owner, socket, Some(hand), &name) }
                }
                None => "give: there is no loadout table".to_owned(),
            },
            GiveRequest::Clear(socket) => unsafe { clear(socket) },
            GiveRequest::LetGo(hand) => match unsafe { act::let_go(hand, 0.0) } {
                Ok(_) => format!("give: the {} hand let go of its weapon", hand.label()),
                Err(refusal) => format!(
                    "give: the {} hand let go of nothing: {refusal}",
                    hand.label()
                ),
            },
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
    DRAWS.lock().clear();
    GIVEN.lock().clear();
    EMPTY.lock().clear();
}

/// The most requests queued at once.
const MAX_QUEUED: usize = 16;
/// How long a give's spawn is waited for when its definition's entity is loaded.
const GIVE_WAIT: Duration = Duration::from_secs(10);
/// How long a give's spawn is waited for at most, while its definition's entity is still streaming
/// in.
const GIVE_WAIT_LOADING: Duration = Duration::from_secs(45);

/// A give whose weapon is on its way.
struct Pending {
    token: SpawnToken,
    socket: Socket,
    /// The hand the weapon is given to, which draws it off the socket once it is there.
    hand: Option<Hand>,
    name: &'static str,
    /// The definition's unique spawn tag.
    tag_index: i32,
    at: Instant,
    /// Whether the wait past [`GIVE_WAIT`] for a streaming entity has been logged.
    noted_loading: bool,
}

/// Why a shipped definition cannot be given.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Refusal {
    /// The spawn system has no definition matching its tags.
    NoDefinition,
    /// Its exported entity's load hit a file error.
    LoadError,
}

impl Refusal {
    const fn label(self) -> &'static str {
        match self {
            Refusal::NoDefinition => "the spawn system has no definition for its tags",
            Refusal::LoadError => "its entity failed to load",
        }
    }
}

static QUEUE: Mutex<Vec<GiveRequest>> = Mutex::new(Vec::new());
/// The weapons given to a hand that came onto their socket on the last tick, for the hand to draw
/// on the next: the hand, the socket, and the definition's name.
static DRAWS: Mutex<Vec<(Hand, Socket, &'static str)>> = Mutex::new(Vec::new());
static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
/// The weapons the spawn callback ([`spawned`]) handed over, with their request's token.
pub(super) static GIVEN: Mutex<Vec<(SpawnToken, Strong)>> = Mutex::new(Vec::new());
/// The gives the spawn callback answered with no weapon, with how many objects it handed over.
pub(super) static EMPTY: Mutex<Vec<(SpawnToken, usize)>> = Mutex::new(Vec::new());
static LAST: Mutex<Option<String>> = Mutex::new(None);
/// The giveable definitions' display names, by definition name, for the language they were looked
/// up in ([`display_name`]).
static NAMES: Mutex<DefinitionNames> = Mutex::new(DefinitionNames {
    locale: None,
    names: Vec::new(),
});

/// The giveable definitions' display names in one language.
struct DefinitionNames {
    locale: Option<Locale>,
    names: Vec<(&'static str, Option<String>)>,
}
/// The shipped definitions a give has shown cannot be given, by name.
static REFUSED: Mutex<Vec<(&'static str, Refusal)>> = Mutex::new(Vec::new());

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

/// Spawn a weapon of the definition `name` for `socket`, clearing the socket once the spawn is asked
/// for, and for `hand` to draw once it is there.
unsafe fn give(owner: &Character, socket: Socket, hand: Option<Hand>, name: &str) -> String {
    let Some(def) = spawn::definitions().iter().find(|def| def.name == name) else {
        return format!("give: no shipped definition is named {name}");
    };
    let name = def.name.as_str();
    let Ok(tags) = CString::new(def.tags.as_str()) else {
        return format!("give: the tags of {name} contain a NUL");
    };
    let Some(system) = (unsafe { SpawnSystem::get() }) else {
        return "give: there is no spawn system".to_owned();
    };
    let tag_index = unsafe { system.GetUniqueTag(tags.as_ptr().cast()) };
    if tag_index <= 0 {
        refuse(name, Refusal::NoDefinition);
        return format!(
            "give: {name} was not asked for: {}",
            Refusal::NoDefinition.label()
        );
    }
    if unsafe { definition_status(system, tag_index) } == Some(SpawnResourceDefStatus::LOAD_ERROR) {
        refuse(name, Refusal::LoadError);
        return format!(
            "give: {name} was not asked for: {}",
            Refusal::LoadError.label()
        );
    }
    let generation = PLAN.lock().generation;
    let token = SpawnToken::Give { generation, socket };
    let transform = owner.m_WorldMatrixT1;
    let transform: *const Matrix4 = &transform;
    let submitted = spawn_safety::submit(RequestKind::Give, || unsafe {
        system.SpawnTagIndex(
            tag_index,
            transform,
            SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS,
            spawned as *const u8,
            token.user_data(),
            u32::MAX,
        )
    });
    if let Err(refusal) = submitted {
        return format!("give: {refusal}");
    }
    // The socket is cleared only once the spawn is asked for, so a closed admission (a shutdown)
    // leaves the weapon there. The spawned weapon lands from a later tick ([`GIVEN`]), never inside
    // the request. A refusal here only means the socket was empty.
    let _ = unsafe { swap::eject(socket) };
    PENDING.lock().push(Pending {
        token,
        socket,
        hand,
        name,
        tag_index,
        at: Instant::now(),
        noted_loading: false,
    });
    match hand {
        Some(hand) => format!(
            "give: asked the spawn system for {name} for the {} hand, by the {}",
            hand.label(),
            socket.label()
        ),
        None => format!(
            "give: asked the spawn system for {name} for the {}",
            socket.label()
        ),
    }
}

/// Draw each weapon given to a hand off the socket it came onto, while the hand's grip is closed
/// and the hand is empty.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn draw_given() {
    let draws = std::mem::take(&mut *DRAWS.lock());
    for (hand, socket, name) in draws {
        let empty = live::snapshot_now().is_some_and(|table| table.held(hand).is_none());
        let line = if bindings::grip_holds_now(hand) != Some(true) {
            format!(
                "give: {name} stays on the {}: the {} hand's grip is open",
                socket.label(),
                hand.label()
            )
        } else if !empty {
            format!(
                "give: {name} stays on the {}: the {} hand holds something",
                socket.label(),
                hand.label()
            )
        } else {
            match unsafe { act::draw(hand, socket, hold::SNAP_S) } {
                Ok(()) => format!("give: {name} is in the {} hand", hand.label()),
                Err(refusal) => format!("give: {name} stays on the {}: {refusal}", socket.label()),
            }
        };
        tracing::info!(target: "controllers", "{line}");
        *LAST.lock() = Some(line);
    }
}

/// Look the giveable definitions' display names up again when the game's language has changed.
///
/// # Safety
///
/// The game thread, past startup.
unsafe fn refresh_names() {
    let locale = unsafe { localise::current_locale() };
    if locale.is_none() || NAMES.lock().locale == locale {
        return;
    }
    let names = weapons_for_hand()
        .into_iter()
        .map(|def| {
            // SAFETY: per the contract.
            let display = unsafe { localise::weapon_name(def.name_hash()) };
            (def.name.as_str(), display)
        })
        .collect();
    *NAMES.lock() = DefinitionNames { locale, names };
}

/// Remember that `name` cannot be given, and why.
fn refuse(name: &'static str, why: Refusal) {
    let mut refused = REFUSED.lock();
    match refused.iter_mut().find(|(refused, _)| *refused == name) {
        Some(entry) => entry.1 = why,
        None => refused.push((name, why)),
    }
}

/// The load state of the definition whose unique spawn tag is `tag_index`, or `None` when the
/// spawn system has no definition for it.
///
/// # Safety
///
/// On the game thread. The spawn system's update job may be running and may change the state as it
/// is read; the definitions themselves are built once at initialization.
unsafe fn definition_status(
    system: &SpawnSystem,
    tag_index: i32,
) -> Option<SpawnResourceDefStatus> {
    let hash = unsafe { system.GetTagHashFromTagIndex(tag_index) };
    let def = unsafe { system.GetResourceDefFromTagHash(hash).as_ref() }?;
    // Read as the integer it is stored as, so that a value outside the enum is never made one.
    let raw = unsafe { std::ptr::read_volatile((&raw const def.m_Status).cast::<i32>()) };
    [
        SpawnResourceDefStatus::UNLOADED,
        SpawnResourceDefStatus::LOADING,
        SpawnResourceDefStatus::LOADED,
        SpawnResourceDefStatus::UNLOADING,
        SpawnResourceDefStatus::LOAD_ERROR,
    ]
    .into_iter()
    .find(|status| *status as i32 == raw)
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
        let Some(restored) = (unsafe { inventory_engine::restored(weapon, pending.socket) }) else {
            continue;
        };
        let (outcome, carried) = adoption::carry_onto(owner, strong, restored);
        let line = match carried {
            Some(object) => {
                // SAFETY: the carry set keeps the weapon alive.
                unsafe {
                    single::designate_if_active(&mut *(object as *mut WeaponBase), "a given pistol")
                };
                let landed = match outcome {
                    Outcome::Restored(socket) => socket,
                    _ => pending.socket,
                };
                if let Some(hand) = pending.hand {
                    DRAWS.lock().push((hand, landed, pending.name));
                }
                format!("give: {} is on the {}", pending.name, landed.label())
            }
            None => format!("give: {} was not carried: {outcome:?}", pending.name),
        };
        tracing::info!(target: "controllers", "{line}");
        *LAST.lock() = Some(line);
    }
    let empty = std::mem::take(&mut *EMPTY.lock());
    for (token, objects) in empty {
        let pending = {
            let mut pending = PENDING.lock();
            let index = pending.iter().position(|p| p.token == token);
            index.map(|i| pending.remove(i))
        };
        let Some(pending) = pending else {
            continue;
        };
        let line = format!(
            "give: the spawn system answered {} with {objects} objects, none of them a weapon",
            pending.name
        );
        tracing::info!(target: "controllers", "{line}");
        *LAST.lock() = Some(line);
    }
    unsafe { expire() };
}

/// Give up on each give that has waited too long: withdraw its request, so that a late spawn does
/// not leave a weapon at the player, and say why it did not come.
unsafe fn expire() {
    let mut system = unsafe { SpawnSystem::get() };
    let mut lines = Vec::new();
    let expired: Vec<(Pending, Option<SpawnResourceDefStatus>)> = {
        let mut pending = PENDING.lock();
        let mut expired = Vec::new();
        let mut waiting = Vec::new();
        for mut p in std::mem::take(&mut *pending) {
            let waited = p.at.elapsed();
            if waited <= GIVE_WAIT {
                waiting.push(p);
                continue;
            }
            let status = system
                .as_deref()
                .and_then(|system| unsafe { definition_status(system, p.tag_index) });
            match wait_verdict(status, waited) {
                Verdict::Wait => {
                    if !p.noted_loading {
                        p.noted_loading = true;
                        lines.push(format!("give: {} is still loading", p.name));
                    }
                    waiting.push(p);
                }
                Verdict::GiveUp => expired.push((p, status)),
            }
        }
        *pending = waiting;
        expired
    };
    for (p, status) in expired {
        let shared = PENDING.lock().iter().any(|other| other.token == p.token);
        if let Some(system) = system.as_deref_mut()
            && !shared
        {
            unsafe { system.CancelSpawn(spawned as *const u8, p.token.user_data()) };
        }
        if status == Some(SpawnResourceDefStatus::LOAD_ERROR) {
            refuse(p.name, Refusal::LoadError);
        }
        lines.push(format!(
            "give: {} never came: {}",
            p.name,
            never_came_reason(status)
        ));
    }
    for line in lines {
        tracing::info!(target: "controllers", "{line}");
        *LAST.lock() = Some(line);
    }
}

/// Whether a give past [`GIVE_WAIT`] keeps waiting.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Verdict {
    Wait,
    GiveUp,
}

/// Whether a give that has waited `waited`, past [`GIVE_WAIT`], keeps waiting, by its definition's
/// load state: while the entity is streaming in or queued to, up to [`GIVE_WAIT_LOADING`].
fn wait_verdict(status: Option<SpawnResourceDefStatus>, waited: Duration) -> Verdict {
    let streaming = matches!(
        status,
        Some(SpawnResourceDefStatus::LOADING | SpawnResourceDefStatus::UNLOADED)
    );
    if streaming && waited <= GIVE_WAIT_LOADING {
        Verdict::Wait
    } else {
        Verdict::GiveUp
    }
}

/// Why a give whose definition was in `status` when it gave up did not come.
fn never_came_reason(status: Option<SpawnResourceDefStatus>) -> &'static str {
    match status {
        None => "the spawn system has no definition for it",
        Some(SpawnResourceDefStatus::LOAD_ERROR) => {
            "its entity failed to load, and the spawn system dropped the request"
        }
        Some(SpawnResourceDefStatus::LOADING | SpawnResourceDefStatus::UNLOADED) => {
            "its entity was still not loaded"
        }
        Some(SpawnResourceDefStatus::LOADED | SpawnResourceDefStatus::UNLOADING) => {
            "its entity is loaded, but the spawn did not pass its checks"
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
            entity_hash: None,
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
    fn a_give_waits_past_the_short_wait_only_while_its_entity_streams() {
        let past = GIVE_WAIT + Duration::from_secs(1);
        for streaming in [
            SpawnResourceDefStatus::LOADING,
            SpawnResourceDefStatus::UNLOADED,
        ] {
            assert_eq!(wait_verdict(Some(streaming), past), Verdict::Wait);
            assert_eq!(
                wait_verdict(Some(streaming), GIVE_WAIT_LOADING + Duration::from_secs(1)),
                Verdict::GiveUp
            );
        }
        for settled in [
            None,
            Some(SpawnResourceDefStatus::LOADED),
            Some(SpawnResourceDefStatus::UNLOADING),
            Some(SpawnResourceDefStatus::LOAD_ERROR),
        ] {
            assert_eq!(wait_verdict(settled, past), Verdict::GiveUp);
        }
    }

    #[test]
    fn a_refused_definition_keeps_its_latest_reason() {
        let name = "w999_test_only_definition";
        assert_eq!(refusal(name), None);
        refuse(name, Refusal::NoDefinition);
        refuse(name, Refusal::LoadError);
        assert_eq!(refusal(name), Some(Refusal::LoadError.label()));
        assert_eq!(REFUSED.lock().iter().filter(|(n, _)| *n == name).count(), 1);
    }

    #[test]
    fn a_hand_give_takes_the_first_free_socket_on_the_hands_side() {
        let none = |_: Socket| false;
        assert_eq!(hand_socket(Hand::Left, true, none), Socket::Hip(Hand::Left));
        assert_eq!(
            hand_socket(Hand::Right, false, none),
            Socket::Shoulder(Hand::Right)
        );
        let own_hip = |socket: Socket| socket == Socket::Hip(Hand::Left);
        assert_eq!(
            hand_socket(Hand::Left, true, own_hip),
            Socket::Hip(Hand::Right)
        );
        let both_hips = |socket: Socket| matches!(socket, Socket::Hip(_));
        assert_eq!(hand_socket(Hand::Left, true, both_hips), Socket::Chest);
        let back_full = |socket: Socket| !matches!(socket, Socket::Hip(_) | Socket::Chest);
        assert_eq!(
            hand_socket(Hand::Left, false, back_full),
            Socket::Shoulder(Hand::Left)
        );
    }

    #[test]
    fn a_hand_is_offered_every_hand_weapon() {
        let hand = weapons_for_hand();
        assert_eq!(
            hand.len(),
            weapons_for(Socket::Chest).len() + weapons_for(Socket::BackOuter).len()
        );
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
