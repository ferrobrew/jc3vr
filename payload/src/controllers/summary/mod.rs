//! The controls summary: a panel on the off-hand wrist listing what every controller input does in
//! the current mode, with the game's own names for its actions and the Valve Index button glyphs,
//! and the suppression of the game's own button hints, whose gamepad glyphs do not match the
//! controllers. See `docs/mod/input/controls-summary.md`.
//!
//! - [`config`]: the settings, [`SummaryConfig`];
//! - [`labels`]: the game's localisation key for each action per mode, and the mod's own strings;
//! - [`localise`]: resolving those through the game's localisation store, with an English
//!   fallback;
//! - [`content`]: the labelled rows grouped by hand, and their sizes on the panel;
//! - [`glyphs`]: the embedded button glyphs;
//! - [`glance`]: the panel's pose on the wrist, the glance that shows it, and its fade;
//! - [`game_hints`]: hiding the HUD's context button hints.
//!
//! The rows come from the binding tables themselves
//! ([`crate::controllers::bindings::tables::describe`]), so the summary cannot drift from what the
//! controllers inject, and each row's control from the sources the runtime bound its actions to
//! ([`crate::vr::input::sources::table`]): a glyph where the source is an input the mod knows, the
//! runtime's name for it otherwise, and "unbound" for an action bound to nothing. The input tick
//! rebuilds the summary when the mode, the table context, the enabled gestures, the game's
//! language, or the bound sources change ([`refresh`]), resolving the strings on the game thread;
//! the render thread draws the latest one ([`crate::hud::summary_panel`]). A rebuild asks for the
//! sources to be read again, and the panel keeps the active sets' sources fresh while it shows
//! ([`note_shown`]), since a rebind within a profile raises no event.

pub mod config;
pub mod content;
pub mod game_hints;
pub mod glance;
pub mod glyphs;
pub mod labels;
pub mod localise;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use jc3gi::ui::locator::Locale;
use parking_lot::Mutex;

pub use crate::controllers::summary::config::SummaryConfig;
use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            tables::{
                declare::TableCtx,
                describe::{DescribeCtx, Features, PhysicalFeatures, describe_bound},
            },
        },
        config::ControllersConfig,
        frame::{ControllerFrame, InteractionProfile},
        mode::Mode,
        summary::content::{Summary, build},
    },
    vr::input::sources,
};

/// The latest summary, for the panel.
#[derive(Debug, Clone)]
pub struct Published {
    pub summary: Arc<Summary>,
    /// Bumped on every rebuild, so that the panel can tell a new summary from the one it drew.
    pub revision: u64,
    /// When the mode last changed to one with another heading.
    pub mode_changed_at: Instant,
}

/// The latest summary, or `None` before the first refresh.
pub fn published() -> Option<Published> {
    STATE.lock().published.clone()
}

/// The summary's readout for the debug UI.
#[derive(Debug, Clone, Default)]
pub struct SummaryStatus {
    pub heading: String,
    pub rows: usize,
    pub locale: Option<Locale>,
    pub revision: u64,
}

pub fn status() -> SummaryStatus {
    let state = STATE.lock();
    SummaryStatus {
        heading: state
            .published
            .as_ref()
            .map(|p| p.summary.heading.clone())
            .unwrap_or_default(),
        rows: state
            .published
            .as_ref()
            .map_or(0, |p| p.summary.groups.iter().map(|g| g.lines.len()).sum()),
        locale: state.key.and_then(|k| k.locale),
        revision: state.published.as_ref().map_or(0, |p| p.revision),
    }
}

/// Note that the panel is showing, so the bound sources it labels are kept fresh. A leaf slot:
/// safe from the draw task.
pub fn note_shown() {
    sources::note_shown();
}

