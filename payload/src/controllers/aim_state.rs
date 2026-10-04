//! The aim-state pin and the mental-state cuts: the plan of record's "The aim-state pin" and "The
//! mental state" (`docs/mod/input/controllers-and-roomscale.md`), with the engine side in
//! `docs/engine/gameplay/aim-state.md`.
//!
//! The game switches the player's body into an aim family whenever a weapon or the grapple is
//! aimed, and the combat mental state draws and holsters weapons on its own. Under the controller
//! scheme the body stays continuous instead:
//!
//! - The weapon pin sets [`StateBitFlag::SB_NO_WEAPON_AIM`] for the duration of the player's
//!   aim-input update, the only code that reads the bit, and restores it afterwards. The update
//!   then leaves [`AimState::m_AimingWeapon`] clear and mirrors "not aiming" into
//!   [`StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL`], while the trigger still fires: fire gating reads
//!   neither.
//! - The grapple pin clears [`AimState::m_AimingGrapple`] after the grapple's pre-simulation update,
//!   which the weapon pin does not reach.
//! - The player's arm and upper-body aim IK tasks are skipped, because the controllers own the arms.
//! - The body's aim point (the control-parameters copy, which turns the body toward it) is held
//!   neutral ahead of the body. The character-level weapon point follows the dominant hand's aim
//!   point while the gun aim acts ([`crate::controllers::aim`], which also hands each gun its own
//!   hand's point at the per-weapon update), and is otherwise left as the game computes it (gaze
//!   aiming).
//! - The wall-penetration block, computed from where the animation would put the barrel, is
//!   cleared for the player's held weapon.
//! - The mental-state cuts block the automatic draw on entering combat and swallow both automatic
//!   holsters, letting a deliberate holster through.
//!
//! Every part has its own toggle in [`AimStateConfig`] and acts only under the controller scheme
//! (or with [`AimStateConfig::without_controllers`]), so the game behaves as before otherwise.

use std::{
    cell::Cell,
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    animation::{
        arm_aim_ik::{
            NAimIKTask_Update_ADDRESS, NGrapplingFireArmIK_Update_ADDRESS, ReelInAimIK,
            ReelInAimIKInstanceProperties, RightArmAimIK, RightArmAimIKInstanceProperties,
        },
        symbol_table::EventIdSymbolTable,
    },
    character::{
        avatar::MentalState,
        character::{AimState, Character},
        control_parameters::ControlParameters,
        inventory::Inventory,
    },
    equipment::weapon::{WallPenetrationWeaponComponent, WeaponData},
    state::{StateBitFlag, StateContext},
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        aim,
        config::AimStateConfig,
        hands,
        mode::{self, Mode},
        scheme, weapons,
    },
    headpose,
    hooks::{character::resolve_act_id, input::locomotion::character_from_context},
};

/// The aim-state hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&HANDLE_AIMING_INPUT_PLAYER_BINDER)
        .with_static_binder(&PRE_SIM_UPDATE_GRAPPLE_BINDER)
        .with_static_binder(&AIM_IK_TASK_UPDATE_BINDER)
        .with_static_binder(&RIGHT_ARM_AIM_IK_UPDATE_BINDER)
        .with_static_binder(&REEL_IN_AIM_IK_UPDATE_BINDER)
        .with_static_binder(&GRAPPLING_FIRE_ARM_IK_UPDATE_BINDER)
        .with_static_binder(&UPDATE_WEAPON_AIMING_BINDER)
        .with_static_binder(&WALL_PENETRATION_UPDATE_BINDER)
        .with_static_binder(&UPDATE_WEAPON_HANDLING_BINDER)
        .with_static_binder(&HOLSTER_WEAPON_BINDER)
}

