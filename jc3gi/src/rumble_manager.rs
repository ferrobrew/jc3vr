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
/// The controller rumble manager (`CRumbleManager`), owned by the effect system. It sums the
/// active rumbles into the gamepad's two body motors and two trigger motors. Its layout is
/// unmapped.
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
