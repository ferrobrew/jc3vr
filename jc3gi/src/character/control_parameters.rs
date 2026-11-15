#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The character's animation control parameters (`CControlParameters`), reached through
/// [`Character::m_ControlParameters`](crate::character::character::Character::m_ControlParameters). Only some
/// members are bound.
pub struct ControlParameters {
    _field_0: [u8; 176],
    /// The weapon aim point (world space), copied here from the player's aim control each frame by
    /// `CPlayer::UpdatePostCamera` alongside
    /// [`Character::UpdateWeaponAiming`](crate::character::character::Character::UpdateWeaponAiming).
    pub m_AimTargetWeapons: crate::types::math::Vector3,
    _field_bc: [u8; 28],
    /// The animation user VM state, holding the character state-bitflag words.
    pub m_UserVM: crate::character::control_parameters::UserVM,
    _field_108: [u8; 380],
    /// The ragdolling timers the animation graph's get-up conditions read.
    pub m_RagdollingControllerParameters: crate::character::control_parameters::RagdollingControllerParams,
}
fn _ControlParameters_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x290], ControlParameters>([0u8; 0x290]);
    }
    unreachable!()
}
impl ControlParameters {}
impl std::convert::AsRef<ControlParameters> for ControlParameters {
    fn as_ref(&self) -> &ControlParameters {
        self
    }
}
impl std::convert::AsMut<ControlParameters> for ControlParameters {
    fn as_mut(&mut self) -> &mut ControlParameters {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// The ragdolling timers (`SRagdollingControllerParams`) in
/// [`ControlParameters::m_RagdollingControllerParameters`](crate::character::control_parameters::ControlParameters::m_RagdollingControllerParameters). The ragdolling state task
/// (`NRagdolling`) runs the total and the limb speed; the graph's `CRagdollingTimerCondition` and
/// `CRagdollIsSettledCondition` read them for the get-up.
pub struct RagdollingControllerParams {
    /// The time spent ragdolling, seconds. Below zero while the ragdoll's displacement is its
    /// owner's; `CRagdollingTimerCondition` holds nothing then, and otherwise compares it with its
    /// threshold.
    pub m_TotalRagdollingTime: f32,
    /// How long the ragdoll has counted as settled, seconds. `CRagdollIsSettledCondition` adds each
    /// update's time while
    /// [`RagdollingControllerParams::m_AverageRagdollLimbSpeed`](crate::character::control_parameters::RagdollingControllerParams::m_AverageRagdollLimbSpeed) is under its speed threshold and
    /// the character is near the ground and pose-matched, zeroes it otherwise, and holds once it
    /// passes its time threshold.
    pub m_SettledRagdollingTime: f32,
    /// The ragdoll's average speed of its bodies relative to each other, metres per second.
    pub m_AverageRagdollLimbSpeed: f32,
}
fn _RagdollingControllerParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], RagdollingControllerParams>([0u8; 0xC]);
    }
    unreachable!()
}
impl RagdollingControllerParams {}
impl std::convert::AsRef<RagdollingControllerParams> for RagdollingControllerParams {
    fn as_ref(&self) -> &RagdollingControllerParams {
        self
    }
}
impl std::convert::AsMut<RagdollingControllerParams> for RagdollingControllerParams {
    fn as_mut(&mut self) -> &mut RagdollingControllerParams {
        self
    }
}
#[repr(C, align(8))]
/// The animation state machine's per-instance data (`SUserVM`, derived from
/// `SAnimatedModelInstanceData`): the register file and the state-bitflag words. Animation-graph
/// evaluation on the SIM tick sets and clears the bits as the state machine enters and leaves
/// states; the one-bit predicates on [`Character`](crate::character::character::Character) read them. The
/// words are game-thread data.
///
/// The bits are named by [`StateBitFlag`](crate::state::StateBitFlag), an index across the words: bit `n`
/// lives in word `n / 64` at position `n % 64`, so its mask within that word is `1 << (n % 64)`.
pub struct UserVM {
    /// The animation register array.
    pub m_Registers: *mut f32,
    pub m_RegisterCount: u32,
    _field_c: [u8; 4],
    /// The current state bits `0..64` ([`StateBitFlag`](crate::state::StateBitFlag) indices `0x00..0x40`).
    pub m_CurrentStateBitFlags0: u64,
    /// The current state bits `64..128` ([`StateBitFlag`](crate::state::StateBitFlag) indices `0x40..0x80`).
    pub m_CurrentStateBitFlags1: u64,
    /// The current state bits `128..192` ([`StateBitFlag`](crate::state::StateBitFlag) indices `0x80..0xC0`).
    pub m_CurrentStateBitFlags2: u64,
    /// The fourth state word; no state bit is assigned to it.
    pub m_CurrentStateBitFlags3: u64,
}
fn _UserVM_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], UserVM>([0u8; 0x30]);
    }
    unreachable!()
}
impl UserVM {}
impl std::convert::AsRef<UserVM> for UserVM {
    fn as_ref(&self) -> &UserVM {
        self
    }
}
impl std::convert::AsMut<UserVM> for UserVM {
    fn as_mut(&mut self) -> &mut UserVM {
        self
    }
}