/// What the aim-state hooks did most recently, for the debug UI. The counters run from injection.
#[derive(Debug, Copy, Clone, Default)]
pub struct AimStateStatus {
    /// The weapon pin, as of the player's last aim-input update.
    pub pin: PinReadout,
    /// Whether the grapple pin cleared the grapple aim on the last pre-simulation grapple update.
    pub grapple_pin_active: bool,
    /// How often the grapple pin found the grapple aim set and cleared it.
    pub grapple_cleared: u64,
    /// How often each aim IK task was skipped for the player, in [`IkTask::ALL`] order.
    pub ik_skipped: [u64; IkTask::ALL.len()],
    /// The neutral aim point last written, world space, or `None` while the hold is inactive.
    pub neutral_point: Option<Vec3>,
    /// How often the wall-penetration block was cleared while the game had it set.
    pub wall_blocks_cleared: u64,
    /// The mental-state cuts.
    pub mental: MentalReadout,
}

/// The weapon pin's readout.
#[derive(Debug, Copy, Clone, Default)]
pub struct PinReadout {
    /// Whether the pin was applied on the last update.
    pub active: bool,
    /// The bits of [`Character::m_AimFlags`] on entry to and exit from the last update.
    pub flags_before: u8,
    pub flags_after: u8,
    /// [`StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL`] after the last update.
    pub aiming_bit_after: bool,
    /// Whether the aim timer was positive after the last update: the game wanted to aim, and the
    /// pin held it clear.
    pub timer_positive: bool,
    /// Updates the pin was applied to, and of those, how many the game wanted to aim.
    pub pinned_updates: u64,
    pub held_against_aim: u64,
}

/// The mental-state cuts' readout.
#[derive(Debug, Copy, Clone, Default)]
pub struct MentalReadout {
    /// The player's mental state at the last weapon-handling update, as the engine's raw value.
    pub mental_state: Option<i32>,
    /// Whether the combat draw was blocked on the last weapon-handling update.
    pub draw_blocked: bool,
    /// Automatic holsters swallowed: `ACT_ON_UNEQUIP_WEAPON_SLOW` from the mental state's decay to
    /// relaxed, and `UB_ACT_HOLSTER` from the weapon handling's relaxed branch.
    pub unequip_slow_swallowed: u64,
    pub relaxed_holster_swallowed: u64,
    /// Deliberate holsters let through the relaxed branch.
    pub deliberate_holsters_passed: u64,
    /// Whether a deliberate holster is waiting for its act.
    pub deliberate_pending: bool,
}

/// The aim IK tasks the pin skips for the player.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum IkTask {
    /// `NAimIKTask`: the upper body toward the aim target.
    Aim,
    /// `NRightArmAimIK`: the gun arm, hand, and head toward the aim point.
    RightArm,
    /// `NReelInAimIK`: the arm and chest during a grapple reel-in.
    ReelIn,
    /// `NGrapplingFireArmIK`: the left hand toward the hook on a grapple shot.
    GrappleFire,
}

impl IkTask {
    pub const ALL: [IkTask; 4] = [
        IkTask::Aim,
        IkTask::RightArm,
        IkTask::ReelIn,
        IkTask::GrappleFire,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            IkTask::Aim => "NAimIKTask",
            IkTask::RightArm => "NRightArmAimIK",
            IkTask::ReelIn => "NReelInAimIK",
            IkTask::GrappleFire => "NGrapplingFireArmIK",
        }
    }
}

/// The latest readout.
pub fn status() -> AimStateStatus {
    *STATUS.lock()
}

/// Whether a part of the aim-state work acts now: its toggle is on, and the controller scheme is in
/// effect or the parts are set to act without it.
pub fn part_active(toggle: bool, without_controllers: bool, controllers: bool) -> bool {
    toggle && (without_controllers || controllers)
}

/// A state bit's position in the state words: the word index and the mask within it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct StateBit {
    pub word: usize,
    pub mask: u64,
}

impl StateBit {
    pub const fn of(flag: StateBitFlag) -> Self {
        let index = flag as u32;
        Self {
            word: (index / 64) as usize,
            mask: 1 << (index % 64),
        }
    }
}

