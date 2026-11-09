//! The native mount constraints and pickup prompt a physically gripped mounted weapon uses.

use jc3gi::{
    equipment::weapon::WeaponBase,
    game::GameObject,
    interaction::{InteractionContext, InteractionContextFlags},
    physics::{Constraint, ConstraintFlags},
};

use crate::controllers::weapons::hold::refs::{Strong, Weak};

/// One mount constraint whose motor the mounted aim drives.
pub(super) struct Motor {
    weak: Weak,
    enabled: bool,
}

impl Motor {
    /// Stop the motor driving the gun while the hands hold it.
    ///
    /// # Safety
    /// The game thread; the constraint's weak reference is live.
    pub(super) unsafe fn disable(&self) {
        let Some(alive) = self.weak.lock() else {
            return;
        };
        let Some(constraint) = (unsafe { constraint(&alive) }) else {
            return;
        };
        let constraint = unsafe { &mut *constraint };
        if constraint.m_Flags.contains(ConstraintFlags::MOTOR_ENABLED) {
            unsafe { constraint.DisableMotor() };
        }
    }

    /// Put the motor back if it was driving before the hands took the gun.
    ///
    /// # Safety
    /// The game thread; the constraint's weak reference is live.
    pub(super) unsafe fn restore(&self) {
        if !self.enabled {
            return;
        }
        let Some(alive) = self.weak.lock() else {
            return;
        };
        let Some(constraint) = (unsafe { constraint(&alive) }) else {
            return;
        };
        let constraint = unsafe { &mut *constraint };
        if !constraint.m_Flags.contains(ConstraintFlags::MOTOR_ENABLED) {
            unsafe { constraint.EnableMotor() };
        }
    }
}

/// The pickup interaction context whose prompt the game offers while the hands hold the gun.
pub(super) struct Prompt {
    weak: Weak,
    disabled: bool,
}

impl Prompt {
    /// Hide the use prompt for as long as the hands hold the gun.
    ///
    /// # Safety
    /// The game thread; the context's weak reference is live.
    pub(super) unsafe fn suppress(&mut self) {
        let Some(alive) = self.weak.lock() else {
            return;
        };
        let object = alive.object();
        let context = unsafe { &*(object as *const InteractionContext) };
        if context.m_Flags.contains(InteractionContextFlags::ENABLED) {
            unsafe { (*object).Enable(false) };
            self.disabled = true;
        }
    }

    /// Put the prompt back if this module hid it.
    ///
    /// # Safety
    /// The game thread; the context's weak reference is live.
    pub(super) unsafe fn restore(&mut self) {
        if !self.disabled {
            return;
        }
        self.disabled = false;
        let Some(alive) = self.weak.lock() else {
            return;
        };
        let object = alive.object();
        let context = unsafe { &*(object as *const InteractionContext) };
        if !context.m_Flags.contains(InteractionContextFlags::ENABLED) {
            unsafe { (*object).Enable(true) };
        }
    }
}

/// The mount constraints of a mounted weapon, deduplicated by control block. A constraint whose
/// weak reference is empty or does not resolve to a [`Constraint`] is skipped.
///
/// # Safety
/// The weapon is live on the game thread.
pub(super) unsafe fn motors(weapon: &WeaponBase) -> Vec<Motor> {
    let sources = [
        &weapon.m_ConstraintHorizontal,
        &weapon.m_ConstraintVertical[0],
        &weapon.m_ConstraintVertical[1],
    ];
    let mut seen = Vec::new();
    let mut motors = Vec::new();
    for source in sources {
        if source.pn.is_null() || seen.contains(&(source.px as usize, source.pn as usize)) {
            continue;
        }
        seen.push((source.px as usize, source.pn as usize));
        let weak = unsafe { Weak::of(source) };
        let Some(alive) = weak.lock() else {
            continue;
        };
        let Some(constraint) = (unsafe { constraint(&alive) }) else {
            continue;
        };
        let enabled = unsafe { &*constraint }
            .m_Flags
            .contains(ConstraintFlags::MOTOR_ENABLED);
        motors.push(Motor { weak, enabled });
    }
    motors
}

/// The weapon's pickup prompt, if it has one.
///
/// # Safety
/// The weapon is live on the game thread, and its interaction context lives with it.
pub(super) unsafe fn prompt(weapon: &mut WeaponBase) -> Option<Prompt> {
    let context = unsafe { weapon.GetPickupContext() };
    if context.is_null() {
        return None;
    }
    let object = context.cast::<GameObject>();
    let weak = unsafe { Weak::of(&(*object).m_WeakThis) };
    weak.lock().map(|_| Prompt {
        weak,
        disabled: false,
    })
}

/// Whether the weapon is in the middle of a deferred start/stop/equip/unequip/drop. A temporary
/// shooter must not be written while one of those will run on the next pre-simulation update.
pub(super) fn lifecycle_pending(weapon: &WeaponBase) -> bool {
    weapon.m_DeferredStartUsing
        || weapon.m_DeferredStopUsing
        || weapon.m_DeferredEquip
        || weapon.m_DeferredUnequip
        || weapon.m_DeferredDrop
}

/// The constraint behind a locked weak reference, if the object is one.
unsafe fn constraint(alive: &Strong) -> Option<*mut Constraint> {
    let object = alive.object();
    if !unsafe { (*object).IsType(Constraint::TYPE_ID()) } {
        return None;
    }
    Some(object as *mut Constraint)
}
