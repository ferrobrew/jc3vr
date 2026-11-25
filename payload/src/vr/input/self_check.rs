//! The input self-check: a one-off probe of how the runtime treats the action layout, for the
//! semantic actions' headset validation.
//!
//! Once per input state (each session, and each reinject), after the first focused sync at which a
//! hand has an interaction profile, it logs one block (info, target `vr`, starting
//! `input: self-check`) and keeps it for the debug UI. The block holds:
//!
//! - the runtime's name and version, the dominant hand the bindings were suggested for, and each
//!   hand's interaction profile path;
//! - for every semantic action in an active set whose default binding is the same input as a raw
//!   action's in the always-active `hands` set, whether each of the two reports itself active. Both
//!   sets have priority 0, so the runtime must process both bindings; a pair whose semantic side is
//!   inactive means the runtime gave the input to one set only;
//! - the grip as a boolean (`squeeze_click`, which the runtime thresholds) beside the grip force as
//!   a float (`squeeze_force`), each with its active flag and value;
//! - the bound sources and their localized names of a sample of actions in the active sets and in
//!   inactive ones, which shows whether the bound-source and localized-name calls work through the
//!   runtime (and wineopenxr) and whether a set must be active to answer.
//!
//! After the block it watches the paired inputs and the grip, and logs one `input: self-check
//! finding` line per pair the first time the player presses and releases it: how many frames the
//! raw and the semantic action each read engaged, and for the grip, the force at which the
//! runtime's boolean pressed and released beside the mod's own thresholds. A pair that both report
//! has passed, and the grip's finding is a measurement; only a pair one side of which read nothing
//! is a problem ([`SelfCheckStatus::problems`]).
//!
//! `controllers.input.self_check` turns it off; [`request_rerun`] runs it again (the debug UI's
//! button), for example after switching to a set that was inactive the first time.

use std::sync::atomic::{AtomicBool, Ordering};

use openxr as xr;
use parking_lot::Mutex;

use crate::{
    controllers::{
        config::InputConfig,
        frame::{ActionSetKind, GripPress, Hand, HandFrame, HandInputs, InteractionProfile},
    },
    vr::input::{
        actions::{Actions, SetId},
        layout::{self, BoolInput, FloatInput, Input, Profile, Vec2Input},
        semantic::{
            SCHEMA_VERSION, SemanticSet, defaults,
            frame::{ActionValue, SemanticFrame},
        },
        sources,
    },
};

/// The self-check's results, for the debug UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct SelfCheckStatus {
    /// The logged block, once it has run.
    pub(crate) report: Option<String>,
    /// The findings logged since.
    pub(crate) findings: Vec<String>,
    /// How many of the findings are problems: a pair the runtime gave to one set only.
    pub(crate) problems: usize,
}

/// A snapshot of the self-check's results.
pub(crate) fn status() -> SelfCheckStatus {
    STATUS.lock().clone()
}

/// Run the self-check again on the next frame.
pub(crate) fn request_rerun() {
    RERUN.store(true, Ordering::Relaxed);
}

/// What one frame's update reads.
pub(crate) struct CheckFrame<'a> {
    pub(crate) session: &'a xr::Session<xr::D3D11>,
    pub(crate) actions: &'a Actions,
    pub(crate) profiles: [InteractionProfile; 2],
    pub(crate) mode: ActionSetKind,
    /// Whether the session has input focus; unfocused, every action reads inactive.
    pub(crate) focused: bool,
    /// The dominant hand the bindings were suggested for.
    pub(crate) dominant: Hand,
    pub(crate) hands: &'a [HandFrame; 2],
    pub(crate) semantic: &'a SemanticFrame,
    pub(crate) config: &'a InputConfig,
}

/// The self-check's progress through one input state.
pub(crate) struct SelfCheck {
    ran: bool,
    watches: Vec<OverlapWatch>,
    grips: [GripWatch; 2],
}

impl SelfCheck {
    pub(crate) fn new() -> SelfCheck {
        SelfCheck {
            ran: false,
            watches: Vec::new(),
            grips: [GripWatch::default(); 2],
        }
    }