/// The neutral aim point: `distance` ahead of `head` along the body's horizontal forward. A
/// forward with no horizontal component (the body pitched straight up or down, which the on-foot
/// body never is) is used as it is.
pub fn neutral_aim_point(head: Vec3, body_forward: Vec3, distance: f32) -> Vec3 {
    let flat = Vec3::new(body_forward.x, 0.0, body_forward.z);
    let forward = flat.try_normalize().unwrap_or(body_forward);
    head + forward * distance
}

/// The two automatic-holster acts the mental-state cuts watch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WatchedAct {
    /// `ACT_ON_UNEQUIP_WEAPON_SLOW`, queued only by the mental state's decay to relaxed.
    UnequipSlow,
    /// `UB_ACT_HOLSTER`, queued by the weapon handling's relaxed branch, which is also how the
    /// player's deliberate holster takes effect.
    Holster,
}

/// What the `QueueAct` detour does with a watched act for the local player.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActVerdict {
    Pass,
    /// Passed as a deliberate holster.
    PassDeliberate,
    Swallow,
}

/// Decide a watched act. `in_weapon_handling` is whether the act is being queued from inside the
/// player's weapon-handling update with the cuts active, and `deliberate` whether a deliberate
/// holster is pending. A `UB_ACT_HOLSTER` from anywhere else (the unequip state task, a script) is
/// not the relaxed branch and passes.
pub fn act_verdict(act: WatchedAct, in_weapon_handling: bool, deliberate: bool) -> ActVerdict {
    match act {
        WatchedAct::UnequipSlow => ActVerdict::Swallow,
        WatchedAct::Holster if !in_weapon_handling => ActVerdict::Pass,
        WatchedAct::Holster if deliberate => ActVerdict::PassDeliberate,
        WatchedAct::Holster => ActVerdict::Swallow,
    }
}

/// Whether a pending deliberate holster stays pending at the start of a weapon-handling update.
/// The player's holster forces the relaxed state and waits for the relaxed branch to queue the
/// act, so it lapses once a draw or holster plays (the act took, or a draw replaced it) or the
/// mental state leaves relaxed (weapon input brought combat back, and the game would not holster
/// either).
pub fn deliberate_holster_holds(relaxed: bool, drawing_or_holstering: bool) -> bool {
    relaxed && !drawing_or_holstering
}

/// The `QueueAct` filter for the local player's acts, called by the detour in
/// [`crate::hooks::character`]. Returns whether to swallow the act.
pub(crate) fn swallow_local_act(id: i32) -> bool {
    let in_weapon_handling = IN_LOCAL_WEAPON_HANDLING.with(Cell::get);
    // The weapon-handling scope is set only while the cuts act. Outside it, nothing is swallowed
    // unless the cuts act, so that check comes before resolving the act ids through the engine's
    // symbol table.
    if !in_weapon_handling {
        let cfg = aim_state_config();
        if !part_active(
            cfg.mental_state_cuts,
            cfg.without_controllers,
            controllers(),
        ) {
            return false;
        }
    }
    let Some(act) = watched_act(id) else {
        return false;
    };
    // A holster inside the scope needs no second gate; the slow unequip does.
    if in_weapon_handling && act == WatchedAct::UnequipSlow {
        let cfg = aim_state_config();
        if !part_active(
            cfg.mental_state_cuts,
            cfg.without_controllers,
            controllers(),
        ) {
            return false;
        }
    }
    let verdict = act_verdict(
        act,
        in_weapon_handling,
        DELIBERATE_HOLSTER.load(Ordering::Relaxed),
    );
    let mut status = STATUS.lock();
    match (verdict, act) {
        (ActVerdict::Swallow, WatchedAct::UnequipSlow) => {
            status.mental.unequip_slow_swallowed += 1;
        }
        (ActVerdict::Swallow, WatchedAct::Holster) => {
            status.mental.relaxed_holster_swallowed += 1;
        }
        (ActVerdict::PassDeliberate, _) => status.mental.deliberate_holsters_passed += 1,
        (ActVerdict::Pass, _) => {}
    }
    verdict == ActVerdict::Swallow
}

