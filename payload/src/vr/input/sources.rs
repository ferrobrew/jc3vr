//! Bound sources: the physical inputs the runtime bound each action to, with the runtime's
//! localized names for them, cached for the debug UI, reduced for each semantic action to the
//! physical inputs it reads on each hand ([`enumerated`]), which the binding tables' masks act on
//! ([`crate::vr::input::semantic::bound`]), and matched to the known components for the controls
//! summary's glyphs ([`table`]).
//!
//! The queries need the session, so they run on the game thread inside the frame's input update,
//! under the runtime lock, which publishes the results; every other reader only reads them (see
//! `docs/mod/threading.md`). The source paths are opaque to OpenXR, and the mod only matches them
//! as text against the inputs it knows on the shipped profiles.
//!
//! The runtime raises no event when the player rebinds within the same interaction profile, so the
//! cache is refreshed:
//!
//! - in full after the interaction profiles change,
//! - per set, the first time the set is active,
//! - in full on the next frame after the controls summary is rebuilt ([`request_refresh`]),
//! - in full once a second while the debug UI's page is showing ([`note_viewed`]),
//! - the active sets once a second while the controls summary is showing ([`note_shown`]).

use std::{
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use openxr as xr;
use openxr::sys;
use parking_lot::Mutex;

use crate::{
    controllers::frame::Hand,
    vr::input::{
        actions::{Actions, SetId},
        layout::hand_path,
        semantic::{
            SemanticSet,
            bound::{self, EnumeratedSource, PhysicalSet, SourceTable},
        },
    },
};

/// One source an action is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundSource {
    /// The source path as the runtime names it.
    pub(crate) path: String,
    /// The runtime's localized name of the source, with the user path and the component, or why
    /// it could not be read.
    pub(crate) localized: Result<String, String>,
    /// The hand whose user path the source is under.
    pub(crate) hand: Option<Hand>,
}

/// The sources of one action, or why they could not be enumerated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionSources {
    pub(crate) name: &'static str,
    pub(crate) sources: Result<Vec<BoundSource>, String>,
}

/// The sources of every action of one set.
#[derive(Debug, Clone)]
pub(crate) struct SetSources {
    pub(crate) set: SetId,
    pub(crate) refreshed_at: Instant,
    /// Whether the set was active when it was queried.
    pub(crate) active: bool,
    pub(crate) actions: Vec<ActionSources>,
}

/// The cached sources, one entry per set that has been queried, in stash order.
pub(crate) fn cached() -> Vec<SetSources> {
    CACHE.lock().clone()
}

/// The physical inputs each action of the semantic `set` was last enumerated as reading, by action
/// index (`None` where its enumeration failed), or `None` before the set has been enumerated.
pub(crate) fn enumerated(set: SemanticSet) -> Option<Enumerated> {
    ENUMERATED.lock()[set.index()].clone()
}

/// The semantic actions' sources as the controls summary shows them, and a generation that
/// changes whenever they do.
pub(crate) fn table() -> (u64, Arc<SourceTable>) {
    let table = TABLE.lock();
    (table.generation, table.table.clone().unwrap_or_default())
}

/// The generation of [`table`], to tell whether it changed without cloning it.
pub(crate) fn table_generation() -> u64 {
    TABLE.lock().generation
}

/// Note that the debug UI is showing the sources, so the frame keeps them all fresh.
pub(crate) fn note_viewed() {
    *VIEWED_AT.lock() = Some(Instant::now());
}

/// Note that the controls summary is showing, so the frame keeps the active sets' sources fresh.
pub(crate) fn note_shown() {
    *SHOWN_AT.lock() = Some(Instant::now());
}

/// Query every set on the next frame: the controls summary was rebuilt, and the player may have
/// rebound since the sources were last read.
pub(crate) fn request_refresh() {
    REFRESH_REQUESTED.store(true, Ordering::Relaxed);
}