    /// Run the check once a hand has a profile, then watch the paired inputs.
    pub(crate) fn update(&mut self, frame: &CheckFrame) {
        if RERUN.swap(false, Ordering::Relaxed) {
            *self = SelfCheck::new();
            *STATUS.lock() = SelfCheckStatus::default();
        }
        if !self.ran {
            if !frame.focused
                || frame
                    .profiles
                    .iter()
                    .all(|&p| p == InteractionProfile::None)
            {
                return;
            }
            self.ran = true;
            self.watches = pairs(frame.profiles, frame.dominant)
                .into_iter()
                .map(OverlapWatch::new)
                .collect();
            let report = report(frame, &self.watches);
            tracing::info!(target: "vr", "input: self-check\n{report}");
            STATUS.lock().report = Some(report);
        }
        self.watch_overlaps(frame);
        self.watch_grips(frame);
    }

    fn watch_overlaps(&mut self, frame: &CheckFrame) {
        for watch in self.watches.iter_mut().filter(|w| !w.done) {
            let Some(state) = frame.semantic.set(watch.pair.set) else {
                continue;
            };
            let hand = watch.pair.hand;
            let semantic = state.actions[watch.pair.index][hand.index()]
                .value
                .engaged();
            let raw = watch.pair.raw.engaged(&frame.hands[hand.index()].inputs);
            if let Some((finding, verdict)) = watch.observe(semantic, raw) {
                note_finding(finding, verdict == Verdict::OneSided);
            }
        }
    }

    fn watch_grips(&mut self, frame: &CheckFrame) {
        for hand in Hand::BOTH {
            let watch = &mut self.grips[hand.index()];
            if watch.done || layout::shipped(frame.profiles[hand.index()]).is_none() {
                continue;
            }
            let Ok(state) = frame
                .actions
                .bool_state(frame.session, BoolInput::SqueezeClick, hand)
            else {
                continue;
            };
            let inputs = &frame.hands[hand.index()].inputs;
            let reading = GripReading {
                force: inputs.squeeze_force,
                value: inputs.squeeze,
            };
            if let Some(finding) = watch.observe(state.value, reading) {
                let rule = match frame.config.grip_press(frame.profiles[hand.index()], hand) {
                    GripPress::Force(t) => {
                        format!("force, press {:.2} release {:.2}", t.press, t.release)
                    }
                    GripPress::Value(t) => {
                        format!("value, press {:.2} release {:.2}", t.press, t.release)
                    }
                };
                note_finding(
                    format!(
                        "grip on the {} hand: the runtime's boolean {finding}; the mod reads the {rule}",
                        hand.label()
                    ),
                    false,
                );
            }
        }
    }
}

/// A semantic action and a raw action that share a default input on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Pair {
    set: SemanticSet,
    index: usize,
    hand: Hand,
    path: &'static str,
    raw: RawProbe,
}

/// A raw input, or one axis of a raw two-axis input.
#[derive(Debug, Copy, Clone, PartialEq)]
struct RawProbe {
    input: Input,
    /// For a two-axis input: 0 for x, 1 for y.
    axis: Option<usize>,
}