static STATUS: Mutex<AimStateStatus> = Mutex::new(AimStateStatus {
    pin: PinReadout {
        active: false,
        flags_before: 0,
        flags_after: 0,
        aiming_bit_after: false,
        timer_positive: false,
        pinned_updates: 0,
        held_against_aim: 0,
    },
    grapple_pin_active: false,
    grapple_cleared: 0,
    ik_skipped: [0; IkTask::ALL.len()],
    neutral_point: None,
    wall_blocks_cleared: 0,
    mental: MentalReadout {
        mental_state: None,
        draw_blocked: false,
        unequip_slow_swallowed: 0,
        relaxed_holster_swallowed: 0,
        deliberate_holsters_passed: 0,
        deliberate_pending: false,
    },
});

/// Set by the `HolsterWeapon` detour for the local player, and cleared by the weapon-handling
/// update once the holster has taken or lapsed (see [`deliberate_holster_holds`]).
static DELIBERATE_HOLSTER: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Whether this thread is inside the local player's weapon-handling update with the cuts
    /// active. The update runs on the character workers, so the scope is per thread.
    static IN_LOCAL_WEAPON_HANDLING: Cell<bool> = const { Cell::new(false) };
}

fn aim_state_config() -> AimStateConfig {
    Config::lock_query(|c| c.controllers.aim_state)
}

fn controllers() -> bool {
    scheme::is_controllers()
}

/// Whether the part selected by `toggle` acts now.
fn active(toggle: impl Fn(&AimStateConfig) -> bool) -> bool {
    let cfg = aim_state_config();
    part_active(toggle(&cfg), cfg.without_controllers, controllers())
}

/// The local player character behind `character`, or `None` for any other character.
unsafe fn local(character: *mut Character) -> Option<&'static mut Character> {
    unsafe { character.as_mut() }.filter(|c| c.m_IsLocalCharacter)
}

/// A character's state words, accessed through a raw pointer because the game writes them inside
/// the hooked calls.
#[derive(Copy, Clone)]
struct StateWords(*mut u64);

impl StateWords {
    /// The words of `character`, or `None` before its control parameters exist.
    unsafe fn of(character: &Character) -> Option<Self> {
        let control: *mut ControlParameters = character.m_ControlParameters;
        if control.is_null() {
            return None;
        }
        // The three bit-carrying words are adjacent `u64` fields, starting with the first.
        Some(Self(unsafe {
            &raw mut (*control).m_UserVM.m_CurrentStateBitFlags0
        }))
    }

    unsafe fn word(self, bit: StateBit) -> *mut u64 {
        unsafe { self.0.add(bit.word) }
    }

    unsafe fn has(self, flag: StateBitFlag) -> bool {
        let bit = StateBit::of(flag);
        unsafe { self.word(bit).read() & bit.mask != 0 }
    }

    /// Set `flag` and return its previous value.
    unsafe fn set(self, flag: StateBitFlag, on: bool) -> bool {
        let bit = StateBit::of(flag);
        unsafe {
            let word = self.word(bit);
            let value = word.read();
            word.write(if on {
                value | bit.mask
            } else {
                value & !bit.mask
            });
            value & bit.mask != 0
        }
    }
}

