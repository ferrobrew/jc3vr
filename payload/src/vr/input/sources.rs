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
//! active sets are enumerated at every sync before sampling. Protected presses never depend on a
//! display refresh interval. Opaque path identities reuse bounded runtime-owned path strings and
//! localized names; source tables rebuild only on changed paths or a requested name refresh.
//! Profile changes, locale requests, and the diagnostic page's periodic refresh query every set.
//! A same-profile rebind between query and read remains a runtime limitation.

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
use tracking_format::input::{MAX_SOURCE_PATH, MAX_SOURCES, SourceQuery};

use crate::{
    controllers::frame::{Hand, InteractionProfile},
    vr::input::{
        actions::{Actions, SetId},
        layout::hand_path,
        semantic::{
            SemanticSet,
            bound::{self, EnumeratedSource, PhysicalSet, SourceTable},
        },
    },
};

mod metadata;
use crate::vr::input::sources::metadata::MetadataCache;

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
    observation: SourceQuery,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum QueryFailureKind {
    Enumeration,
    PathConversion,
    Limit,
    RetryExhausted,
}

#[derive(Debug)]
struct QueryFailure {
    kind: QueryFailureKind,
    diagnostic: String,
}

impl QueryFailure {
    fn observation(&self) -> SourceQuery {
        match self.kind {
            QueryFailureKind::Limit => SourceQuery::LimitExceeded,
            QueryFailureKind::Enumeration
            | QueryFailureKind::PathConversion
            | QueryFailureKind::RetryExhausted => SourceQuery::Failed,
        }
    }
}

impl ActionSources {
    fn from_query(name: &'static str, result: Result<Vec<BoundSource>, QueryFailure>) -> Self {
        match result {
            Ok(sources) => Self {
                name,
                observation: SourceQuery::Paths(
                    sources
                        .iter()
                        .map(|source| source.path.clone())
                        .collect::<Vec<_>>()
                        .into(),
                ),
                sources: Ok(sources),
            },
            Err(error) => Self {
                name,
                observation: error.observation(),
                sources: Err(error.diagnostic),
            },
        }
    }
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

/// A cached query's exact paths, including successful emptiness and explicit bounded failure.
pub(crate) fn observation_sources(set: SetId, name: &str) -> SourceQuery {
    OBSERVATION_SOURCES.lock().query(set, name)
}

#[derive(Default)]
struct ObservationSources {
    revision: u64,
    entries: Vec<(SetId, &'static str, SourceQuery)>,
}

static OBSERVATION_SOURCES: Mutex<ObservationSources> = Mutex::new(ObservationSources {
    revision: 0,
    entries: Vec::new(),
});

fn refresh_observation_sources(cache: &[SetSources]) {
    OBSERVATION_SOURCES.lock().refresh(cache);
}

impl ObservationSources {
    fn query(&self, set: SetId, name: &str) -> SourceQuery {
        self.entries
            .iter()
            .find(|entry| entry.0 == set && entry.1 == name)
            .map_or(SourceQuery::NotEnumerated, |entry| entry.2.clone())
    }