/// Drop the cache: the session's actions are gone.
pub(crate) fn clear() {
    CACHE.lock().clear();
    *ENUMERATED.lock() = [const { None }; SemanticSet::ALL.len()];
    publish_table(SourceTable::default());
}

/// The sources `action` is bound to, enumerated and named by the runtime.
pub(crate) fn query_action(
    session: &xr::Session<xr::D3D11>,
    action: sys::Action,
) -> Result<Vec<BoundSource>, String> {
    let paths = enumerate(session, action).map_err(|e| format!("enumerating: {e}"))?;
    let instance = session.instance();
    let flags =
        xr::InputSourceLocalizedNameFlags::USER_PATH | xr::InputSourceLocalizedNameFlags::COMPONENT;
    Ok(paths
        .into_iter()
        .map(|source| {
            let path = instance
                .path_to_string(source)
                .unwrap_or_else(|e| format!("<unnamed: {e}>"));
            let hand = Hand::BOTH
                .into_iter()
                .find(|&h| path.starts_with(&format!("{}/", hand_path(h))));
            BoundSource {
                localized: session
                    .input_source_localized_name(source, flags)
                    .map_err(|e| e.to_string()),
                path,
                hand,
            }
        })
        .collect())
}

/// When the cache is refreshed. Owned by the input state, driven from the frame's update.
pub(crate) struct SourcesRefresh {
    /// Whether each set (in [`SetId::all`] order) has been queried while active.
    seen_active: Vec<bool>,
    /// Whether every set must be queried on the next update.
    stale: bool,
    last_full: Option<Instant>,
    /// When the active sets were last queried for the showing controls summary.
    last_shown: Option<Instant>,
}

impl SourcesRefresh {
    pub(crate) fn new() -> SourcesRefresh {
        SourcesRefresh {
            seen_active: vec![false; SetId::all().count()],
            stale: true,
            last_full: None,
            last_shown: None,
        }
    }

    /// Query every set on the next update: the interaction profiles changed.
    pub(crate) fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Refresh whatever the rules call for, given the sets active this frame.
    pub(crate) fn update(
        &mut self,
        session: &xr::Session<xr::D3D11>,
        actions: &Actions,
        active: &[SetId],
    ) {
        let now = Instant::now();
        let recent = |at: &Mutex<Option<Instant>>| {
            at.lock()
                .is_some_and(|t| now.saturating_duration_since(t) < VIEW_LINGER)
        };
        let elapsed = |last: Option<Instant>| {
            last.is_none_or(|t| now.saturating_duration_since(t) >= VIEWED_INTERVAL)
        };
        let due = recent(&VIEWED_AT) && elapsed(self.last_full);
        let requested = REFRESH_REQUESTED.swap(false, Ordering::Relaxed);
        let full = self.stale || due || requested;
        let shown = !full && recent(&SHOWN_AT) && elapsed(self.last_shown);
        let mut fresh = Vec::new();
        for (i, set) in SetId::all().enumerate() {
            let is_active = active.contains(&set);
            let first_active = is_active && !self.seen_active[i];
            if !full && !first_active && !(shown && is_active) {
                continue;
            }
            if is_active {
                self.seen_active[i] = true;
            }
            fresh.push(SetSources {
                set,
                refreshed_at: now,
                active: is_active,
                actions: actions
                    .set_actions(set)
                    .into_iter()
                    .map(|(name, action)| ActionSources {
                        name,
                        sources: query_action(session, action),
                    })
                    .collect(),
            });
        }
        if full {
            self.stale = false;
            self.last_full = Some(now);
        }
        if full || shown {
            self.last_shown = Some(now);
        }
        if fresh.is_empty() {
            return;
        }
        {
            let mut enumerated = ENUMERATED.lock();
            for entry in &fresh {
                if let SetId::Semantic(set) = entry.set {
                    enumerated[set.index()] = Some(physical_inputs(entry));
                }
            }
        }
        let mut cache = CACHE.lock();
        for entry in fresh {
            match cache.iter_mut().find(|c| c.set == entry.set) {
                Some(slot) => *slot = entry,
                None => cache.push(entry),
            }
        }
        cache.sort_by_key(|c| SetId::all().position(|s| s == c.set));
        let table = source_table(&cache);
        drop(cache);
        publish_table(table);
    }
}