/// The player's aim-input update, with [`StateBitFlag::SB_NO_WEAPON_AIM`] set around the call.
/// Nothing else reads the bit, so setting it for the call alone is invisible elsewhere, and
/// restoring the previous value leaves the graph's own scope tasks' bookkeeping intact.
#[detour(address = jc3gi::character::character::Character::HandleAimingInputPlayer_ADDRESS)]
fn handle_aiming_input_player(character: *mut Character, dt: f32) {
    let original = || {
        HANDLE_AIMING_INPUT_PLAYER
            .get()
            .unwrap()
            .call(character, dt)
    };
    let Some(c) = (unsafe { local(character) }) else {
        return original();
    };
    let words = unsafe { StateWords::of(c) };
    let Some(words) = words.filter(|_| active(|cfg| cfg.pin)) else {
        STATUS.lock().pin.active = false;
        return original();
    };
    let flags_before = c.m_AimFlags.bits();
    let saved = unsafe { words.set(StateBitFlag::SB_NO_WEAPON_AIM, true) };
    original();
    unsafe { words.set(StateBitFlag::SB_NO_WEAPON_AIM, saved) };

    // Re-borrowed after the call, which wrote the flags and the timer.
    let Some(c) = (unsafe { character.as_ref() }) else {
        return;
    };
    let timer_positive = c.m_AimTimer > 0.0;
    let mut status = STATUS.lock();
    let pin = &mut status.pin;
    pin.active = true;
    pin.flags_before = flags_before;
    pin.flags_after = c.m_AimFlags.bits();
    pin.aiming_bit_after = unsafe { words.has(StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL) };
    pin.timer_positive = timer_positive;
    pin.pinned_updates += 1;
    pin.held_against_aim += u64::from(timer_positive);
}

/// The grapple's pre-simulation update, with the grapple aim cleared after it. The serial state
/// pass reads the flag later in the tick, and the next aim-input update mirrors the clear flag into
/// [`StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL`].
#[detour(address = jc3gi::character::character::Character::PreSimUpdateGrapple_ADDRESS)]
fn pre_sim_update_grapple(character: *mut Character, dt: f32) {
    PRE_SIM_UPDATE_GRAPPLE.get().unwrap().call(character, dt);
    let Some(c) = (unsafe { local(character) }) else {
        return;
    };
    let pinned = active(|cfg| cfg.pin_grapple);
    let was_aiming = c.m_AimFlags.contains(AimState::m_AimingGrapple);
    if pinned {
        c.m_AimFlags.remove(AimState::m_AimingGrapple);
    }
    let mut status = STATUS.lock();
    status.grapple_pin_active = pinned;
    status.grapple_cleared += u64::from(pinned && was_aiming);
}

/// Whether to skip an aim IK task's update: the task runs for the local player and the skip acts.
/// Skipping `Update` alone keeps the tasks' enter and exit bookkeeping.
fn skip_ik_task(ctx: *mut StateContext, task: IkTask) -> bool {
    let local = unsafe { character_from_context(ctx) }.is_some_and(|c| c.m_IsLocalCharacter);
    if !local || !active(|cfg| cfg.skip_arm_ik) {
        return false;
    }
    if !ik_task_owned(task, hands::ik_owned()) {
        return false;
    }
    let index = IkTask::ALL.iter().position(|t| *t == task).unwrap_or(0);
    STATUS.lock().ik_skipped[index] += 1;
    true
}

fn ik_task_owned(task: IkTask, [left, right]: [bool; 2]) -> bool {
    match task {
        IkTask::Aim | IkTask::ReelIn => left && right,
        IkTask::RightArm => right,
        IkTask::GrappleFire => left,
    }
}

#[cfg(test)]
mod ik_ownership_tests {
    use super::*;

    #[test]
    fn native_tasks_remain_when_their_hands_are_not_owned() {
        for task in IkTask::ALL {
            assert!(!ik_task_owned(task, [false; 2]));
            assert!(ik_task_owned(task, [true; 2]));
        }
        assert!(ik_task_owned(IkTask::RightArm, [false, true]));
        assert!(!ik_task_owned(IkTask::RightArm, [true, false]));
        assert!(ik_task_owned(IkTask::GrappleFire, [true, false]));
        assert!(!ik_task_owned(IkTask::Aim, [true, false]));
        assert!(!ik_task_owned(IkTask::ReelIn, [false, true]));
    }
}

