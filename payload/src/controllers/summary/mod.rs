//! The controls summary: a current-usable panel on the off-hand wrist and a separate complete
//! binding reference, with the game's own names for actions and the runtime's actual source names,
//! and the suppression of the game's own button hints, whose gamepad glyphs do not match the
//! controllers. See `docs/mod/input/controls-summary.md`.
//!
//! - [`config`]: the settings, [`SummaryConfig`];
//! - [`labels`]: the game's localisation key for each action per mode, and the mod's own strings;
//! - [`localise`]: resolving those through the game's localisation store, with an English
//!   fallback;
//! - [`content`]: the labelled rows grouped by hand, and their sizes on the panel;
//! - the test-only glyph mapping retains asset and physical-source compatibility checks;
//! - [`glance`]: the panel's pose on the wrist, the glance that shows it, and its fade;
//! - [`game_hints`]: hiding the HUD's context button hints;
//! - [`context`]: those hints relabelled for the controllers, first on the wrist while they show.
//!
//! The rows come from the binding tables themselves
//! ([`crate::controllers::bindings::tables::describe`]), so the summary cannot drift from what the
//! controllers inject, and each row's control from the sources the runtime bound its actions to
//! ([`crate::vr::input::sources::table`]): a compact control label for a known source, the runtime's
//! name otherwise, and "unbound" in the reference for an action bound to nothing. The input tick
//! rebuilds the summary when the mode, the table context, the enabled gestures, the game's
//! language, or the bound sources change ([`refresh`]), resolving the strings on the game thread;
//! the render thread draws the latest one ([`crate::hud::summary_panel`]). A rebuild asks for the
//! sources to be read again, and the panel keeps the active sets' sources fresh while it shows
//! ([`note_shown`]), since a rebind within a profile raises no event.

pub mod config;
pub mod content;
pub mod context;
pub(crate) mod current;
pub(crate) mod eligibility;
pub mod game_hints;
pub mod glance;
#[cfg(test)]
mod glyphs;
pub(crate) mod grid;
pub mod labels;
pub mod localise;
pub(crate) mod page_clock;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
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
                describe::{
                    DescribeCtx, Features, PhysicalFeatures, describe_bound, describe_sources,
                },
            },
        },
        config::ControllersConfig,
        frame::{ControllerFrame, InteractionProfile},
        mode::{AirborneVariant, Mode},
        summary::{
            content::{Side, Summary, build, with_context},
            context::ContextHint,
            current::{CurrentContext, CurrentSummary, project},
            eligibility::{Eligibility, Omission},
            labels::{HOLD_KEY, ModText, Text},
        },
    },
    vr::input::sources,
};

