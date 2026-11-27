//! Scoped weapons: the plan of record's "Scopes" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`suppress`]: the game's precision aim (its camera zoom, its sniper overlay, and its hiding of
//!   the character) refused under the controller scheme;
//! - [`lens`]: the lens's geometry: the eyepiece on the sight line, the sighting eye, and the
//!   magnification;
//! - the render layer's `controllers::scope::renderer`: the D3D11 pipeline that copies the eyes'
//!   images and draws the lens;
//! - the render layer's `controllers::scope::pass`: the capture and the draw, which read the
//!   frame's lens this module plans through the view layer's [`lens_frame`].
//!
//! The lens is a magnified resample of a rendered eye image, drawn as a disc at the scope's eyepiece
//! while an eye is within [`LensTuning::max_eye_distance_m`](lens::LensTuning::max_eye_distance_m)
//! of it. The scene is not rendered a third time: the scope is at the eye and looks along almost
//! the same line, so the region around the sight line's projection, magnified, is the scope's view,
//! correct in parallax at the aim point. The sighting eye's own image has the scope's model in
//! front of that region, so by default the lens reads the other eye's image, which the scope does
//! not block (the render layer's `pass::source_eye`). A narrow-field scene render of its own is not
//! built.
//!
//! The frame runs in three steps at the end of `RenderEngine::PostDraw`, beside the world-space
//! overlay (gameplay's dispatch ticks run [`tick`] after the overlay's):
//!
//! 1. [`tick`], once per frame on the first eye, outside the engine's context lock: the scoped
//!    weapon, its sight line anchored to the holding wrist, the lens placed from the render-time
//!    wrist, and the sighting eye, handed to the pass through [`lens_frame::set_frame`];
//! 2. the pass's `capture`, every dispatch, before the overlay draws: the dispatch's eye images
//!    copied while a scoped weapon is held;
//! 3. the pass's `draw`, for each eye after the overlay: the lens, on the sighting eye only.
//!
//! The scoped weapons are identified on the input tick ([`input_tick`]) by their class, the sniper
//! class, which is what the game gives its scoped rifles: the gun in either hand, the wielded weapon
//! or one armed beside it. There is one lens, so with a scoped gun in each hand the lens is the one
//! raised to the eye ([`chosen`]): a scope an eye is at, then the one whose eyepiece is nearest an
//! eye.

pub mod lens;
pub mod suppress;

use glam::{Mat4, Vec3};
use jc3gi::equipment::weapon::{WeaponBase, WeaponClass};
use jc3vr_body::{
    controllers::{
        aim,
        aim::{AimStatus, HandSource, gun::HeldWeapon, sights::SightSource},
        hands::late,
        scheme,
    },
    tuning::Millis,
};
use jc3vr_core::published::{Published, Stage, Window};
use jc3vr_input::frame::{Hand, Pose};
use jc3vr_view::controllers::lens_frame::{self, LensFrame};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use serde::Deserialize;

use crate::controllers::weapons::{
    handling_tuning::HANDLING,
    scope::lens::{Lens, LensTuning, SightAnchor},
};

/// The scope's tuning: the `scope` section of the handling tuning ([`HANDLING`]).
#[derive(Debug, Copy, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeTuning {
    /// How long an input tick's identification of the scoped weapon stands. The input tick runs at
    /// the simulation rate while the bindings inject, so an older one belongs to a menu or the
    /// gamepad scheme.
    pub(crate) scoped_ms: Millis,
    pub lens: LensTuning,
}

/// The scope hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    suppress::hook_library()
}

/// Why no lens is drawn.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LensIdle {
    /// The controllers or the lens are off.
    Disabled,
    /// The gamepad scheme is in effect, outside gameplay, or under the clean view.
    Inactive,
    /// No gun in the hands is a scoped one.
    NoScope,
    /// The scoped weapon's sight line is not aimed by a tracked hand.
    NoSightLine,
    /// No eye is at the eyepiece.
    NotAtEye,
}