/// How long after the last [`note_viewed`] the page counts as showing.
const VIEW_LINGER: Duration = Duration::from_secs(2);

/// How often the sources are refreshed while the page is showing.
const VIEWED_INTERVAL: Duration = Duration::from_secs(1);

static CACHE: Mutex<Vec<SetSources>> = Mutex::new(Vec::new());

/// Each action of one set's physical inputs per hand, by action index, `None` where its
/// enumeration failed.
type Enumerated = Vec<Option<[PhysicalSet; 2]>>;

static ENUMERATED: Mutex<[Option<Enumerated>; SemanticSet::ALL.len()]> =
    Mutex::new([const { None }; SemanticSet::ALL.len()]);

/// Each action's physical inputs per hand, from its enumerated sources.
fn physical_inputs(entry: &SetSources) -> Enumerated {
    entry
        .actions
        .iter()
        .map(|action| {
            action
                .sources
                .as_ref()
                .ok()
                .map(|sources| bound::of_sources(sources.iter().map(|s| s.path.as_str())))
        })
        .collect()
}

static VIEWED_AT: Mutex<Option<Instant>> = Mutex::new(None);

static SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

static REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The published [`SourceTable`] and its generation.
struct PublishedTable {
    generation: u64,
    /// `None` before the first publication, which reads as an empty table.
    table: Option<Arc<SourceTable>>,
}

static TABLE: Mutex<PublishedTable> = Mutex::new(PublishedTable {
    generation: 0,
    table: None,
});

/// The summary's view of the cached semantic sets.
fn source_table(cache: &[SetSources]) -> SourceTable {
    let mut table = SourceTable::default();
    for entry in cache {
        let SetId::Semantic(set) = entry.set else {
            continue;
        };
        table.insert(
            set,
            entry.actions.iter().map(|action| {
                action.sources.as_ref().ok().map(|sources| {
                    sources
                        .iter()
                        .map(|s| EnumeratedSource {
                            path: &s.path,
                            localized: s.localized.as_deref().ok(),
                        })
                        .collect()
                })
            }),
        );
    }
    table
}

/// Publish `table`, moving the generation on only when it differs from the published one, so a
/// refresh that finds the same bindings rebuilds nothing.
fn publish_table(table: SourceTable) {
    let mut published = TABLE.lock();
    if published.table.as_deref() != Some(&table) {
        published.generation += 1;
        published.table = Some(Arc::new(table));
    }
}

/// `xrEnumerateBoundSourcesForAction`, with the two-call idiom.
fn enumerate(session: &xr::Session<xr::D3D11>, action: sys::Action) -> xr::Result<Vec<xr::Path>> {
    let info = sys::BoundSourcesForActionEnumerateInfo {
        ty: sys::BoundSourcesForActionEnumerateInfo::TYPE,
        next: ptr::null(),
        action,
    };
    let fp = session.instance().fp().enumerate_bound_sources_for_action;
    loop {
        let mut count = 0u32;
        // SAFETY: a zero capacity with a null buffer asks for the count.
        let result = unsafe { fp(session.as_raw(), &info, 0, &mut count, ptr::null_mut()) };
        if result.into_raw() < 0 {
            return Err(result);
        }
        let mut paths = vec![xr::Path::NULL; count as usize];
        // SAFETY: `paths` holds `count` elements, the capacity passed.
        let result = unsafe {
            fp(
                session.as_raw(),
                &info,
                count,
                &mut count,
                paths.as_mut_ptr(),
            )
        };
        if result == sys::Result::ERROR_SIZE_INSUFFICIENT {
            continue;
        }
        if result.into_raw() < 0 {
            return Err(result);
        }
        paths.truncate(count as usize);
        return Ok(paths);
    }
}