/// The latest summary, for the panel.
#[derive(Debug, Clone)]
pub struct Published {
    /// The complete binding reference, including unavailable and unbound routes.
    pub summary: Arc<Summary>,
    /// The current usable wrist projection, independent of the reference inventory.
    pub(crate) current: Arc<CurrentSummary>,
    pub(crate) omitted: Arc<
        Vec<(
            crate::controllers::bindings::tables::describe::Row,
            Omission,
        )>,
    >,
    /// Bumped on every complete-reference rebuild.
    pub revision: u64,
    /// Changes only when the projected wrist content changes.
    pub(crate) wrist_revision: u64,
    /// When the mode last changed to one the player settles in with another heading than the last
    /// such mode's ([`shows_mode_change`]).
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
    pub omitted: usize,
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
        omitted: state.published.as_ref().map_or(0, |p| p.omitted.len()),
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
    eligibility: &Eligibility,
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
            sight_chord: controls.sight_chord,
            trackpad_shows_summary: config.summary.show_on_trackpad_touch
                && frame.hand(off).profile == InteractionProfile::ValveIndex,
            wheel_trackpad: weapons.wheel.enabled && weapons.wheel.trackpad,
            wheel_stick: weapons.wheel.enabled && weapons.wheel.stick,
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
    // The game's context hints are relabelled on the wrist only while the HUD's are hidden.
    let hints = if config.summary.hide_game_hints {
        // SAFETY: the input tick runs on the main thread past startup.
        let read = unsafe { context::read() };
        let mut state = STATE.lock();
        if let Some(read) = read {
            state.hints = read;
        }
        state.hints.clone()
    } else {
        Vec::new()
    };
    // SAFETY: the input tick runs on the main thread past startup.
    let locale = unsafe { localise::current_locale() };
    let key = Key {
        ctx,
        locale,
        sources: sources::table_generation(),
        hints: hints_fingerprint(&hints),
    };
    let (generation, table) = sources::table();
    let known_rows = describe_sources(&ctx, &table);
    let report = eligibility::resolve(&known_rows, eligibility);
    let current = &report.context;
    let (previous, same_current) = {
        let state = STATE.lock();
        (state.key, state.current.as_ref() == Some(current))
    };
    if previous == Some(key) && same_current {
        return;
    }
    // Built outside the state lock: the render thread reads the published summary under it, and
    // the lookups take the localisation store's own lock.
    // SAFETY: as above.
    let resolve =
        |text| localise::resolve(text, locale, |key| unsafe { localise::game_string(key) });
    let rows = describe_bound(&ctx, &table);
    let summary = build(&rows, mode, controls.dominant, &resolve, &|name| {
        table.name(name).to_owned()
    });
    let renders = locale.is_some_and(localise::renders);
    // SAFETY: as above.
    let description = |hash| {
        renders
            .then(|| unsafe { localise::game_string_by_hash(hash) })
            .flatten()
    };
    let context_lines = context::lines(
        &hints,
        &rows,
        mode,
        &resolve(Text::Game(HOLD_KEY)),
        &description,
        &resolve,
    );
    let summary = with_context(
        summary,
        context_lines,
        resolve(Text::Mod(ModText::OnScreen)),
    );
    let wrist = project(
        &known_rows,
        mode,
        current,
        &hints,
        &resolve,
        &|name| table.name(name).to_owned(),
        &description,
    );
    // A rebind within a profile raises no event, so read the sources again; the summary rebuilds
    // only if they changed.
    sources::request_refresh();
    tracing::debug!(
        target: "controllers",
        heading = %summary.heading,
        "controls summary rebuilt"
    );
    let mut state = STATE.lock();
    let first = previous.is_none();
    let (shows, settled) = shows_mode_change(state.settled, mode);
    if shows && !first {
        tracing::info!(
            target: "controllers",
            heading = %summary.heading,
            "controls summary: the mode changed, showing the summary"
        );
    }
    // A hint the game has just raised shows the summary too, with the hint first.
    let raised = new_hints(&state.seen_hints, &hints);
    state.seen_hints = hints.iter().map(|h| h.id).collect();
    let on_screen = summary
        .groups
        .first()
        .filter(|g| g.side == Side::Context)
        .map_or(0, |g| g.lines.len());
    if raised && on_screen > 0 {
        tracing::info!(
            target: "controllers",
            hints = hints.len(),
            relabelled = on_screen,
            "controls summary: the game raised a context hint, showing it on the wrist"
        );
    }
    let mode_changed = first || shows || (raised && on_screen > 0);
    state.settled = settled;
    state.key = Some(Key {
        sources: generation,
        ..key
    });
    let revision = state.published.as_ref().map_or(1, |p| p.revision + 1);
    let mode_changed_at = match &state.published {
        Some(p) if !mode_changed => p.mode_changed_at,
        _ => Instant::now(),
    };
    let (wrist_revision, wrist) = retain_wrist(
        state
            .published
            .as_ref()
            .map(|published| (published.wrist_revision, &published.current)),
        wrist,
    );
    state.current = Some(current.clone());
    state.published = Some(Published {
        summary: Arc::new(summary),
        current: wrist,
        omitted: Arc::new(report.omitted),
        revision,
        wrist_revision,
        mode_changed_at,
    });
}

/// Whether a change to `mode` shows the summary, given the heading of the last mode the player
/// settled in, and that heading after the change. Falling is passed through on the way from one mode
/// to another (every jump, every drop off a ledge, every grapple launch), so it neither shows the
/// summary nor counts as the mode the player left: a jump that lands shows nothing, and a fall that
/// opens the parachute or the wingsuit shows that mode's. A change between modes that share a
/// heading (walking and swimming, a seat and another in the same class) shows nothing either.
pub fn shows_mode_change(settled: Option<Text>, mode: Mode) -> (bool, Option<Text>) {
    if transient(mode) {
        return (false, settled);
    }
    let heading = labels::heading(mode);
    (settled != Some(heading), Some(heading))
}

/// Preserve page identity and the shared allocation across reference-only rebuilds, including
/// table-local named-source index changes that do not change the projected display.
fn retain_wrist(
    previous: Option<(u64, &Arc<CurrentSummary>)>,
    next: CurrentSummary,
) -> (u64, Arc<CurrentSummary>) {
    match previous {
        Some((revision, current)) if current.as_ref() == &next => (revision, Arc::clone(current)),
        Some((revision, _)) => (revision + 1, Arc::new(next)),
        None => (1, Arc::new(next)),
    }
}

/// Whether `hints` holds a hint whose id is not in `seen`, the ids of the last read.
fn new_hints(seen: &[u32], hints: &[ContextHint]) -> bool {
    hints.iter().any(|h| !seen.contains(&h.id))
}

/// A fingerprint of `hints`, for the summary's key.
fn hints_fingerprint(hints: &[ContextHint]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for hint in hints {
        (hint.id, hint.action as i32, hint.hold, hint.description).hash(&mut hasher);
    }
    hasher.finish()
}