impl LensIdle {
    pub fn label(self) -> &'static str {
        match self {
            LensIdle::Disabled => "off",
            LensIdle::Inactive => "gamepad scheme, outside gameplay, or clean view",
            LensIdle::NoScope => "no scoped weapon",
            LensIdle::NoSightLine => "no sight line",
            LensIdle::NotAtEye => "no eye at the eyepiece",
        }
    }
}

/// The scoped weapon as the lens sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ScopeReadout {
    /// The weapon type's name hash.
    pub hash: u32,
    /// The hand that holds it.
    pub hand: Hand,
    /// Where its sight correction comes from; on the fire point the sight line runs along the
    /// barrel, below the scope.
    pub sight: SightSource,
}

/// The scopes' readout for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScopeStatus {
    /// The scoped weapon, when one is held and aimed.
    pub scope: Option<ScopeReadout>,
    /// The nearest eye's distance from the eyepiece, metres.
    pub eyepiece_distance_m: Option<f32>,
    /// Why no lens is drawn, when none is.
    pub idle: Option<LensIdle>,
    /// The eye that sights through the lens and the eye whose image it reads.
    pub eyes: Option<(usize, usize)>,
    /// Frames the lens was drawn in, since the payload loaded.
    pub frames: u64,
    /// The last failure to build the pipeline, copy an eye, or draw.
    pub error: Option<String>,
}

/// The latest readout: the frame's plan, with what the render layer's scope pass drew.
pub fn status() -> ScopeStatus {
    let drawn = lens_frame::draw_status();
    ScopeStatus {
        eyes: drawn.eyes,
        frames: drawn.frames,
        error: drawn.error,
        ..STATUS.lock().clone()
    }
}

/// Identify the gun in each hand, by [`Hand::index`], as scoped or not. Called in the game stage,
/// on the game thread before the world update, while the bindings inject.
pub(crate) fn input_tick(guns: [Option<usize>; 2]) {
    // SAFETY: each address is a local player's weapon in a hand, read on the game thread before
    // the world update; the class is read as its raw value.
    let scoped = guns.map(|gun| {
        gun.filter(|&address| {
            unsafe { (address as *const WeaponBase).as_ref() }.is_some_and(|w| {
                let class = unsafe { (&raw const w.m_Class).cast::<i32>().read() };
                class == WeaponClass::E_WEAPON_CLASS_SNIPER as i32
            })
        })
    });
    SCOPED.publish(scoped);
}

/// Whether the weapon at `address` is a scoped weapon in a hand, as the last input tick identified.
pub(crate) fn is_scoped(address: usize) -> bool {
    SCOPED
        .latest()
        .is_some_and(|scoped| scoped.value.contains(&Some(address)))
}

/// Plan the frame's lens. Called once per render dispatch at the end of `RenderEngine::PostDraw`,
/// before the engine's context lock is taken; it acts only on the frame's first eye.
pub fn tick(eye: usize) {
    if !jc3vr_view::stereo::dispatch::first_ui_eye(eye) {
        return;
    }
    let enabled = scheme::enabled();
    let mut state = STATE.lock();
    let tuning = &HANDLING.get().scope;
    let plan = plan(enabled, tuning, &mut state.anchors);
    let mut status = STATUS.lock();
    status.scope = plan.scope;
    status.eyepiece_distance_m = plan.eyepiece_distance_m;
    status.idle = plan.idle;
    // A plan is idle exactly when no eye sights, which is when the pass clears the drawn eyes.
    lens_frame::set_frame(LensFrame {
        armed: plan.armed,
        sighting: plan.sighting,
        lens: plan.lens.map(|lens| lens::draw_of(&lens, &tuning.lens)),
    });
}

/// The frame's lens plan.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Plan {
    /// Whether a scoped weapon is held and aimed, so the eyes' images are copied.
    armed: bool,
    /// The lens, while armed.
    lens: Option<Lens>,
    /// The eye at the eyepiece.
    sighting: Option<usize>,
    scope: Option<ScopeReadout>,
    eyepiece_distance_m: Option<f32>,
    idle: Option<LensIdle>,
}

