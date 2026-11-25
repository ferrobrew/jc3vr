#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A message raised by an animation track (`NAnimationSystem::SAnimationMessageData`). The
/// character's message poll, run in its serial post-physics pass, hands each one to the character
/// ([`Character::HandleTrackMessage`](crate::character::character::Character::HandleTrackMessage)), which
/// forwards the weapon messages to a weapon of its inventory, normally the wielded one
/// ([`WeaponBase::HandleTrackMessage`](crate::equipment::weapon::WeaponBase::HandleTrackMessage)).
pub struct AnimationMessageData {
    /// The message's id.
    pub m_MessageId: u32,
    /// How many of [`m_Parameters`](crate::animation::message::AnimationMessageData::m_Parameters) the message carries.
    pub m_ParameterCount: u8,
    _field_5: [u8; 3],
    /// The message's numeric parameters. Each entry is a union of a float, a signed integer, and a
    /// name hash; the receiver decides which it reads. The weapon messages carry the message's
    /// name hash in the second entry and their arguments in the entries after it.
    pub m_Parameters: [u32; 10],
    /// The message's two string parameters.
    pub m_Char: [[u8; 48]; 2],
}
fn _AnimationMessageData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x90], AnimationMessageData>([0u8; 0x90]);
    }
    unreachable!()
}
impl AnimationMessageData {}
impl std::convert::AsRef<AnimationMessageData> for AnimationMessageData {
    fn as_ref(&self) -> &AnimationMessageData {
        self
    }
}
impl std::convert::AsMut<AnimationMessageData> for AnimationMessageData {
    fn as_mut(&mut self) -> &mut AnimationMessageData {
        self
    }
}
