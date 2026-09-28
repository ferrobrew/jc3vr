#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The player's aim mode (`CPlayer::EAimModes`), held in [`Player::m_AimMode`](crate::character::player::Player::m_AimMode).
pub enum AimMode {
    AIM_NORMAL = 0isize as _,
    AIM_PRECISION = 1isize as _,
    AIM_MAX = 2isize as _,
}
fn _AimMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AimMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The local player object (`CPlayer`). It derives from `NEvent::CEventHandler` and then
/// `CAvatar`, so the [`Avatar`](crate::character::avatar::Avatar) base sits at [`AVATAR_BASE_OFFSET`](crate::character::player::Player::AVATAR_BASE_OFFSET) and a
/// character's [`m_Avatar`](crate::character::character::Character::m_Avatar) points there rather than at
/// the start of the player. Only some members are bound.
pub struct Player {
    _field_0: [u8; 272],
    /// The player's character.
    pub m_Character: *mut crate::character::character::Character,
    _field_118: [u8; 64],
    /// The player's aim control, which resolves the aim points each frame. Its weapon aim point
    /// (the [`TARGET_WEAPONS`](crate::aim::target::TargetType::TARGET_WEAPONS) slot of
    /// [`m_AimPos`](crate::aim::aim::PlayerAimControl::m_AimPos)) is the target the arm aim IK tasks steer
    /// toward.
    pub m_AimControl: *mut crate::aim::aim::PlayerAimControl,
    _field_160: [u8; 4780],
    /// The player's aim mode: [`AIM_PRECISION`](crate::character::player::AimMode::AIM_PRECISION) while in precision
    /// (down-sights) aim, [`AIM_NORMAL`](crate::character::player::AimMode::AIM_NORMAL) otherwise. Written by
    /// [`SetAimMode`](crate::character::player::Player::SetAimMode).
    pub m_AimMode: crate::character::player::AimMode,
    /// The game time ([`Clock`](crate::clock::Clock) game-time milliseconds) at which
    /// [`m_AimMode`](crate::character::player::Player::m_AimMode) last changed.
    pub m_AimModeChangeTime: u32,
    _field_1414: [u8; 4],
}
fn _Player_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1418], Player>([0u8; 0x1418]);
    }
    unreachable!()
}
impl Player {
    pub const SetAimMode_ADDRESS: usize = 0x140C3CEB0;
    /// Switches the player's aim mode. Entering precision aim
    /// ([`AIM_PRECISION`](crate::character::player::AimMode::AIM_PRECISION)) is refused while
    /// falling without a parachute or when the character has none of the aimable weapon slots
    /// filled. On a change it records [`m_AimModeChangeTime`](crate::character::player::Player::m_AimModeChangeTime), adds or
    /// removes the precision-aim camera hint (adding it only when `add_camera_hint` is set), sets
    /// the aim control's aim state, and forwards the mode to the selected weapon (or draws it).
    pub unsafe fn SetAimMode(
        &mut self,
        mode: crate::character::player::AimMode,
        add_camera_hint: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                mode: crate::character::player::AimMode,
                add_camera_hint: bool,
            ) = ::std::mem::transmute(Self::SetAimMode_ADDRESS);
            f(self as *mut Self as _, mode, add_camera_hint)
        }
    }
    pub const UpdateAimMode_ADDRESS: usize = 0x140C3D100;
    /// Drops the player out of precision aim ([`SetAimMode`](crate::character::player::Player::SetAimMode) with
    /// [`AIM_NORMAL`](crate::character::player::AimMode::AIM_NORMAL)) when the
    /// character can no longer precision-aim and is not planting an explosive. Called from
    /// `CPlayer::UpdatePostPhysics`.
    pub unsafe fn UpdateAimMode(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateAimMode_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl Player {
    /// The offset of the [`Avatar`](crate::character::avatar::Avatar) base within the player.
    pub const AVATAR_BASE_OFFSET: u64 = 16;
}
impl std::convert::AsRef<Player> for Player {
    fn as_ref(&self) -> &Player {
        self
    }
}
impl std::convert::AsMut<Player> for Player {
    fn as_mut(&mut self) -> &mut Player {
        self
    }
}
pub const Player_UpdatePostCamera_ADDRESS: usize = 0x140CF7BB0;
/// `CPlayer::UpdatePostCamera`, the player's override of the `CAvatar::UpdatePostCamera` virtual;
/// `avatar` is the player's [`Avatar`](crate::character::avatar::Avatar) base. Runs the aim control's post-simulation update, then
/// publishes its weapon aim point to the character (the control parameters' aim target and
/// [`Character::UpdateWeaponAiming`](crate::character::character::Character::UpdateWeaponAiming)) and its grenade aim point
/// ([`Character::UpdateGrenadeAiming`](crate::character::character::Character::UpdateGrenadeAiming)), and, when the player sits in a vehicle's driver seat, to
/// the vehicle ([`Vehicle::SetAimTargetPosition`](crate::vehicle::vehicle::Vehicle::SetAimTargetPosition)).
pub unsafe fn Player_UpdatePostCamera(
    avatar: *mut crate::character::avatar::Avatar,
    dt: f32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            avatar: *mut crate::character::avatar::Avatar,
            dt: f32,
        ) = ::std::mem::transmute(Player_UpdatePostCamera_ADDRESS);
        f(avatar, dt)
    }
}
pub const Player_UpdateMentalState_ADDRESS: usize = 0x140C12BE0;
/// `CPlayer::UpdateMentalState`, the player's override of the `CAvatar` virtual; `avatar` is the
/// player's [`Avatar`](crate::character::avatar::Avatar) base. After the base update it sets the character's mental state:
/// relaxed while [`Character_ForceNonCombat`](crate::character::character::get_Character_ForceNonCombat) is
/// set, combat while [`Character_ForceCombat`](crate::character::character::get_Character_ForceCombat) is
/// set, relaxed while the character is holstering (or a further character state check holds), and
/// combat while it has weapon input, throws a grenade, wields a heavy weapon, or aims a weapon
/// outside explosive planting. Otherwise combat decays to relaxed once the combat hold expires,
/// and on that transition, with a weapon wielded, it queues `ACT_ON_UNEQUIP_WEAPON_SLOW` through
/// [`Character::QueueAct`](crate::character::character::Character::QueueAct), an automatic holster.
pub unsafe fn Player_UpdateMentalState(
    avatar: *mut crate::character::avatar::Avatar,
    dt: f32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            avatar: *mut crate::character::avatar::Avatar,
            dt: f32,
        ) = ::std::mem::transmute(Player_UpdateMentalState_ADDRESS);
        f(avatar, dt)
    }
}
