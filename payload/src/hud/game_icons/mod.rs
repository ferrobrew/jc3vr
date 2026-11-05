//! The game's own UI icons, read from its archives for the mod's quads.
//!
//! The HUD's weapon icons are standalone textures, one per weapon id
//! (`textures/ui/weapon_icons/<id>.ddsc`, the path the HUD movie's `GetWeaponIconName` builds for
//! the selection cross, the pickup panel, and the pickup markers), not frames inside the movie, so
//! the mod reads them straight from the archives on disk ([`archive`]) and decodes them itself
//! ([`avtx`]) rather than through the game's streaming. The hand grenade and the C4 charge have
//! icons under their weapons' ids too.
//!
//! [`lookup`] never blocks: the first lookup of an icon queues it for a worker thread, which
//! indexes the archives once and decodes each icon asked for; later lookups find it ready, or
//! missing when no archive holds it or it does not decode. A failure is logged once per icon.

mod archive;
mod avtx;

use std::{
    collections::{HashMap, hash_map::Entry},
    path::PathBuf,
    sync::{
        Arc, LazyLock,
        mpsc::{self, Receiver, Sender},
    },
};

pub(crate) use avtx::Image;
use parking_lot::Mutex;

use crate::hud::game_icons::archive::ArchiveIndex;

/// A weapon icon, by the weapon's id
/// ([`WeaponBase::m_WeaponId`](jc3gi::equipment::weapon::WeaponBase::m_WeaponId)).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct IconId(pub i16);

impl IconId {
    /// The icon's path in the archives.
    fn path(self) -> String {
        format!("textures/ui/weapon_icons/{}.ddsc", self.0)
    }
}

/// Where an icon's loading stands.
#[derive(Debug, Clone)]
pub(crate) enum Lookup {
    /// Queued or being read.
    Pending,
    Ready(Arc<Image>),
    /// No archive holds it, or it did not decode.
    Missing,
}

/// The icon `id`, queuing it for the worker on its first lookup.
pub(crate) fn lookup(id: IconId) -> Lookup {
    let mut cache = CACHE.lock();
    match cache.icons.entry(id) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
            entry.insert(Lookup::Pending);
            let sender = cache.sender.get_or_insert_with(spawn_worker);
            if sender.send(id).is_err() {
                // The worker could not start or has ended; nothing will ever load.
                cache.icons.insert(id, Lookup::Missing);
                return Lookup::Missing;
            }
            Lookup::Pending
        }
    }
}

struct Cache {
    icons: HashMap<IconId, Lookup>,
    sender: Option<Sender<IconId>>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| {
    Mutex::new(Cache {
        icons: HashMap::new(),
        sender: None,
    })
});

/// Start the worker that reads the queued icons. A worker that cannot start leaves the sender's
/// receiver dropped, so the send fails and the icon is marked missing.
fn spawn_worker() -> Sender<IconId> {
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("jc3vr-game-icons".into())
        .spawn(move || work(receiver));
    if let Err(e) = spawned {
        tracing::error!("game icons: could not start the loader thread: {e}");
    }
    sender
}

/// Read each queued icon until the sender is gone.
fn work(receiver: Receiver<IconId>) {
    let index = game_dir().and_then(|dir| ArchiveIndex::open(&dir).map_err(|e| e.to_string()));
    match &index {
        Ok(index) => tracing::info!("game icons: indexed {} archives", index.archives()),
        Err(e) => tracing::error!("game icons: {e}; the mod's icons stay hidden"),
    }
    for id in receiver {
        let loaded = match &index {
            Ok(index) => load(index, id),
            Err(_) => Lookup::Missing,
        };
        CACHE.lock().icons.insert(id, loaded);
    }
}

/// The game's install directory: the one holding the running executable.
fn game_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("could not find the game's executable: {e}"))?;
    exe.parent()
        .map(PathBuf::from)
        .ok_or_else(|| format!("the game's executable {} has no directory", exe.display()))
}

fn load(index: &ArchiveIndex, id: IconId) -> Lookup {
    let path = id.path();
    match index.read(&path) {
        Ok(Some(bytes)) => match avtx::decode(&bytes) {
            Ok(image) => Lookup::Ready(Arc::new(image)),
            Err(e) => {
                tracing::warn!("game icons: {path}: {e}");
                Lookup::Missing
            }
        },
        Ok(None) => {
            tracing::warn!("game icons: no archive holds {path}");
            Lookup::Missing
        }
        Err(e) => {
            tracing::warn!("game icons: {path}: {e}");
            Lookup::Missing
        }
    }
}