impl RawProbe {
    fn engaged(self, inputs: &HandInputs) -> bool {
        let value = match self.input {
            Input::Bool(b) => ActionValue::Bool(match b {
                BoolInput::TriggerClick => inputs.trigger_click,
                BoolInput::TriggerTouch => inputs.trigger_touch,
                BoolInput::ThumbstickClick => inputs.thumbstick_click,
                BoolInput::ThumbstickTouch => inputs.thumbstick_touch,
                BoolInput::TrackpadTouch => inputs.trackpad_touch,
                BoolInput::AClick => inputs.a_click,
                BoolInput::ATouch => inputs.a_touch,
                BoolInput::BClick => inputs.b_click,
                BoolInput::BTouch => inputs.b_touch,
                BoolInput::SystemClick => inputs.system_click,
                BoolInput::SqueezeClick => false,
            }),
            Input::Float(f) => ActionValue::Float(match f {
                FloatInput::Trigger => inputs.trigger,
                FloatInput::Squeeze => inputs.squeeze,
                FloatInput::SqueezeForce => inputs.squeeze_force,
                FloatInput::TrackpadForce => inputs.trackpad_force,
            }),
            Input::Vec2(v) => {
                let value = match v {
                    Vec2Input::Thumbstick => inputs.thumbstick,
                    Vec2Input::Trackpad => inputs.trackpad,
                };
                match self.axis {
                    Some(axis) => ActionValue::Float(value[axis]),
                    None => ActionValue::Vec2(value),
                }
            }
            Input::Hands(_) => return false,
        };
        value.engaged()
    }

    fn name(self) -> &'static str {
        match self.input {
            Input::Bool(b) => b.name(),
            Input::Float(f) => f.name(),
            Input::Vec2(v) => v.name(),
            Input::Hands(h) => h.name(),
        }
    }
}

/// Every pair of a semantic action and a raw action whose default bindings name the same input,
/// for the hands' profiles and a player whose dominant hand is `dominant`.
fn pairs(profiles: [InteractionProfile; 2], dominant: Hand) -> Vec<Pair> {
    let mut out = Vec::new();
    for set in SemanticSet::ALL {
        for index in 0..set.actions().len() {
            for hand in Hand::BOTH {
                let Some(profile) = layout::shipped(profiles[hand.index()]) else {
                    continue;
                };
                let Some(path) = defaults::default_path(set, index, profile, hand, dominant) else {
                    continue;
                };
                if let Some(raw) = raw_partner(profile, hand, path) {
                    out.push(Pair {
                        set,
                        index,
                        hand,
                        path,
                        raw,
                    });
                }
            }
        }
    }
    out
}

/// The raw action bound to `path` on `hand` of `profile`, or the raw two-axis input whose axis
/// `path` is. Scalars are preferred over booleans, since Touch binds its boolean trigger click to
/// the trigger value too; the self-check's own grip boolean is never a partner.
fn raw_partner(profile: Profile, hand: Hand, path: &str) -> Option<RawProbe> {
    let candidates = FloatInput::ALL
        .iter()
        .map(|&f| Input::Float(f))
        .chain(Vec2Input::ALL.iter().map(|&v| Input::Vec2(v)))
        .chain(
            BoolInput::ALL
                .iter()
                .filter(|&&b| b != BoolInput::SqueezeClick)
                .map(|&b| Input::Bool(b)),
        );
    let mut axis_parent = None;
    for input in candidates {
        let Some(bound) = layout::binding(profile, hand, input) else {
            continue;
        };
        if bound == path {
            return Some(RawProbe { input, axis: None });
        }
        if axis_parent.is_none() && matches!(input, Input::Vec2(_)) {
            for (axis, suffix) in ["/x", "/y"].into_iter().enumerate() {
                if path.strip_suffix(suffix) == Some(bound) {
                    axis_parent = Some(RawProbe {
                        input,
                        axis: Some(axis),
                    });
                }
            }
        }
    }
    axis_parent
}

/// The press test of one pair.
struct OverlapWatch {
    pair: Pair,
    raw_frames: u32,
    semantic_frames: u32,
    both_frames: u32,
    done: bool,
}

impl OverlapWatch {
    fn new(pair: Pair) -> OverlapWatch {
        OverlapWatch {
            pair,
            raw_frames: 0,
            semantic_frames: 0,
            both_frames: 0,
            done: false,
        }
    }

