//! The weapon wheel: the game's own selection cross, opened with a held input and steered by the
//! hand. The game's side is in `docs/engine/gameplay/weapon-selection.md`; the design is in
//! `docs/mod/weapon-selector.md`.
//!
//! The game shows the cross while a slot-select action is held and commits on the release, so the
//! wheel drives it with those actions alone, and the game's own selection, offer, and commit rules
//! stay in charge:
//!
//! - [`select`] (pure) decides when the wheel opens and closes and which arm the hand chooses, and
//!   names the one slot-select action to hold each tick;
//! - [`steer`] (pure) reads the steering hand's turn or offset from where the wheel opened;
//! - [`placement`] moves the cross on the floating panel to where the hand was when the wheel
//!   opened;
//! - this module runs on the input tick, after the binding tables and the weapon handling
//!   ([`crate::controllers::bindings::input_tick`]): it reads the game's state, steps the wheel,
//!   puts its slot-select hold in the tick's wanted actions (dropping the tables' own trackpad
//!   quadrant holds, which the wheel takes over), pulses the steering hand, and clears the game's
//!   pending selection when the wheel cancels ([`Inventory::m_SlotToSelectOnRelease`]).
//!
//! It opens with the weapon-select trackpad press (the off-hand Index trackpad, pressed) or the
//! `weapon_wheel` action (the dominant stick held up), and acts where the hands hold the hand
//! weapons ([`crate::controllers::weapons::gameplay`]). While the stick holds the wheel open, the
//! stick's turn is withheld from the body turn.

pub mod config;
mod placement;
pub mod select;
mod steer;

use std::time::{Duration, Instant};

use glam::Quat;
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    ui::hud_ui::HUDUI,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        bindings::hold::{Desired, MapKind, Target, Want},
        frame::Hand,
        hands,
        mode::Mode,
        weapons::{
            engine, gameplay,
            haptics::{self, PulseSource},
            wheel::{
                config::WheelConfig,
                select::{Close, HandPose, Wheel, WheelInput, WheelPress, WheelSlot, resting},
            },
        },
    },
};

/// The wheel's hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    placement::hook_library()
}

/// What the input tick hands the wheel.
pub(crate) struct TickInputs {
    pub mode: Mode,
    /// Each hand's opening inputs, by [`Hand::index`], as the binding tables read them.
    pub presses: [WheelPress; 2],
}

/// What the wheel hands back to the input tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct WheelTick {
    /// The stick holds the wheel open: its push must not turn the body.
    pub by_stick: bool,
}

/// One input tick of the wheel: read the game, step the wheel, and put its slot-select hold in
/// `desired`. Called while the bindings inject, outside the menus.
pub(crate) fn input_tick(inputs: &TickInputs, desired: &mut Vec<Desired>) -> WheelTick {
    let config = Config::lock_query(|c| c.controllers.weapons.wheel);
    // SAFETY: the input tick runs on the main thread outside the world update.
    let owner = unsafe { engine::local_player() };
    let acting = config.enabled && gameplay(inputs.mode) && owner.is_some();
    // SAFETY: as above.
    let game = owner.as_deref().map(|owner| unsafe { read_game(owner) });
    let active = hands::active_hands_raw();
    let input = WheelInput {
        acting,
        presses: inputs.presses,
        head: active
            .as_ref()
            .map_or(Quat::IDENTITY, |a| a.head.orientation),
        hands: Hand::BOTH.map(|hand| {
            let h = active.as_ref()?.hands[hand.index()]?;
            Some(HandPose {
                aim: h.aim,
                grip: h.grip.position,
            })
        }),
        gate: game.is_some_and(|g| g.gate),
        offered: game.map_or([false; 4], |g| g.offered),
        wielded: game.and_then(|g| g.wielded),
        resting: game.and_then(|g| resting(g.wielded, g.selected, &g.offered)),
    };

    let mut shell = SHELL.lock();
    let out = shell.wheel.step(&input, &config);
    if acting && config.trackpad {
        drop_table_selects(desired);
    }
    if let Some(arm) = out.hold {
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: arm.action(),
            },
            want: Want::Hold(1.0),
        });
    }

    if let Some((opener, hand)) = out.opened {
        shell.steering = Some(hand);
        shell.cancel_ticks = 0;
        shell.point = config
            .place_at_hand
            .then(|| {
                let active = active.as_ref()?;
                let grip = active.hands[hand.index()]?.grip.position;
                placement::panel_point(active.head.position, grip)
            })
            .flatten();
        tracing::debug!(target: "controllers", ?opener, ?hand, chosen = ?out.chosen, "weapon wheel: opened");
        pulse(hand, &config, false);
    } else if out.changed
        && let Some(hand) = out.steering
    {
        pulse(hand, &config, false);
    }
    match out.closed {
        Some(Close::Commit(arm)) => {
            tracing::debug!(target: "controllers", ?arm, "weapon wheel: committed");
            if let Some(hand) = shell.steering.take() {
                pulse(hand, &config, true);
            }
        }
        Some(Close::Cancel) => {
            tracing::debug!(target: "controllers", "weapon wheel: cancelled");
            shell.steering = None;
            shell.cancel_ticks = CANCEL_TICKS;
        }
        None => {}
    }
    if shell.wheel.is_open() {
        placement::publish(shell.point, true);
    } else if out.closed.is_some() {
        placement::publish(None, false);
    }
    publish_anchor(
        shell.wheel.is_open().then_some(shell.steering).flatten(),
        out.closed.is_some(),
    );
    service_cancel(&mut shell, owner);
    WheelTick {
        by_stick: out.by_stick,
    }
}