#[detour(address = NAimIKTask_Update_ADDRESS)]
fn aim_ik_task_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    if !skip_ik_task(ctx, IkTask::Aim) {
        AIM_IK_TASK_UPDATE.get().unwrap().call(ctx, p1, p2);
    }
}

#[detour(address = RightArmAimIK::Update_ADDRESS)]
fn right_arm_aim_ik_update(
    ctx: *mut StateContext,
    p1: *mut c_void,
    props: *mut RightArmAimIKInstanceProperties,
) {
    if !skip_ik_task(ctx, IkTask::RightArm) {
        RIGHT_ARM_AIM_IK_UPDATE.get().unwrap().call(ctx, p1, props);
    }
}

#[detour(address = ReelInAimIK::Update_ADDRESS)]
fn reel_in_aim_ik_update(
    ctx: *mut StateContext,
    p1: *mut c_void,
    props: *mut ReelInAimIKInstanceProperties,
) {
    if !skip_ik_task(ctx, IkTask::ReelIn) {
        REEL_IN_AIM_IK_UPDATE.get().unwrap().call(ctx, p1, props);
    }
}

#[detour(address = NGrapplingFireArmIK_Update_ADDRESS)]
fn grappling_fire_arm_ik_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    if !skip_ik_task(ctx, IkTask::GrappleFire) {
        GRAPPLING_FIRE_ARM_IK_UPDATE
            .get()
            .unwrap()
            .call(ctx, p1, p2);
    }
}

/// The character's weapon aim point setter, which the player's post-camera update calls right
/// after copying the same point into the control parameters. The copy is what turns the body
/// toward the aim point, so it is replaced by the neutral point (on foot only: mounted guns and
/// vehicles want the real point). The character-level point, which fans out to the weapons as
/// their fire target, becomes the dominant hand's aim point while the gun aim acts, and is
/// otherwise left as the game computed it. The original always runs first, so the point counts
/// as set this frame and the weapon update's body-forward fallback never kicks in.
#[detour(address = jc3gi::character::character::Character::UpdateWeaponAiming_ADDRESS)]
fn update_weapon_aiming(character: *mut Character, target: *const Vector3) {
    UPDATE_WEAPON_AIMING.get().unwrap().call(character, target);
    let Some(c) = (unsafe { local(character) }) else {
        return;
    };
    if let Some(point) = aim::character_aim_point() {
        c.m_AimTargetPositionWeapons = Vector3 {
            data: point.to_array(),
        };
    }
    let cfg = aim_state_config();
    let on_foot =
        matches!(mode::current().mode, Mode::OnFoot(_)) && !unsafe { c.IsInMountedGunState() };
    let control = unsafe { c.m_ControlParameters.as_mut() };
    let Some(control) = control.filter(|_| {
        on_foot
            && part_active(
                cfg.neutral_aim_point,
                cfg.without_controllers,
                controllers(),
            )
    }) else {
        STATUS.lock().neutral_point = None;
        return;
    };
    let world = glam::Mat4::from(c.m_WorldMatrixT1);
    let position = world.w_axis.truncate();
    // The game's own body-forward fallback uses the negated third axis of the world matrix.
    let forward = -world.z_axis.truncate();
    let point = neutral_aim_point(
        headpose::anchor().unwrap_or(position),
        forward,
        cfg.neutral_aim_distance_m.max(0.0),
    );
    control.m_AimTargetWeapons = Vector3 {
        data: point.to_array(),
    };
    STATUS.lock().neutral_point = Some(point);
}

