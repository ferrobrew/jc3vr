//! The detonator: the C4 weapon's own trigger model, held in a hand like a gun, whose trigger
//! detonates the placed charges.
//!
//! A hand comes to hold it in two ways: pulling the trigger of a hand holding a charge swaps the
//! charge for it (the charge goes back into the pouch), and a grab at the C4 pouch with no charge
//! left to give hands it over instead of a charge ([`Detonator::take`]). There is one detonator, so
//! a hand taking it takes it from the other. It sits in the hand by its grip, upright in the fist as
//! a pistol grip is held ([`DETONATOR_HOLD`]), and the hand closes on it as on a gun's grip. Opening
//! the hand puts it back on the chest: it is never dropped into the world.
//!
//! The trigger of the hand holding it detonates ([`Detonator::step`]): a press with charges placed
//! asks the weapon to detonate them, and a press with none placed, or while the last detonation's
//! charges are still going off, buzzes the hand and asks for nothing. The trigger press that
//! swapped a charge for the detonator is not a detonation: the trigger has to let go before the
//! detonator fires.
//!
//! The model is an instance the payload owns of the C4 weapon's base model
//! ([`WeaponData::m_BaseModelInstance`]), which the weapon's entity names as its trigger: the same
//! model the game shows in Rico's hand while the detonate act plays. The input tick shows it at the
//! hand and releases it when the hand lets go ([`place_model`]); the render frame moves it onto the
//! hand as it is drawn ([`render_frame`]), as the explosives in the hands are. The C4 pouch draws
//! the same model once no charge is left to give ([`trigger_model`]).
//!
//! [`WeaponData::m_BaseModelInstance`]: jc3gi::equipment::weapon::WeaponData::m_BaseModelInstance

use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::weapon::WeaponBase,
    graphics_engine::model_instance::{Model, ModelInstance},
    types::shared_ptr::SharedPtr,
};
use parking_lot::Mutex;

use crate::controllers::{
    bindings::controls::ButtonControl,
    frame::{Hand, Pose},
    hands::{
        held::{self, HoldPlacement},
        late,
    },
    weapons::{c4::slot_c4, engine, pouch_models::owned::OwnedModel},
};

/// The detonator, and the hand that holds it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Detonator {
    hand: Option<Hand>,
    /// Whether the trigger has let go since the detonator came into the hand, so that a press
    /// detonates.
    armed: bool,
}

/// What a tick of the detonator did.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DetonatorStep {
    /// The hand's trigger pressed: detonate.
    Detonate(Hand),
    /// The hand opened: the detonator went back on the chest.
    Returned(Hand),
}

impl Detonator {
    pub const NONE: Detonator = Detonator {
        hand: None,
        armed: false,
    };

    /// The hand that holds it.
    pub const fn hand(self) -> Option<Hand> {
        self.hand
    }

    /// Put the detonator in `hand`, whose trigger is `trigger` this tick, taking it from the other
    /// hand if that held it. A trigger still down from the press that brought it does not
    /// detonate until it has let go.
    pub fn take(&mut self, hand: Hand, trigger: ButtonControl) {
        *self = Detonator {
            hand: Some(hand),
            armed: !trigger.down,
        };
    }

    /// Put the detonator back on the chest.
    pub fn put_back(&mut self) {
        *self = Detonator::NONE;
    }

    /// The tick's step for the hand holding the detonator: `opened` and `triggers` by
    /// [`Hand::index`]. An opened hand puts it back; otherwise a trigger press after the trigger has
    /// let go detonates.
    pub fn step(
        &mut self,
        opened: [bool; 2],
        triggers: [ButtonControl; 2],
    ) -> Option<DetonatorStep> {
        let hand = self.hand?;
        if opened[hand.index()] {
            self.put_back();
            return Some(DetonatorStep::Returned(hand));
        }
        let trigger = triggers[hand.index()];
        // A release this tick comes before a press this tick: a tap after the swap's press still
        // detonates.
        if trigger.released || !trigger.held() {
            self.armed = true;
        }
        if self.armed && trigger.pressed {
            // The press holds the trigger down; the next press needs it to let go first.
            self.armed = !trigger.down;
            return Some(DetonatorStep::Detonate(hand));
        }
        None
    }
}