struct State {
    /// Each hand's scoped gun's sight line in its wrist's frame, and the aim update it was taken
    /// from, by [`Hand::index`].
    anchors: [Option<(u64, SightAnchor)>; 2],
}

static STATE: Mutex<State> = Mutex::new(State { anchors: [None; 2] });

static STATUS: Mutex<ScopeStatus> = Mutex::new(ScopeStatus {
    scope: None,
    eyepiece_distance_m: None,
    idle: None,
    eyes: None,
    frames: 0,
    error: None,
});

/// The gun in each hand the last input tick found scoped, by [`Hand::index`].
static SCOPED: Published<[Option<usize>; 2]> = Published::new(Stage::InputGame);

/// Plan the frame's lens, re-anchoring each scoped gun's sight line to its wrist when the aim has
/// run since its anchor was taken, and taking the lens of the scope raised to the eye.
fn plan(
    enabled: bool,
    config: &ScopeTuning,
    anchors: &mut [Option<(u64, SightAnchor)>; 2],
) -> Plan {
    if !enabled {
        return idle(LensIdle::Disabled);
    }
    if !scheme::is_controllers()
        || !jc3vr_core::hooks::in_gameplay()
        || jc3vr_core::clean_view::hides_everything()
    {
        return idle(LensIdle::Inactive);
    }
    let scoped = SCOPED
        .fresh(Window::new(config.scoped_ms.duration()))
        .unwrap_or([None; 2]);
    let aim = aim::status();
    let frame = FrameInputs {
        aim: &aim,
        wrists: late::drawn_hands(jc3vr_view::hooks::camera::last_dtf())
            .map(|h| h.map(|h| h.wrist)),
        eyes: eye_positions(),
        config: &config.lens,
    };
    let plans = Hand::BOTH.map(|hand| {
        let planned = scoped[hand.index()]
            .and_then(|address| scoped_weapon(&aim, hand, address))
            .map(|weapon| plan_hand(hand, &weapon, &frame, &mut anchors[hand.index()]));
        if planned.is_none() {
            anchors[hand.index()] = None;
        }
        planned
    });
    chosen(&plans)
        .and_then(|hand| plans[hand.index()])
        .unwrap_or_else(|| idle(LensIdle::NoScope))
}

/// A plan that draws nothing, for `idle`.
fn idle(idle: LensIdle) -> Plan {
    Plan {
        idle: Some(idle),
        ..Plan::default()
    }
}

/// What every hand's lens is planned from in a frame.
struct FrameInputs<'a> {
    aim: &'a AimStatus,
    /// Each hand's wrist as drawn this frame, by [`Hand::index`].
    wrists: [Option<Pose>; 2],
    /// Both eyes' world positions.
    eyes: [Option<Vec3>; 2],
    config: &'a LensTuning,
}

/// The aim's view of the scoped gun at `address` in `hand`: the wielded weapon when its first gun
/// rides that hand, or the weapon armed there.
fn scoped_weapon(aim: &AimStatus, hand: Hand, address: usize) -> Option<HeldWeapon> {
    let wielded = aim
        .weapon
        .filter(|w| w.address == address && w.gun_hands.iter().flatten().next() == Some(&hand));
    wielded.or(aim.armed[hand.index()].filter(|w| w.address == address))
}

