#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A game setting (`CSettingsManager::ESettingType`), the index into
/// [`SettingsManager::m_Settings`](crate::settings_manager::SettingsManager::m_Settings).
pub enum SettingType {
    EFirstSettingType = 0isize as _,
    EGraphicsMotionBlur = 1isize as _,
    EGraphicsAniso = 2isize as _,
    EGraphicsLodFactor = 3isize as _,
    EGraphicsShadowsQuality = 4isize as _,
    EGraphicsPostEffects = 5isize as _,
    EGraphicsTextureQuality = 6isize as _,
    EGraphicsWaterDetail = 7isize as _,
    EGraphicsShadowsResolution = 8isize as _,
    EGraphicsSSAO = 9isize as _,
    EGraphicsEdgeFade = 10isize as _,
    EGraphicsGI = 11isize as _,
    EGraphicsSSReflection = 12isize as _,
    EGraphicsBokehDOF = 13isize as _,
    EDisplayMode = 14isize as _,
    EDisplayFullscreen = 15isize as _,
    EDisplayFullscreenWidth = 16isize as _,
    EDisplayFullscreenHeight = 17isize as _,
    EDisplayWindowedWidth = 18isize as _,
    EDisplayWindowedHeight = 19isize as _,
    EDisplayRefreshRate = 20isize as _,
    EDisplayVSync = 21isize as _,
    EDisplayGamma = 22isize as _,
    EDisplayHudState = 23isize as _,
    EControllerVibration = 24isize as _,
    EControllerInvertAimingY = 25isize as _,
    EControllerInvertAimingX = 26isize as _,
    EControllerFlightYAxis = 27isize as _,
    EControllerFireButton = 28isize as _,
    EControllerSensitivityPitch = 29isize as _,
    EControllerSensitivityYaw = 30isize as _,
    EControllerCrouchToggle = 31isize as _,
    EControllerDisableInputWhenOutOfFocus = 32isize as _,
    EPCUseGamepad = 33isize as _,
    EPCGamepadExclusiveLockControllerMode = 34isize as _,
    EMouseInvertAimingX = 35isize as _,
    EMouseInvertAimingY = 36isize as _,
    EMouseSensitivityX = 37isize as _,
    EMouseSensitivityY = 38isize as _,
    EDifficulty = 39isize as _,
    ECameraCentering = 40isize as _,
    ELoadAndSaveEnabled = 41isize as _,
    EDLCEnabled = 42isize as _,
    EProfileSignInEnabled = 43isize as _,
    ESoundGameplayVolume = 44isize as _,
    ESoundMusicVolume = 45isize as _,
    ESoundVoiceVolume = 46isize as _,
    ESubtitleEnabled = 47isize as _,
    EPS4ControllerSpeakerEnabled = 48isize as _,
    EGraphicsAA = 49isize as _,
    ESkinParachute = 50isize as _,
    ESkinWingsuit = 51isize as _,
    ESkinRico = 52isize as _,
    ENoir = 53isize as _,
    EDoge = 54isize as _,
    EExtendedFriends = 55isize as _,
    EForceArabic = 56isize as _,
    EForceEnglishVO = 57isize as _,
    EGraphicsWaterTesselation = 58isize as _,
    EGraphicsAOITQuality = 59isize as _,
    ENumSettingTypes = 60isize as _,
}
fn _SettingType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SettingType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The settings manager (`CSettingsManager`, a `Base::CSingle` singleton): the game's options, one
/// integer per [`SettingType`](crate::settings_manager::SettingType), with their persistence and change handling. Only some members are
/// bound.
pub struct SettingsManager {
    _field_0: [u8; 132],
    /// The value of each setting, indexed by [`SettingType`](crate::settings_manager::SettingType). `CSettingsManager::SetValue` writes
    /// an entry and raises an update flag that applies the change (and may persist it); a direct
    /// write to an entry applies nothing else.
    pub m_Settings: [i32; 60],
    _field_174: [u8; 4],
}
fn _SettingsManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x178], SettingsManager>([0u8; 0x178]);
    }
    unreachable!()
}
impl SettingsManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418220496usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl SettingsManager {
    pub const GetValue_ADDRESS: usize = 0x140D3B3C0;
    /// Returns the value of `setting`: the entry in [`m_Settings`](crate::settings_manager::SettingsManager::m_Settings), except for the
    /// few settings whose values live elsewhere (see [`SettingType`](crate::settings_manager::SettingType)).
    pub unsafe fn GetValue(&self, setting: crate::settings_manager::SettingType) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                setting: crate::settings_manager::SettingType,
            ) -> i32 = ::std::mem::transmute(Self::GetValue_ADDRESS);
            f(self as *const Self as _, setting)
        }
    }
}
impl std::convert::AsRef<SettingsManager> for SettingsManager {
    fn as_ref(&self) -> &SettingsManager {
        self
    }
}
impl std::convert::AsMut<SettingsManager> for SettingsManager {
    fn as_mut(&mut self) -> &mut SettingsManager {
        self
    }
}
