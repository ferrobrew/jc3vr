#![cfg_attr(any(), rustfmt::skip)]
/// `NGameSoundLibrary::g_WindowFocused`: whether the game window holds the foreground, as the
/// sound library sees it. While it is clear the sound library's post-sim update silences the master
/// bus, as it does for the pause reasons that mute all audio.
pub unsafe fn get_WindowFocused() -> &'static mut bool {
    unsafe { &mut *(0x142D93109 as *mut bool) }
}
pub const SetListenerTransform_ADDRESS: usize = 0x140D2AFF0;
/// Sets the 3D audio listener pose and velocity. Free function.
unsafe fn SetListenerTransform(
    mat: *const crate::types::math::Matrix4,
    velocity: *const crate::types::math::Vector3,
    enable: bool,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            mat: *const crate::types::math::Matrix4,
            velocity: *const crate::types::math::Vector3,
            enable: bool,
        ) = ::std::mem::transmute(SetListenerTransform_ADDRESS);
        f(mat, velocity, enable)
    }
}
pub const SetWindowFocused_ADDRESS: usize = 0x140DE6880;
/// `NGameSoundLibrary::SetWindowFocused`: records whether the game window holds the foreground in
/// [`WindowFocused`](get_WindowFocused) and, when the value changes, runs one sound-library fixed update so the
/// change takes effect at once. Called by [`LoseFocus`](crate::window::LoseFocus) and
/// [`GainFocus`](crate::window::GainFocus).
pub unsafe fn SetWindowFocused(focused: bool) {
    unsafe {
        let f: unsafe extern "system" fn(focused: bool) = ::std::mem::transmute(
            SetWindowFocused_ADDRESS,
        );
        f(focused)
    }
}
