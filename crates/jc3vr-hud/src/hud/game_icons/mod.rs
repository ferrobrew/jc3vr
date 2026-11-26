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
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, LazyLock,
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

pub(crate) use avtx::Image;
use parking_lot::Mutex;

pub(crate) use crate::controllers::weapons::hud_feed::icon::IconId;
use crate::hud::game_icons::archive::ArchiveIndex;

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
    CACHE.lookup(id)
}

#[must_use = "a loader that did not stop means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    CACHE.shutdown()
}

static CACHE: LazyLock<Worker> = LazyLock::new(|| Worker::new(open_loader));

const SHUTDOWN_POLLS: u32 = 1000;

type Load = dyn Fn(IconId) -> Lookup + Send;
type LoaderFactory = fn() -> Box<Load>;

struct Worker {
    state: Arc<Mutex<State>>,
    loader: LoaderFactory,
    polls: u32,
}

struct State {
    icons: HashMap<IconId, Lookup>,
    sender: Option<Sender<IconId>>,
    handle: Option<JoinHandle<()>>,
    shutdown: Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shutdown {
    Running,
    Joining,
    Stopped,
    Failed,
}

enum Role {
    Join(JoinHandle<()>),
    Wait,
    Done(bool),
}

impl Worker {
    fn new(loader: LoaderFactory) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                icons: HashMap::new(),
                sender: None,
                handle: None,
                shutdown: Shutdown::Running,
            })),
            loader,
            polls: SHUTDOWN_POLLS,
        }
    }

    #[cfg(test)]
    fn with_polls(mut self, polls: u32) -> Self {
        self.polls = polls;
        self
    }

    fn lookup(&self, id: IconId) -> Lookup {
        let mut state = self.state.lock();
        if state.shutdown != Shutdown::Running {
            return state.icons.get(&id).cloned().unwrap_or(Lookup::Missing);
        }
        if let Some(lookup) = state.icons.get(&id) {
            return lookup.clone();
        }
        if state.sender.is_none() {
            match spawn(&self.state, self.loader) {
                Ok((sender, handle)) => {
                    state.sender = Some(sender);
                    state.handle = Some(handle);
                }
                Err(e) => {
                    tracing::error!("game icons: could not start the loader thread: {e}");
                    state.icons.insert(id, Lookup::Missing);
                    return Lookup::Missing;
                }
            }
        }
        let queued = state
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(id).is_ok());
        if !queued {
            state.icons.insert(id, Lookup::Missing);
            return Lookup::Missing;
        }
        state.icons.insert(id, Lookup::Pending);
        Lookup::Pending
    }

    fn shutdown(&self) -> bool {
        let role = {
            let mut state = self.state.lock();
            match state.shutdown {
                Shutdown::Stopped => Role::Done(true),
                Shutdown::Failed => Role::Done(false),
                Shutdown::Joining => Role::Wait,
                Shutdown::Running => {
                    state.sender = None;
                    match state.handle.take() {
                        Some(handle) => {
                            state.shutdown = Shutdown::Joining;
                            Role::Join(handle)
                        }
                        None => {
                            state.shutdown = Shutdown::Stopped;
                            Role::Done(true)
                        }
                    }
                }
            }
        };
        match role {
            Role::Done(stopped) => stopped,
            Role::Join(handle) => self.join(handle),
            Role::Wait => self.await_join(),
        }
    }

    fn join(&self, handle: JoinHandle<()>) -> bool {
        let finished = self.wait_finished(&handle);
        let joined = finished && handle.join().is_ok();
        if !finished {
            tracing::error!(
                "game icons: the loader did not finish within {} s; leaving the payload mapped \
                 rather than unmapping under a live thread",
                self.polls / 100,
            );
        } else if !joined {
            tracing::error!("game icons: the loader panicked; leaving the payload mapped");
        }
        let mut state = self.state.lock();
        state.shutdown = if joined {
            Shutdown::Stopped
        } else {
            Shutdown::Failed
        };
        joined
    }

    fn wait_finished(&self, handle: &JoinHandle<()>) -> bool {
        for _ in 0..self.polls {
            if handle.is_finished() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn await_join(&self) -> bool {
        for _ in 0..self.polls {
            match self.state.lock().shutdown {
                Shutdown::Stopped => return true,
                Shutdown::Failed => return false,
                Shutdown::Running | Shutdown::Joining => {}
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

fn spawn(
    state: &Arc<Mutex<State>>,
    loader: LoaderFactory,
) -> std::io::Result<(Sender<IconId>, JoinHandle<()>)> {
    let (sender, receiver) = mpsc::channel();
    let state = Arc::clone(state);
    let handle = std::thread::Builder::new()
        .name("jc3vr-game-icons".into())
        .spawn(move || serve(receiver, state, loader()))?;
    Ok((sender, handle))
}

fn serve(receiver: Receiver<IconId>, state: Arc<Mutex<State>>, load: Box<Load>) {
    for id in receiver {
        let loaded = load(id);
        state.lock().icons.insert(id, loaded);
    }
}

fn open_loader() -> Box<Load> {
    let index = game_dir().and_then(|dir| ArchiveIndex::open(&dir).map_err(|e| e.to_string()));
    match &index {
        Ok(index) => tracing::info!("game icons: indexed {} archives", index.archives()),
        Err(e) => tracing::error!("game icons: {e}; the mod's icons stay hidden"),
    }
    Box::new(move |id| match &index {
        Ok(index) => load(index, id),
        Err(_) => Lookup::Missing,
    })
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
    let path = icon_path(id);
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

/// The icon's path in the archives.
fn icon_path(id: IconId) -> String {
    format!("textures/ui/weapon_icons/{}.ddsc", id.0)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    static RELEASE: AtomicBool = AtomicBool::new(false);

    fn missing_loader() -> Box<Load> {
        Box::new(|_| Lookup::Missing)
    }

    fn panicking_loader() -> Box<Load> {
        Box::new(|_| panic!("the loader failed"))
    }

    fn blocking_loader() -> Box<Load> {
        Box::new(|_| {
            while !RELEASE.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            Lookup::Missing
        })
    }

    #[test]
    fn queued_icons_drain_before_shutdown_returns() {
        let worker = Worker::new(missing_loader);
        let ids = [IconId(1), IconId(2), IconId(3)];
        for id in ids {
            assert!(matches!(worker.lookup(id), Lookup::Pending));
        }
        assert!(worker.shutdown(), "the loader stops after the queue drains");
        assert_eq!(worker.state.lock().shutdown, Shutdown::Stopped);
        let state = worker.state.lock();
        for id in ids {
            assert!(
                matches!(state.icons.get(&id), Some(Lookup::Missing)),
                "icon {} was left undecoded",
                id.0,
            );
        }
        assert!(state.sender.is_none());
        assert!(state.handle.is_none());
    }

    #[test]
    fn lookup_after_shutdown_does_not_respawn() {
        let worker = Worker::new(missing_loader);
        assert!(worker.shutdown());
        assert!(matches!(worker.lookup(IconId(7)), Lookup::Missing));
        let state = worker.state.lock();
        assert!(state.sender.is_none(), "a lookup after shutdown respawned");
        assert!(state.handle.is_none(), "a lookup after shutdown respawned");
        assert!(!state.icons.contains_key(&IconId(7)));
    }

    #[test]
    fn lookup_after_shutdown_reports_the_established_result() {
        let worker = Worker::new(missing_loader);
        assert!(matches!(worker.lookup(IconId(4)), Lookup::Pending));
        assert!(worker.shutdown());
        assert!(matches!(worker.lookup(IconId(4)), Lookup::Missing));
    }

    #[test]
    fn shutdown_without_a_worker_reports_stopped() {
        let worker = Worker::new(missing_loader);
        assert!(worker.shutdown());
        assert_eq!(worker.state.lock().shutdown, Shutdown::Stopped);
    }

    #[test]
    fn repeated_shutdown_reports_stopped() {
        let unused = Worker::new(missing_loader);
        assert!(unused.shutdown());
        assert!(unused.shutdown());

        let spawned = Worker::new(missing_loader);
        assert!(matches!(spawned.lookup(IconId(1)), Lookup::Pending));
        assert!(spawned.shutdown());
        assert!(spawned.shutdown());
    }

    #[test]
    fn timeout_is_sticky_and_unconfirmed() {
        let worker = Worker::new(missing_loader).with_polls(0);
        assert!(matches!(worker.lookup(IconId(1)), Lookup::Pending));
        assert!(
            !worker.shutdown(),
            "an unconfirmed stop must not report true"
        );
        assert_eq!(worker.state.lock().shutdown, Shutdown::Failed);
        assert!(
            !worker.shutdown(),
            "a repeated shutdown must stay unconfirmed"
        );
    }

    #[test]
    fn shutdown_reports_a_panicked_loader() {
        let worker = Worker::new(panicking_loader);
        assert!(matches!(worker.lookup(IconId(1)), Lookup::Pending));
        assert!(!worker.shutdown(), "a panicked loader did not stop cleanly");
        assert_eq!(worker.state.lock().shutdown, Shutdown::Failed);
        assert!(!worker.shutdown());
    }

    #[test]
    fn concurrent_shutdown_confirms_only_after_the_join() {
        RELEASE.store(false, Ordering::SeqCst);
        let worker = Arc::new(Worker::new(blocking_loader));
        assert!(matches!(worker.lookup(IconId(1)), Lookup::Pending));

        let first = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.shutdown())
        };
        let second = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.shutdown())
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !first.is_finished(),
            "shutdown confirmed while the loader was still running"
        );
        assert!(
            !second.is_finished(),
            "a concurrent shutdown confirmed while the loader was still running"
        );

        RELEASE.store(true, Ordering::SeqCst);
        assert!(first.join().unwrap());
        assert!(second.join().unwrap());
        assert_eq!(worker.state.lock().shutdown, Shutdown::Stopped);
    }
}