/// The wall-penetration component's update, with its block cleared afterwards for a weapon the
/// local player holds. Nothing else writes the fields, so the clear lasts to the fire check.
#[detour(
    address = jc3gi::equipment::weapon::WallPenetrationWeaponComponent::UpdateWeapon_Serial_ADDRESS
)]
fn wall_penetration_update(
    component: *mut WallPenetrationWeaponComponent,
    dt: f32,
    weapon_data: *mut WeaponData,
    p3: f32,
) {
    WALL_PENETRATION_UPDATE
        .get()
        .unwrap()
        .call(component, dt, weapon_data, p3);
    let Some(weapon) = (unsafe { component.as_ref() }).and_then(|c| unsafe { c.m_Owner.as_mut() })
    else {
        return;
    };
    // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character.
    let user = weapon.m_User[0] as *mut Character;
    if unsafe { local(user) }.is_none() || !active(|cfg| cfg.clear_wall_block) {
        return;
    }
    let was_blocked = weapon.m_IsWeaponPenetratingWallIfInAim
        || weapon.m_IsWeaponPenetratingCharacterIfInAim
        || !weapon.m_IsLeanOverFirePossible
        || !weapon.m_AimingWeaponIsPossible;
    weapon.m_IsWeaponPenetratingWallIfInAim = false;
    weapon.m_IsWeaponPenetratingCharacterIfInAim = false;
    weapon.m_IsLeanOverFirePossible = true;
    weapon.m_AimingWeaponIsPossible = true;
    STATUS.lock().wall_blocks_cleared += u64::from(was_blocked);
}

/// The inventory's weapon handling. For the local player with the cuts active: the combat draw's
/// permission bit is set for the call (the permission update clears it every tick, so it is set
/// every tick), a lapsed deliberate holster is dropped, and the call is marked so that the
/// `QueueAct` filter can tell the relaxed branch's holster apart.
///
/// Around all of it, the fire and precision-aim inputs that would draw a weapon with nothing in the
/// hands are held back ([`weapons::unarmed_fire`]).
#[detour(address = jc3gi::character::inventory::Inventory::UpdateWeaponHandling_ADDRESS)]
fn update_weapon_handling(inventory: *mut Inventory, dt: f32) {
    let original = || UPDATE_WEAPON_HANDLING.get().unwrap().call(inventory, dt);
    let character = unsafe { inventory.as_ref() }.map_or(std::ptr::null_mut(), |i| i.m_Character);
    let Some(c) = (unsafe { local(character) }) else {
        return original();
    };
    let words = unsafe { StateWords::of(c) };
    let holstering = words.is_some_and(|w| unsafe { w.has(StateBitFlag::SB_IS_HOLSTERING_WEAPON) });
    let held = unsafe { weapons::unarmed_fire::hold_back(c, holstering) };
    update_local_weapon_handling(c, words, original);
    if let Some(held) = held {
        unsafe { held.restore(c) };
    }
}

/// The mental-state cuts around the local player's weapon handling, `original`.
fn update_local_weapon_handling(c: &mut Character, words: Option<StateWords>, original: impl Fn()) {
    let Some(words) = words.filter(|_| active(|cfg| cfg.mental_state_cuts)) else {
        STATUS.lock().mental.draw_blocked = false;
        return original();
    };
    unsafe { words.set(StateBitFlag::SB_DISABLE_MENTAL_STATE_DRAW_WEAPON, true) };

    let mental_state = unsafe { c.m_Avatar.as_ref() }
        .map(|avatar| unsafe { *(&raw const avatar.m_MentalState).cast::<i32>() });
    let relaxed = mental_state == Some(MentalState::E_RELAXED as i32);
    let drawing_or_holstering = unsafe {
        words.has(StateBitFlag::SB_IS_DRAWING_WEAPON)
            || words.has(StateBitFlag::SB_IS_HOLSTERING_WEAPON)
    };
    if !deliberate_holster_holds(relaxed, drawing_or_holstering) {
        DELIBERATE_HOLSTER.store(false, Ordering::Relaxed);
    }

    IN_LOCAL_WEAPON_HANDLING.with(|scope| scope.set(true));
    original();
    IN_LOCAL_WEAPON_HANDLING.with(|scope| scope.set(false));

    let mut status = STATUS.lock();
    status.mental.mental_state = mental_state;
    status.mental.draw_blocked = true;
    status.mental.deliberate_pending = DELIBERATE_HOLSTER.load(Ordering::Relaxed);
}