    /// Count one frame; at the end of the first engagement, the finding and its verdict.
    fn observe(&mut self, semantic: bool, raw: bool) -> Option<(String, Verdict)> {
        if semantic || raw {
            self.raw_frames += u32::from(raw);
            self.semantic_frames += u32::from(semantic);
            self.both_frames += u32::from(semantic && raw);
            return None;
        }
        if self.raw_frames == 0 && self.semantic_frames == 0 {
            return None;
        }
        self.done = true;
        let (verdict, kind) = match (self.raw_frames > 0, self.semantic_frames > 0) {
            (true, true) => ("both report", Verdict::Both),
            (true, false) => ("only the raw action reports", Verdict::OneSided),
            _ => ("only the semantic action reports", Verdict::OneSided),
        };
        let p = &self.pair;
        let text = format!(
            "{}.{} and hands.{} on the {} hand ({}): {verdict} (raw {} frames, semantic {}, both {})",
            p.set.name(),
            p.set.actions()[p.index].name,
            p.raw.name(),
            p.hand.label(),
            p.path,
            self.raw_frames,
            self.semantic_frames,
            self.both_frames
        );
        Some((text, kind))
    }
}

/// What a pair's press found.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Verdict {
    /// Both actions read the input: the pair passes.
    Both,
    /// Only one did: the runtime gave the input to one set.
    OneSided,
}

#[derive(Debug, Copy, Clone, Default)]
struct GripReading {
    force: f32,
    value: f32,
}

/// The press test of one hand's runtime-thresholded grip boolean.
#[derive(Debug, Copy, Clone, Default)]
struct GripWatch {
    was_pressed: bool,
    pressed_at: Option<GripReading>,
    done: bool,
}

impl GripWatch {
    /// Follow the boolean; at its first release after a press, describe the two edges.
    fn observe(&mut self, pressed: bool, reading: GripReading) -> Option<String> {
        let was = std::mem::replace(&mut self.was_pressed, pressed);
        match (was, pressed, self.pressed_at) {
            (false, true, _) => {
                self.pressed_at = Some(reading);
                None
            }
            (true, false, Some(at)) => {
                self.done = true;
                Some(format!(
                    "pressed at force {:.2} (squeeze {:.2}) and released at force {:.2} (squeeze {:.2})",
                    at.force, at.value, reading.force, reading.value
                ))
            }
            _ => None,
        }
    }
}

/// The actions whose bound sources the block lists, across active and inactive sets.
const SOURCE_SAMPLE: &[(SetId, &str)] = &[
    (SetId::Hands, "a_click"),
    (SetId::Hands, "squeeze_force"),
    (SetId::Hands, "squeeze_click"),
    (SetId::Semantic(SemanticSet::Global), "pause"),
    (SetId::Semantic(SemanticSet::OnFoot), "jump"),
    (SetId::Semantic(SemanticSet::OnFoot), "grapple"),
    (SetId::Semantic(SemanticSet::OnFoot), "turn"),
    (SetId::Semantic(SemanticSet::Wingsuit), "boost"),
    (SetId::Semantic(SemanticSet::LandVehicle), "accelerate"),
    (SetId::Semantic(SemanticSet::Menu), "menu_confirm"),
];