/// Rebuild the summary if what it depends on changed. Runs on the input tick, after the bindings
/// are evaluated for `mode` with `controls`.
pub(crate) fn refresh(
    mode: Mode,
    controls: &Controls,
    config: &ControllersConfig,
    frame: &ControllerFrame,
) {
    if !config.summary.enabled {
        return;
    }
    let off = controls.dominant.other();
    let weapons = &config.weapons;
    let physical = &config.vehicles.physical;
    let ctx = DescribeCtx {
        mode,
        dominant: controls.dominant,
        table: TableCtx::of(controls),
        features: Features {
            yank: config.bindings.yank.enabled,
            holsters: weapons.holsters.enabled,
            foregrip: weapons.foregrip.enabled,
            magazine: weapons.reload.enabled,
            belt_reload: weapons.reload.enabled && weapons.reload.belt.enabled,
            flick_eject: weapons.reload.enabled && weapons.reload.flick_eject,
            work_action: weapons.reload.enabled && weapons.reload.chamber,
            grenade_throw: weapons.grenade_throw,
            c4_throw: weapons.c4_throw,
            wrist_detonator: weapons.wrist_detonator,
            melee: weapons.melee,
            sight_chord: controls.sight_chord,
            trackpad_shows_summary: config.summary.show_on_trackpad_touch
                && frame.hand(off).profile == InteractionProfile::ValveIndex,
            parachute_handles: config.vehicles.parachute_handles.enabled,
            arm_steering: config.vehicles.arm_steering.wingsuit,
            physical: PhysicalFeatures {
                wheel: physical.wheel.enabled,
                handlebars: physical.handlebars.enabled,
                helicopter: physical.cyclic.enabled || physical.collective.enabled,
                plane: physical.flight_stick.enabled || physical.throttle.enabled,
            },
        },
    };
    // SAFETY: the input tick runs on the main thread past startup.
    let locale = unsafe { localise::current_locale() };
    let key = Key {
        ctx,
        locale,
        sources: sources::table_generation(),
    };
    let previous = STATE.lock().key;
    if previous == Some(key) {
        return;
    }
    let (generation, table) = sources::table();
    // Built outside the state lock: the render thread reads the published summary under it, and
    // the lookups take the localisation store's own lock.
    // SAFETY: as above.
    let resolve =
        |text| localise::resolve(text, locale, |key| unsafe { localise::game_string(key) });
    let summary = build(
        &describe_bound(&ctx, &table),
        mode,
        controls.dominant,
        &resolve,
        &|name| table.name(name).to_owned(),
    );
    // A rebind within a profile raises no event, so read the sources again; the summary rebuilds
    // only if they changed.
    sources::request_refresh();
    tracing::debug!(
        target: "controllers",
        heading = %summary.heading,
        "controls summary rebuilt"
    );
    // A change between modes that share a heading (walking and swimming, a seat and another in the
    // same class) is not shown as a mode change.
    let mode_changed =
        previous.is_none_or(|k| labels::heading(k.ctx.mode) != labels::heading(mode));
    let mut state = STATE.lock();
    state.key = Some(Key {
        sources: generation,
        ..key
    });
    let revision = state.published.as_ref().map_or(1, |p| p.revision + 1);
    let mode_changed_at = match &state.published {
        Some(p) if !mode_changed => p.mode_changed_at,
        _ => Instant::now(),
    };
    state.published = Some(Published {
        summary: Arc::new(summary),
        revision,
        mode_changed_at,
    });
}

/// Whether the mode changed within `window` of `now`.
pub fn mode_changed_within(published: &Published, now: Instant, window: Duration) -> bool {
    now.saturating_duration_since(published.mode_changed_at) < window
}

/// What a summary was built for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Key {
    ctx: DescribeCtx,
    locale: Option<Locale>,
    /// The generation of the bound sources ([`sources::table`]).
    sources: u64,
}

struct State {
    key: Option<Key>,
    published: Option<Published>,
}

static STATE: Mutex<State> = Mutex::new(State {
    key: None,
    published: None,
});