/// Where the detonator's model sits in the hand ([`HoldPlacement`]), derived from the trigger
/// model's bounds: an upright cylinder about 3.3 cm across and [`DETONATOR_LENGTH_M`] long,
/// standing on its origin along the model's +Y. It stands up the fist
/// as a pistol grip does, its +Y along the grip's -Z, toward the thumb, centred on the grip's
/// origin, the middle of the closed hand.
pub const DETONATOR_HOLD: HoldPlacement = HoldPlacement {
    offset_m: Vec3::new(0.0, 0.0, DETONATOR_LENGTH_M / 2.0),
    axes: [Vec3::X, Vec3::NEG_Z, Vec3::Y],
};

/// The trigger model's length along its +Y, metres.
pub const DETONATOR_LENGTH_M: f32 = 0.088;

/// Show the detonator's model in `hand`, whose grip in the world is `grip`, or with `None`, release
/// it. Released too when the local player has no C4 weapon, or the weapon no base model.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn place_model(held: Option<(Hand, Pose)>) {
    let mut model = MODEL.lock();
    let Some((hand, grip)) = held else {
        unsafe { model.release() };
        return;
    };
    ensure_cleanup();
    let local = DETONATOR_HOLD.in_hand(hand);
    let matrix = held::held_matrix(grip, local);
    let source =
        unsafe { engine::local_player() }.and_then(|owner| unsafe { owner_trigger_model(owner) });
    let shown = source.and_then(|source| unsafe { model.owned.show(source, matrix) });
    if shown.is_none() {
        unsafe { model.release() };
        return;
    }
    model.hand = Some(hand);
    // The render frame moves it to the hand as drawn; until then it sits at the tick's hand.
    unsafe { model.owned.push(matrix) };
}

/// Move the detonator's model onto the hand as it is drawn at the frame's interpolation fraction
/// `dtf`.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(crate) unsafe fn render_frame(dtf: f32) {
    let model = MODEL.lock();
    let Some(hand) = model.hand.filter(|_| model.owned.visible()) else {
        return;
    };
    let Some(drawn) = late::drawn_hands(dtf)[hand.index()] else {
        return;
    };
    let matrix = held::held_matrix(drawn.grip, DETONATOR_HOLD.in_hand(hand));
    if matrix.is_finite() {
        unsafe { model.owned.push(matrix) };
    }
}

/// Release the detonator's model.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn release_model() {
    unsafe { MODEL.lock().release() };
}

/// Whether the detonator's model is drawn.
pub fn model_shown() -> bool {
    MODEL.lock().owned.visible()
}

/// The C4 weapon's base model, its trigger, while `weapon` (the planted explosive slot's) has a
/// first gun whose base model is loaded. The detonator in the hand and on the C4 pouch are drawn
/// with it.
///
/// # Safety
///
/// `weapon` must be a live weapon of the local player, on the game thread.
pub(crate) unsafe fn trigger_model(weapon: &WeaponBase) -> Option<*const SharedPtr<Model>> {
    let data = unsafe { engine::gun_ptr(weapon, 0) }?;
    let instance: *mut ModelInstance = unsafe { data.as_ref() }.m_BaseModelInstance;
    let instance = unsafe { instance.as_ref() }?;
    let model: *const SharedPtr<Model> = &instance.m_spModel;
    (!instance.m_spModel.px.is_null()).then_some(model)
}

struct HeldModel {
    owned: OwnedModel,
    hand: Option<Hand>,
}

impl HeldModel {
    /// # Safety
    ///
    /// The game thread, before the world update.
    unsafe fn release(&mut self) {
        unsafe { self.owned.release() };
        self.hand = None;
    }
}

static MODEL: Mutex<HeldModel> = Mutex::new(HeldModel {
    owned: OwnedModel::EMPTY,
    hand: None,
});

static CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);

fn ensure_cleanup() {
    if !CLEANUP_REGISTERED.swap(true, Ordering::AcqRel) {
        crate::lifecycle::on_cleanup(|_renderer| {
            // SAFETY: the shutdown cleanups run on the game thread, outside the game's update.
            unsafe { MODEL.lock().release() };
        });
    }
}