fn report(frame: &CheckFrame, watches: &[OverlapWatch]) -> String {
    let CheckFrame {
        session,
        actions,
        mode,
        dominant,
        semantic,
        ..
    } = *frame;
    let instance = session.instance();
    let mut lines = Vec::new();
    lines.push(match instance.properties() {
        Ok(p) => format!("runtime: {} {}", p.runtime_name, p.runtime_version),
        Err(e) => format!("runtime: unknown ({e})"),
    });
    lines.push(format!(
        "schema {SCHEMA_VERSION}, bindings suggested for a {}-handed player",
        dominant.label()
    ));
    for hand in Hand::BOTH {
        let profile = session
            .current_interaction_profile(actions.hand_path(hand))
            .map_err(|e| e.to_string())
            .and_then(|p| match p {
                xr::Path::NULL => Ok("none".to_owned()),
                p => instance.path_to_string(p).map_err(|e| e.to_string()),
            });
        lines.push(format!(
            "{} profile: {}",
            hand.label(),
            profile.unwrap_or_else(|e| format!("unreadable ({e})"))
        ));
    }
    let active = [
        SetId::Hands,
        SetId::Semantic(SemanticSet::Global),
        SetId::Semantic(SemanticSet::of(mode)),
    ];
    lines.push(format!(
        "active sets: {}",
        active.map(|s| s.name()).join(", ")
    ));

    lines.push(
        "one input in two active sets at priority 0 (expect both active on every line):".to_owned(),
    );
    for watch in watches {
        let p = &watch.pair;
        let Some(state) = semantic.set(p.set) else {
            continue;
        };
        let semantic_active = state.actions[p.index][p.hand.index()].active;
        let raw_active = raw_active(session, actions, p.raw.input, p.hand);
        lines.push(format!(
            "  {}.{} / hands.{} on the {} hand ({}): semantic {}, raw {}",
            p.set.name(),
            p.set.actions()[p.index].name,
            p.raw.name(),
            p.hand.label(),
            p.path,
            active_label(Ok(semantic_active)),
            active_label(raw_active)
        ));
    }

    lines.push(
        "grip as a boolean and as a float (expect both active where the profile has the input):"
            .to_owned(),
    );
    for hand in Hand::BOTH {
        let click = actions.bool_state(session, BoolInput::SqueezeClick, hand);
        let force = actions.float_state(session, FloatInput::SqueezeForce, hand);
        lines.push(format!(
            "  {}: squeeze_click {}, squeeze_force {}",
            hand.label(),
            match click {
                Ok(s) => format!("{} = {}", active_label(Ok(s.active)), s.value),
                Err(e) => format!("unreadable ({e:#})"),
            },
            match force {
                Ok(s) => format!("{} = {:.2}", active_label(Ok(s.active)), s.value),
                Err(e) => format!("unreadable ({e:#})"),
            },
        ));
    }

    lines.push(
        "bound sources (expect the default inputs, in inactive sets as in active ones):".to_owned(),
    );
    for &(set, name) in SOURCE_SAMPLE {
        let state = if active.contains(&set) {
            "active"
        } else {
            "inactive"
        };
        let Some((_, action)) = actions
            .set_actions(set)
            .into_iter()
            .find(|(n, _)| *n == name)
        else {
            continue;
        };
        let sources = match sources::query_action(session, action) {
            Ok(list) if list.is_empty() => "no sources".to_owned(),
            Ok(list) => list
                .iter()
                .map(|s| match &s.localized {
                    Ok(text) => format!("{} \"{text}\"", s.path),
                    Err(e) => format!("{} (no localized name: {e})", s.path),
                })
                .collect::<Vec<_>>()
                .join("; "),
            Err(e) => format!("error: {e}"),
        };
        lines.push(format!("  {}.{name} ({state}): {sources}", set.name()));
    }
    lines.push(
        "press each paired input once; each logs an `input: self-check finding` line".to_owned(),
    );
    lines.join("\n")
}

fn raw_active(
    session: &xr::Session<xr::D3D11>,
    actions: &Actions,
    input: Input,
    hand: Hand,
) -> Result<bool, String> {
    let active = match input {
        Input::Bool(b) => actions.bool_state(session, b, hand).map(|s| s.active),
        Input::Float(f) => actions.float_state(session, f, hand).map(|s| s.active),
        Input::Vec2(v) => actions.vec2_state(session, v, hand).map(|s| s.active),
        Input::Hands(_) => return Err("not a raw input".to_owned()),
    };
    active.map_err(|e| format!("{e:#}"))
}

fn active_label(active: Result<bool, String>) -> String {
    match active {
        Ok(true) => "active".to_owned(),
        Ok(false) => "INACTIVE".to_owned(),
        Err(e) => format!("unreadable ({e})"),
    }
}

fn note_finding(finding: String, problem: bool) {
    if problem {
        tracing::warn!(target: "vr", "input: self-check finding: {finding}");
    } else {
        tracing::info!(target: "vr", "input: self-check finding: {finding}");
    }
    let mut status = STATUS.lock();
    status.findings.push(finding);
    status.problems += usize::from(problem);
}