    fn refresh(&mut self, cache: &[SetSources]) {
        let entries = cache
            .iter()
            .flat_map(|entry| {
                entry
                    .actions
                    .iter()
                    .map(move |action| (entry.set, action.name, action.observation.clone()))
            })
            .collect::<Vec<_>>();
        if self.entries != entries {
            self.revision = self
                .revision
                .checked_add(1)
                .expect("input: source revision exhausted");
            self.entries = entries;
        }
    }
}

/// An interpretation generation, not the summary's glyph/localization generation. Same-profile
/// rebinding remains undetectable until enumeration; this cache is not a validation authority.
pub(crate) fn observation_generation(profiles: [InteractionProfile; 2], dominant: Hand) -> u64 {
    let revision = OBSERVATION_SOURCES.lock().revision;
    advance_interpretation(
        &mut INTERPRETATION.lock(),
        Interpretation {
            profiles,
            dominant,
            revision,
        },
    )
}

fn advance_interpretation(
    published: &mut (u64, Option<Interpretation>),
    key: Interpretation,
) -> u64 {
    if published.1.as_ref() != Some(&key) {
        published.0 = published
            .0
            .checked_add(1)
            .expect("input: interpretation generation exhausted");
        published.1 = Some(key);
    }
    published.0
}

#[derive(PartialEq)]
struct Interpretation {
    profiles: [InteractionProfile; 2],
    dominant: Hand,
    revision: u64,
}

static INTERPRETATION: Mutex<(u64, Option<Interpretation>)> = Mutex::new((0, None));

/// Note that the debug UI is showing the sources, so the frame keeps them all fresh.
pub(crate) fn note_viewed() {
    *VIEWED_AT.lock() = Some(Instant::now());
}

/// Refresh every set's source names on the next frame after a locale or diagnostic request.
pub(crate) fn request_refresh() {
    REFRESH_REQUESTED.store(true, Ordering::Relaxed);
}

/// Drop the cache: the session's actions are gone.
pub(crate) fn clear() {
    CACHE.lock().clear();
    refresh_observation_sources(&[]);
    *ENUMERATED.lock() = [const { None }; SemanticSet::ALL.len()];
    publish_table(SourceTable::default());
}

/// The sources `action` is bound to, enumerated and named by the runtime.
pub(crate) fn query_action(
    session: &xr::Session<xr::D3D11>,
    action: sys::Action,
) -> Result<Vec<BoundSource>, String> {
    query_action_typed(session, action).map_err(|error| error.diagnostic)
}

fn query_action_typed(
    session: &xr::Session<xr::D3D11>,
    action: sys::Action,
) -> Result<Vec<BoundSource>, QueryFailure> {
    let paths = enumerate(session, action)?;
    let flags =
        xr::InputSourceLocalizedNameFlags::USER_PATH | xr::InputSourceLocalizedNameFlags::COMPONENT;
    identify_sources(
        paths,
        |source| {
            session
                .instance()
                .path_to_string(source)
                .map_err(|error| error.to_string())
        },
        |source| {
            session
                .input_source_localized_name(source, flags)
                .map_err(|error| error.to_string())
        },
    )
}

fn identify_sources<T: Copy>(
    paths: Vec<T>,
    mut identify: impl FnMut(T) -> Result<String, String>,
    mut localize: impl FnMut(T) -> Result<String, String>,
) -> Result<Vec<BoundSource>, QueryFailure> {
    if paths.len() > MAX_SOURCES {
        return Err(QueryFailure {
            kind: QueryFailureKind::Limit,
            diagnostic: "sources: source count exceeds the capture bound".to_owned(),
        });
    }
    paths
        .into_iter()
        .map(|source| {
            let path = identify(source).map_err(|error| QueryFailure {
                kind: QueryFailureKind::PathConversion,
                diagnostic: format!("sources: path identification: {error}"),
            })?;
            if path.len() > MAX_SOURCE_PATH {
                return Err(QueryFailure {
                    kind: QueryFailureKind::Limit,
                    diagnostic: "sources: source path exceeds the capture bound".to_owned(),
                });
            }
            let hand = Hand::BOTH
                .into_iter()
                .find(|&hand| path.starts_with(&format!("{}/", hand_path(hand))));
            Ok(BoundSource {
                path,
                hand,
                localized: localize(source),
            })
        })
        .collect()
}

/// When the cache is refreshed. Owned by the input state, driven from the frame's update.
pub(crate) struct SourcesRefresh {
    /// Whether every set must be queried on the next update.
    stale: bool,
    last_full: Option<Instant>,
    metadata: MetadataCache,
    action_paths: Vec<(sys::Action, Result<Vec<xr::Path>, SourceQuery>)>,
}

impl SourcesRefresh {
    pub(crate) fn new() -> SourcesRefresh {
        SourcesRefresh {
            stale: true,
            last_full: None,
            metadata: MetadataCache::default(),
            action_paths: Vec::new(),
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
        if full {
            self.metadata.clear();
        }
        let mut fresh = Vec::new();
        for set in SetId::all() {
            let is_active = active.contains(&set);
            if !full && !is_active {
                continue;
            }
            let queries = actions
                .set_actions(set)
                .into_iter()
                .map(|(name, action)| {
                    let paths = enumerate(session, action).map(|mut paths| {
                        paths.sort_by_key(|path| path.into_raw());
                        paths.dedup();
                        paths
                    });
                    let identity = paths
                        .as_ref()
                        .map(Clone::clone)
                        .map_err(QueryFailure::observation);
                    let slot = self.action_paths.iter_mut().find(|(key, _)| *key == action);
                    let changed = match slot {
                        Some((_, previous)) if *previous == identity => false,
                        Some((_, previous)) => {
                            *previous = identity;
                            true
                        }
                        None => {
                            self.action_paths.push((action, identity));
                            true
                        }
                    };
                    (name, action, paths, changed)
                })
                .collect::<Vec<_>>();
            if !full && !queries.iter().any(|(_, _, _, changed)| *changed) {
                continue;
            }
            let flags = xr::InputSourceLocalizedNameFlags::USER_PATH
                | xr::InputSourceLocalizedNameFlags::COMPONENT;
            fresh.push(SetSources {
                set,
                refreshed_at: now,
                active: is_active,
                actions: queries
                    .into_iter()
                    .map(|(name, action, paths, _)| {
                        let result = paths.and_then(|paths| {
                            self.metadata.resolve(
                                paths,
                                |path| {
                                    session
                                        .instance()
                                        .path_to_string(path)
                                        .map_err(|error| error.to_string())
                                },
                                |path| {
                                    session
                                        .input_source_localized_name(path, flags)
                                        .map_err(|error| error.to_string())
                                },
                            )
                        });
                        if result.is_err() {
                            self.action_paths.retain(|(key, _)| *key != action);
                        }
                        ActionSources::from_query(name, result)
                    })
                    .collect(),
            });
        }
        if full {
            self.stale = false;
            self.last_full = Some(now);
        }
        {
            let mut cache = CACHE.lock();
            for entry in cache.iter_mut() {
                entry.active = active.contains(&entry.set);
                if entry.active {
                    entry.refreshed_at = now;
                }
            }
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
        let mut changed = false;
        for entry in fresh {
            match cache.iter_mut().find(|c| c.set == entry.set) {
                Some(slot) => {
                    changed |= slot.actions != entry.actions;
                    *slot = entry;
                }
                None => {
                    changed = true;
                    cache.push(entry);
                }
            }
        }
        cache.sort_by_key(|c| SetId::all().position(|s| s == c.set));
        if !changed {
            return;
        }
        let table = source_table(&cache);
        refresh_observation_sources(&cache);
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
fn enumerate(
    session: &xr::Session<xr::D3D11>,
    action: sys::Action,
) -> Result<Vec<xr::Path>, QueryFailure> {
    let info = sys::BoundSourcesForActionEnumerateInfo {
        ty: sys::BoundSourcesForActionEnumerateInfo::TYPE,
        next: ptr::null(),
        action,
    };
    let fp = session.instance().fp().enumerate_bound_sources_for_action;
    enumerate_with(|buffer| {
        let mut count = 0;
        let capacity = buffer.len() as u32;
        let pointer = if buffer.is_empty() {
            ptr::null_mut()
        } else {
            buffer.as_mut_ptr()
        };
        // SAFETY: the buffer has the advertised capacity and remains live for the call.
        let result = unsafe { fp(session.as_raw(), &info, capacity, &mut count, pointer) };
        (result, count)
    })
}

fn enumerate_with(
    mut query: impl FnMut(&mut [xr::Path]) -> (sys::Result, u32),
) -> Result<Vec<xr::Path>, QueryFailure> {
    for _ in 0..3 {
        let (result, count) = query(&mut []);
        enumeration_result(result)?;
        check_count(count)?;
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut paths = vec![xr::Path::NULL; count as usize];
        let (result, written) = query(&mut paths);
        check_count(written)?;
        if result == sys::Result::ERROR_SIZE_INSUFFICIENT {
            continue;
        }
        enumeration_result(result)?;
        if written as usize > paths.len() {
            continue;
        }
        paths.truncate(written as usize);
        return Ok(paths);
    }
    Err(QueryFailure {
        kind: QueryFailureKind::RetryExhausted,
        diagnostic: "sources: enumeration retry bound exhausted".to_owned(),
    })
}

fn enumeration_result(result: sys::Result) -> Result<(), QueryFailure> {
    if result.into_raw() >= 0 {
        return Ok(());
    }
    Err(QueryFailure {
        kind: if result == sys::Result::ERROR_LIMIT_REACHED {
            QueryFailureKind::Limit
        } else {
            QueryFailureKind::Enumeration
        },
        diagnostic: format!("sources: enumeration: {result}"),
    })
}

fn check_count(count: u32) -> Result<(), QueryFailure> {
    if count as usize <= MAX_SOURCES {
        return Ok(());
    }
    Err(QueryFailure {
        kind: QueryFailureKind::Limit,
        diagnostic: "sources: source count exceeds the capture bound".to_owned(),
    })
}

#[cfg(test)]
mod tests;