/// Where the HUD draws the selection cross while the wheel shows it: at the steering hand
/// ([`crate::hud::anchored`]).
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct CrossAnchor {
    /// The hand steering the wheel, or that steered it until it closed.
    pub hand: Hand,
    /// Whether the wheel is open.
    pub open: bool,
    /// Seconds since the wheel closed, while it lingers after closing.
    pub closed_s: f32,
}

/// How long the cross's anchor outlives the wheel's close: the length of the cross's hide
/// animation.
pub(crate) const CROSS_LINGER: Duration = Duration::from_millis(500);

/// The selection cross's anchor: the steering hand while the wheel is open, and for
/// [`CROSS_LINGER`] after it closes, so the cross's hide animation plays where it was.
pub(crate) fn cross_anchor() -> Option<CrossAnchor> {
    let (hand, closed_at) = (*ANCHOR.lock())?;
    let closed_s = closed_at.map_or(0.0, |at| at.elapsed().as_secs_f32());
    (closed_s <= CROSS_LINGER.as_secs_f32()).then_some(CrossAnchor {
        hand,
        open: closed_at.is_none(),
        closed_s,
    })
}

/// Close an open wheel as a cancel while a game menu is up: the menu takes the slot-select inputs
/// away, and the game would commit the pending selection when it resumes.
pub(crate) fn pause() {
    close_as_cancel();
}

/// Stop acting: close an open wheel as a cancel and withdraw its placement. Called whenever the
/// bindings do not inject.
pub(crate) fn stand_down() {
    close_as_cancel();
    placement::clear();
    *ANCHOR.lock() = None;
}

/// How many input ticks a cancel keeps clearing the game's pending selection: the release's own
/// and the next few, in case the game read a slot-select input that was still held on its next
/// update and recorded it again.
const CANCEL_TICKS: u32 = 3;

struct Shell {
    wheel: Wheel,
    /// The hand steering the open wheel.
    steering: Option<Hand>,
    /// The panel point the open wheel's cross is placed at.
    point: Option<placement::PanelPoint>,
    /// Input ticks left that clear the game's pending selection after a cancel.
    cancel_ticks: u32,
}

static SHELL: Mutex<Shell> = Mutex::new(Shell {
    wheel: Wheel::new(),
    steering: None,
    point: None,
    cancel_ticks: 0,
});

/// The game's state the wheel reads.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct GameState {
    gate: bool,
    offered: [bool; 4],
    wielded: Option<WeaponSlot>,
    selected: Option<WeaponSlot>,
}