static STATUS: Mutex<SelfCheckStatus> = Mutex::new(SelfCheckStatus {
    report: None,
    findings: Vec::new(),
    problems: 0,
});

static RERUN: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr::input::semantic::vocabulary::{GlobalAction, OnFootAction};

    fn find(pairs: &[Pair], set: SemanticSet, index: usize, hand: Hand) -> Option<RawProbe> {
        pairs
            .iter()
            .find(|p| p.set == set && p.index == index && p.hand == hand)
            .map(|p| p.raw)
    }

    #[test]
    fn semantic_actions_pair_with_the_raw_action_on_their_input() {
        let index = [InteractionProfile::ValveIndex; 2];
        let pairs = pairs(index, Hand::Right);
        let jump = OnFootAction::Jump as usize;
        assert_eq!(
            find(&pairs, SemanticSet::OnFoot, jump, Hand::Right),
            Some(RawProbe {
                input: Input::Bool(BoolInput::AClick),
                axis: None
            })
        );
        assert_eq!(find(&pairs, SemanticSet::OnFoot, jump, Hand::Left), None);
        assert_eq!(
            find(
                &pairs,
                SemanticSet::OnFoot,
                OnFootAction::Turn as usize,
                Hand::Right
            ),
            Some(RawProbe {
                input: Input::Vec2(Vec2Input::Thumbstick),
                axis: Some(0)
            })
        );
        assert_eq!(
            find(
                &pairs,
                SemanticSet::OnFoot,
                OnFootAction::Grapple as usize,
                Hand::Left
            ),
            Some(RawProbe {
                input: Input::Bool(BoolInput::BClick),
                axis: None
            })
        );
    }

    #[test]
    fn touch_pairs_by_position_and_prefers_the_trigger_value() {
        let touch = [InteractionProfile::OculusTouch; 2];
        let pairs = pairs(touch, Hand::Right);
        let pause = GlobalAction::Pause as usize;
        assert_eq!(
            find(&pairs, SemanticSet::Global, pause, Hand::Left),
            Some(RawProbe {
                input: Input::Bool(BoolInput::SystemClick),
                axis: None
            })
        );
        assert_eq!(find(&pairs, SemanticSet::Global, pause, Hand::Right), None);
        assert_eq!(
            find(
                &pairs,
                SemanticSet::OnFoot,
                OnFootAction::Fire as usize,
                Hand::Right
            ),
            Some(RawProbe {
                input: Input::Float(FloatInput::Trigger),
                axis: None
            })
        );
    }

    #[test]
    fn an_overlap_watch_reports_after_the_first_release() {
        let pair = pairs([InteractionProfile::ValveIndex; 2], Hand::Right)[0];
        let mut watch = OverlapWatch::new(pair);
        assert_eq!(watch.observe(false, false), None);
        assert_eq!(watch.observe(true, true), None);
        assert_eq!(watch.observe(false, true), None);
        let (finding, verdict) = watch
            .observe(false, false)
            .expect("a finding at the release");
        assert_eq!(verdict, Verdict::Both);
        assert!(finding.contains("both report"), "{finding}");
        assert!(
            finding.contains("raw 2 frames, semantic 1, both 1"),
            "{finding}"
        );
        assert!(watch.done);
        // A pair only the raw action reads is a problem.
        let mut watch = OverlapWatch::new(pair);
        watch.observe(false, true);
        let (_, verdict) = watch.observe(false, false).expect("a finding");
        assert_eq!(verdict, Verdict::OneSided);
    }

    #[test]
    fn a_grip_watch_reports_the_runtime_threshold() {
        let mut watch = GripWatch::default();
        let at = |force| GripReading { force, value: 1.0 };
        assert_eq!(watch.observe(false, at(0.1)), None);
        assert_eq!(watch.observe(true, at(0.4)), None);
        let finding = watch.observe(false, at(0.2)).expect("a finding");
        assert!(finding.contains("pressed at force 0.40"), "{finding}");
        assert!(finding.contains("released at force 0.20"), "{finding}");
        assert!(watch.done);
    }
}
