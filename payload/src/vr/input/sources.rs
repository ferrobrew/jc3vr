//! Bound sources: the physical inputs the runtime bound each action to, with the runtime's
//! localized names for them, cached for the debug UI.
//!
//! The queries need the session, so they run on the VR thread inside the frame's input update,
//! which publishes the cache; the UI only reads it. The source paths are opaque to OpenXR and are
//! shown, not interpreted, except that a source under a hand's user path is attributed to that
//! hand.
//!
//! The runtime raises no event when the player rebinds within the same interaction profile, so the
//! cache is refreshed:
//!
//! - in full after the interaction profiles change,
//! - per set, the first time the set is active,
//! - in full once a second while the debug UI's page is showing ([`note_viewed`]).

use std::{
    ptr,
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

/// Note that the debug UI is showing the sources, so the VR thread keeps them fresh.
pub(crate) fn note_viewed() {
    *VIEWED_AT.lock() = Some(Instant::now());
}

/// Drop the cache: the session's actions are gone.
pub(crate) fn clear() {
    CACHE.lock().clear();
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
}

impl SourcesRefresh {
    pub(crate) fn new() -> SourcesRefresh {
        SourcesRefresh {
            seen_active: vec![false; SetId::all().count()],
            stale: true,
            last_full: None,
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
        let viewed = VIEWED_AT
            .lock()
            .is_some_and(|t| now.saturating_duration_since(t) < VIEW_LINGER);
        let due = viewed
            && self
                .last_full
                .is_none_or(|t| now.saturating_duration_since(t) >= VIEWED_INTERVAL);
        let full = self.stale || due;
        let mut fresh = Vec::new();
        for (i, set) in SetId::all().enumerate() {
            let is_active = active.contains(&set);
            let first_active = is_active && !self.seen_active[i];
            if !full && !first_active {
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
        if fresh.is_empty() {
            return;
        }
        let mut cache = CACHE.lock();
        for entry in fresh {
            match cache.iter_mut().find(|c| c.set == entry.set) {
                Some(slot) => *slot = entry,
                None => cache.push(entry),
            }
        }
        cache.sort_by_key(|c| SetId::all().position(|s| s == c.set));
    }
}

/// How long after the last [`note_viewed`] the page counts as showing.
const VIEW_LINGER: Duration = Duration::from_secs(2);

/// How often the sources are refreshed while the page is showing.
const VIEWED_INTERVAL: Duration = Duration::from_secs(1);

static CACHE: Mutex<Vec<SetSources>> = Mutex::new(Vec::new());

static VIEWED_AT: Mutex<Option<Instant>> = Mutex::new(None);

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