/// Whether the mode is one the player passes through rather than settles in.
const fn transient(mode: Mode) -> bool {
    matches!(mode, Mode::Airborne(AirborneVariant::Falling))
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
    /// A fingerprint of the game's context hints the summary relabels.
    hints: u64,
}

struct State {
    key: Option<Key>,
    current: Option<CurrentContext>,
    published: Option<Published>,
    /// The heading of the last mode the player settled in ([`shows_mode_change`]).
    settled: Option<Text>,
    /// The game's context hints as last read.
    hints: Vec<ContextHint>,
    /// The ids of the hints last read, to tell a newly raised one.
    seen_hints: Vec<u32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    key: None,
    current: None,
    published: None,
    settled: None,
    hints: Vec::new(),
    seen_hints: Vec::new(),
});

#[cfg(test)]
mod tests {
    use jc3gi::input::input_action_map::Action;

    use super::*;
    use crate::controllers::{
        bindings::tables::describe::{Control, Effect, Gesture, Row},
        frame::Hand,
        mode::{OnFootVariant, VehicleClass, VehicleSeat},
        summary::{
            current::CurrentRouting,
            page_clock::{DWELL_S, PageClock},
        },
    };

    const GROUND: Mode = Mode::OnFoot(OnFootVariant::Ground);
    const FALLING: Mode = Mode::Airborne(AirborneVariant::Falling);

    #[test]
    fn reference_and_ignored_hint_churn_preserve_wrist_page_and_dwell() {
        let row = Row {
            control: Control::A(Hand::Left),
            gesture: Gesture::Press,
            effect: Effect::Actions(vec![Action::JUMP]),
        };
        let current = CurrentContext {
            usable: vec![row.clone()],
            routing: CurrentRouting {
                ready: [true; 2],
                ..Default::default()
            },
            ..Default::default()
        };
        let resolve = |text: Text| format!("{text:?}");
        let wrist = project(
            std::slice::from_ref(&row),
            GROUND,
            &current,
            &[],
            &resolve,
            &|_| String::new(),
            &|_| None,
        );
        let (revision, original) = retain_wrist(None, wrist);
        let mut clock = PageClock::new();
        clock.step(revision, 3, true, true, 0.0);
        let before = clock.step(revision, 3, true, true, DWELL_S + 1.0);
        assert_eq!(before.index, 1);
        let mut rows = vec![row];
        rows.push(Row {
            control: Control::Unbound,
            gesture: Gesture::Press,
            effect: Effect::Actions(vec![Action::RELOAD]),
        });
        let hint = ContextHint {
            id: 42,
            action: Action::SKIP_CUTSCENE,
            hold: true,
            description: 17,
        };
        let unchanged = project(
            &rows,
            GROUND,
            &current,
            &[hint],
            &resolve,
            &|_| String::new(),
            &|_| Some("Unusable hint".to_owned()),
        );
        let (unchanged_revision, retained) = retain_wrist(Some((revision, &original)), unchanged);
        assert_eq!(unchanged_revision, revision);
        assert!(Arc::ptr_eq(&retained, &original));
        assert_eq!(clock.step(unchanged_revision, 3, true, true, 0.0), before);
        let mut changed = retained.as_ref().clone();
        changed.heading.push_str(" changed");
        let (changed_revision, _) = retain_wrist(Some((revision, &retained)), changed);
        assert_eq!(changed_revision, revision + 1);
        assert_eq!(clock.step(changed_revision, 3, true, true, 0.0).index, 0);
    }

    /// Run `modes` from nothing settled, and say which changes showed the summary.
    fn shown(modes: &[Mode]) -> Vec<bool> {
        let mut settled = None;
        modes
            .iter()
            .map(|&mode| {
                let (shows, next) = shows_mode_change(settled, mode);
                settled = next;
                shows
            })
            .collect()
    }

    #[test]
    fn a_jump_that_lands_shows_nothing() {
        assert_eq!(
            shown(&[GROUND, FALLING, GROUND, FALLING, GROUND]),
            [true, false, false, false, false]
        );
    }

    #[test]
    fn a_fall_into_the_wingsuit_or_the_parachute_shows_it() {
        let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
        let parachute = Mode::Airborne(AirborneVariant::Parachute);
        assert_eq!(
            shown(&[
                GROUND, FALLING, wingsuit, FALLING, parachute, FALLING, GROUND
            ]),
            [true, false, true, false, true, false, true]
        );
    }

    #[test]
    fn modes_that_share_a_heading_show_nothing() {
        let car = |driver| {
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Car,
                driver,
            })
        };
        let swimming = Mode::OnFoot(OnFootVariant::Swimming);
        assert_eq!(
            shown(&[GROUND, swimming, car(true), car(false), GROUND]),
            [true, false, true, false, true]
        );
    }
}