/// The trigger model of `owner`'s C4 weapon, when the slot holds one.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread.
unsafe fn owner_trigger_model(owner: &mut Character) -> Option<*const SharedPtr<Model>> {
    let weapon = unsafe { slot_c4(owner) }?;
    let base = unsafe { &*std::ptr::from_mut(weapon).cast::<WeaponBase>() };
    unsafe { trigger_model(base) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger(down: bool, pressed: bool, released: bool) -> ButtonControl {
        ButtonControl {
            down,
            pressed,
            released,
        }
    }

    const IDLE: ButtonControl = ButtonControl {
        down: false,
        pressed: false,
        released: false,
    };

    fn on(hand: Hand, control: ButtonControl) -> [ButtonControl; 2] {
        let mut out = [IDLE; 2];
        out[hand.index()] = control;
        out
    }

    #[test]
    fn the_press_that_brought_the_detonator_does_not_detonate() {
        for hand in Hand::BOTH {
            let mut d = Detonator::NONE;
            let press = trigger(true, true, false);
            d.take(hand, press);
            assert_eq!(d.hand(), Some(hand));
            // Still held from the swap's press.
            assert_eq!(
                d.step([false; 2], on(hand, trigger(true, false, false))),
                None
            );
            // Let go, then pressed again: a detonation.
            assert_eq!(
                d.step([false; 2], on(hand, trigger(false, false, true))),
                None
            );
            assert_eq!(
                d.step([false; 2], on(hand, press)),
                Some(DetonatorStep::Detonate(hand))
            );
            // Held on, it does not detonate again.
            assert_eq!(
                d.step([false; 2], on(hand, trigger(true, false, false))),
                None
            );
        }
    }

    #[test]
    fn a_quick_tap_after_the_swap_detonates() {
        let mut d = Detonator::NONE;
        d.take(Hand::Right, trigger(true, true, false));
        // Released and pressed again between two ticks.
        assert_eq!(
            d.step([false; 2], on(Hand::Right, trigger(true, true, true))),
            Some(DetonatorStep::Detonate(Hand::Right))
        );
    }

    #[test]
    fn a_tap_that_brought_it_and_let_go_arms_it_at_once() {
        let mut d = Detonator::NONE;
        d.take(Hand::Left, trigger(false, true, true));
        assert_eq!(
            d.step([false; 2], on(Hand::Left, trigger(true, true, false))),
            Some(DetonatorStep::Detonate(Hand::Left))
        );
    }

    #[test]
    fn the_other_hands_trigger_does_nothing() {
        let mut d = Detonator::NONE;
        d.take(Hand::Right, IDLE);
        assert_eq!(
            d.step([false; 2], on(Hand::Left, trigger(true, true, false))),
            None
        );
    }

    #[test]
    fn opening_the_hand_puts_it_back() {
        let mut d = Detonator::NONE;
        d.take(Hand::Left, IDLE);
        assert_eq!(
            d.step([true, false], on(Hand::Left, trigger(true, true, false))),
            Some(DetonatorStep::Returned(Hand::Left))
        );
        assert_eq!(d.hand(), None);
        assert_eq!(d.step([true; 2], [trigger(true, true, false); 2]), None);
    }

    #[test]
    fn taking_it_into_the_other_hand_moves_it() {
        let mut d = Detonator::NONE;
        d.take(Hand::Left, IDLE);
        d.take(Hand::Right, trigger(true, true, false));
        assert_eq!(d.hand(), Some(Hand::Right));
        assert_eq!(
            d.step([true, false], on(Hand::Right, trigger(true, false, false))),
            None
        );
    }

    #[test]
    fn the_hold_stands_the_detonator_up_the_fist() {
        let right = DETONATOR_HOLD.in_hand(Hand::Right);
        // The model's +Y runs up the fist toward the thumb, the grip's -Z.
        assert!((right.orientation * Vec3::Y).abs_diff_eq(Vec3::NEG_Z, 1e-5));
        // Centred on the grip's origin along its length.
        let middle = right.position + right.orientation * (Vec3::Y * DETONATOR_LENGTH_M / 2.0);
        assert!(middle.abs_diff_eq(Vec3::ZERO, 1e-5), "{middle}");
        let left = DETONATOR_HOLD.in_hand(Hand::Left);
        assert!((left.orientation * Vec3::Y).abs_diff_eq(Vec3::NEG_Z, 1e-5));
    }
}