/// A holster request. For the player it only forces the relaxed state, and the weapon handling's
/// relaxed branch queues the act, so the request is remembered to let that act through.
#[detour(address = jc3gi::character::character::Character::HolsterWeapon_ADDRESS)]
fn holster_weapon(character: *mut Character) {
    if unsafe { local(character) }.is_some() {
        DELIBERATE_HOLSTER.store(true, Ordering::Relaxed);
    }
    HOLSTER_WEAPON.get().unwrap().call(character);
}

/// The runtime ids of the watched acts, resolved once the event-id symbol table is live.
fn watched_act(id: i32) -> Option<WatchedAct> {
    let ids = match WATCHED_ACT_IDS.get() {
        Some(ids) => *ids,
        None => {
            let ids = [
                resolve_act_id(EventIdSymbolTable::ACT_ON_UNEQUIP_WEAPON_SLOW)?,
                resolve_act_id(EventIdSymbolTable::UB_ACT_HOLSTER)?,
            ];
            *WATCHED_ACT_IDS.get_or_init(|| ids)
        }
    };
    match id {
        _ if id == ids[0] => Some(WatchedAct::UnequipSlow),
        _ if id == ids[1] => Some(WatchedAct::Holster),
        _ => None,
    }
}

static WATCHED_ACT_IDS: OnceLock<[i32; 2]> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_act_under_the_controller_scheme_or_when_forced() {
        assert!(part_active(true, false, true));
        assert!(!part_active(true, false, false));
        assert!(part_active(true, true, false));
        assert!(!part_active(false, true, true));
    }

    #[test]
    fn state_bits_split_into_words() {
        assert_eq!(
            StateBit::of(StateBitFlag::SB_NO_WEAPON_AIM),
            StateBit {
                word: 0,
                mask: 1 << 44
            }
        );
        assert_eq!(
            StateBit::of(StateBitFlag::SB_DISABLE_MENTAL_STATE_DRAW_WEAPON),
            StateBit {
                word: 1,
                mask: 1 << 21
            }
        );
        assert_eq!(
            StateBit::of(StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL),
            StateBit {
                word: 0,
                mask: 1 << 51
            }
        );
    }

    #[test]
    fn the_neutral_point_is_level_ahead_of_the_head() {
        let head = Vec3::new(1.0, 1.7, -2.0);
        let point = neutral_aim_point(head, Vec3::new(0.0, -0.3, -1.0).normalize(), 25.0);
        assert!((point.y - head.y).abs() < 1e-5);
        assert!(((point - head).length() - 25.0).abs() < 1e-4);
        assert!(point.z < head.z);
    }

    #[test]
    fn a_vertical_forward_is_used_as_it_is() {
        let point = neutral_aim_point(Vec3::ZERO, Vec3::Y, 10.0);
        assert_eq!(point, Vec3::new(0.0, 10.0, 0.0));
    }

    #[test]
    fn the_slow_unequip_is_always_swallowed() {
        for (inside, deliberate) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                act_verdict(WatchedAct::UnequipSlow, inside, deliberate),
                ActVerdict::Swallow
            );
        }
    }

    #[test]
    fn only_the_relaxed_branch_holster_is_swallowed() {
        assert_eq!(
            act_verdict(WatchedAct::Holster, false, false),
            ActVerdict::Pass
        );
        assert_eq!(
            act_verdict(WatchedAct::Holster, true, false),
            ActVerdict::Swallow
        );
        assert_eq!(
            act_verdict(WatchedAct::Holster, true, true),
            ActVerdict::PassDeliberate
        );
    }

    #[test]
    fn a_deliberate_holster_lapses_once_it_plays_or_combat_returns() {
        assert!(deliberate_holster_holds(true, false));
        assert!(!deliberate_holster_holds(true, true));
        assert!(!deliberate_holster_holds(false, false));
    }
}