/// Read the game's state for the wheel.
///
/// # Safety
/// `owner` must be the live local player character, on the main thread outside the world update.
unsafe fn read_game(owner: &Character) -> GameState {
    let inventory: &Inventory = &owner.m_Inventory;
    // SAFETY: forwarded from the caller.
    let gate = unsafe { inventory.GetAnimationAllowsSwitchWeaponGUI() };
    // The selection the game last pushed to its HUD carries the offer as the game decides it (the
    // planted explosive's depends on more than its slot); without a HUD, a slot that holds a
    // weapon is offered.
    // SAFETY: the HUD is a live singleton in gameplay, written only inside the world update.
    let offered = match unsafe { HUDUI::get() } {
        Some(hud) => {
            let p = &hud.m_LastWeaponSelectionParams;
            [
                p.m_DualWieldIsValid,
                p.m_TwoHandedIsValid,
                p.m_TwoHandedSpecialIsValid,
                p.m_PlantedExplosiveIsValid,
            ]
        }
        None => WheelSlot::ALL.map(|arm| {
            inventory
                .m_Weapons
                .get(arm.inventory_slot() as usize)
                .is_some_and(|w| !w.px.is_null())
        }),
    };
    // Read as the raw value: the slot can be out of the enum's range.
    // SAFETY: the character is live; the field is plain data.
    let selected = unsafe { (&raw const owner.m_SelectedWeaponSlot).cast::<i32>().read() };
    let selected = engine::slot_from_raw(selected);
    // SAFETY: forwarded from the caller.
    let wielded = unsafe { gun::held_weapon(owner) }.and(selected);
    GameState {
        gate,
        offered,
        wielded,
        selected,
    }
}

/// Drop the binding tables' slot-select holds: the trackpad quadrant presses, which the wheel
/// takes over. The holsters' slot-select clicks are left alone.
fn drop_table_selects(desired: &mut Vec<Desired>) {
    desired.retain(|d| {
        !(d.target.map == MapKind::Player
            && matches!(d.want, Want::Hold(_))
            && WheelSlot::of_action(d.target.action).is_some())
    });
}

fn pulse(hand: Hand, config: &WheelConfig, commit: bool) {
    let spec = if commit { config.commit } else { config.tick };
    haptics::pulse(hand, spec, PulseSource::WeaponWheel);
}

fn close_as_cancel() {
    let mut shell = SHELL.lock();
    if shell.wheel.cancel() {
        tracing::debug!(target: "controllers", "weapon wheel: cancelled by a mode change");
        shell.steering = None;
        shell.cancel_ticks = CANCEL_TICKS;
        placement::publish(None, false);
        publish_anchor(None, true);
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    let owner = unsafe { engine::local_player() };
    service_cancel(&mut shell, owner);
}

/// The steering hand and when the wheel closed (`None` while open), for [`cross_anchor`]. A leaf
/// slot: the input tick writes it, the render thread reads it.
static ANCHOR: Mutex<Option<(Hand, Option<Instant>)>> = Mutex::new(None);

/// Publish the open wheel's steering hand, or mark the wheel closed (`closed`) so the anchor
/// lingers at the last hand.
fn publish_anchor(open: Option<Hand>, closed: bool) {
    let mut anchor = ANCHOR.lock();
    match open {
        Some(hand) => *anchor = Some((hand, None)),
        None if closed => {
            if let Some((_, closed_at)) = anchor.as_mut()
                && closed_at.is_none()
            {
                *closed_at = Some(Instant::now());
            }
        }
        None => {}
    }
}

/// Clear the game's pending selection while a cancel asks for it.
fn service_cancel(shell: &mut Shell, owner: Option<&mut Character>) {
    if shell.cancel_ticks == 0 {
        return;
    }
    shell.cancel_ticks -= 1;
    if let Some(owner) = owner {
        owner.m_Inventory.m_SlotToSelectOnRelease = WeaponSlot::E_NONE;
    }
}

#[cfg(test)]
mod tests {
    use jc3gi::input::input_action_map::Action;

    use super::*;

    #[test]
    fn only_the_tables_slot_select_holds_are_dropped() {
        let want = |action, want| Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want,
        };
        let mut desired = vec![
            want(Action::SELECT_TWO_HANDED, Want::Hold(1.0)),
            want(Action::SELECT_DUEL_WIELD, Want::Click),
            want(Action::JUMP, Want::Hold(1.0)),
        ];
        drop_table_selects(&mut desired);
        assert_eq!(
            desired,
            vec![
                want(Action::SELECT_DUEL_WIELD, Want::Click),
                want(Action::JUMP, Want::Hold(1.0)),
            ]
        );
    }
}