/// The lens of the scoped `weapon` in `hand`, re-anchoring its sight line in `anchor`.
fn plan_hand(
    hand: Hand,
    weapon: &HeldWeapon,
    frame: &FrameInputs,
    anchor: &mut Option<(u64, SightAnchor)>,
) -> Plan {
    let scope = Some(ScopeReadout {
        hash: weapon.hash,
        hand,
        sight: weapon.sight.source,
    });
    let no_sight_line = Plan {
        scope,
        ..idle(LensIdle::NoSightLine)
    };
    let Some(wrist) = frame.wrists[hand.index()] else {
        return no_sight_line;
    };
    let aim = frame.aim;
    let hand_aim = aim.hands[hand.index()].filter(|h| matches!(h.source, HandSource::Gun { .. }));
    if let Some(h) = hand_aim
        && anchor.is_none_or(|(updates, a)| updates != aim.updates || a.hand != hand)
    {
        let focus = h.ray.origin.distance(h.target);
        *anchor = Some((
            aim.updates,
            SightAnchor::new(hand, &h.ray, wrist, focus, frame.config),
        ));
    }
    let Some((_, a)) = anchor.filter(|(_, a)| a.hand == hand) else {
        return no_sight_line;
    };
    let Some(sight) = a.place(wrist) else {
        return no_sight_line;
    };
    let lens = Lens::new(&sight, a.focus_m, frame.config);
    let eyepiece_distance_m = lens::nearest_eye(lens.eyepiece, frame.eyes).map(|(_, d)| d);
    let sighting = lens::sighting_eye(&lens, frame.eyes, frame.config.max_eye_distance_m);
    Plan {
        armed: true,
        lens: Some(lens),
        sighting,
        scope,
        eyepiece_distance_m,
        idle: sighting.is_none().then_some(LensIdle::NotAtEye),
    }
}

/// The hand whose scope has the lens, of each hand's plan by [`Hand::index`]: the scope raised to
/// the eye. A scope an eye is at wins over one placed away from the eyes, which wins over one
/// without a sight line, and between two alike the one whose eyepiece is nearest an eye wins.
fn chosen(plans: &[Option<Plan>; 2]) -> Option<Hand> {
    let rank = |p: &Plan| {
        (
            p.sighting.is_some(),
            p.lens.is_some(),
            -p.eyepiece_distance_m.unwrap_or(f32::INFINITY),
        )
    };
    Hand::BOTH
        .into_iter()
        .filter_map(|hand| Some((hand, plans[hand.index()]?)))
        .max_by(|(_, a), (_, b)| {
            let (a, b) = (rank(a), rank(b));
            a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2))
        })
        .map(|(hand, _)| hand)
}

/// Both eyes' world positions this frame: the centre camera moved by each eye's offset. `None`
/// for an eye without a VR frame's render parameters.
fn eye_positions() -> [Option<Vec3>; 2] {
    let Some((head, orientation)) = jc3vr_view::hud::render_camera_pose() else {
        return [None; 2];
    };
    let center = Mat4::from_rotation_translation(orientation, head);
    [0, 1]
        .map(|eye| jc3vr_view::vr::render_params(eye).map(|p| p.resolve(center).w_axis.truncate()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_eye(distance: f32, sighting: bool) -> Plan {
        Plan {
            armed: true,
            lens: Some(Lens::new(
                &aim::ray::Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap(),
                100.0,
                &HANDLING.get().scope.lens,
            )),
            sighting: sighting.then_some(0),
            eyepiece_distance_m: Some(distance),
            ..Plan::default()
        }
    }

    #[test]
    fn the_lens_is_the_scope_raised_to_the_eye() {
        let (left, right) = (Hand::Left.index(), Hand::Right.index());
        let mut plans = [None; 2];
        assert_eq!(chosen(&plans), None);
        // A lone scope has the lens, even away from the eyes.
        plans[right] = Some(at_eye(0.4, false));
        assert_eq!(chosen(&plans), Some(Hand::Right));
        // A scope an eye is at wins over one nearer an eye but not at it.
        plans[left] = Some(at_eye(0.05, true));
        plans[right] = Some(at_eye(0.02, false));
        assert_eq!(chosen(&plans), Some(Hand::Left));
        // Two at the eyes: the nearer eyepiece.
        plans[right] = Some(at_eye(0.02, true));
        assert_eq!(chosen(&plans), Some(Hand::Right));
        // A scope without a sight line loses to one placed.
        plans[right] = Some(idle(LensIdle::NoSightLine));
        plans[left] = Some(at_eye(0.5, false));
        assert_eq!(chosen(&plans), Some(Hand::Left));
    }
}
