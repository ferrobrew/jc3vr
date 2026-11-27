//! The weapon update hooks and the native start-using guard the physically gripped mounted weapons
//! use.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    character::character::Character,
    equipment::weapon::{ProjectileFireDirectionMode, WeaponBase, WeaponInputs, WeaponState},
    game::GameObjectUpdateContext,
    types::math::Vector3,
};
use jc3vr_body::controllers::{aim::gun, held::native::weapon_at};
use jc3vr_weapons::controllers::weapons::hold::armed;
use re_utilities::hook_library::HookLibrary;

use crate::vehicles::mounted;

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&WEAPON_PRE_SIM_BINDER)
        .with_static_binder(&WEAPON_POST_SIM_BINDER)
        .with_static_binder(&WEAPON_START_USING_DEFERRED_BINDER)
}

#[detour(address = WeaponBase::UpdatePreSim_ADDRESS)]
fn weapon_pre_sim(object: *mut c_void, context: *mut GameObjectUpdateContext) {
    let weapon = weapon_at(object.cast());
    let Some(base) = (unsafe { weapon.as_ref() }) else {
        WEAPON_PRE_SIM.get().unwrap().call(object, context);
        return;
    };
    unsafe { mounted::observe(base) };
    let Some((shooter, _)) = mounted::shooter(base) else {
        WEAPON_PRE_SIM.get().unwrap().call(object, context);
        return;
    };
    let result = unsafe {
        mounted::with_shooter(weapon, &shooter, || {
            WEAPON_PRE_SIM.get().unwrap().call(object, context)
        })
    };
    if result.is_none() {
        WEAPON_PRE_SIM.get().unwrap().call(object, context);
    }
}

#[detour(address = WeaponBase::StartUsingDeferred_ADDRESS)]
fn weapon_start_using_deferred(weapon: *mut WeaponBase, character: *mut Character) {
    let suppressed = unsafe { weapon.as_ref() }.is_some_and(mounted::suppress_start_using);
    if suppressed {
        tracing::debug!(target: "controllers", "mounted gun: the native start-using was refused");
        return;
    }
    WEAPON_START_USING_DEFERRED
        .get()
        .unwrap()
        .call(weapon, character);
}

#[detour(address = WeaponBase::UpdatePostSim_ADDRESS)]
fn weapon_post_sim(object: *mut c_void, context: *mut GameObjectUpdateContext) {
    let weapon = weapon_at(object.cast());
    let Some(base) = (unsafe { weapon.as_ref() }) else {
        WEAPON_POST_SIM.get().unwrap().call(object, context);
        return;
    };
    unsafe { mounted::observe(base) };
    let Some((shooter, physical)) = mounted::shooter(base) else {
        WEAPON_POST_SIM.get().unwrap().call(object, context);
        return;
    };
    let trigger = mounted::input(base).is_some_and(|input| input.trigger());
    let mounted_state = base.m_State == WeaponState::E_WS_MOUNTED;
    let result = unsafe {
        mounted::with_shooter(weapon, &shooter, || {
            if physical && mounted_state {
                let weapon = &mut *weapon;
                if !trigger {
                    armed::release_fire(weapon);
                }
                let target = gun::guns(weapon).next().flatten().map(|gun| {
                    let matrix = glam::Mat4::from(gun.m_WorldMatrixT1);
                    matrix.w_axis.truncate() - matrix.z_axis.truncate() * 100.0
                });
                if let (Some(target), Some(context)) =
                    (target.filter(|point| point.is_finite()), context.as_ref())
                {
                    let direction = weapon.m_ProjectileTempFireDirection;
                    weapon.m_ProjectileTempFireDirection =
                        ProjectileFireDirectionMode::E_PROJECTILE_FIRE_DIRECTION_BARREL;
                    let input = WeaponInputs {
                        m_PrimaryFireInput: f32::from(trigger),
                        m_SecondaryFireInput: 0.0,
                    };
                    let target = Vector3 {
                        data: target.to_array(),
                    };
                    weapon.UpdateFromUserPostSim_Parallel(context.m_Dt, &target, &input);
                    weapon.UpdateFromUserPostSim_Serial(context.m_Dt, &target, &input);
                    weapon.m_ProjectileTempFireDirection = direction;
                }
            }
            WEAPON_POST_SIM.get().unwrap().call(object, context)
        })
    };
    if result.is_none() {
        WEAPON_POST_SIM.get().unwrap().call(object, context);
    }
    if physical {
        // Keep the weapon registered while the hands hold it. The native update clears this flag
        // when it has no attached-effect work left, which would otherwise stop the firing path.
        let weapon = unsafe { &mut *weapon };
        if weapon.m_State == WeaponState::E_WS_MOUNTED && weapon.m_User[0].is_null() {
            weapon.m_RequiresPostSimUpdates = true;
        }
    }
}
