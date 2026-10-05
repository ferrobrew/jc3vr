#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a rumble's strength depends on where it happens (`CRumbleEffect::EType`).
pub enum RumbleEffectType {
    EType_Spatial = 0isize as _,
    EType_Absolute = 1isize as _,
    EType_Disabled = 2isize as _,
}
fn _RumbleEffectType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RumbleEffectType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The controller rumble manager (`CRumbleManager`), owned by the effect system. It tracks two
/// kinds of rumble: a pool of timed one-off rumbles started by [`SpawnRumble`](crate::rumble_manager::RumbleManager::SpawnRumble),
/// and any number of effects a system activates and keeps alive itself for as long as it wants a
/// continuous rumble, such as a vehicle's engine and road noise. [`Update`](crate::rumble_manager::RumbleManager::Update) ages the
/// one-offs, sums every active effect of both kinds into the gamepad's two body motors and two
/// trigger motors, and pushes the totals to the gamepad once a frame. Its layout is unmapped.
pub struct RumbleManager {}
impl RumbleManager {
    pub const SpawnRumble_ADDRESS: usize = 0x14048EAD0;
    /// Starts a one-off rumble lasting `time` seconds with the given strength for each motor,
    /// unless `time` or the motors' sum is not positive. `position` and `max_distance` place an
    /// [`EType_Spatial`](crate::rumble_manager::RumbleEffectType::EType_Spatial) rumble; `debug_name` names the source.
    /// Every one-off rumble goes through it: the weapon fire rumble
    /// ([`WeaponBase::RumbleFire`](crate::equipment::weapon::WeaponBase::RumbleFire)), the player's damage
    /// rumbles (`CPlayer::OnDamage`: impacts, bullets, and drowning), the character's vehicle
    /// collisions, rumbles on animation track messages and melee hits, the reel-in kick, the
    /// underwater swim, the force pulse, and event-driven rumbles. It may be called from any
    /// thread; it locks the manager.
    pub unsafe fn SpawnRumble(
        &mut self,
        effect_type: crate::rumble_manager::RumbleEffectType,
        position: *const crate::types::math::Vector3,
        left_motor_value: f32,
        right_motor_value: f32,
        max_distance: f32,
        time: f32,
        debug_name: *const u8,
        left_trigger_motor_value: f32,
        right_trigger_motor_value: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                effect_type: crate::rumble_manager::RumbleEffectType,
                position: *const crate::types::math::Vector3,
                left_motor_value: f32,
                right_motor_value: f32,
                max_distance: f32,
                time: f32,
                debug_name: *const u8,
                left_trigger_motor_value: f32,
                right_trigger_motor_value: f32,
            ) = ::std::mem::transmute(Self::SpawnRumble_ADDRESS);
            f(
                self as *mut Self as _,
                effect_type,
                position,
                left_motor_value,
                right_motor_value,
                max_distance,
                time,
                debug_name,
                left_trigger_motor_value,
                right_trigger_motor_value,
            )
        }
    }
    pub const Update_ADDRESS: usize = 0x14048E740;
    /// The per-frame tick: ages every active one-off rumble by `dt`, retiring and freeing those
    /// past their duration, then, unless the game has rumble disabled, the clock is paused, the
    /// window is out of focus, or the debug menu is active, sums every active rumble (one-off and
    /// continuous alike) into a left, right, left-trigger, and right-trigger total, attenuating a
    /// spatial rumble by its distance from the manager's last-set listener position. It clamps
    /// the left total to `0..=1` and sends it to the gamepad's motor 0, then clamps the sum of
    /// the right, left-trigger, and right-trigger totals to `0..=1` and sends that to
    /// motor 1, through [`Gamepad::SetMotorValue`](crate::input::gamepad::Gamepad::SetMotorValue). It
    /// still calls both, with zero, when nothing is rumbling or rumble is suppressed, as long as a
    /// gamepad is present; it is a no-op without one.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<RumbleManager> for RumbleManager {
    fn as_ref(&self) -> &RumbleManager {
        self
    }
}
impl std::convert::AsMut<RumbleManager> for RumbleManager {
    fn as_mut(&mut self) -> &mut RumbleManager {
        self
    }
}
